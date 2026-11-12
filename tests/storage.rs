use glyph_workspace::{
    shell::{Effect, Shell},
    storage::{Platform, StoragePaths, StorageSession},
    workspace::Workspace,
};
use std::{collections::BTreeMap, ffi::OsString, fs, path::PathBuf};
use tempfile::TempDir;
use zeroize::Zeroizing;

fn paths(dir: &TempDir) -> StoragePaths {
    StoragePaths {
        data: dir.path().join("data"),
        cache: dir.path().join("cache"),
    }
}
fn no_password() -> anyhow::Result<Zeroizing<String>> {
    panic!("unencrypted storage must not request a key")
}
fn password() -> anyhow::Result<Zeroizing<String>> {
    Ok(Zeroizing::new("a sufficiently long secret".into()))
}

#[test]
fn os_data_and_cache_paths_follow_platform_conventions() {
    let env = BTreeMap::from([
        ("HOME", "/users/example"),
        ("XDG_DATA_HOME", "/custom/data"),
        ("XDG_CACHE_HOME", "/custom/cache"),
        ("LOCALAPPDATA", "C:/Users/Example/AppData/Local"),
    ]);
    let lookup = |key: &str| env.get(key).map(OsString::from);
    let linux = StoragePaths::from_environment(Platform::Linux, lookup).unwrap();
    assert_eq!(linux.data, PathBuf::from("/custom/data/glyph"));
    assert_eq!(linux.cache, PathBuf::from("/custom/cache/glyph"));
    let windows = StoragePaths::from_environment(Platform::Windows, lookup).unwrap();
    assert_eq!(
        windows.data,
        PathBuf::from("C:/Users/Example/AppData/Local/Glyph")
    );
    let mac = StoragePaths::from_environment(Platform::MacOs, lookup).unwrap();
    assert_eq!(
        mac.data,
        PathBuf::from("/users/example/Library/Application Support/Glyph")
    );
    let defaults = StoragePaths::from_environment(Platform::Linux, |key| {
        if key == "HOME" {
            Some("/users/example".into())
        } else {
            Some("relative/ignored".into())
        }
    })
    .unwrap();
    assert_eq!(
        defaults.data,
        PathBuf::from("/users/example/.local/share/glyph")
    );
    assert_eq!(defaults.cache, PathBuf::from("/users/example/.cache/glyph"));
}

#[test]
fn normal_close_compresses_files_and_removes_expanded_session() {
    let dir = TempDir::new().unwrap();
    let mut session = StorageSession::open(paths(&dir), no_password).unwrap();
    let workspace = Workspace::open(session.root()).unwrap();
    let body = "compress this repeated workspace data\n".repeat(40_000);
    workspace.write("/home/data.txt", &body).unwrap();
    fs::create_dir(workspace.root.join("home/empty")).unwrap();
    let expanded = session.root().to_owned();
    session.finish().unwrap();
    assert!(!expanded.exists());
    let container = session.container.clone();
    assert!(fs::metadata(&container).unwrap().len() < body.len() as u64 / 10);
    drop(session);
    let mut reopened = StorageSession::open(paths(&dir), no_password).unwrap();
    let workspace = Workspace::open(reopened.root()).unwrap();
    assert_eq!(workspace.read("/home/data.txt").unwrap(), body);
    assert!(workspace.root.join("home/empty").is_dir());
    reopened.finish().unwrap();
}

#[test]
fn encryption_wrong_key_tampering_and_rotation_are_handled() {
    let dir = TempDir::new().unwrap();
    let mut session = StorageSession::open(paths(&dir), no_password).unwrap();
    let workspace = Workspace::open(session.root()).unwrap();
    workspace
        .write("/home/private", "private workspace content")
        .unwrap();
    assert!(session.set_key("short", false).is_err());
    session
        .set_key("a sufficiently long secret", false)
        .unwrap();
    assert!(session.encrypted());
    let encrypted = fs::read(&session.container).unwrap();
    assert_eq!(encrypted[8], 1);
    assert!(
        !encrypted
            .windows(25)
            .any(|window| window == b"private workspace content")
    );
    session.checkpoint().unwrap();
    let second = fs::read(&session.container).unwrap();
    assert_ne!(encrypted, second, "each save must use a new random nonce");
    session.finish().unwrap();
    drop(session);
    let committed = fs::read(paths(&dir).data.join("workspace.glyph")).unwrap();
    assert!(
        StorageSession::open(paths(&dir), || Ok(Zeroizing::new(
            "a completely wrong key".into()
        )))
        .is_err()
    );
    assert_eq!(
        fs::read(paths(&dir).data.join("workspace.glyph")).unwrap(),
        committed
    );
    let mut opened = StorageSession::open(paths(&dir), password).unwrap();
    assert_eq!(
        Workspace::open(opened.root())
            .unwrap()
            .read("/home/private")
            .unwrap(),
        "private workspace content"
    );
    opened.set_key("a replacement secret phrase", true).unwrap();
    opened.finish().unwrap();
    drop(opened);
    assert!(StorageSession::open(paths(&dir), password).is_err());
    let mut opened = StorageSession::open(paths(&dir), || {
        Ok(Zeroizing::new("a replacement secret phrase".into()))
    })
    .unwrap();
    opened.finish().unwrap();
    drop(opened);
    let path = paths(&dir).data.join("workspace.glyph");
    let mut damaged = fs::read(&path).unwrap();
    *damaged.last_mut().unwrap() ^= 1;
    fs::write(&path, &damaged).unwrap();
    assert!(
        StorageSession::open(paths(&dir), || Ok(Zeroizing::new(
            "a replacement secret phrase".into()
        )))
        .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), damaged);
}

