use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const APPS: &[&str] = &[
    "kitty",
    "fish",
    "fastfetch",
    "neofetch",
    "nvim",
    "vim",
    "nano",
    "vis",
    "emacs",
    "impala",
    "bluetui",
];
#[derive(Default, Serialize, Deserialize)]
pub struct RiceReport {
    pub copied: BTreeMap<String, usize>,
    pub skipped: Vec<String>,
}

pub fn copy_once(root: &Path, host_config: &Path) -> Result<RiceReport> {
    let marker = root.join("system/rice-import.json");
    let mut report = if marker.is_file() {
        serde_json::from_slice(&fs::read(&marker)?)?
    } else {
        RiceReport::default()
    };
    for app in APPS {
        if !report.copied.contains_key(*app) {
            let count = copy_app(root, host_config, app, &mut report.skipped)?;
            report.copied.insert((*app).into(), count);
        }
    }
    fs::write(marker, serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}
pub fn copy_app(
    root: &Path,
    host_config: &Path,
    app: &str,
    skipped: &mut Vec<String>,
) -> Result<usize> {
    ensure!(
        !app.is_empty()
            && app != "."
            && app != ".."
            && app
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "rice app must be one configuration directory name"
    );
    let source = host_config.join(app);
    if !source.is_dir() {
        return Ok(0);
    }
    let source = source.canonicalize()?;
    let destination = root.join("configs/user").join(app);
    let mut count = 0;
    let mut bytes = 0;
    copy(
        &source,
        &source,
        &destination,
        &mut count,
        &mut bytes,
        skipped,
    )?;
    Ok(count)
}
fn copy(
    base: &Path,
    source: &Path,
    target: &Path,
    count: &mut usize,
    bytes: &mut u64,
    skipped: &mut Vec<String>,
) -> Result<()> {
    let filename = source.file_name().unwrap_or_default().to_string_lossy();
    if [
        ".git",
        ".cache",
        "history",
        "fish_history",
        "known_hosts",
        "auth.json",
        "credentials.json",
        "tokens.json",
        "lazy-lock.json.tmp",
    ]
    .contains(&filename.as_ref())
    {
        return Ok(());
    }
    let meta = fs::symlink_metadata(source)?;
    if meta.file_type().is_symlink() {
        let resolved = source.canonicalize()?;
        if !resolved.starts_with(base) || !resolved.is_file() {
            skipped.push(source.to_string_lossy().into());
            return Ok(());
        }
        return copy(base, &resolved, target, count, bytes, skipped);
    }
    if meta.is_dir() {
        fs::create_dir_all(target)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy(
                base,
                &entry.path(),
                &target.join(entry.file_name()),
                count,
                bytes,
                skipped,
            )?;
        }
        return Ok(());
    }
    if !meta.is_file() || target.exists() {
        return Ok(());
    }
    ensure!(
        *count < 10000 && *bytes + meta.len() < 256 * 1024 * 1024,
        "rice copy is larger than the 256 MiB/10,000-file import limit"
    );
    if filename == "fish_variables" {
        let original = fs::read_to_string(source)?;
        let colors = original
            .lines()
            .filter(|line| line.contains("fish_color_") || line.contains("fish_pager_color_"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(
            target,
            format!(
                "# Copied color variables; private/unrelated universal variables were omitted.\n{colors}\n"
            ),
        )?;
    } else {
        fs::copy(source, target).with_context(|| format!("copying rice {}", source.display()))?;
    }
    *count += 1;
    *bytes += meta.len();
    Ok(())
}
pub fn host_config() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rice_is_a_copy_and_never_overwrites_diverged_configuration() {
        let t = tempfile::tempdir().unwrap();
        let host = t.path().join("host");
        let glyph = t.path().join("glyph");
        fs::create_dir_all(host.join("kitty")).unwrap();
        fs::create_dir_all(glyph.join("configs/user")).unwrap();
        fs::create_dir_all(glyph.join("system")).unwrap();
        fs::write(host.join("kitty/kitty.conf"), "font_size 12").unwrap();
        copy_once(&glyph, &host).unwrap();
        fs::write(glyph.join("configs/user/kitty/kitty.conf"), "font_size 18").unwrap();
        fs::write(host.join("kitty/kitty.conf"), "font_size 9").unwrap();
        copy_once(&glyph, &host).unwrap();
        assert_eq!(
            fs::read_to_string(glyph.join("configs/user/kitty/kitty.conf")).unwrap(),
            "font_size 18"
        );
        assert_eq!(
            fs::read_to_string(host.join("kitty/kitty.conf")).unwrap(),
            "font_size 9"
        );
    }
}
