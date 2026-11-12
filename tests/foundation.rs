use glyph_workspace::{
    config::Action,
    sandbox,
    shell::{Effect, Shell},
    workspace::{Workspace, normalize},
};
use std::{fs, path::Path};
use tempfile::TempDir;

fn fixture(extra: &str) -> (TempDir, Workspace) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("glyph");
    let first = Workspace::open(&root).unwrap();
    fs::write(
        root.join("configs/init.lua"),
        format!("{}\n{extra}", glyph_workspace::workspace::DEFAULT_CONFIG),
    )
    .unwrap();
    drop(first);
    let ws = Workspace::open(&root).unwrap();
    (dir, ws)
}

#[test]
fn workspace_and_files_survive_restart() {
    let (dir, ws) = fixture("");
    assert!(ws.root.join("apps/hello.app").is_file());
    assert!(!ws.root.join("apps/hello.app").is_dir());
    ws.write("/home/note", "light").unwrap();
    let next = Workspace::open(&dir.path().join("glyph")).unwrap();
    assert_eq!(next.read("/home/note").unwrap(), "light");
    assert_eq!(normalize("/a/b", "../..").unwrap(), "/");
    assert_eq!(normalize("/home", "../../etc").unwrap(), "/etc");
    assert!(next.read("/etc/passwd").is_err());
}

#[test]
fn shell_quoting_and_error_status_are_not_host_shell_syntax() {
    let (_dir, ws) = fixture("");
    let mut shell = Shell::new(ws, false);
    shell.execute("write note 'a > b && literal'").unwrap();
    assert_eq!(
        shell.workspace.read("/home/note").unwrap(),
        "a > b && literal"
    );
    assert!(shell.execute("cd /missing").is_err());
    assert!(shell.execute("/usr/bin/true").is_err());
    assert!(shell.execute("write /configs/init.lua nope").is_err());
    assert!(shell.execute("write /apps/hello.app nope").is_err());
    shell.execute("mkdir a").unwrap();
    shell.execute("cd a").unwrap();
    shell.execute("cd ../..").unwrap();
    assert_eq!(shell.cwd, "/");
}

#[test]
fn lua_theme_commands_and_bindings_work() {
    let (_dir, ws) = fixture("glyph.theme { accent = '#abcdef' }");
    assert_eq!(ws.config.theme().accent, "#abcdef");
    let actions = ws
        .config
        .invoke("hello", vec!["two words".into()], false)
        .unwrap()
        .unwrap();
    assert!(
        matches!(&actions[0], Action::Launch(path, args) if path == "/apps/hello.app" && args == &["two words"])
    );
    let actions = ws
        .config
        .invoke("Ctrl+Space", vec![], true)
        .unwrap()
        .unwrap();
    assert!(matches!(actions[0], Action::Launcher));
}

#[test]
fn lua_cannot_use_host_io_or_run_forever() {
    let (dir, ws) = fixture("");
    fs::write(ws.root.join("configs/init.lua"), "os.execute('true')").unwrap();
    assert!(Workspace::open(&dir.path().join("glyph")).is_err());
    fs::write(ws.root.join("configs/init.lua"), "while true do end").unwrap();
    assert!(Workspace::open(&dir.path().join("glyph")).is_err());
}