#[test]
fn concurrent_sessions_cannot_overwrite_each_other() {
    let dir = TempDir::new().unwrap();
    let mut first = StorageSession::open(paths(&dir), no_password).unwrap();
    Workspace::open(first.root()).unwrap();
    assert!(StorageSession::open(paths(&dir), no_password).is_err());
    first.finish().unwrap();
    drop(first);
    let mut second = StorageSession::open(paths(&dir), no_password).unwrap();
    second.finish().unwrap();
}

#[test]
fn legacy_directories_convert_only_after_a_successful_container_save() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().join("legacy");
    let workspace = Workspace::open(&store).unwrap();
    workspace.write("/home/keep", "existing user data").unwrap();
    fs::write(store.join("unrelated.txt"), "do not remove").unwrap();
    let mut session =
        StorageSession::open(StoragePaths::portable(store.clone()), no_password).unwrap();
    assert!(store.join("home/keep").exists());
    let recovered = Workspace::open(session.root()).unwrap();
    assert_eq!(recovered.read("/home/keep").unwrap(), "existing user data");
    session.finish().unwrap();
    drop(session);
    assert!(!store.join("home").exists());
    assert!(!store.join("configs").exists());
    assert_eq!(
        fs::read_to_string(store.join("unrelated.txt")).unwrap(),
        "do not remove"
    );
    let mut reopened = StorageSession::open(StoragePaths::portable(store), no_password).unwrap();
    assert_eq!(
        Workspace::open(reopened.root())
            .unwrap()
            .read("/home/keep")
            .unwrap(),
        "existing user data"
    );
    reopened.finish().unwrap();
}

#[test]
fn linked_host_files_remain_external_and_are_not_archived() {
    let dir = TempDir::new().unwrap();
    let host_file = dir.path().join("host-original");
    fs::write(&host_file, "HOST CONTENT").unwrap();
    let mut session = StorageSession::open(paths(&dir), no_password).unwrap();
    let workspace = Workspace::open(session.root()).unwrap();
    fs::write(
        workspace.root.join("configs/links/host.lua"),
        format!(
            "glyph.link {{ source = {:?}, target = '/home/linked', access = 'read-write' }}",
            host_file.to_str().unwrap()
        ),
    )
    .unwrap();
    let workspace = Workspace::open(session.root()).unwrap();
    workspace.write("/home/linked", "HOST MODIFIED").unwrap();
    session.finish().unwrap();
    drop(session);
    assert_eq!(fs::read_to_string(&host_file).unwrap(), "HOST MODIFIED");
    let mut reopened = StorageSession::open(paths(&dir), no_password).unwrap();
    assert!(!reopened.root().join("home/linked").exists());
    assert_eq!(
        Workspace::open(reopened.root())
            .unwrap()
            .read("/home/linked")
            .unwrap(),
        "HOST MODIFIED"
    );
    reopened.finish().unwrap();
}

#[cfg(unix)]
#[test]
fn failed_saves_keep_the_previous_container_and_recovery_files() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new().unwrap();
    let mut session = StorageSession::open(paths(&dir), no_password).unwrap();
    Workspace::open(session.root()).unwrap();
    session.checkpoint().unwrap();
    let committed = fs::read(&session.container).unwrap();
    let expanded = session.root().to_owned();
    symlink("/etc/passwd", expanded.join("home/unsupported-link")).unwrap();
    assert!(session.finish().is_err());
    assert_eq!(fs::read(&session.container).unwrap(), committed);
    drop(session);
    assert!(
        expanded.exists(),
        "failed save must retain plaintext recovery files"
    );
}

#[test]
fn encryption_commands_are_user_only_and_never_take_password_arguments() {
    let dir = TempDir::new().unwrap();
    let session = StorageSession::open(paths(&dir), no_password).unwrap();
    let workspace = Workspace::open(session.root()).unwrap();
    let mut shell = Shell::new(workspace, true);
    shell.storage = Some(session);
    assert!(shell.execute("key set").is_err());
    assert!(shell.execute("key set this-is-a-secret").is_err());
    shell.agent = false;
    assert!(
        matches!(&shell.execute("key status").unwrap()[0], Effect::Output(info) if info.contains("Encryption: off"))
    );
    assert!(
        glyph_workspace::ui::process_effects(
            &mut shell,
            vec![Effect::Key { change: false }],
            false
        )
        .is_err()
    );
    assert!(!shell.storage.as_ref().unwrap().encrypted());
}
