use crate::{
    sandbox,
    shell::{Effect, Shell},
};
use anyhow::{Result, ensure};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use std::{
    collections::VecDeque,
    io::{self, IsTerminal, Write},
    time::Duration,
};

type Screen = Terminal<CrosstermBackend<io::Stdout>>;
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}

fn enter() -> Result<Screen> {
    enable_raw_mode()?;
    if let Err(error) = execute!(io::stdout(), EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error.into());
    }
    match Terminal::new(CrosstermBackend::new(io::stdout())) {
        Ok(screen) => Ok(screen),
        Err(error) => {
            restore();
            Err(error.into())
        }
    }
}
fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
}

fn color(value: &str) -> Color {
    Color::Rgb(
        u8::from_str_radix(&value[1..3], 16).unwrap(),
        u8::from_str_radix(&value[3..5], 16).unwrap(),
        u8::from_str_radix(&value[5..7], 16).unwrap(),
    )
}

fn approve_host(plan: &crate::app::Launch, interactive: bool) -> Result<bool> {
    if plan.sandbox {
        return Ok(false);
    }
    ensure!(
        interactive && io::stdin().is_terminal(),
        "host launch requires an interactive user; agents and scripted commands cannot approve it"
    );
    println!(
        "{} requests execution outside Glyph's sandbox.\nCommand: {:?}\nType 'allow host' to launch once:",
        plan.app, plan.argv
    );
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    ensure!(answer.trim() == "allow host", "host launch cancelled");
    Ok(true)
}

fn manage_key(shell: &mut Shell, change: bool, interactive: bool) -> Result<String> {
    use zeroize::Zeroizing;
    ensure!(
        interactive && !shell.agent && io::stdin().is_terminal(),
        "key management requires the user at an interactive terminal"
    );
    let storage = shell
        .storage
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("this workspace has no managed storage session"))?;
    ensure!(
        storage.encrypted() == change,
        "use key change for an encrypted store, or key set to enable encryption"
    );
    let password = Zeroizing::new(rpassword::prompt_password(
        "New encryption passphrase (at least 12 characters): ",
    )?);
    let confirm = Zeroizing::new(rpassword::prompt_password("Confirm passphrase: ")?);
    ensure!(
        *password == *confirm,
        "passphrases did not match; key was not changed"
    );
    storage.set_key(&password, change)?;
    Ok("Managed disk encrypted and saved. The passphrase is required next session. Linked host files remain external.".into())
}

fn user_action(shell: &mut Shell, action: &str, args: &[String]) -> Result<String> {
    ensure!(!shell.agent, "this operation is user-only");
    let storage = shell
        .storage
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("managed storage is required"))?;
    match action {
        "wallpaper" => crate::appearance::wallpaper(&shell.workspace, args),
        "reload" => {
            shell.workspace = crate::workspace::Workspace::open_existing(&shell.workspace.root)?;
            crate::appearance::apply_colors(&shell.workspace)?;
            Ok(
                "Lua configuration reloaded. Apps already running retain their launch permissions."
                    .into(),
            )
        }
        "agent-add" => {
            let name = args
                .first()
                .ok_or_else(|| anyhow::anyhow!("agent add NAME"))?;
            crate::package_install::register(&shell.workspace, name, true)?;
            shell.workspace.config =
                crate::config::Config::load(&shell.workspace.root.join("configs"))?;
            Ok(format!(
                "Registered {name} from its Glyph-local installation."
            ))
        }
        "editor-setup" => crate::system::setup_editor(&shell.workspace),
        "source-status" => Ok(format!(
            "Source: /system/source\nCandidate ready: {}\nRuntime: {}",
            shell
                .workspace
                .root
                .join("system/builds/candidate/glyph")
                .exists(),
            crate::system::read_manifest(&storage.paths.data)?
                .active
                .unwrap_or_else(|| "bundled".into())
        )),
        "source-build" => crate::system::build(&shell.workspace, &storage.paths.cache),
        "source-activate" => {
            storage.save_snapshot("before source activation")?;
            crate::system::activate(&shell.workspace, &storage.paths.data)
        }
        "source-rollback" => crate::system::rollback_runtime(&storage.paths.data),
        "rice" => {
            if args.first().map(String::as_str) == Some("copy") {
                let app = args
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("rice copy APP"))?;
                let host = crate::rice::host_config()
                    .ok_or_else(|| anyhow::anyhow!("host config location unavailable"))?;
                let mut skipped = Vec::new();
                let n = crate::rice::copy_app(&shell.workspace.root, &host, app, &mut skipped)?;
                Ok(format!(
                    "Copied {n} new files for {app}. Existing Glyph changes were preserved."
                ))
            } else {
                Ok(shell
                    .workspace
                    .read("/system/rice-import.json")
                    .unwrap_or_else(|_| "No host rice imported.".into()))
            }
        }
        _ => anyhow::bail!("unknown user action"),
    }
}

