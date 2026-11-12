use glyph_workspace::disk::{CHUNK, DiskStore};
use tempfile::TempDir;
use zeroize::Zeroizing;
fn open(dir: &TempDir) -> DiskStore {
    DiskStore::open(&dir.path().join("disk"), 6, 5, false, || {
        panic!("unexpected unlock")
    })
    .unwrap()
}
#[test]
fn chunks_support_sparse_writes_truncation_rename_and_reopen() {
    let dir = TempDir::new().unwrap();
    let mut disk = open(&dir);
    let folder = disk.create(1, "home", 1, 0o700, "").unwrap();
    let file = disk.create(folder.ino, "note", 2, 0o600, "").unwrap();
    disk.write(file.ino, (CHUNK - 2) as u64, b"abcdef").unwrap();
    assert_eq!(
        disk.read(file.ino, (CHUNK - 4) as u64, 12).unwrap(),
        b"\0\0abcdef"
    );
    disk.setattr(file.ino, None, Some(2)).unwrap();
    disk.setattr(file.ino, None, Some(CHUNK as u64 + 10))
        .unwrap();
    assert_eq!(
        disk.read(file.ino, (CHUNK - 4) as u64, 10).unwrap(),
        [0; 10]
    );
    disk.rename(folder.ino, "note", folder.ino, "renamed", false)
        .unwrap();
    disk.finish().unwrap();
    drop(disk);
    let disk = open(&dir);
    assert!(disk.lookup(folder.ino, "note").is_err());
    assert_eq!(disk.lookup(folder.ino, "renamed").unwrap().ino, file.ino);
}
#[test]
fn crash_enters_readonly_last_safe_and_preserves_unclean_snapshot() {
    let dir = TempDir::new().unwrap();
    let mut disk = open(&dir);
    let file = disk.create(1, "note", 2, 0o600, "").unwrap();
    disk.write(file.ino, 0, b"safe").unwrap();
    disk.checkpoint("known good", true).unwrap();
    disk.write(file.ino, 0, b"unfinished").unwrap();
    drop(disk);
    let mut disk = open(&dir);
    assert!(disk.emergency && disk.readonly);
    assert_eq!(disk.read(file.ino, 0, 100).unwrap(), b"safe");
    assert!(disk.write(file.ino, 0, b"bad").is_err());
    let unfinished = disk
        .snapshots()
        .unwrap()
        .into_iter()
        .find(|s| s.label.contains("unclean"))
        .unwrap();
    disk.restore(unfinished.id).unwrap();
    drop(disk);
    let disk = open(&dir);
    assert!(!disk.emergency);
    assert_eq!(disk.read(file.ino, 0, 100).unwrap(), b"unfinished");
}
#[test]
fn snapshot_retention_preserves_last_safe_and_restores_content() {
    let dir = TempDir::new().unwrap();
    let mut disk = open(&dir);
    let file = disk.create(1, "note", 2, 0o600, "").unwrap();
    disk.write(file.ino, 0, b"original").unwrap();
    let safe = disk.checkpoint("private snapshot label", true).unwrap();
    for n in 0..10 {
        disk.write(file.ino, 0, &[n]).unwrap();
        disk.checkpoint("manual", false).unwrap();
    }
    assert!(disk.snapshots().unwrap().len() <= 6);
    disk.restore(safe).unwrap();
    assert_eq!(disk.read(file.ino, 0, 100).unwrap(), b"original");
}
#[test]
fn encrypted_chunks_and_metadata_reopen_and_key_rotation_keeps_snapshots() {
    let dir = TempDir::new().unwrap();
    let mut disk = open(&dir);
    let file = disk.create(1, "private-name", 2, 0o600, "").unwrap();
    disk.write(file.ino, 0, b"secret contents").unwrap();
    let safe = disk.checkpoint("private snapshot label", true).unwrap();
    disk.set_password("test-only-disk-password", false).unwrap();
    assert!(
        disk.snapshots()
            .unwrap()
            .iter()
            .any(|s| s.label == "private snapshot label")
    );
    disk.finish().unwrap();
    drop(disk);
    assert!(
        DiskStore::open(&dir.path().join("disk"), 6, 5, false, || Ok(
            Zeroizing::new("wrong password".into())
        ))
        .is_err()
    );
    let bytes = std::fs::read(dir.path().join("disk")).unwrap();
    assert!(
        !bytes
            .windows(b"private snapshot label".len())
            .any(|w| w == b"private snapshot label")
    );
    let mut disk = DiskStore::open(&dir.path().join("disk"), 6, 5, false, || {
        Ok(Zeroizing::new("test-only-disk-password".into()))
    })
    .unwrap();
    assert_eq!(disk.read(file.ino, 0, 100).unwrap(), b"secret contents");
    disk.restore(safe).unwrap();
    disk.set_password("test-only-replacement-password", true)
        .unwrap();
    disk.finish().unwrap();
    drop(disk);
    let disk = DiskStore::open(&dir.path().join("disk"), 6, 5, false, || {
        Ok(Zeroizing::new("test-only-replacement-password".into()))
    })
    .unwrap();
    assert_eq!(disk.read(file.ino, 0, 100).unwrap(), b"secret contents");
}
#[test]
fn directories_cannot_be_renamed_inside_themselves_and_unlinks_keep_open_inode() {
    let dir = TempDir::new().unwrap();
    let mut disk = open(&dir);
    let a = disk.create(1, "a", 1, 0o700, "").unwrap();
    let b = disk.create(a.ino, "b", 1, 0o700, "").unwrap();
    assert!(disk.rename(1, "a", b.ino, "bad", false).is_err());
    let file = disk.create(a.ino, "file", 2, 0o600, "").unwrap();
    disk.write(file.ino, 0, b"open").unwrap();
    disk.unlink(a.ino, "file", false).unwrap();
    assert!(disk.lookup(a.ino, "file").is_err());
    assert_eq!(disk.read(file.ino, 0, 100).unwrap(), b"open");
}
