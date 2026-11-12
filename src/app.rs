use crate::{
    config::App,
    workspace::{Workspace, normalize},
};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::Path,
};

#[derive(Clone, Debug, Serialize)]
pub struct Launch {
    pub app: String,
    pub kind: String,
    pub argv: Vec<String>,
    pub sandbox: bool,
    pub network: bool,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
    pub runtime_mounts: Vec<crate::workspace::Mount>,
    pub write_configs: bool,
    pub write_source: bool,
}

fn kind_from_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|x| x.to_str())
        .map(str::to_lowercase)
        .filter(|x| x != "app")
}

pub fn detect(path: &Path) -> Result<String> {
    let mut prefix = [0; 512];
    let n = File::open(path)?.read(&mut prefix)?;
    let bytes = &prefix[..n];
    if bytes.starts_with(b"\x7fELF") {
        return Ok(if bytes.get(8..11) == Some(b"AI\x02") {
            "appimage"
        } else {
            "native"
        }
        .into());
    }
    if bytes.starts_with(b"#!") {
        return Ok("shebang".into());
    }
    if bytes.starts_with(b"MZ") {
        return Ok("exe".into());
    }
    if bytes.starts_with(b"[Flatpak Ref]") {
        return Ok("flatpakref".into());
    }
    kind_from_extension(path)
        .context("cannot identify the application; declare its kind or command in /configs/apps")
}

impl Workspace {
    pub fn import_app(&self, source: &Path, name: &str, kind: Option<&str>) -> Result<String> {
        ensure!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                && name != "."
                && name != "..",
            "application names may contain letters, digits, dots, hyphens, and underscores"
        );
        let name = if name.ends_with(".app") {
            name.to_owned()
        } else {
            format!("{name}.app")
        };
        ensure!(source.is_file(), "import requires an application file");
        let path = format!("/apps/{name}");
        let destination = self.root.join("apps").join(&name);
        ensure!(!destination.exists(), "application already exists: {path}");
        let kind = match kind
            .map(str::to_owned)
            .or_else(|| kind_from_extension(source))
        {
            Some(kind) => kind,
            None => detect(source)?,
        };
        let app = App {
            path: path.clone(),
            kind: Some(kind),
            command: None,
            sandbox: true,
            network: false,
            env: BTreeMap::new(),
        };
        let config_path = self.root.join("configs/apps").join(format!("{name}.lua"));
        ensure!(!config_path.exists(), "application config already exists");
        // Byte escapes produce valid Lua literals even for non-ASCII kinds.
        let payload = format!(
            "glyph.app {{ path = {}, kind = {} }}\n",
            lua_string(&app.path),
            lua_string(app.kind.as_deref().unwrap())
        );
        fs::copy(source, &destination)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
        }
        if let Err(error) = fs::write(&config_path, payload) {
            let _ = fs::remove_file(destination);
            return Err(error.into());
        }
        Ok(path)
    }

    pub fn launch_plan(
        &self,
        cwd: &str,
        input: &str,
        args: &[String],
        agent: bool,
    ) -> Result<Launch> {
        let path = if input.contains('/') {
            normalize(cwd, input)?
        } else {
            format!("/apps/{input}")
        };
        let path = if path.ends_with(".app") || self.resolve(&path, false)?.is_file() {
            path
        } else {
            format!("{path}.app")
        };
        let source = self.resolve(&path, false)?;
        ensure!(source.is_file(), "application not found: {path}");
        let app = self.config.app(&path).unwrap_or(App {
            path: path.clone(),
            kind: None,
            command: None,
            sandbox: true,
            network: false,
            env: BTreeMap::new(),
        });
        ensure!(
            !agent || app.sandbox,
            "agents cannot launch applications with host access"
        );
        let kind = match &app.kind {
            Some(kind) => kind.clone(),
            None if app.command.is_some() => "custom".into(),
            None => detect(&source)?,
        };
        let handler = self.config.handler(&kind);
        let template = if let Some(command) = app.command {
            command
        } else if let Some(handler) = handler {
            handler.command
        } else {
            match kind.as_str() {
                "py" | "python" => vec!["/usr/bin/python3".into(), "{app}".into(), "{args}".into()],
                "sh" | "bash" => vec!["/bin/bash".into(), "{app}".into(), "{args}".into()],
                "native" => vec!["{app}".into(), "{args}".into()],
                "appimage" => vec![
                    "{app}".into(),
                    "--appimage-extract-and-run".into(),
                    "{args}".into(),
                ],
                "shebang" => {
                    let mut bytes = [0; 512];
                    let n = File::open(&source)?.read(&mut bytes)?;
                    let text = String::from_utf8_lossy(&bytes[..n]);
                    let first = text.lines().next().context("empty shebang")?;
                    let mut words = shlex::split(first.trim_start_matches("#!").trim())
                        .context("invalid interpreter line")?;
                    ensure!(
                        !words.is_empty() && words[0].starts_with('/'),
                        "shebang requires an absolute interpreter"
                    );
                    words.extend(["{app}".into(), "{args}".into()]);
                    words
                }
                "flatpak" | "flatpakref" => {
                    ensure!(
                        !app.sandbox,
                        "Flatpak requires host installation/session access. Set sandbox = false explicitly in this app's configuration, then approve its launch interactively. Agents cannot launch host-mode applications."
                    );
                    vec![
                        "/usr/bin/python3".into(),
                        self.root
                            .join("configs/handlers/flatpak.py")
                            .to_string_lossy()
                            .into_owned(),
                        kind.clone(),
                        "{app}".into(),
                        "{args}".into(),
                    ]
                }
                _ => bail!(
                    "no handler for '{kind}'; define glyph.handler {{ kind = ..., command = {{..., \"{{app}}\", \"{{args}}\"}} }} in /configs"
                ),
            }
        };
        ensure!(!template.is_empty(), "empty application command");
        let app_path = if app.sandbox {
            path.clone()
        } else {
            source.to_string_lossy().into_owned()
        };
        let mut argv = Vec::new();
        let mut has_args = false;
        for word in template {
            match word.as_str() {
                "{args}" => {
                    argv.extend_from_slice(args);
                    has_args = true;
                }
                "{app}" => argv.push(app_path.clone()),
                "{cwd}" => argv.push(if app.sandbox {
                    cwd.into()
                } else {
                    self.resolve(cwd, false)?.to_string_lossy().into_owned()
                }),
                _ => argv.push(word),
            }
        }
        if !has_args {
            argv.extend_from_slice(args);
        }
        ensure!(
            !argv.is_empty() && !argv[0].is_empty(),
            "invalid application command"
        );
        Ok(Launch {
            app: path,
            kind,
            argv,
            sandbox: app.sandbox,
            network: app.network,
            cwd: cwd.into(),
            env: app.env,
            runtime_mounts: Vec::new(),
            write_configs: false,
            write_source: false,
        })
    }
}

fn lua_string(value: &str) -> String {
    let mut result = String::from("\"");
    for byte in value.bytes() {
        match byte {
            b'"' => result.push_str("\\\""),
            b'\\' => result.push_str("\\\\"),
            32..=126 => result.push(byte as char),
            _ => result.push_str(&format!("\\{byte:03}")),
        }
    }
    result.push('"');
    result
}
