use glyph_workspace::{
    package_install,
    shell::{Effect, Shell},
    workspace::Workspace,
};
use std::fs;
#[test]
fn fresh_workspaces_have_no_preinstalled_or_registered_agents() {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    assert!(w.config.agents().is_empty());
    for name in ["opencode", "codex"] {
        assert!(!dir.path().join(format!("apps/{name}.app")).exists());
        assert!(
            !dir.path()
                .join(format!("configs/agents/{name}.lua"))
                .exists()
        );
    }
    let mut shell = Shell::new(w, false);
    assert!(
        matches!(&shell.execute("agent install opencode").unwrap()[0],Effect::Package(op,args) if op=="install"&&args==&["opencode"])
    );
    assert!(shell.workspace.config.agents().is_empty());
    assert!(package_install::register(&shell.workspace, "opencode", true).is_err());
    assert!(!dir.path().join("configs/agents/opencode.lua").exists());
}
#[test]
fn registration_requires_a_glyph_local_executable_and_keeps_agent_confined() {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    fs::create_dir_all(dir.path().join("home/.local/bin")).unwrap();
    fs::write(
        dir.path().join("home/.local/bin/opencode"),
        "#!/bin/sh\nexit 0\n",
    )
    .unwrap();
    package_install::register(&w, "opencode", true).unwrap();
    let w = Workspace::open_existing(dir.path()).unwrap();
    assert_eq!(w.config.agents()[0].name, "opencode");
    let mut shell = Shell::new(w, false);
    match &shell.execute("agent opencode").unwrap()[0] {
        Effect::Launch(p) => {
            assert!(p.sandbox);
            assert!(p.network);
            assert_eq!(p.app, "/apps/opencode.app");
        }
        _ => panic!("expected agent launch"),
    }
    shell.agent = true;
    for command in [
        "agent install codex",
        "agent add codex",
        "edit /configs/agents/opencode.lua",
        "source activate",
    ] {
        assert!(shell.execute(command).is_err());
    }
}
#[test]
fn failed_package_install_does_not_register_an_agent() {
    let dir = tempfile::tempdir().unwrap();
    Workspace::open(dir.path()).unwrap();
    fs::write(dir.path().join("configs/packages.lua"),r#"glyph.package_manager {name='fail',network=true,install={'/usr/bin/false','{[pkg}]'},remove={'/usr/bin/false','{[pkg}]'},upgrade={'/usr/bin/false','{[pkg}]'}}"#).unwrap();
    let mut shell = Shell::new(Workspace::open_existing(dir.path()).unwrap(), false);
    let effects = shell.execute("agent install opencode").unwrap();
    assert!(glyph_workspace::ui::process_effects(&mut shell, effects, false).is_err());
    assert!(!dir.path().join("configs/agents/opencode.lua").exists());
    assert!(!dir.path().join("apps/opencode.app").exists());
}

#[test]
fn removing_registration_preserves_user_customizations() {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    fs::create_dir_all(dir.path().join("home/.local/bin")).unwrap();
    fs::write(dir.path().join("home/.local/bin/opencode"), "installed").unwrap();
    package_install::register(&w, "opencode", true).unwrap();
    fs::write(
        dir.path().join("configs/apps/opencode.lua"),
        "-- user custom configuration\n",
    )
    .unwrap();
    package_install::unregister(&w, "opencode").unwrap();
    assert!(!dir.path().join("apps/opencode.app").exists());
    assert!(!dir.path().join("configs/agents/opencode.lua").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("configs/apps/opencode.lua")).unwrap(),
        "-- user custom configuration\n"
    );
}
