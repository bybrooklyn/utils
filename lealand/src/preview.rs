//! The fullscreen preview: code coloured by bat, and pictures drawn with the kitty graphics
//! protocol, which Ghostty speaks.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::{
    env, fs,
    path::Path,
    process::{Command, Stdio},
};

/// A file's first `n` lines, coloured by bat when it's there.
pub fn text(path: &Path, n: usize, width: u16) -> Option<Vec<Line<'static>>> {
    let out = Command::new("bat")
        .args(["--color=always", "--style=numbers", "--paging=never", "--wrap=never"])
        .arg(format!("--line-range=:{n}"))
        .arg(format!("--terminal-width={width}"))
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(ansi(&String::from_utf8_lossy(&out.stdout)))
}

/// A code block coloured by bat for its language; None when bat doesn't know it.
pub fn highlight(code: &str, lang: &str) -> Option<Vec<Line<'static>>> {
    if lang.is_empty() {
        return None;
    }
    let mut child = Command::new("bat")
        .args(["--color=always", "--style=plain", "--paging=never"])
        .arg(format!("--language={lang}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let code = code.to_string();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let _ = stdin.write_all(code.as_bytes());
    });
    let out = child.wait_with_output().ok()?;
    let _ = writer.join();
    out.status.success().then(|| ansi(&String::from_utf8_lossy(&out.stdout)))
}

/// Terminal colour codes to styled lines. Only colours and bold/italic/underline matter here.
pub fn ansi(text: &str) -> Vec<Line<'static>> {
    text.lines()
        .map(|l| {
            let (mut spans, mut style, mut rest) = (Vec::new(), Style::new(), l);
            while let Some(i) = rest.find('\x1b') {
                if i > 0 {
                    spans.push(Span::styled(rest[..i].replace('\t', "    "), style));
                }
                let after = &rest[i + 1..];
                match after.strip_prefix('[') {
                    Some(body) => {
                        let end = body.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(body.len());
                        if body[end..].starts_with('m') {
                            style = sgr(style, &body[..end]);
                        }
                        rest = body.get(end + 1..).unwrap_or("");
                    }
                    None => rest = after,
                }
            }
            if !rest.is_empty() {
                spans.push(Span::styled(rest.replace('\t', "    "), style));
            }
            Line::from(spans)
        })
        .collect()
}

fn sgr(mut st: Style, params: &str) -> Style {
    let v: Vec<u16> = params.split(';').map(|x| x.parse().unwrap_or(0)).collect();
    let at = |i: usize| v.get(i).copied().unwrap_or(0);
    let mut i = 0;
    while i < v.len() {
        match v[i] {
            0 => st = Style::new(),
            1 => st = st.add_modifier(Modifier::BOLD),
            3 => st = st.add_modifier(Modifier::ITALIC),
            4 => st = st.add_modifier(Modifier::UNDERLINED),
            22 => st = st.remove_modifier(Modifier::BOLD),
            23 => st = st.remove_modifier(Modifier::ITALIC),
            24 => st = st.remove_modifier(Modifier::UNDERLINED),
            c @ 30..=37 => st = st.fg(Color::Indexed((c - 30) as u8)),
            c @ 90..=97 => st = st.fg(Color::Indexed((c - 90 + 8) as u8)),
            39 => st = st.fg(Color::Reset),
            38 | 48 if at(i + 1) == 5 => {
                if v[i] == 38 {
                    st = st.fg(Color::Indexed(at(i + 2) as u8));
                }
                i += 2;
            }
            38 | 48 if at(i + 1) == 2 => {
                if v[i] == 38 {
                    st = st.fg(Color::Rgb(at(i + 2) as u8, at(i + 3) as u8, at(i + 4) as u8));
                }
                i += 4;
            }
            _ => {}
        }
        i += 1;
    }
    st
}

pub fn is_image(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "avif" | "heic" | "tiff" | "ico" | "svg")
}

/// Whether this terminal draws pictures (Ghostty, kitty, WezTerm).
pub fn can_draw_images() -> bool {
    env::var("TERM_PROGRAM").is_ok_and(|t| t == "ghostty" || t == "WezTerm")
        || env::var_os("KITTY_WINDOW_ID").is_some()
        || env::var("TERM").is_ok_and(|t| t.contains("kitty") || t.contains("ghostty"))
}

/// The picture as PNG with its size: read as is, or converted by ImageMagick.
pub fn png(path: &Path) -> Option<(Vec<u8>, u32, u32)> {
    if fs::metadata(path).ok()?.len() > 64 << 20 {
        return None;
    }
    let is_png = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"));
    let bytes = if is_png {
        fs::read(path).ok()?
    } else {
        let src = format!("{}[0]", path.display());
        let out = Command::new("magick")
            .args([src.as_str(), "-auto-orient", "-resize", "1600x1600>", "png:-"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status.success().then_some(out.stdout)?
    };
    if bytes.len() < 24 || &bytes[1..4] != b"PNG" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((bytes, w, h))
}

/// Escape codes that draw `png` at the cursor, `cols` by `rows` cells.
pub fn kitty(png: &[u8], cols: u16, rows: u16) -> String {
    let data = base64(png);
    let chunks: Vec<&str> = data.as_bytes().chunks(4096).map(|c| std::str::from_utf8(c).unwrap()).collect();
    let mut out = String::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        if i == 0 {
            out += &format!("\x1b_Gf=100,a=T,q=2,C=1,c={cols},r={rows},m={more};{chunk}\x1b\\");
        } else {
            out += &format!("\x1b_Gm={more};{chunk}\x1b\\");
        }
    }
    out
}

/// Removes every picture this program drew.
pub const CLEAR: &str = "\x1b_Ga=d,d=A,q=2\x1b\\";

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64() {
        assert_eq!(super::base64(b"lealand"), "bGVhbGFuZA==");
    }

    #[test]
    fn ansi() {
        let l = &super::ansi("\x1b[38;2;1;2;3mfn\x1b[0m main")[0];
        assert_eq!(l.spans.len(), 2);
        assert_eq!(l.spans[0].style.fg, Some(ratatui::style::Color::Rgb(1, 2, 3)));
    }
}
