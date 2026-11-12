use anyhow::{Context, Result, ensure};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use rand::{RngCore, rngs::OsRng};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

pub const CHUNK: usize = 64 * 1024;
pub type FsResult<T> = std::result::Result<T, i32>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub ino: u64,
    pub parent: u64,
    pub name: String,
    /// 1 directory, 2 file, 3 symlink.
    pub kind: u8,
    pub mode: u16,
    pub size: u64,
    pub modified: u64,
    #[serde(default)]
    pub nanos: u32,
    pub chunks: BTreeMap<u64, i64>,
    #[serde(default)]
    pub target: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Tree {
    next: u64,
    nodes: BTreeMap<u64, Node>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Crypto {
    salt: Vec<u8>,
    nonce: Vec<u8>,
    wrapped: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: i64,
    pub label: String,
    pub created: u64,
    pub safe: bool,
}

pub struct DiskStore {
    db: Connection,
    tree: Tree,
    key: Option<Zeroizing<[u8; 32]>>,
    level: u32,
    retention: usize,
    pub emergency: bool,
    pub readonly: bool,
    pub recovery_reason: Option<String>,
    restored: bool,
}

fn nanos() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn derive(password: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(64 * 1024, 3, 1, Some(32)).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut key = Zeroizing::new([0; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(key)
}
fn crypt(key: &[u8], bytes: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut nonce);
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| anyhow::anyhow!("invalid key"))?;
    let payload = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: bytes, aad })
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    let mut result = nonce.to_vec();
    result.extend(payload);
    Ok(result)
}
fn decrypt(key: &[u8], bytes: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    ensure!(bytes.len() >= 40, "truncated encrypted disk record");
    let cipher =
        XChaCha20Poly1305::new_from_slice(key).map_err(|_| anyhow::anyhow!("invalid key"))?;
    Ok(Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&bytes[..24]),
                Payload {
                    msg: &bytes[24..],
                    aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("incorrect key or damaged authenticated disk record"))?,
    ))
}

