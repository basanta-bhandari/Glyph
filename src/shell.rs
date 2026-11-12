use crate::{
    app::Launch,
    config::Action,
    storage::StorageSession,
    workspace::{Workspace, normalize},
};
use anyhow::{Result, bail, ensure};
use std::fs;

pub enum Effect {
    Output(String),
    Launch(Launch),
    Quit,
    Launcher,
    Key { change: bool },
    SnapshotSave(String),
    SnapshotRestore(i64),
    UserAction(String, Vec<String>),
    Package(String, Vec<String>),
    Store,
}
pub struct Shell {
    pub workspace: Workspace,
    pub cwd: String,
    pub agent: bool,
    pub last_exit: i32,
    pub storage: Option<StorageSession>,
    pub emergency: bool,
}

pub const HELP: &str = "Glyph · files, applications, and tools\n\n  ls [path]                  list files\n  cd <path>                  change directory\n  pwd                        show Glyph directory\n  cat <file>                 read a text file\n  write <file> <text>         write a file beneath /home\n  mkdir <path>               create a directory beneath /home\n  copy <source> <target>     copy a file\n  move <source> <target>     move a file\n  rm <file>                  remove a file (not directories)\n  run|exec <app> [args...]    launch an application\n  plan <app> [args...]        inspect launch configuration\n  apps                       show applications\n  agents                     show configured agents\n  agent <name> [args...]      launch an agent, always confined\n  links                      show host-resource mappings\n  help                       show this help\n  quit                       leave Glyph\n\nCtrl+Space opens the launcher. Ctrl+C clears input; Ctrl+D exits.\nConfiguration changes take effect on restart. No implicit host-shell fallback.";

impl Shell {
    pub fn new(workspace: Workspace, agent: bool) -> Self {
        Self {
            workspace,
            cwd: "/home".into(),
            agent,
            last_exit: 0,
            storage: None,
            emergency: false,
        }
    }

    pub fn actions(&self, actions: Vec<Action>) -> Result<Vec<Effect>> {
        actions
            .into_iter()
            .map(|action| match action {
                Action::Launcher => Ok(Effect::Launcher),
                Action::Launch(path, args) => Ok(Effect::Launch(
                    self.workspace
                        .launch_plan(&self.cwd, &path, &args, self.agent)?,
                )),
            })
            .collect()
    }

