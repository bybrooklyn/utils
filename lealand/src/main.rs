//! lealand (the `ll` command): an eza-style listing you can walk around in, drawn inline under the prompt.
//! It prints where you ended up (and maybe a command) on stdout; the zsh `ll` function cds and runs it.

mod apps;
mod icons;
mod jump;
mod logo;
mod preview;

use crokey::{KeyCombination, key};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    crossterm::{
        cursor,
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
        execute,
        terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::Rect,
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};
use std::{
    cmp::Ordering,
    collections::HashMap,
    env,
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::MetadataExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant, SystemTime},
};
use unicode_width::UnicodeWidthChar;

/// Keys of a chord must come within this long of each other. The footer shows what can follow
/// while you think, and enter or space runs a chord that is waiting for a longer one.
const GAP: Duration = Duration::from_millis(1000);
/// Esc twice within this long quits.
const ESC_GAP: Duration = Duration::from_millis(400);

/// `keys action [command]`. ~/.config/lealand/chords uses the same format and wins; `xx none` removes one.
/// A chord that starts a longer one (mk, mkcl) fires when no next key comes within GAP.
const DEFAULT_CHORDS: &str = r#"
qq    quit
?     help
fu    full
op    open-with
cl    exit  claude
fi    spawn dolphin .
code  spawn code .
mk    mkdir
mkcl  mkdir-exit claude
rm    trash
tt    spawn ghostty +new-tab --working-directory="$PWD"
"#;

type Term = Terminal<CrosstermBackend<io::Stdout>>;

#[derive(Clone)]
enum Action {
    Quit,
    Help,
    Full,
    OpenWith,
    Mkdir(Option<String>),
    Trash,
    Exit(String),
    Spawn(String),
}

struct Chord {
    keys: String,
    act: Action,
}

struct Entry {
    name: String,
    path: PathBuf,
    dir: bool,
    hidden: bool,
    size: u64,
    mtime: Option<SystemTime>,
    mode: u32,
    uid: u32,
    link: Option<String>,
}

enum Mode {
    Browse,
    Help,
    Jump { q: String, at: Instant, sel: usize, hits: Vec<PathBuf> },
    Pick { file: PathBuf, mime: String, apps: Vec<apps::App>, handles: usize, filter: String, sel: usize, current: Option<PathBuf> },
    Name { text: String, then: Option<String> },
    Confirm(PathBuf),
}

struct App {
    dir: PathBuf,
    entries: Vec<Entry>,
    sel: usize,
    scroll: usize,
    rows: usize,
    back: Vec<PathBuf>,
    chords: Vec<Chord>,
    pending: String,
    pending_at: Instant,
    esc_at: Option<Instant>,
    mode: Mode,
    full: bool,
    toggle_full: bool,
    git: HashMap<String, char>,
    git_rx: Option<mpsc::Receiver<(PathBuf, HashMap<String, char>)>>,
    counts: HashMap<PathBuf, Option<usize>>,
    widths: Option<(usize, usize)>,
    preview: Option<(PathBuf, Vec<Line<'static>>)>,
    image: Option<(PathBuf, Vec<u8>, u32, u32)>,
    image_area: Rect,
    shown_image: Option<PathBuf>,
    sizes: HashMap<PathBuf, Option<u64>>,
    sizes_rx: mpsc::Receiver<(PathBuf, u64)>,
    sizes_tx: mpsc::Sender<(PathBuf, u64)>,
    msg: Option<(String, Instant)>,
    index: Arc<jump::Index>,
    uid: u32,
    users: HashMap<u32, String>,
    tty_cmd: Option<Command>,
    intro: Option<Instant>,
    cascade: Option<Instant>,
    outro: Option<Instant>,
    outro_skipped: bool,
    body_top: u16,
    click_at: Option<Instant>,
    animate: bool,
    done: bool,
    answer: Option<String>,
}

fn main() {
    // stdout is the zsh function's cd target, so everything said here goes to stderr
    let args: Vec<String> = env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => {}
        ["--disable-flag"] => {
            let f = splash_off_file();
            let _ = fs::create_dir_all(f.parent().unwrap());
            let _ = fs::write(&f, "");
            eprintln!("lealand: no launch animation from now on (ll --enable-flag brings it back)");
            return;
        }
        ["--flag"] => return show_flag(),
        ["--enable-flag"] => {
            let _ = fs::remove_file(splash_off_file());
            eprintln!("lealand: the launch animation is back");
            return;
        }
        _ => {
            eprintln!("ll takes no arguments, apart from --flag, --disable-flag and --enable-flag");
            std::process::exit(2);
        }
    }
    // stdout is the zsh function's $(...); keep it for the answer and point fd 1 at the terminal
    let answer_fd = unsafe { libc::dup(1) };
    if let Ok(tty) = fs::OpenOptions::new().read(true).write(true).open("/dev/tty") {
        unsafe { libc::dup2(tty.as_raw_fd(), 1) };
    }
    std::panic::set_hook(Box::new(|info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, cursor::Show);
        eprintln!("ll crashed: {info}");
    }));
    let dir = env::current_dir().unwrap_or_else(|_| jump::home());
    let mut app = App::new(dir);
    let result = terminal::enable_raw_mode().and_then(|_| execute!(io::stdout(), EnableMouseCapture)).and_then(|_| run(&mut app));
    let _ = execute!(io::stdout(), DisableMouseCapture);
    let _ = terminal::disable_raw_mode();
    let _ = execute!(io::stdout(), cursor::Show);
    if let Err(e) = result {
        eprintln!("ll: {e}");
        std::process::exit(1);
    }
    if let Some(answer) = app.answer {
        let _ = unsafe { File::from_raw_fd(answer_fd) }.write_all(answer.as_bytes());
    }
}

/// `ll --flag`: the wordmark, wiped in row by row, then back to the prompt. On stderr, because
/// stdout is where the zsh function looks for a folder to cd to.
fn show_flag() {
    let mut err = io::stderr().lock();
    let _ = writeln!(err);
    for row in logo::LOGO {
        let mut line = String::from("  ");
        for &(c, fg, bg) in row.iter() {
            if c == ' ' {
                line += "\x1b[0m ";
                continue;
            }
            line += &format!("\x1b[38;2;{};{};{}m", fg >> 16, (fg >> 8) & 255, fg & 255);
            line += &match bg {
                Some(b) => format!("\x1b[48;2;{};{};{}m", b >> 16, (b >> 8) & 255, b & 255),
                None => "\x1b[49m".into(),
            };
            line.push(c);
        }
        let _ = writeln!(err, "{line}\x1b[0m");
        let _ = err.flush();
        thread::sleep(Duration::from_millis(16));
    }
    let _ = writeln!(err, "\n  \x1b[38;2;158;211;138m\u{f06c} \x1b[3mlealand\x1b[0m \x1b[38;2;134;148;130m{}  ·  run it as ll\x1b[0m\n", env!("CARGO_PKG_VERSION"));
}

fn inline(h: u16) -> io::Result<Term> {
    Terminal::with_options(CrosstermBackend::new(io::stdout()), TerminalOptions { viewport: Viewport::Inline(h) })
}

/// Wipes the inline view (leaving the cursor where it started) or leaves the alternate screen.
fn erase(term: &mut Term, full: bool) -> io::Result<()> {
    if full {
        return execute!(io::stdout(), LeaveAlternateScreen);
    }
    let y = term.get_frame().area().y;
    execute!(io::stdout(), cursor::MoveTo(0, y), terminal::Clear(ClearType::FromCursorDown))
}

