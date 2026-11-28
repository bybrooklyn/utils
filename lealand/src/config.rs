//! ~/.config/lealand/lealand.toml (settings and remembered apps) and lealand.csv (the chords).
//! Both are written the first time ll runs; after that they're yours, and when ll changes a
//! setting it edits just that line, so your comments and changes stay.

use std::{collections::HashMap, env, fs, path::PathBuf, time::Duration};
use toml_edit::{DocumentMut, value};

pub fn dir() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME").map_or_else(|| super::jump::home().join(".config"), PathBuf::from).join("lealand")
}

fn toml_path() -> PathBuf {
    dir().join("lealand.toml")
}

pub fn csv_path() -> PathBuf {
    dir().join("lealand.csv")
}

pub const SKIP: [&str; 11] = ["node_modules", "target", "__pycache__", "venv", ".venv", "dist", "steamapps", "drive_c", "go/pkg", "Android/Sdk", "Steam"];

const DEFAULT_TOML: &str = r#"# lealand settings. ll wrote this the first time it ran; change anything you like, ll keeps
# your edits. The chords are in lealand.csv next to this file.

[look]
# the launch and quit animations (ll --disable-flag and ll --enable-flag set this)
animations = true
# how much of the terminal the inline view may take, in percent
max_height = 40
# dotfiles: "dim" (after everything else, greyed), "show" (mixed in) or "hide"
hidden = "dim"

[chords]
# how long you have between the keys of a chord, in milliseconds
gap_ms = 1000

[jump]
# folders the / index skips, on top of hidden ones and anything a .gitignore covers.
# A name with a slash is a path from ~.
skip = SKIP_LIST

[openers]
# the app picked for each kind of file. op changes these; del in the picker forgets one.
"#;

/// The chords ll starts with: keys, action, command.
pub const DEFAULT_CHORDS: [(&str, &str, &str); 11] = [
    ("qq", "quit", ""),
    ("?", "help", ""),
    ("fu", "full", ""),
    ("op", "open-with", ""),
    ("cl", "exit", "claude"),
    ("fi", "spawn", "dolphin ."),
    ("code", "spawn", "code ."),
    ("mk", "mkdir", ""),
    ("mkcl", "mkdir-exit", "claude"),
    ("rm", "trash", ""),
    ("tt", "spawn", r#"ghostty +new-tab --working-directory="$PWD""#),
];

#[derive(Clone, Copy, PartialEq)]
pub enum Hidden {
    Dim,
    Show,
    Hide,
}

pub struct Config {
    pub animations: bool,
    pub max_height: u16,
    pub hidden: Hidden,
    pub gap: Duration,
    pub skip: Vec<String>,
    pub openers: HashMap<String, PathBuf>,
}

fn doc() -> DocumentMut {
    let text = fs::read_to_string(toml_path()).unwrap_or_default();
    text.parse().unwrap_or_default()
}

/// Reads the settings, writing both files first if they aren't there yet.
pub fn load() -> (Config, Option<String>) {
    let mut err = None;
    if !toml_path().exists() {
        first_toml();
    }
    if !csv_path().exists() {
        first_csv();
    }
    let text = fs::read_to_string(toml_path()).unwrap_or_default();
    let d: DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(e) => {
            err = Some(format!("lealand.toml: {}", e.to_string().lines().next().unwrap_or("can't read it")));
            DocumentMut::new()
        }
    };
    let look = |k: &str| d.get("look").and_then(|t| t.get(k));
    let hidden = match look("hidden").and_then(|v| v.as_str()) {
        Some("show") => Hidden::Show,
        Some("hide") => Hidden::Hide,
        _ => Hidden::Dim,
    };
    let skip = d
        .get("jump")
        .and_then(|t| t.get("skip"))
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_else(|| SKIP.map(String::from).to_vec());
    let openers = d
        .get("openers")
        .and_then(|t| t.as_table())
        .map(|t| t.iter().filter_map(|(k, v)| Some((k.to_string(), PathBuf::from(v.as_str()?)))).collect())
        .unwrap_or_default();
    let config = Config {
        animations: look("animations").and_then(|v| v.as_bool()).unwrap_or(true),
        max_height: look("max_height").and_then(|v| v.as_integer()).unwrap_or(40).clamp(10, 100) as u16,
        hidden,
        gap: Duration::from_millis(d.get("chords").and_then(|t| t.get("gap_ms")).and_then(|v| v.as_integer()).unwrap_or(1000).clamp(150, 5000) as u64),
        skip,
        openers,
    };
    (config, err)
}

