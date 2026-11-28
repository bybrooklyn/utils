//! ll: an eza-style listing you can walk around in, drawn inline under the prompt.
//! It prints where you ended up (and maybe a command) on stdout; the zsh `ll` function cds and runs it.

mod apps;
mod jump;

use crokey::{KeyCombination, key};
use ratatui::{
    Frame, Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
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

/// Keys of a chord must come within this long of each other.
const GAP: Duration = Duration::from_millis(300);

/// `keys action [command]`. ~/.config/ll/chords uses the same format and wins; `xx none` removes one.
const DEFAULT_CHORDS: &str = "
qq    quit
?     help
fu    full
op    open-with
cl    exit  claude
fi    spawn dolphin .
code  spawn code .
tt    spawn ghostty +new-tab --working-directory=.
";

type Term = Terminal<CrosstermBackend<io::Stdout>>;

#[derive(Clone)]
enum Action {
    Quit,
    Help,
    Full,
    OpenWith,
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
    Pick { file: PathBuf, mime: String, apps: Vec<apps::App>, handles: usize, filter: String, sel: usize },
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
    preview: Option<(PathBuf, Vec<Line<'static>>)>,
    msg: Option<(String, Instant)>,
    index: Arc<jump::Index>,
    uid: u32,
    users: HashMap<u32, String>,
    tty_cmd: Option<Command>,
    done: bool,
    answer: Option<String>,
}

fn main() {
    if env::args_os().len() > 1 {
        eprintln!("ll takes no arguments: run it, walk, and qq to land there");
        std::process::exit(2);
    }
    // stdout is the zsh function's $(...); keep it for the answer and point fd 1 at the terminal
    let answer_fd = unsafe { libc::dup(1) };
    if let Ok(tty) = fs::OpenOptions::new().read(true).write(true).open("/dev/tty") {
        unsafe { libc::dup2(tty.as_raw_fd(), 1) };
    }
    std::panic::set_hook(Box::new(|info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
        eprintln!("ll crashed: {info}");
    }));
    let dir = env::current_dir().unwrap_or_else(|_| jump::home());
    let mut app = App::new(dir);
    let result = terminal::enable_raw_mode().and_then(|_| run(&mut app));
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
    while !app.done {
        if app.toggle_full {
            app.toggle_full = false;
            erase(&mut term, app.full)?;
            app.full = !app.full;
            term = if app.full {
                execute!(io::stdout(), EnterAlternateScreen)?;
                Terminal::new(CrosstermBackend::new(io::stdout()))?
            } else {
                h = app.height();
                inline(h)?
            };
        }
        if let Some(mut cmd) = app.tty_cmd.take() {
            erase(&mut term, app.full)?;
            terminal::disable_raw_mode()?;
            execute!(io::stdout(), cursor::Show)?;
            let tty = || File::options().write(true).open("/dev/tty").map_or(Stdio::inherit(), Stdio::from);
            if let Err(e) = cmd.stdout(tty()).status() {
                app.flash(format!("couldn't run it: {e}"));
            }
            terminal::enable_raw_mode()?;
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
        let wait = if app.busy() { Duration::from_millis(30) } else { Duration::from_secs(1) };
        if event::poll(wait)? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.key(k);
                }
            }
        }
        app.tick();
    }
    erase(&mut term, app.full)
}