fn package_action(
    shell: &mut Shell,
    operation: &str,
    args: &[String],
    interactive: bool,
) -> Result<()> {
    let owned = args.first().is_some_and(|name| {
        shell
            .workspace
            .root
            .join("home/.local/share/glyph/packages")
            .join(format!("{name}.json"))
            .is_file()
    });
    let plan = shell
        .workspace
        .package_plan(&shell.cwd, operation, args, shell.agent)?;
    let host = approve_host(&plan, interactive && !shell.agent)?;
    let status = sandbox::execute(&shell.workspace, &plan, host)?;
    shell.last_exit = status.code().unwrap_or(1);
    ensure!(
        status.success(),
        "package operation failed with status {}",
        shell.last_exit
    );
    if !shell.agent && owned && ["remove", "uninstall"].contains(&operation) {
        crate::package_install::unregister(&shell.workspace, &args[0])?;
        shell.workspace.config =
            crate::config::Config::load(&shell.workspace.root.join("configs"))?;
    }
    if !shell.agent && ["install", "upgrade"].contains(&operation) {
        let package = &args[0];
        let receipt = shell
            .workspace
            .root
            .join("home/.local/share/glyph/packages")
            .join(format!("{package}.json"));
        if receipt.is_file() {
            crate::package_install::register(
                &shell.workspace,
                package,
                ["codex", "opencode"].contains(&package.as_str()),
            )?;
            shell.workspace.config =
                crate::config::Config::load(&shell.workspace.root.join("configs"))?;
            println!(
                "Registered /apps/{package}.app{}",
                if ["codex", "opencode"].contains(&package.as_str()) {
                    " and its agent configuration"
                } else {
                    ""
                }
            );
        }
    }
    Ok(())
}

pub fn process_effects(shell: &mut Shell, effects: Vec<Effect>, interactive: bool) -> Result<bool> {
    shell.last_exit = 0;
    for effect in effects {
        match effect {
            Effect::Output(output) => {
                if !output.is_empty() {
                    println!("{output}");
                }
            }
            Effect::Quit => return Ok(true),
            Effect::Package(operation, args) => {
                package_action(shell, &operation, &args, interactive)?
            }
            Effect::Store => println!("{}", crate::catalog::listing(&shell.workspace, "")?),
            Effect::UserAction(action, args) => println!("{}", user_action(shell, &action, &args)?),
            Effect::SnapshotSave(label) => {
                let id = shell.storage.as_mut().unwrap().save_snapshot(&label)?;
                println!("Saved snapshot {id}");
            }
            Effect::SnapshotRestore(id) => {
                shell.storage.as_mut().unwrap().restore_snapshot(id)?;
                println!("Snapshot {id} selected. Restart Glyph to leave emergency mode.");
                return Ok(true);
            }
            Effect::Key { change } => println!("{}", manage_key(shell, change, interactive)?),
            Effect::Launcher => {
                for (name, _) in shell.workspace.list("/apps")? {
                    println!("{name}");
                }
            }
            Effect::Launch(plan) => {
                let approved = approve_host(&plan, interactive && !shell.agent)?;
                let status = sandbox::execute(&shell.workspace, &plan, approved)?;
                shell.last_exit = status.code().unwrap_or(1);
                if !status.success() {
                    eprintln!(
                        "Glyph: application exited with status {}. Confined launches never fall back to host execution.",
                        shell.last_exit
                    );
                }
            }
        }
    }
    Ok(false)
}

pub fn plain(shell: &mut Shell) -> Result<()> {
    let interactive = io::stdin().is_terminal();
    if interactive {
        println!("GLYPH · a place for your tools\nType help to explore.");
    }
    loop {
        if interactive {
            print!("glyph {} › ", shell.cwd);
            io::stdout().flush()?;
        }
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let result = shell
            .execute(line.trim_end())
            .and_then(|effects| process_effects(shell, effects, interactive));
        match result {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => {
                shell.last_exit = 1;
                eprintln!("Glyph: {error:#}");
            }
        }
    }
    Ok(())
}

