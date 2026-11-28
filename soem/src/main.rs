use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, BufWriter, IsTerminal, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, atomic::AtomicBool, atomic::Ordering::Relaxed};
use std::{env, fs, time::UNIX_EPOCH};

use ignore::{WalkBuilder, WalkState, overrides::OverrideBuilder};
use tokei::{Config, LanguageType as Lang};

const HELP: &str = "usage: soem [PATH] [-d|--depth N] [-a|--all] [-e|--exclude GLOB]... \
                    [--no-default-excludes] [--no-cache]";
const DEFAULT_EXCLUDES: [&str; 4] = ["node_modules", "target", "__pycache__", "venv"];
const CACHE_MAGIC: &[u8] = b"soem1-tokei15";
const BAR: usize = 20;
const GAP: &str = "   ";
const EIGHTHS: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

struct File {
    rel: PathBuf,
    lang: Lang,
    code: usize,
    size: u64,
    mtime: u64,
}

#[derive(Default)]
struct Node {
    code: usize,
    langs: HashMap<Lang, usize>,
    kids: HashMap<String, Node>,
}

impl Node {
    /// Main language; with `direct`, only of the files directly in this folder.
    fn top_lang(&self, direct: bool) -> Option<Lang> {
        let in_kids = |l| self.kids.values().filter_map(|k| k.langs.get(l)).sum::<usize>();
        let langs = self.langs.iter().map(|(l, c)| (*l, c - if direct { in_kids(l) } else { 0 }));
        langs.filter(|x| x.1 > 0).max_by_key(|x| x.1).map(|x| x.0)
    }

    /// Walk down folders holding just one subfolder, so a/ -> b/ -> c/ shows as "a/b/c".
    fn fold(&self, name: &mut String) -> &Node {
        let mut n = self;
        while let Some((k, sub)) = n.kids.iter().next()
            && n.kids.len() == 1
            && sub.code == n.code
        {
            if !name.ends_with('/') {
                name.push('/');
            }
            name.push_str(k);
            n = sub;
        }
        n
    }
}

#[derive(Default)]
struct Row {
    prefix: String,
    name: String, // empty: spacer line between top-level folders
    code: usize,
    frac: f64,   // share of the parent folder (bar length, %)
    global: f64, // share of the whole project (colour, fading)
    lang: Option<Lang>,
    dir: bool,
}

fn die(msg: &str) -> ! {
    eprintln!("soem: {msg}");
    std::process::exit(2)
}

fn main() {
    let (mut path, mut depth, mut all, mut cache, mut defaults) = (PathBuf::from("."), 2, false, true, true);
    let mut excludes = Vec::new();
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().and_then(|v| v.into_string().ok()).unwrap_or_else(|| die(HELP));
        match arg.to_str() {
            Some("-d" | "--depth") => depth = value().parse().unwrap_or_else(|_| die(HELP)),
            Some("-a" | "--all") => all = true,
            Some("-e" | "--exclude") => excludes.push(value()),
            Some("--no-default-excludes") => defaults = false,
            Some("--no-cache") => cache = false,
            Some("-h" | "--help") => return println!("{HELP}"),
            Some(s) if s.starts_with('-') => die(HELP),
            _ => path = arg.into(),
        }
    }
    if defaults {
        excludes.extend(DEFAULT_EXCLUDES.map(String::from));
    }
    if !path.exists() {
        die(&format!("{}: no such file or directory", path.display()));
    }
    let cache_file = (cache.then(|| fs::canonicalize(&path).ok()).flatten()).and_then(|canon| {
        let base = env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
        let base = base.or_else(|| Some(PathBuf::from(env::var_os("HOME")?).join(".cache")))?;
        Some(base.join("soem").join(format!("{:016x}", hash(&canon))))
    });

    let files = scan(&path, &excludes, cache_file.as_deref());
    let mut tree = Node::default();
    for f in files.iter().filter(|f| f.code > 0) {
        let mut node = &mut tree;
        *node.langs.entry(f.lang).or_default() += f.code;
        node.code += f.code;
        for comp in f.rel.parent().into_iter().flat_map(Path::components) {
            let Component::Normal(name) = comp else {
                continue;
            };
            node = node.kids.entry(name.to_string_lossy().into_owned()).or_default();
            *node.langs.entry(f.lang).or_default() += f.code;
            node.code += f.code;
        }
    }
    let mut shown = path.display().to_string();
    if let Some(rest) = env::var("HOME").ok().and_then(|h| shown.strip_prefix(&h).map(String::from))
        && (rest.is_empty() || rest.starts_with('/'))
    {
        shown = format!("~{rest}");
    }
    let _ = render(shown, &tree, files.len(), depth, all);
}

