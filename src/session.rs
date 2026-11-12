use crate::{storage::StorageSession, workspace::Workspace};
use anyhow::{Context, Result, ensure};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Serialize, Deserialize)]
pub struct Descriptor {
    pub root: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    pub socket: PathBuf,
    pub token: String,
    pub emergency: bool,
}
#[derive(Clone)]
pub struct Remote {
    pub descriptor: Descriptor,
}
impl Remote {
    pub fn request(&self, operation: &str, argument: Value) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.descriptor.socket)
            .context("Glyph session controller is unavailable")?;
        stream.set_read_timeout(Some(Duration::from_secs(120)))?;
        let value =
            json!({"token":self.descriptor.token,"operation":operation,"argument":argument});
        let mut bytes = Zeroizing::new(serde_json::to_vec(&value)?);
        bytes.push(b'\n');
        stream.write_all(&bytes)?;
        let mut line = Zeroizing::new(String::new());
        BufReader::new(stream).read_line(&mut line)?;
        let reply: Value = serde_json::from_str(&line)?;
        if let Some(error) = reply.get("error") {
            anyhow::bail!("{}", error.as_str().unwrap_or("session request failed"));
        }
        Ok(reply["value"].clone())
    }
}
pub fn attach(path: &Path) -> Result<StorageSession> {
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "invalid session descriptor"
    );
    let descriptor: Descriptor = serde_json::from_slice(&fs::read(path)?)?;
    StorageSession::remote(Remote { descriptor })
}

pub fn kitty(storage: &mut StorageSession, workspace: &Workspace, binary: &Path) -> Result<i32> {
    let control = tempfile::Builder::new()
        .prefix("controller-")
        .tempdir_in(&storage.paths.cache)?;
    let socket = control.path().join("control.sock");
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    let mut random = [0; 32];
    OsRng.fill_bytes(&mut random);
    let token = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let descriptor = Descriptor {
        root: workspace.root.clone(),
        data: storage.paths.data.clone(),
        cache: storage.paths.cache.clone(),
        socket,
        token,
        emergency: storage.emergency(),
    };
    let path = control.path().join("session.json");
    fs::write(&path, serde_json::to_vec(&descriptor)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    }
    let config = workspace.root.join("configs/user/kitty/kitty.conf");
    let mut command = Command::new("kitty");
    command
        .env("HOME", workspace.root.join("home"))
        .env("XDG_CONFIG_HOME", workspace.root.join("configs/user"))
        .env("XDG_CACHE_HOME", workspace.root.join("home/.cache"));
    command.arg("--config").arg(&config).args([
        "--class",
        "Glyph",
        "--title",
        "Glyph · your terminal, your software",
    ]);
    let kitty_socket = format!("unix:{}", control.path().join("kitty.sock").display());
    command
        .arg("--listen-on")
        .arg(&kitty_socket)
        .args(["--override", "allow_remote_control=socket-only"]);
    if let Some(image) = crate::appearance::saved(&workspace.root) {
        if let Ok(image) = workspace.resolve(&image, false) {
            command
                .arg("--override")
                .arg(format!("background_image={}", image.display()));
        }
    }
    command.env("GLYPH_KITTY_SOCKET", &kitty_socket);
    command.arg(binary).arg("--attach").arg(&path);
    let mut child = command
        .spawn()
        .context("starting Glyph's own Kitty instance")?;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                let mut line = Zeroizing::new(String::new());
                let read = BufReader::new(&stream).take(65536).read_line(&mut line);
                let reply = (|| -> Result<Value> {
                    read?;
                    let mut request: Value = serde_json::from_str(&line)?;
                    ensure!(
                        request["token"].as_str() == Some(&descriptor.token),
                        "invalid controller capability"
                    );
                    let arg = &request["argument"];
                    let result = match request["operation"].as_str().unwrap_or("") {
                        "status" => json!(storage.summary()),
                        "encrypted" => json!(storage.encrypted()),
                        "snapshots" => serde_json::to_value(storage.snapshots()?)?,
                        "save" => json!(storage.save_snapshot(arg.as_str().unwrap_or("manual"))?),
                        "restore" => {
                            storage.restore_snapshot(
                                arg.as_i64().context("snapshot ID is required")?,
                            )?;
                            Value::Null
                        }
                        "key" => {
                            storage.set_key(
                                arg["password"].as_str().context("passphrase missing")?,
                                arg["change"].as_bool().unwrap_or(false),
                            )?;
                            Value::Null
                        }
                        _ => anyhow::bail!("unsupported controller operation"),
                    };
                    if let Some(Value::String(password)) = request
                        .get_mut("argument")
                        .and_then(|v| v.get_mut("password"))
                    {
                        password.zeroize();
                    }
                    Ok(result)
                })();
                let reply = match reply {
                    Ok(value) => json!({"value":value}),
                    Err(error) => json!({"error":format!("{error:#}")}),
                };
                stream.write_all(&serde_json::to_vec(&reply)?)?;
                stream.write_all(b"\n")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(error) => return Err(error.into()),
        }
    }
}