/// Writes lealand.toml, carrying over the old no-flag and apps files from before it existed.
fn first_toml() {
    let list = format!("[{}]", SKIP.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join(", "));
    let mut d: DocumentMut = DEFAULT_TOML.replace("SKIP_LIST", &list).parse().expect("the default settings parse");
    let old_flag = dir().join("no-flag");
    if old_flag.exists() {
        d["look"]["animations"] = value(false);
        let _ = fs::rename(&old_flag, dir().join("no-flag.old"));
    }
    let old_apps = dir().join("apps");
    if let Ok(text) = fs::read_to_string(&old_apps) {
        for line in text.lines() {
            if let Some((mime, path)) = line.split_once(' ') {
                d["openers"][mime] = value(path);
            }
        }
        let _ = fs::rename(&old_apps, dir().join("apps.old"));
    }
    let _ = fs::create_dir_all(dir());
    let _ = fs::write(toml_path(), d.to_string());
}

/// Writes lealand.csv: the default chords, with any from the old chords file on top.
fn first_csv() {
    let mut rows: Vec<(String, String, String)> = DEFAULT_CHORDS.iter().map(|(k, a, c)| (k.to_string(), a.to_string(), c.to_string())).collect();
    let old = dir().join("chords");
    if let Ok(text) = fs::read_to_string(&old) {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let Some(keys) = parts.next().filter(|k| !k.starts_with('#')) else { continue };
            let action = parts.next().unwrap_or("").to_string();
            let command = parts.collect::<Vec<_>>().join(" ");
            rows.retain(|r| r.0 != keys);
            if action != "none" {
                rows.push((keys.to_string(), action, command));
            }
        }
        let _ = fs::rename(&old, dir().join("chords.old"));
    }
    let mut out = String::from("keys,action,command\n");
    for (k, a, c) in rows {
        out += &format!("{},{},{}\n", quote(&k), quote(&a), quote(&c));
    }
    let _ = fs::create_dir_all(dir());
    let _ = fs::write(csv_path(), out);
}

fn quote(s: &str) -> String {
    if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// The rows of lealand.csv after the header: keys, action, command.
pub fn chord_rows() -> Vec<(String, String, String)> {
    let text = fs::read_to_string(csv_path()).unwrap_or_default();
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let f = fields(l);
            let get = |i: usize| f.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
            (get(0), get(1), get(2))
        })
        .filter(|r| !r.0.is_empty())
        .collect()
}

/// One CSV line into fields, with "quoted, fields" and "" for a quote inside them.
fn fields(line: &str) -> Vec<String> {
    let (mut v, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => v.push(std::mem::take(&mut cur)),
            c => cur.push(c),
        }
    }
    v.push(cur);
    v
}

pub fn set_animations(on: bool) {
    let mut d = doc();
    d["look"]["animations"] = value(on);
    let _ = fs::write(toml_path(), d.to_string());
}

pub fn set_opener(mime: &str, app: &std::path::Path) {
    let mut d = doc();
    d["openers"][mime] = value(app.to_string_lossy().as_ref());
    let _ = fs::write(toml_path(), d.to_string());
}

pub fn forget_opener(mime: &str) {
    let mut d = doc();
    if let Some(t) = d.get_mut("openers").and_then(|t| t.as_table_mut()) {
        t.remove(mime);
    }
    let _ = fs::write(toml_path(), d.to_string());
}

#[cfg(test)]
mod tests {
    #[test]
    fn csv() {
        assert_eq!(super::fields(r#"tt,spawn,"ghostty --working-directory=""$PWD""""#)[2], r#"ghostty --working-directory="$PWD""#);
        assert_eq!(super::fields("a,b,"), vec!["a", "b", ""]);
    }

    #[test]
    fn default_toml_parses() {
        let list = format!("[{}]", super::SKIP.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>().join(", "));
        let d: toml_edit::DocumentMut = super::DEFAULT_TOML.replace("SKIP_LIST", &list).parse().unwrap();
        assert_eq!(d["look"]["max_height"].as_integer(), Some(40));
    }
}