fn hash(p: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    p.hash(&mut h);
    h.finish()
}

/// Walk in parallel and count each file with tokei, reusing cached counts for
/// files whose size and mtime are unchanged.
fn scan(root: &Path, excludes: &[String], cache_file: Option<&Path>) -> Vec<File> {
    let data = cache_file.and_then(|p| fs::read(p).ok()).unwrap_or_default();
    let mut lines = data.split(|b| *b == b'\n');
    let cache: HashMap<&Path, (u64, u64, usize)> = if lines.next() == Some(CACHE_MAGIC) {
        lines
            .filter_map(|l| {
                let mut f = l.splitn(4, |b| *b == b'\t');
                let mut num = || std::str::from_utf8(f.next()?).ok()?.parse().ok();
                let (size, mtime, code) = (num()?, num()?, num()?);
                Some((Path::new(OsStr::from_bytes(f.next()?)), (size, mtime, code as usize)))
            })
            .collect()
    } else {
        HashMap::new()
    };

    let mut overrides = OverrideBuilder::new(root);
    for g in excludes {
        overrides.add(&format!("!{g}")).unwrap_or_else(|e| die(&format!("bad exclude {g:?}: {e}")));
    }
    let (cfg, out, dirty) = (Config::default(), Mutex::new(Vec::new()), AtomicBool::new(false));
    let (cfg, out_ref, dirty_ref, cache_ref) = (&cfg, &out, &dirty, &cache);
    WalkBuilder::new(root)
        .add_custom_ignore_filename(".tokeignore") // same ignore rules as tokei
        .overrides(overrides.build().unwrap())
        .threads(std::thread::available_parallelism().map_or(8, |n| n.get()))
        .build_parallel()
        .run(|| {
            Box::new(move |entry| {
                let file = (|| {
                    let entry = entry.ok().filter(|e| e.file_type().is_some_and(|t| t.is_file()))?;
                    let (path, meta) = (entry.path(), entry.metadata().ok()?);
                    let lang = Lang::from_path(path, cfg)?;
                    let (size, mtime) = (meta.len(), meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?);
                    let rel = path.strip_prefix(root).unwrap_or(path);
                    let code = match cache_ref.get(rel) {
                        Some(&(s, m, c)) if (s, m) == (size, mtime.as_nanos() as u64) => c,
                        _ => {
                            dirty_ref.store(true, Relaxed);
                            lang.parse(path.into(), cfg).ok()?.stats.summarise().code
                        }
                    };
                    let mtime = mtime.as_nanos() as u64;
                    Some(File { rel: rel.into(), lang, code, size, mtime })
                })();
                out_ref.lock().unwrap().extend(file);
                WalkState::Continue
            })
        });

    let files = out.into_inner().unwrap();
    if let Some(p) = cache_file.filter(|_| dirty.into_inner() || files.len() != cache.len()) {
        let _ = (|| -> io::Result<()> {
            let mut buf = [CACHE_MAGIC, b"\n"].concat();
            for f in files.iter().filter(|f| !f.rel.as_os_str().as_bytes().contains(&b'\n')) {
                buf.extend(format!("{}\t{}\t{}\t", f.size, f.mtime, f.code).bytes());
                buf.extend(f.rel.as_os_str().as_bytes());
                buf.push(b'\n');
            }
            fs::create_dir_all(p.parent().unwrap())?;
            let tmp = p.with_extension(std::process::id().to_string());
            fs::write(&tmp, buf)?;
            fs::rename(tmp, p)
        })();
    }
    files
}

