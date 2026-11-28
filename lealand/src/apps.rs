//! "Open with": the installed .desktop apps for a file's type, and the one you picked for that type.

use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone)]
pub struct App {
    pub name: String,
    pub generic: String,
    pub exec: String,
    pub terminal: bool,
    pub path: PathBuf,
    mimes: Vec<String>,
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

pub fn mime(file: &Path) -> String {
    run("xdg-mime", &["query", "filetype", &file.to_string_lossy()]).unwrap_or_else(|| "application/octet-stream".into())
}

fn saved_file() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME").map_or_else(|| super::jump::home().join(".config"), PathBuf::from).join("lealand/apps")
}

/// The app picked for this type before, if it's still installed.
pub fn saved(mime: &str) -> Option<App> {
    let text = fs::read_to_string(saved_file()).ok()?;
    text.lines().find_map(|l| l.strip_prefix(mime)?.strip_prefix(' ').map(PathBuf::from)).and_then(|p| parse(&p))
}

pub fn save(mime: &str, app: &App) {
    let file = saved_file();
    let old = fs::read_to_string(&file).unwrap_or_default();
    let mut text: String = old.lines().filter(|l| l.split(' ').next() != Some(mime)).map(|l| format!("{l}\n")).collect();
    text += &format!("{mime} {}\n", app.path.display());
    let _ = fs::create_dir_all(file.parent().unwrap());
    let _ = fs::write(file, text);
}

fn dirs() -> Vec<PathBuf> {
    let home = super::jump::home();
    let data = env::var_os("XDG_DATA_DIRS").map_or("/usr/local/share:/usr/share".into(), |s| s.to_string_lossy().into_owned());
    let mut v = vec![home.join(".local/share/applications"), home.join(".local/share/flatpak/exports/share/applications")];
    v.extend(data.split(':').map(|d| Path::new(d).join("applications")));
    v.push("/var/lib/flatpak/exports/share/applications".into());
    v
}

/// Every app, the ones that say they handle `mime` first (the system default at the very top).
/// The number is how many of them handle it.
pub fn list(mime: &str) -> (Vec<App>, usize) {
    let default = run("xdg-mime", &["query", "default", mime]);
    let mut seen = HashSet::new();
    let mut all: Vec<App> = dirs()
        .iter()
        .flat_map(|d| fs::read_dir(d).into_iter().flatten().flatten())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".desktop") && seen.insert(e.file_name()))
        .filter_map(|e| parse(&e.path()))
        .collect();
    let family = format!("{}/*", mime.split('/').next().unwrap_or(""));
    let handles = |a: &App| a.mimes.iter().any(|m| m == mime || *m == family || (mime.starts_with("text/") && m == "text/plain"));
    let rank = |a: &App| {
        let is_default = default.as_deref() == a.path.file_name().and_then(|n| n.to_str());
        (!is_default, !handles(a), a.name.to_lowercase())
    };
    all.sort_by_cached_key(rank);
    let n = all.iter().filter(|a| handles(a)).count();
    (all, n)
}

fn parse(path: &Path) -> Option<App> {
    let text = fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let (mut name, mut generic, mut exec, mut terminal, mut mimes) = (None, String::new(), None, false, Vec::new());
    for line in text.lines() {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        let Some((k, v)) = line.split_once('=').filter(|_| in_entry) else { continue };
        match k.trim() {
            "Name" => name = Some(v.to_string()),
            "GenericName" => generic = v.to_string(),
            "Exec" => exec = Some(v.to_string()),
            "Terminal" => terminal = v == "true",
            "MimeType" => mimes = v.split(';').filter(|m| !m.is_empty()).map(String::from).collect(),
            "NoDisplay" | "Hidden" if v == "true" => return None,
            "Type" if v != "Application" => return None,
            _ => {}
        }
    }
    Some(App { name: name?, generic, exec: exec?, terminal, path: path.to_path_buf(), mimes })
}

/// The app's Exec line with the file filled in.
pub fn command(app: &App, file: &Path) -> Command {
    let f = file.to_string_lossy();
    let mut placed = false;
    let mut args: Vec<String> = Vec::new();
    for w in words(&app.exec) {
        if ["%i", "%c", "%k", "@@", "@@u", "@@f"].contains(&w.as_str()) {
            continue;
        }
        let mut s = w.replace("%%", "\0");
        for code in ["%f", "%F", "%u", "%U"] {
            if s.contains(code) {
                s = s.replace(code, &f);
                placed = true;
            }
        }
        args.push(s.replace('\0', "%"));
    }
    if !placed {
        args.push(f.into_owned());
    }
    let mut c = Command::new(&args[0]);
    c.args(&args[1..]);
    c
}

fn words(s: &str) -> Vec<String> {
    let (mut v, mut cur, mut quoted, mut escaped, mut any) = (Vec::new(), String::new(), false, false, false);
    for c in s.chars() {
        match c {
            _ if escaped => {
                cur.push(c);
                escaped = false;
            }
            '\\' => escaped = true,
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    v.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        v.push(cur);
    }
    v
}
