use crate::config::{Config, Link};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub const DEFAULT_CONFIG: &str = include_str!("../assets/init.lua");
pub const DEFAULT_AGENT_GUIDE: &str = include_str!("../assets/AGENTS.md");

pub struct Workspace {
    pub root: PathBuf,
    pub config: Config,
    pub links: Vec<Link>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Mount {
    pub source: PathBuf,
    pub target: String,
    pub writable: bool,
}

pub fn normalize(cwd: &str, input: &str) -> Result<String> {
    ensure!(!input.contains('\0'), "paths cannot contain NUL");
    let absolute = if input.starts_with('/') {
        input.to_owned()
    } else {
        format!("{cwd}/{input}")
    };
    let mut parts = Vec::new();
    for part in absolute.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

impl Workspace {
    pub fn open_existing(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let config = Config::load(&root.join("configs"))?;
        let links = config.links();
        let workspace = Self {
            root,
            config,
            links,
        };
        workspace.validate_links()?;
        Ok(workspace)
    }
    pub fn emergency(root: &Path) -> Result<Self> {
        let temporary = tempfile::tempdir()?;
        let defaults = Self::open(temporary.path())?;
        Ok(Self {
            root: root.canonicalize()?,
            config: defaults.config,
            links: Vec::new(),
        })
    }
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        for dir in [
            "configs",
            "configs/themes",
            "configs/links",
            "configs/apps",
            "configs/agents",
            "configs/handlers",
            "configs/user",
            "configs/editor",
            "system",
            "system/source",
            "system/builds",
            "system/docs",
            "apps",
            "home",
            "home/.cache",
        ] {
            let path = root.join(dir);
            fs::create_dir_all(&path)?;
            ensure!(
                path.canonicalize()?.starts_with(&root),
                "workspace directory escapes its root: {dir}"
            );
            ensure!(
                !fs::symlink_metadata(&path)?.is_symlink(),
                "workspace directories cannot be symlinks: {dir}"
            );
        }
        seed(&root.join("configs/init.lua"), DEFAULT_CONFIG)?;
        seed(
            &root.join("configs/editor.lua"),
            include_str!("../assets/editor.lua"),
        )?;
        seed(&root.join("AGENTS.md"), DEFAULT_AGENT_GUIDE)?;
        seed(&root.join("home/AGENTS.md"), DEFAULT_AGENT_GUIDE)?;
        seed(
            &root.join("configs/packages.lua"),
            include_str!("../assets/packages.lua"),
        )?;
        seed(
            &root.join("apps/hello.app"),
            include_str!("../assets/hello.app"),
        )?;
        seed(
            &root.join("configs/handlers/flatpak.py"),
            include_str!("../assets/flatpak.py"),
        )?;
        seed(
            &root.join("home/welcome.txt"),
            "Welcome to Glyph.\nYour files live here. Try: run /apps/hello.app\n",
        )?;
        let config = Config::load(&root.join("configs"))?;
        let links = config.links();
        let workspace = Self {
            root,
            config,
            links,
        };
        workspace.validate_links()?;
        for link in &workspace.links {
            let target = Path::new(link.target.trim_start_matches('/'));
            reject_symlinks(&workspace.root, target)?;
            fs::create_dir_all(workspace.root.join(target.parent().unwrap()))?;
        }
        Ok(workspace)
    }

    fn validate_links(&self) -> Result<()> {
        let mut targets = Vec::<String>::new();
        for link in &self.links {
            ensure!(
                ["read", "read-write"].contains(&link.access.as_str()),
                "link access must be read or read-write"
            );
            let target = normalize("/", &link.target)?;
            ensure!(
                target == link.target
                    && target.starts_with("/home/")
                    && target != "/home/.cache"
                    && !target.starts_with("/home/.cache/")
                    && target != "/home/AGENTS.md"
                    && !target.starts_with("/home/AGENTS.md/"),
                "link targets must be canonical paths beneath /home and cannot replace the protected agent guide"
            );
            ensure!(
                !targets.iter().any(|old| target == *old
                    || target.starts_with(&format!("{old}/"))
                    || old.starts_with(&format!("{target}/"))),
                "link targets cannot overlap"
            );
            targets.push(target);
            let source = Path::new(&link.source);
            ensure!(
                source.is_absolute(),
                "link sources must be absolute host paths"
            );
            ensure!(
                source.canonicalize()? == source,
                "link sources must be canonical and contain no symlink aliases"
            );
            ensure!(
                !self.root.starts_with(source) && !source.starts_with(&self.root),
                "links cannot expose Glyph's storage or configuration"
            );
            ensure!(
                source.is_file() || source.is_dir(),
                "link source must be a regular file or directory"
            );
            if source.is_dir() {
                Self::check_link_tree(source, link.recursive)?;
            }
        }
        Ok(())
    }

    fn check_link_tree(source: &Path, recursive: bool) -> Result<()> {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            ensure!(
                !kind.is_symlink(),
                "linked trees cannot contain symlinks: {}",
                entry.path().display()
            );
            ensure!(
                kind.is_file() || kind.is_dir(),
                "linked trees cannot expose special files"
            );
            if recursive && kind.is_dir() {
                Self::check_link_tree(&entry.path(), true)?;
            }
        }
        Ok(())
    }