fn collect(node: &Node, grand: f64, prefix: &str, depth: usize, all: bool, rows: &mut Vec<Row>) {
    if depth == 0 {
        return;
    }
    let total = node.code.max(1) as f64;
    let direct = node.code - node.kids.values().map(|k| k.code).sum::<usize>();
    let mut items: Vec<_> = node.kids.iter().map(|k| (Some(k), k.1.code)).collect();
    if direct > 0 && !items.is_empty() {
        items.push((None, direct)); // the loose files in this folder
    }
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.map(|k| k.0).cmp(&b.0.map(|k| k.0))));
    // One child is (almost) all of this folder: its row would just repeat the parent's.
    if !prefix.is_empty() && items.first().is_some_and(|i| i.1 as f64 >= 0.95 * total) {
        return;
    }
    let (mut shown, mut hidden): (Vec<_>, Vec<_>) = items.into_iter().partition(|i| all || i.1 as f64 / total >= 0.01);
    if hidden.len() == 1 {
        shown.append(&mut hidden); // "… 1 more" would take the same line as the item itself
    }
    let n = shown.len() + usize::from(!hidden.is_empty());

    for (i, (item, code)) in shown.into_iter().enumerate() {
        let last = i + 1 == n;
        let branch = format!("{prefix}{}", if last { "└─ " } else { "├─ " });
        let (frac, global) = (code as f64 / total, code as f64 / grand);
        let row = |name, lang, dir| Row { prefix: branch, name, code, frac, global, lang, dir };
        let Some((name, kid)) = item else {
            rows.push(row("(files)".into(), node.top_lang(true), false));
            continue;
        };
        let mut name = name.clone();
        let kid = kid.fold(&mut name);
        rows.push(row(name + "/", kid.top_lang(false), true));
        let before = rows.len();
        collect(kid, grand, &format!("{prefix}{}", if last { "   " } else { "│  " }), depth - 1, all, rows);
        if prefix.is_empty() && !last && rows.len() > before {
            rows.push(Row { prefix: "│".into(), ..Default::default() });
        }
    }
    if !hidden.is_empty() {
        let code: usize = hidden.iter().map(|i| i.1).sum();
        let name = format!("… {} more", hidden.len());
        rows.push(Row {
            prefix: format!("{prefix}└─ "),
            name,
            code,
            frac: code as f64 / total,
            global: code as f64 / grand,
            ..Default::default()
        });
    }
}

/// 1234 -> "1.2k", 123456 -> "123k", 1234567 -> "1.2M".
fn short(n: usize) -> String {
    let (mut v, mut units) = (n as f64, ["", "k", "M", "B"].iter());
    let mut unit = units.next().unwrap();
    while v >= 999.5 && units.len() > 0 {
        (v, unit) = (v / 1000.0, units.next().unwrap());
    }
    if unit.is_empty() || v >= 99.95 { format!("{v:.0}{unit}") } else { format!("{v:.1}{unit}") }
}

/// 1234567 -> "1,234,567".
fn full(n: usize) -> String {
    let s = n.to_string();
    let digits = s.chars().enumerate();
    digits.map(|(i, c)| if i > 0 && (s.len() - i).is_multiple_of(3) { format!(",{c}") } else { c.into() }).collect()
}

fn pct(frac: f64) -> String {
    if frac > 0.0 && frac < 0.01 { "<1%".into() } else { format!("{:.0}%", frac * 100.0) }
}

/// Cut to `max` chars with an ellipsis, keeping the start (or the end, for paths).
fn trunc(s: &str, max: usize, keep_end: bool) -> String {
    let n = s.chars().count();
    match () {
        _ if n <= max => s.into(),
        _ if keep_end => "…".chars().chain(s.chars().skip(n + 1 - max.max(1))).collect(),
        _ => s.chars().take(max.saturating_sub(1)).chain("…".chars()).collect(),
    }
}