    pub fn execute(&mut self, input: &str) -> Result<Vec<Effect>> {
        let parts = shlex::split(input)
            .ok_or_else(|| anyhow::anyhow!("unclosed quote or invalid escape"))?;
        if parts.is_empty() {
            return Ok(Vec::new());
        }
        let command = parts[0].as_str();
        let args = &parts[1..];
        if self.emergency {
            ensure!(
                [
                    "help",
                    "pwd",
                    "cd",
                    "ls",
                    "dir",
                    "cat",
                    "storage",
                    "snapshot",
                    "snapshots",
                    "quit",
                    "exit"
                ]
                .contains(&command)
                    || (command == "key" && args == ["status"]),
                "emergency mode is read-only: inspect files, list snapshots, and restore a safe snapshot before restarting"
            );
        }
        let path = |i: usize| -> Result<String> {
            normalize(
                &self.cwd,
                args.get(i)
                    .ok_or_else(|| anyhow::anyhow!("missing path; see help"))?,
            )
        };
        let output = match command {
            "wallpaper"=>{ensure!(!self.agent,"wallpaper management is user-only");return Ok(vec![Effect::UserAction("wallpaper".into(),args.to_vec())]);},
            "reload"=>{ensure!(!self.agent,"policy reload is user-only");return Ok(vec![Effect::UserAction("reload".into(),Vec::new())]);},
            "store"=>{
                match args.first().map(String::as_str).unwrap_or("menu"){
                    "list"=>crate::catalog::listing(&self.workspace,&args.get(1..).unwrap_or_default().join(" "))?,
                    "install"=>{ensure!(!self.agent,"catalog installation is user-only");let query=args.get(1).ok_or_else(||anyhow::anyhow!("store install NAME"))?;let item=crate::catalog::entries(&self.workspace)?.into_iter().find(|e|e.name.eq_ignore_ascii_case(query)||e.command==*query).ok_or_else(||anyhow::anyhow!("catalog entry not found"))?;return Ok(vec![Effect::Package("install".into(),vec![item.package])]);},
                    "menu"=>return Ok(vec![Effect::Store]),
                    _=>bail!("store | store list [query] | store install NAME"),
                }
            },
            "edit" => {ensure!(!args.is_empty(),"usage: edit /path/to/file");return Ok(vec![Effect::Launch(crate::system::editor_plan(&self.workspace,&self.cwd,&args[0],self.agent)?)]);},
            "editor" => {if args.first().map(String::as_str)==Some("setup"){ensure!(!self.agent,"editor setup is user-only");return Ok(vec![Effect::UserAction("editor-setup".into(),Vec::new())]);}serde_json::to_string_pretty(&self.workspace.config.editor())?},
            "source" => {
                let action=args.first().map(String::as_str).unwrap_or("status");ensure!(!self.agent,"system source management is user-only");
                if action=="edit"{let file=args.get(1).map(String::as_str).unwrap_or("src/main.rs");let file=crate::workspace::normalize("/system/source",file)?;ensure!(file.starts_with("/system/source/"),"source edit must stay in the source tree");return Ok(vec![Effect::Launch(crate::system::editor_plan(&self.workspace,&self.cwd,&file,false)?)]);}
                ensure!(["status","build","activate","rollback"].contains(&action),"source status | source edit [file] | source build | source activate | source rollback");return Ok(vec![Effect::UserAction(format!("source-{action}"),Vec::new())]);
            },
            "rice"=>{ensure!(!self.agent,"rice import is user-only");return Ok(vec![Effect::UserAction("rice".into(),args.to_vec())]);},
            "guide"=>{return Ok(vec![Effect::Launch(self.workspace.launch_plan(&self.cwd,"/apps/guide.app",args,self.agent)?)]);},
            "snapshot" | "snapshots" => {
                let storage=self.storage.as_ref().ok_or_else(||anyhow::anyhow!("no managed storage"))?;
                match args.first().map(String::as_str).unwrap_or("list") {
                    "list"=>serde_json::to_string_pretty(&storage.snapshots()?)?,
                    "save"=>{ensure!(!self.agent&&!self.emergency,"snapshot management is a user-only operation outside emergency mode");return Ok(vec![Effect::SnapshotSave(args.get(1..).unwrap_or_default().join(" "))]);},
                    "restore"=>{ensure!(!self.agent,"snapshot restoration is user-only");let id=args.get(1).ok_or_else(||anyhow::anyhow!("usage: snapshot restore ID"))?.parse()?;return Ok(vec![Effect::SnapshotRestore(id)]);},
                    _=>bail!("usage: snapshot list | snapshot save [label] | snapshot restore ID"),
                }
            },
            "help" => format!("{HELP}\n\n{}\n\nYour system:\n  edit FILE                  configured editor (Neovim/NvChad by default)\n  editor setup               set up NvChad inside Glyph\n  source status|edit|build|activate|rollback\n  rice status|copy APP        inspect/import independent host rice copies\n  guide [topic]              human docs, figures, and terminal jokes\n\nStorage:\n  storage | key status       show disk, compression, and encryption\n  key set                    enable encryption (hidden user prompt)\n  key change                 change the encryption passphrase\n  snapshot list              inspect retained recovery points\n  snapshot save [label]      save a manual snapshot\n  snapshot restore ID        select recovery snapshot and exit\nMounted writes are compressed and committed incrementally.", crate::packages::HELP),
            "storage" => self.storage_summary(),
            "key" => {
                ensure!(args.len() == 1, "usage: key set | key change | key status; never put a passphrase in a command");
                match args[0].as_str() {
                    "status" => self.storage_summary(),
                    "set" | "change" => {
                        ensure!(!self.agent, "encryption key management is a user-only operation");
                        return Ok(vec![Effect::Key { change: args[0] == "change" }]);
                    },
                    _ => bail!("usage: key set | key change | key status"),
                }
            },
            "pwd" => self.cwd.clone(),
            "cd" => {
                let target = path(0)?;
                ensure!(
                    self.workspace.is_dir(&target),
                    "directory not found: {target}"
                );
                self.cwd = target;
                String::new()
            }
            "ls" | "dir" => {
                let target = if args.is_empty() {
                    self.cwd.clone()
                } else {
                    path(0)?
                };
                self.workspace
                    .list(&target)?
                    .into_iter()
                    .map(|(name, dir)| format!("{}{name}", if dir { "/ " } else { "  " }))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "cat" => self.workspace.read(&path(0)?)?,
            "write" => {
                ensure!(args.len() >= 2, "usage: write <file> <text>");
                self.workspace.write(&path(0)?, &args[1..].join(" "))?;
                "Written.".into()
            }
            "mkdir" => {
                fs::create_dir(self.workspace.resolve(&path(0)?, true)?)?;
                "Directory created.".into()
            }
            "copy" => {
                fs::copy(
                    self.workspace.resolve(&path(0)?, false)?,
                    self.workspace.resolve(&path(1)?, true)?,
                )?;
                "Copied.".into()
            }
            "move" => {
                ensure!(
                    self.workspace.resolve(&path(0)?, true)?.is_file(),
                    "move currently supports files only"
                );
                fs::rename(
                    self.workspace.resolve(&path(0)?, true)?,
                    self.workspace.resolve(&path(1)?, true)?,
                )?;
                "Moved.".into()
            }
            "rm" => {
                fs::remove_file(self.workspace.resolve(&path(0)?, true)?)?;
                "Removed.".into()
            }
            "install" | "remove" | "uninstall" | "upgrade" => {
                return Ok(vec![Effect::Package(command.into(),args.to_vec())]);
            }
            "packages" => match self.workspace.config.package_manager() {
                Some(manager) => serde_json::to_string_pretty(&manager)?,
                None => "No package manager selected. Edit /configs/packages.lua to enable an example or define your own.".into(),
            },
            "run" | "exec" | "plan" => {
                ensure!(!args.is_empty(), "usage: {command} <application> [args...]");
                if command == "plan" && ["install", "remove", "uninstall", "upgrade"].contains(&args[0].as_str()) {
                    return Ok(vec![Effect::Output(serde_json::to_string_pretty(&self.workspace.package_plan(&self.cwd, &args[0], &args[1..], self.agent)?)?)]);
                }
                let plan =
                    self.workspace
                        .launch_plan(&self.cwd, &args[0], &args[1..], self.agent)?;
                if command == "plan" {
                    serde_json::to_string_pretty(&plan)?
                } else {
                    return Ok(vec![Effect::Launch(plan)]);
                }
            }
            "apps" => self
                .workspace
                .list("/apps")?
                .into_iter()
                .filter(|(_, dir)| !dir)
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
                .join("\n"),
            "links" => {
                if self.workspace.links.is_empty() {
                    "No host resources linked. Define glyph.link in /configs/links/*.lua.".into()
                } else {
                    self.workspace
                        .links
                        .iter()
                        .map(|l| {
                            format!(
                                "{} → {} [{}; {}]",
                                l.source,
                                l.target,
                                l.access,
                                if l.recursive { "recursive" } else { "shallow" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "agents" => self
                .workspace
                .config
                .agents()
                .iter()
                .map(|a| format!("{} → {}", a.name, a.app))
                .collect::<Vec<_>>()
                .join("\n"),
            "agent" => {
                ensure!(!args.is_empty(), "usage: agent <name> [args...]");
                if args[0]=="install"{ensure!(!self.agent,"agent installation is user-only");let provider=args.get(1).ok_or_else(||anyhow::anyhow!("agent install NAME"))?;ensure!(self.workspace.config.package_manager().is_some_and(|m|m.sandbox),"agent packages must be installed inside Glyph, not through a host-mode manager");return Ok(vec![Effect::Package("install".into(),vec![provider.clone()])]);}
                if args[0]=="add"{ensure!(!self.agent,"agent registration is user-only");return Ok(vec![Effect::UserAction("agent-add".into(),args[1..].to_vec())]);}
                let agent = self
                    .workspace
                    .config
                    .agents()
                    .into_iter()
                    .find(|a| a.name == args[0])
                    .ok_or_else(|| anyhow::anyhow!("agent is not configured"))?;
                return Ok(vec![Effect::Launch(self.workspace.launch_plan(
                    &self.cwd,
                    &agent.app,
                    &args[1..],
                    true,
                )?)]);
            }
            "quit" | "exit" => return Ok(vec![Effect::Quit]),
            _ => {
                if let Some(app)=crate::catalog::capability(&self.workspace,command){return Ok(vec![Effect::Launch(self.workspace.launch_plan(&self.cwd,&format!("/apps/{app}"),args,self.agent)?)]);}
                if let Some(actions) =
                    self.workspace
                        .config
                        .invoke(command, args.to_vec(), false)?
                {
                    return self.actions(actions);
                }
                bail!(
                    "unknown command '{command}'; see help or define glyph.command in /configs/init.lua"
                )
            }
        };
        Ok(vec![Effect::Output(output)])
    }

    fn storage_summary(&self) -> String {
        self.storage.as_ref().map(StorageSession::summary).unwrap_or_else(|| format!("Unmanaged workspace: {}\nUse the Glyph CLI for compressed/encrypted session storage.", self.workspace.root.display()))
    }
}
