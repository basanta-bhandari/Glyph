use crate::{app::Launch, workspace::Workspace};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::Path,
    process::{Command, ExitStatus},
};

pub fn command(workspace: &Workspace, plan: &Launch) -> Result<Command> {
    ensure!(
        plan.sandbox,
        "host-mode commands require separate user approval"
    );
    ensure!(
        cfg!(target_os = "linux"),
        "sandbox backend currently requires Linux and Bubblewrap; no host fallback is allowed"
    );
    let mut command = Command::new("bwrap");
    command.args([
        "--die-with-parent",
        "--new-session",
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--cap-drop",
        "ALL",
        "--clearenv",
    ]);
    if !plan.network {
        command.arg("--unshare-net");
    }
    // Runtime dependencies are exposed read-only, not the host root or home.
    command.args(["--ro-bind", "/usr", "/usr"]);
    for path in ["/bin", "/lib", "/lib64", "/sbin"] {
        if Path::new(path).is_symlink() {
            command.arg("--symlink").arg(fs::read_link(path)?).arg(path);
        } else if Path::new(path).is_dir() {
            command.args(["--ro-bind", path, path]);
        }
    }
    command.args([
        "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--dir", "/etc", "--dir", "/run",
    ]);
    for path in [
        "/etc/ssl/certs",
        "/etc/ssl/cert.pem",
        "/etc/ca-certificates",
        "/etc/pki",
        "/etc/ld.so.cache",
        "/etc/resolv.conf",
        "/etc/hosts",
        "/etc/nsswitch.conf",
    ] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    command
        .arg(if plan.write_configs {
            "--bind"
        } else {
            "--ro-bind"
        })
        .arg(workspace.root.join("configs"))
        .arg("/configs");
    command
        .arg("--ro-bind")
        .arg(workspace.root.join("apps"))
        .arg("/apps");
    command
        .arg("--bind")
        .arg(workspace.root.join("home"))
        .arg("/home");
    if workspace.root.join("system").exists() {
        command
            .arg("--ro-bind")
            .arg(workspace.root.join("system"))
            .arg("/system");
    }
    if plan.write_source {
        command
            .arg("--bind")
            .arg(workspace.root.join("system/source"))
            .arg("/system/source");
    }
    for mount in &plan.runtime_mounts {
        command
            .arg(if mount.writable {
                "--bind"
            } else {
                "--ro-bind"
            })
            .arg(&mount.source)
            .arg(&mount.target);
    }
    for path in ["/opt/google/chrome", "/etc/fonts"] {
        if Path::new(path).exists() {
            command.args(["--ro-bind", path, path]);
        }
    }
    command
        .arg("--ro-bind")
        .arg(workspace.root.join("AGENTS.md"))
        .arg("/AGENTS.md");
    command
        .arg("--ro-bind")
        .arg(workspace.root.join("home/AGENTS.md"))
        .arg("/home/AGENTS.md");
    for link in &workspace.links {
        // Validate the destination separately from the link's host source.
        let mut target = workspace.root.clone();
        for segment in link.target.trim_start_matches('/').split('/') {
            target.push(segment);
            if let Ok(meta) = fs::symlink_metadata(&target) {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "link target contains a symlink"
                );
            }
        }
        if Path::new(&link.source).is_dir() && !link.recursive {
            command.arg("--tmpfs").arg(&link.target);
        }
    }
    for mount in workspace.mounts()? {
        command
            .arg(if mount.writable {
                "--bind"
            } else {
                "--ro-bind"
            })
            .arg(mount.source)
            .arg(mount.target);
    }
    command.args([
        "--setenv",
        "HOME",
        "/home",
        "--setenv",
        "PATH",
        "/home/.local/bin:/usr/local/bin:/usr/bin:/bin",
        "--setenv",
        "XDG_CONFIG_HOME",
        "/configs/user",
        "--setenv",
        "XDG_CACHE_HOME",
        "/home/.cache",
        "--setenv",
        "GLYPH_SANDBOX",
        "1",
        "--setenv",
        "GLYPH_AGENT_GUIDE",
        "/AGENTS.md",
    ]);
    let term = std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into());
    for (key, value) in &plan.env {
        command.args(["--setenv", key, value]);
    }
    command.args([
        "--setenv", "TERM", &term, "--setenv", "LANG", "C.UTF-8", "--chdir", &plan.cwd, "--",
    ]);
    command.args(&plan.argv);
    Ok(command)
}

pub fn execute(workspace: &Workspace, plan: &Launch, host_approved: bool) -> Result<ExitStatus> {
    let mut command = if plan.sandbox {
        command(workspace, plan)?
    } else {
        ensure!(
            host_approved,
            "host access requires interactive user approval"
        );
        ensure!(
            std::env::var_os("GLYPH_SANDBOX").is_none(),
            "a sandboxed process cannot grant host access"
        );
        let mut cmd = Command::new(&plan.argv[0]);
        cmd.args(&plan.argv[1..]);
        cmd.current_dir(workspace.resolve(&plan.cwd, false)?);
        cmd.env("HOME", workspace.root.join("home"))
            .env("XDG_CONFIG_HOME", workspace.root.join("configs/user"))
            .env("XDG_CACHE_HOME", workspace.root.join("home/.cache"));
        for (key, value) in &plan.env {
            let value = if value.starts_with("/configs/") || value.starts_with("/home/") {
                workspace
                    .root
                    .join(value.trim_start_matches('/'))
                    .to_string_lossy()
                    .into_owned()
            } else {
                value.clone()
            };
            cmd.env(key, value);
        }
        cmd
    };
    // The terminal sends Ctrl+C to the foreground group, which includes Glyph.
    // Keep the launcher alive while the child receives its normal SIGINT.
    let _signals = InterruptGuard::new()?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command.status().context("launch failed; install/enable Bubblewrap for confined apps. Glyph never retries outside the sandbox")
}

struct InterruptGuard {
    previous: libc::sigaction,
    _lock: std::sync::MutexGuard<'static, ()>,
}
extern "C" fn keep_launcher_alive(_: libc::c_int) {}
impl InterruptGuard {
    fn new() -> Result<Self> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let lock = LOCK
            .lock()
            .map_err(|_| anyhow::anyhow!("application signal lock poisoned"))?;
        unsafe {
            let mut previous: libc::sigaction = std::mem::zeroed();
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = keep_launcher_alive as *const () as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            ensure!(
                libc::sigaction(libc::SIGINT, &action, &mut previous) == 0,
                "cannot protect launcher during app handoff"
            );
            Ok(Self {
                previous,
                _lock: lock,
            })
        }
    }
}
impl Drop for InterruptGuard {
    fn drop(&mut self) {
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
        }
    }
}
