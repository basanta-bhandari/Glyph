use anyhow::{Context, Result};
use glyph_workspace::{storage_config::StorageSettings, system};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
fn main() {
    if let Err(e) = run() {
        eprintln!("Glyph launcher: {e:#}");
        std::process::exit(1);
    }
}
fn option(args: &[String], name: &str) -> Option<PathBuf> {
    args.iter()
        .position(|v| v == name)
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
}
fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let here = std::env::current_exe()?
        .parent()
        .context("launcher has no directory")?
        .to_owned();
    let bundled = here.join("glyph");
    if args
        .iter()
        .any(|a| ["--version", "-V", "--help", "-h", "--health-check"].contains(&a.as_str()))
    {
        let s = Command::new(&bundled).args(&args).status()?;
        std::process::exit(s.code().unwrap_or(1));
    }
    let settings = StorageSettings::load(
        option(&args, "--root").as_deref(),
        option(&args, "--storage-config").as_deref(),
    )?;
    let manifest = system::read_manifest(&settings.paths.data)?;
    let selected = manifest
        .active
        .as_ref()
        .filter(|id| id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|id| settings.paths.data.join("runtimes").join(id).join("glyph"))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| bundled.clone());
    let status = launch(&selected, &bundled, &args)?;
    if status.success()
        || status.code() == Some(77)
        || args
            .iter()
            .any(|a| ["--command", "-c", "import", "--emergency"].contains(&a.as_str()))
    {
        std::process::exit(status.code().unwrap_or(1));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() == Some(libc::SIGINT) {
            std::process::exit(130);
        }
    }
    eprintln!(
        "Glyph did not start cleanly. Booting the bundled runtime in emergency mode. Your unfinished session is preserved."
    );
    if selected != bundled {
        system::rollback_runtime(&settings.paths.data)?;
    }
    let mut recovery = args;
    recovery.push("--emergency".into());
    let status = launch(&bundled, &bundled, &recovery)?;
    std::process::exit(status.code().unwrap_or(1));
}
fn launch(path: &Path, bundled: &Path, args: &[String]) -> Result<std::process::ExitStatus> {
    Ok(Command::new(path)
        .args(args)
        .env("GLYPH_BUNDLED_RUNTIME", bundled)
        .status()?)
}
