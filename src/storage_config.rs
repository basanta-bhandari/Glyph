use crate::storage::{Platform, StoragePaths};
use anyhow::{Context, Result, ensure};
use mlua::{HookTriggers, Lua, LuaSerdeExt, StdLib, Table, VmState};
use serde::Deserialize;
use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone, Debug)]
pub struct StorageSettings {
    pub paths: StoragePaths,
    pub compression_level: u32,
    pub config_path: PathBuf,
    pub backend: String,
    pub snapshots: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Definition {
    backend: String,
    data_dir: Option<String>,
    session_dir: Option<String>,
    compression_level: u32,
    snapshots: usize,
}
impl Default for Definition {
    fn default() -> Self {
        Self {
            backend: "mounted".into(),
            data_dir: None,
            session_dir: None,
            compression_level: 6,
            snapshots: 5,
        }
    }
}

impl StorageSettings {
    pub fn load(root: Option<&Path>, explicit: Option<&Path>) -> Result<Self> {
        let platform = if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        };
        let path = match explicit {
            Some(path) => path.to_path_buf(),
            None => match root {
                Some(root) => root.join("storage.lua"),
                None => default_config_path(platform, |key| std::env::var_os(key))?,
            },
        };
        if explicit.is_none() {
            seed_config(&path)?;
        }
        ensure!(
            fs::symlink_metadata(&path)
                .with_context(|| format!("reading storage configuration {}", path.display()))?
                .is_file(),
            "storage configuration must be a regular file, not a symlink"
        );
        let path = path.canonicalize()?;
        let defaults = match root {
            Some(root) => StoragePaths::portable(root.to_path_buf()),
            None => StoragePaths::defaults()?,
        };
        Self::from_text(&fs::read_to_string(&path)?, &path, defaults, root, |key| {
            std::env::var_os(key)
        })
    }

    pub fn from_text(
        text: &str,
        path: &Path,
        mut defaults: StoragePaths,
        root_override: Option<&Path>,
        get: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self> {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            Default::default(),
        )?;
        lua.set_memory_limit(2 * 1024 * 1024)?;
        let fuel = Rc::new(Cell::new(0));
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(1000),
            move |_, _| {
                fuel.set(fuel.get() + 1);
                if fuel.get() > 200 {
                    return Err(mlua::Error::runtime(
                        "storage Lua instruction budget exceeded",
                    ));
                }
                Ok(VmState::Continue)
            },
        );
        for name in ["dofile", "loadfile", "load", "collectgarbage"] {
            lua.globals().set(name, mlua::Value::Nil)?;
        }
        let definition = Rc::new(RefCell::new(None));
        let output = definition.clone();
        let glyph = lua.create_table()?;
        glyph.set(
            "storage",
            lua.create_function(move |lua, table: Table| {
                let value: Definition = lua.from_value(mlua::Value::Table(table))?;
                let mut output = output.borrow_mut();
                if output.is_some() {
                    return Err(mlua::Error::runtime("define glyph.storage only once"));
                }
                *output = Some(value);
                Ok(())
            })?,
        )?;
        lua.globals().set("glyph", glyph)?;
        lua.load(text)
            .set_name(path.to_string_lossy())
            .exec()
            .with_context(|| format!("loading storage configuration {}", path.display()))?;
        let definition = definition.take().unwrap_or_default();
        ensure!(
            ["container", "mounted"].contains(&definition.backend.as_str()),
            "storage backend '{}' is unavailable; use mounted or container. No silent fallback is performed.",
            definition.backend
        );
        ensure!(
            (1..=9).contains(&definition.compression_level),
            "compression_level must be between 1 (faster) and 9 (smaller)"
        );
        ensure!(
            (2..=20).contains(&definition.snapshots),
            "snapshots must be between 2 and 20"
        );
        let parent = path
            .parent()
            .context("storage configuration needs a parent directory")?;
        if root_override.is_none() {
            if let Some(data) = definition.data_dir {
                defaults.data = resolve_path(&data, parent, &get)?;
            }
        }
        if let Some(session) = definition.session_dir {
            defaults.cache = resolve_path(&session, parent, &get)?;
        }
        Ok(Self {
            paths: defaults,
            compression_level: definition.compression_level,
            config_path: path.to_path_buf(),
            backend: definition.backend,
            snapshots: definition.snapshots,
        })
    }
}

pub fn default_config_path(
    platform: Platform,
    get: impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    Ok(match platform {
        Platform::Linux => {
            let config = get("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute());
            config
                .or_else(|| {
                    get("HOME")
                        .map(PathBuf::from)
                        .filter(|home| home.is_absolute())
                        .map(|home| home.join(".config"))
                })
                .context("HOME is missing; use --storage-config FILE")?
                .join("glyph/storage.lua")
        }
        Platform::MacOs => get("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())
            .map(|home| home.join("Library/Application Support/Glyph/storage.lua"))
            .context("HOME is missing; use --storage-config FILE")?,
        Platform::Windows => get("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| get("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Local")))
            .context("LOCALAPPDATA is missing; use --storage-config FILE")?
            .join("Glyph/storage.lua"),
    })
}

fn resolve_path(
    value: &str,
    parent: &Path,
    get: &impl Fn(&str) -> Option<OsString>,
) -> Result<PathBuf> {
    ensure!(
        !value.is_empty() && !value.contains('\0'),
        "storage paths cannot be empty or contain NUL"
    );
    if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = get("HOME")
            .or_else(|| get("USERPROFILE"))
            .context("cannot expand ~ without a home directory")?;
        return Ok(PathBuf::from(home).join(value.get(2..).unwrap_or_default()));
    }
    ensure!(
        !value.starts_with('~'),
        "use ~ or ~/path; other-user home expansion is unsupported"
    );
    let path = PathBuf::from(value);
    Ok(if path.is_absolute() {
        path
    } else {
        parent.join(path)
    })
}

fn seed_config(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("storage configuration needs a directory")?;
    fs::create_dir_all(parent)?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(include_str!("../assets/storage.lua").as_bytes())?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
