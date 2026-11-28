//! What this machine is, for `ll fetch` and the ? screen. Read from /proc and /sys, plus
//! kscreen-doctor on KDE for refresh rates and scaling.

use crate::pal;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::{collections::HashMap, collections::HashSet, env, ffi::CString, fs, path::Path, process::Command};

pub struct Item {
    pub group: &'static str,
    pub icon: char,
    pub label: &'static str,
    pub value: String,
    /// how full, 0 to 1, for memory and disks
    pub bar: Option<f64>,
}

fn item(group: &'static str, icon: char, label: &'static str, value: String) -> Item {
    Item { group, icon, label, value, bar: None }
}

pub fn all() -> Vec<Item> {
    let mut v = vec![
        item("system", '\u{f303}', "os", os()),
        item("system", '\u{f2db}', "board", board()),
        item("system", '\u{f17c}', "kernel", read("/proc/sys/kernel/osrelease")),
        item("system", '\u{f017}', "uptime", uptime()),
        item("system", '\u{f187}', "packages", packages()),
        item("system", '\u{f108}', "desktop", desktop()),
        item("system", '\u{f489}', "shell", env::var("SHELL").unwrap_or_default().rsplit('/').next().unwrap_or("").to_string()),
        item("system", '\u{f120}', "terminal", env::var("TERM_PROGRAM").unwrap_or_else(|_| env::var("TERM").unwrap_or_default())),
        item("system", '\u{f1ab}', "language", env::var("LANG").unwrap_or_default()),
        item("hardware", '\u{f2db}', "cpu", cpu()),
    ];
    v.extend(gpus().into_iter().map(|g| item("hardware", '\u{f26c}', "gpu", g)));
    v.extend(memory());
    v.extend(monitors().into_iter().map(|m| item("display", '\u{f108}', "monitor", m)));
    v.extend(disks());
    v.extend(addresses().into_iter().map(|a| item("network", '\u{f0ac}', "local ip", a)));
    v.retain(|i| !i.value.is_empty());
    v
}

fn read(p: &str) -> String {
    fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

fn os() -> String {
    let text = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let name = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).unwrap_or("Linux").trim_matches('"').to_string();
    format!("{name} {}", env::consts::ARCH)
}

fn board() -> String {
    let vendor = read("/sys/devices/virtual/dmi/id/board_vendor").replace("Micro-Star International Co., Ltd.", "MSI");
    let vendor = vendor.split(" Co").next().unwrap_or(&vendor).to_string();
    let name = read("/sys/devices/virtual/dmi/id/board_name");
    let name = name.split(" (").next().unwrap_or(&name).to_string();
    format!("{vendor} {name}").trim().to_string()
}

fn uptime() -> String {
    let secs = read("/proc/uptime").split('.').next().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let (d, h, m) = (secs / 86400, secs / 3600 % 24, secs / 60 % 60);
    match (d, h) {
        (0, 0) => format!("{m} min"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} d {h} h {m} min"),
    }
}

fn packages() -> String {
    let count = |p: &str| fs::read_dir(p).map(|d| d.flatten().filter(|e| e.path().is_dir()).count()).unwrap_or(0);
    let pacman = count("/var/lib/pacman/local");
    let flatpak = count("/var/lib/flatpak/app") + count(&format!("{}/.local/share/flatpak/app", env::var("HOME").unwrap_or_default()));
    let mut parts = Vec::new();
    if pacman > 0 {
        parts.push(format!("{pacman} pacman"));
    }
    if flatpak > 0 {
        parts.push(format!("{flatpak} flatpak"));
    }
    parts.join(", ")
}

fn desktop() -> String {
    let de = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let plasma = Command::new("plasmashell").arg("--version").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().replace("plasmashell ", ""));
    let de = match (de.as_str(), plasma) {
        ("KDE", Some(v)) if !v.is_empty() => format!("KDE Plasma {v}"),
        ("KDE", _) => "KDE Plasma".into(),
        _ => de,
    };
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
    let ghz = read("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq").parse::<f64>().map(|k| format!(" up to {:.1} GHz", k / 1e6)).unwrap_or_default();
    let temp = hwmon_temp(&["k10temp", "coretemp", "zenpower"]).map(|t| format!(", {t:.0} °C")).unwrap_or_default();
    format!("{name}, {threads} threads{ghz}{temp}")
}