fn run(app: &mut App) -> io::Result<()> {
    let mut h = app.height();
    let mut term = inline(h)?;
    while !app.done || app.outro.is_some_and(|t| t.elapsed().as_millis() < OUTRO_END) {
        if app.toggle_full {
            app.toggle_full = false;
            erase(&mut term, app.full)?;
            app.full = !app.full;
            term = if app.full {
                execute!(io::stdout(), EnterAlternateScreen)?;
                let mut t = Terminal::new(CrosstermBackend::new(io::stdout()))?;
                t.clear()?;
                t
            } else {
                h = app.height();
                inline(h)?
            };
        }
        if let Some(mut cmd) = app.tty_cmd.take() {
            erase(&mut term, app.full)?;
            execute!(io::stdout(), DisableMouseCapture)?;
            terminal::disable_raw_mode()?;
            execute!(io::stdout(), cursor::Show)?;
            let tty = || File::options().write(true).open("/dev/tty").map_or(Stdio::inherit(), Stdio::from);
            if let Err(e) = cmd.stdout(tty()).status() {
                app.flash(format!("couldn't run it: {e}"));
            }
            terminal::enable_raw_mode()?;
            execute!(io::stdout(), EnableMouseCapture)?;
            term = if app.full {
                execute!(io::stdout(), EnterAlternateScreen)?;
                Terminal::new(CrosstermBackend::new(io::stdout()))?
            } else {
                h = app.height();
                inline(h)?
            };
        }
        if !app.full && app.height() != h {
            erase(&mut term, false)?;
            h = app.height();
            term = inline(h)?;
        }
        term.draw(|f| app.draw(f))?;
        app.place_image()?;
        let wait = if app.busy() { Duration::from_millis(30) } else { Duration::from_secs(1) };
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if app.outro.is_some() {
                        app.outro = None; // a key skips the goodbye
                    } else {
                        app.key(k);
                    }
                }
                Event::Mouse(m) if app.outro.is_none() => app.mouse(m),
                _ => {}
            }
        }
        app.tick();
        if app.done && app.outro.is_none() && app.animate && !app.outro_skipped {
            app.outro = Some(Instant::now());
            app.outro_skipped = true; // only once
        }
    }
    erase(&mut term, app.full)?;
    // push everything above into scrollback, so the shell's next prompt starts at the top
    let y = cursor::position().map_or(0, |p| p.1);
    let rows = terminal::size().map_or(0, |s| s.1);
    if y > 0 && rows > 0 {
        execute!(io::stdout(), cursor::MoveTo(0, rows - 1))?;
        io::stdout().write_all("\n".repeat(y as usize).as_bytes())?;
        execute!(io::stdout(), cursor::MoveTo(0, 0))?;
    }
    Ok(())
}

impl App {
    fn new(dir: PathBuf) -> Self {
        let (chords, err) = load_chords();
        let (sizes_tx, sizes_rx) = mpsc::channel();
        let mut app = App {
            entries: read_entries(&dir).unwrap_or_default(),
            dir: dir.clone(),
            sel: 0,
            scroll: 0,
            rows: 10,
            back: Vec::new(),
            chords,
            pending: String::new(),
            pending_at: Instant::now(),
            esc_at: None,
            mode: Mode::Browse,
            full: false,
            toggle_full: false,
            git: HashMap::new(),
            git_rx: None,
            counts: HashMap::new(),
            widths: None,
            preview: None,
            image: None,
            image_area: Rect::default(),
            shown_image: None,
            sizes: HashMap::new(),
            sizes_rx,
            sizes_tx,
            msg: err.map(|e| (e, Instant::now())),
            index: jump::start(),
            uid: unsafe { libc::getuid() },
            users: HashMap::new(),
            tty_cmd: None,
            intro: None,
            cascade: None,
            outro: None,
            outro_skipped: false,
            body_top: 0,
            click_at: None,
            animate: !splash_off_file().exists(),
            done: false,
            answer: None,
        };
        app.git_rx = Some(git_status(dir));
        let (cols, rows) = terminal::size().unwrap_or((0, 0));
        if app.animate && cols as usize >= logo::LOGO.iter().map(|r| r.len()).max().unwrap_or(0) + 2 && rows >= app.intro_height() + 2 {
            app.intro = Some(Instant::now());
        }
        app
    }

    fn intro_height(&self) -> u16 {
        (LOG_LINES + 1 + logo::LOGO.len()) as u16
    }

    fn max_height(&self) -> u16 {
        let rows = terminal::size().map_or(24, |s| s.1);
        (rows * 2 / 5).max(10).min(rows)
    }

    fn height(&self) -> u16 {
        if self.intro.is_some() {
            return self.intro_height();
        }
        let want = match &self.mode {
            Mode::Browse | Mode::Name { .. } | Mode::Confirm(_) => self.entries.len().max(1),
            Mode::Help => self.chords.len() + 10,
            Mode::Jump { .. } => self.entries.len().max(8),
            Mode::Pick { .. } => 12,
        };
        let cap = if matches!(self.mode, Mode::Help) { terminal::size().map_or(24, |s| s.1) } else { self.max_height() };
        (want as u16 + CHROME).min(cap)
    }

    fn busy(&self) -> bool {
        self.intro.is_some()
            || self.outro.is_some()
            || self.cascade.is_some()
            || !self.pending.is_empty()
            || self.esc_at.is_some()
            || self.msg.is_some()
            || self.git_rx.is_some()
            || matches!(self.mode, Mode::Jump { .. })
    }

    /// A folder's total size, worked out by du in the background; None until it's known.
    fn folder_size(&mut self, path: &Path) -> Option<u64> {
        while let Ok((p, n)) = self.sizes_rx.try_recv() {
            self.sizes.insert(p, Some(n));
        }
        if let Some(n) = self.sizes.get(path) {
            return *n;
        }
        self.sizes.insert(path.to_path_buf(), None);
        let (tx, p) = (self.sizes_tx.clone(), path.to_path_buf());
        thread::spawn(move || {
            let out = Command::new("du").arg("-sb").arg(&p).stdin(Stdio::null()).stderr(Stdio::null()).output();
            let n = out.ok().and_then(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next()?.parse().ok());
            if let Some(n) = n {
                let _ = tx.send((p, n));
            }
        });
        None
    }

    /// Puts the selected picture over the preview pane, or takes the old one away.
    fn place_image(&mut self) -> io::Result<()> {
        let want = match (&self.image, self.selected()) {
            (Some((p, ..)), Some(e)) if self.full && *p == e.path && preview::can_draw_images() => Some(p.clone()),
            _ => None,
        };
        if want == self.shown_image {
            return Ok(());
        }
        let mut out = io::stdout();
        out.write_all(preview::CLEAR.as_bytes())?;
        if let (Some(_), Some((_, png, w, h))) = (&want, &self.image) {
            let a = self.image_area;
            let ws = terminal::window_size().ok();
            let (cw, ch) = ws.filter(|s| s.width > 0 && s.columns > 0).map_or((10.0, 20.0), |s| {
                (s.width as f64 / s.columns as f64, s.height as f64 / s.rows as f64)
            });
            // as big as fits in the pane without stretching
            let scale = (a.width as f64 * cw / *w as f64).min(a.height as f64 * ch / *h as f64).min(1.0); // never blown up past its own size
            let cols = ((*w as f64 * scale / cw).round() as u16).clamp(1, a.width.max(1));
            let rows = ((*h as f64 * scale / ch).round() as u16).clamp(1, a.height.max(1));
            execute!(out, cursor::MoveTo(a.x, a.y))?;
            out.write_all(preview::kitty(png, cols, rows).as_bytes())?;
        }
        out.flush()?;
        self.shown_image = want;
        Ok(())
    }

    fn flash(&mut self, s: impl Into<String>) {
        self.msg = Some((s.into(), Instant::now()));
    }

