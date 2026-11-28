//! `/` jump: every directory under ~ (cached, refreshed in the background) ranked by how well its name
//! matches what you typed, typos included, and by how often and how recently you went there.

use ignore::{WalkBuilder, WalkState, overrides::OverrideBuilder};
use std::{
    collections::HashMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock, atomic::{AtomicBool, Ordering}},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Skipped by the index on top of what tokei skips (hidden folders, anything .gitignored).
/// Paths with a slash are from ~; the rest match anywhere.
const SKIP: [&str; 11] =
    ["node_modules", "target", "__pycache__", "venv", ".venv", "dist", "steamapps", "drive_c", "go/pkg", "Android/Sdk", "Steam"];
const STALE: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct Index {
    dirs: RwLock<Vec<(PathBuf, String)>>, // path, lowercase name
    frec: RwLock<HashMap<PathBuf, f64>>,
    pub ready: AtomicBool,
}

pub fn home() -> PathBuf {
    env::var_os("HOME").map_or_else(|| "/".into(), PathBuf::from)
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    env::var_os(var).map_or_else(|| home().join(fallback), PathBuf::from).join("lealand")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Same line the zsh chpwd hook writes: "<unix time> <path>".
pub fn record(dir: &Path) {
    let data = xdg("XDG_DATA_HOME", ".local/share");
    let _ = fs::create_dir_all(&data);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(data.join("visits")) {
        let _ = writeln!(f, "{} {}", now(), dir.display());
    }
}

/// Loads everything on a background thread so the listing never waits for it.
pub fn start() -> Arc<Index> {
    let ix = Arc::new(Index::default());
    let i = ix.clone();
    thread::spawn(move || {
        *i.frec.write().unwrap() = frecency();
        let file = xdg("XDG_CACHE_HOME", ".cache").join("dirs");
        if let Ok(s) = fs::read_to_string(&file) {
            *i.dirs.write().unwrap() = s.lines().map(|l| named(l.into())).collect();
            i.ready.store(true, Ordering::Relaxed);
        }
        let age = fs::metadata(&file).and_then(|m| m.modified()).map(|t| t.elapsed().unwrap_or_default());
        if age.map_or(true, |a| a > STALE) {
            let dirs = walk();
            let _ = fs::create_dir_all(file.parent().unwrap());
            let tmp = file.with_extension("tmp");
            let text: String = dirs.iter().map(|d| format!("{}\n", d.display())).collect();
            if fs::write(&tmp, text).is_ok() {
                let _ = fs::rename(&tmp, &file);
            }
            *i.dirs.write().unwrap() = dirs.into_iter().map(named).collect();
        }
        i.ready.store(true, Ordering::Relaxed);
    });
    ix
}

fn named(p: PathBuf) -> (PathBuf, String) {
    let name = p.file_name().map_or(String::new(), |n| n.to_string_lossy().to_lowercase());
    (p, name)
}

fn walk() -> Vec<PathBuf> {
    let home = home();
    let mut ov = OverrideBuilder::new(&home);
    for g in SKIP {
        ov.add(&format!("!{g}")).unwrap();
    }
    let config = home.join(".config");
    let out = Mutex::new(Vec::new());
    let out_ref = &out;
    // hidden folders are skipped like tokei does, except ~/.config, which you do go into
    WalkBuilder::new(&home)
        .add(home.join(".config"))
        .overrides(ov.build().unwrap())
        .max_depth(Some(12))
        // ~/.config/app/part, no deeper: below that it's mostly browser and Electron caches
        .filter_entry(move |e| {
            e.file_type().is_some_and(|t| t.is_dir())
                && e.path().strip_prefix(&config).map_or(true, |rest| rest.components().count() <= 2)
        })
        .threads(thread::available_parallelism().map_or(8, |n| n.get()))
        .build_parallel()
        .run(|| {
            Box::new(move |e| {
                if let Ok(e) = e {
                    out_ref.lock().unwrap().push(e.into_path());
                }
                WalkState::Continue
            })
        });
    out.into_inner().unwrap()
}

/// Each visit counts for more the more recent it is, like zoxide.
fn frecency() -> HashMap<PathBuf, f64> {
    let file = xdg("XDG_DATA_HOME", ".local/share").join("visits");
    if !file.exists() {
        seed(&file);
    }
    let text = fs::read_to_string(&file).unwrap_or_default();
    let now = now();
    let mut m = HashMap::new();
    let mut keep = String::new();
    for line in text.lines() {
        let Some((ts, path)) = line.split_once(' ') else { continue };
        let age = now.saturating_sub(ts.parse().unwrap_or(0));
        let w = match age {
            0..3600 => 4.0,
            3600..86400 => 2.0,
            86400..604800 => 1.0,
            604800..7776000 => 0.25,
            _ => continue,
        };
        *m.entry(PathBuf::from(path)).or_insert(0.0) += w;
        keep += line;
        keep.push('\n');
    }
    if text.len() > 4 << 20 {
        let _ = fs::write(&file, keep); // drop visits older than 90 days once the log gets big
    }
    m
}

/// First run: learn from the cd's in ~/.zsh_history. Only absolute and ~ paths can be placed, since
/// the history doesn't say where a relative cd started; those are tried against ~.
fn seed(file: &Path) {
    let hist = fs::read(home().join(".zsh_history")).unwrap_or_default();
    let mut out = String::new();
    for line in String::from_utf8_lossy(&hist).lines() {
        let (ts, cmd) = match line.strip_prefix(": ").and_then(|l| l.split_once(';')) {
            Some((meta, cmd)) => (meta.split(':').next().unwrap_or("0").trim(), cmd),
            None => ("0", line),
        };
        let Some(arg) = cmd.trim().strip_prefix("cd ") else { continue };
        let arg = arg.trim().trim_matches(['"', '\'']);
        if arg.starts_with('-') || arg.starts_with("..") {
            continue;
        }
        let p = match arg.strip_prefix('~') {
            Some(rest) => home().join(rest.trim_start_matches('/')),
            None if arg.starts_with('/') => arg.into(),
            None => home().join(arg),
        };
        if let Ok(p) = fs::canonicalize(p).map(PathBuf::from) {
            if p.is_dir() {
                out += &format!("{ts} {}\n", p.display());
            }
        }
    }
    let _ = fs::create_dir_all(file.parent().unwrap());
    let _ = fs::write(file, out);
}

impl Index {
    /// Places you've been, and folders in the index.
    pub fn counts(&self) -> (usize, usize) {
        (self.frec.read().unwrap().len(), self.dirs.read().unwrap().len())
    }

    /// Best directories for `q`, best first. `local` are the folders in the current directory, which
    /// count even when they're hidden or ignored.
    pub fn search(&self, q: &str, cwd: &Path, local: &[PathBuf]) -> Vec<PathBuf> {
        let q = q.to_lowercase();
        let frec = self.frec.read().unwrap();
        let dirs = self.dirs.read().unwrap();
        let mut best: HashMap<&Path, f64> = HashMap::new();
        let local: Vec<_> = local.iter().map(|p| named(p.clone())).collect();
        let frec_named: Vec<_> = frec.keys().map(|p| named(p.clone())).collect();
        // nothing typed: only the places you go
        let (local, dirs): (&[_], &[_]) = if q.is_empty() { (&[], &[]) } else { (&local, &dirs) };
        for (p, name) in local.iter().chain(&frec_named).chain(dirs.iter()) {
            if best.contains_key(p.as_path()) || p == cwd {
                continue;
            }
            let s = if q.is_empty() { 1.0 } else { score(&q, name) };
            if s <= 0.0 {
                continue;
            }
            let f = frec.get(p).map_or(0.0, |f| (1.0 + f).ln() * 10.0);
            let near = if p.parent() == Some(cwd) { 15.0 } else { 0.0 };
            best.insert(p, s + f + near - p.components().count() as f64 * 1.5);
        }
        let mut v: Vec<_> = best.into_iter().collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v.into_iter().map(|(p, _)| p.to_path_buf()).filter(|p| p.is_dir()).take(8).collect()
    }
}

/// How well a typed query matches a folder name; 0 means not at all.
fn score(q: &str, name: &str) -> f64 {
    if name == q {
        return 100.0;
    }
    if let Some(rest) = name.strip_prefix(q) {
        // "ups" is a whole word of "ups-guard", only the start of "upscale"
        return if rest.starts_with(['-', '_', '.', ' ']) { 90.0 } else { 80.0 - rest.len() as f64 * 0.5 };
    }
    if let Some(i) = name.find(q) {
        return if name.as_bytes()[i - 1].is_ascii_alphanumeric() { 50.0 } else { 65.0 };
    }
    // typos and skipped letters still have to get the first letter about right; this also keeps
    // the slow checks below off most of the 100k names
    let first = q.chars().next().unwrap_or(' ');
    if !name.chars().take(2).any(|c| c == first) {
        return 0.0;
    }
    let (qc, nc): (Vec<char>, Vec<char>) = (q.chars().collect(), name.chars().collect());
    let n = qc.len();
    if n >= 3 {
        let max = if n >= 7 { 2 } else { 1 };
        // typed the start of the name with a typo, or the whole name with one
        let d = (n - 1..=n + 1).filter(|&l| l <= nc.len()).map(|l| typos(&qc, &nc[..l])).chain([typos(&qc, &nc)]).min();
        if let Some(d) = d.filter(|&d| d <= max) {
            return 45.0 - 10.0 * d as f64;
        }
    }
    let mut it = nc.iter();
    if n >= 2 && qc.iter().all(|c| it.any(|x| x == c)) {
        return 25.0;
    }
    0.0
}

/// Edits between two words, where swapping two neighbouring letters counts as one.
fn typos(a: &[char], b: &[char]) -> usize {
    let w = b.len() + 1;
    let mut d: Vec<usize> = (0..(a.len() + 1) * w).map(|k| if k < w { k } else if k % w == 0 { k / w } else { 0 }).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[(i - 1) * w + j] + 1).min(d[i * w + j - 1] + 1).min(d[(i - 1) * w + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[(i - 2) * w + j - 2] + 1);
            }
            d[i * w + j] = v;
        }
    }
    d[a.len() * w + b.len()]
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn matches() {
        assert!(score("dow", "downloads") > score("dow", "shadow"));
        assert!(score("dwonlods", "downloads") > 0.0);
        assert!(score("utls", "utils") > 0.0);
        assert_eq!(score("xyz", "downloads"), 0.0);
        assert!(score("ups", "ups-guard") > score("ups", "upscale"));
    }

    /// cargo test --release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn speed() {
        let ix = super::start();
        while !ix.ready.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for q in ["d", "dw", "dwo", "dwon", "dwonl", "dwonlo", "dwonlod", "dwonlods", "utls", "ups"] {
            let t = std::time::Instant::now();
            let hits = ix.search(q, std::path::Path::new("/tmp"), &[]);
            println!("{q:<10} {:>6.1} ms  {:?}", t.elapsed().as_secs_f64() * 1000.0, hits.first());
        }
    }
}
