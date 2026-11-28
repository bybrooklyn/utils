//! What this machine is, for `ll fetch` and the ? screen. Read from /proc and /sys, plus lspci.

use std::{collections::HashSet, env, ffi::CString, fs, process::Command};

pub fn all() -> Vec<(&'static str, String)> {
    let mut v = vec![
        ("os", os()),
        ("kernel", fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim().to_string()),
        ("uptime", uptime()),
        ("desktop", desktop()),
        ("shell", env::var("SHELL").unwrap_or_default().rsplit('/').next().unwrap_or("").to_string()),
        ("terminal", env::var("TERM_PROGRAM").unwrap_or_else(|_| env::var("TERM").unwrap_or_default())),
        ("cpu", cpu()),
    ];
    v.extend(gpus().into_iter().map(|g| ("gpu", g)));
    v.push(("memory", memory()));
    v.extend(monitors().into_iter().map(|m| ("monitor", m)));
    v.extend(disks().into_iter().map(|d| ("storage", d)));
    v.retain(|(_, s)| !s.is_empty());
    v
}

fn os() -> String {
    let text = fs::read_to_string("/etc/os-release").unwrap_or_default();
    text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).unwrap_or("Linux").trim_matches('"').to_string()
}

fn uptime() -> String {
    let secs = fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split('.').next()?.parse::<u64>().ok()).unwrap_or(0);
    let (d, h, m) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} d {h} h"),
    }
}

fn desktop() -> String {
    let de = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let de = if de == "KDE" { "KDE Plasma".into() } else { de };
    match (de.is_empty(), session.is_empty()) {
        (true, _) => session,
        (_, true) => de,
        _ => format!("{de} on {}", if session == "wayland" { "Wayland" } else { &session }),
    }
}

fn cpu() -> String {
    let text = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let name = text.lines().find_map(|l| l.strip_prefix("model name")?.split_once(':').map(|x| x.1.trim().to_string())).unwrap_or_default();
    let name = name.replace(" Processor", "").replace("(R)", "").replace("(TM)", "");
    let name = name.split(" with ").next().unwrap_or("").trim().to_string();
    let threads = text.lines().filter(|l| l.starts_with("processor")).count();
    format!("{name}, {threads} threads")
}

fn gpus() -> Vec<String> {
    let out = Command::new("lspci").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    out.lines()
        .filter(|l| l.contains("VGA") || l.contains("3D controller") || l.contains("Display controller"))
        .map(|l| {
            let desc = l.splitn(3, ':').nth(2).unwrap_or(l).trim();
            let vendor = if desc.contains("NVIDIA") {
                "NVIDIA"
            } else if desc.contains("AMD") || desc.contains("ATI") {
                "AMD"
            } else if desc.contains("Intel") {
                "Intel"
            } else {
                ""
            };
            // "GA104 [GeForce RTX 3070 Ti]" -> the part in brackets
            let model = desc.rsplit_once('[').and_then(|(_, r)| r.split(']').next()).unwrap_or(desc);
            let model = model.split('/').next().unwrap_or(model).trim();
            let built_in = desc.contains("Radeon") && (model.ends_with('M') || desc.contains("Granite") || desc.contains("Raphael"));
            format!("{vendor} {model}{}", if built_in { " (built in)" } else { "" }).trim().to_string()
        })
        .collect()
}

fn memory() -> String {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let kb = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.split_whitespace().next()?.parse::<f64>().ok()).unwrap_or(0.0);
    let (total, avail) = (kb("MemTotal:"), kb("MemAvailable:"));
    format!("{:.1} of {:.0} GiB used", (total - avail) / 1048576.0, total / 1048576.0)
}

/// Connected displays: the name in their EDID and their preferred resolution.
fn monitors() -> Vec<String> {
    let mut v: Vec<(String, String)> = fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let port = e.file_name().to_string_lossy().split_once('-')?.1.to_string();
            if fs::read_to_string(p.join("status")).ok()?.trim() != "connected" {
                return None;
            }
            let mode = fs::read_to_string(p.join("modes")).ok()?.lines().next()?.replace('x', " × ");
            let name = fs::read(p.join("edid")).ok().and_then(|e| edid_name(&e)).unwrap_or_else(|| port.clone());
            Some((port, format!("{name}, {mode}")))
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, m)| m).collect()
}

fn edid_name(e: &[u8]) -> Option<String> {
    for off in [54, 72, 90, 108] {
        let d = e.get(off..off + 18)?;
        if d[0..3] == [0, 0, 0] && d[3] == 0xfc {
            let name: String = d[5..].iter().take_while(|&&b| b != 0x0a).map(|&b| b as char).collect();
            return Some(name.trim().to_string()).filter(|n| !n.is_empty());
        }
    }
    None
}

/// Each real disk once, by the shortest place it's mounted.
fn disks() -> Vec<String> {
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut seen = HashSet::new();
    let mut list: Vec<(&str, &str)> = mounts
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some((f.next()?, f.next()?))
        })
        .filter(|(dev, at)| dev.starts_with("/dev/") && !at.starts_with("/boot") && !at.starts_with("/efi"))
        .collect();
    list.sort_by_key(|(_, at)| at.len());
    list.into_iter()
        .filter(|(dev, _)| seen.insert(*dev))
        .filter_map(|(_, at)| {
            let c = CString::new(at).ok()?;
            let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 || st.f_blocks == 0 {
                return None;
            }
            let total = st.f_blocks as f64 * st.f_frsize as f64;
            let used = total - st.f_bavail as f64 * st.f_frsize as f64;
            let name = if at.starts_with("/run/media/") || at.starts_with("/media/") { at.rsplit('/').next().unwrap_or(at) } else { at };
            Some(format!("{name}  {} of {} used", gb(used), gb(total)))
        })
        .collect()
}

fn gb(b: f64) -> String {
    let g = b / 1073741824.0;
    if g >= 1000.0 { format!("{:.1} TiB", g / 1024.0) } else { format!("{g:.0} GiB") }
}
