//! Markdown for the preview pane, laid out the way GitHub shows it: headings with a rule under
//! the big ones, code in a tinted panel coloured by bat, lists, task lists, quotes, alerts and
//! tables, all wrapped to the pane.

use crate::{pal, preview};
use pulldown_cmark::{BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

const CODE_BG: Color = Color::Rgb(0x1f, 0x26, 0x21);
const INLINE_BG: Color = Color::Rgb(0x2a, 0x33, 0x2c);

pub fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_FOOTNOTES;
    let mut r = R { width: width.max(20), ..R::default() };
    for ev in Parser::new_ext(text, opts) {
        r.event(ev);
    }
    r.flush();
    while r.out.last().is_some_and(|l| l.width() == 0) {
        r.out.pop();
    }
    r.out
}

#[derive(Default)]
struct R {
    out: Vec<Line<'static>>,
    width: usize,
    inline: Vec<Span<'static>>,
    styles: Vec<Style>,
    quotes: Vec<Color>,
    lists: Vec<Option<u64>>,
    bullet: Option<String>,
    heading: Option<HeadingLevel>,
    code: Option<(String, String)>, // language, text
    table: Option<Vec<Vec<Vec<Span<'static>>>>>,
    in_meta: bool,
    link: bool,
}

impl R {
    fn style(&self) -> Style {
        self.styles.iter().fold(Style::new().fg(pal::TEXT), |a, s| a.patch(*s))
    }

    fn blank(&mut self) {
        if self.out.last().is_some_and(|l| l.width() > 0) {
            self.out.push(Line::default());
        }
    }