fn hwmon_temp(names: &[&str]) -> Option<f64> {
    fs::read_dir("/sys/class/hwmon").ok()?.flatten().find_map(|e| {
        let p = e.path();
        let name = fs::read_to_string(p.join("name")).ok()?;
        names.contains(&name.trim()).then(|| fs::read_to_string(p.join("temp1_input")).ok()?.trim().parse::<f64>().ok().map(|m| m / 1000.0))?
    })
}

/// Display controllers on the PCI bus, named from the hwdata database (what lspci reads, without
/// the 180 ms it takes to start), with their driver.
fn gpus() -> Vec<String> {
    let ids = fs::read_to_string("/usr/share/hwdata/pci.ids").unwrap_or_default();
    let get = |p: &Path, f: &str| fs::read_to_string(p.join(f)).unwrap_or_default().trim().trim_start_matches("0x").to_string();
    let mut found: Vec<_> = fs::read_dir("/sys/bus/pci/devices").into_iter().flatten().flatten().map(|e| e.path()).filter(|p| get(p, "class").starts_with("03")).collect();
    found.sort();
    let mut names: Vec<String> = found
        .iter()
        .map(|p| {
            let (vendor, model) = pci_name(&ids, &get(p, "vendor"), &get(p, "device"));
            let vendor = ["NVIDIA", "AMD", "Intel"].into_iter().find(|v| vendor.contains(v) || (*v == "AMD" && vendor.contains("ATI"))).unwrap_or(vendor.as_str()).to_string();
            let short = model.rsplit_once('[').and_then(|(_, r)| r.split(']').next()).unwrap_or(&model);
            let short = short.split('/').next().unwrap_or(short).trim();
            let built_in = vendor == "AMD" && (short.ends_with('M') || model.contains("Granite") || model.contains("Raphael"));
            let driver = fs::read_link(p.join("driver")).ok().and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
            let version = if driver == "nvidia" { read("/sys/module/nvidia/version") } else { String::new() };
            let vram = get(p, "mem_info_vram_total").parse::<f64>().ok().map(|b| format!(", {:.0} GiB", b / 1073741824.0).replace(", 1 GiB", ", 1 GiB").replace(", 0 GiB", ", 512 MiB")).unwrap_or_default();
            let mut s = format!("{vendor} {short}");
            if built_in {
                s += " (built in)";
            }
            if !driver.is_empty() {
                s += &format!(", {driver} {version}");
            }
            s.trim().to_string() + &vram
        })
        .collect();
    names.sort_by_key(|g| g.contains("built in"));
    names
}

fn pci_name(ids: &str, vendor: &str, device: &str) -> (String, String) {
    let mut lines = ids.lines().skip_while(|l| !l.starts_with(vendor));
    let vname = lines.next().map(|l| l[vendor.len()..].trim().to_string()).unwrap_or_default();
    let tab = format!("\t{device}");
    let dname = lines
        .take_while(|l| l.starts_with('\t') || l.starts_with('#') || l.is_empty())
        .find(|l| l.starts_with(&tab))
        .map(|l| l[tab.len()..].trim().to_string())
        .unwrap_or_else(|| format!("device {device}"));
    (vname, dname)
}

fn memory() -> Vec<Item> {
    let text = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let kb = |key: &str| text.lines().find_map(|l| l.strip_prefix(key)?.split_whitespace().next()?.parse::<f64>().ok()).unwrap_or(0.0);
    let (total, avail) = (kb("MemTotal:"), kb("MemAvailable:"));
    let mut v = vec![Item {
        group: "hardware",
        icon: '\u{f2db}',
        label: "memory",
        value: format!("{:.1} of {:.0} GiB", (total - avail) / 1048576.0, total / 1048576.0),
        bar: Some((total - avail) / total.max(1.0)),
    }];
    let (st, sf) = (kb("SwapTotal:"), kb("SwapFree:"));
    if st > 0.0 {
        let zram = fs::read_to_string("/proc/swaps").is_ok_and(|s| s.contains("zram"));
        v.push(Item {
            group: "hardware",
            icon: '\u{f0c5}',
            label: "swap",
            value: format!("{:.1} of {:.0} GiB{}", (st - sf) / 1048576.0, st / 1048576.0, if zram { " (zram)" } else { "" }),
            bar: Some((st - sf) / st),
        });
    }
    v
}

