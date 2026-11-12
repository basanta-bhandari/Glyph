use crate::workspace::Workspace;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, process::Command};
const INDEX: &str = "https://raw.githubusercontent.com/basanta-bhandari/walls/main/";
fn fetch(path: &str) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .args([
            "-q",
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--max-time",
            "45",
            "--max-filesize",
            "33554432",
        ])
        .arg(format!("{INDEX}{}", encode(path)))
        .output()?;
    ensure!(output.status.success(), "wallpaper download failed");
    Ok(output.stdout)
}
fn encode(path: &str) -> String {
    path.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~/".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn component(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value != "."
            && value != ".."
            && !value.contains(['/', '\\', '\0', '\n', '\r']),
        "use one category or filename, without path separators"
    );
    Ok(())
}
pub fn categories() -> Result<Vec<String>> {
    let text = String::from_utf8(fetch(".github/README.md")?)?;
    Ok(text
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .filter(|v| !["Showcase", "Sources", "Ending Note"].contains(v))
        .map(str::to_owned)
        .collect())
}
pub fn files(category: &str) -> Result<Vec<String>> {
    component(category)?;
    let text = String::from_utf8(fetch(&format!("{category}/README.md"))?)?;
    Ok(text
        .split("href=\"")
        .skip(1)
        .filter_map(|v| v.split('"').next())
        .filter(|v| !v.contains('/') && image_extension(v))
        .map(str::to_owned)
        .collect())
}
fn image_extension(name: &str) -> bool {
    ["png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff"].contains(
        &name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
    )
}
fn image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF8")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
        || bytes.starts_with(b"BM")
        || bytes.starts_with(b"II*\0")
        || bytes.starts_with(b"MM\0*")
}
#[derive(Serialize, Deserialize)]
struct Wallpaper {
    path: String,
}
pub fn saved(root: &Path) -> Option<String> {
    serde_json::from_slice::<Wallpaper>(&fs::read(root.join("configs/wallpaper.json")).ok()?)
        .ok()
        .map(|w| w.path)
}
fn remote(args: &[&str]) -> Result<bool> {
    let Some(socket) = std::env::var_os("GLYPH_KITTY_SOCKET") else {
        return Ok(false);
    };
    let status = Command::new("kitten")
        .arg("@")
        .arg("--to")
        .arg(socket)
        .args(args)
        .status()?;
    ensure!(
        status.success(),
        "Glyph's Kitty instance rejected the appearance change"
    );
    Ok(true)
}
pub fn wallpaper(workspace: &Workspace, args: &[String]) -> Result<String> {
    match args.first().map(String::as_str).unwrap_or("status") {
        "categories" => Ok(categories()?.join("\n")),
        "list" => Ok(files(args.get(1).context("wallpaper list CATEGORY")?)?.join("\n")),
        "fetch" => {
            let category = args.get(1).context("wallpaper fetch CATEGORY FILE")?;
            let name = args.get(2).context("wallpaper fetch CATEGORY FILE")?;
            component(category)?;
            component(name)?;
            ensure!(
                files(category)?.contains(name),
                "file is not in the wallpaper category index"
            );
            let bytes = fetch(&format!("{category}/{name}"))?;
            ensure!(image(&bytes), "download is not a supported image");
            let logical = format!("/home/.local/share/wallpapers/{category}-{name}");
            let path = workspace.resolve(&logical, true)?;
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(path, bytes)?;
            Ok(format!(
                "Downloaded {logical}. Use wallpaper set '{logical}' to apply it."
            ))
        }
        "set" => {
            let logical = crate::workspace::normalize(
                "/home",
                args.get(1).context("wallpaper set /home/IMAGE")?,
            )?;
            ensure!(
                logical.starts_with("/home/"),
                "wallpapers must be stored in Glyph's /home"
            );
            let path = workspace.resolve(&logical, false)?;
            ensure!(image(&fs::read(&path)?), "not a supported image");
            fs::write(
                workspace.root.join("configs/wallpaper.json"),
                serde_json::to_vec(&Wallpaper { path: logical })?,
            )?;
            let live = remote(&[
                "set-background-image",
                "--all",
                path.to_str().context("non-UTF8 image path")?,
            ])?;
            Ok(if live {
                "Wallpaper applied to Glyph's Kitty instance."
            } else {
                "Wallpaper saved. It will appear when launched through start.sh / --kitty."
            }
            .into())
        }
        "clear" => {
            let path = workspace.root.join("configs/wallpaper.json");
            if path.exists() {
                fs::remove_file(path)?;
            }
            remote(&["set-background-image", "--all", "none"])?;
            Ok("Glyph wallpaper cleared.".into())
        }
        "status" => Ok(saved(&workspace.root)
            .unwrap_or_else(|| "No Glyph wallpaper selected. Try wallpaper categories.".into())),
        _ => anyhow::bail!(
            "wallpaper categories | list CATEGORY | fetch CATEGORY FILE | set /home/IMAGE | clear"
        ),
    }
}
pub fn apply_colors(workspace: &Workspace) -> Result<()> {
    let t = workspace.config.theme();
    remote(&[
        "set-colors",
        "--all",
        &format!("background={}", t.background),
        &format!("foreground={}", t.foreground),
        &format!("cursor={}", t.accent),
    ])?;
    Ok(())
}
