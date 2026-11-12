use anyhow::{Context, Result, ensure};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use fs2::FileExt;
use rand::{RngCore, rngs::OsRng};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};
use tempfile::{NamedTempFile, TempDir};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"GLYPH001";
const TREE: &[u8; 8] = b"GLYPHT01";
const HEADER: usize = 49;
const MAX_CONTAINER: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED: u64 = 4 * 1024 * 1024 * 1024;
const ROOTS: &[&str] = &["AGENTS.md", "configs", "apps", "home", "system"];

#[derive(Clone, Copy)]
pub enum Platform {
    Linux,
    Windows,
    MacOs,
}

#[derive(Clone, Debug)]
pub struct StoragePaths {
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl StoragePaths {
    pub fn defaults() -> Result<Self> {
        let platform = if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        };
        Self::from_environment(platform, |key| std::env::var_os(key))
    }

    pub fn from_environment(
        platform: Platform,
        get: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self> {
        let absolute = |key: &str| {
            get(key)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
        };
        Ok(match platform {
            Platform::Linux => {
                let home = absolute("HOME").context("HOME is missing; specify --root DIR")?;
                Self {
                    data: absolute("XDG_DATA_HOME")
                        .unwrap_or_else(|| home.join(".local/share"))
                        .join("glyph"),
                    cache: absolute("XDG_CACHE_HOME")
                        .unwrap_or_else(|| home.join(".cache"))
                        .join("glyph"),
                }
            }
            Platform::MacOs => {
                let home = absolute("HOME").context("HOME is missing; specify --root DIR")?;
                Self {
                    data: home.join("Library/Application Support/Glyph"),
                    cache: home.join("Library/Caches/Glyph"),
                }
            }
            Platform::Windows => {
                // Known-folder environment variables are supplied by Windows.
                // Do not require host-platform Path::is_absolute in cross-platform
                // path-selection tests, where a Windows drive is not a Unix root.
                let local = get("LOCALAPPDATA")
                    .filter(|value| !value.is_empty())
                    .map(PathBuf::from)
                    .or_else(|| {
                        get("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Local"))
                    })
                    .context("LOCALAPPDATA is missing; specify --root DIR")?;
                Self {
                    data: local.join("Glyph"),
                    cache: local.join("Glyph/Cache"),
                }
            }
        })
    }

    pub fn portable(root: PathBuf) -> Self {
        Self {
            cache: root.join(".cache"),
            data: root,
        }
    }
}

pub struct StorageSession {
    pub paths: StoragePaths,
    pub container: PathBuf,
    working: Option<TempDir>,
    root: PathBuf,
    key: Option<Zeroizing<[u8; 32]>>,
    salt: [u8; 16],
    _lock: Option<File>,
    legacy: Vec<PathBuf>,
    closed: bool,
    compression_level: u32,
    config_source: Option<PathBuf>,
    mounted: Option<crate::mount::MountedDisk>,
    boot_validated: bool,
    remote: Option<crate::session::Remote>,
}

impl StorageSession {
    pub fn open(
        paths: StoragePaths,
        password: impl FnMut() -> Result<Zeroizing<String>>,
    ) -> Result<Self> {
        Self::open_with_compression(paths, 6, password)
    }

    pub fn configured(
        settings: crate::storage_config::StorageSettings,
        password: impl FnMut() -> Result<Zeroizing<String>>,
    ) -> Result<Self> {
        if settings.backend == "mounted" {
            return Self::open_mounted(settings, false, password);
        }
        let mut session =
            Self::open_with_compression(settings.paths, settings.compression_level, password)?;
        session.config_source = Some(settings.config_path);
        Ok(session)
    }