    fn tick(&mut self) {
        if self.intro.is_some_and(|t| t.elapsed().as_millis() > INTRO_END) {
            self.intro = None;
            self.cascade = Some(Instant::now());
        }
        if self.cascade.is_some_and(|t| t.elapsed().as_millis() > ROW_STEP * self.rows as u128 + 50) {
            self.cascade = None;
        }
        self.expire();
        if self.esc_at.is_some_and(|t| t.elapsed() > ESC_GAP) {
            self.esc_at = None;
        }
        if self.msg.as_ref().is_some_and(|m| m.1.elapsed() > Duration::from_secs(3)) {
            self.msg = None;
        }
        if let Some(rx) = &self.git_rx {
            match rx.try_recv() {
                Ok((dir, map)) if dir == self.dir => {
                    self.git = map;
                    self.git_rx = None;
                }
                Ok(_) | Err(mpsc::TryRecvError::Disconnected) => self.git_rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Mode::Jump { q, at, hits, .. } = &mut self.mode {
            // 2 s to get going, then 500 ms between letters; running out cancels
            let limit = if q.chars().count() <= 1 { 2000 } else { 500 };
            if at.elapsed() > Duration::from_millis(limit) {
                self.mode = Mode::Browse;
            } else if hits.is_empty() && self.index.ready.load(std::sync::atomic::Ordering::Relaxed) {
                let q = q.clone();
                self.search(&q); // the index finished loading after the last letter
            }
        }
    }

    fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.sel)
    }

    fn go(&mut self, dir: PathBuf, select: Option<String>, remember: bool) {
        let entries = match read_entries(&dir) {
            Ok(e) => e,
            Err(e) => return self.flash(format!("can't open {}: {}", tilde(&dir), e.kind())),
        };
        if remember && dir != self.dir {
            self.back.push(std::mem::replace(&mut self.dir, dir.clone()));
        }
        self.dir = dir.clone();
        self.sel = select.and_then(|n| entries.iter().position(|e| e.name == n)).unwrap_or(0);
        self.entries = entries;
        self.scroll = 0;
        self.git.clear();
        self.counts.clear();
        self.widths = None;
        self.git_rx = Some(git_status(dir.clone()));
        jump::record(&dir);
    }

    fn step(&mut self, by: isize) {
        let last = self.entries.len().saturating_sub(1) as isize;
        self.sel = (self.sel as isize + by).clamp(0, last.max(0)) as usize;
    }