impl DiskStore {
    pub fn open(
        path: &Path,
        level: u32,
        retention: usize,
        force_emergency: bool,
        mut password: impl FnMut() -> Result<Zeroizing<String>>,
    ) -> Result<Self> {
        let db = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON;
            CREATE TABLE IF NOT EXISTS settings (name TEXT PRIMARY KEY, value BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS objects (id INTEGER PRIMARY KEY AUTOINCREMENT, payload BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS snapshots (id INTEGER PRIMARY KEY AUTOINCREMENT, label TEXT NOT NULL, created INTEGER NOT NULL, safe INTEGER NOT NULL, tree BLOB NOT NULL);")?;
        let format: Option<Vec<u8>> = db
            .query_row("SELECT value FROM settings WHERE name='format'", [], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(format) = format {
            ensure!(
                format == b"glyph-mounted-1",
                "unsupported Glyph disk format"
            );
        } else {
            db.execute(
                "INSERT INTO settings VALUES ('format', ?1)",
                [b"glyph-mounted-1".as_slice()],
            )?;
        }
        let crypto: Option<Vec<u8>> = db
            .query_row("SELECT value FROM settings WHERE name='crypto'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let key = if let Some(bytes) = crypto {
            let crypto: Crypto = serde_json::from_slice(&bytes)?;
            ensure!(
                crypto.salt.len() == 16 && crypto.nonce.len() == 24,
                "invalid disk key header"
            );
            let wrapping = derive(&password()?, &crypto.salt)?;
            let mut record = crypto.nonce;
            record.extend(crypto.wrapped);
            let master = decrypt(wrapping.as_ref(), &record, b"glyph-mounted-1/key")?;
            ensure!(master.len() == 32, "invalid disk key");
            let mut key = Zeroizing::new([0; 32]);
            key.copy_from_slice(&master);
            Some(key)
        } else {
            None
        };
        let tree = Tree {
            next: 2,
            nodes: BTreeMap::from([(
                1,
                Node {
                    ino: 1,
                    parent: 1,
                    name: String::new(),
                    kind: 1,
                    mode: 0o700,
                    size: 0,
                    modified: now(),
                    nanos: nanos(),
                    chunks: BTreeMap::new(),
                    target: String::new(),
                },
            )]),
        };
        let mut store = Self {
            db,
            tree,
            key,
            level,
            retention: retention.clamp(2, 20),
            emergency: false,
            readonly: false,
            recovery_reason: None,
            restored: false,
        };
        let dirty = store.setting("dirty")?.is_some_and(|v| v == b"1");
        let working = store.setting("working")?;
        let safe = store
            .setting("safe")?
            .and_then(|v| String::from_utf8(v).ok()?.parse::<i64>().ok());
        if let Some(working) = working {
            match store.decode_tree(&working) {
                Ok(tree) => store.tree = tree,
                Err(error) if safe.is_some() => {
                    store.recovery_reason = Some(format!(
                        "working metadata failed authentication/validation: {error}"
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        if (dirty || force_emergency || store.recovery_reason.is_some()) && safe.is_some() {
            if dirty && store.recovery_reason.is_none() {
                store.checkpoint("unclean session (preserved)", false)?;
            }
            let safe = safe.unwrap();
            let bytes: Vec<u8> =
                store
                    .db
                    .query_row("SELECT tree FROM snapshots WHERE id=?1", [safe], |row| {
                        row.get(0)
                    })?;
            store.tree = store
                .decode_tree(&bytes)
                .context("last safe snapshot is damaged")?;
            store.emergency = true;
            store.readonly = true;
            store.recovery_reason.get_or_insert_with(|| {
                if dirty {
                    "previous session did not shut down cleanly".into()
                } else {
                    "emergency mode requested".into()
                }
            });
        } else {
            store
                .tree
                .nodes
                .retain(|ino, node| *ino == 1 || node.parent != 0);
            store.save_working()?;
        }
        store.set_setting("dirty", b"1")?;
        Ok(store)
    }

    pub fn encrypted(&self) -> bool {
        self.key.is_some()
    }
    fn setting(&self, name: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .db
            .query_row("SELECT value FROM settings WHERE name=?1", [name], |r| {
                r.get(0)
            })
            .optional()?)
    }
    fn set_setting(&self, name: &str, value: &[u8]) -> Result<()> {
        self.db.execute(
            "INSERT OR REPLACE INTO settings VALUES (?1,?2)",
            params![name, value],
        )?;
        Ok(())
    }
    fn encode(&self, bytes: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        self.encode_level(bytes, aad, self.level)
    }
    fn encode_level(&self, bytes: &[u8], aad: &[u8], level: u32) -> Result<Vec<u8>> {
        let mut gzip = GzEncoder::new(Vec::new(), Compression::new(level));
        gzip.write_all(bytes)?;
        let compressed = Zeroizing::new(gzip.finish()?);
        let mut out = vec![u8::from(self.encrypted())];
        if let Some(key) = &self.key {
            out.extend(crypt(key.as_ref(), &compressed, aad)?);
        } else {
            out.extend_from_slice(&compressed);
        }
        Ok(out)
    }
    fn decode(&self, bytes: &[u8], aad: &[u8], limit: u64) -> Result<Zeroizing<Vec<u8>>> {
        ensure!(!bytes.is_empty(), "empty disk record");
        let compressed = match bytes[0] {
            0 => {
                ensure!(
                    !self.encrypted(),
                    "unexpected plaintext record in encrypted disk"
                );
                Zeroizing::new(bytes[1..].to_vec())
            }
            1 => decrypt(
                self.key.as_ref().context("disk is locked")?.as_ref(),
                &bytes[1..],
                aad,
            )?,
            _ => anyhow::bail!("unsupported disk record encoding"),
        };
        let mut out = Zeroizing::new(Vec::new());
        GzDecoder::new(&compressed[..])
            .take(limit + 1)
            .read_to_end(&mut out)?;
        ensure!(out.len() as u64 <= limit, "oversized disk record");
        Ok(out)
    }
    fn decode_tree(&self, bytes: &[u8]) -> Result<Tree> {
        let tree: Tree = serde_json::from_slice(&self.decode(
            bytes,
            b"glyph-mounted-1/tree",
            64 * 1024 * 1024,
        )?)?;
        ensure!(
            tree.nodes.get(&1).is_some_and(|n| n.kind == 1) && tree.nodes.len() <= 100_000,
            "invalid filesystem metadata"
        );
        for (ino, node) in &tree.nodes {
            ensure!(
                *ino == node.ino && [1, 2, 3].contains(&node.kind),
                "invalid inode"
            );
            if *ino != 1 && node.parent != 0 {
                ensure!(
                    tree.nodes.get(&node.parent).is_some_and(|n| n.kind == 1),
                    "invalid parent inode"
                );
                ensure!(
                    !node.name.is_empty()
                        && !node.name.contains('/')
                        && node.name != "."
                        && node.name != "..",
                    "invalid directory name"
                );
            }
        }
        Ok(tree)
    }
    fn save_working(&self) -> Result<()> {
        self.set_setting(
            "working",
            // Working metadata changes on every filesystem operation. Keep it
            // compressed with the fast preset; file chunks and snapshots use
            // the user's selected compression level.
            &self.encode_level(&serde_json::to_vec(&self.tree)?, b"glyph-mounted-1/tree", 1)?,
        )
    }
    fn mutate<T>(&mut self, action: impl FnOnce(&mut Self) -> FsResult<T>) -> FsResult<T> {
        if self.readonly {
            return Err(libc::EROFS);
        }
        let original = self.tree.clone();
        self.db
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| libc::EIO)?;
        let result = action(self).and_then(|value| {
            self.save_working().map_err(|_| libc::EIO)?;
            Ok(value)
        });
        match result {
            Ok(value) => {
                if self.db.execute_batch("COMMIT").is_err() {
                    let _ = self.db.execute_batch("ROLLBACK");
                    self.tree = original;
                    return Err(libc::EIO);
                }
                Ok(value)
            }
            Err(error) => {
                let _ = self.db.execute_batch("ROLLBACK");
                self.tree = original;
                Err(error)
            }
        }
    }
    pub fn node(&self, ino: u64) -> FsResult<Node> {
        self.tree.nodes.get(&ino).cloned().ok_or(libc::ENOENT)
    }
    pub fn lookup(&self, parent: u64, name: &str) -> FsResult<Node> {
        if self.node(parent)?.kind != 1 {
            return Err(libc::ENOTDIR);
        }
        self.tree
            .nodes
            .values()
            .find(|n| n.parent == parent && n.name == name && n.ino != 1)
            .cloned()
            .ok_or(libc::ENOENT)
    }
    pub fn children(&self, parent: u64) -> Vec<Node> {
        self.tree
            .nodes
            .values()
            .filter(|n| n.ino != 1 && n.parent == parent)
            .cloned()
            .collect()
    }
    pub fn create(
        &mut self,
        parent: u64,
        name: &str,
        kind: u8,
        mode: u16,
        target: &str,
    ) -> FsResult<Node> {
        self.mutate(|store| {
            if store.node(parent)?.kind != 1 {
                return Err(libc::ENOTDIR);
            }
            if name.is_empty()
                || name.contains('/')
                || name.contains('\0')
                || name == "."
                || name == ".."
            {
                return Err(libc::EINVAL);
            }
            if store.lookup(parent, name).is_ok() {
                return Err(libc::EEXIST);
            }
            if store.tree.nodes.len() >= 100_000 {
                return Err(libc::ENOSPC);
            }
            let ino = store.tree.next;
            store.tree.next += 1;
            let node = Node {
                ino,
                parent,
                name: name.into(),
                kind,
                mode: mode & 0o777,
                size: if kind == 3 { target.len() as u64 } else { 0 },
                modified: now(),
                nanos: nanos(),
                chunks: BTreeMap::new(),
                target: target.into(),
            };
            store.tree.nodes.insert(ino, node.clone());
            Ok(node)
        })
    }
    fn chunk(&self, node: &Node, index: u64) -> Result<Zeroizing<Vec<u8>>> {
        if let Some(id) = node.chunks.get(&index) {
            let blob: Vec<u8> =
                self.db
                    .query_row("SELECT payload FROM objects WHERE id=?1", [id], |r| {
                        r.get(0)
                    })?;
            self.decode(
                &blob,
                format!("glyph-mounted-1/object/{id}").as_bytes(),
                CHUNK as u64,
            )
        } else {
            Ok(Zeroizing::new(Vec::new()))
        }
    }
    fn store_chunk(&self, bytes: &[u8]) -> Result<i64> {
        self.db
            .execute("INSERT INTO objects(payload) VALUES (zeroblob(0))", [])?;
        let id = self.db.last_insert_rowid();
        let payload = self.encode(bytes, format!("glyph-mounted-1/object/{id}").as_bytes())?;
        self.db.execute(
            "UPDATE objects SET payload=?1 WHERE id=?2",
            params![payload, id],
        )?;
        Ok(id)
    }
    pub fn read(&self, ino: u64, offset: u64, length: usize) -> FsResult<Vec<u8>> {
        let node = self.node(ino)?;
        if node.kind != 2 {
            return Err(libc::EISDIR);
        }
        let length = length.min(node.size.saturating_sub(offset) as usize);
        let mut out = vec![0; length];
        let mut done = 0;
        while done < length {
            let position = offset + done as u64;
            let index = position / CHUNK as u64;
            let within = position as usize % CHUNK;
            let count = (CHUNK - within).min(length - done);
            let bytes = self.chunk(&node, index).map_err(|_| libc::EIO)?;
            if within < bytes.len() {
                let available = count.min(bytes.len() - within);
                out[done..done + available].copy_from_slice(&bytes[within..within + available]);
            }
            done += count;
        }
        Ok(out)
    }
    pub fn write(&mut self, ino: u64, offset: u64, bytes: &[u8]) -> FsResult<usize> {
        self.mutate(|store| {
            let mut node = store.node(ino)?;
            if node.kind != 2 {
                return Err(libc::EISDIR);
            }
            if offset
                .checked_add(bytes.len() as u64)
                .is_none_or(|n| n > 16 * 1024 * 1024 * 1024)
            {
                return Err(libc::EFBIG);
            }
            let mut done = 0;
            while done < bytes.len() {
                let position = offset + done as u64;
                let index = position / CHUNK as u64;
                let within = position as usize % CHUNK;
                let count = (CHUNK - within).min(bytes.len() - done);
                let mut chunk = store.chunk(&node, index).map_err(|_| libc::EIO)?;
                chunk.resize(CHUNK, 0);
                chunk[within..within + count].copy_from_slice(&bytes[done..done + count]);
                let id = store.store_chunk(&chunk).map_err(|_| libc::EIO)?;
                node.chunks.insert(index, id);
                done += count;
            }
            node.size = node.size.max(offset + bytes.len() as u64);
            node.modified = now();
            node.nanos = nanos();
            store.tree.nodes.insert(ino, node);
            Ok(bytes.len())
        })
    }
    pub fn setattr(&mut self, ino: u64, mode: Option<u16>, size: Option<u64>) -> FsResult<Node> {
        self.mutate(|store| {
            let mut node = store.node(ino)?;
            if let Some(mode) = mode {
                node.mode = mode & 0o777;
            }
            if let Some(size) = size {
                if node.kind != 2 {
                    return Err(libc::EISDIR);
                }
                if size > 16 * 1024 * 1024 * 1024 {
                    return Err(libc::EFBIG);
                }
                if size < node.size {
                    node.chunks
                        .retain(|index, _| *index * (CHUNK as u64) < size);
                    if size % CHUNK as u64 != 0 {
                        let index = size / CHUNK as u64;
                        if node.chunks.contains_key(&index) {
                            let mut bytes = store.chunk(&node, index).map_err(|_| libc::EIO)?;
                            bytes.truncate(size as usize % CHUNK);
                            let id = store.store_chunk(&bytes).map_err(|_| libc::EIO)?;
                            node.chunks.insert(index, id);
                        }
                    }
                }
                node.size = size;
            }
            node.modified = now();
            node.nanos = nanos();
            store.tree.nodes.insert(ino, node.clone());
            Ok(node)
        })
    }
    pub fn unlink(&mut self, parent: u64, name: &str, directory: bool) -> FsResult<()> {
        self.mutate(|store| {
            let node = store.lookup(parent, name)?;
            if directory && node.kind != 1 {
                return Err(libc::ENOTDIR);
            }
            if !directory && node.kind == 1 {
                return Err(libc::EISDIR);
            }
            if directory && !store.children(node.ino).is_empty() {
                return Err(libc::ENOTEMPTY);
            }
            store.tree.nodes.get_mut(&node.ino).unwrap().parent = 0;
            Ok(())
        })
    }
    pub fn rename(
        &mut self,
        parent: u64,
        name: &str,
        new_parent: u64,
        new_name: &str,
        no_replace: bool,
    ) -> FsResult<()> {
        self.mutate(|store| {
            let node = store.lookup(parent, name)?;
            if store.node(new_parent)?.kind != 1 {
                return Err(libc::ENOTDIR);
            }
            if new_name.is_empty() || new_name.contains('/') || [".", ".."].contains(&new_name) {
                return Err(libc::EINVAL);
            }
            let mut ancestor = new_parent;
            loop {
                if ancestor == node.ino {
                    return Err(libc::EINVAL);
                }
                if ancestor == 1 {
                    break;
                }
                ancestor = store.node(ancestor)?.parent;
            }
            if let Ok(existing) = store.lookup(new_parent, new_name) {
                if existing.ino == node.ino {
                    return Ok(());
                }
                if no_replace {
                    return Err(libc::EEXIST);
                }
                if existing.kind == 1 && node.kind != 1 {
                    return Err(libc::EISDIR);
                }
                if existing.kind != 1 && node.kind == 1 {
                    return Err(libc::ENOTDIR);
                }
                if existing.kind == 1 && !store.children(existing.ino).is_empty() {
                    return Err(libc::ENOTEMPTY);
                }
                store.tree.nodes.get_mut(&existing.ino).unwrap().parent = 0;
            }
            let node = store.tree.nodes.get_mut(&node.ino).unwrap();
            node.parent = new_parent;
            node.name = new_name.into();
            node.modified = now();
            node.nanos = nanos();
            Ok(())
        })
    }
    pub fn snapshots(&self) -> Result<Vec<Snapshot>> {
        let mut query = self
            .db
            .prepare("SELECT id,label,created,safe FROM snapshots ORDER BY id DESC")?;
        let rows = query
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, rusqlite::types::Value>(1)?,
                    r.get::<_, u64>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(id, label, created, safe)| {
                let label = match label {
                    rusqlite::types::Value::Text(text) => text,
                    rusqlite::types::Value::Blob(bytes) => String::from_utf8(
                        self.decode(&bytes, b"glyph-mounted-1/label", 65536)?
                            .to_vec(),
                    )?,
                    _ => anyhow::bail!("invalid snapshot label"),
                };
                Ok(Snapshot {
                    id,
                    label,
                    created,
                    safe,
                })
            })
            .collect()
    }

    pub fn checkpoint(&mut self, label: &str, safe: bool) -> Result<i64> {
        ensure!(!self.readonly, "emergency filesystem is read-only");
        let blob = self.encode(&serde_json::to_vec(&self.tree)?, b"glyph-mounted-1/tree")?;
        self.db.execute(
            "INSERT INTO snapshots(label,created,safe,tree) VALUES (?1,?2,?3,?4)",
            params![
                self.encode(label.as_bytes(), b"glyph-mounted-1/label")?,
                now(),
                safe,
                blob
            ],
        )?;
        let id = self.db.last_insert_rowid();
        if safe {
            self.set_setting("safe", id.to_string().as_bytes())?;
        }
        self.prune()?;
        Ok(id)
    }
    fn prune(&self) -> Result<()> {
        let mut retained = self
            .snapshots()?
            .into_iter()
            .take(self.retention)
            .map(|s| s.id)
            .collect::<BTreeSet<_>>();
        if let Some(safe) = self
            .setting("safe")?
            .and_then(|b| String::from_utf8(b).ok()?.parse::<i64>().ok())
        {
            retained.insert(safe);
        }
        for snapshot in self.snapshots()? {
            if !retained.contains(&snapshot.id) {
                self.db
                    .execute("DELETE FROM snapshots WHERE id=?1", [snapshot.id])?;
            }
        }
        let mut objects = self
            .tree
            .nodes
            .values()
            .flat_map(|n| n.chunks.values().copied())
            .collect::<BTreeSet<_>>();
        let blobs = self
            .db
            .prepare("SELECT tree FROM snapshots")?
            .query_map([], |r| r.get::<_, Vec<u8>>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for blob in blobs {
            let tree = self.decode_tree(&blob)?;
            objects.extend(tree.nodes.values().flat_map(|n| n.chunks.values().copied()));
        }
        let ids = self
            .db
            .prepare("SELECT id FROM objects")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for id in ids {
            if !objects.contains(&id) {
                self.db.execute("DELETE FROM objects WHERE id=?1", [id])?;
            }
        }
        Ok(())
    }
    pub fn restore(&mut self, id: i64) -> Result<()> {
        let blob: Vec<u8> =
            self.db
                .query_row("SELECT tree FROM snapshots WHERE id=?1", [id], |r| r.get(0))?;
        let tree = self.decode_tree(&blob)?;
        self.tree = tree;
        self.save_working()?;
        self.set_setting("safe", id.to_string().as_bytes())?;
        self.set_setting("dirty", b"0")?;
        self.restored = true;
        Ok(())
    }
    pub fn finish(&mut self) -> Result<()> {
        if self.readonly {
            return Ok(());
        }
        self.save_working()?;
        self.checkpoint("clean shutdown", true)?;
        self.set_setting("dirty", b"0")?;
        Ok(())
    }
    pub fn set_password(&mut self, password: &str, change: bool) -> Result<()> {
        ensure!(
            !self.readonly,
            "key management is disabled in emergency mode"
        );
        ensure!(password.chars().count() >= 12, "use at least 12 characters");
        ensure!(
            change == self.encrypted(),
            "use key set to enable encryption or key change to rotate it"
        );
        let mut salt = [0; 16];
        OsRng.fill_bytes(&mut salt);
        let wrapping = derive(password, &salt)?;
        if self.encrypted() {
            let record = crypt(
                wrapping.as_ref(),
                self.key.as_ref().unwrap().as_ref(),
                b"glyph-mounted-1/key",
            )?;
            let crypto = Crypto {
                salt: salt.to_vec(),
                nonce: record[..24].to_vec(),
                wrapped: record[24..].to_vec(),
            };
            self.set_setting("crypto", &serde_json::to_vec(&crypto)?)?;
            return Ok(());
        }
        let objects = self
            .db
            .prepare("SELECT id,payload FROM objects")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let snapshots = self
            .db
            .prepare("SELECT id,tree FROM snapshots")?
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let labels = self
            .snapshots()?
            .into_iter()
            .map(|s| (s.id, s.label))
            .collect::<Vec<_>>();
        let mut master = Zeroizing::new([0; 32]);
        OsRng.fill_bytes(master.as_mut());
        let record = crypt(wrapping.as_ref(), master.as_ref(), b"glyph-mounted-1/key")?;
        let crypto = Crypto {
            salt: salt.to_vec(),
            nonce: record[..24].to_vec(),
            wrapped: record[24..].to_vec(),
        };
        self.db.execute_batch("BEGIN IMMEDIATE")?;
        let converted = (|| -> Result<()> {
            for (id, payload) in objects {
                let plain = self.decode(
                    &payload,
                    format!("glyph-mounted-1/object/{id}").as_bytes(),
                    CHUNK as u64,
                )?;
                let mut gzip = GzEncoder::new(Vec::new(), Compression::new(self.level));
                gzip.write_all(&plain)?;
                let compressed = Zeroizing::new(gzip.finish()?);
                let mut encoded = vec![1];
                encoded.extend(crypt(
                    master.as_ref(),
                    &compressed,
                    format!("glyph-mounted-1/object/{id}").as_bytes(),
                )?);
                self.db.execute(
                    "UPDATE objects SET payload=?1 WHERE id=?2",
                    params![encoded, id],
                )?;
            }
            for (id, payload) in snapshots {
                let plain = self.decode(&payload, b"glyph-mounted-1/tree", 64 * 1024 * 1024)?;
                let mut gzip = GzEncoder::new(Vec::new(), Compression::new(self.level));
                gzip.write_all(&plain)?;
                let compressed = Zeroizing::new(gzip.finish()?);
                let mut encoded = vec![1];
                encoded.extend(crypt(
                    master.as_ref(),
                    &compressed,
                    b"glyph-mounted-1/tree",
                )?);
                self.db.execute(
                    "UPDATE snapshots SET tree=?1 WHERE id=?2",
                    params![encoded, id],
                )?;
            }
            self.set_setting("crypto", &serde_json::to_vec(&crypto)?)?;
            self.key = Some(master);
            for (id, label) in labels {
                let encoded = self.encode(label.as_bytes(), b"glyph-mounted-1/label")?;
                self.db.execute(
                    "UPDATE snapshots SET label=?1 WHERE id=?2",
                    params![encoded, id],
                )?;
            }
            self.save_working()?;
            Ok(())
        })();
        if let Err(error) = converted {
            self.key = None;
            self.db.execute_batch("ROLLBACK")?;
            return Err(error);
        }
        if let Err(error) = self.db.execute_batch("COMMIT") {
            self.key = None;
            let _ = self.db.execute_batch("ROLLBACK");
            return Err(error.into());
        }
        self.db.execute_batch("VACUUM")?;
        Ok(())
    }
}
