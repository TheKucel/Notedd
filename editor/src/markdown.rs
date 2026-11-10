//! Markdown files <-> document model. One block per line; fenced code runs
//! between ``` lines. Reuses the inline scanner of `notedd-core`.

use crate::model::*;
use notedd_core::md::{self, Style};
use notedd_core::store::{self, Kind as ListKind};
use std::sync::Arc;

fn hidden(line: &[u16], s: &md::Span) -> bool {
    let at = |i: usize, c: u8| line.get(i) == Some(&(c as u16));
    s.style == Style::Mark || (s.style == Style::Url && s.a >= 3 && at(s.a - 2, b']') && at(s.a - 1, b'(') && !at(s.a - 3, b'['))
}

/// Characters a backslash escapes. Inline markup and the first character of block markers.
const ESCAPABLE: &str = "*_~=`[]#>-+.)!";
const PUA: u16 = 0xE000;
const BACKSLASH: char = 0x5C as char;

/// Replace `\*`-style escapes by private-use stand-ins (code spans are left alone).
fn unescape(w: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        let c = w[i];
        if c == b'`' as u16 {
            if let Some(j) = (i + 2..w.len()).find(|&j| w[j] == c) { out.extend_from_slice(&w[i..=j]); i = j + 1; continue; }
        }
        if c == BACKSLASH as u16 && i + 1 < w.len() {
            if let Some(k) = ESCAPABLE.bytes().position(|e| e as u16 == w[i + 1]) { out.push(PUA + k as u16); i += 2; continue; }
        }
        out.push(c);
        i += 1;
    }
    out
}
fn restore(c: u16) -> u16 {
    if (PUA..PUA + ESCAPABLE.len() as u16).contains(&c) { ESCAPABLE.as_bytes()[(c - PUA) as usize] as u16 } else { c }
}

/// Inline Markdown to styled runs; delimiters are dropped.
pub fn parse_inline(text: &str) -> Vec<Run> {
    let w = unescape(&text.encode_utf16().collect::<Vec<u16>>());
    let (spans, links) = md::inline(&w);
    let mut bits = vec![0u8; w.len()];
    let mut link: Vec<Option<Arc<str>>> = vec![None; w.len()];
    let mut shown = vec![true; w.len()];
    for s in &spans {
        if hidden(&w, s) { shown[s.a..s.b].fill(false); continue; }
        let b = match s.style {
            Style::Bold => BOLD, Style::Italic => ITALIC, Style::Strike => STRIKE,
            Style::Highlight => HIGHLIGHT, Style::Code => CODE, _ => 0,
        };
        for k in s.a..s.b { bits[k] |= b; }
    }
    for l in &links {
        let u: Arc<str> = l.url.as_str().into();
        for k in l.a..l.b { link[k] = Some(u.clone()); }
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut cur: Vec<u16> = Vec::new();
    let mut key: Option<Marks> = None;
    for i in 0..w.len() {
        if !shown[i] { continue; }
        let b = if bits[i] & CODE != 0 { CODE } else { bits[i] };
        let m = Marks { bits: b, link: link[i].clone() };
        if key.as_ref() != Some(&m) {
            if let Some(k) = key.take() { runs.push(Run::new(String::from_utf16_lossy(&cur), k)); cur.clear(); }
            key = Some(m);
        }
        cur.push(if b & CODE != 0 { w[i] } else { restore(w[i]) });
    }
    if let Some(k) = key { runs.push(Run::new(String::from_utf16_lossy(&cur), k)); }
    normalize(&mut runs);
    runs
}

/// A closing fence: at least `n` backticks and nothing else.
fn closes_fence(line: &str, n: usize) -> bool {
    let t = line.trim();
    t.len() >= n && t.bytes().all(|c| c == b'`')
}

pub fn parse_blocks(src: &str) -> Vec<Block> {
    let lines: Vec<&str> = src.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut out: Vec<Block> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start_matches(' ');
        if t.starts_with("```") {
            let n = t.bytes().take_while(|&c| c == b'`').count();
            let info = &t[n..];
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !closes_fence(lines[i], n) { body.push(lines[i]); i += 1; }
            i += 1;
            let mut b = Block::new(Kind::Code(info.trim().to_string()));
            let text = body.join("\n");
            if !text.is_empty() { b.runs.push(Run::new(text, Marks::default())); }
            out.push(b);
            continue;
        }
        i += 1;
        let n = t.bytes().take_while(|&c| c == b'#').count();
        let block = if (1..=6).contains(&n) && t[n..].starts_with(' ') {
            Block { kind: Kind::Heading(n.min(3) as u8), runs: parse_inline(&t[n + 1..]) }
        } else if let Some(r) = t.strip_prefix('>') {
            Block { kind: Kind::Quote, runs: parse_inline(r.strip_prefix(' ').unwrap_or(r)) }
        } else if let Some(path) = md::image_path(&line.encode_utf16().collect::<Vec<_>>()) {
            let w = line.find("notedd-width=").and_then(|i| { let d: String = line[i + 13..].chars().take_while(char::is_ascii_digit).collect(); d.parse::<u32>().ok() });
            Block::new(Kind::Image(match w { Some(w) => format!("{path}\x01{w}"), None => path }))
        } else {
            let (k, rest) = store::marker(line);
            match k {
                ListKind::Bullet => match store::task(k, rest) {
                    Some((done, r)) => Block { kind: Kind::Todo(done), runs: parse_inline(r) },
                    None => Block { kind: Kind::Bullet, runs: parse_inline(rest) },
                },
                ListKind::Number => Block { kind: Kind::Number, runs: parse_inline(rest) },
                _ => Block { kind: Kind::Para, runs: parse_inline(line) },
            }
        };
        out.push(block);
    }
    if out.is_empty() { out.push(Block::new(Kind::Para)); }
    out
}

