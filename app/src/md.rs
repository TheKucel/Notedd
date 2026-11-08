//! Markdown recognised in the editor: a block kind per paragraph and inline spans.
//! Offsets are UTF-16 units, the same as RichEdit character positions.

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
    let mut p = s[k + 2..s.len() - 1].trim();
    if let Some(sp) = p.find(' ') { p = &p[..sp]; }
    if p.starts_with('<') && p.ends_with('>') && p.len() >= 2 { p = &p[1..p.len() - 1]; }
    if p.is_empty() { None } else { Some(p.to_string()) }
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style { Mark, Bold, Italic, Strike, Code, Link, Url }
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
        // ~~strike~~
        if c == b'~' as u16 && at(i + 1) == c {
            if let Some(j) = (i + 3..to.saturating_sub(1)).find(|&j| s[j] == c && s[j + 1] == c) {
                push(sp, i, i + 2, Style::Mark); push(sp, i + 2, j, Style::Strike); push(sp, j, j + 2, Style::Mark);
                scan(s, i + 2, j, sp, ln);
                i = j + 2; continue;
            }
        }
        if c == b'*' as u16 || c == b'_' as u16 {
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
                    let url = String::from_utf16_lossy(&s[k + 2..m]).trim().to_string();
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

/// plain text of a line, for the previews in the drawer
pub fn strip(line: &str) -> String {
    let w: Vec<u16> = line.encode_utf16().collect();
    if image_path(&w).is_some() || line.trim_start().starts_with("```") { return String::new(); }
    let b = blocks(&[&w]).remove(0);
    let w = &w[mark_len(&w, &b)..];
    let (spans, _) = inline(w);
    let mut keep = vec![true; w.len()];
    for s in &spans {
        let link_url = s.style == Style::Url && s.a >= 2 && w[s.a - 1] == b'(' as u16 && w[s.a - 2] == b']' as u16;
        if s.style == Style::Mark || link_url { for k in s.a..s.b { keep[k] = false; } }
    }
    let out: Vec<u16> = w.iter().zip(keep).filter(|(_, k)| *k).map(|(c, _)| *c).collect();
    String::from_utf16_lossy(&out)
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
}