impl App {
    fn new(dir: PathBuf) -> Self {
        let (chords, err) = load_chords();
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
            preview: None,
            msg: err.map(|e| (e, Instant::now())),
            index: jump::start(),
            uid: unsafe { libc::getuid() },
            users: HashMap::new(),
            tty_cmd: None,
            done: false,
            answer: None,
        };
        app.git_rx = Some(git_status(dir));
        app
    }

    fn max_height(&self) -> u16 {
        let rows = terminal::size().map_or(24, |s| s.1);
        (rows * 2 / 5).max(6).min(rows)
    }

    fn height(&self) -> u16 {
        let want = match &self.mode {
            Mode::Browse => self.entries.len().max(1),
            Mode::Help => self.chords.len() + 8,
            Mode::Jump { .. } => self.entries.len().max(8),
            Mode::Pick { .. } => 12,
        };
        let cap = if matches!(self.mode, Mode::Help) { terminal::size().map_or(24, |s| s.1) } else { self.max_height() };
        (want as u16 + 1).min(cap)
    }

    fn busy(&self) -> bool {
        !self.pending.is_empty()
            || self.esc_at.is_some()
            || self.msg.is_some()
            || self.git_rx.is_some()
            || matches!(self.mode, Mode::Jump { .. })
    }

    fn flash(&mut self, s: impl Into<String>) {
        self.msg = Some((s.into(), Instant::now()));
    }

    fn tick(&mut self) {
        if !self.pending.is_empty() && self.pending_at.elapsed() > GAP {
            self.pending.clear();
        }
        if self.esc_at.is_some_and(|t| t.elapsed() > GAP) {
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
            } else if hits.is_empty() && !q.is_empty() && self.index.ready.load(std::sync::atomic::Ordering::Relaxed) {
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
        self.git_rx = Some(git_status(dir.clone()));
        jump::record(&dir);
    }

    fn step(&mut self, by: isize) {
        let last = self.entries.len().saturating_sub(1) as isize;
        self.sel = (self.sel as isize + by).clamp(0, last.max(0)) as usize;
    }

    fn key(&mut self, k: KeyEvent) {
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
                    _ => {}
                }
            }
            Mode::Browse => match (ch, combo) {
                (Some('/'), _) => {
                    self.pending.clear();
                    self.mode = Mode::Jump { q: String::new(), at: Instant::now(), sel: 0, hits: Vec::new() };
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
                (_, key!(right) | key!(enter)) => match self.selected() {
                    Some(e) if e.dir => self.go(e.path.clone(), None, true),
                    Some(e) => self.open(e.path.clone(), false),
                    None => {}
                },
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

    fn search(&mut self, q: &str) {
        let local: Vec<PathBuf> = self.entries.iter().filter(|e| e.dir).map(|e| e.path.clone()).collect();
        let found = self.index.search(q, &self.dir, &local);
        if let Mode::Jump { sel, hits, .. } = &mut self.mode {
            *hits = found;
            *sel = 0;
        }
    }

    fn feed(&mut self, c: char) {
        if self.pending_at.elapsed() > GAP {
            self.pending.clear();
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
        if let Some(ch) = self.chords.iter().find(|ch| ch.keys == self.pending) {
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
            Action::Spawn(cmd) => {
                let mut c = Command::new("sh");
                c.arg("-c").arg(&cmd).current_dir(&self.dir);
                if let Some(e) = self.selected() {
                    c.env("f", &e.path);
                }
                if let Err(e) = detach(c) {
                    self.flash(format!("{cmd}: {e}"));
                }
            }
        }
    }

    /// Opens a file with the app picked for its type, or asks which app that should be.
    fn open(&mut self, file: PathBuf, ask: bool) {
        let mime = apps::mime(&file);
        if !ask {
            if let Some(a) = apps::saved(&mime) {
                return self.launch(&a, &file);
            }
        }
        let (apps, handles) = apps::list(&mime);
        if apps.is_empty() {
            return self.flash("no apps found");
        }
        self.mode = Mode::Pick { file, mime, apps, handles, filter: String::new(), sel: 0 };
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
        if uid != self.uid {
            f.push(self.user(uid));
        }
        if !dir && mode & 0o111 != 0 {
            f.push("exec".into());
        }
        if uid == self.uid && mode & 0o077 == 0 {
            f.push("private".into());
        }
        let writable = if uid == self.uid { mode & 0o200 != 0 } else { mode & 0o002 != 0 };
        if !writable && self.uid != 0 {
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
        let area = f.area();
        let (main, side) = if self.full && area.width >= 70 {
            let w = area.width * 11 / 20;
            (Rect { width: w, ..area }, Some(Rect { x: area.x + w, width: area.width - w, ..area }))
        } else {
            (area, None)
        };
        let head = Rect { height: 1, ..main };
        let body = Rect { y: main.y + 1, height: main.height.saturating_sub(1), ..main };
        let mut lines: Vec<Line> = Vec::new();
        let right: Line;
        let left: Line;
        let rows = body.height as usize;
        match &self.mode {
            Mode::Browse => {
                left = Line::from(vec![" ".into(), tilde(&self.dir).bold(), git_summary(&self.git)]);
                right = self.status();
                if self.sel < self.scroll {
                    self.scroll = self.sel;
                }
                if self.sel >= self.scroll + rows {
                    self.scroll = self.sel + 1 - rows;
                }
                self.rows = rows.max(1);
                if self.entries.is_empty() {
                    lines.push(Line::from("   empty".dark_gray()));
                }
                for i in self.scroll..(self.scroll + rows).min(self.entries.len()) {
                    lines.push(self.row(i, body.width as usize));
                }
            }
            Mode::Help => {
                left = Line::from(" chords and keys".bold());
                right = Line::from("any key closes ".dark_gray());
                for c in &self.chords {
                    lines.push(Line::from(vec![format!("  {:<8}", c.keys).yellow(), describe(&c.act).into()]));
                }
                for (k, d) in [
                    ("← →", "parent / open"),
                    ("↑ ↓", "move, with ctrl 5 at a time"),
                    ("pgup pgdn", "page, home/end for first/last"),
                    ("backspace", "back to the last folder"),
                    ("/", "jump to any folder: type a bit, tab to go"),
                    ("esc esc", "quit, shell stays where it was"),
                ] {
                    lines.push(Line::from(vec![format!("  {k:<10}").cyan(), d.into()]));
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
                left = Line::from(vec![" /".yellow().bold(), q.clone().bold(), rest.dark_gray()]);
                right = Line::from(top.map(|p| tilde(p)).unwrap_or_default().dark_gray());
                if hits.is_empty() {
                    let ready = self.index.ready.load(std::sync::atomic::Ordering::Relaxed);
                    let say = if q.is_empty() { "type part of a folder name" } else if ready { "no match" } else { "still indexing ~" };
                    lines.push(Line::from(format!("   {say}").dark_gray()));
                }
                for (i, p) in hits.iter().enumerate().take(rows) {
                    let name = p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                    let parent = p.parent().map(tilde).unwrap_or_default();
                    let mut l = Line::from(vec!["  \u{f07b} ".blue(), name.blue().bold(), format!("  {parent}").dark_gray()]);
                    if i == *sel {
                        l = l.style(Style::new().bg(SEL_BG));
                    }
                    lines.push(l);
                }
            }
            Mode::Pick { file, mime, apps, handles, filter, sel } => {
                let name = file.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                left = Line::from(vec![" open ".into(), name.bold(), " with".into()]);
                right = Line::from(vec![filter.clone().yellow(), format!("  {mime} ").dark_gray()]);
                let shown = picked(apps, filter);
                let start = sel.saturating_sub(rows.saturating_sub(1));
                for (row, &i) in shown.iter().enumerate().skip(start).take(rows) {
                    let a = &apps[i];
                    let fits = i < *handles;
                    let mut spans = vec![
                        "  ".into(),
                        if fits { a.name.clone().bold() } else { a.name.clone().dark_gray() },
                        format!("  {}", a.generic).dark_gray(),
                    ];
                    if i == 0 && fits && filter.is_empty() {
                        spans.push("  default".green());
                    }
                    if a.terminal {
                        spans.push("  terminal".cyan());
                    }
                    let mut l = Line::from(spans);
                    if row == *sel {
                        l = l.style(Style::new().bg(SEL_BG));
                    }
                    lines.push(l);
                }
                if shown.is_empty() {
                    lines.push(Line::from("   nothing matches".dark_gray()));
                }
            }
        }
        let rw = right.width() as u16;
        f.render_widget(Paragraph::new(left), Rect { width: head.width.saturating_sub(rw + 1), ..head });
        f.render_widget(Paragraph::new(right), Rect { x: head.x + head.width.saturating_sub(rw), width: rw.min(head.width), ..head });
        f.render_widget(Paragraph::new(lines), body);
        if let Some(side) = side {
            let lines = self.preview_lines(side.height as usize);
            let block = ratatui::widgets::Block::new().borders(ratatui::widgets::Borders::LEFT).border_style(Style::new().dark_gray());
            f.render_widget(Paragraph::new(lines).block(block), side);
        }
    }

    fn status(&self) -> Line<'static> {
        if !self.pending.is_empty() {
            return Line::from(format!("{}… ", self.pending).yellow().bold());
        }
        if self.esc_at.is_some() {
            return Line::from("esc again to leave ".yellow());
        }
        if let Some((m, _)) = &self.msg {
            return Line::from(format!("{m} ").red());
        }
        let pos = if self.entries.len() > self.rows { format!("{}/{}  ", self.sel + 1, self.entries.len()) } else { String::new() };
        Line::from(format!("{pos}? help ").dark_gray())
    }

    fn row(&mut self, i: usize, width: usize) -> Line<'static> {
        let flags = self.flags(i);
        let size = {
            let e = &self.entries[i];
            if e.dir { self.count(&e.path.clone()) } else { human(e.size) }
        };
        let e = &self.entries[i];
        let git = self.git.get(&e.name).copied();
        let base = if e.hidden {
            Style::new().fg(Color::DarkGray)
        } else if e.dir {
            Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD)
        } else if e.link.is_some() {
            Style::new().fg(Color::Cyan)
        } else if e.mode & 0o111 != 0 {
            Style::new().fg(Color::Green)
        } else {
            Style::new()
        };
        let mut name = e.name.clone();
        if let Some(t) = &e.link {
            name += &format!(" → {t}");
        }
        let fixed = 4 + 10 + 11 + 2 + flags.chars().count() + 1;
        let name_w = width.saturating_sub(fixed).max(8);
        let dim = Style::new().fg(Color::DarkGray);
        let (gc, gs) = match git {
            Some('?') => ("?", Style::new().fg(Color::Magenta)),
            Some('D') => ("D", Style::new().fg(Color::Red)),
            Some('A') => ("A", Style::new().fg(Color::Green)),
            Some(c) => (if c == 'M' { "M" } else { "~" }, Style::new().fg(Color::Yellow)),
            None => (" ", dim),
        };
        let mut l = Line::from(vec![
            Span::styled(format!(" {} ", icon(e)), base.remove_modifier(Modifier::BOLD)),
            Span::styled(fit(&name, name_w), base),
            Span::styled(format!("{size:>9}"), dim),
            Span::styled(format!("  {:<9}", ago(e.mtime)), dim),
            Span::styled(format!(" {gc}"), gs),
            Span::styled(format!(" {flags}"), Style::new().fg(Color::Yellow)),
        ]);
        if i == self.sel {
            l = l.style(Style::new().bg(SEL_BG));
        }
        l
    }

    fn preview_lines(&mut self, n: usize) -> Vec<Line<'static>> {
        let Some(e) = self.selected() else { return Vec::new() };
        let path = e.path.clone();
        if let Some((p, lines)) = &self.preview {
            if *p == path {
                return lines.clone();
            }
        }
        let lines: Vec<Line<'static>> = if e.dir {
            match read_entries(&path) {
                Ok(v) if v.is_empty() => vec![Line::from(" empty".dark_gray())],
                Ok(v) => v
                    .iter()
                    .take(n)
                    .map(|c| {
                        let s = if c.hidden { Style::new().fg(Color::DarkGray) } else if c.dir { Style::new().fg(Color::Blue) } else { Style::new() };
                        Line::from(Span::styled(format!(" {} {}", icon(c), c.name), s))
                    })
                    .collect(),
                Err(e) => vec![Line::from(format!(" {}", e.kind()).red())],
            }
        } else {
            let mut buf = vec![0; 64 * 1024];
            let got = File::open(&path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
            buf.truncate(got);
            if buf.contains(&0) {
                vec![Line::from(format!(" binary, {}", human(e.size)).dark_gray())]
            } else {
                String::from_utf8_lossy(&buf).lines().take(n).map(|l| Line::from(format!(" {}", l.replace('\t', "    ")))).collect()
            }
        };
        self.preview = Some((path, lines.clone()));
        lines
    }
}