pub fn parse(src: &str) -> Doc {
    let mut blocks = parse_blocks(src);
    // The first line is the title: always a plain paragraph.
    match blocks[0].kind {
        Kind::Para => {}
        Kind::Image(_) | Kind::Code(_) => blocks.insert(0, Block::new(Kind::Para)),
        _ => {
            let first = src.lines().next().unwrap_or("");
            blocks[0] = Block { kind: Kind::Para, runs: parse_inline(first) };
        }
    }
    let mut d = Doc { blocks };
    d.fix();
    d
}

const ORDER: [u8; 5] = [BOLD, ITALIC, STRIKE, HIGHLIGHT, CODE];

fn marker(bit: u8, bold: &'static str) -> &'static str {
    match bit { BOLD => bold, ITALIC => "*", STRIKE => "~~", HIGHLIGHT => "==", _ => "`" }
}

/// Runs of one link (or none) to Markdown, styles nested as a stack.
fn write_styles(runs: &[Run], escape: bool, bold: &'static str) -> String {
    // Whitespace at the edges of a styled piece goes outside its delimiters.
    let mut pieces: Vec<(String, u8)> = Vec::new();
    let mut push = |t: &str, b: u8| {
        if t.is_empty() { return; }
        match pieces.last_mut() { Some(l) if l.1 == b => l.0.push_str(t), _ => pieces.push((t.to_string(), b)) }
    };
    for r in runs {
        let core = r.text.trim();
        if r.marks.bits == 0 || core.is_empty() { push(&r.text, 0); continue; }
        let lead = r.text.len() - r.text.trim_start().len();
        push(&r.text[..lead], 0);
        push(core, r.marks.bits);
        push(&r.text[lead + core.len()..], 0);
    }
    let mut out = String::new();
    let mut stack: Vec<u8> = Vec::new();
    for (text, bits) in pieces {
        let want: Vec<u8> = ORDER.iter().copied().filter(|b| bits & b != 0).collect();
        let keep = stack.iter().zip(&want).take_while(|(a, b)| a == b).count();
        for b in stack.drain(keep..).rev() { out.push_str(marker(b, bold)); }
        for &b in &want[keep..] { out.push_str(marker(b, bold)); stack.push(b); }
        if escape && bits & CODE == 0 { for ch in text.chars() { if "*_~=`[]".contains(ch) { out.push(BACKSLASH); } out.push(ch); } } else { out.push_str(&text); }
    }
    for b in stack.drain(..).rev() { out.push_str(marker(b, bold)); }
    out
}

fn write_raw(runs: &[Run], escape: bool, bold: &'static str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < runs.len() {
        let link = runs[i].marks.link.clone();
        let mut j = i;
        while j < runs.len() && runs[j].marks.link == link { j += 1; }
        let seg = &runs[i..j];
        match link {
            None => out.push_str(&write_styles(seg, escape, bold)),
            Some(url) => {
                let text: String = seg.iter().map(|r| r.text.as_str()).collect();
                if text == *url && seg.iter().all(|r| r.marks.bits == 0) { out.push_str(&url); }
                else { out.push('['); out.push_str(&write_styles(seg, escape, bold)); out.push_str("]("); out.push_str(&url); out.push(')'); }
            }
        }
        i = j;
    }
    out
}

/// What the reader sees: each char with its marks (whitespace has none, it may move outside delimiters).
fn flat(runs: &[Run]) -> Vec<(char, Marks)> {
    runs.iter().flat_map(|r| r.text.chars().map(move |c| (c, if c.is_whitespace() { Marks::default() } else { r.marks.clone() }))).collect()
}

/// Inline Markdown that reads back to the same runs: literal markup characters are
/// escaped only when the plain form would be taken for markup.
pub fn write_inline(runs: &[Run]) -> String {
    let want = flat(runs);
    for (escape, bold) in [(false, "**"), (false, "__"), (true, "**"), (true, "__")] {
        let s = write_raw(runs, escape, bold);
        if flat(&parse_inline(&s)) == want { return s; }
    }
    write_raw(runs, true, "__")
}

