use crate::{
    app::Launch,
    config::{CommandTemplate, PackageManager},
    workspace::Workspace,
};
use anyhow::{Context, Result, bail, ensure};

pub const PACKAGE_TOKEN: &str = "{[pkg}]";
pub const HELP: &str = "Package commands (configured in /configs/packages.lua):\n  install <package>          install through your selected manager\n  remove <package>           remove a package (uninstall is an alias)\n  upgrade <package>          upgrade through your selected manager\n  plan install <package>     preview a package operation without running it\n  packages                   show the selected manager and templates\nThe placeholder {[pkg}] is passed as one argument, without shell evaluation.";

impl CommandTemplate {
    pub fn words(&self) -> Result<Vec<String>> {
        let words = match self {
            Self::Text(text) => shlex::split(text).context("invalid quoting in package command")?,
            Self::Args(args) => args.clone(),
        };
        ensure!(
            !words.is_empty() && !words[0].is_empty(),
            "package command cannot be empty"
        );
        ensure!(
            words[0] != PACKAGE_TOKEN,
            "package placeholder cannot be the executable"
        );
        ensure!(
            words.iter().all(|word| !word.contains('\0')),
            "package commands cannot contain NUL"
        );
        ensure!(
            words.iter().any(|word| word == PACKAGE_TOKEN),
            "each package command must contain {{[pkg}}] as a complete argument"
        );
        ensure!(
            words
                .iter()
                .all(|word| !word.contains(PACKAGE_TOKEN) || word == PACKAGE_TOKEN),
            "{{[pkg}}] must occupy a complete argument"
        );
        Ok(words)
    }
}

impl PackageManager {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty(),
            "package manager requires a name"
        );
        for (operation, template) in [
            ("install", &self.install),
            ("remove", &self.remove),
            ("upgrade", &self.upgrade),
        ] {
            template
                .words()
                .with_context(|| format!("{} {operation} configuration", self.name))?;
        }
        Ok(())
    }
}

impl Workspace {
    pub fn package_plan(
        &self,
        cwd: &str,
        operation: &str,
        packages: &[String],
        agent: bool,
    ) -> Result<Launch> {
        ensure!(
            packages.len() == 1,
            "usage: {operation} <package> (one package per command)"
        );
        let package = &packages[0];
        ensure!(
            !package.is_empty()
                && !package.starts_with('-')
                && !package.chars().any(char::is_control),
            "package must be a nonempty name or ID, not an option"
        );
        let manager = self.config.package_manager().context(
            "no package manager configured; edit /configs/packages.lua and enable one example",
        )?;
        ensure!(
            !agent || manager.sandbox,
            "agents cannot invoke a package manager with host access"
        );
        let template = match operation {
            "install" => &manager.install,
            "remove" | "uninstall" => &manager.remove,
            "upgrade" => &manager.upgrade,
            _ => bail!("unknown package operation: {operation}"),
        };
        let argv = template
            .words()?
            .into_iter()
            .map(|word| {
                if word == PACKAGE_TOKEN {
                    package.clone()
                } else {
                    word
                }
            })
            .collect();
        Ok(Launch {
            app: format!("package manager {}: {operation}", manager.name),
            kind: "package-manager".into(),
            argv,
            sandbox: manager.sandbox,
            network: manager.network,
            cwd: cwd.into(),
            env: Default::default(),
            runtime_mounts: Vec::new(),
            write_configs: false,
            write_source: false,
        })
    }
}
