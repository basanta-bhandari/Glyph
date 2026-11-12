use glyph_workspace::{
    shell::{Effect, Shell},
    workspace::Workspace,
};
use std::fs;
use tempfile::TempDir;

fn fixture(configuration: &str) -> (TempDir, Workspace) {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("glyph");
    Workspace::open(&root).unwrap();
    fs::write(root.join("configs/packages.lua"), configuration).unwrap();
    let workspace = Workspace::open(&root).unwrap();
    (dir, workspace)
}

const MANAGER: &str = r#"glyph.package_manager {
    name = 'test',
    install = 'tool install "{[pkg}]"',
    remove = {'tool', 'remove', '{[pkg}]'},
    upgrade = 'tool upgrade "{[pkg}]"',
}"#;

#[test]
fn install_remove_upgrade_use_configured_templates() {
    let (_dir, workspace) = fixture(MANAGER);
    for action in ["install", "remove", "upgrade"] {
        let plan = workspace
            .package_plan("/home", action, &["my.package".into()], false)
            .unwrap();
        assert_eq!(plan.argv, ["tool", action, "my.package"]);
        assert!(plan.sandbox);
    }
    let plan = workspace
        .package_plan("/home", "uninstall", &["my.package".into()], false)
        .unwrap();
    assert_eq!(plan.argv, ["tool", "remove", "my.package"]);
}

#[test]
fn package_arguments_do_not_expand_shell_syntax_or_become_options() {
    let (_dir, workspace) = fixture(MANAGER);
    let package = "two words; $(touch /SHOULD_NOT_EXIST)".to_owned();
    let plan = workspace
        .package_plan("/home", "install", std::slice::from_ref(&package), false)
        .unwrap();
    assert_eq!(plan.argv, ["tool".to_owned(), "install".into(), package]);
    for args in [
        vec![],
        vec!["-y".into()],
        vec!["".into()],
        vec!["one".into(), "two".into()],
        vec!["new\nline".into()],
    ] {
        assert!(
            workspace
                .package_plan("/home", "install", &args, false)
                .is_err()
        );
    }
}

#[test]
fn shell_commands_and_preview_do_not_require_app_files() {
    let (_dir, workspace) = fixture(MANAGER);
    let mut shell = Shell::new(workspace, false);
    assert!(
        matches!(&shell.execute("install sample").unwrap()[0], Effect::Package(operation,args) if operation == "install" && args == &["sample"])
    );
    assert!(
        matches!(&shell.execute("remove sample").unwrap()[0], Effect::Package(operation,args) if operation == "remove" && args == &["sample"])
    );
    assert!(
        matches!(&shell.execute("plan upgrade sample").unwrap()[0], Effect::Output(text) if text.contains("sample") && text.contains("package-manager"))
    );
    assert!(
        matches!(&shell.execute("packages").unwrap()[0], Effect::Output(text) if text.contains("test"))
    );
}

#[test]
fn host_package_managers_keep_existing_approval_and_agent_boundary() {
    let (_dir, workspace) =
        fixture(&MANAGER.replace("name = 'test',", "name = 'test', sandbox = false,"));
    assert!(
        workspace
            .package_plan("/home", "install", &["sample".into()], true)
            .is_err()
    );
    let plan = workspace
        .package_plan("/home", "install", &["sample".into()], false)
        .unwrap();
    let mut shell = Shell::new(workspace, false);
    assert!(
        glyph_workspace::ui::process_effects(&mut shell, vec![Effect::Launch(plan)], false)
            .is_err()
    );
}

#[test]
fn package_templates_are_validated_before_execution() {
    let (dir, workspace) = fixture(MANAGER);
    for invalid in [
        MANAGER.replace("tool install \"{[pkg}]\"", "tool install"),
        MANAGER.replace("tool install \"{[pkg}]\"", "tool install --name={[pkg}]"),
        MANAGER.replace("tool install \"{[pkg}]\"", "{[pkg}] install"),
        format!("{MANAGER}\n{MANAGER}"),
    ] {
        fs::write(workspace.root.join("configs/packages.lua"), invalid).unwrap();
        assert!(Workspace::open(&dir.path().join("glyph")).is_err());
    }
}

#[test]
fn bundled_examples_parse_with_their_expected_commands() {
    let examples = include_str!("../assets/packages.lua");
    let mut active = false;
    let mut block = String::new();
    let mut names = Vec::new();
    for line in examples.lines() {
        if line == "-- glyph.package_manager {" {
            active = true;
            block.clear();
        }
        if active {
            block.push_str(line.strip_prefix("-- ").unwrap_or(line));
            block.push('\n');
        }
        if active && line == "-- }" {
            let (_dir, workspace) = fixture(&block);
            let manager = workspace.config.package_manager().unwrap();
            names.push(manager.name.clone());
            for action in ["install", "remove", "upgrade"] {
                let plan = workspace
                    .package_plan("/home", action, &["example.package".into()], false)
                    .unwrap();
                assert!(plan.argv.iter().any(|word| word == "example.package"));
            }
            active = false;
        }
    }
    assert_eq!(names, ["pacman", "apt", "brew", "winget", "custom"]);
}

#[cfg(target_os = "linux")]
#[test]
fn configured_package_command_really_runs_inside_glyph() {
    let (_dir, workspace) = fixture(
        r#"glyph.package_manager {
        name = 'mock', network = true,
        install = {'/usr/bin/python3', '-c', "import json,sys; open('/home/package-args.json','w').write(json.dumps(sys.argv[1:])); sys.exit(7)", '{[pkg}]'},
        remove = {'mock', 'remove', '{[pkg}]'},
        upgrade = {'mock', 'upgrade', '{[pkg}]'},
    }"#,
    );
    let package = "literal; $(not executed)".to_owned();
    let plan = workspace
        .package_plan("/home", "install", std::slice::from_ref(&package), true)
        .unwrap();
    let mut shell = Shell::new(workspace, true);
    glyph_workspace::ui::process_effects(&mut shell, vec![Effect::Launch(plan)], false).unwrap();
    assert_eq!(shell.last_exit, 7);
    let arguments: Vec<String> =
        serde_json::from_str(&shell.workspace.read("/home/package-args.json").unwrap()).unwrap();
    assert_eq!(arguments, [package]);
}
