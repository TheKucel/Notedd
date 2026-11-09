//! Markdown recognised in the editor: a block kind per paragraph and inline spans.
//! Offsets are UTF-16 units, the same as RichEdit character positions.

use std::{collections::HashSet, ops::Range};

#[derive(Clone, PartialEq, Debug)]
pub enum Block { Plain, Heading(u8), Quote, Fence, Code, Image(String) }

const SP: u16 = b' ' as u16;

fn starts(s: &[u16], i: usize, p: &str) -> bool {
    let p = p.as_bytes();
    i + p.len() <= s.len() && p.iter().enumerate().all(|(k, &c)| s[i + k] == c as u16)
}
fn word(c: u16) -> bool { c >= 128 || (c < 128 && (c as u8).is_ascii_alphanumeric()) }
fn lead(s: &[u16]) -> usize { s.iter().take_while(|&&c| c == SP).count() }

/// one block per line; fenced code runs between ``` lines
pub fn blocks(lines: &[&[u16]]) -> Vec<Block> {
    let mut in_code = false;
    lines.iter().map(|line| {
        let t = &line[lead(line)..];
        if starts(t, 0, "```") { in_code = !in_code; return Block::Fence; }
        if in_code { return Block::Code; }
        let n = t.iter().take_while(|&&c| c == b'#' as u16).count();
        if (1..=6).contains(&n) && t.get(n) == Some(&SP) { return Block::Heading(n.min(3) as u8); }
        if t.first() == Some(&(b'>' as u16)) { return Block::Quote; }
        if let Some(p) = image_path(line) { return Block::Image(p); }
        Block::Plain
    }).collect()
}

/// `![alt](path "title")` filling the whole line
pub fn image_path(line: &[u16]) -> Option<String> {
    let s = String::from_utf16_lossy(line);
    let s = s.trim();
    if !(s.starts_with("![") && s.ends_with(')')) { return None; }
    let k = s.find("](")?;
    let destination = &s[k + 2..s.len() - 1];
    let mut p = link_target(destination);
    if p == destination.trim() && !p.starts_with('<') { p = p.split_whitespace().next().unwrap_or(""); }
    if p.is_empty() { None } else { Some(p.to_string()) }
}

fn link_target(target: &str) -> &str {
    let target = target.trim();
    if let Some(angled) = target.strip_prefix('<') {
        if let Some((path, _)) = angled.split_once('>') { return path; }
    }
    // A quoted title follows the path, separated by a space.
    if let Some((path, title)) = target.split_once(" \"") {
        if title.ends_with('"') { return path; }
    }
    target
}

