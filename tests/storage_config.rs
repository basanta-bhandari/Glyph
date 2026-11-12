use glyph_workspace::{
    storage::{Platform, StoragePaths, StorageSession},
    storage_config::{StorageSettings, default_config_path},
    workspace::Workspace,
};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

fn parse(text: &str) -> anyhow::Result<StorageSettings> {
    StorageSettings::from_text(
        text,
        Path::new("/config/glyph/storage.lua"),
        StoragePaths {
            data: "/default/data".into(),
            cache: "/default/cache".into(),
        },
        None,
        |key| (key == "HOME").then(|| "/home/example".into()),
    )
}

#[test]
fn lua_resolves_home_relative_and_absolute_storage_paths() {
    let value = parse("glyph.storage { data_dir = '~/GlyphData', session_dir = 'sessions', compression_level = 2 }").unwrap();
    assert_eq!(value.paths.data, PathBuf::from("/home/example/GlyphData"));
    assert_eq!(value.paths.cache, PathBuf::from("/config/glyph/sessions"));
    assert_eq!(value.compression_level, 2);
    let value =
        parse("glyph.storage { data_dir = '/external/disk', session_dir = '/dev/shm/glyph' }")
            .unwrap();
    assert_eq!(value.paths.data, PathBuf::from("/external/disk"));
    assert_eq!(value.paths.cache, PathBuf::from("/dev/shm/glyph"));
}

#[test]
fn bootstrap_paths_follow_os_config_conventions() {
    let env = |key: &str| match key {
        "HOME" => Some("/home/example".into()),
        "XDG_CONFIG_HOME" => Some("/custom/config".into()),
        "LOCALAPPDATA" => Some("C:/Local".into()),
        _ => None,
    };
    assert_eq!(
        default_config_path(Platform::Linux, env).unwrap(),
        PathBuf::from("/custom/config/glyph/storage.lua")
    );
    assert_eq!(
        default_config_path(Platform::MacOs, env).unwrap(),
        PathBuf::from("/home/example/Library/Application Support/Glyph/storage.lua")
    );
    assert_eq!(
        default_config_path(Platform::Windows, env).unwrap(),
        PathBuf::from("C:/Local/Glyph/storage.lua")
    );
}

#[test]
fn root_override_wins_for_data_but_keeps_explicit_session_location() {
    let settings = StorageSettings::from_text("glyph.storage { data_dir = '/ignored', session_dir = '/memory/session', compression_level = 9 }", Path::new("/config/storage.lua"), StoragePaths::portable("/chosen".into()), Some(Path::new("/chosen")), |_| None).unwrap();
    assert_eq!(settings.paths.data, PathBuf::from("/chosen"));
    assert_eq!(settings.paths.cache, PathBuf::from("/memory/session"));
    assert_eq!(settings.compression_level, 9);
}

#[test]
fn config_validation_rejects_unavailable_backends_and_bad_preferences() {
    for value in [
        "glyph.storage { backend = 'vm' }",
        "glyph.storage { compression_level = 0 }",
        "glyph.storage { compression_level = 10 }",
        "glyph.storage { data_dir = '' }",
        "glyph.storage { data_dir = '~someone/path' }",
        "glyph.storage { unexpected = true }",
        "glyph.storage {}; glyph.storage {}",
    ] {
        assert!(parse(value).is_err(), "accepted {value}");
    }
}

#[test]
fn bootstrap_lua_has_no_host_execution_and_is_bounded() {
    assert!(parse("os.execute('true')").is_err());
    assert!(parse("while true do end").is_err());
    assert!(parse("glyph.storage { data_dir = io.open('/etc/passwd'):read('*a') }").is_err());
}

#[test]
fn portable_default_is_seeded_without_overwriting_user_configuration() {
    let dir = TempDir::new().unwrap();
    let settings = StorageSettings::load(Some(dir.path()), None).unwrap();
    assert_eq!(settings.compression_level, 6);
    let config = dir.path().join("storage.lua");
    assert!(config.is_file());
    let custom = "glyph.storage { compression_level = 9, session_dir = 'custom-sessions' }";
    fs::write(&config, custom).unwrap();
    let settings = StorageSettings::load(Some(dir.path()), None).unwrap();
    assert_eq!(settings.compression_level, 9);
    assert_eq!(settings.paths.cache, dir.path().join("custom-sessions"));
    assert_eq!(fs::read_to_string(config).unwrap(), custom);
    assert!(
        StorageSettings::load(Some(dir.path()), Some(&dir.path().join("missing.lua"))).is_err()
    );
}

#[test]
fn configured_session_uses_selected_paths_and_compression_and_reopens() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("profile.lua");
    fs::write(
        &config,
        "glyph.storage { backend = 'container', data_dir = 'disk', session_dir = 'working', compression_level = 1 }",
    )
    .unwrap();
    let settings = StorageSettings::load(None, Some(&config)).unwrap();
    let mut session =
        StorageSession::configured(settings, || panic!("no encryption configured")).unwrap();
    let workspace = Workspace::open(session.root()).unwrap();
    workspace
        .write("/home/configured", "selected storage")
        .unwrap();
    assert!(session.root().starts_with(dir.path().join("working")));
    assert!(session.summary().contains("gzip level 1"));
    assert!(session.summary().contains("profile.lua"));
    session.finish().unwrap();
    drop(session);
    assert!(dir.path().join("disk/workspace.glyph").is_file());
    fs::write(
        &config,
        "glyph.storage { backend = 'container', data_dir = 'disk', session_dir = 'working', compression_level = 9 }",
    )
    .unwrap();
    let mut session =
        StorageSession::configured(StorageSettings::load(None, Some(&config)).unwrap(), || {
            panic!("no encryption configured")
        })
        .unwrap();
    assert_eq!(
        Workspace::open(session.root())
            .unwrap()
            .read("/home/configured")
            .unwrap(),
        "selected storage"
    );
    assert!(session.summary().contains("gzip level 9"));
    session.finish().unwrap();
}

#[test]
fn wrong_mounted_passphrase_never_runs_archive_cleanup_or_changes_disk() {
    use glyph_workspace::disk::DiskStore;
    use zeroize::Zeroizing;
    let dir = TempDir::new().unwrap();
    let settings = StorageSettings::load(Some(dir.path()), None).unwrap();
    let path = dir.path().join("workspace.glyphdisk");
    let mut disk = DiskStore::open(&path, 6, 5, false, || unreachable!()).unwrap();
    disk.set_password("test-only-valid-passphrase", false)
        .unwrap();
    disk.finish().unwrap();
    drop(disk);
    let original = fs::read(&path).unwrap();
    assert!(
        StorageSession::configured(settings, || Ok(Zeroizing::new(
            "test-only-wrong-passphrase".into()
        )))
        .is_err()
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "failed unlock must not mutate the backing file"
    );
    let mut disk = DiskStore::open(&path, 6, 5, false, || {
        Ok(Zeroizing::new("test-only-valid-passphrase".into()))
    })
    .unwrap();
    disk.finish().unwrap();
}