const SEL_BG: Color = Color::Indexed(237);

fn picked(apps: &[apps::App], filter: &str) -> Vec<usize> {
    let f = filter.to_lowercase();
    (0..apps.len()).filter(|&i| f.is_empty() || apps[i].name.to_lowercase().contains(&f)).collect()
}

fn describe(a: &Action) -> String {
    match a {
        Action::Quit => "quit and cd here".into(),
        Action::Help => "this list".into(),
        Action::Full => "fullscreen with a preview, on and off".into(),
        Action::OpenWith => "open the file with another app (and remember it)".into(),
        Action::Exit(c) => format!("quit, cd here, run {c}"),
        Action::Spawn(c) => format!("run {c} here"),
    }
}

fn load_chords() -> (Vec<Chord>, Option<String>) {
    let file = env::var_os("XDG_CONFIG_HOME").map_or_else(|| jump::home().join(".config"), PathBuf::from).join("ll/chords");
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
            "exit" => Action::Exit(rest),
            "spawn" => Action::Spawn(rest),
            "none" => continue,
            _ => {
                err = Some(format!("chords: {keys} has unknown action {kind:?}"));
                continue;
            }
        };
        if let Some(c) = v.iter().find(|c| c.keys.starts_with(keys) || keys.starts_with(&c.keys)) {
            err = Some(format!("chords: {keys} clashes with {}", c.keys));
            continue;
        }
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
    if git.is_empty() { "".into() } else { format!("  {} changed", git.len()).yellow() }
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

