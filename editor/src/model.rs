//! Document model. Offsets are UTF-8 byte offsets on char boundaries.

use std::sync::Arc;

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const STRIKE: u8 = 4;
pub const HIGHLIGHT: u8 = 8;
pub const CODE: u8 = 16;

#[derive(Clone, Default, PartialEq, Eq, Hash, Debug)]
pub struct Marks { pub bits: u8, pub link: Option<Arc<str>> }

impl Marks {
    pub fn has(&self, bit: u8) -> bool { self.bits & bit != 0 }
    pub fn with(&self, bit: u8, on: bool) -> Marks {
        let mut m = self.clone();
        if on { m.bits |= bit; } else { m.bits &= !bit; }
        // Code stands alone: it never combines with the other styles.
        if on && bit == CODE { m.bits = CODE; }
        if on && bit != CODE { m.bits &= !CODE; }
        m
    }
    pub fn plain(&self) -> bool { self.bits == 0 && self.link.is_none() }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Run { pub text: String, pub marks: Marks }

impl Run {
    pub fn new(text: impl Into<String>, marks: Marks) -> Run { Run { text: text.into(), marks } }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Kind { Para, Heading(u8), Quote, Bullet, Number, Todo(bool), Code(String), Image(String) }

/// A picture is `path` or `path<0x01>width` (the width the user dragged it to).
pub fn image_parts(s: &str) -> (&str, Option<f32>) {
    match s.split_once('\x01') { Some((p, w)) => (p, w.parse().ok()), None => (s, None) }
}

impl Kind {
    pub fn is_image(&self) -> bool { matches!(self, Kind::Image(_)) }
    pub fn is_code(&self) -> bool { matches!(self, Kind::Code(_)) }
    pub fn is_list(&self) -> bool { matches!(self, Kind::Bullet | Kind::Number | Kind::Todo(_)) }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos { pub block: usize, pub off: usize }

impl Pos {
    pub fn new(block: usize, off: usize) -> Pos { Pos { block, off } }
}

/// The deepest nesting a list item can have (0 is the top level).
pub const MAX_INDENT: u8 = 3;

/// `indent` is how deep a list item sits; it means nothing for other kinds.
/// Read it through `levels`, which keeps an item at most one level below the one before it.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Block { pub kind: Kind, pub runs: Vec<Run>, pub indent: u8 }

/// The nesting level of every block: 0 for anything but a list item, and a list item
/// is at most one level deeper than the list item right before it.
pub fn levels(blocks: &[Block]) -> Vec<u8> {
    let mut prev: Option<u8> = None;
    blocks.iter().map(|b| {
        if !b.kind.is_list() { prev = None; return 0; }
        let l = b.indent.min(prev.map_or(0, |p| p + 1)).min(MAX_INDENT);
        prev = Some(l);
        l
    }).collect()
}

/// The number shown beside each numbered item (0 for the rest). Every level counts on its own.
pub fn numbering(blocks: &[Block]) -> Vec<usize> {
    let mut n = [0usize; MAX_INDENT as usize + 1];
    levels(blocks).into_iter().zip(blocks).map(|(l, b)| {
        let l = l as usize;
        if !b.kind.is_list() { n = [0; MAX_INDENT as usize + 1]; return 0; }
        for k in l + 1..n.len() { n[k] = 0; }
        if b.kind == Kind::Number { n[l] += 1; n[l] } else { n[l] = 0; 0 }
    }).collect()
}

/// The part of `runs` between byte offsets `a..b`.
pub fn cut(runs: &[Run], a: usize, b: usize) -> Vec<Run> {
    let mut out = Vec::new();
    let mut at = 0;
    for r in runs {
        let end = at + r.text.len();
        let (s, e) = (a.max(at), b.min(end));
        if s < e { out.push(Run { text: r.text[s - at..e - at].to_string(), marks: r.marks.clone() }); }
        at = end;
    }
    out
}

pub fn normalize(runs: &mut Vec<Run>) {
    runs.retain(|r| !r.text.is_empty());
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for r in runs.drain(..) {
        match out.last_mut() {
            Some(l) if l.marks == r.marks => l.text.push_str(&r.text),
            _ => out.push(r),
        }
    }
    *runs = out;
}

impl Block {
    pub fn new(kind: Kind) -> Block { Block { kind, runs: Vec::new(), indent: 0 } }
    pub fn para(text: &str) -> Block {
        let mut b = Block::new(Kind::Para);
        if !text.is_empty() { b.runs.push(Run::new(text, Marks::default())); }
        b
    }
    pub fn len(&self) -> usize { self.runs.iter().map(|r| r.text.len()).sum() }
    pub fn is_empty(&self) -> bool { self.runs.is_empty() }
    pub fn text(&self) -> String { self.runs.iter().map(|r| r.text.as_str()).collect() }
    pub fn slice(&self, a: usize, b: usize) -> Vec<Run> { cut(&self.runs, a, b) }

