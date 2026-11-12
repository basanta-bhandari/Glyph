use anyhow::{Context, Result, bail, ensure};
use glyph_workspace::{
    shell::Shell, storage::StorageSession, storage_config::StorageSettings, ui,
    workspace::Workspace,
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("Glyph: {error:#}");
            let message = format!("{error:#}");
            std::process::exit(
                if message.contains("passphrase")
                    || message.contains("authentication failed")
                    || message.contains("incorrect key")
                {
                    77
                } else {
                    1
                },
            );
        }
    }
}

fn run() -> Result<i32> {
    let mut args = std::env::args().skip(1);
    let mut root = None;
    let mut storage_config = None;
    let mut plain = false;
    let mut emergency = false;
    let mut kitty = false;
    let mut minimal = false;
    let mut attached = None;
    let mut agent = std::env::var_os("GLYPH_SANDBOX").is_some();
    let mut command = None;
    let mut import = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = Some(PathBuf::from(
                    args.next()
                        .context("--root requires a host storage directory")?,
                ))
            }
            "--plain" => plain = true,
            "--emergency" => emergency = true,
            "--kitty" => kitty = true,
            "--no-kitty" => kitty = false,
            "--minimal" => minimal = true,
            "--attach" => {
                attached = Some(PathBuf::from(
                    args.next().context("--attach requires a descriptor")?,
                ))
            }
            "--health-check" => {
                println!(
                    "{{\"glyph\":\"{}\",\"status\":\"ok\",\"disk_format\":1}}",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(0);
            }
            "--storage-config" => {
                storage_config = Some(PathBuf::from(
                    args.next().context("--storage-config requires FILE")?,
                ))
            }
            "--agent" => agent = true,
            "--command" | "-c" => {
                command = Some(
                    args.next()
                        .context("--command requires one quoted Glyph command")?,
                )
            }
            "--version" | "-V" => {
                println!("Glyph {}", env!("CARGO_PKG_VERSION"));
                return Ok(0);
            }
            "--help" | "-h" => {
                println!(
                    "Glyph — a terminal workspace\n\nUsage: glyph [--root DIR] [--plain] [--agent] [--command 'COMMAND']\n       glyph [--root DIR] import HOST_FILE NAME [KIND]\n\nDefault storage: OS app-data directory, in mounted, compressed workspace.glyphdisk.\n--root selects a portable storage directory. Old Glyph roots are converted.\nUse storage to inspect paths; key set enables encrypted storage.\n--kitty opens a dedicated Kitty instance with Glyph's copied configuration.\n--emergency opens the last safe disk snapshot read-only.\nThe default interface is a TUI. Piped input uses plain mode.\n--agent disallows host execution; it does not itself create a sandbox.\nApplications use a real Linux/Bubblewrap sandbox by default."
                );
                println!(
                    "\n--storage-config FILE selects a host-side Lua storage profile.\nProfiles configure backend, data_dir, session_dir, snapshots, and compression_level.\nThe default profile is in the OS config directory; --root uses DIR/storage.lua."
                );
                return Ok(0);
            }
            "import" => {
                import = Some((
                    PathBuf::from(args.next().context("import requires HOST_FILE")?),
                    args.next().context("import requires NAME")?,
                    args.next(),
                ));
                ensure!(args.next().is_none(), "too many import arguments");
                break;
            }
            _ => bail!("unknown argument {arg}; use --help"),
        }
    }
    if let Some(path) = attached {
        let storage = glyph_workspace::session::attach(&path)?;
        let recovered = storage.emergency();
        let workspace = if recovered {
            Workspace::emergency(storage.root())?
        } else {
            Workspace::open_existing(storage.root())?
        };
        let mut shell = Shell::new(workspace, agent);
        shell.emergency = recovered;
        shell.storage = Some(storage);
        ui::run(&mut shell)?;
        return Ok(0);
    }
    let settings = StorageSettings::load(root.as_deref(), storage_config.as_deref())?;
    let unlock = || {
        ensure!(
            io::stdin().is_terminal(),
            "encrypted storage requires an interactive terminal to unlock"
        );
        Ok(zeroize::Zeroizing::new(rpassword::prompt_password(
            "Glyph encryption passphrase: ",
        )?))
    };
    let mut storage = if emergency && settings.backend == "mounted" {
        StorageSession::open_mounted(settings, true, unlock)?
    } else {
        StorageSession::configured(settings, unlock)?
    };
    let recovered = storage.emergency();
    let mut workspace = if recovered {
        Workspace::emergency(storage.root())?
    } else {
        Workspace::open(storage.root())?
    };
    if !recovered && !minimal {
        if !storage.root().join("system/source/Cargo.toml").exists() {
            eprintln!(
                "Preparing Glyph: editable source, guides, applications, and independent rice copies. First setup can take a moment."
            );
        }
        glyph_workspace::system::bootstrap(storage.root())?;
        workspace = Workspace::open_existing(storage.root())?;
    }
    storage.mark_boot_validated()?;
    if kitty
        && !plain
        && command.is_none()
        && import.is_none()
        && (std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some())
    {
        let code =
            glyph_workspace::session::kitty(&mut storage, &workspace, &std::env::current_exe()?)?;
        storage.finish()?;
        return Ok(code);
    }
    if let Some((source, name, kind)) = import {
        ensure!(
            !agent,
            "agents cannot import host applications or modify trusted configuration"
        );
        let result = workspace.import_app(&source, &name, kind.as_deref());
        storage.finish()?;
        println!("Imported {}", result?);
        return Ok(0);
    }
    let mut shell = Shell::new(workspace, agent);
    shell.emergency = recovered;
    shell.storage = Some(storage);
    let scripted = command.is_some();
    let result = (|| -> Result<()> {
        if let Some(command) = command {
            let effects = shell.execute(&command)?;
            ui::process_effects(&mut shell, effects, false)?;
        } else if plain || !io::stdin().is_terminal() {
            ui::plain(&mut shell)?;
        } else {
            ui::run(&mut shell)?;
        }
        Ok(())
    })();
    shell.storage.as_mut().unwrap().finish()?;
    result?;
    Ok(if scripted { shell.last_exit } else { 0 })
}
