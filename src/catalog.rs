use crate::workspace::Workspace;
use anyhow::{Result, ensure};
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub category: String,
    pub package: String,
    pub command: String,
    pub description: String,
}
pub fn entries(workspace: &Workspace) -> Result<Vec<Entry>> {
    let source = workspace.read("/configs/catalog.tsv")?;
    let mut entries = Vec::new();
    for line in source
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let fields = line.split('\t').collect::<Vec<_>>();
        ensure!(
            fields.len() == 6,
            "catalog rows need rank, name, category, package, command, description"
        );
        entries.push(Entry {
            name: fields[1].into(),
            category: fields[2].into(),
            package: fields[3].into(),
            command: fields[4].into(),
            description: fields[5].into(),
        });
    }
    Ok(entries)
}
pub fn app_name(command: &str) -> Option<String> {
    let name = command.split_whitespace().next()?;
    (!name.is_empty()
        && name != "-"
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)))
    .then(|| format!("{name}.app"))
}
pub fn listing(workspace: &Workspace, query: &str) -> Result<String> {
    Ok(entries(workspace)?
        .into_iter()
        .filter(|e| {
            format!("{} {} {}", e.name, e.category, e.description)
                .to_lowercase()
                .contains(&query.to_lowercase())
        })
        .map(|e| {
            let installed =
                app_name(&e.command).is_some_and(|n| workspace.root.join("apps").join(n).is_file());
            format!(
                "{:<12} {:<20} {:<16} {}",
                if installed { "INSTALLED" } else { "AVAILABLE" },
                e.name,
                e.category,
                e.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
pub fn capability(workspace: &Workspace, name: &str) -> Option<String> {
    let source = workspace.read("/configs/capabilities.tsv").ok()?;
    source
        .lines()
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| {
            let f = l.split('\t').collect::<Vec<_>>();
            (f.len() == 5 && f[0] == name)
                .then(|| app_name(f[3]))
                .flatten()
        })
}