    pub fn resolve(&self, path: &str, write: bool) -> Result<PathBuf> {
        let path = normalize("/", path)?;
        ensure!(
            !write || (path != "/AGENTS.md" && path != "/home/AGENTS.md"),
            "agent guides are read-only inside Glyph; the user can edit them outside the sandbox"
        );
        for link in &self.links {
            if path == link.target || path.starts_with(&format!("{}/", link.target)) {
                ensure!(
                    !write || link.access == "read-write",
                    "read-only host link: {}",
                    link.target
                );
                let tail = path
                    .strip_prefix(&link.target)
                    .unwrap()
                    .trim_start_matches('/');
                let source = Path::new(&link.source);
                ensure!(
                    source.canonicalize()? == source,
                    "host link source changed or became a symlink; restart after fixing its configuration"
                );
                if !tail.is_empty() {
                    ensure!(source.is_dir(), "cannot traverse a file link");
                    if !link.recursive {
                        ensure!(
                            !tail.contains('/'),
                            "link does not allow recursive traversal"
                        );
                        ensure!(
                            !source.join(tail).is_dir(),
                            "link does not expose subdirectories"
                        );
                    }
                }
                let resolved = if tail.is_empty() {
                    source.to_path_buf()
                } else {
                    source.join(tail)
                };
                reject_symlinks(source, Path::new(tail))?;
                return Ok(resolved);
            }
        }
        if write {
            ensure!(
                path.starts_with("/home/"),
                "write commands are restricted to /home; edit trusted configuration outside Glyph"
            );
        }
        let relative = Path::new(path.trim_start_matches('/'));
        reject_symlinks(&self.root, relative)?;
        Ok(self.root.join(relative))
    }

    pub fn list(&self, path: &str) -> Result<Vec<(String, bool)>> {
        let real = self.resolve(path, false)?;
        ensure!(real.is_dir(), "not a directory: {path}");
        let shallow_link = self.links.iter().any(|l| l.target == path && !l.recursive);
        let mut entries = Vec::new();
        for entry in fs::read_dir(real)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() || (shallow_link && kind.is_dir()) {
                continue;
            }
            entries.push((
                entry.file_name().to_string_lossy().into_owned(),
                kind.is_dir(),
            ));
        }
        for link in &self.links {
            let parent = Path::new(&link.target).parent().unwrap().to_string_lossy();
            if parent == path {
                entries.push((
                    Path::new(&link.target)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    Path::new(&link.source).is_dir(),
                ));
            } else if link
                .target
                .starts_with(&format!("{}/", path.trim_end_matches('/')))
            {
                let tail = link
                    .target
                    .trim_start_matches(path.trim_end_matches('/'))
                    .trim_start_matches('/');
                if let Some(first) = tail.split('/').next() {
                    entries.push((first.into(), true));
                }
            }
        }
        entries.sort();
        entries.dedup();
        Ok(entries)
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.resolve(path, false).is_ok_and(|p| p.is_dir())
            || self.links.iter().any(|l| {
                l.target
                    .starts_with(&format!("{}/", path.trim_end_matches('/')))
            })
    }

    pub fn mounts(&self) -> Result<Vec<Mount>> {
        self.validate_links()?;
        let mut mounts = Vec::new();
        for link in &self.links {
            let source = PathBuf::from(&link.source);
            let writable = link.access == "read-write";
            if source.is_dir() && !link.recursive {
                for entry in fs::read_dir(source)? {
                    let entry = entry?;
                    if entry.file_type()?.is_file() {
                        mounts.push(Mount {
                            source: entry.path(),
                            target: format!(
                                "{}/{}",
                                link.target,
                                entry.file_name().to_string_lossy()
                            ),
                            writable,
                        });
                    }
                }
            } else {
                mounts.push(Mount {
                    source,
                    target: link.target.clone(),
                    writable,
                });
            }
        }
        Ok(mounts)
    }

    pub fn read(&self, path: &str) -> Result<String> {
        fs::read_to_string(self.resolve(path, false)?).with_context(|| format!("reading {path}"))
    }
    pub fn write(&self, path: &str, text: &str) -> Result<()> {
        fs::write(self.resolve(path, true)?, text).with_context(|| format!("writing {path}"))
    }
}

fn reject_symlinks(base: &Path, relative: &Path) -> Result<()> {
    let mut cursor = base.to_path_buf();
    for part in relative.components() {
        if let Component::Normal(part) = part {
            cursor.push(part);
            match fs::symlink_metadata(&cursor) {
                Ok(meta) => ensure!(
                    !meta.file_type().is_symlink(),
                    "symlink traversal is not allowed: {}",
                    cursor.display()
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        } else {
            bail!("invalid workspace path");
        }
    }
    Ok(())
}

fn seed(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(contents.as_bytes())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::symlink_metadata(path)?.is_file(),
                "workspace seed files must be regular files, not symlinks: {}",
                path.display()
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