    fn key(&mut self, k: KeyEvent) {
        // any key skips the launch animation, and still does what it does
        self.intro = None;
        self.cascade = None;
        let combo = KeyCombination::from(k);
        if combo == key!(ctrl - c) {
            self.done = true;
            return;
        }
        let ch = match k.code {
            KeyCode::Char(c) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
            _ => None,
        };
        match &mut self.mode {
            Mode::Help => self.mode = Mode::Browse,
            Mode::Confirm(path) => {
                let p = path.clone();
                self.mode = Mode::Browse;
                if ch == Some('y') {
                    self.trash(p);
                }
            }
            Mode::Name { text, then } => match (ch, combo) {
                (Some(c), _) => text.push(c),
                (_, key!(backspace)) => {
                    text.pop();
                }
                (_, key!(enter)) => {
                    let (name, then) = (text.trim().to_string(), then.clone());
                    self.mode = Mode::Browse;
                    self.mkdir(&name, then);
                }
                (_, key!(esc)) => self.mode = Mode::Browse,
                _ => {}
            },
            Mode::Jump { q, at, sel, hits } => {
                *at = Instant::now();
                match (ch, combo) {
                    (Some(c), _) => {
                        q.push(c);
                        let q = q.clone();
                        self.search(&q);
                    }
                    (_, key!(backspace)) => {
                        q.pop();
                        let q = q.clone();
                        self.search(&q);
                    }
                    (_, key!(up)) => *sel = sel.saturating_sub(1),
                    (_, key!(down)) => *sel = (*sel + 1).min(hits.len().saturating_sub(1)),
                    (_, key!(tab) | key!(enter) | key!(right)) => {
                        if let Some(p) = hits.get(*sel).cloned() {
                            self.mode = Mode::Browse;
                            self.go(p, None, true);
                        }
                    }
                    _ => self.mode = Mode::Browse,
                }
            }
            Mode::Pick { file, mime, apps, filter, sel, .. } => {
                let shown = picked(apps, filter);
                match (ch, combo) {
                    (Some(c), _) => {
                        filter.push(c);
                        *sel = 0;
                    }
                    (_, key!(backspace)) => {
                        filter.pop();
                        *sel = 0;
                    }
                    (_, key!(up)) => *sel = sel.saturating_sub(1),
                    (_, key!(down)) => *sel = (*sel + 1).min(shown.len().saturating_sub(1)),
                    (_, key!(enter) | key!(right) | key!(tab)) => {
                        if let Some(&i) = shown.get(*sel) {
                            let (a, f) = (apps[i].clone(), file.clone());
                            apps::save(mime, &a);
                            self.mode = Mode::Browse;
                            self.launch(&a, &f);
                        }
                    }
                    (_, key!(esc) | key!(left)) => self.mode = Mode::Browse,
                    (_, key!(delete)) => {
                        let m = mime.clone();
                        apps::forget(&m);
                        self.mode = Mode::Browse;
                        self.flash(format!("forgot the app for {m}; the next open asks"));
                    }
                    _ => {}
                }
            }
            Mode::Browse => match (ch, combo) {
                (Some('/'), _) => {
                    self.pending.clear();
                    self.mode = Mode::Jump { q: String::new(), at: Instant::now(), sel: 0, hits: Vec::new() };
                    self.search(""); // your most-visited folders, before you type anything
                }
                (Some(c), _) => self.feed(c),
                (_, key!(up)) => self.step(-1),
                (_, key!(down)) => self.step(1),
                (_, key!(ctrl - up)) => self.step(-5),
                (_, key!(ctrl - down)) => self.step(5),
                (_, key!(pageup)) => self.step(-(self.rows as isize)),
                (_, key!(pagedown)) => self.step(self.rows as isize),
                (_, key!(home)) => self.sel = 0,
                (_, key!(end)) => self.step(isize::MAX / 2),
                (_, key!(enter)) if !self.pending.is_empty() => {
                    if !self.fire_pending() {
                        self.pending.clear();
                    }
                }
                (_, key!(right) | key!(enter)) => self.enter(),
                (_, key!(left)) => {
                    if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
                        let name = self.dir.file_name().map(|n| n.to_string_lossy().into_owned());
                        self.go(parent, name, true);
                    }
                }
                (_, key!(backspace)) => {
                    if let Some(prev) = self.back.pop() {
                        self.go(prev, None, false);
                    }
                }
                (_, key!(esc)) => {
                    if !self.pending.is_empty() {
                        self.pending.clear();
                    } else if self.esc_at.take().is_some() {
                        self.done = true; // esc esc: leave without moving the shell
                    } else {
                        self.esc_at = Some(Instant::now());
                    }
                }
                _ => {}
            },
        }
    }

    /// Opens what the cursor is on: into a folder, or a file with its app.
    fn enter(&mut self) {
        match self.selected() {
            Some(e) if e.dir => self.go(e.path.clone(), None, true),
            Some(e) => self.open(e.path.clone(), false),
            None => {}
        }
    }

    /// Click picks a row, a second click on it within 400 ms opens it, the wheel moves.
    fn mouse(&mut self, m: MouseEvent) {
        if !matches!(m.kind, MouseEventKind::Moved | MouseEventKind::Drag(_)) {
            self.intro = None;
            self.cascade = None;
        }
        if !matches!(self.mode, Mode::Browse) {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp => self.step(-3),
            MouseEventKind::ScrollDown => self.step(3),
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(row) = m.row.checked_sub(self.body_top).map(usize::from).filter(|&r| r < self.rows) else { return };
                let i = self.scroll + row;
                if i >= self.entries.len() {
                    return;
                }
                let again = i == self.sel && self.click_at.is_some_and(|t| t.elapsed() < Duration::from_millis(400));
                self.sel = i;
                self.click_at = if again { None } else { Some(Instant::now()) };
                if again {
                    self.enter();
                }
            }
            _ => {}
        }
    }

    fn search(&mut self, q: &str) {
        let local: Vec<PathBuf> = self.entries.iter().filter(|e| e.dir).map(|e| e.path.clone()).collect();
        let found = self.index.search(q, &self.dir, &local);
        if let Mode::Jump { sel, hits, .. } = &mut self.mode {
            *hits = found;
            *sel = 0;
        }
    }

    /// Runs the chord typed so far, if it is one. True when it ran.
    fn fire_pending(&mut self) -> bool {
        let p = self.pending.clone();
        match self.chords.iter().find(|c| c.keys == p).map(|c| c.act.clone()) {
            Some(act) => {
                self.pending.clear();
                self.act(act);
                true
            }
            None => false,
        }
    }

    /// A chord left waiting for a longer one fires once the gap has passed.
    fn expire(&mut self) {
        if !self.pending.is_empty() && self.pending_at.elapsed() > GAP {
            let p = std::mem::take(&mut self.pending);
            if let Some(act) = self.chords.iter().find(|c| c.keys == p).map(|c| c.act.clone()) {
                self.act(act);
            }
        }
    }

    fn feed(&mut self, c: char) {
        self.expire();
        if c == ' ' && self.fire_pending() {
            return;
        }
        self.pending_at = Instant::now();
        self.pending.push(c);
        let starts = |p: &str| self.chords.iter().any(|ch| ch.keys.starts_with(p));
        if !starts(&self.pending) {
            self.pending = c.to_string(); // the old chord went nowhere; maybe this key starts a new one
            if !starts(&self.pending) {
                self.pending.clear();
                return;
            }
        }
        let longer = self.chords.iter().any(|ch| ch.keys.len() > self.pending.len() && ch.keys.starts_with(&self.pending));
        if let Some(ch) = self.chords.iter().find(|ch| ch.keys == self.pending).filter(|_| !longer) {
            let act = ch.act.clone();
            self.pending.clear();
            self.act(act);
        }
    }

    fn act(&mut self, act: Action) {
        let here = self.dir.to_string_lossy().into_owned();
        match act {
            Action::Quit => {
                self.answer = Some(here);
                self.done = true;
            }
            Action::Exit(cmd) => {
                self.answer = Some(format!("{here}\n{cmd}"));
                self.done = true;
            }
            Action::Help => self.mode = Mode::Help,
            Action::Full => self.toggle_full = true,
            Action::OpenWith => match self.selected() {
                Some(e) if !e.dir => self.open(e.path.clone(), true),
                _ => self.flash("op opens a file with another app; this is a folder"),
            },
            Action::Mkdir(then) => self.mode = Mode::Name { text: String::new(), then },
            Action::Trash => {
                if let Some(e) = self.selected() {
                    self.mode = Mode::Confirm(e.path.clone());
                }
            }
            Action::Spawn(cmd) => {
                let mut c = Command::new("sh");
                c.arg("-c").arg(&cmd).current_dir(&self.dir).env("PWD", &self.dir);
                if let Some(e) = self.selected() {
                    c.env("f", &e.path);
                }
                if let Err(e) = detach(c) {
                    self.flash(format!("{cmd}: {e}"));
                }
            }
        }
    }

    fn mkdir(&mut self, name: &str, then: Option<String>) {
        if name.is_empty() {
            return;
        }
        let path = self.dir.join(name);
        if let Err(e) = fs::create_dir_all(&path) {
            return self.flash(format!("couldn't make {name}: {}", e.kind()));
        }
        match then {
            Some(cmd) => {
                jump::record(&path);
                self.answer = Some(format!("{}\n{cmd}", path.display()));
                self.done = true;
            }
            None => {
                let first = name.split('/').next().map(String::from);
                self.go(self.dir.clone(), first, false);
            }
        }
    }

    /// Into the desktop trash (Dolphin can put it back), not deleted.
    fn trash(&mut self, path: PathBuf) {
        match Command::new("gio").arg("trash").arg(&path).stdin(Stdio::null()).output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let why = String::from_utf8_lossy(&o.stderr);
                let why = why.lines().next().unwrap_or("").rsplit(": ").next().unwrap_or("").to_string();
                return self.flash(format!("couldn't trash it: {why}"));
            }
            Err(e) => return self.flash(format!("couldn't run gio: {}", e.kind())),
        }
        let sel = self.sel;
        self.go(self.dir.clone(), None, false);
        self.sel = sel.min(self.entries.len().saturating_sub(1));
    }

    /// Opens a file with the app picked for its type, or asks which app that should be.
    fn open(&mut self, file: PathBuf, ask: bool) {
        let mime = apps::mime(&file);
        if !ask {
            if let Some(a) = apps::saved(&mime) {
                self.launch(&a, &file);
                if self.msg.is_none() {
                    self.flash(format!("opened in {}, op picks another", a.name));
                }
                return;
            }
        }
        let (apps, handles) = apps::list(&mime);
        if apps.is_empty() {
            return self.flash("no apps found");
        }
        let current = apps::saved(&mime).map(|a| a.path);
        self.mode = Mode::Pick { file, mime, apps, handles, filter: String::new(), sel: 0, current };
    }

    fn launch(&mut self, a: &apps::App, file: &Path) {
        let mut c = apps::command(a, file);
        c.current_dir(&self.dir);
        if a.terminal {
            self.tty_cmd = Some(c); // runs here, in this terminal, and ll comes back after
        } else if let Err(e) = detach(c) {
            self.flash(format!("{}: {e}", a.name));
        }
    }

    fn user(&mut self, uid: u32) -> String {
        self.users
            .entry(uid)
            .or_insert_with(|| unsafe {
                let pw = libc::getpwuid(uid);
                if pw.is_null() {
                    uid.to_string()
                } else {
                    std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned()
                }
            })
            .clone()
    }

    /// The column that only says something when a file is unusual.
    fn flags(&mut self, i: usize) -> String {
        let (uid, mode, dir, broken) = {
            let e = &self.entries[i];
            (e.uid, e.mode, e.dir, e.link.is_some() && !e.path.exists())
        };
        let mut f = Vec::new();
        if broken {
            f.push("broken link".to_string());
        }
        // judged against the folder: in /usr/bin everything is root's and read-only, so nothing is
        let (dir_uid, dir_writable) = fs::metadata(&self.dir).map_or((self.uid, true), |m| {
            (m.uid(), if m.uid() == self.uid { m.mode() & 0o200 != 0 } else { m.mode() & 0o002 != 0 })
        });
        if uid != self.uid && uid != dir_uid {
            f.push(self.user(uid));
        }
        if !dir && mode & 0o111 != 0 {
            f.push("exec".into());
        }
        if uid == self.uid && mode & 0o077 == 0 {
            f.push("private".into());
        }
        let writable = if uid == self.uid { mode & 0o200 != 0 } else { mode & 0o002 != 0 };
        if !writable && dir_writable && self.uid != 0 {
            f.push("read-only".into());
        }
        if mode & 0o002 != 0 && mode & 0o1000 == 0 && mode & 0o170000 != 0o120000 {
            f.push("world-writable".into());
        }
        f.join(", ")
    }

    fn count(&mut self, p: &Path) -> String {
        let n = *self.counts.entry(p.to_path_buf()).or_insert_with(|| fs::read_dir(p).ok().map(|r| r.count()));
        match n {
            None => "-".into(),
            Some(0) => "empty".into(),
            Some(1) => "1 item".into(),
            Some(n) => format!("{n} items"),
        }
    }

    // ---------- drawing ----------

    fn draw(&mut self, f: &mut Frame) {
        if let Some(t) = self.intro {
            return self.draw_intro(f, t.elapsed().as_millis());
        }
        self.draw_main(f);
        if let Some(t) = self.outro {
            leaves(f, t.elapsed().as_millis());
        }
    }

    fn draw_main(&mut self, f: &mut Frame) {
        let area = f.area();
        let (main, side) = if self.full && area.width >= 70 {
            let w = area.width * 11 / 20;
            (Rect { width: w, ..area }, Some(Rect { x: area.x + w, width: area.width - w, ..area }))
        } else {
            // inline, the view is only as wide as its rows
            let want = match self.mode {
                Mode::Browse if !self.pending.is_empty() => (self.row_width() + 2).max(self.chord_hints().width() + 2),
                Mode::Browse => (self.row_width() + 2).max(58),
                Mode::Name { .. } | Mode::Confirm(_) => self.row_width().max(72),
                _ => area.width as usize,
            };
            (Rect { width: (want as u16).min(area.width), ..area }, None)
        };
        let head = Rect { y: main.y + 1, height: 1, ..main };
        let rule = Rect { x: main.x + 2, y: main.y + 2, width: main.width.saturating_sub(4), height: 1 };
        let body = Rect { y: main.y + 3, height: main.height.saturating_sub(CHROME), width: main.width.saturating_sub(2), ..main };
        let foot = Rect { y: main.y + main.height.saturating_sub(1), height: 1, ..main };
        self.body_top = body.y;
        let mut lines: Vec<Line> = Vec::new();
        let right: Line;
        let left: Line;
        let rows = body.height as usize;
        match &self.mode {
            Mode::Browse | Mode::Name { .. } | Mode::Confirm(_) => {
                (left, right) = self.head();
                if self.sel < self.scroll {
                    self.scroll = self.sel;
                }
                if self.sel >= self.scroll + rows {
                    self.scroll = self.sel + 1 - rows;
                }
                self.rows = rows.max(1);
                if self.entries.is_empty() {
                    lines.push(Line::from("   empty".fg(pal::DIM)));
                }
                let (names, flags) = self.widths();
                let name_w = names.min((body.width as usize).saturating_sub(ROW_FIXED + flags)).max(8);
                let shown = self.cascade.map_or(rows, |t| (t.elapsed().as_millis() / ROW_STEP) as usize + 1);
                for i in self.scroll..(self.scroll + rows.min(shown)).min(self.entries.len()) {
                    lines.push(self.row(i, name_w));
                }
            }
            Mode::Help => {
                left = Line::from(vec![
                    "   \u{f06c} ".fg(pal::DIR),
                    Span::styled("lealand", Style::new().fg(pal::DIR).add_modifier(Modifier::ITALIC)),
                    "   chords and keys".fg(pal::TEXT).bold(),
                ]);
                right = Line::from("any key closes ".fg(pal::DIM));
                lines.push(Line::from(Span::styled("  chords, typed quickly", Style::new().fg(pal::DIR).add_modifier(Modifier::ITALIC))));
                for c in &self.chords {
                    lines.push(Line::from(vec![format!("    {:<11}", c.keys).fg(pal::AMBER).bold(), describe(&c.act).fg(pal::TEXT)]));
                }
                lines.push(Line::default());
                lines.push(Line::from(Span::styled("  keys", Style::new().fg(pal::DIR).add_modifier(Modifier::ITALIC))));
                for (k, d) in [
                    ("← →", "parent / open"),
                    ("↑ ↓", "move, with ctrl 5 at a time"),
                    ("pgup pgdn", "page, home/end for first/last"),
                    ("backspace", "back to the last folder"),
                    ("/", "jump to any folder: type a bit, tab to go"),
                    ("esc esc", "quit, shell stays where it was"),
                ] {
                    lines.push(Line::from(vec![format!("    {k:<11}").fg(pal::LINK).bold(), d.fg(pal::TEXT)]));
                }
            }
            Mode::Jump { q, sel, hits, .. } => {
                let top = hits.get(*sel);
                let ghost = top.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let rest = match ghost.get(..q.len()) {
                    Some(start) if start.eq_ignore_ascii_case(q) => ghost[q.len()..].to_string(),
                    _ if ghost.is_empty() => String::new(),
                    _ => format!("  → {ghost}"),
                };
                left = Line::from(vec!["   /".fg(pal::AMBER).bold(), q.clone().bold(), rest.fg(pal::DIM)]);
                right = Line::from(top.map(|p| tilde(p)).unwrap_or_default().fg(pal::DIM));
                if hits.is_empty() {
                    let ready = self.index.ready.load(std::sync::atomic::Ordering::Relaxed);
                    let say = if q.is_empty() { "type part of a folder name" } else if ready { "no match" } else { "still indexing ~" };
                    lines.push(Line::from(format!("   {say}").fg(pal::DIM)));
                }
                for (i, p) in hits.iter().enumerate().take(rows) {
                    let name = p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                    let parent = p.parent().map(tilde).unwrap_or_default();
                    let mark = if i == *sel { " ▎ " } else { "   " };
                    let mut l = Line::from(vec![mark.fg(pal::DIR), "\u{f07b}  ".fg(pal::DIR), name.fg(pal::DIR).bold(), format!("    {parent}").fg(pal::DIM)]);
                    if i == *sel {
                        l = l.style(Style::new().bg(pal::SEL));
                    }
                    lines.push(l);
                }
            }
            Mode::Pick { file, mime, apps, handles, filter, sel, current } => {
                let name = file.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                left = Line::from(vec!["   open ".fg(pal::TEXT), name.bold(), " with".into()]);
                let forget = if current.is_some() { "  del forgets" } else { "" };
                right = Line::from(vec![filter.clone().fg(pal::AMBER), format!("  {mime}{forget} ").fg(pal::DIM)]);
                let shown = picked(apps, filter);
                let start = sel.saturating_sub(rows.saturating_sub(1));
                for (row, &i) in shown.iter().enumerate().skip(start).take(rows) {
                    let a = &apps[i];
                    let fits = i < *handles;
                    let mut spans = vec![
                        "  ".into(),
                        if fits { a.name.clone().bold() } else { a.name.clone().fg(pal::DIM) },
                        format!("  {}", a.generic).fg(pal::DIM),
                    ];
                    if i == 0 && fits && filter.is_empty() {
                        spans.push("  default".fg(pal::DIR));
                    }
                    if a.terminal {
                        spans.push("  terminal".fg(pal::LINK));
                    }
                    if current.as_ref() == Some(&a.path) {
                        spans.push("  current".fg(pal::AMBER));
                    }
                    spans[0] = if row == *sel { " ▎ ".fg(pal::DIR) } else { "   ".into() };
                    let mut l = Line::from(spans);
                    if row == *sel {
                        l = l.style(Style::new().bg(pal::SEL));
                    }
                    lines.push(l);
                }
                if shown.is_empty() {
                    lines.push(Line::from("   nothing matches".fg(pal::DIM)));
                }
            }
        }
        let rw = right.width() as u16 + 1;
        f.render_widget(Paragraph::new(left), Rect { width: head.width.saturating_sub(rw + 1), ..head });
        f.render_widget(Paragraph::new(right), Rect { x: head.x + head.width.saturating_sub(rw), width: rw.min(head.width), ..head });
        f.render_widget(Paragraph::new(lines), body);
        f.render_widget(Paragraph::new("─".repeat(rule.width as usize).fg(pal::FAINT)), rule);
        f.render_widget(Paragraph::new(self.footer()), foot);
        if let Some(side) = side {
            let side = Rect { y: side.y + 1, height: side.height.saturating_sub(1), ..side };
            let lines = self.preview_lines(side.height.saturating_sub(2) as usize, side.width.saturating_sub(2));
            let title = self.selected().map(|e| {
                let what = if e.dir { format!("{}/", e.name) } else { e.name.clone() };
                Line::from(vec![" ".into(), what.fg(pal::TEXT).bold(), " ".into()])
            });
            let info = self.selected().map(|e| (e.dir, e.path.clone(), e.size, e.mtime));
            let about = info.map(|(dir, path, size, mtime)| {
                let size = if dir { self.folder_size(&path).map_or("…".into(), human) } else { human(size) };
                Line::from(format!(" {size}  ·  {} ", ago(mtime)).fg(pal::DIM)).right_aligned()
            });
            self.image_area = Rect { x: side.x + 2, y: side.y + 1, width: side.width.saturating_sub(3), height: side.height.saturating_sub(2) };
            let mut block = ratatui::widgets::Block::new()
                .borders(ratatui::widgets::Borders::LEFT | ratatui::widgets::Borders::TOP)
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(Style::new().fg(pal::FAINT));
            if let Some(t) = title {
                block = block.title(t);
            }
            if let Some(a) = about {
                block = block.title(a);
            }
            f.render_widget(Paragraph::new(lines).block(block), side);
        }
    }

    fn head(&self) -> (Line<'static>, Line<'static>) {
        match &self.mode {
            Mode::Name { text, then } => {
                let after = then.as_ref().map_or("enter makes it".into(), |c| format!("enter makes it and runs {c}"));
                (Line::from(vec!["   new folder: ".fg(pal::AMBER), text.clone().bold(), "▏".into()]), Line::from(format!("{after}, esc cancels ").fg(pal::DIM)))
            }
            Mode::Confirm(p) => {
                let name = p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                (Line::from(vec!["   trash ".fg(pal::RED), name.bold(), "?".into()]), Line::from("y moves it to the trash, any other key keeps it ".fg(pal::DIM)))
            }
            _ => (self.crumbs(), self.status()),
        }
    }

    /// Real log lines, then the willow wipes in from the top and dissolves into the listing.
    fn draw_intro(&mut self, f: &mut Frame, t: u128) {
        let ready = self.index.ready.load(std::sync::atomic::Ordering::Relaxed);
        let (places, dirs) = self.index.counts();
        let logs = [
            format!("lealand {}", env!("CARGO_PKG_VERSION")),
            format!("read {}, {} entries", tilde(&self.dir), self.entries.len()),
            format!("{} chords", self.chords.len()),
            if places > 0 { format!("{} places you've been", thousands(places)) } else { "no places yet".into() },
            if ready { format!("{} folders indexed", thousands(dirs)) } else { "indexing ~ in the background".into() },
        ];
        let gone = if t > DISSOLVE_AT { (t - DISSOLVE_AT) as f64 / (INTRO_END - DISSOLVE_AT) as f64 } else { 0.0 };
        let mut lines: Vec<Line> = Vec::new();
        for (i, l) in logs.iter().enumerate().take((t / LOG_STEP) as usize + 1) {
            if noise(i, 99) < gone {
                lines.push(Line::default());
                continue;
            }
            let (mark, color) = if i == 0 { (" \u{f06c} ", pal::TEXT) } else { ("  ✓ ", pal::DIM) };
            lines.push(Line::from(vec![mark.fg(pal::DIR), Span::styled(l.clone(), Style::new().fg(color))]));
        }
        lines.resize(LOG_LINES + 1, Line::default());
        if t >= LOGO_AT {
            let rows = ((t - LOGO_AT) / WIPE_STEP) as usize + 1;
            for (y, row) in logo::LOGO.iter().enumerate().take(rows) {
                let mut spans = vec![Span::raw("  ")];
                for (x, &(c, fg, bg)) in row.iter().enumerate() {
                    if c == ' ' || noise(x, y) < gone {
                        spans.push(Span::raw(" "));
                    } else {
                        let mut st = Style::new().fg(rgb(fg));
                        if let Some(b) = bg {
                            st = st.bg(rgb(b));
                        }
                        spans.push(Span::styled(c.to_string(), st));
                    }
                }
                lines.push(Line::from(spans));
            }
        }
        f.render_widget(Paragraph::new(lines), f.area());
    }

    /// While a chord is half typed: the keys that can come next and what they do.
    fn chord_hints(&self) -> Line<'static> {
        let mut spans: Vec<Span<'static>> = vec!["    ".into()];
        for c in self.chords.iter().filter(|c| c.keys.starts_with(&self.pending)) {
            if spans.len() > 1 {
                spans.push("   ·   ".fg(pal::FAINT));
            }
            let rest = &c.keys[self.pending.len()..];
            let key = if rest.is_empty() { "enter".to_string() } else { rest.to_string() };
            spans.push(key.fg(pal::AMBER).bold());
            spans.push(format!("  {}", describe(&c.act)).fg(pal::DIM));
        }
        Line::from(spans)
    }

    /// The quiet line at the bottom: what's here, or the keys for the screen you're on.
    fn footer(&self) -> Line<'static> {
        let say = match &self.mode {
            Mode::Jump { .. } => "tab or enter goes  ·  ↑ ↓ picks  ·  esc cancels".to_string(),
            Mode::Pick { .. } => "enter opens  ·  type to filter  ·  del forgets  ·  esc cancels".to_string(),
            Mode::Help => "add your own in ~/.config/lealand/chords".to_string(),
            _ if !self.pending.is_empty() => return self.chord_hints(),
            _ => {
                let folders = self.entries.iter().filter(|e| e.dir).count();
                let hidden = self.entries.iter().filter(|e| e.hidden).count();
                let n = self.entries.len();
                let mut v = vec![format!("{n} item{}", if n == 1 { "" } else { "s" })];
                if folders > 0 {
                    v.push(format!("{folders} folder{}", if folders == 1 { "" } else { "s" }));
                }
                if hidden > 0 {
                    v.push(format!("{hidden} hidden"));
                }
                if n > self.rows {
                    v.push(format!("{} of {n}", self.sel + 1));
                }
                v.join("  ·  ")
            }
        };
        Line::from(format!("    {say}").fg(pal::FAINT))
    }

    /// lealand, then the path as ~ › utils › lealand with the folder you're in brightest.
    fn crumbs(&self) -> Line<'static> {
        let mut spans = vec![
            Span::styled("   \u{f06c} ", Style::new().fg(pal::DIR)),
            Span::styled("lealand", Style::new().fg(pal::DIR).add_modifier(Modifier::ITALIC)),
            Span::raw("   "),
        ];
        let path = tilde(&self.dir);
        let mut parts: Vec<String> = path.split('/').filter(|p| !p.is_empty()).map(String::from).collect();
        if path.starts_with('/') {
            parts.insert(0, "/".into());
        }
        let last = parts.len().saturating_sub(1);
        for (i, p) in parts.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" › ", Style::new().fg(pal::FAINT)));
            }
            spans.push(if i == last { Span::styled(p, Style::new().fg(pal::TEXT).bold()) } else { Span::styled(p, Style::new().fg(pal::DIM)) });
        }
        spans.push(git_summary(&self.git));
        Line::from(spans)
    }

    fn status(&self) -> Line<'static> {
        if !self.pending.is_empty() {
            return Line::from(format!("{}… ", self.pending).fg(pal::AMBER).bold());
        }
        if self.esc_at.is_some() {
            return Line::from("esc again to leave ".fg(pal::AMBER));
        }
        if let Some((m, _)) = &self.msg {
            return Line::from(format!("{m} ").fg(pal::RED));
        }
        Line::from("? help ".fg(pal::DIM))
    }

    /// Widest name (with its link target) and widest flags in this folder, so columns sit right
    /// after the names like eza instead of across the whole terminal.
    fn widths(&mut self) -> (usize, usize) {
        if let Some(w) = self.widths {
            return w;
        }
        let mut w = (0, 0);
        for i in 0..self.entries.len() {
            let e = &self.entries[i];
            let link = e.link.as_ref().map_or(0, |t| 3 + t.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>());
            let slash = usize::from(e.dir);
            w.0 = w.0.max((e.name.chars().map(|c| c.width().unwrap_or(0)).sum::<usize>() + link + slash).min(40));
            w.1 = w.1.max(self.flags(i).chars().count());
        }
        self.widths = Some(w);
        w
    }

    /// Columns besides the name: icon 3, size 9, date 11, git 2, flags after a space.
    fn row_width(&mut self) -> usize {
        let (n, f) = self.widths();
        n + ROW_FIXED + f
    }

    fn row(&mut self, i: usize, name_w: usize) -> Line<'static> {
        let flags = self.flags(i);
        let size = {
            let e = &self.entries[i];
            if e.dir { self.count(&e.path.clone()) } else { human(e.size) }
        };
        let e = &self.entries[i];
        let git = self.git.get(&e.name).copied();
        let base = if e.hidden {
            Style::new().fg(pal::FAINT)
        } else if e.dir {
            Style::new().fg(pal::DIR).add_modifier(Modifier::BOLD)
        } else if e.link.is_some() {
            Style::new().fg(pal::LINK)
        } else if e.mode & 0o111 != 0 {
            Style::new().fg(pal::LEAF)
        } else {
            Style::new().fg(pal::TEXT)
        };
        let mut name = e.name.clone();
        if e.dir {
            name.push('/');
        }
        if let Some(t) = &e.link {
            name += &format!(" → {t}");
        }
        let dim = Style::new().fg(pal::FAINT);
        let (gc, gs) = match git {
            Some('?') => ("?", Style::new().fg(pal::LILAC)),
            Some('D') => ("D", Style::new().fg(pal::RED)),
            Some('A') => ("A", Style::new().fg(pal::LEAF)),
            Some(c) => (if c == 'M' { "M" } else { "~" }, Style::new().fg(pal::ROSE)),
            None => (" ", dim),
        };
        let selected = i == self.sel;
        let mut l = Line::from(vec![
            Span::raw(" "),
            Span::styled(if selected { "▎ " } else { "  " }, Style::new().fg(pal::DIR)),
            Span::styled(format!("{}  ", icons::icon(&e.name, e.dir, e.link.is_some(), e.mode & 0o111 != 0, selected)), base.remove_modifier(Modifier::BOLD)),
            Span::styled(fit(&name, name_w), base),
            Span::styled(format!("    {size:>9}"), dim),
            Span::styled(format!("    {:<9}", ago(e.mtime)), dim),
            Span::styled(format!("   {gc}"), gs),
            Span::styled(format!("  {flags}"), Style::new().fg(pal::AMBER)),
        ]);
        if selected {
            l = l.style(Style::new().bg(pal::SEL));
        }
        l
    }

    fn preview_lines(&mut self, n: usize, width: u16) -> Vec<Line<'static>> {
        let Some(e) = self.selected() else { return Vec::new() };
        let path = e.path.clone();
        if let Some((p, lines)) = &self.preview {
            if *p == path {
                return lines.clone();
            }
        }
        let lines: Vec<Line<'static>> = if e.dir {
            match read_entries(&path) {
                Ok(v) if v.is_empty() => vec![Line::from(" empty".fg(pal::DIM))],
                Ok(v) => v
                    .iter()
                    .take(n)
                    .map(|c| {
                        let s = if c.hidden { Style::new().fg(pal::FAINT) } else if c.dir { Style::new().fg(pal::DIR) } else { Style::new().fg(pal::TEXT) };
                        let slash = if c.dir { "/" } else { "" };
                        Line::from(Span::styled(format!(" {} {}{slash}", icon(c), c.name), s))
                    })
                    .collect(),
                Err(e) => vec![Line::from(format!(" {}", e.kind()).fg(pal::RED))],
            }
        } else if preview::is_image(&path) {
            match preview::png(&path) {
                Some((png, w, h)) => {
                    let say = if preview::can_draw_images() { String::new() } else { format!(" a picture, {w} × {h}") };
                    self.image = Some((path.clone(), png, w, h));
                    vec![Line::from(say.fg(pal::DIM))]
                }
                None => vec![Line::from(" a picture ImageMagick couldn't read".fg(pal::DIM))],
            }
        } else {
            let mut buf = vec![0; 64 * 1024];
            let got = File::open(&path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
            buf.truncate(got);
            if let Some(lines) = (!buf.contains(&0)).then(|| preview::text(&path, n, width)).flatten() {
                lines
            } else if buf.contains(&0) {
                vec![Line::from(format!(" binary, {}", human(e.size)).fg(pal::DIM))]
            } else {
                String::from_utf8_lossy(&buf)
                    .lines()
                    .take(n)
                    .enumerate()
                    .map(|(i, l)| Line::from(vec![format!(" {:>3} ", i + 1).fg(pal::FAINT), l.replace('\t', "    ").fg(pal::TEXT)]))
                    .collect()
            }
        };
        self.preview = Some((path, lines.clone()));
        lines
    }
}