    pub fn open_with_compression(
        mut paths: StoragePaths,
        compression_level: u32,
        mut password: impl FnMut() -> Result<Zeroizing<String>>,
    ) -> Result<Self> {
        ensure!(
            (1..=9).contains(&compression_level),
            "compression_level must be between 1 and 9"
        );
        private_directory(&paths.data)?;
        private_directory(&paths.cache)?;
        paths.data = paths.data.canonicalize()?;
        paths.cache = paths.cache.canonicalize()?;
        let container = paths.data.join("workspace.glyph");
        let lock_path = paths.data.join("workspace.lock");
        if let Ok(meta) = fs::symlink_metadata(&lock_path) {
            ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "invalid storage lock file"
            );
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock_exclusive()
            .context("this Glyph store is already open in another session")?;
        let mut key = None;
        let mut salt = [0; 16];
        let mut payload = None;
        if container.exists() {
            ensure!(
                fs::symlink_metadata(&container)?.is_file(),
                "storage container must be a regular file"
            );
            ensure!(
                fs::metadata(&container)?.len() <= MAX_CONTAINER,
                "container exceeds the 512 MiB prototype limit"
            );
            let bytes = Zeroizing::new(fs::read(&container)?);
            ensure!(
                bytes.len() >= HEADER && &bytes[..8] == MAGIC,
                "not a supported Glyph container"
            );
            ensure!(bytes[8] <= 1, "unknown container encryption mode");
            salt.copy_from_slice(&bytes[9..25]);
            if bytes[8] == 1 {
                let derived = derive_key(password()?.as_bytes(), &salt)?;
                let cipher = XChaCha20Poly1305::new_from_slice(derived.as_ref())
                    .map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
                let plaintext = cipher.decrypt(XNonce::from_slice(&bytes[25..49]), Payload { msg: &bytes[HEADER..], aad: &bytes[..HEADER] }).map_err(|_| anyhow::anyhow!("incorrect key or damaged encrypted container; existing data was not changed"))?;
                key = Some(derived);
                payload = Some(Zeroizing::new(plaintext));
            } else {
                payload = Some(Zeroizing::new(bytes[HEADER..].to_vec()));
            }
        }
        let working = tempfile::Builder::new()
            .prefix("session-")
            .tempdir_in(&paths.cache)?;
        let root = working.path().to_path_buf();
        let mut legacy = Vec::new();
        if let Some(payload) = payload {
            unpack(&payload, &root)?;
            if paths.data.join("migration.pending").is_file() {
                legacy = ROOTS
                    .iter()
                    .map(|name| paths.data.join(name))
                    .filter(|path| path.exists())
                    .collect();
            }
        } else if paths.data.join("configs/init.lua").is_file()
            && paths.data.join("apps").is_dir()
            && paths.data.join("home").is_dir()
        {
            // Convert the old explicit --root layout without touching host links.
            let payload = pack(&paths.data, compression_level)?;
            unpack(&payload, &root)?;
            legacy = ROOTS
                .iter()
                .map(|name| paths.data.join(name))
                .filter(|path| path.exists())
                .collect();
            fs::write(
                paths.data.join("migration.pending"),
                b"legacy workspace conversion\n",
            )?;
        }
        Ok(Self {
            paths,
            container,
            working: Some(working),
            root,
            key,
            salt,
            _lock: Some(lock),
            legacy,
            closed: false,
            compression_level,
            config_source: None,
            mounted: None,
            boot_validated: false,
            remote: None,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn encrypted(&self) -> bool {
        if let Some(remote) = &self.remote {
            return remote
                .request("encrypted", serde_json::Value::Null)
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
        if let Some(mounted) = &self.mounted {
            return mounted.store.lock().unwrap().encrypted();
        }
        self.key.is_some()
    }

    pub fn summary(&self) -> String {
        if let Some(remote) = &self.remote {
            return remote
                .request("status", serde_json::Value::Null)
                .map(|v| v.as_str().unwrap_or_default().to_owned())
                .unwrap_or_else(|e| format!("Controller unavailable: {e}"));
        }
        if let Some(mounted) = &self.mounted {
            let store = mounted.store.lock().unwrap();
            return format!(
                "Backend: mounted virtual filesystem (FUSE)\nStorage configuration: {}\nVirtual disk: {}\nMount point: {}\nCompression: gzip level {}, independent 64 KiB chunks\nEncryption: {}\nMode: {}\nRecovery: {}",
                self.config_source
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                self.container.display(),
                self.root.display(),
                self.compression_level,
                if store.encrypted() {
                    "unlocked; authenticated encryption"
                } else {
                    "off; use key set"
                },
                if store.emergency {
                    "EMERGENCY — read only"
                } else {
                    "normal"
                },
                store.recovery_reason.as_deref().unwrap_or("not required")
            );
        }
        format!(
            "Backend: container (expanded working directory)\nStorage configuration: {}\nContainer: {}\nSession files: {}\nCompression: gzip level {}, on every normal session close\nEncryption: {}\nCaches/runtime: {}\nHost links: external originals; excluded from Glyph's encrypted storage",
            self.config_source
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "explicit library settings".into()),
            self.container.display(),
            self.root.display(),
            self.compression_level,
            if self.encrypted() {
                "XChaCha20-Poly1305; unlocked for this session"
            } else {
                "off; use key set"
            },
            self.paths.cache.display()
        )
    }

    pub fn set_key(&mut self, password: &str, change: bool) -> Result<()> {
        if let Some(remote) = &self.remote {
            remote.request(
                "key",
                serde_json::json!({"password":password,"change":change}),
            )?;
            return Ok(());
        }
        if let Some(mounted) = &self.mounted {
            return mounted.store.lock().unwrap().set_password(password, change);
        }
        ensure!(
            password.chars().count() >= 12,
            "use an encryption passphrase of at least 12 characters"
        );
        ensure!(
            change == self.encrypted(),
            "use key change for an encrypted store, or key set to enable encryption"
        );
        let mut salt = [0; 16];
        OsRng.fill_bytes(&mut salt);
        let new_key = derive_key(password.as_bytes(), &salt)?;
        self.commit(Some((new_key, salt)))
    }

    pub fn checkpoint(&mut self) -> Result<()> {
        if let Some(mounted) = &self.mounted {
            mounted
                .store
                .lock()
                .unwrap()
                .checkpoint("manual checkpoint", false)?;
            return Ok(());
        }
        self.commit(None)
    }

    fn commit(&mut self, replacement: Option<(Zeroizing<[u8; 32]>, [u8; 16])>) -> Result<()> {
        let payload = pack(&self.root, self.compression_level)?;
        let (key, salt) = match &replacement {
            Some((key, salt)) => (Some(key), salt),
            None => (self.key.as_ref(), &self.salt),
        };
        let mut header = Vec::from(MAGIC.as_slice());
        header.push(u8::from(key.is_some()));
        header.extend_from_slice(salt);
        let mut nonce = [0; 24];
        OsRng.fill_bytes(&mut nonce);
        header.extend_from_slice(&nonce);
        let encoded = if let Some(key) = key {
            let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
                .map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
            Zeroizing::new(
                cipher
                    .encrypt(
                        XNonce::from_slice(&nonce),
                        Payload {
                            msg: &payload,
                            aad: &header,
                        },
                    )
                    .map_err(|_| anyhow::anyhow!("failed to encrypt workspace"))?,
            )
        } else {
            payload
        };
        ensure!(
            encoded.len() as u64 + HEADER as u64 <= MAX_CONTAINER,
            "compressed container exceeds the 512 MiB prototype limit; session files are retained for recovery"
        );
        let mut temporary = NamedTempFile::new_in(&self.paths.data)?;
        temporary.write_all(&header)?;
        temporary.write_all(&encoded)?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.container)
            .map_err(|error| error.error)?;
        // The new key becomes active exactly when the replacement is committed.
        // Later cleanup errors must not revert the key for the already-written disk.
        if let Some((key, salt)) = replacement {
            self.key = Some(key);
            self.salt = salt;
        }
        #[cfg(unix)]
        {
            File::open(&self.paths.data)?.sync_all()?;
        }
        // Only the four known legacy workspace roots are removed, and only after
        // a durable, complete replacement container has been written.
        for path in &self.legacy {
            if path.exists() {
                if fs::symlink_metadata(path)?.is_dir() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
            }
        }
        self.legacy.clear();
        let marker = self.paths.data.join("migration.pending");
        if marker.exists() {
            fs::remove_file(marker)?;
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        if self.remote.is_some() {
            self.closed = true;
            return Ok(());
        }
        if self.closed {
            return Ok(());
        }
        if let Some(mut mounted) = self.mounted.take() {
            let readonly = mounted.store.lock().unwrap().readonly;
            let validation = if readonly || !self.boot_validated {
                Ok(())
            } else {
                crate::workspace::Workspace::open_existing(&self.root).map(|_| ())
            };
            let result = if readonly {
                Ok(())
            } else if self.boot_validated && validation.is_ok() {
                mounted.store.lock().unwrap().finish()
            } else {
                mounted
                    .store
                    .lock()
                    .unwrap()
                    .checkpoint("incomplete or invalid session (preserved)", false)
                    .map(|_| ())
            };
            mounted.unmount();
            self.closed = true;
            let marker = self.paths.data.join("mountpoint.json");
            if marker.exists() {
                fs::remove_file(marker)?;
            }
            if let Some(working) = self.working.take() {
                working.close()?;
            }
            result?;
            validation?;
            return Ok(());
        }
        self.checkpoint()?;
        // Do not rewrite a partially removed session after a cleanup failure.
        self.closed = true;
        if let Some(working) = self.working.take() {
            working
                .close()
                .context("container saved, but plaintext session cleanup failed")?;
        }
        Ok(())
    }
}

impl StorageSession {
    pub fn open_mounted(
        settings: crate::storage_config::StorageSettings,
        force_emergency: bool,
        mut password: impl FnMut() -> Result<Zeroizing<String>>,
    ) -> Result<Self> {
        let mut paths = settings.paths;
        private_directory(&paths.data)?;
        private_directory(&paths.cache)?;
        paths.data = paths.data.canonicalize()?;
        paths.cache = paths.cache.canonicalize()?;
        let disk_path = paths.data.join("workspace.glyphdisk");
        let existed = disk_path.exists();
        let mut cached_password: Option<Zeroizing<String>> = None;
        let mut session = if !existed {
            Self::open_with_compression(paths.clone(), settings.compression_level, || {
                let value = password()?;
                cached_password = Some(Zeroizing::new(value.to_string()));
                Ok(value)
            })?
        } else {
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(paths.data.join("workspace.lock"))?;
            lock.try_lock_exclusive()
                .context("this Glyph disk is already open")?;
            let working = tempfile::Builder::new()
                .prefix("mount-")
                .tempdir_in(&paths.cache)?;
            Self {
                paths: paths.clone(),
                container: disk_path.clone(),
                root: working.path().to_owned(),
                working: Some(working),
                key: None,
                salt: [0; 16],
                _lock: Some(lock),
                legacy: Vec::new(),
                closed: true, // Never archive-cleanup an unopened mounted disk.
                compression_level: settings.compression_level,
                config_source: None,
                mounted: None,
                boot_validated: false,
                remote: None,
            }
        };
        let marker = paths.data.join("mountpoint.json");
        if marker.is_file() {
            let old: PathBuf = serde_json::from_slice(&fs::read(&marker)?)?;
            ensure!(
                old.starts_with(&paths.cache)
                    && old
                        .components()
                        .all(|c| !matches!(c, std::path::Component::ParentDir))
                    && old
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("mount-")),
                "invalid stale mount record"
            );
            let status = std::process::Command::new("fusermount3")
                .args(["-uz", "--"])
                .arg(&old)
                .output();
            if let Ok(status) = status {
                if status.status.success() {
                    let _ = fs::remove_dir(&old);
                }
            }
            fs::remove_file(&marker)?;
        }
        let store = crate::disk::DiskStore::open(
            &disk_path,
            settings.compression_level,
            settings.snapshots,
            force_emergency,
            password,
        )?;
        let mount_dir = tempfile::Builder::new()
            .prefix("mount-")
            .tempdir_in(&paths.cache)?;
        let mounted = crate::mount::MountedDisk::mount(store, mount_dir.path())?;
        fs::write(&marker, serde_json::to_vec(mount_dir.path())?)?;
        if !existed {
            for name in ROOTS {
                let source = session.root.join(name);
                if source.exists() {
                    copy_tree(&source, &mount_dir.path().join(name))?;
                }
            }
            if let Some(value) = cached_password {
                mounted.store.lock().unwrap().set_password(&value, false)?;
            }
            if paths.data.join("workspace.glyph").exists() || !session.legacy.is_empty() {
                session.finish()?;
            }
        }
        if let Some(working) = session.working.take() {
            working.close()?;
        }
        session.root = mount_dir.path().to_owned();
        session.working = Some(mount_dir);
        session.mounted = Some(mounted);
        session.container = disk_path;
        session.closed = false;
        session.key = None;
        session.config_source = Some(settings.config_path);
        session.boot_validated = false;
        Ok(session)
    }
    pub fn emergency(&self) -> bool {
        self.remote
            .as_ref()
            .map(|r| r.descriptor.emergency)
            .unwrap_or_else(|| {
                self.mounted
                    .as_ref()
                    .is_some_and(|m| m.store.lock().unwrap().emergency)
            })
    }
    pub fn mark_boot_validated(&mut self) -> Result<()> {
        if let Some(mounted) = &self.mounted {
            let mut store = mounted.store.lock().unwrap();
            if !store.readonly {
                store.checkpoint("validated boot", true)?;
            }
        }
        self.boot_validated = true;
        Ok(())
    }
    pub fn snapshots(&self) -> Result<Vec<crate::disk::Snapshot>> {
        if let Some(r) = &self.remote {
            return Ok(serde_json::from_value(
                r.request("snapshots", serde_json::Value::Null)?,
            )?);
        }
        self.mounted
            .as_ref()
            .context("snapshots require the mounted backend")?
            .store
            .lock()
            .unwrap()
            .snapshots()
    }
    pub fn save_snapshot(&mut self, label: &str) -> Result<i64> {
        if let Some(r) = &self.remote {
            return r
                .request("save", serde_json::json!(label))?
                .as_i64()
                .context("invalid snapshot reply");
        }
        self.mounted
            .as_ref()
            .context("snapshots require the mounted backend")?
            .store
            .lock()
            .unwrap()
            .checkpoint(label, false)
    }
    pub fn restore_snapshot(&mut self, id: i64) -> Result<()> {
        if let Some(r) = &self.remote {
            r.request("restore", serde_json::json!(id))?;
            return Ok(());
        }
        self.mounted
            .as_ref()
            .context("snapshot recovery requires the mounted backend")?
            .store
            .lock()
            .unwrap()
            .restore(id)
    }
    pub fn remote(remote: crate::session::Remote) -> Result<Self> {
        let d = &remote.descriptor;
        Ok(Self {
            paths: StoragePaths {
                data: d.data.clone(),
                cache: d.cache.clone(),
            },
            container: d.data.join("workspace.glyphdisk"),
            root: d.root.clone(),
            working: None,
            key: None,
            salt: [0; 16],
            _lock: None,
            legacy: Vec::new(),
            closed: false,
            compression_level: 6,
            config_source: None,
            mounted: None,
            boot_validated: true,
            remote: Some(remote),
        })
    }
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(source)?;
    ensure!(
        !meta.file_type().is_symlink(),
        "migration rejects symlink sources"
    );
    if meta.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else {
        fs::copy(source, target)?;
        fs::set_permissions(target, meta.permissions())?;
    }
    Ok(())
}

impl Drop for StorageSession {
    fn drop(&mut self) {
        if !self.closed {
            if let Err(error) = self.finish() {
                if let Some(working) = self.working.take() {
                    let recovery = working.keep();
                    eprintln!(
                        "Glyph: storage save failed: {error:#}. Plaintext recovery files retained at {}",
                        recovery.display()
                    );
                } else {
                    eprintln!("Glyph: {error:#}");
                }
            }
        }
    }
}

fn derive_key(password: &[u8], salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>> {
    let params =
        Params::new(64 * 1024, 3, 1, Some(32)).map_err(|error| anyhow::anyhow!("{error}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0; 32]);
    argon
        .hash_password_into(password, salt, key.as_mut())
        .map_err(|error| anyhow::anyhow!("key derivation failed: {error}"))?;
    Ok(key)
}

fn private_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
    }
    ensure!(
        fs::symlink_metadata(path)?.is_dir(),
        "storage paths must be actual directories, not symlinks"
    );
    Ok(())
}

// A small versioned, length-prefixed filesystem archive. It accepts only
// regular files/directories, never follows links, and is always gzip compressed.
fn pack(root: &Path, compression_level: u32) -> Result<Zeroizing<Vec<u8>>> {
    let mut gzip = GzEncoder::new(Vec::new(), Compression::new(compression_level));
    gzip.write_all(TREE)?;
    let mut size = 0;
    let mut count = 0;
    for name in ROOTS {
        let path = root.join(name);
        if path.exists() {
            pack_entry(root, &path, &mut gzip, &mut size, &mut count)?;
        }
    }
    gzip.write_all(&[0])?;
    Ok(Zeroizing::new(gzip.finish()?))
}

fn pack_entry(
    root: &Path,
    path: &Path,
    gzip: &mut GzEncoder<Vec<u8>>,
    size: &mut u64,
    count: &mut usize,
) -> Result<()> {
    *count += 1;
    ensure!(
        *count <= 100_000,
        "workspace exceeds the 100,000-entry prototype limit"
    );
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() || metadata.is_dir(),
        "workspace archive rejects symlinks/special files: {}",
        path.display()
    );
    let relative = path
        .strip_prefix(root)?
        .to_str()
        .context("container paths must use Unicode filenames")?
        .replace(std::path::MAIN_SEPARATOR, "/");
    ensure!(
        !relative.contains('\\'),
        "container filenames cannot contain backslashes"
    );
    ensure!(relative.len() <= 4096, "archive path is too long");
    let length = if metadata.is_file() {
        metadata.len()
    } else {
        0
    };
    *size = size
        .checked_add(length + relative.len() as u64 + 17)
        .context("workspace size overflow")?;
    ensure!(
        *size <= MAX_EXPANDED,
        "workspace exceeds the 4 GiB expanded prototype limit"
    );
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mode: u32 = 0o600;
    gzip.write_all(&[if metadata.is_dir() { 1 } else { 2 }])?;
    gzip.write_all(&(relative.len() as u32).to_le_bytes())?;
    gzip.write_all(&length.to_le_bytes())?;
    gzip.write_all(&mode.to_le_bytes())?;
    gzip.write_all(relative.as_bytes())?;
    if metadata.is_file() {
        ensure!(
            io::copy(&mut File::open(path)?.take(length), gzip)? == length,
            "file changed while saving; previous container retained"
        );
    } else {
        let mut entries = fs::read_dir(path)?.collect::<io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            pack_entry(root, &entry.path(), gzip, size, count)?;
        }
    }
    Ok(())
}

fn unpack(bytes: &[u8], root: &Path) -> Result<()> {
    let mut gzip = GzDecoder::new(bytes);
    let mut magic = [0; 8];
    gzip.read_exact(&mut magic)?;
    ensure!(&magic == TREE, "invalid compressed workspace");
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    loop {
        let mut kind = [0];
        gzip.read_exact(&mut kind)?;
        if kind[0] == 0 {
            let mut trailing = [0];
            ensure!(gzip.read(&mut trailing)? == 0, "trailing archive data");
            break;
        }
        ensure!([1, 2].contains(&kind[0]), "invalid archive entry type");
        let mut entry = [0; 16];
        gzip.read_exact(&mut entry)?;
        let path_len = u32::from_le_bytes(entry[..4].try_into()?) as usize;
        let length = u64::from_le_bytes(entry[4..12].try_into()?);
        let mode = u32::from_le_bytes(entry[12..].try_into()?);
        ensure!(
            path_len > 0 && path_len <= 4096,
            "invalid archive path length"
        );
        total = total
            .checked_add(length)
            .and_then(|n| n.checked_add(path_len as u64 + 17))
            .context("archive size overflow")?;
        ensure!(
            total <= MAX_EXPANDED,
            "expanded archive exceeds the prototype limit"
        );
        let mut name = vec![0; path_len];
        gzip.read_exact(&mut name)?;
        let name = String::from_utf8(name)?;
        let relative = Path::new(&name);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                && !name.contains('\\')
                && !name.contains('\0'),
            "unsafe archive path"
        );
        ensure!(
            ROOTS.contains(&name.split('/').next().unwrap()),
            "unexpected archive root"
        );
        ensure!(seen.insert(name.clone()), "duplicate archive path");
        ensure!(
            seen.len() <= 100_000,
            "container exceeds the 100,000-entry prototype limit"
        );
        let destination = root.join(relative);
        if kind[0] == 1 {
            ensure!(length == 0, "directory entry has payload");
            fs::create_dir(&destination)?;
        } else {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&destination)?;
            ensure!(
                io::copy(&mut (&mut gzip).take(length), &mut file)? == length,
                "truncated archive file"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(mode & 0o777))?;
            }
            #[cfg(not(unix))]
            let _ = mode;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(names: &[&str]) -> Vec<u8> {
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(TREE).unwrap();
        for name in names {
            gzip.write_all(&[1]).unwrap();
            gzip.write_all(&(name.len() as u32).to_le_bytes()).unwrap();
            gzip.write_all(&0u64.to_le_bytes()).unwrap();
            gzip.write_all(&0o700u32.to_le_bytes()).unwrap();
            gzip.write_all(name.as_bytes()).unwrap();
        }
        gzip.write_all(&[0]).unwrap();
        gzip.finish().unwrap()
    }

    #[test]
    fn container_extraction_rejects_traversal_and_unexpected_roots() {
        for name in [
            "../escape",
            "/home/escape",
            "home/../escape",
            "home\\escape",
            "unrelated",
        ] {
            let temp = TempDir::new().unwrap();
            assert!(
                unpack(&archive(&[name]), temp.path()).is_err(),
                "accepted {name}"
            );
        }
    }

    #[test]
    fn container_extraction_rejects_duplicate_entries_and_truncation() {
        let temp = TempDir::new().unwrap();
        assert!(unpack(&archive(&["home", "home"]), temp.path()).is_err());
        let temp = TempDir::new().unwrap();
        let bytes = archive(&["home"]);
        assert!(unpack(&bytes[..bytes.len() - 6], temp.path()).is_err());
    }
}
