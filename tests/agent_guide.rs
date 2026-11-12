use glyph_workspace::{
    sandbox,
    shell::Shell,
    workspace::{DEFAULT_AGENT_GUIDE, Workspace},
};
use std::fs;
use tempfile::TempDir;

#[test]
fn guide_is_available_at_root_and_home_and_user_edits_survive_restart() {
    let dir = TempDir::new().unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    assert_eq!(workspace.read("/AGENTS.md").unwrap(), DEFAULT_AGENT_GUIDE);
    assert_eq!(
        workspace.read("/home/AGENTS.md").unwrap(),
        DEFAULT_AGENT_GUIDE
    );
    fs::write(dir.path().join("AGENTS.md"), "User's root instructions").unwrap();
    fs::write(
        dir.path().join("home/AGENTS.md"),
        "User's workspace instructions",
    )
    .unwrap();
    let reopened = Workspace::open(dir.path()).unwrap();
    assert_eq!(
        reopened.read("/AGENTS.md").unwrap(),
        "User's root instructions"
    );
    assert_eq!(
        reopened.read("/home/AGENTS.md").unwrap(),
        "User's workspace instructions"
    );
}

#[test]
fn glyph_commands_cannot_rewrite_remove_or_replace_the_guide() {
    let dir = TempDir::new().unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    workspace.write("/home/source", "replacement").unwrap();
    let mut shell = Shell::new(workspace, true);
    for command in [
        "write /AGENTS.md replacement",
        "write /home/AGENTS.md replacement",
        "rm /home/AGENTS.md",
        "move /home/AGENTS.md /home/old-guide",
        "copy /home/source /home/AGENTS.md",
        "move /home/source /home/AGENTS.md",
    ] {
        assert!(
            shell.execute(command).is_err(),
            "unexpectedly allowed: {command}"
        );
    }
    assert_eq!(
        shell.workspace.read("/home/AGENTS.md").unwrap(),
        DEFAULT_AGENT_GUIDE
    );
    shell.execute("mkdir /home/project").unwrap();
    shell
        .execute("write /home/project/AGENTS.md 'project instructions'")
        .unwrap();
    assert_eq!(
        shell.workspace.read("/home/project/AGENTS.md").unwrap(),
        "project instructions"
    );
}

#[test]
fn a_host_link_cannot_shadow_the_protected_guide() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("glyph");
    Workspace::open(&root).unwrap();
    let source = dir.path().join("replacement");
    fs::write(&source, "replacement").unwrap();
    fs::write(
        root.join("configs/links/override.lua"),
        format!(
            "glyph.link {{ source = {:?}, target = '/home/AGENTS.md', access = 'read-write' }}",
            source.to_str().unwrap()
        ),
    )
    .unwrap();
    assert!(Workspace::open(&root).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn real_agent_process_reads_guide_but_cannot_modify_or_delete_it() {
    let dir = TempDir::new().unwrap();
    Workspace::open(dir.path()).unwrap();
    fs::write(
        dir.path().join("configs/apps/probe.lua"),
        "glyph.app { path = '/apps/hello.app', network = true }",
    )
    .unwrap();
    fs::write(dir.path().join("apps/hello.app"), r#"#!/usr/bin/env python3
import os
assert os.environ['GLYPH_AGENT_GUIDE'] == '/AGENTS.md'
for path in ['/AGENTS.md', '/home/AGENTS.md']:
    with open(path) as f:
        assert 'Agent guide to Glyph' in f.read()
    for operation in [lambda: open(path, 'w'), lambda: os.unlink(path), lambda: os.rename(path, '/home/stolen-guide')]:
        try:
            operation()
        except OSError:
            pass
        else:
            raise AssertionError('protected guide was modified: ' + path)
with open('/home/agent-result', 'w') as f:
    f.write('guide visible and protected')
"#).unwrap();
    let workspace = Workspace::open(dir.path()).unwrap();
    let plan = workspace.launch_plan("/home", "hello", &[], true).unwrap();
    let result = sandbox::command(&workspace, &plan)
        .unwrap()
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        workspace.read("/home/agent-result").unwrap(),
        "guide visible and protected"
    );
    assert_eq!(
        workspace.read("/home/AGENTS.md").unwrap(),
        DEFAULT_AGENT_GUIDE
    );
}