/// The logo's colours, used everywhere.
mod pal {
    use ratatui::style::Color::{self, Rgb};
    pub const TEXT: Color = Rgb(0xe8, 0xe1, 0xcf); // cream, like the wordmark
    pub const DIR: Color = Rgb(0x9e, 0xd3, 0x8a); // willow
    pub const LEAF: Color = Rgb(0xc8, 0xe0, 0x7c); // young leaf, for things that run
    pub const LINK: Color = Rgb(0x7b, 0xc4, 0xb8);
    pub const DIM: Color = Rgb(0x86, 0x94, 0x82); // sage
    pub const FAINT: Color = Rgb(0x5d, 0x66, 0x5a);
    pub const AMBER: Color = Rgb(0xe0, 0xa8, 0x5c);
    pub const ROSE: Color = Rgb(0xe0, 0x8a, 0x9a);
    pub const LILAC: Color = Rgb(0xb7, 0x9f, 0xd9);
    pub const RED: Color = Rgb(0xe0, 0x6c, 0x6c);
    pub const SEL: Color = Rgb(0x22, 0x30, 0x25); // the selected row
}

/// Launch animation, in ms: log lines, then the wordmark wipes in, holds, dissolves, and the
/// listing's rows come in one after another.
const LOG_STEP: u128 = 30;
const LOGO_AT: u128 = 160;
const WIPE_STEP: u128 = 16;
const DISSOLVE_AT: u128 = 760;
const INTRO_END: u128 = 1060;
const ROW_STEP: u128 = 14;
const LOG_LINES: usize = 5;
/// Goodbye: the view falls apart into leaves for this long.
const OUTRO_END: u128 = 420;
/// Rows around the list: a blank line, the header, the rule, then a blank line and the footer.
const CHROME: u16 = 5;
/// Columns around the name in a row: margin, marker, gap, icon, 2, then 4 before the size (9),
/// 4 before the date (9), 3 before git (1) and 2 before the flags.
const ROW_FIXED: usize = 38;