pub fn run(shell: &mut Shell) -> Result<()> {
    ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "interactive TUI requires a terminal; use --plain or --command"
    );
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        previous(info);
    }));
    let _guard = TerminalGuard;
    let mut screen = enter()?;
    let mut log = VecDeque::from([
        "Welcome to Glyph.".to_owned(),
        "Try help, ls, or run /apps/hello.app. Ctrl+Space opens your apps.".into(),
    ]);
    let mut input = String::new();
    let mut history = Vec::<String>::new();
    let mut history_index: usize = 0;
    let mut launcher: Option<usize> = None;
    loop {
        let theme = shell.workspace.config.theme();
        let bg = color(&theme.background);
        let fg = color(&theme.foreground);
        let accent = color(&theme.accent);
        let border = match theme.border.as_str() {
            "double" => BorderType::Double,
            "plain" => BorderType::Plain,
            _ => BorderType::Rounded,
        };

        let entries = shell
            .workspace
            .list("/apps")?
            .into_iter()
            .filter(|(_, dir)| !dir)
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        screen.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Block::default().style(Style::default().bg(bg).fg(fg)), area);
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(4),
                    Constraint::Length(3),
                    Constraint::Length(1),
                ])
                .split(area);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        " GLYPH ",
                        Style::default().fg(accent).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" / files · applications · tools"),
                ]))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(border)
                        .border_style(Style::default().fg(accent)),
                ),
                rows[0],
            );
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(24), Constraint::Min(10)])
                .split(rows[1]);
            let mut sidebar = vec![
                Line::from(Span::styled(" WORKSPACE", Style::default().fg(accent))),
                Line::from(""),
                Line::from(" /home"),
                Line::from(" /apps"),
                Line::from(" /configs"),
                Line::from(""),
                Line::from(Span::styled(" APPLICATIONS", Style::default().fg(accent))),
            ];
            sidebar.extend(entries.iter().map(|name| Line::from(format!(" {name}"))));
            frame.render_widget(
                Paragraph::new(sidebar).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(border)
                        .title(" Places "),
                ),
                columns[0],
            );
            let height = columns[1].height.saturating_sub(2) as usize;
            let visible = log
                .iter()
                .skip(log.len().saturating_sub(height))
                .map(|text| Line::from(text.as_str()))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(visible).wrap(Wrap { trim: false }).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(border)
                        .title(format!(" {} ", shell.cwd)),
                ),
                columns[1],
            );
            let width = rows[2].width.saturating_sub(5) as usize;
            let shown = input
                .chars()
                .rev()
                .take(width)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>();
            frame.render_widget(
                Paragraph::new(format!("› {shown}")).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_type(border)
                        .border_style(Style::default().fg(accent)),
                ),
                rows[2],
            );
            frame.render_widget(
                Paragraph::new(format!(
                    " Ctrl+Space apps   ↑↓ history   Ctrl+D exit   status {}",
                    shell.last_exit
                ))
                .style(Style::default().fg(accent)),
                rows[3],
            );
            frame.set_cursor_position((
                rows[2].x + 3 + shown.chars().count().min(width) as u16,
                rows[2].y + 1,
            ));
            if let Some(selected) = launcher {
                let popup = ratatui::layout::Rect::new(
                    area.width / 6,
                    area.height / 5,
                    area.width * 2 / 3,
                    (entries.len() as u16 + 2).min(area.height * 3 / 5).max(3),
                );
                frame.render_widget(Clear, popup);
                let items = entries
                    .iter()
                    .enumerate()
                    .map(|(i, name)| {
                        ListItem::new(format!("{} {name}", if i == selected { "›" } else { " " }))
                            .style(if i == selected {
                                Style::default().fg(bg).bg(accent)
                            } else {
                                Style::default().fg(fg).bg(bg)
                            })
                    })
                    .collect::<Vec<_>>();
                frame.render_widget(
                    List::new(items).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(border)
                            .title(" Applications · Enter to launch ")
                            .style(Style::default().fg(fg).bg(bg)),
                    ),
                    popup,
                );
            }
        })?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let mut effects = None;
        if let Some(selected) = launcher {
            match key.code {
                KeyCode::Esc => launcher = None,
                KeyCode::Down => launcher = Some((selected + 1) % entries.len().max(1)),
                KeyCode::Up => {
                    launcher =
                        Some((selected + entries.len().saturating_sub(1)) % entries.len().max(1))
                }
                KeyCode::Enter => {
                    if let Some(app) = entries.get(selected) {
                        effects = Some(shell.execute(&format!("run {app}")));
                    }
                    launcher = None;
                }
                _ => {}
            }
        } else if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('d') {
            break;
        } else if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            input.clear();
        } else {
            let binding = binding_name(key);
            if let Some(actions) = shell.workspace.config.invoke(&binding, Vec::new(), true)? {
                effects = Some(shell.actions(actions));
            } else {
                match key.code {
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        input.push(c)
                    }
                    KeyCode::Backspace => {
                        input.pop();
                    }
                    KeyCode::Up if !history.is_empty() => {
                        history_index = history_index.saturating_sub(1);
                        input = history[history_index].clone();
                    }
                    KeyCode::Down if !history.is_empty() => {
                        history_index = (history_index + 1).min(history.len());
                        input = history.get(history_index).cloned().unwrap_or_default();
                    }
                    KeyCode::Enter => {
                        let line = std::mem::take(&mut input);
                        log.push_back(format!("{} › {line}", shell.cwd));
                        if !line.trim().is_empty() {
                            history.push(line.clone());
                            history_index = history.len();
                        }
                        effects = Some(shell.execute(&line));
                    }
                    _ => {}
                }
            }
        }
        if let Some(result) = effects {
            match result {
                Err(error) => {
                    shell.last_exit = 1;
                    log.push_back(format!("{error:#}"));
                }
                Ok(effects) => {
                    for effect in effects {
                        match effect {
                            Effect::Quit => return Ok(()),
                            Effect::Package(operation, args) => {
                                restore();
                                let result = package_action(shell, &operation, &args, true);
                                screen = enter()?;
                                if let Err(error) = result {
                                    log.push_back(format!("{error:#}"));
                                }
                            }
                            Effect::Store => {
                                if let Some(effect) = store_menu(&mut screen, shell)? {
                                    restore();
                                    let result = process_effects(shell, vec![effect], true);
                                    screen = enter()?;
                                    if let Err(error) = result {
                                        log.push_back(format!("{error:#}"));
                                    }
                                }
                            }
                            Effect::UserAction(action, args) => {
                                restore();
                                let result = user_action(shell, &action, &args);
                                screen = enter()?;
                                match result {
                                    Ok(message) => log.extend(message.lines().map(str::to_owned)),
                                    Err(error) => {
                                        shell.last_exit = 1;
                                        log.push_back(format!("{error:#}"));
                                    }
                                }
                            }
                            Effect::SnapshotSave(label) => {
                                let id = shell.storage.as_mut().unwrap().save_snapshot(&label)?;
                                log.push_back(format!("Saved snapshot {id}"));
                            }
                            Effect::SnapshotRestore(id) => {
                                shell.storage.as_mut().unwrap().restore_snapshot(id)?;
                                return Ok(());
                            }
                            Effect::Key { change } => {
                                restore();
                                let result = manage_key(shell, change, true);
                                screen = enter()?;
                                match result {
                                    Ok(message) => {
                                        shell.last_exit = 0;
                                        log.push_back(message);
                                    }
                                    Err(error) => {
                                        shell.last_exit = 1;
                                        log.push_back(format!("{error:#}"));
                                    }
                                }
                            }
                            Effect::Launcher => launcher = Some(0),
                            Effect::Output(output) => {
                                log.extend(output.lines().map(str::to_owned));
                                shell.last_exit = 0;
                            }
                            Effect::Launch(plan) => {
                                restore();
                                let launched =
                                    approve_host(&plan, !shell.agent).and_then(|approved| {
                                        sandbox::execute(&shell.workspace, &plan, approved)
                                    });
                                screen = enter()?;
                                match launched {
                                    Ok(status) => {
                                        shell.last_exit = status.code().unwrap_or(1);
                                        log.push_back(format!(
                                            "{} exited with status {}",
                                            plan.app, shell.last_exit
                                        ));
                                    }
                                    Err(error) => {
                                        shell.last_exit = 1;
                                        log.push_back(format!("{error:#}"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        while log.len() > 1000 {
            log.pop_front();
        }
    }
    Ok(())
}

fn binding_name(key: KeyEvent) -> String {
    let mut name = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        name.push_str("Ctrl+");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        name.push_str("Alt+");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) && !matches!(key.code, KeyCode::Char(_)) {
        name.push_str("Shift+");
    }
    name.push_str(&match key.code {
        KeyCode::Char(' ' | '\0') => "Space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::F(n) => format!("F{n}"),
        _ => format!("{:?}", key.code),
    });
    name
}

fn store_menu(screen: &mut Screen, shell: &Shell) -> Result<Option<Effect>> {
    let all = crate::catalog::entries(&shell.workspace)?;
    let mut query = String::new();
    let mut selected = 0usize;
    let mut confirm = false;
    loop {
        let filtered = all
            .iter()
            .filter(|e| {
                format!("{} {} {}", e.name, e.category, e.description)
                    .to_lowercase()
                    .contains(&query.to_lowercase())
            })
            .collect::<Vec<_>>();
        selected = selected.min(filtered.len().saturating_sub(1));
        screen.draw(|frame|{
            let area=frame.area();frame.render_widget(Clear,area);
            let rows=Layout::vertical([Constraint::Length(3),Constraint::Min(4),Constraint::Length(4)]).split(area);
            frame.render_widget(Paragraph::new(format!(" Search: {query}")).block(Block::bordered().title(" Glyph Store · 100 tools, your choices ")),rows[0]);
            let height=rows[1].height.saturating_sub(2)as usize;let start=selected.saturating_sub(height.saturating_sub(1));
            let items=filtered.iter().enumerate().skip(start).take(height).map(|(i,e)|{
                let installed=crate::catalog::app_name(&e.command).is_some_and(|n|shell.workspace.root.join("apps").join(n).is_file());
                ListItem::new(format!(" {} {:<10} {:<20} {:<15} {}",if i==selected{"›"}else{" "},if installed{"INSTALLED"}else if e.package=="-"{"EXTERNAL"}else{"AVAILABLE"},e.name,e.category,e.description)).style(if i==selected{Style::default().fg(Color::Black).bg(Color::Rgb(214,163,93))}else{Style::default()})
            }).collect::<Vec<_>>();frame.render_widget(List::new(items).block(Block::bordered().title(" Applications ")),rows[1]);
            let message=if confirm{filtered.get(selected).map(|e|format!("Install {} using the configured package manager?\nPackage: {} · {}\nY confirms · Esc cancels",e.name,e.package,shell.workspace.config.package_manager().map(|p|p.name).unwrap_or_else(||"none".into()))).unwrap_or_default()}else{"Type to search · ↑/↓ select · Enter runs an installed app\nF2 previews installation · Esc returns. AVAILABLE is catalog metadata, not a compatibility promise.".into()};
            frame.render_widget(Paragraph::new(message).block(Block::bordered()),rows[2]);
        })?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if confirm {
                match key.code {
                    KeyCode::Char('y' | 'Y') => {
                        ensure!(!shell.agent, "catalog installation is user-only");
                        if let Some(entry) = filtered.get(selected) {
                            ensure!(
                                entry.package != "-",
                                "this catalog entry needs a custom installer"
                            );
                            return Ok(Some(Effect::Package(
                                "install".into(),
                                vec![entry.package.clone()],
                            )));
                        }
                    }
                    KeyCode::Esc => confirm = false,
                    _ => {}
                }
                continue;
            }
            match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Down => selected = (selected + 1).min(filtered.len().saturating_sub(1)),
                KeyCode::Enter => {
                    if let Some(entry) = filtered.get(selected) {
                        if let Some(name) = crate::catalog::app_name(&entry.command) {
                            let path = format!("/apps/{name}");
                            if shell
                                .workspace
                                .root
                                .join(path.trim_start_matches('/'))
                                .is_file()
                            {
                                return Ok(Some(Effect::Launch(shell.workspace.launch_plan(
                                    &shell.cwd,
                                    &path,
                                    &[],
                                    shell.agent,
                                )?)));
                            }
                        }
                    }
                }
                KeyCode::Char('i') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    confirm = true
                }
                KeyCode::F(2) => confirm = true,
                KeyCode::Backspace => {
                    query.pop();
                    selected = 0;
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    selected = 0;
                }
                _ => {}
            }
        }
    }
}