    /// What goes before every line here: a bar per quote, two spaces per list level.
    fn prefix(&mut self, first: bool) -> Vec<Span<'static>> {
        let mut p: Vec<Span<'static>> = self.quotes.iter().map(|c| Span::styled("▎ ", Style::new().fg(*c))).collect();
        let depth = self.lists.len();
        if depth > 0 {
            p.push(Span::raw("  ".repeat(depth - 1)));
            match (first, self.bullet.take()) {
                (true, Some(b)) => {
                    let w = b.width();
                    p.push(Span::styled(format!("{b}{}", " ".repeat(3usize.saturating_sub(w))), Style::new().fg(pal::DIR)));
                }
                _ => p.push(Span::raw("   ")),
            }
        }
        p
    }

    /// Wraps what's been gathered for this paragraph, heading or item into lines.
    fn flush(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.inline);
        let first_prefix = self.prefix(true);
        let rest_prefix = self.prefix(false);
        let avail = self.width.saturating_sub(first_prefix.iter().map(|s| s.width()).sum::<usize>()).max(10);
        let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
        let mut used = 0;
        for span in spans {
            for piece in split_keep_spaces(&span.content) {
                let w = piece.width();
                if piece == "\n" {
                    lines.push(Vec::new());
                    used = 0;
                    continue;
                }
                if used + w > avail && used > 0 {
                    if piece.trim().is_empty() {
                        continue;
                    }
                    lines.push(Vec::new());
                    used = 0;
                }
                if used == 0 && piece.trim().is_empty() {
                    continue;
                }
                lines.last_mut().unwrap().push(Span::styled(piece.to_string(), span.style));
                used += w;
            }
        }
        for (i, l) in lines.into_iter().enumerate() {
            let mut all = if i == 0 { first_prefix.clone() } else { rest_prefix.clone() };
            all.extend(l);
            self.out.push(Line::from(all));
        }
    }

    fn text(&mut self, s: &str) {
        if self.in_meta {
            return;
        }
        if let Some((_, code)) = &mut self.code {
            code.push_str(s);
            return;
        }
        let st = self.style();
        if let Some(t) = &mut self.table {
            if let Some(cell) = t.last_mut().and_then(|r| r.last_mut()) {
                cell.push(Span::styled(s.to_string(), st));
            }
            return;
        }
        self.inline.push(Span::styled(s.to_string(), st));
    }

    fn event(&mut self, ev: Event) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => {
                let st = self.style().fg(pal::AMBER).bg(INLINE_BG);
                let span = Span::styled(format!(" {t} "), st);
                match &mut self.table {
                    Some(tb) => {
                        if let Some(cell) = tb.last_mut().and_then(|r| r.last_mut()) {
                            cell.push(span);
                        }
                    }
                    None => self.inline.push(span),
                }
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.inline.push(Span::raw("\n")),
            Event::Rule => {
                self.flush();
                self.blank();
                self.out.push(Line::from(Span::styled("─".repeat(self.width), Style::new().fg(pal::FAINT))));
                self.blank();
            }
            Event::TaskListMarker(done) => {
                self.bullet = Some(if done { "✔".into() } else { "☐".into() });
            }
            Event::FootnoteReference(n) => self.inline.push(Span::styled(format!("[{n}]"), Style::new().fg(pal::LINK))),
            Event::Html(h) | Event::InlineHtml(h) => {
                // keep the words of simple HTML, drop the tags
                let mut text = String::new();
                let mut in_tag = false;
                for c in h.chars() {
                    match c {
                        '<' => in_tag = true,
                        '>' => in_tag = false,
                        c if !in_tag => text.push(c),
                        _ => {}
                    }
                }
                let text = text.trim();
                if !text.is_empty() {
                    self.text(text);
                }
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush();
                self.blank();
                self.heading = Some(level);
                let st = match level {
                    HeadingLevel::H1 => Style::new().fg(pal::TEXT).add_modifier(Modifier::BOLD),
                    HeadingLevel::H2 => Style::new().fg(pal::TEXT).add_modifier(Modifier::BOLD),
                    HeadingLevel::H3 => Style::new().fg(pal::DIR).add_modifier(Modifier::BOLD),
                    _ => Style::new().fg(pal::DIM).add_modifier(Modifier::BOLD),
                };
                self.styles.push(st);
            }
            Tag::BlockQuote(kind) => {
                self.flush();
                self.blank();
                let (color, title) = match kind {
                    Some(BlockQuoteKind::Note) => (pal::LINK, Some("\u{f05a}  Note")),
                    Some(BlockQuoteKind::Tip) => (pal::DIR, Some("\u{f0eb}  Tip")),
                    Some(BlockQuoteKind::Important) => (pal::LILAC, Some("\u{f06a}  Important")),
                    Some(BlockQuoteKind::Warning) => (pal::AMBER, Some("\u{f071}  Warning")),
                    Some(BlockQuoteKind::Caution) => (pal::RED, Some("\u{f05e}  Caution")),
                    None => (pal::FAINT, None),
                };
                self.quotes.push(color);
                if let Some(t) = title {
                    let mut p = self.prefix(false);
                    p.push(Span::styled(t, Style::new().fg(color).add_modifier(Modifier::BOLD)));
                    self.out.push(Line::from(p));
                } else {
                    self.styles.push(Style::new().fg(pal::DIM));
                }
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.blank();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.split([',', ' ']).next().unwrap_or("").to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                if self.lists.is_empty() {
                    self.blank();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let b = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let s = format!("{n}.");
                        *n += 1;
                        s
                    }
                    _ => ["•", "◦", "▪"][(self.lists.len().saturating_sub(1)) % 3].to_string(),
                };
                self.bullet = Some(b);
            }
            Tag::Emphasis => self.styles.push(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.styles.push(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.styles.push(Style::new().add_modifier(Modifier::CROSSED_OUT).fg(pal::DIM)),
            Tag::Link { .. } => {
                self.link = true;
                self.styles.push(Style::new().fg(pal::LINK).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { .. } => {
                self.inline.push(Span::styled("\u{f03e} ", Style::new().fg(pal::LILAC)));
                self.styles.push(Style::new().fg(pal::LILAC).add_modifier(Modifier::ITALIC));
            }
            Tag::Table(_) => {
                self.flush();
                self.blank();
                self.table = Some(Vec::new());
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = &mut self.table {
                    t.push(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(r) = self.table.as_mut().and_then(|t| t.last_mut()) {
                    r.push(Vec::new());
                }
            }
            Tag::MetadataBlock(_) => self.in_meta = true,
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::Heading(level) => {
                self.flush();
                self.styles.pop();
                self.heading = None;
                if matches!(level, HeadingLevel::H1 | HeadingLevel::H2) {
                    let ch = if level == HeadingLevel::H1 { "━" } else { "─" };
                    self.out.push(Line::from(Span::styled(ch.repeat(self.width), Style::new().fg(pal::FAINT))));
                }
                self.blank();
            }
            TagEnd::BlockQuote(kind) => {
                self.flush();
                self.quotes.pop();
                if kind.is_none() {
                    self.styles.pop();
                }
                self.blank();
            }
            TagEnd::CodeBlock => {
                if let Some((lang, code)) = self.code.take() {
                    self.code_block(&lang, &code);
                }
                self.blank();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::Item => self.flush(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Image => {
                self.styles.pop();
            }
            TagEnd::Link => {
                self.styles.pop();
                self.link = false;
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.table_lines(t);
                }
                self.blank();
            }
            TagEnd::MetadataBlock(_) => self.in_meta = false,
            _ => {}
        }
    }

    fn code_block(&mut self, lang: &str, code: &str) {
        let prefix = self.prefix(false);
        let inner = self.width.saturating_sub(prefix.iter().map(|s| s.width()).sum::<usize>());
        if !lang.is_empty() {
            let mut p = prefix.clone();
            let used: usize = p.iter().map(|s| s.width()).sum::<usize>() + lang.width() + 1;
            p.push(Span::styled(format!(" {lang}{}", " ".repeat(self.width.saturating_sub(used))), Style::new().fg(pal::FAINT).bg(CODE_BG)));
            self.out.push(Line::from(p));
        }
        let lines = preview::highlight(code, lang).unwrap_or_else(|| code.lines().map(|l| Line::from(l.to_string())).collect());
        for l in lines {
            let mut p = prefix.clone();
            p.push(Span::raw(" "));
            p.extend(l.spans.into_iter().map(|s| {
                let st = if s.style.fg.is_none() { s.style.fg(pal::TEXT) } else { s.style };
                Span::styled(s.content.into_owned(), st.bg(CODE_BG))
            }));
            let w: usize = p.iter().map(|s| s.width()).sum();
            if w < inner + p.len() {
                p.push(Span::styled(" ".repeat(self.width.saturating_sub(w)), Style::new().bg(CODE_BG)));
            }
            self.out.push(Line::from(p));
        }
    }

    fn table_lines(&mut self, rows: Vec<Vec<Vec<Span<'static>>>>) {
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let cell_w = |c: &Vec<Span>| c.iter().map(|s| s.width()).sum::<usize>();
        let mut widths: Vec<usize> = (0..cols).map(|i| rows.iter().filter_map(|r| r.get(i)).map(cell_w).max().unwrap_or(0)).collect();
        // squeeze the widest columns until the table fits
        let room = self.width.saturating_sub(cols * 3);
        while widths.iter().sum::<usize>() > room && widths.iter().any(|&w| w > 4) {
            let i = (0..cols).max_by_key(|&i| widths[i]).unwrap();
            widths[i] -= 1;
        }
        let bar = || Span::styled(" │ ", Style::new().fg(pal::FAINT));
        for (ri, row) in rows.iter().enumerate() {
            let mut spans = vec![Span::raw(" ")];
            for (ci, w) in widths.iter().enumerate() {
                if ci > 0 {
                    spans.push(bar());
                }
                let text: String = row.get(ci).map(|c| c.iter().map(|s| s.content.as_ref()).collect()).unwrap_or_default();
                let style = row.get(ci).and_then(|c| c.first()).map_or(Style::new().fg(pal::TEXT), |s| s.style);
                let style = if ri == 0 { style.add_modifier(Modifier::BOLD) } else { style };
                spans.push(Span::styled(fit(&text, *w), style));
            }
            self.out.push(Line::from(spans));
            if ri == 0 {
                let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
                self.out.push(Line::from(Span::styled(format!("─{}─", rule.join("─┼─")), Style::new().fg(pal::FAINT))));
            }
        }
    }
}

fn split_keep_spaces(s: &str) -> Vec<&str> {
    let mut v = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if c == ' ' || c == '\n' {
            if start < i {
                v.push(&s[start..i]);
            }
            v.push(&s[i..i + 1]);
            start = i + 1;
        }
    }
    if start < s.len() {
        v.push(&s[start..]);
    }
    v
}

fn fit(s: &str, w: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + cw > w {
            out.pop();
            out.push('…');
            used = w;
            break;
        }
        out.push(c);
        used += cw;
    }
    out + &" ".repeat(w.saturating_sub(used))
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders() {
        let md = "# Title\n\nSome **bold** and `code`.\n\n- [x] done\n- [ ] todo\n\n> [!NOTE]\n> careful\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let lines = super::render(md, 40);
        let text: Vec<String> = lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect()).collect();
        assert!(text[0].contains("Title"));
        assert!(text.iter().any(|t| t.contains("✔") && t.contains("done")));
        assert!(text.iter().any(|t| t.contains("Note")));
        assert!(text.iter().any(|t| t.contains("│")));
    }
}