/// Nerd Font icons, like `eza --icons`.
fn icon(e: &Entry) -> char {
    if e.dir {
        return if e.link.is_some() { '\u{f482}' } else { '\u{f07b}' };
    }
    let lower = e.name.to_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |x| x.1);
    match ext {
        "rs" => '\u{e7a8}',
        "py" => '\u{e73c}',
        "sh" | "bash" | "zsh" | "fish" => '\u{f489}',
        "md" | "markdown" | "mdx" => '\u{f48a}',
        "json" | "jsonc" => '\u{e60b}',
        "toml" | "yaml" | "yml" | "ini" | "conf" | "cfg" => '\u{e615}',
        "js" | "mjs" | "cjs" => '\u{e74e}',
        "ts" | "tsx" => '\u{e628}',
        "html" | "htm" => '\u{e736}',
        "css" | "scss" => '\u{e749}',
        "go" => '\u{e626}',
        "c" | "h" => '\u{e61e}',
        "cpp" | "cc" | "hpp" => '\u{e61d}',
        "lua" => '\u{e620}',
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "bmp" | "ico" => '\u{f1c5}',
        "mp4" | "mkv" | "webm" | "mov" | "avi" => '\u{f1c8}',
        "mp3" | "flac" | "ogg" | "wav" | "opus" | "m4a" => '\u{f1c7}',
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar" | "bz2" => '\u{f1c6}',
        "pdf" => '\u{f1c1}',
        "lock" => '\u{f023}',
        "txt" | "log" => '\u{f15c}',
        _ if lower.starts_with(".git") => '\u{f1d3}',
        _ if e.link.is_some() => '\u{f481}',
        _ if e.mode & 0o111 != 0 => '\u{f489}',
        _ => '\u{f15b}',
    }
}