fn render(mut root: String, tree: &Node, n_files: usize, depth: usize, all: bool) -> io::Result<()> {
    let tty = io::stdout().is_terminal();
    let color = tty && env::var_os("NO_COLOR").is_none();
    let c = |code: &'static str| if color { code } else { "" };
    let (track_c, dim, bold, rs) = (c("\x1b[38;2;70;76;88m"), c("\x1b[2m"), c("\x1b[1m"), c("\x1b[0m"));
    let ws = winsize().filter(|_| tty);

    let tree = tree.fold(&mut root);
    let mut rows = Vec::new();
    collect(tree, tree.code.max(1) as f64, "", depth, all, &mut rows);
    // The longer the bar, the darker: #6496e6 for an empty bar down to #3c5a96 for a full one.
    let shade = |frac: f64| {
        let mix = |empty: f64, full: f64| (empty + (full - empty) * frac.clamp(0.0, 1.0)).round() as u8;
        let rgb = (mix(100.0, 60.0), mix(150.0, 90.0), mix(230.0, 150.0));
        if color { format!("\x1b[38;2;{};{};{}m", rgb.0, rgb.1, rgb.2) } else { String::new() }
    };

    // Fit the terminal: shrink the name column first, then the bar. No limit when piped.
    let cols = ws.map_or(usize::MAX, |w| w.ws_col as usize);
    let natural = rows.iter().map(|r| r.prefix.chars().count() + r.name.chars().count()).max().unwrap_or(0);
    let num_w = rows.iter().map(|r| short(r.code).len()).max().unwrap_or(1);
    let lang_w = rows.iter().filter_map(|r| r.lang).map(|l| l.name().len()).max().unwrap_or(0);
    let avail = cols.saturating_sub(4 * GAP.len() + num_w + 4 + lang_w);
    let (mut width, mut bar_w) = (natural, BAR);
    if width + bar_w > avail {
        width = avail.saturating_sub(bar_w).max(16).min(natural);
        bar_w = avail.saturating_sub(width).clamp(6, BAR);
    }

    let mut out = BufWriter::new(io::stdout().lock());
    let plural = |n, s| format!("{s}{}", if n == 1 { "" } else { "s" });
    let root = trunc(&root, cols.saturating_sub(40).max(10), true);
    let (code, files) = (full(tree.code), full(n_files));
    let (lines_w, files_w) = (plural(tree.code, "line"), plural(n_files, "file"));
    writeln!(out, "{bold}{root}{rs}{GAP}{bold}{code}{rs} {dim}{lines_w} of code in{rs} {files} {dim}{files_w}{rs}")?;

    for r in &rows {
        if r.name.is_empty() {
            writeln!(out, "{dim}{}{rs}", r.prefix)?;
            continue;
        }
        let faded = r.global < 0.02;
        let (bar_c, num_c) = (shade(r.frac), if faded { dim } else { "" });
        let name_c = if r.dir && !faded { bold } else { dim };
        let units = (r.frac.clamp(0.0, 1.0) * (bar_w * 8) as f64).round() as usize;
        let (full_cells, rem) = (units / 8, units % 8);
        let partial = if rem > 0 && full_cells < bar_w { EIGHTHS[rem].to_string() } else { String::new() };
        let filled = "█".repeat(full_cells) + &partial;
        let track = "·".repeat(bar_w - full_cells - partial.len().min(1));
        let prefix = trunc(&r.prefix, width, false);
        let name = trunc(&r.name, width - prefix.chars().count(), false);
        let pad = width - prefix.chars().count() - name.chars().count();
        let (num, p, lang) = (short(r.code), pct(r.frac), r.lang.map_or("", |l| l.name()));
        writeln!(
            out,
            "{dim}{prefix}{rs}{name_c}{name}{rs}{:pad$}{GAP}{bar_c}{filled}{rs}{track_c}{track}{rs}{GAP}{num_c}{num:>num_w$}{rs}{GAP}{bar_c}{p:>4}{rs}{GAP}{dim}{lang}{rs}",
            ""
        )?;
    }
    out.flush()
}

fn winsize() -> Option<libc::winsize> {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    (unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0).then_some(ws)
}
