use glyph_workspace::{system, workspace::Workspace};
use sha2::{Digest, Sha256};
use std::fs;
#[test]
fn activation_requires_the_exact_health_checked_binary_and_retains_previous_runtimes() {
    let t = tempfile::tempdir().unwrap();
    let w = Workspace::open(&t.path().join("workspace")).unwrap();
    let data = t.path().join("data");
    let directory = w.root.join("system/builds/candidate");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("glyph"), b"unvalidated").unwrap();
    assert!(system::activate(&w, &data).is_err());
    for n in 0..5u8 {
        let bytes = vec![n; 16];
        let hash = format!("{:x}", Sha256::digest(&bytes));
        fs::write(directory.join("glyph"), &bytes).unwrap();
        fs::write(
            directory.join("health.json"),
            serde_json::to_vec(&serde_json::json!({"status":"ok","runtime_sha256":hash})).unwrap(),
        )
        .unwrap();
        system::activate(&w, &data).unwrap();
    }
    let manifest = system::read_manifest(&data).unwrap();
    assert_eq!(manifest.previous.len(), 3);
    assert_eq!(fs::read_dir(data.join("runtimes")).unwrap().count(), 5); // four runtimes + manifest
    fs::write(directory.join("glyph"), b"changed after check").unwrap();
    assert!(system::activate(&w, &data).is_err());
    assert_eq!(
        system::read_manifest(&data).unwrap().active,
        manifest.active
    );
    system::rollback_runtime(&data).unwrap();
    assert_eq!(
        system::read_manifest(&data).unwrap().active,
        manifest.previous.first().cloned()
    );
}
