use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Serialize, Deserialize)]
pub struct Receipt {
    pub name: String,
    pub version: String,
    pub command: String,
    pub sha256: String,
    pub origin: String,
}
pub fn run() -> Result<()> {
    ensure!(
        std::env::var("GLYPH_SANDBOX").as_deref() == Ok("1"),
        "this package manager installs only inside a Glyph sandbox"
    );
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 2,
        "package-manager install|remove|upgrade PACKAGE"
    );
    let package = &args[1];
    ensure!(
        ["opencode", "codex"].contains(&package.as_str()),
        "no built-in download recipe for {package}. Configure another Glyph-local backend in /configs/packages.lua, or import an application explicitly."
    );
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME missing")?);
    ensure!(
        home == Path::new("/home"),
        "Glyph package prefix must be /home"
    );
    let binary = home.join(".local/bin").join(package);
    let receipts = home.join(".local/share/glyph/packages");
    fs::create_dir_all(&receipts)?;
    let receipt = receipts.join(format!("{package}.json"));
    if args[0] == "remove" {
        if binary.exists() {
            fs::remove_file(&binary)?;
        }
        if receipt.exists() {
            fs::remove_file(receipt)?;
        }
        println!("Removed {package} from Glyph's user prefix.");
        return Ok(());
    }
    ensure!(
        ["install", "upgrade"].contains(&args[0].as_str()),
        "unknown package operation"
    );
    let (repo, candidates) = match package.as_str() {
        "codex" => (
            "openai/codex",
            vec!["codex-x86_64-unknown-linux-musl.tar.gz"],
        ),
        _ => (
            "anomalyco/opencode",
            vec![
                "opencode-linux-x64-baseline.tar.gz",
                "opencode-linux-x64.tar.gz",
            ],
        ),
    };
    ensure!(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "built-in binary recipes currently target Linux x86-64"
    );
    let metadata = fetch(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))?;
    let release: Value = serde_json::from_slice(&metadata)?;
    let asset = release["assets"]
        .as_array()
        .context("release has no assets")?
        .iter()
        .find(|a| candidates.contains(&a["name"].as_str().unwrap_or_default()))
        .context("no compatible release asset")?;
    let url = asset["browser_download_url"]
        .as_str()
        .context("asset has no URL")?;
    ensure!(
        url.starts_with(&format!("https://github.com/{repo}/releases/download/")),
        "unexpected release asset host/path"
    );
    let temporary = tempfile::tempdir()?;
    let archive = temporary.path().join("package.tar.gz");
    let status = Command::new("/usr/bin/curl")
        .args([
            "-q",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--max-filesize",
            "536870912",
            "--output",
        ])
        .arg(&archive)
        .arg(url)
        .status()?;
    ensure!(status.success(), "package download failed");
    let mut hash = Sha256::new();
    let mut input = fs::File::open(&archive)?;
    let mut buffer = [0; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let digest = format!("{:x}", hash.finalize());
    if let Some(expected) = asset["digest"].as_str() {
        if let Some(expected) = expected.strip_prefix("sha256:") {
            ensure!(expected == digest, "release checksum mismatch");
        }
    }
    let listing = Command::new("/usr/bin/tar")
        .args(["-tzf"])
        .arg(&archive)
        .output()?;
    ensure!(listing.status.success(), "invalid package archive");
    let member = String::from_utf8(listing.stdout)?
        .lines()
        .find(|name| {
            !name.starts_with('/')
                && !name.split('/').any(|p| p == "..")
                && (name.rsplit('/').next() == Some(package.as_str())
                    || (package == "codex"
                        && name
                            .rsplit('/')
                            .next()
                            .is_some_and(|p| p.starts_with("codex-") && !p.ends_with('/'))))
        })
        .context("package executable not found")?
        .to_owned();
    fs::create_dir_all(binary.parent().unwrap())?;
    let mut output = tempfile::NamedTempFile::new_in(binary.parent().unwrap())?;
    let extracted = Command::new("/usr/bin/tar")
        .args(["-xOzf"])
        .arg(&archive)
        .arg("--")
        .arg(&member)
        .stdout(output.as_file().try_clone()?)
        .status()?;
    ensure!(extracted.success(), "package extraction failed");
    let mut header = [0; 4];
    fs::File::open(output.path())?.read_exact(&mut header)?;
    ensure!(
        &header == b"\x7fELF",
        "package member is not a native executable"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        output
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    output.flush()?;
    output.persist(&binary).map_err(|e| e.error)?;
    let value = Receipt {
        name: package.clone(),
        version: release["tag_name"].as_str().unwrap_or("latest").into(),
        command: binary.to_string_lossy().into(),
        sha256: digest,
        origin: repo.into(),
    };
    fs::write(receipt, serde_json::to_vec_pretty(&value)?)?;
    println!(
        "Installed {package} {} inside Glyph. No host package manager was invoked.",
        value.version
    );
    Ok(())
}
fn fetch(url: &str) -> Result<Vec<u8>> {
    let out = Command::new("/usr/bin/curl")
        .args([
            "-q",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--max-filesize",
            "8388608",
            url,
        ])
        .output()?;
    ensure!(out.status.success(), "release metadata fetch failed");
    Ok(out.stdout)
}

pub fn register(workspace: &crate::workspace::Workspace, name: &str, agent: bool) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "invalid installed package name"
    );
    let program = format!("/home/.local/bin/{name}");
    ensure!(
        workspace.resolve(&program, false)?.is_file(),
        "installed executable is missing: {program}"
    );
    let wrapper = workspace.root.join("apps").join(format!("{name}.app"));
    if !wrapper.exists() {
        fs::write(&wrapper, format!("#!/bin/sh\nexec {program} \"$@\"\n"))?;
    }
    let config = workspace
        .root
        .join("configs/apps")
        .join(format!("{name}.lua"));
    if !config.exists() {
        fs::write(
            config,
            format!("glyph.app {{ path = '/apps/{name}.app', network = true }}\n"),
        )?;
    }
    if agent {
        let config = workspace
            .root
            .join("configs/agents")
            .join(format!("{name}.lua"));
        if !config.exists() {
            fs::write(
                config,
                format!("glyph.agent {{ name = '{name}', app = '/apps/{name}.app' }}\n"),
            )?;
        }
    }
    Ok(())
}

/// Remove only registration files generated by Glyph; preserve user edits.
pub fn unregister(workspace: &crate::workspace::Workspace, name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "invalid package name"
    );
    let generated = [
        (
            format!("apps/{name}.app"),
            format!("#!/bin/sh\nexec /home/.local/bin/{name} \"$@\"\n"),
        ),
        (
            format!("configs/apps/{name}.lua"),
            format!("glyph.app {{ path = '/apps/{name}.app', network = true }}\n"),
        ),
        (
            format!("configs/agents/{name}.lua"),
            format!("glyph.agent {{ name = '{name}', app = '/apps/{name}.app' }}\n"),
        ),
    ];
    for (path, expected) in generated {
        let path = workspace.root.join(path);
        if fs::read_to_string(&path).ok().as_deref() == Some(&expected) {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}
