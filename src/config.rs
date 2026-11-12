use anyhow::{Context, Result, bail};
use mlua::{Function, HookTriggers, Lua, LuaSerdeExt, StdLib, Table, VmState};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    fs,
    path::Path,
    rc::Rc,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub border: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "#181818".into(),
            foreground: "#e8d8b8".into(),
            accent: "#d6a35d".into(),
            border: "rounded".into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default = "read_access")]
    pub access: String,
}
fn read_access() -> String {
    "read".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub path: String,
    pub kind: Option<String>,
    pub command: Option<Vec<String>>,
    #[serde(default = "yes")]
    pub sandbox: bool,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Editor {
    pub command: Vec<String>,
    pub preset: String,
    pub network: bool,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            command: vec!["/usr/bin/nvim".into(), "{file}".into()],
            preset: "nvchad".into(),
            network: true,
        }
    }
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Handler {
    pub kind: String,
    pub command: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub name: String,
    pub app: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CommandTemplate {
    Text(String),
    Args(Vec<String>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManager {
    pub name: String,
    pub install: CommandTemplate,
    pub remove: CommandTemplate,
    pub upgrade: CommandTemplate,
    #[serde(default = "yes")]
    pub sandbox: bool,
    #[serde(default)]
    pub network: bool,
}

#[derive(Clone, Debug)]
pub enum Action {
    Launch(String, Vec<String>),
    Launcher,
}

#[derive(Default)]
struct Settings {
    theme: Theme,
    links: Vec<Link>,
    apps: BTreeMap<String, App>,
    handlers: BTreeMap<String, Handler>,
    agents: BTreeMap<String, Agent>,
    package_manager: Option<PackageManager>,
    editor: Editor,
    commands: BTreeMap<String, Function>,
    binds: BTreeMap<String, Function>,
}

pub struct Config {
    lua: Lua,
    settings: Rc<RefCell<Settings>>,
    actions: Rc<RefCell<Vec<Action>>>,
    fuel: Rc<Cell<u32>>,
}

impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        // Configuration has a deliberately small API, without host IO, OS, or module loading.
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            Default::default(),
        )?;
        lua.set_memory_limit(16 * 1024 * 1024)?;
        let fuel = Rc::new(Cell::new(0));
        let count = fuel.clone();
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            move |_, _| {
                count.set(count.get() + 1);
                if count.get() > 100 {
                    return Err(mlua::Error::runtime("Lua instruction budget exceeded"));
                }
                Ok(VmState::Continue)
            },
        );
        for name in ["dofile", "loadfile", "load", "collectgarbage"] {
            lua.globals().set(name, mlua::Value::Nil)?;
        }
        let settings = Rc::new(RefCell::new(Settings::default()));
        let actions = Rc::new(RefCell::new(Vec::new()));
        let api = lua.create_table()?;
        let s = settings.clone();
        api.set(
            "editor",
            lua.create_function(move |lua, table: Table| {
                s.borrow_mut().editor = lua.from_value(mlua::Value::Table(table))?;
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "theme",
            lua.create_function(move |lua, table: Table| {
                s.borrow_mut().theme = lua.from_value(mlua::Value::Table(table))?;
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "link",
            lua.create_function(move |lua, table: Table| {
                s.borrow_mut()
                    .links
                    .push(lua.from_value(mlua::Value::Table(table))?);
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "app",
            lua.create_function(move |lua, table: Table| {
                let app: App = lua.from_value(mlua::Value::Table(table))?;
                if s.borrow_mut().apps.insert(app.path.clone(), app).is_some() {
                    return Err(mlua::Error::runtime("duplicate app definition"));
                }
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "handler",
            lua.create_function(move |lua, table: Table| {
                let handler: Handler = lua.from_value(mlua::Value::Table(table))?;
                if s.borrow_mut()
                    .handlers
                    .insert(handler.kind.clone(), handler)
                    .is_some()
                {
                    return Err(mlua::Error::runtime("duplicate handler"));
                }
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "agent",
            lua.create_function(move |lua, table: Table| {
                let agent: Agent = lua.from_value(mlua::Value::Table(table))?;
                if s.borrow_mut()
                    .agents
                    .insert(agent.name.clone(), agent)
                    .is_some()
                {
                    return Err(mlua::Error::runtime("duplicate agent"));
                }
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "package_manager",
            lua.create_function(move |lua, table: Table| {
                let manager: PackageManager = lua.from_value(mlua::Value::Table(table))?;
                let mut settings = s.borrow_mut();
                if settings.package_manager.is_some() {
                    return Err(mlua::Error::runtime(
                        "configure only one active package manager",
                    ));
                }
                settings.package_manager = Some(manager);
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "command",
            lua.create_function(move |_, (name, function): (String, Function)| {
                s.borrow_mut().commands.insert(name, function);
                Ok(())
            })?,
        )?;
        let s = settings.clone();
        api.set(
            "bind",
            lua.create_function(move |_, (name, function): (String, Function)| {
                s.borrow_mut().binds.insert(name, function);
                Ok(())
            })?,
        )?;
        let apps = lua.create_table()?;
        let a = actions.clone();
        apps.set(
            "launch",
            lua.create_function(move |_, (path, args): (String, Option<Vec<String>>)| {
                a.borrow_mut()
                    .push(Action::Launch(path, args.unwrap_or_default()));
                Ok(())
            })?,
        )?;
        api.set("apps", apps)?;
        let launcher = lua.create_table()?;
        let a = actions.clone();
        launcher.set(
            "open",
            lua.create_function(move |_, ()| {
                a.borrow_mut().push(Action::Launcher);
                Ok(())
            })?,
        )?;
        api.set("launcher", launcher)?;
        lua.globals().set("glyph", api)?;
        let config = Self {
            lua,
            settings,
            actions,
            fuel,
        };
        config.eval_file(&root.join("init.lua"))?;
        let packages = root.join("packages.lua");
        if packages.exists() {
            let builtin = fs::read_to_string(&packages)?.contains("GLYPH_BUILTIN_DEFAULT");
            if !builtin || config.settings.borrow().package_manager.is_none() {
                config.eval_file(&packages)?;
            }
        }
        let editor = root.join("editor.lua");
        if editor.exists() {
            config.eval_file(&editor)?;
        }
        for dir in ["themes", "links", "apps", "agents"] {
            let mut files = fs::read_dir(root.join(dir))?.collect::<std::io::Result<Vec<_>>>()?;
            files.sort_by_key(|entry| entry.file_name());
            for file in files {
                let path = file.path();
                if path.extension().is_some_and(|ext| ext == "lua") {
                    if file.file_type()?.is_symlink() {
                        bail!("config files cannot be symlinks: {}", path.display());
                    }
                    config.eval_file(&path)?;
                }
            }
        }
        config.actions.borrow_mut().clear();
        config.validate()?;
        Ok(config)
    }

    fn eval_file(&self, path: &Path) -> Result<()> {
        if fs::symlink_metadata(path)?.file_type().is_symlink() {
            bail!("configuration files cannot be symlinks");
        }
        self.fuel.set(0);
        self.lua
            .load(fs::read_to_string(path)?)
            .set_name(path.to_string_lossy())
            .exec()
            .with_context(|| format!("loading {}", path.display()))?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let s = self.settings.borrow();
        if s.editor.command.is_empty() || !s.editor.command.iter().any(|a| a == "{file}") {
            bail!("editor command must include {{file}} as a complete argument");
        }
        if !["nvchad", "user", "plain"].contains(&s.editor.preset.as_str()) {
            bail!("editor preset must be nvchad, user, or plain");
        }
        for color in [&s.theme.background, &s.theme.foreground, &s.theme.accent] {
            if color.len() != 7
                || !color.starts_with('#')
                || !color[1..].bytes().all(|b| b.is_ascii_hexdigit())
            {
                bail!("theme colors must use #RRGGBB");
            }
        }
        if !["rounded", "plain", "double"].contains(&s.theme.border.as_str()) {
            bail!("border must be rounded, plain, or double");
        }
        for handler in s.handlers.values() {
            if handler.kind.is_empty() || handler.command.is_empty() {
                bail!("handlers require kind and command");
            }
        }
        for app in s.apps.values() {
            if !app.path.starts_with("/apps/") || !app.path.ends_with(".app") {
                bail!("app definitions must identify a file under /apps ending in .app");
            }
        }
        if let Some(manager) = &s.package_manager {
            manager.validate()?;
        }
        Ok(())
    }

    pub fn theme(&self) -> Theme {
        self.settings.borrow().theme.clone()
    }
    pub fn editor(&self) -> Editor {
        self.settings.borrow().editor.clone()
    }
    pub fn links(&self) -> Vec<Link> {
        self.settings.borrow().links.clone()
    }
    pub fn app(&self, path: &str) -> Option<App> {
        self.settings.borrow().apps.get(path).cloned()
    }
    pub fn handler(&self, kind: &str) -> Option<Handler> {
        self.settings.borrow().handlers.get(kind).cloned()
    }
    pub fn agents(&self) -> Vec<Agent> {
        self.settings.borrow().agents.values().cloned().collect()
    }
    pub fn commands(&self) -> Vec<String> {
        self.settings.borrow().commands.keys().cloned().collect()
    }
    pub fn package_manager(&self) -> Option<PackageManager> {
        self.settings.borrow().package_manager.clone()
    }
    pub fn invoke(
        &self,
        name: &str,
        args: Vec<String>,
        binding: bool,
    ) -> Result<Option<Vec<Action>>> {
        let callback = {
            let s = self.settings.borrow();
            if binding {
                s.binds.get(name).cloned()
            } else {
                s.commands.get(name).cloned()
            }
        };
        let Some(callback) = callback else {
            return Ok(None);
        };
        self.fuel.set(0);
        self.actions.borrow_mut().clear();
        if let Err(error) = callback.call::<()>(args) {
            self.actions.borrow_mut().clear();
            return Err(error.into());
        }
        Ok(Some(self.actions.take()))
    }
}
