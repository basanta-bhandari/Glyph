use crate::{
    app::Launch,
    workspace::{Mount, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
include!(concat!(env!("OUT_DIR"), "/source_bundle.rs"));

pub fn seed_source(root: &Path) -> Result<()> {
    if root.join("system/source/Cargo.toml").exists() {
        return Ok(());
    }
    for (path, bytes) in SOURCE_FILES {
        let destination = root.join("system/source").join(path);
        fs::create_dir_all(destination.parent().unwrap())?;
        fs::write(destination, bytes)?;
    }
    fs::write(
        root.join("system/source/README-GLYPH.md"),
        "This is your Glyph source. Edit it, run source build, inspect the result, and source activate. The old runtime remains available for recovery.\n",
    )?;
    Ok(())
}

pub fn bootstrap(root: &Path) -> Result<()> {
    seed_source(root)?;
    for (path, bytes) in SOURCE_FILES {
        if let Some(name) = path.strip_prefix("assets/human/") {
            let dest = root.join("system/docs").join(name);
            if !dest.exists() {
                fs::create_dir_all(dest.parent().unwrap())?;
                fs::write(dest, bytes)?;
            }
        }
        if let Some(name) = path.strip_prefix("assets/nvchad-starter/") {
            let dest = root.join("configs/editor/nvim").join(name);
            if !dest.exists() {
                fs::create_dir_all(dest.parent().unwrap())?;
                fs::write(dest, bytes)?;
            }
        }
    }
    if !root.join("apps/guide.app").exists() {
        fs::write(
            root.join("apps/guide.app"),
            include_str!("../assets/guide.app"),
        )?;
    }
    if !root.join("configs/catalog.tsv").exists() {
        fs::write(
            root.join("configs/catalog.tsv"),
            include_str!("../assets/tui-catalog.tsv"),
        )?;
    }
    if !root.join("configs/capabilities.tsv").exists() {
        fs::write(
            root.join("configs/capabilities.tsv"),
            include_str!("../assets/capabilities.tsv"),
        )?;
    }
    if let Some(host) = crate::rice::host_config() {
        crate::rice::copy_once(root, &host)?;
    }
    let kitty = root.join("configs/user/kitty/kitty.conf");
    if !kitty.exists() {
        fs::create_dir_all(kitty.parent().unwrap())?;
        fs::write(
            kitty,
            "font_family monospace\nfont_size 12\nbackground #181818\nforeground #e8d8b8\nwindow_padding_width 10\n",
        )?;
    }
    for (name, host) in [
        ("nmtui", true),
        ("impala", true),
        ("bluetui", true),
        ("fish", false),
        ("fastfetch", false),
        ("neofetch", false),
        ("nvim", false),
        ("nano", false),
        ("vim", false),
        ("btop", false),
        ("yazi", false),
    ] {
        let app = root.join("apps").join(format!("{name}.app"));
        if app.exists() {
            continue;
        }
        let Some(program) = find_program(name) else {
            continue;
        };
        if host || !program.starts_with("/usr/") {
            fs::copy(&program, &app)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&app, fs::Permissions::from_mode(0o755))?;
            }
        } else {
            fs::write(
                &app,
                format!("#!/bin/sh\nexec {} \"$@\"\n", program.display()),
            )?;
        }
        fs::write(
            root.join("configs/apps").join(format!("{name}.lua")),
            format!(
                "glyph.app {{ path = '/apps/{name}.app', sandbox = {}, network = {} }}\n",
                !host, host
            ),
        )?;
    }
    let dist = std::env::current_exe()?
        .parent()
        .context("missing runtime directory")?
        .to_path_buf();
    for name in ["browser", "package-manager"] {
        let source = dist.join(format!("glyph-{name}"));
        let dest = root.join("apps").join(format!("{name}.app"));
        if source.is_file() && !dest.exists() {
            fs::copy(source, &dest)?;
            fs::write(
                root.join("configs/apps").join(format!("{name}.lua")),
                format!(
                    "glyph.app {{ path = '/apps/{name}.app', network = true{} }}\n",
                    if name == "browser" {
                        ", env = { GLYPH_BROWSER_ENGINE_SANDBOX = 'off' }"
                    } else {
                        ""
                    }
                ),
            )?;
        }
    }
    let tools = dist.join("tools");
    let kat = tools.join("kat.app");
    if kat.is_file() && !root.join("apps/kat.app").exists() {
        fs::copy(kat, root.join("apps/kat.app"))?;
    }
    for name in ["pytris", "terminal-spiel"] {
        let source = tools.join(format!("{name}.app"));
        let target = root.join("apps").join(format!("{name}.app"));
        if source.is_file() && !target.exists() {
            fs::copy(source, &target)?;
        }
    }
    Ok(())
}
pub fn find_program(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
}
pub fn editor_plan(workspace: &Workspace, cwd: &str, input: &str, agent: bool) -> Result<Launch> {
    let path = crate::workspace::normalize(cwd, input)?;
    let privileged = path.starts_with("/configs/") || path.starts_with("/system/source/");
    ensure!(
        !agent || !privileged,
        "agents cannot edit running policy or system source"
    );
    ensure!(
        path.starts_with("/home/") || privileged,
        "editor writes are limited to /home, /configs, and /system/source"
    );
    ensure!(
        !["/home/AGENTS.md", "/AGENTS.md"].contains(&path.as_str()),
        "default agent guides remain protected"
    );
    let editor = workspace.config.editor();
    if editor.preset == "nvchad" {
        ensure!(
            workspace.root.join("configs/editor/nvim/init.lua").exists(),
            "NvChad is not set up. Run editor setup (it only installs into Glyph)."
        );
    }
    let argv = editor
        .command
        .iter()
        .map(|v| {
            if v == "{file}" {
                path.clone()
            } else {
                v.clone()
            }
        })
        .collect();
    let mut env = BTreeMap::new();
    env.insert("XDG_DATA_HOME".into(), "/home/.local/share".into());
    env.insert("XDG_STATE_HOME".into(), "/home/.local/state".into());
    if editor.preset == "nvchad" {
        env.insert("XDG_CONFIG_HOME".into(), "/configs/editor".into());
    }
    Ok(Launch {
        app: format!("editor: {path}"),
        kind: "editor".into(),
        argv,
        sandbox: true,
        network: editor.network,
        cwd: cwd.into(),
        env,
        runtime_mounts: Vec::new(),
        write_configs: path.starts_with("/configs/"),
        write_source: path.starts_with("/system/source/"),
    })
}
pub fn setup_editor(workspace: &Workspace) -> Result<String> {
    let target = workspace.root.join("configs/editor/nvim");
    if target.join("init.lua").exists() {
        return Ok("NvChad configuration already exists; it was preserved.".into());
    }
    ensure!(
        !target.exists(),
        "the NvChad target already exists; inspect it before setup"
    );
    let status = Command::new("git")
        .args(["clone", "--depth=1", "https://github.com/NvChad/starter"])
        .arg(&target)
        .status()?;
    ensure!(
        status.success(),
        "NvChad starter download failed; your host Neovim configuration was not touched"
    );
    if target.join(".git").exists() {
        fs::remove_dir_all(target.join(".git"))?;
    }
    Ok("NvChad starter installed in /configs/editor/nvim. Open edit <file>; its plugin manager installs dependencies into Glyph's home.".into())
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct RuntimeManifest {
    pub active: Option<String>,
    pub previous: Vec<String>,
}
pub fn runtime_manifest(data: &Path) -> PathBuf {
    data.join("runtimes/manifest.json")
}
pub fn read_manifest(data: &Path) -> Result<RuntimeManifest> {
    let path = runtime_manifest(data);
    Ok(if path.exists() {
        serde_json::from_slice(&fs::read(path)?)?
    } else {
        RuntimeManifest::default()
    })
}
fn write_manifest(data: &Path, value: &RuntimeManifest) -> Result<()> {
    let path = runtime_manifest(data);
    fs::create_dir_all(path.parent().unwrap())?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn build(workspace: &Workspace, cache: &Path) -> Result<String> {
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .context("Rust toolchain is required to rebuild Glyph")?;
    ensure!(sysroot.status.success(), "cannot find a Rust sysroot");
    let sysroot = PathBuf::from(String::from_utf8(sysroot.stdout)?.trim()).canonicalize()?;
    let cache = cache.join("build");
    fs::create_dir_all(&cache)?;
    let mut env = BTreeMap::from([
        ("CARGO_HOME".into(), "/build/cargo".into()),
        ("CARGO_TARGET_DIR".into(), "/build/target".into()),
        ("RUSTC".into(), "/toolchain/bin/rustc".into()),
        ("RUSTDOC".into(), "/toolchain/bin/rustdoc".into()),
        ("PATH".into(), "/toolchain/bin:/usr/bin:/bin".into()),
    ]);
    let mounts = vec![
        Mount {
            source: sysroot,
            target: "/toolchain".into(),
            writable: false,
        },
        Mount {
            source: cache.clone(),
            target: "/build".into(),
            writable: true,
        },
    ];
    for args in [
        vec!["test", "--locked", "--lib"],
        vec!["build", "--release", "--locked", "--bins"],
    ] {
        let plan = Launch {
            app: "Glyph source build".into(),
            kind: "source-build".into(),
            argv: std::iter::once("/toolchain/bin/cargo".into())
                .chain(args.into_iter().map(String::from))
                .collect(),
            sandbox: true,
            network: true,
            cwd: "/system/source".into(),
            env: env.clone(),
            runtime_mounts: mounts.clone(),
            write_configs: false,
            write_source: false,
        };
        let status = crate::sandbox::execute(workspace, &plan, false)?;
        ensure!(
            status.success(),
            "source build/check failed; active runtime unchanged"
        );
    }
    env.clear();
    let candidate = cache.join("target/release/glyph");
    ensure!(candidate.is_file(), "build produced no glyph executable");
    let directory = workspace.root.join("system/builds/candidate");
    fs::create_dir_all(&directory)?;
    let health_file = directory.join("health.json");
    if health_file.exists() {
        fs::remove_file(&health_file)?;
    }
    for name in ["glyph", "glyph-browser", "glyph-package-manager"] {
        let source = cache.join("target/release").join(name);
        if source.exists() {
            fs::copy(source, directory.join(name))?;
        }
    }
    let plan = Launch {
        app: "Candidate health check".into(),
        kind: "health-check".into(),
        argv: vec![
            "/system/builds/candidate/glyph".into(),
            "--health-check".into(),
        ],
        sandbox: true,
        network: false,
        cwd: "/home".into(),
        env: BTreeMap::new(),
        runtime_mounts: Vec::new(),
        write_configs: false,
        write_source: false,
    };
    let health = crate::sandbox::command(workspace, &plan)?.output()?;
    ensure!(
        health.status.success()
            && String::from_utf8_lossy(&health.stdout).contains("\"status\":\"ok\""),
        "candidate health check failed; active runtime unchanged"
    );
    let mut report: serde_json::Value = serde_json::from_slice(&health.stdout)?;
    report["runtime_sha256"] =
        format!("{:x}", Sha256::digest(fs::read(directory.join("glyph"))?)).into();
    fs::write(
        directory.join("health.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok("Build and library checks passed. Candidate is /system/builds/candidate/glyph. Use source activate when you want to boot it.".into())
}
pub fn activate(workspace: &Workspace, data: &Path) -> Result<String> {
    let candidate = workspace.root.join("system/builds/candidate/glyph");
    ensure!(candidate.is_file(), "run source build first");
    let bytes = fs::read(&candidate)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let health: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.root.join("system/builds/candidate/health.json"))
            .context("candidate has no successful health report; run source build")?,
    )?;
    ensure!(
        health["status"] == "ok" && health["runtime_sha256"].as_str() == Some(&digest),
        "candidate changed after its health check; run source build again"
    );
    let version = &digest[..16];
    let folder = data.join("runtimes").join(version);
    fs::create_dir_all(&folder)?;
    fs::write(folder.join("glyph"), bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(folder.join("glyph"), fs::Permissions::from_mode(0o700))?;
    }
    let browser = workspace.root.join("system/builds/candidate/glyph-browser");
    if browser.exists() {
        fs::copy(browser, folder.join("glyph-browser"))?;
    }
    let mut manifest = read_manifest(data)?;
    if let Some(active) = manifest.active.take() {
        if active != version {
            manifest.previous.insert(0, active);
        }
    }
    let discarded = manifest
        .previous
        .iter()
        .skip(3)
        .cloned()
        .collect::<Vec<_>>();
    manifest.previous.truncate(3);
    manifest.active = Some(version.into());
    write_manifest(data, &manifest)?;
    for id in discarded {
        if id != version && id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            let path = data.join("runtimes").join(id);
            if path.is_dir() {
                fs::remove_dir_all(path)?;
            }
        }
    }
    Ok(format!(
        "Activated build {version} for the next launch. Previous runtimes and disk snapshots remain available."
    ))
}
pub fn rollback_runtime(data: &Path) -> Result<String> {
    let mut manifest = read_manifest(data)?;
    manifest.active = if manifest.previous.is_empty() {
        None
    } else {
        Some(manifest.previous.remove(0))
    };
    write_manifest(data, &manifest)?;
    Ok("Previous runtime selected. Restart Glyph; recovery snapshots are unchanged.".into())
}
