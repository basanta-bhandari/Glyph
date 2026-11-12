use crate::disk::{DiskStore, Node};
use anyhow::{Context, Result};
use fuser::*;
use std::{
    ffi::OsStr,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const TTL: Duration = Duration::from_millis(100);
fn kind(node: &Node) -> FileType {
    match node.kind {
        1 => FileType::Directory,
        3 => FileType::Symlink,
        _ => FileType::RegularFile,
    }
}
fn attr(node: &Node) -> FileAttr {
    let time = UNIX_EPOCH + Duration::new(node.modified, node.nanos);
    FileAttr {
        ino: node.ino,
        size: node.size,
        blocks: node.size.div_ceil(512),
        atime: time,
        mtime: time,
        ctime: time,
        crtime: time,
        kind: kind(node),
        perm: node.mode,
        nlink: if node.kind == 1 { 2 } else { 1 },
        uid: unsafe { libc::geteuid() },
        gid: unsafe { libc::getegid() },
        rdev: 0,
        flags: 0,
        blksize: 65536,
    }
}
fn name(name: &OsStr) -> std::result::Result<&str, i32> {
    name.to_str().ok_or(libc::EILSEQ)
}

struct Files {
    store: Arc<Mutex<DiskStore>>,
}
impl Filesystem for Files {
    fn lookup(&mut self, _: &Request<'_>, parent: u64, value: &OsStr, reply: ReplyEntry) {
        let result =
            name(value).and_then(|n| self.store.lock().map_err(|_| libc::EIO)?.lookup(parent, n));
        match result {
            Ok(n) => reply.entry(&TTL, &attr(&n), 0),
            Err(e) => reply.error(e),
        }
    }
    fn getattr(&mut self, _: &Request<'_>, ino: u64, _: Option<u64>, reply: ReplyAttr) {
        match self.store.lock().unwrap().node(ino) {
            Ok(n) => reply.attr(&TTL, &attr(&n)),
            Err(e) => reply.error(e),
        }
    }
    fn setattr(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        mode: Option<u32>,
        _: Option<u32>,
        _: Option<u32>,
        size: Option<u64>,
        _: Option<TimeOrNow>,
        _: Option<TimeOrNow>,
        _: Option<SystemTime>,
        _: Option<u64>,
        _: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<SystemTime>,
        _: Option<u32>,
        reply: ReplyAttr,
    ) {
        match self
            .store
            .lock()
            .unwrap()
            .setattr(ino, mode.map(|m| m as u16), size)
        {
            Ok(n) => reply.attr(&TTL, &attr(&n)),
            Err(e) => reply.error(e),
        }
    }
    fn readlink(&mut self, _: &Request<'_>, ino: u64, reply: ReplyData) {
        match self.store.lock().unwrap().node(ino) {
            Ok(n) if n.kind == 3 => reply.data(n.target.as_bytes()),
            Ok(_) => reply.error(libc::EINVAL),
            Err(e) => reply.error(e),
        }
    }
    fn mknod(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        value: &OsStr,
        mode: u32,
        umask: u32,
        _: u32,
        reply: ReplyEntry,
    ) {
        if mode & libc::S_IFMT != libc::S_IFREG {
            reply.error(libc::EPERM);
            return;
        }
        let result = name(value).and_then(|n| {
            self.store
                .lock()
                .unwrap()
                .create(parent, n, 2, (mode & !umask) as u16, "")
        });
        match result {
            Ok(n) => reply.entry(&TTL, &attr(&n), 0),
            Err(e) => reply.error(e),
        }
    }
    fn mkdir(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        value: &OsStr,
        mode: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        let result = name(value).and_then(|n| {
            self.store
                .lock()
                .unwrap()
                .create(parent, n, 1, (mode & !umask) as u16, "")
        });
        match result {
            Ok(n) => reply.entry(&TTL, &attr(&n), 0),
            Err(e) => reply.error(e),
        }
    }
    fn unlink(&mut self, _: &Request<'_>, parent: u64, value: &OsStr, reply: ReplyEmpty) {
        let result = name(value).and_then(|n| self.store.lock().unwrap().unlink(parent, n, false));
        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }
    fn rmdir(&mut self, _: &Request<'_>, parent: u64, value: &OsStr, reply: ReplyEmpty) {
        let result = name(value).and_then(|n| self.store.lock().unwrap().unlink(parent, n, true));
        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }
    fn symlink(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        value: &OsStr,
        target: &Path,
        reply: ReplyEntry,
    ) {
        let result = name(value).and_then(|n| {
            target
                .to_str()
                .ok_or(libc::EILSEQ)
                .and_then(|t| self.store.lock().unwrap().create(parent, n, 3, 0o777, t))
        });
        match result {
            Ok(n) => reply.entry(&TTL, &attr(&n), 0),
            Err(e) => reply.error(e),
        }
    }
    fn rename(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        old: &OsStr,
        newparent: u64,
        new: &OsStr,
        flags: u32,
        reply: ReplyEmpty,
    ) {
        if flags & !1 != 0 {
            reply.error(libc::EINVAL);
            return;
        }
        let result = name(old).and_then(|o| {
            name(new).and_then(|n| {
                self.store
                    .lock()
                    .unwrap()
                    .rename(parent, o, newparent, n, flags == 1)
            })
        });
        match result {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }
    fn open(&mut self, _: &Request<'_>, ino: u64, flags: i32, reply: ReplyOpen) {
        let store = self.store.lock().unwrap();
        match store.node(ino) {
            Ok(_) if store.readonly && flags & libc::O_ACCMODE != libc::O_RDONLY => {
                reply.error(libc::EROFS)
            }
            Ok(_) => reply.opened(ino, 0),
            Err(e) => reply.error(e),
        }
    }
    fn read(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _: u64,
        offset: i64,
        size: u32,
        _: i32,
        _: Option<u64>,
        reply: ReplyData,
    ) {
        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }
        match self
            .store
            .lock()
            .unwrap()
            .read(ino, offset as u64, size as usize)
        {
            Ok(bytes) => reply.data(&bytes),
            Err(e) => reply.error(e),
        }
    }
    fn write(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _: u64,
        offset: i64,
        data: &[u8],
        _: u32,
        _: i32,
        _: Option<u64>,
        reply: ReplyWrite,
    ) {
        if offset < 0 {
            reply.error(libc::EINVAL);
            return;
        }
        match self.store.lock().unwrap().write(ino, offset as u64, data) {
            Ok(n) => reply.written(n as u32),
            Err(e) => reply.error(e),
        }
    }
    fn flush(&mut self, _: &Request<'_>, _: u64, _: u64, _: u64, reply: ReplyEmpty) {
        reply.ok()
    }
    fn fsync(&mut self, _: &Request<'_>, _: u64, _: u64, _: bool, reply: ReplyEmpty) {
        reply.ok()
    }
    fn fsyncdir(&mut self, _: &Request<'_>, _: u64, _: u64, _: bool, reply: ReplyEmpty) {
        reply.ok()
    }
    fn readdir(
        &mut self,
        _: &Request<'_>,
        ino: u64,
        _: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let store = self.store.lock().unwrap();
        let parent = match store.node(ino) {
            Ok(n) => n.parent,
            Err(e) => {
                reply.error(e);
                return;
            }
        };
        let mut entries = vec![
            (ino, FileType::Directory, ".".to_owned()),
            (parent, FileType::Directory, "..".to_owned()),
        ];
        entries.extend(
            store
                .children(ino)
                .iter()
                .map(|n| (n.ino, kind(n), n.name.clone())),
        );
        for (index, (ino, kind, name)) in entries.iter().enumerate().skip(offset.max(0) as usize) {
            if reply.add(*ino, (index + 1) as i64, *kind, name) {
                break;
            }
        }
        reply.ok()
    }
    fn statfs(&mut self, _: &Request<'_>, _: u64, reply: ReplyStatfs) {
        reply.statfs(1 << 28, 1 << 27, 1 << 27, 100000, 90000, 65536, 255, 65536)
    }
    fn access(&mut self, _: &Request<'_>, ino: u64, mask: i32, reply: ReplyEmpty) {
        let store = self.store.lock().unwrap();
        match store.node(ino) {
            Ok(_) if store.readonly && mask & libc::W_OK != 0 => reply.error(libc::EROFS),
            Ok(_) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }
    fn create(
        &mut self,
        _: &Request<'_>,
        parent: u64,
        value: &OsStr,
        mode: u32,
        umask: u32,
        _: i32,
        reply: ReplyCreate,
    ) {
        let result = name(value).and_then(|n| {
            self.store
                .lock()
                .unwrap()
                .create(parent, n, 2, (mode & !umask) as u16, "")
        });
        match result {
            Ok(n) => reply.created(&TTL, &attr(&n), 0, n.ino, 0),
            Err(e) => reply.error(e),
        }
    }
}

pub struct MountedDisk {
    pub store: Arc<Mutex<DiskStore>>,
    session: Option<BackgroundSession>,
}
impl MountedDisk {
    pub fn mount(store: DiskStore, path: &Path) -> Result<Self> {
        let store = Arc::new(Mutex::new(store));
        let options = vec![
            MountOption::FSName("Glyph".into()),
            MountOption::DefaultPermissions,
            MountOption::RW,
        ];
        let session=fuser::spawn_mount2(Files{store:store.clone()},path,&options).context("mounting Glyph disk: Linux FUSE and /dev/fuse are required; no archive fallback is performed")?;
        Ok(Self {
            store,
            session: Some(session),
        })
    }
    pub fn unmount(&mut self) {
        if let Some(session) = self.session.take() {
            session.join();
        }
    }
}