    /// Replace `a..b` with `ins`, keeping runs normalized.
    pub fn replace(&mut self, a: usize, b: usize, ins: Vec<Run>) {
        let len = self.len();
        let mut runs = cut(&self.runs, 0, a);
        runs.extend(ins);
        runs.extend(cut(&self.runs, b.min(len), len));
        normalize(&mut runs);
        self.runs = runs;
    }

    /// Marks of the char before `off` (or after it at the block start).
    pub fn marks_near(&self, off: usize) -> Marks {
        let before = if off > 0 { self.marks_of(off - 1) } else { None };
        let after = self.marks_of(off);
        match (before, after) {
            (Some(b), Some(a)) => {
                let link = if b.link == a.link { b.link.clone() } else { None };
                Marks { bits: b.bits, link }
            }
            (Some(b), None) => Marks { bits: b.bits, link: None },
            (None, Some(a)) => Marks { bits: a.bits, link: None },
            (None, None) => Marks::default(),
        }
    }
    /// Marks of the byte at `i`.
    pub fn marks_of(&self, i: usize) -> Option<&Marks> {
        let mut at = 0;
        for r in &self.runs {
            let end = at + r.text.len();
            if i >= at && i < end { return Some(&r.marks); }
            at = end;
        }
        None
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Doc { pub blocks: Vec<Block> }

impl Default for Doc {
    fn default() -> Doc { Doc { blocks: vec![Block::new(Kind::Para)] } }
}

impl Doc {
    /// Structural invariants: a title paragraph first, no text in images,
    /// plain code, normalized runs.
    pub fn check(&self) -> Result<(), String> {
        let first = self.blocks.first().ok_or("empty document")?;
        if first.kind != Kind::Para { return Err(format!("block 0 is {:?}", first.kind)); }
        let lv = levels(&self.blocks);
        for (i, b) in self.blocks.iter().enumerate() {
            if b.kind.is_image() && !b.runs.is_empty() { return Err(format!("image {i} has text")); }
            if b.kind.is_code() && b.runs.iter().any(|r| !r.marks.plain()) { return Err(format!("code {i} has marks")); }
            if b.runs.iter().any(|r| r.text.is_empty()) { return Err(format!("empty run in {i}")); }
            if b.runs.windows(2).any(|w| w[0].marks == w[1].marks) { return Err(format!("unmerged runs in {i}")); }
            if b.indent != lv[i] { return Err(format!("block {i} is indented {} but sits at level {}", b.indent, lv[i])); }
        }
        Ok(())
    }
    /// Restore the invariants after structural surgery.
    pub fn fix(&mut self) {
        if self.blocks.first().is_none_or(|b| b.kind != Kind::Para) {
            self.blocks.insert(0, Block::new(Kind::Para));
        }
        let lv = levels(&self.blocks);
        for (b, l) in self.blocks.iter_mut().zip(lv) {
            b.indent = l;
            if b.kind.is_image() { b.runs.clear(); }
            if b.kind.is_code() { for r in &mut b.runs { r.marks = Marks::default(); } }
            normalize(&mut b.runs);
        }
    }
    pub fn valid(&self, p: Pos) -> bool {
        let Some(b) = self.blocks.get(p.block) else { return false };
        if b.kind.is_image() { return p.off == 0; }
        p.off <= b.len() && b.text().is_char_boundary(p.off)
    }
    pub fn end(&self) -> Pos {
        let i = self.blocks.len() - 1;
        Pos::new(i, self.blocks[i].len())
    }
}