/// the leading block marker ("## ", "> "), dimmed in the editor
pub fn mark_len(line: &[u16], b: &Block) -> usize {
    let l = lead(line);
    match b {
        Block::Heading(_) => { let n = line[l..].iter().take_while(|&&c| c == b'#' as u16).count(); (l + n + 1).min(line.len()) }
        Block::Quote => l + 1 + (line.get(l + 1) == Some(&SP)) as usize,
        Block::Fence => line.len(),
        _ => 0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Style { Mark, Bold, Italic, Strike, Highlight, Code, Link, Url }

/// Opening and closing ranges around visible inline content, innermost first.
pub fn delimiters(line: &[u16]) -> Vec<(Range<usize>, Range<usize>)> {
    let mut pairs: Vec<_> = inline(line).0.into_iter().filter_map(|s| {
        let width = match s.style {
            Style::Bold | Style::Strike | Style::Highlight => 2,
            Style::Italic | Style::Code => 1,
            Style::Link => {
                let start = if s.a >= 2 && starts(line, s.a - 2, "![") { s.a - 2 } else { s.a - 1 };
                let end = (s.b + 2..line.len()).find(|&i| line[i] == b')' as u16)?;
                return Some((start..s.a, s.b..end + 1));
            }
            _ => return None,
        };
        Some((s.a - width..s.a, s.b..s.b + width))
    }).collect();
    pairs.sort_unstable_by_key(|(open, close)| close.start - open.end);
    pairs
}

/// Keeps pairs intact when an edit crosses hidden inline delimiters.
pub fn preserve_delimiters(
    text: &[u16], range: Range<usize>, mut pairs: Vec<(Range<usize>, Range<usize>)>, erase: bool,
) -> Option<(Range<usize>, Vec<u16>, Vec<u16>)> {
    if pairs.is_empty() { return None; }
    let (mut a, mut b) = (range.start, range.end);
    let original = (a, b);
    for (open, close) in &pairs {
        if a <= open.end && b >= close.start {
            a = a.min(open.start);
            b = b.max(close.end);
        }
    }
    // Move a delimiter past whitespace exposed by an erase so its span stays valid.
    let (mut left_space, mut right_space) = (a..a, b..b);
    if erase {
        let blank = |c| c != 13 && c != 10 && space(c);
        let left = a - text[..a].iter().rev().take_while(|&&c| blank(c)).count();
        let right = b + text[b..].iter().take_while(|&&c| blank(c)).count();
        for (open, close) in &pairs {
            if !matches!(text[open.start], 42 | 95 | 61) { continue; }
            if left < a && left > open.end && b >= close.start {
                left_space = left..a;
                b = b.max(close.end);
            }
            if right > b && right < close.start && a <= open.end {
                right_space = b..right;
                a = a.min(open.start);
            }
        }
    }
    pairs.sort_unstable_by_key(|(l, _)| l.start);
    let mut openings = Vec::new();
    let mut closings = Vec::new();
    for (open, close) in pairs {
        if a <= open.end && b >= close.start { continue; }
        if a < open.end && b > open.start { openings.extend_from_slice(&text[a.max(open.start)..b.min(open.end)]); }
        if a < close.end && b > close.start { closings.push(a.max(close.start)..b.min(close.end)); }
    }
    let (start, end) = (a.min(left_space.start), b.max(right_space.end));
    if (start, end) == original && openings.is_empty() && closings.is_empty() { return None; }
    closings.sort_unstable_by_key(|r| r.start);
    let before = closings.into_iter().flat_map(|r| text[r].iter().copied())
        .chain(text[left_space].iter().copied()).collect();
    let after = text[right_space].iter().copied().chain(openings).collect();
    Some((start..end, before, after))
}

#[derive(Clone, Copy, PartialEq)]
pub enum Format { Bold, Italic, Strike, Highlight, Code }

impl Format {
    fn style(self) -> Style {
        match self {
            Self::Bold => Style::Bold, Self::Italic => Style::Italic, Self::Strike => Style::Strike,
            Self::Highlight => Style::Highlight, Self::Code => Style::Code,
        }
    }
    fn markers(self) -> &'static [&'static [u16]] {
        match self {
            Self::Bold => &[&[42, 42], &[95, 95]],
            Self::Italic => &[&[42], &[95]],
            Self::Strike => &[&[126, 126]],
            Self::Highlight => &[&[61, 61]],
            Self::Code => &[&[96]],
        }
    }
    pub fn width(self) -> usize { self.markers()[0].len() }
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Span { pub a: usize, pub b: usize, pub style: Style }
#[derive(Clone, PartialEq, Debug)]
pub struct Link { pub a: usize, pub b: usize, pub url: String }

pub fn inline(s: &[u16]) -> (Vec<Span>, Vec<Link>) {
    let mut spans = Vec::new();
    let mut links = Vec::new();
    scan(s, 0, s.len(), &mut spans, &mut links);
    (spans, links)
}

fn scan(s: &[u16], from: usize, to: usize, sp: &mut Vec<Span>, ln: &mut Vec<Link>) {
    let push = |sp: &mut Vec<Span>, a: usize, b: usize, style: Style| if a < b { sp.push(Span { a, b, style }) };
    let at = |i: usize| if i < to { s[i] } else { 0 };
    let mut i = from;
    while i < to {
        let c = s[i];
        // `code`
        if c == b'`' as u16 {
            if let Some(j) = (i + 2..to).find(|&j| s[j] == c) {
                push(sp, i, i + 1, Style::Mark); push(sp, i + 1, j, Style::Code); push(sp, j, j + 1, Style::Mark);
                i = j + 1; continue;
            }
            i += 1; continue;
        }
        // ==highlight==
        if c == b'=' as u16 && at(i + 1) == c && (i == from || s[i - 1] != c) && at(i + 2) != c {
            if i + 2 < to && !space(s[i + 2]) {
                if let Some(j) = (i + 3..to.saturating_sub(1)).find(|&j|
                    s[j] == c && s[j + 1] == c && s[j - 1] != c && !space(s[j - 1]) && at(j + 2) != c
                ) {
                    push(sp, i, i + 2, Style::Mark);
                    push(sp, i + 2, j, Style::Highlight);
                    push(sp, j, j + 2, Style::Mark);
                    scan(s, i + 2, j, sp, ln);
                    i = j + 2; continue;
                }
            }
        }
        // ~~strike~~
        if c == b'~' as u16 && at(i + 1) == c {
            if let Some(j) = (i + 3..to.saturating_sub(1)).find(|&j| s[j] == c && s[j + 1] == c) {
                push(sp, i, i + 2, Style::Mark); push(sp, i + 2, j, Style::Strike); push(sp, j, j + 2, Style::Mark);
                scan(s, i + 2, j, sp, ln);
                i = j + 2; continue;
            }
        }
        if c == b'*' as u16 || c == b'_' as u16 {
            if at(i + 1) == c && at(i + 2) == c && at(i + 3) != c && !space(at(i + 3)) {
                if let Some(j) = (i + 4..to.saturating_sub(2)).find(|&j| {
                    s[j] == c && s[j + 1] == c && s[j + 2] == c
                        && s[j - 1] != c && !space(s[j - 1]) && at(j + 3) != c
                }) {
                    push(sp, i, i + 2, Style::Mark);
                    push(sp, i + 2, j + 1, Style::Bold);
                    push(sp, i + 2, i + 3, Style::Mark);
                    push(sp, i + 3, j, Style::Italic);
                    scan(s, i + 3, j, sp, ln);
                    push(sp, j, j + 1, Style::Mark);
                    push(sp, j + 1, j + 3, Style::Mark);
                    i = j + 3;
                    continue;
                }
            }
            // **bold** / __bold__
            if at(i + 1) == c {
                if i + 2 < to && s[i + 2] != SP {
                    if let Some(j) = (i + 3..to.saturating_sub(1)).find(|&j| s[j] == c && s[j + 1] == c && s[j - 1] != SP) {
                        push(sp, i, i + 2, Style::Mark); push(sp, i + 2, j, Style::Bold); push(sp, j, j + 2, Style::Mark);
                        scan(s, i + 2, j, sp, ln);
                        i = j + 2; continue;
                    }
                }
                i += 2; continue;
            }
            // *italic* / _italic_
            let under = c == b'_' as u16;
            let open_ok = i + 1 < to && s[i + 1] != SP && !(under && i > 0 && word(s[i - 1]));
            if open_ok {
                let close = (i + 2..to).find(|&j| s[j] == c && s[j - 1] != SP && at(j + 1) != c && !(under && j + 1 < to && word(s[j + 1])));
                if let Some(j) = close {
                    push(sp, i, i + 1, Style::Mark); push(sp, i + 1, j, Style::Italic); push(sp, j, j + 1, Style::Mark);
                    scan(s, i + 1, j, sp, ln);
                    i = j + 1; continue;
                }
            }
            i += 1; continue;
        }
        // [text](url) and ![alt](url)
        if c == b'[' as u16 || (c == b'!' as u16 && at(i + 1) == b'[' as u16) {
            let open = if c == b'!' as u16 { 2 } else { 1 };
            let st = i + open;
            let mut k = st;
            while k + 1 < to && s[k] != b']' as u16 { k += 1; }
            if k + 1 < to && s[k] == b']' as u16 && s[k + 1] == b'(' as u16 {
                if let Some(m) = (k + 2..to).find(|&m| s[m] == b')' as u16) {
                    let target = String::from_utf16_lossy(&s[k + 2..m]);
                    let url = link_target(&target).to_owned();
                    push(sp, i, st, Style::Mark); push(sp, st, k, Style::Link); push(sp, k, k + 2, Style::Mark);
                    push(sp, k + 2, m, Style::Url); push(sp, m, m + 1, Style::Mark);
                    if st < k { ln.push(Link { a: st, b: k, url: url.clone() }); }
                    if k + 2 < m { ln.push(Link { a: k + 2, b: m, url }); }
                    scan(s, st, k, sp, ln);
                    i = m + 1; continue;
                }
            }
            i += 1; continue;
        }
        // bare http(s) links
        if (starts(s, i, "https://") || starts(s, i, "http://")) && (i == 0 || matches!(s[i - 1], 32 | 40 | 60)) {
            let mut j = i;
            while j < to && s[j] != SP && s[j] != b'>' as u16 { j += 1; }
            while j > i && ".,;:!?)'\"".bytes().any(|b| b as u16 == s[j - 1]) { j -= 1; }
            push(sp, i, j, Style::Url);
            ln.push(Link { a: i, b: j, url: String::from_utf16_lossy(&s[i..j]) });
            i = j.max(i + 1); continue;
        }
        i += 1;
    }
}

pub fn space(c: u16) -> bool { char::from_u32(c as u32).is_some_and(char::is_whitespace) }

pub(crate) fn hidden_mark(line: &[u16], span: &Span) -> bool {
    span.style == Style::Mark || (span.style == Style::Url && span.a >= 3
        && starts(line, span.a - 2, "](") && line[span.a - 3] != b'[' as u16)
}

fn visible_offsets(line: &[u16], spans: &[Span]) -> (Vec<u16>, Vec<usize>) {
    let mut shown = vec![true; line.len()];
    for s in spans.iter().filter(|s| hidden_mark(line, s)) { shown[s.a..s.b].fill(false); }
    let mut visible = Vec::new();
    let mut offsets: Vec<_> = line.iter().zip(shown).map(|(&ch, shown)| {
        let at = visible.len();
        if shown { visible.push(ch); }
        at
    }).collect();
    offsets.push(visible.len());
    (visible, offsets)
}

/// The range and delimiters to insert, or the whole existing span to unwrap.
pub fn format_edit(line: &[u16], selection: Range<usize>, format: Format, simple: bool) -> Option<(Range<usize>, Option<&'static [u16]>)> {
    let (spans, _) = inline(line);
    let (visible, offsets) = visible_offsets(line, &spans);
    let (mut start, mut end) = (offsets[selection.start], offsets[selection.end]);
    while start < end && space(visible[start]) { start += 1; }
    while start < end && space(visible[end - 1]) { end -= 1; }
    if start == end { return None; }
    let style = format.style();
    for s in &spans {
        let (a, b) = (offsets[s.a], offsets[s.b]);
        if start >= b || end <= a { continue; }
        if s.style == style {
            let width = format.width();
            return (start >= a && end <= b).then_some((s.a - width..s.b + width, None));
        }
        if s.style == Style::Code && (start > a || end < b || (!simple && (selection.start >= s.a || selection.end <= s.b))) {
            return None;
        }
    }

    // At a hidden delimiter the same visible position has several raw offsets.
    // Try inside existing styles first, then around them when the delimiters would collide.
    let mut starts = vec![offsets.partition_point(|&pos| pos <= start) - 1];
    let mut ends = vec![offsets.partition_point(|&pos| pos < end)];
    for s in spans.iter().filter(|s| hidden_mark(line, s)) {
        if offsets[s.a] == start { starts.push(s.a); }
        if offsets[s.b] == end { ends.push(s.b); }
    }
    starts.sort_unstable_by(|a, b| b.cmp(a));
    ends.sort_unstable();
    starts.dedup();
    ends.dedup();
    let existing: Vec<_> = spans.iter().filter(|s| !hidden_mark(line, s))
        .map(|s| (s.style, offsets[s.a], offsets[s.b])).collect();
    for a in starts {
        for &b in &ends {
            for &marker in format.markers() {
                let candidate = [&line[..a], marker, &line[a..b], marker, &line[b..]].concat();
                let new_spans = inline(&candidate).0;
                let (new_visible, new_offsets) = visible_offsets(&candidate, &new_spans);
                if new_visible != visible { continue; }
                let styles: HashSet<_> = new_spans.iter().map(|s| (s.style, new_offsets[s.a], new_offsets[s.b])).collect();
                if styles.contains(&(style, start, end)) && existing.iter().all(|s| styles.contains(s)) {
                    return Some((a..b, Some(marker)));
                }
            }
        }
    }
    None
}

/// Text and highlight ranges in the same UTF-16 coordinates, with the markers removed.
pub fn preview(line: &str) -> (Vec<u16>, Vec<Range<u32>>) {
    let w: Vec<u16> = line.encode_utf16().collect();
    if image_path(&w).is_some() || line.trim_start().starts_with("```") { return (Vec::new(), Vec::new()); }
    let b = blocks(&[&w]).remove(0);
    let w = &w[mark_len(&w, &b)..];
    let (spans, _) = inline(w);
    let mut keep = vec![true; w.len()];
    let mut highlights = Vec::new();
    for s in &spans {
        let link_url = s.style == Style::Url && s.a >= 2 && w[s.a - 1] == b'(' as u16 && w[s.a - 2] == b']' as u16;
        if s.style == Style::Mark || link_url { for k in s.a..s.b { keep[k] = false; } }
    }
    if spans.iter().any(|s| s.style == Style::Highlight) {
        let mut offset = 0;
        let mut positions: Vec<_> = keep.iter().map(|&k| { let start = offset; offset += u32::from(k); start }).collect();
        positions.push(offset);
        highlights.extend(spans.iter().filter(|s| s.style == Style::Highlight).map(|s| positions[s.a]..positions[s.b]));
    }
    let out = w.iter().zip(keep).filter(|(_, k)| *k).map(|(c, _)| *c).collect();
    (out, highlights)
}

pub fn strip(line: &str) -> String {
    String::from_utf16_lossy(&preview(line).0)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn u(s: &str) -> Vec<u16> { s.encode_utf16().collect() }

    #[test]
    fn block_kinds() {
        let src = ["Title", "## Sub", "> q", "```", "code", "```", "![](assets/a.png)", "x"];
        let v: Vec<Vec<u16>> = src.iter().map(|s| u(s)).collect();
        let r: Vec<&[u16]> = v.iter().map(|x| x.as_slice()).collect();
        assert_eq!(blocks(&r), vec![Block::Plain, Block::Heading(2), Block::Quote, Block::Fence, Block::Code, Block::Fence, Block::Image("assets/a.png".into()), Block::Plain]);
    }
    #[test]
    fn bold() {
        let (s, _) = inline(&u("a **b** c"));
        assert!(s.contains(&Span { a: 2, b: 4, style: Style::Mark }));
        assert!(s.contains(&Span { a: 4, b: 5, style: Style::Bold }));
        assert!(s.contains(&Span { a: 5, b: 7, style: Style::Mark }));
    }
    #[test]
    fn snake() { assert!(!inline(&u("snake_case_name")).0.iter().any(|s| s.style == Style::Italic)); }
    #[test]
    fn italic() { assert!(inline(&u("an *it* x")).0.contains(&Span { a: 4, b: 6, style: Style::Italic })); }
    #[test]
    fn links() {
        assert!(inline(&u("see [site](https://x.io) now")).1.iter().any(|l| l.url == "https://x.io"));
        assert!(inline(&u("go https://a.b/c. ok")).1.iter().any(|l| l.url == "https://a.b/c"));
    }
    #[test]
    fn stripped() { assert_eq!(strip("## **Hi** [a](http://b)"), "Hi a"); }

    #[test]
    fn highlight_preserves_nested_styles_and_preview_offsets() {
        let (text, ranges) = preview("📌 ==a **b**== `==code==`");
        assert_eq!(String::from_utf16_lossy(&text), "📌 a b ==code==");
        assert_eq!(ranges, vec![3..6]);
        assert!(inline(&u("x == y && y == z")).0.is_empty());
        assert!(inline(&u("===not a highlight===")).0.is_empty());
    }

    #[test]
    fn highlight_selection_wraps_text_and_unwraps_existing_markers() {
        let format = Format::Highlight;
        assert_eq!(format_edit(&u(" hello "), 0..7, format, false), Some((1..6, Some(&[61, 61][..]))));
        assert_eq!(format_edit(&u("==hello=="), 2..7, format, false), Some((0..9, None)));
        assert_eq!(format_edit(&u("==hello=="), 0..9, format, false), Some((0..9, None)));
        assert_eq!(format_edit(&u("`hello`"), 1..6, format, false), None);
    }

    #[test]
    fn format_selection_supports_nested_emphasis() {
        assert_eq!(format_edit(&u("**hello**"), 2..7, Format::Italic, true), Some((2..7, Some(&[42][..]))));
        assert_eq!(format_edit(&u("*hello*"), 1..6, Format::Bold, true), Some((1..6, Some(&[42, 42][..]))));
        assert_eq!(format_edit(&u("**_hello_**"), 3..8, Format::Bold, true), Some((0..11, None)));
        assert_eq!(strip("==***hello***=="), "hello");
    }

    #[test]
    fn simple_edits_keep_hidden_delimiters_paired() {
        let text = u("A **b** C");
        assert_eq!(preserve_delimiters(&text, 4..5, delimiters(&text), true),
                   Some((2..7, Vec::new(), Vec::new())));
        let text = u("A [abc](https://example.org) Z");
        let tail = text.len();
        assert_eq!(preserve_delimiters(&text, 4..tail, delimiters(&text), false),
                   Some((4..tail, u("](https://example.org)"), Vec::new())));
    }

    #[test]
    fn format_selection_combines_and_removes_styles_independently() {
        let apply = |text: &[u16], sel: Range<usize>, format: Format| {
            let (range, marker) = format_edit(text, sel, format, true).expect("format command applies");
            let replacement = match marker {
                Some(m) => [m, &text[range.clone()], m].concat(),
                None => text[range.start + format.width()..range.end - format.width()].to_vec(),
            };
            [&text[..range.start], &replacement, &text[range.end..]].concat()
        };
        let formats = [Format::Bold, Format::Italic, Format::Strike, Format::Highlight];
        for first in formats {
            for second in formats.into_iter().filter(|&f| f != first) {
                let mut text = u("слово");
                let mut active = vec![first, second];
                active.extend(formats.into_iter().filter(|&f| f != first && f != second));
                for &format in &active {
                    text = apply(&text, 0..text.len(), format);
                    assert_eq!(strip(&String::from_utf16_lossy(&text)), "слово");
                }
                for &format in active.iter().rev() {
                    text = apply(&text, 0..text.len(), format);
                    assert!(!inline(&text).0.iter().any(|s| s.style == format.style()));
                    assert_eq!(strip(&String::from_utf16_lossy(&text)), "слово");
                }
                assert_eq!(text, u("слово"));
            }
        }
        let text = u("*word*");
        assert_eq!(strip(&String::from_utf16_lossy(&apply(&text, 0..5, Format::Highlight))), "word");
        let text = u("==word==");
        assert_eq!(strip(&String::from_utf16_lossy(&apply(&text, 0..text.len(), Format::Code))), "word");
    }
}