/// Whether a paragraph line would be read as another kind of block.
fn block_like(line: &str) -> Option<usize> {
    let t = line.trim_start_matches(' ');
    let lead = line.len() - t.len();
    let hashes = t.bytes().take_while(|&c| c == b'#').count();
    if t.starts_with("```") || t.starts_with('>') || ((1..=6).contains(&hashes) && t[hashes..].starts_with(' ')) { return Some(lead); }
    if md::image_path(&line.encode_utf16().collect::<Vec<_>>()).is_some() { return Some(lead); }
    let d = line.bytes().take_while(u8::is_ascii_digit).count();
    match store::marker(line).0 {
        ListKind::Bullet => Some(0),
        ListKind::Number => Some(d),
        _ => None,
    }
}

pub fn write_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    let mut n = 0;
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 { out.push('\n'); }
        if matches!(b.kind, Kind::Number) { n += 1; } else { n = 0; }
        match &b.kind {
            Kind::Para => {
                let line = write_inline(&b.runs);
                match block_like(&line) {
                    Some(at) => { out.push_str(&line[..at]); out.push(BACKSLASH); out.push_str(&line[at..]); }
                    None => out.push_str(&line),
                }
            }
            Kind::Heading(l) => { out.push_str(&"#".repeat(*l as usize)); out.push(' '); out.push_str(&write_inline(&b.runs)); }
            Kind::Quote => { out.push_str("> "); out.push_str(&write_inline(&b.runs)); }
            Kind::Bullet => {
                let line = write_inline(&b.runs);
                out.push_str("- ");
                // Text that looks like a task box must not turn the bullet into a task.
                if store::task(ListKind::Bullet, &line).is_some() { out.push(BACKSLASH); }
                out.push_str(&line);
            }
            Kind::Number => { out.push_str(&format!("{n}. ")); out.push_str(&write_inline(&b.runs)); }
            Kind::Todo(d) => { out.push_str(if *d { "- [x] " } else { "- [ ] " }); out.push_str(&write_inline(&b.runs)); }
            Kind::Code(l) => {
                let text = b.text();
                // The fence is longer than any run of backticks that starts a line of the code.
                let longest = text.lines().map(|l| l.trim_start().bytes().take_while(|&c| c == b'`').count()).max().unwrap_or(0);
                let fence = "`".repeat(longest.max(2) + 1);
                out.push_str(&fence);
                out.push_str(l);
                out.push('\n');
                out.push_str(&text);
                out.push('\n');
                out.push_str(&fence);
            }
            Kind::Image(p) => {
                let (path, w) = crate::model::image_parts(p);
                out.push_str("![](");
                out.push_str(path);
                if let Some(w) = w { out.push_str(&format!(" \"notedd-width={}\"", w.round() as i32)); }
                out.push(')');
            }
        }
    }
    out
}

pub fn write(doc: &Doc) -> String { write_blocks(&doc.blocks) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_and_inline() {
        let d = parse("Title\n## Sub\n> q\n- a\n1. b\n- [x] c\n```rs\nlet x;\n```\n![](a.png)\nplain **b** and [l](http://x.io)");
        let k: Vec<_> = d.blocks.iter().map(|b| b.kind.clone()).collect();
        assert_eq!(k, vec![Kind::Para, Kind::Heading(2), Kind::Quote, Kind::Bullet, Kind::Number, Kind::Todo(true),
            Kind::Code("rs".into()), Kind::Image("a.png".into()), Kind::Para]);
        let last = &d.blocks[8];
        assert_eq!(last.text(), "plain b and l");
        assert!(last.runs.iter().any(|r| r.text == "b" && r.marks.has(BOLD)));
        assert!(last.runs.iter().any(|r| r.text == "l" && r.marks.link.as_deref() == Some("http://x.io")));
    }

    #[test]
    fn roundtrip_is_stable() {
        for src in ["a", "", "T\n\nx\n\n", "T\n- a\n- b\n1. c\n2. d", "T\n**a** *b* ~~c~~ ==d== `e`", "T\nsee [x](http://a.b) and http://c.d",
            "T\n***bi*** and **b *bi* b**", "T\n```\n\n```", "T\n> q **b**\n# not title"] {
            let once = write(&parse(src));
            assert_eq!(write(&parse(&once)), once, "unstable for {src:?}");
        }
        assert_eq!(write(&parse("T\n- a\n- [ ] t\n2. x")), "T\n- a\n- [ ] t\n1. x");
    }

    #[test]
    fn title_is_always_a_paragraph() {
        assert_eq!(parse("## x").blocks[0].kind, Kind::Para);
        assert_eq!(parse("![](a.png)").blocks.len(), 2);
        assert!(parse("![](a.png)").check().is_ok());
    }
}