fn splash_off_file() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME").map_or_else(|| jump::home().join(".config"), PathBuf::from).join("lealand/no-flag")
}

fn rgb(v: u32) -> Color {
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Fixed per-cell noise in 0..1, for the dissolve.
fn noise(x: usize, y: usize) -> f64 {
    let h = (x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (y as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    (h >> 40) as f64 / (1u64 << 24) as f64
}

/// The goodbye: each character drops out at its own moment, and about a third of them turn into
/// leaves that sway down and fade.
fn leaves(f: &mut Frame, t: u128) {
    let area = f.area();
    let buf = f.buffer_mut();
    let old = buf.clone();
    buf.reset();
    let greens = [pal::DIR, pal::LEAF, Color::Rgb(0x6f, 0xb6, 0x5a), Color::Rgb(0x3f, 0x8a, 0x46)];
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &old[(x, y)];
            if cell.symbol() == " " && cell.bg == Color::Reset {
                continue;
            }
            let n = noise(x as usize, y as usize + 7);
            let start = (n * 170.0) as u128;
            if t < start {
                buf[(x, y)] = cell.clone();
                continue;
            }
            if cell.symbol() == " " || noise(x as usize + 3, y as usize) > 0.33 {
                continue;
            }
            let age = t - start;
            let fall = y as u128 + age / 45;
            if fall >= area.bottom() as u128 {
                continue;
            }
            let sway = ((age as f64 / 90.0 + n * 6.0).sin() * 1.3).round() as i32;
            let nx = (x as i32 + sway).clamp(area.left() as i32, area.right() as i32 - 1) as u16;
            let color = if age > 200 { pal::FAINT } else { greens[(n * 40.0) as usize % 4] };
            let glyph = ["\u{f06c}", "\u{f06c}", "\u{f18c}", "‚"][(n * 97.0) as usize % 4];
            buf[(nx, fall as u16)].set_symbol(glyph).set_style(Style::new().fg(color));
        }
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn picked(apps: &[apps::App], filter: &str) -> Vec<usize> {
    let f = filter.to_lowercase();
    (0..apps.len()).filter(|&i| f.is_empty() || apps[i].name.to_lowercase().contains(&f)).collect()
}

fn describe(a: &Action) -> String {
    match a {
        Action::Quit => "quit and cd here".into(),
        Action::Help => "this list".into(),
        Action::Full => "fullscreen with a preview, on and off".into(),
        Action::OpenWith => "pick the app for this kind of file again (del there forgets it)".into(),
        Action::Mkdir(None) => "make a folder here".into(),
        Action::Mkdir(Some(c)) => format!("make a folder, cd into it and run {}", friendly(c)),
        Action::Trash => "move to the trash (asks first)".into(),
        Action::Exit(c) => format!("quit, cd here and run {}", friendly(c)),
        Action::Spawn(c) => format!("open {} here", friendly(c)),
    }
}

/// "dolphin ." reads better as Dolphin; anything unknown is shown as typed.
fn friendly(cmd: &str) -> String {
    let first = cmd.split_whitespace().next().unwrap_or("");
    let name = match first {
        "claude" => "Claude Code",
        "dolphin" => "Dolphin",
        "code" => "VS Code",
        "ghostty" if cmd.contains("+new-tab") => "a new Ghostty tab",
        "ghostty" => "Ghostty",
        "konsole" => "Konsole",
        "kate" => "Kate",
        "lazygit" => "lazygit",
        _ => return format!("`{cmd}`"),
    };
    name.into()
}

fn load_chords() -> (Vec<Chord>, Option<String>) {
    let file = env::var_os("XDG_CONFIG_HOME").map_or_else(|| jump::home().join(".config"), PathBuf::from).join("lealand/chords");
    let user = fs::read_to_string(file).unwrap_or_default();
    let (mut v, mut err): (Vec<Chord>, _) = (Vec::new(), None);
    for line in DEFAULT_CHORDS.lines().chain(user.lines()) {
        let mut parts = line.split_whitespace();
        let Some(keys) = parts.next().filter(|k| !k.starts_with('#')) else { continue };
        let kind = parts.next().unwrap_or("");
        let rest = parts.collect::<Vec<_>>().join(" ");
        v.retain(|c| c.keys != keys);
        let act = match kind {
            "quit" => Action::Quit,
            "help" => Action::Help,
            "full" => Action::Full,
            "open-with" => Action::OpenWith,
            "mkdir" => Action::Mkdir(None),
            "mkdir-exit" => Action::Mkdir(Some(rest)),
            "trash" => Action::Trash,
            "exit" => Action::Exit(rest),
            "spawn" => Action::Spawn(rest),
            "none" => continue,
            _ => {
                err = Some(format!("chords: {keys} has unknown action {kind:?}"));
                continue;
            }
        };
        v.push(Chord { keys: keys.into(), act });
    }
    (v, err)
}

fn read_entries(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut v: Vec<Entry> = fs::read_dir(dir)?
        .flatten()
        .filter_map(|d| {
            let lmeta = d.metadata().ok()?;
            let path = d.path();
            let link = lmeta.is_symlink().then(|| fs::read_link(&path).map(|t| t.display().to_string()).unwrap_or_default());
            let meta = if link.is_some() { fs::metadata(&path).unwrap_or(lmeta) } else { lmeta };
            let name = d.file_name().to_string_lossy().into_owned();
            Some(Entry {
                hidden: name.starts_with('.'),
                dir: meta.is_dir(),
                size: meta.len(),
                mtime: meta.modified().ok(),
                mode: meta.mode(),
                uid: meta.uid(),
                name,
                path,
                link,
            })
        })
        .collect();
    v.sort_by(|a, b| (!a.dir, a.hidden).cmp(&(!b.dir, b.hidden)).then_with(|| natural(&a.name, &b.name)));
    Ok(v)
}

/// file2 before file10, case ignored.
fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let num = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s.trim_start_matches('0').to_string()
                };
                let (s, t) = (num(&mut x), num(&mut y));
                let o = s.len().cmp(&t.len()).then(s.cmp(&t));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

fn git_status(dir: PathBuf) -> mpsc::Receiver<(PathBuf, HashMap<String, char>)> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let git = |args: &[&str]| {
            let o = Command::new("git").arg("-C").arg(&dir).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
            o.status.success().then_some(o.stdout)
        };
        let Some(prefix) = git(&["rev-parse", "--show-prefix"]) else { return };
        let prefix = String::from_utf8_lossy(&prefix).trim_end().to_string();
        let Some(out) = git(&["status", "--porcelain=v1", "-z", "--no-renames", "--", "."]) else { return };
        let mut map = HashMap::new();
        for rec in out.split(|&b| b == 0).filter(|r| r.len() > 3) {
            let (x, y) = (rec[0] as char, rec[1] as char);
            let path = String::from_utf8_lossy(&rec[3..]);
            let Some(rel) = path.strip_prefix(prefix.as_str()) else { continue };
            let c = if x == '?' { '?' } else if y != ' ' { y } else { x };
            map.entry(rel.split('/').next().unwrap_or("").to_string()).and_modify(|o: &mut char| if *o == '?' { *o = c }).or_insert(c);
        }
        let _ = tx.send((dir, map));
    });
    rx
}