/// Connected displays: the name in their EDID, and from KDE the mode, scale, HDR and which is first.
fn monitors() -> Vec<String> {
    let kde = kscreen();
    let mut v: Vec<(u32, String)> = fs::read_dir("/sys/class/drm")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let port = e.file_name().to_string_lossy().split_once('-')?.1.to_string();
            if fs::read_to_string(p.join("status")).ok()?.trim() != "connected" {
                return None;
            }
            let name = fs::read(p.join("edid")).ok().and_then(|e| edid_name(&e)).unwrap_or_else(|| port.clone());
            let Some(k) = kde.get(&port) else {
                let mode = fs::read_to_string(p.join("modes")).ok()?.lines().next()?.replace('x', " × ");
                return Some((99, format!("{name}, {mode}")));
            };
            let mut s = format!("{name}, {}", k.mode);
            if k.scale != "1" {
                s += &format!(", scale {}", k.scale);
            }
            if k.hdr {
                s += ", HDR";
            }
            if k.vrr {
                s += ", VRR";
            }
            if k.priority == 1 {
                s += ", main";
            }
            Some((k.priority, s))
        })
        .collect();
    v.sort();
    v.into_iter().map(|(_, m)| m).collect()
}

struct Screen {
    mode: String,
    scale: String,
    hdr: bool,
    vrr: bool,
    priority: u32,
}

fn kscreen() -> HashMap<String, Screen> {
    let Ok(out) = Command::new("kscreen-doctor").arg("-o").output() else { return HashMap::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut map = HashMap::new();
    let mut cur: Option<(String, Screen)> = None;
    for raw in text.lines() {
        let line = strip_ansi(raw);
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("Output:") {
            if let Some((k, v)) = cur.take() {
                map.insert(k, v);
            }
            let port = rest.split_whitespace().nth(1).unwrap_or("").to_string();
            cur = Some((port, Screen { mode: String::new(), scale: "1".into(), hdr: false, vrr: false, priority: 99 }));
        } else if let Some((_, s)) = &mut cur {
            if let Some(m) = l.strip_prefix("Modes:") {
                if let Some(now) = m.split_whitespace().find(|t| t.contains('*')) {
                    let mode = now.split(':').nth(1).unwrap_or("").trim_end_matches(['*', '!']);
                    if let Some((res, hz)) = mode.split_once('@') {
                        let hz = hz.trim_end_matches(['*', '!']).parse::<f64>().map(|h| format!("{h:.0} Hz")).unwrap_or_default();
                        s.mode = format!("{} @ {hz}", res.replace('x', " × "));
                    }
                }
            } else if let Some(x) = l.strip_prefix("Scale:") {
                s.scale = x.trim().to_string();
            } else if let Some(x) = l.strip_prefix("priority ") {
                s.priority = x.trim().parse().unwrap_or(99);
            } else if l.starts_with("HDR:") {
                s.hdr = l.contains("enabled");
            } else if l.starts_with("Vrr:") {
                s.vrr = l.contains("Always") || l.contains("Automatic");
            }
        }
    }
    if let Some((k, v)) = cur {
        map.insert(k, v);
    }
    map
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut esc = false;
    for c in s.chars() {
        match c {
            '\x1b' => esc = true,
            c if esc && c.is_ascii_alphabetic() => esc = false,
            _ if esc => {}
            c => out.push(c),
        }
    }
    out
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
fn disks() -> Vec<Item> {
    let mounts = fs::read_to_string("/proc/mounts").unwrap_or_default();
    let mut seen = HashSet::new();
    let mut list: Vec<(&str, &str, &str)> = mounts
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            Some((f.next()?, f.next()?, f.next()?))
        })
        .filter(|(dev, at, _)| dev.starts_with("/dev/") && !at.starts_with("/boot") && !at.starts_with("/efi"))
        .collect();
    list.sort_by_key(|(_, at, _)| at.len());
    list.into_iter()
        .filter(|(dev, ..)| seen.insert(*dev))
        .filter_map(|(_, at, fs_type)| {
            let c = CString::new(at).ok()?;
            let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
            if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 || st.f_blocks == 0 {
                return None;
            }
            let total = st.f_blocks as f64 * st.f_frsize as f64;
            let used = total - st.f_bavail as f64 * st.f_frsize as f64;
            let name = if at.starts_with("/run/media/") || at.starts_with("/media/") { at.rsplit('/').next().unwrap_or(at) } else { at };
            let fs_type = if fs_type == "fuseblk" { "ntfs" } else { fs_type };
            Some(Item {
                group: "storage",
                icon: '\u{f0a0}',
                label: "disk",
                value: format!("{name}  {} of {}, {fs_type}", gb(used), gb(total)),
                bar: Some(used / total),
            })
        })
        .collect()
}

