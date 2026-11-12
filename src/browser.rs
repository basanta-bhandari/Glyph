use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use crossterm::{
    cursor::MoveTo,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Engine {
    child: Child,
    input: fs::File,
    output: BufReader<fs::File>,
    next: u64,
}
fn pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    ensure!(
        unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0,
        "cannot create browser pipe"
    );
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}
impl Engine {
    pub fn start(profile: &Path, no_sandbox: bool) -> Result<Self> {
        fs::create_dir_all(profile)?;
        let engine=std::env::var_os("GLYPH_BROWSER_ENGINE").map(PathBuf::from).or_else(||["/usr/lib/chromium/chromium","/usr/bin/chromium","/opt/google/chrome/chrome"].into_iter().map(PathBuf::from).find(|p|p.is_file())).context("Chromium/Chrome runtime is required; set GLYPH_BROWSER_ENGINE in browser app configuration")?;
        let (read, child_write) = pipe()?;
        let (child_read, write) = pipe()?;
        let fd3 = unsafe { libc::fcntl(child_read.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        let fd4 = unsafe { libc::fcntl(child_write.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        ensure!(fd3 >= 0 && fd4 >= 0, "cannot prepare browser control pipes");
        let child_read = unsafe { OwnedFd::from_raw_fd(fd3) };
        let child_write = unsafe { OwnedFd::from_raw_fd(fd4) };
        let log = fs::File::create(profile.join("engine.log"))?;
        let mut command = Command::new(engine);
        let engine_config = profile.join("engine-config");
        let engine_cache = profile.join("engine-cache");
        fs::create_dir_all(&engine_config)?;
        fs::create_dir_all(&engine_cache)?;
        command
            .env("XDG_CONFIG_HOME", &engine_config)
            .env("XDG_CACHE_HOME", &engine_cache);
        command.args([
            "--headless=new",
            "--remote-debugging-pipe",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-dev-shm-usage",
            "--disable-background-networking",
            "--disable-breakpad",
        ]);
        command
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        if no_sandbox {
            command.arg("--no-sandbox");
        }
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(child_read.as_raw_fd(), 3) < 0
                    || libc::dup2(child_write.as_raw_fd(), 4) < 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(command);
        let mut engine = Self {
            child,
            input: fs::File::from(write),
            output: BufReader::new(fs::File::from(read)),
            next: 0,
        };
        engine
            .rpc(None, "Browser.getVersion", json!({}))
            .context("Chromium failed to start; inspect engine.log in the browser profile")?;
        Ok(engine)
    }
    pub fn rpc(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        let mut message = json!({"id":id,"method":method,"params":params});
        if let Some(session) = session {
            message["sessionId"] = session.into();
        }
        self.input.write_all(&serde_json::to_vec(&message)?)?;
        self.input.write_all(&[0])?;
        self.input.flush()?;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            ensure!(
                Instant::now() < deadline,
                "browser engine response timed out during {method}"
            );
            if self.output.buffer().is_empty() {
                let mut fd = libc::pollfd {
                    fd: self.output.get_ref().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                ensure!(
                    unsafe { libc::poll(&mut fd, 1, 20000) } > 0,
                    "browser engine response timed out during {method}"
                );
            }
            let mut bytes = Vec::new();
            ensure!(
                self.output.read_until(0, &mut bytes)? > 0,
                "browser engine closed its control pipe during {method}"
            );
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            let reply: Value = serde_json::from_slice(&bytes)?;
            if reply["id"].as_u64() == Some(id) {
                ensure!(
                    reply.get("error").is_none(),
                    "browser engine rejected {method}: {}",
                    reply["error"]
                );
                return Ok(reply["result"].clone());
            }
        }
    }
    fn tab(&mut self, url: &str) -> Result<String> {
        let target =
            self.rpc(None, "Target.createTarget", json!({"url":"about:blank"}))?["targetId"]
                .as_str()
                .context("missing target")?
                .to_owned();
        let session = self.rpc(
            None,
            "Target.attachToTarget",
            json!({"targetId":target,"flatten":true}),
        )?["sessionId"]
            .as_str()
            .context("missing browser session")?
            .to_owned();
        self.rpc(Some(&session), "Page.enable", json!({}))?;
        self.rpc(Some(&session), "Page.navigate", json!({"url":url}))?;
        Ok(session)
    }
    pub fn screenshot(&mut self, session: &str) -> Result<String> {
        Ok(self.rpc(
            Some(session),
            "Page.captureScreenshot",
            json!({"format":"png","captureBeyondViewport":false}),
        )?["data"]
            .as_str()
            .context("missing image")?
            .to_owned())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self
            .input
            .write_all(b"{\"id\":999999,\"method\":\"Browser.close\"}\0");
        let _ = self.input.flush();
        for _ in 0..20 {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Screen;
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            std::io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        let _ = write!(std::io::stdout(), "\x1b_Ga=d,d=i,i=19,q=2;\x1b\\");
    }
}
fn dimensions() -> (u16, u16, u32, u32) {
    let (cols, rows) = terminal::size().unwrap_or((120, 40));
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut size) };
    let width = if size.ws_xpixel > 0 {
        size.ws_xpixel as u32
    } else {
        cols as u32 * 9
    };
    let height = if size.ws_ypixel > 0 {
        size.ws_ypixel as u32 * (rows.saturating_sub(3)) as u32 / rows.max(1) as u32
    } else {
        rows.saturating_sub(3) as u32 * 18
    };
    (cols, rows, width.max(100), height.max(100))
}
fn metrics(engine: &mut Engine, session: &str) -> Result<()> {
    let (_, _, width, height) = dimensions();
    engine.rpc(
        Some(session),
        "Emulation.setDeviceMetricsOverride",
        json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":false}),
    )?;
    Ok(())
}
fn render(
    engine: &mut Engine,
    session: &str,
    url: &str,
    address: bool,
    status: &str,
) -> Result<()> {
    let (cols, rows, _, _) = dimensions();
    execute!(
        std::io::stdout(),
        MoveTo(0, 0),
        Clear(ClearType::CurrentLine)
    )?;
    print!(
        "\x1b[38;2;214;163;93m GLYPH WEB \x1b[0m {}{}",
        if address { "› " } else { "  " },
        url.chars()
            .take(cols.saturating_sub(16) as usize)
            .collect::<String>()
    );
    execute!(std::io::stdout(), MoveTo(0, 1))?;
    let image = engine.screenshot(session)?;
    for (index, chunk) in image.as_bytes().chunks(4096).enumerate() {
        let more = usize::from((index + 1) * 4096 < image.len());
        let controls = if index == 0 {
            format!(
                "a=T,f=100,i=19,c={cols},r={},q=2,m={more}",
                rows.saturating_sub(3)
            )
        } else {
            format!("m={more}")
        };
        print!("\x1b_G{controls};{}\x1b\\", std::str::from_utf8(chunk)?);
    }
    execute!(
        std::io::stdout(),
        MoveTo(0, rows.saturating_sub(1)),
        Clear(ClearType::CurrentLine)
    )?;
    print!(
        " Ctrl+L URL · Ctrl+T tab · Ctrl+Tab next · Alt+←/→ history · Ctrl+R reload · Ctrl+Q exit {status}"
    );
    std::io::stdout().flush()?;
    Ok(())
}
fn navigate(engine: &mut Engine, session: &str, value: &str) -> Result<String> {
    let url = if value.contains("://") || value.starts_with("about:") || value.starts_with("data:")
    {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    engine.rpc(Some(session), "Page.navigate", json!({"url":url}))?;
    Ok(url)
}
pub fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let test = args.iter().any(|a| a == "--engine-test");
    let option = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let profile = option("--profile").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/home".into()))
            .join(".local/share/glyph/browser")
    });
    let no_sandbox = std::env::var("GLYPH_BROWSER_ENGINE_SANDBOX").as_deref() == Ok("off") || test;
    ensure!(
        !no_sandbox || test || std::env::var("GLYPH_SANDBOX").as_deref() == Ok("1"),
        "disabling Chromium's nested sandbox is allowed only inside Glyph's outer sandbox"
    );
    let mut engine = Engine::start(&profile, no_sandbox)?;
    let initial=option("--url").or_else(||args.iter().find(|a|a.starts_with("http://")||a.starts_with("https://")).cloned()).unwrap_or_else(||"data:text/html,<html><body style='background:%23181818;color:%23e8d8b8;font:24px monospace;padding:7vw'><h1>Glyph Web.</h1><p>The web, with actual pixels. Fancy that.</p><p>Ctrl%2BL opens a URL. Ctrl%2BQ brings you home.</p></body></html>".into());
    let mut tabs = vec![(engine.tab(&initial)?, initial)];
    let mut active = 0;
    metrics(&mut engine, &tabs[0].0)?;
    let downloads = if test {
        profile.join("test-downloads")
    } else {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/home".into())).join("Downloads")
    };
    fs::create_dir_all(&downloads)?;
    engine.rpc(
        None,
        "Browser.setDownloadBehavior",
        json!({"behavior":"allow","downloadPath":downloads}),
    )?;
    if test {
        let session = &tabs[0].0;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ready=engine.rpc(Some(session),"Runtime.evaluate",json!({"expression":"document.readyState === 'complete' && !!document.body","returnByValue":true}))?;
            if ready["result"]["value"] == true {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "test page did not finish loading"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        if args.iter().any(|a| a == "--interaction-test") {
            engine.rpc(Some(session),"Runtime.evaluate",json!({"expression":"document.body.innerHTML='<h1>Glyph Web</h1><p>A real graphical page, rendered by Chromium.</p><input id=field><button id=button onclick=\"this.textContent=String(Number(this.textContent||0)+1)\">0</button>'; document.querySelector('input').focus()"}))?;
            engine.rpc(
                Some(session),
                "Input.insertText",
                json!({"text":"Glyph input works"}),
            )?;
            let rect=engine.rpc(Some(session),"Runtime.evaluate",json!({"expression":"JSON.stringify((()=>{let r=document.querySelector('button').getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2}})())","returnByValue":true}))?;
            let point: Value = serde_json::from_str(
                rect["result"]["value"]
                    .as_str()
                    .context("missing button position")?,
            )?;
            for kind in ["mousePressed", "mouseReleased"] {
                engine.rpc(Some(session),"Input.dispatchMouseEvent",json!({"type":kind,"x":point["x"],"y":point["y"],"button":"left","clickCount":1}))?;
            }
            let check=engine.rpc(Some(session),"Runtime.evaluate",json!({"expression":"document.querySelector('input').value+'|'+document.querySelector('button').textContent","returnByValue":true}))?;
            ensure!(
                check["result"]["value"].as_str() == Some("Glyph input works|1"),
                "graphical browser input bridge failed"
            );
        }
        let result = engine.rpc(
            Some(session),
            "Runtime.evaluate",
            json!({"expression":"document.title","returnByValue":true}),
        )?;
        let output = option("--output").context("--engine-test requires --output PNG")?;
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(engine.screenshot(session)?)?;
        fs::write(&output, bytes)?;
        println!(
            "{}",
            json!({"status":"ok","title":result["result"]["value"],"png":output})
        );
        return Ok(());
    }
    ensure!(
        std::env::var("TERM").unwrap_or_default().contains("kitty"),
        "Glyph Web requires Kitty's graphical terminal protocol"
    );
    terminal::enable_raw_mode()?;
    execute!(
        std::io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        crossterm::cursor::Hide
    )?;
    let _screen = Screen;
    let mut address = false;
    let mut url = String::new();
    let mut status = String::new();
    let mut dirty = true;
    let mut last = Instant::now() - Duration::from_secs(1);
    loop {
        let session = tabs[active].0.clone();
        if dirty || last.elapsed() > Duration::from_millis(500) {
            render(
                &mut engine,
                &session,
                if address { &url } else { &tabs[active].1 },
                address,
                &status,
            )?;
            last = Instant::now();
            dirty = false;
        }
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        match event::read()? {
            Event::Resize(_, _) => {
                metrics(&mut engine, &session)?;
                dirty = true;
            }
            Event::Paste(text) => {
                if address {
                    url.push_str(&text);
                } else {
                    engine.rpc(Some(&session), "Input.insertText", json!({"text":text}))?;
                }
                dirty = true;
            }
            Event::Mouse(mouse) => {
                if mouse.row == 0 {
                    address = true;
                    url = tabs[active].1.clone();
                    dirty = true;
                    continue;
                }
                let (cols, rows, width, height) = dimensions();
                let x = mouse.column as f64 * width as f64 / cols.max(1) as f64;
                let y = mouse.row.saturating_sub(1) as f64 * height as f64
                    / rows.saturating_sub(3).max(1) as f64;
                match mouse.kind {
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                        engine.rpc(Some(&session),"Input.dispatchMouseEvent",json!({"type":"mouseWheel","x":x,"y":y,"deltaX":0,"deltaY":if mouse.kind==MouseEventKind::ScrollDown{140}else{-140}}))?;
                    }
                    MouseEventKind::Down(button) | MouseEventKind::Up(button) => {
                        let down = matches!(mouse.kind, MouseEventKind::Down(_));
                        engine.rpc(Some(&session),"Input.dispatchMouseEvent",json!({"type":if down{"mousePressed"}else{"mouseReleased"},"x":x,"y":y,"button":match button{MouseButton::Left=>"left",MouseButton::Right=>"right",_=>"middle"},"clickCount":1}))?;
                    }
                    MouseEventKind::Drag(_) => {
                        engine.rpc(
                            Some(&session),
                            "Input.dispatchMouseEvent",
                            json!({"type":"mouseMoved","x":x,"y":y,"button":"left","buttons":1}),
                        )?;
                    }
                    _ => {}
                }
                dirty = true;
            }
            Event::Key(key) if key.kind == event::KeyEventKind::Press => {
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                let alt = key.modifiers.contains(KeyModifiers::ALT);
                if ctrl && key.code == KeyCode::Char('q') {
                    break;
                }
                if ctrl && key.code == KeyCode::Char('l') {
                    address = true;
                    url.clear();
                    dirty = true;
                    continue;
                }
                if ctrl && key.code == KeyCode::Char('t') {
                    let page = engine.tab("about:blank")?;
                    tabs.push((page, "about:blank".into()));
                    active = tabs.len() - 1;
                    metrics(&mut engine, &tabs[active].0)?;
                    address = true;
                    url.clear();
                    dirty = true;
                    continue;
                }
                if ctrl && key.code == KeyCode::Char('w') {
                    if tabs.len() == 1 {
                        break;
                    }
                    engine.rpc(Some(&session), "Page.close", json!({}))?;
                    tabs.remove(active);
                    active = active.min(tabs.len() - 1);
                    dirty = true;
                    continue;
                }
                if ctrl && key.code == KeyCode::Tab {
                    active = (active + 1) % tabs.len();
                    metrics(&mut engine, &tabs[active].0)?;
                    dirty = true;
                    continue;
                }
                if ctrl && key.code == KeyCode::Char('r') {
                    engine.rpc(Some(&session), "Page.reload", json!({}))?;
                    dirty = true;
                    continue;
                }
                if alt && [KeyCode::Left, KeyCode::Right].contains(&key.code) {
                    let history =
                        engine.rpc(Some(&session), "Page.getNavigationHistory", json!({}))?;
                    let index = history["currentIndex"].as_i64().unwrap_or(0)
                        + if key.code == KeyCode::Left { -1 } else { 1 };
                    if index >= 0 {
                        if let Some(entry) = history["entries"]
                            .as_array()
                            .and_then(|e| e.get(index as usize))
                        {
                            engine.rpc(
                                Some(&session),
                                "Page.navigateToHistoryEntry",
                                json!({"entryId":entry["id"]}),
                            )?;
                        }
                    }
                    dirty = true;
                    continue;
                }
                if address {
                    match key.code {
                        KeyCode::Enter => {
                            match navigate(&mut engine, &session, &url) {
                                Ok(value) => tabs[active].1 = value,
                                Err(e) => status = e.to_string(),
                            };
                            address = false;
                        }
                        KeyCode::Esc => address = false,
                        KeyCode::Backspace => {
                            url.pop();
                        }
                        KeyCode::Char(c) if !ctrl && !alt => url.push(c),
                        _ => {}
                    }
                    dirty = true;
                    continue;
                }
                let special = match key.code {
                    KeyCode::Enter => Some(("Enter", 13)),
                    KeyCode::Backspace => Some(("Backspace", 8)),
                    KeyCode::Tab => Some(("Tab", 9)),
                    KeyCode::Left => Some(("ArrowLeft", 37)),
                    KeyCode::Right => Some(("ArrowRight", 39)),
                    KeyCode::Up => Some(("ArrowUp", 38)),
                    KeyCode::Down => Some(("ArrowDown", 40)),
                    KeyCode::Esc => Some(("Escape", 27)),
                    KeyCode::Delete => Some(("Delete", 46)),
                    _ => None,
                };
                if let Some((key, code)) = special {
                    for kind in ["keyDown", "keyUp"] {
                        engine.rpc(Some(&session),"Input.dispatchKeyEvent",json!({"type":kind,"key":key,"windowsVirtualKeyCode":code,"nativeVirtualKeyCode":code,"modifiers":if ctrl{2}else{0}}))?;
                    }
                } else if let KeyCode::Char(c) = key.code {
                    if ctrl {
                        for kind in ["keyDown", "keyUp"] {
                            engine.rpc(Some(&session),"Input.dispatchKeyEvent",json!({"type":kind,"key":c.to_string(),"windowsVirtualKeyCode":c.to_ascii_uppercase()as u32,"modifiers":2}))?;
                        }
                    } else {
                        engine.rpc(
                            Some(&session),
                            "Input.insertText",
                            json!({"text":c.to_string()}),
                        )?;
                    }
                }
                dirty = true;
            }
            _ => {}
        }
    }
    Ok(())
}