fn git_summary(git: &HashMap<String, char>) -> Span<'static> {
    if git.is_empty() { "".into() } else { format!("   {} changed", git.len()).fg(pal::ROSE) }
}

/// Runs something in its own session with no terminal, so it can't scribble on the view or die with it.
fn detach(mut c: Command) -> io::Result<()> {
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
}

fn tilde(p: &Path) -> String {
    let home = jump::home();
    match p.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn fit(s: &str, w: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    let total: usize = s.chars().map(|c| c.width().unwrap_or(0)).sum();
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if total > w && used + cw >= w {
            out.push('…');
            used += 1;
            break;
        }
        out.push(c);
        used += cw;
    }
    out + &" ".repeat(w.saturating_sub(used))
}

fn human(n: u64) -> String {
    const UNITS: [&str; 5] = ["", "k", "M", "G", "T"];
    let (mut f, mut i) = (n as f64, 0);
    while f >= 1000.0 && i < 4 {
        f /= 1024.0;
        i += 1;
    }
    match i {
        0 => n.to_string(),
        _ if f < 10.0 => format!("{f:.1}{}", UNITS[i]),
        _ => format!("{f:.0}{}", UNITS[i]),
    }
}

fn ago(t: Option<SystemTime>) -> String {
    let Some(s) = t.and_then(|t| t.elapsed().ok()).map(|d| d.as_secs()) else { return String::new() };
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{}m ago", s / 60),
        3600..86400 => format!("{}h ago", s / 3600),
        86400..172800 => "yesterday".into(),
        172800..2592000 => format!("{}d ago", s / 86400),
        2592000..31536000 => format!("{}mo ago", s / 2592000),
        _ => format!("{}y ago", s / 31536000),
    }
}

fn icon(e: &Entry) -> char {
    icons::icon(&e.name, e.dir, e.link.is_some(), e.mode & 0o111 != 0, false)
}