fn gb(b: f64) -> String {
    let g = b / 1073741824.0;
    if g >= 1000.0 { format!("{:.1} TiB", g / 1024.0) } else { format!("{g:.0} GiB") }
}

/// IPv4 addresses on the network cards that are up, apart from loopback and virtual ones.
fn addresses() -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut ifap: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifap) != 0 {
            return out;
        }
        let mut cur = ifap;
        while !cur.is_null() {
            let ifa = &*cur;
            cur = ifa.ifa_next;
            if ifa.ifa_addr.is_null() || (*ifa.ifa_addr).sa_family as i32 != libc::AF_INET {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().into_owned();
            if ["lo", "tun", "wg", "docker", "br-", "veth", "virbr"].iter().any(|p| name.starts_with(p)) {
                continue;
            }
            let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
            let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            out.push(format!("{ip} ({name})"));
        }
        libc::freeifaddrs(ifap);
    }
    out
}

/// The items as styled lines, grouped under small headings, with bars for how full things are.
/// `head` puts user@host and a rule on top.
pub fn lines(items: &[Item], head: bool) -> Vec<Line<'static>> {
    let label_w = items.iter().map(|i| i.label.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    if head {
        let user = env::var("USER").unwrap_or_default();
        let host = read("/proc/sys/kernel/hostname");
        out.push(Line::from(vec![
            Span::styled(user.clone(), Style::new().fg(pal::TEXT).add_modifier(Modifier::BOLD)),
            Span::styled("@", Style::new().fg(pal::FAINT)),
            Span::styled(host.clone(), Style::new().fg(pal::DIR).add_modifier(Modifier::BOLD)),
        ]));
        out.push(Line::from(Span::styled("─".repeat(user.len() + host.len() + 1), Style::new().fg(pal::FAINT))));
    }
    let mut group = "";
    let mut last_label = "";
    for i in items {
        if i.group != group {
            if !out.is_empty() {
                out.push(Line::default());
            }
            out.push(Line::from(Span::styled(i.group, Style::new().fg(group_color(i.group)).add_modifier(Modifier::ITALIC))));
            group = i.group;
            last_label = "";
        }
        let (icon, label) = if i.label == last_label { (' ', "") } else { (i.icon, i.label) };
        last_label = i.label;
        let mut spans = vec![
            Span::styled(format!(" {icon}  "), Style::new().fg(group_color(i.group))),
            Span::styled(format!("{label:<label_w$}  "), Style::new().fg(pal::DIM)),
        ];
        if let Some(b) = i.bar {
            spans.extend(bar(b));
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(i.value.clone(), Style::new().fg(pal::TEXT)));
        out.push(Line::from(spans));
    }
    out
}

fn group_color(g: &str) -> Color {
    match g {
        "system" => pal::DIR,
        "hardware" => pal::AMBER,
        "display" => pal::LINK,
        "storage" => pal::LILAC,
        _ => pal::ROSE,
    }
}

/// A 12-cell bar, green when there's room, amber past 70%, red past 90%.
fn bar(f: f64) -> Vec<Span<'static>> {
    let f = f.clamp(0.0, 1.0);
    let full = (f * 12.0).round() as usize;
    let color = if f > 0.9 { pal::RED } else if f > 0.7 { pal::AMBER } else { pal::DIR };
    vec![
        Span::styled("━".repeat(full), Style::new().fg(color)),
        Span::styled("━".repeat(12 - full), Style::new().fg(Color::Rgb(0x33, 0x3b, 0x34))),
        Span::styled(format!(" {:>3.0}%", f * 100.0), Style::new().fg(color)),
    ]
}

/// Styled lines as terminal colour codes, for printing.
pub fn ansi(lines: &[Line]) -> String {
    let code = |c: Color| match c {
        Color::Rgb(r, g, b) => format!("38;2;{r};{g};{b}"),
        _ => "39".into(),
    };
    let mut out = String::new();
    for l in lines {
        out += "  ";
        for s in &l.spans {
            let mut sgr = vec![s.style.fg.map_or("39".into(), code)];
            if s.style.add_modifier.contains(Modifier::BOLD) {
                sgr.push("1".into());
            }
            if s.style.add_modifier.contains(Modifier::ITALIC) {
                sgr.push("3".into());
            }
            out += &format!("\x1b[{}m{}\x1b[0m", sgr.join(";"), s.content);
        }
        out += "\n";
    }
    out
}