#[test]
fn links_enforce_access_and_recursion() {
    let (dir, ws) = fixture("");
    let host = dir.path().join("host");
    fs::create_dir_all(host.join("nested")).unwrap();
    fs::write(host.join("visible"), "original").unwrap();
    fs::write(host.join("nested/hidden"), "secret").unwrap();
    fs::write(ws.root.join("configs/links/host.lua"), format!("glyph.link {{ source = {:?}, target = '/home/host', access = 'read', recursive = false }}", host.to_str().unwrap())).unwrap();
    let ws = Workspace::open(&ws.root).unwrap();
    assert_eq!(ws.read("/home/host/visible").unwrap(), "original");
    assert!(ws.write("/home/host/visible", "changed").is_err());
    assert!(ws.read("/home/host/nested/hidden").is_err());
    assert!(ws.resolve("/home/host/nested", false).is_err());
    assert_eq!(
        ws.list("/home/host").unwrap(),
        vec![("visible".into(), false)]
    );
    fs::write(ws.root.join("configs/links/host.lua"), format!("glyph.link {{ source = {:?}, target = '/home/host', access = 'read-write', recursive = true }}", host.to_str().unwrap())).unwrap();
    let ws = Workspace::open(&ws.root).unwrap();
    ws.write("/home/host/visible", "changed").unwrap();
    assert_eq!(fs::read_to_string(host.join("visible")).unwrap(), "changed");
    assert_eq!(ws.read("/home/host/nested/hidden").unwrap(), "secret");
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_or_alias_policy() {
    use std::os::unix::fs::symlink;
    let (dir, ws) = fixture("");
    symlink("/etc", ws.root.join("home/escape")).unwrap();
    assert!(ws.read("/home/escape/passwd").is_err());
    symlink("../configs/init.lua", ws.root.join("home/policy")).unwrap();
    assert!(ws.write("/home/policy", "changed").is_err());
    let host = dir.path().join("host");
    fs::create_dir(&host).unwrap();
    symlink("/etc/passwd", host.join("escape")).unwrap();
    fs::write(
        ws.root.join("configs/links/host.lua"),
        format!(
            "glyph.link {{ source = {:?}, target = '/home/host', recursive = true }}",
            host.to_str().unwrap()
        ),
    )
    .unwrap();
    assert!(Workspace::open(&ws.root).is_err());
}

#[test]
fn overlapping_and_policy_links_are_rejected() {
    let (dir, ws) = fixture("");
    let host = dir.path().join("host");
    fs::create_dir(&host).unwrap();
    let source = host.to_str().unwrap();
    fs::write(
        ws.root.join("configs/links/host.lua"),
        format!("glyph.link {{ source = {source:?}, target = '/configs/evil' }}"),
    )
    .unwrap();
    assert!(Workspace::open(&ws.root).is_err());
    fs::write(ws.root.join("configs/links/host.lua"), format!("glyph.link {{ source = {source:?}, target = '/home/a' }}\nglyph.link {{ source = {source:?}, target = '/home/a/b' }}")).unwrap();
    assert!(Workspace::open(&ws.root).is_err());
}

#[test]
fn arbitrary_formats_use_user_handlers_and_preserve_arguments() {
    let (dir, ws) = fixture(
        "glyph.handler { kind = 'anything', command = {'/usr/bin/custom-runtime', '{app}', '{args}'} }",
    );
    let source = dir.path().join("program.anything");
    fs::write(&source, "opaque payload").unwrap();
    ws.import_app(&source, "custom", None).unwrap();
    let ws = Workspace::open(&ws.root).unwrap();
    let plan = ws
        .launch_plan(
            "/home",
            "custom",
            &["two words".into(), "$(touch /oops)".into()],
            false,
        )
        .unwrap();
    assert_eq!(
        plan.argv,
        [
            "/usr/bin/custom-runtime",
            "/apps/custom.app",
            "two words",
            "$(touch /oops)"
        ]
    );
    assert!(plan.sandbox);
    assert!(!plan.network);
}

#[test]
fn agents_and_headless_launches_cannot_approve_host_access() {
    let (_dir, ws) = fixture("glyph.app { path = '/apps/hello.app', sandbox = false }");
    assert!(ws.launch_plan("/home", "hello", &[], true).is_err());
    let plan = ws.launch_plan("/home", "hello", &[], false).unwrap();
    assert!(sandbox::execute(&ws, &plan, false).is_err());
    let mut shell = Shell::new(ws, false);
    assert!(
        glyph_workspace::ui::process_effects(&mut shell, vec![Effect::Launch(plan)], false)
            .is_err()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn actual_sandbox_hides_host_and_enforces_link_mounts() {
    let (dir, ws) = fixture("glyph.app { path = '/apps/hello.app', network = true }");
    // Network sharing is enabled only to make this filesystem test independent
    // of CI environments that prohibit creating network namespaces.
    let ro = dir.path().join("ro");
    let rw = dir.path().join("rw");
    fs::create_dir(&ro).unwrap();
    fs::create_dir(&rw).unwrap();
    fs::create_dir(ro.join("nested")).unwrap();
    fs::write(ro.join("visible"), "read only").unwrap();
    fs::write(ro.join("nested/hidden"), "hidden").unwrap();
    fs::write(rw.join("writable"), "before").unwrap();
    let secret = dir.path().join("HOST_SECRET");
    fs::write(&secret, "not exposed").unwrap();
    fs::write(ws.root.join("configs/links/test.lua"), format!("glyph.link {{ source = {:?}, target = '/home/ro', access = 'read' }}\nglyph.link {{ source = {:?}, target = '/home/rw', access = 'read-write', recursive = true }}", ro.to_str().unwrap(), rw.to_str().unwrap())).unwrap();
    let probe = format!(
        r#"#!/usr/bin/env python3
import os
assert not os.path.exists({secret:?})
assert os.environ.get('GLYPH_TEST_SECRET') is None
assert os.environ['HOME'] == '/home'
assert os.path.exists('/home/ro/visible')
assert not os.path.exists('/home/ro/nested/hidden')
for path in ['/configs/init.lua', '/apps/hello.app', '/home/ro/visible']:
    try:
        with open(path, 'a') as f: f.write('ESCAPED')
    except OSError:
        pass
    else:
        raise AssertionError('writable protected path: ' + path)
with open('/home/rw/writable', 'w') as f: f.write('after')
with open('/home/result', 'w') as f: f.write('isolated')
print('sandbox boundary verified')
"#,
        secret = secret.to_str().unwrap()
    );
    fs::write(ws.root.join("apps/hello.app"), probe).unwrap();
    let ws = Workspace::open(&ws.root).unwrap();
    let plan = ws.launch_plan("/home", "hello", &[], true).unwrap();
    let mut command = sandbox::command(&ws, &plan).unwrap();
    command.env("GLYPH_TEST_SECRET", "must not reach application");
    let output = command
        .output()
        .expect("Bubblewrap must be installed for the integration test");
    assert!(
        output.status.success(),
        "sandbox failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(rw.join("writable")).unwrap(), "after");
    assert_eq!(ws.read("/home/result").unwrap(), "isolated");
}

#[test]
fn no_network_is_requested_by_default_and_failure_has_no_fallback() {
    let (_dir, ws) = fixture("");
    let plan = ws.launch_plan("/home", "hello", &[], false).unwrap();
    let command = sandbox::command(&ws, &plan).unwrap();
    assert!(command.get_args().any(|arg| arg == "--unshare-net"));
    assert_eq!(command.get_program(), Path::new("bwrap"));
}

#[test]
fn individual_file_links_work_and_write_through() {
    let (dir, ws) = fixture("");
    let host = dir.path().join("host-note");
    fs::write(&host, "before").unwrap();
    fs::write(
        ws.root.join("configs/links/file.lua"),
        format!(
            "glyph.link {{ source = {:?}, target = '/home/note', access = 'read-write' }}",
            host.to_str().unwrap()
        ),
    )
    .unwrap();
    let ws = Workspace::open(&ws.root).unwrap();
    assert_eq!(ws.read("/home/note").unwrap(), "before");
    ws.write("/home/note", "after").unwrap();
    assert_eq!(fs::read_to_string(host).unwrap(), "after");
    assert!(ws.read("/home/note/anything").is_err());
}

#[test]
fn opaque_app_files_accept_explicit_kinds_and_commands() {
    let (dir, ws) =
        fixture("glyph.handler { kind = 'special', command = {'/usr/bin/special', '{app}'} }");
    let source = dir.path().join("opaque.app");
    fs::write(&source, "not a recognized format").unwrap();
    ws.import_app(&source, "opaque", Some("special")).unwrap();
    fs::write(ws.root.join("apps/other.app"), "also opaque").unwrap();
    fs::write(
        ws.root.join("configs/apps/other.lua"),
        "glyph.app { path = '/apps/other.app', command = {'/usr/bin/other', '{app}', '{args}'} }",
    )
    .unwrap();
    let ws = Workspace::open(&ws.root).unwrap();
    assert_eq!(
        ws.launch_plan("/home", "opaque", &[], false).unwrap().kind,
        "special"
    );
    assert_eq!(
        ws.launch_plan("/home", "other", &[], false).unwrap().kind,
        "custom"
    );
}

#[test]
fn flatpak_host_adapter_requires_explicit_policy() {
    let (_dir, ws) = fixture("");
    fs::write(
        ws.root.join("apps/package.app"),
        "[Flatpak Ref]\nName=org.example.App\n",
    )
    .unwrap();
    assert!(ws.launch_plan("/home", "package", &[], false).is_err());
    fs::write(
        ws.root.join("configs/apps/package.lua"),
        "glyph.app { path = '/apps/package.app', kind = 'flatpakref', sandbox = false }",
    )
    .unwrap();
    let ws = Workspace::open(&ws.root).unwrap();
    let plan = ws.launch_plan("/home", "package", &[], false).unwrap();
    assert!(!plan.sandbox);
    assert!(plan.argv[1].ends_with("/configs/handlers/flatpak.py"));
    assert!(ws.launch_plan("/home", "package", &[], true).is_err());
}

#[cfg(unix)]
#[test]
fn startup_does_not_follow_dangling_seed_symlinks() {
    use std::os::unix::fs::symlink;
    let (dir, ws) = fixture("");
    let outside = dir.path().join("must-not-be-created");
    fs::remove_file(ws.root.join("home/welcome.txt")).unwrap();
    symlink(&outside, ws.root.join("home/welcome.txt")).unwrap();
    assert!(Workspace::open(&ws.root).is_err());
    assert!(!outside.exists());
}
