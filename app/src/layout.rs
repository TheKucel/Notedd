//! Shaping and geometry of the document. Text is shaped per block and cached by
//! content hash, so typing re-shapes only the paragraph being edited.

use crate::pictures::{Loaded, Pictures};
use gpui::*;
use notedd_editor::model::{Block, Doc, Kind, Pos, BOLD, CODE, HIGHLIGHT, ITALIC, STRIKE};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Hsla,
    pub ink: Hsla,
    pub dim: Hsla,
    pub accent: Hsla,
    pub selection: Hsla,
    pub highlight: Hsla,
    pub code_bg: Hsla,
    pub line: Hsla,
    pub ink3: Hsla,
    pub find: Hsla,
    pub dark: Hsla,
    pub on_dark: Hsla,
}

impl Theme {
    pub fn from_app(a: &crate::theme::AppTheme) -> Theme {
        use crate::theme::{c, mix};
        let sel = (a.ink << 8) | if a.night { 0x3d } else { 0x26 };
        Theme {
            bg: c(a.paper()), ink: c(a.ink), dim: c(a.ink2), accent: c(a.link), selection: rgba(sel).into(),
            highlight: c(a.highlight()), code_bg: c(mix(a.paper(), a.ink, 0.08)), line: c(a.line), ink3: c(a.ink3), find: rgba((a.ink << 8) | 0x21).into(), dark: c(a.tab_dark), on_dark: c(a.on_dark),
        }
    }
}

pub const BODY: &str = "Geologica";
pub const MONO: &str = "Cascadia Mono";

/// Sizes of a block kind, in logical pixels.
#[derive(Clone, Copy)]
pub struct Metrics {
    pub size: f32,
    pub lh: f32,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub indent: f32,
    pub before: f32,
    pub after: f32,
    pub pad: f32,
}

impl Metrics {
    pub fn scaled(mut self, k: f32) -> Metrics {
        self.size *= k; self.lh *= k; self.indent *= k; self.before *= k; self.after *= k; self.pad *= k;
        self
    }
}

/// How far each level of a nested list moves in, in unscaled pixels.
const LEVEL_STEP: f32 = 24.;

pub fn metrics(kind: &Kind, index: usize) -> Metrics {
    let m = |size, lh, before, after| Metrics { size, lh, bold: false, italic: false, mono: false, indent: 0., before, after, pad: 0. };
    // the old app: title 21/27, body 16/23, gaps 9 (3 between list items), code 14.5/21
    if index == 0 { return Metrics { bold: true, ..m(21., 27., 0., 12.) }; }
    match kind {
        Kind::Para => m(16., 23., 0., 9.),
        Kind::Heading(1) => Metrics { bold: true, ..m(21., 27., 9., 9.) },
        Kind::Heading(2) => Metrics { bold: true, ..m(18.5, 25., 9., 9.) },
        Kind::Heading(_) => Metrics { bold: true, ..m(16., 23., 9., 9.) },
        Kind::Quote => Metrics { italic: true, indent: 16., ..m(16., 23., 0., 9.) },
        Kind::Bullet | Kind::Number | Kind::Todo(_) => Metrics { indent: 25., ..m(16., 23., 0., 9.) },
        Kind::Code(_) => Metrics { mono: true, pad: 8., ..m(14.5, 21., 0., 9.) },
        Kind::Image(_) => m(16., 23., 0., 9.),
    }
}

pub fn font(family: &'static str, bold: bool, italic: bool) -> Font {
    Font {
        family: family.into(),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: if bold { FontWeight::BOLD } else { FontWeight::NORMAL },
        style: if italic { FontStyle::Italic } else { FontStyle::Normal },
    }
}

fn runs_for(b: &Block, m: &Metrics, th: &Theme) -> Vec<TextRun> {
    b.runs.iter().map(|r| {
        let bits = r.marks.bits;
        let mono = m.mono || bits & CODE != 0;
        let mut color = if m.bold || m.mono { th.ink } else { th.dim };
        if r.marks.link.is_some() { color = th.accent; }
        TextRun {
            len: r.text.len(),
            font: font(if mono { MONO } else { BODY }, m.bold || bits & BOLD != 0, m.italic || bits & ITALIC != 0),
            color,
            background_color: if bits & HIGHLIGHT != 0 { Some(th.highlight) } else if bits & CODE != 0 && !m.mono { Some(th.code_bg) } else { None },
            underline: r.marks.link.is_some().then(|| UnderlineStyle { thickness: px(1.), color: Some(th.accent), wavy: false }),
            strikethrough: (bits & STRIKE != 0).then(|| StrikethroughStyle { thickness: px(1.), color: Some(th.ink) }),
        }
    }).collect()
}

pub struct Line {
    /// Byte offset of the line in the block text.
    pub start: usize,
    /// Top of the line inside the block's text area.
    pub y: f32,
    pub shaped: WrappedLine,
}

pub struct BlockBox {
    pub y: f32,
    pub h: f32,
    /// Left edge of the text and its width.
    pub x: f32,
    pub w: f32,
    pub m: Metrics,
    pub lines: Vec<Line>,
    /// Top of the text inside the block (after padding).
    pub top: f32,
    /// A picture block: the decoded image and its displayed size.
    pub img: Option<(Arc<Loaded>, f32, f32)>,
}

#[derive(Default)]
pub struct Layout {
    pub blocks: Vec<BlockBox>,
    pub height: f32,
    pub width: f32,
}

#[derive(Default)]
pub struct ShapeCache {
    entries: Vec<Option<(u64, Vec<WrappedLine>)>>,
    pub shaped: usize,
}

fn key(b: &Block, index: usize, width: f32, salt: u64) -> u64 {
    let mut h = DefaultHasher::new();
    (b, index == 0, width.to_bits(), salt).hash(&mut h);
    h.finish()
}

/// Height of a picture whose file is missing.
pub const MISSING_H: f32 = 64.;
const MAX_PIC_H: f32 = 520.;

impl ShapeCache {
    pub fn layout(&mut self, doc: &Doc, width: f32, th: &Theme, salt: u64, scale: f32, single: bool, pics: &mut Pictures, window: &mut Window) -> Layout {
        let salt = salt ^ (scale.to_bits() as u64) << 32 ^ single as u64;
        self.entries.resize_with(doc.blocks.len(), || None);
        self.entries.truncate(doc.blocks.len());
        let mut out = Layout { blocks: Vec::with_capacity(doc.blocks.len()), height: 0., width };
        let mut y = 0.;
        let mut prev_after = 0.;
        let nesting = notedd_editor::model::levels(&doc.blocks);
        for (i, b) in doc.blocks.iter().enumerate() {
            let mut m = if single { metrics(&Kind::Para, 1) } else { metrics(&b.kind, i) };
            // each level of a nested list moves the item (and its marker) in by one step
            if !single && b.kind.is_list() { m.indent += nesting[i] as f32 * LEVEL_STEP; }
            let m = m.scaled(scale);
            let text_w = (width - m.indent - 2. * m.pad).max(40.);
            y += m.before + prev_after;
            let top = m.pad;
            let lines_src: Vec<WrappedLine> = if b.kind.is_image() || b.is_empty() { Vec::new() } else {
                let k = key(b, i, text_w, salt);
                match &self.entries[i] {
                    Some((ck, l)) if *ck == k => l.clone(),
                    _ => {
                        self.shaped += 1;
                        let text: SharedString = b.text().into();
                        let runs = runs_for(b, &m, th);
                        let wrap = if m.mono { None } else { Some(px(text_w)) };
                        let l = window.text_system().shape_text(text, px(m.size), &runs, wrap, None).map(|v| v.into_vec()).unwrap_or_default();
                        self.entries[i] = Some((k, l.clone()));
                        l
                    }
                }
            };
            let mut lines = Vec::new();
            let (mut start, mut ly) = (0usize, 0f32);
            for l in lines_src {
                let h = f32::from(l.size(px(m.lh)).height);
                let len = l.len();
                lines.push(Line { start, y: ly, shaped: l });
                start += len + 1;
                ly += h;
            }
            let mut img = None;
            let text_h = if let Kind::Image(link) = &b.kind {
                let (link, want) = notedd_editor::model::image_parts(link);
                match pics.get(link) {
                    Some(p) => {
                        let k = match want { Some(w) => (w / p.w).min(text_w / p.w).min(MAX_PIC_H / p.h), None => (text_w / p.w).min(MAX_PIC_H / p.h).min(1.) };
                        let (w, h) = (p.w * k, p.h * k);
                        img = Some((p, w, h));
                        h
                    }
                    None => MISSING_H,
                }
            } else { ly.max(m.lh) };
            let h = text_h + 2. * m.pad;
            out.blocks.push(BlockBox { y, h, x: m.indent + m.pad, w: text_w, m, lines, top, img });
            y += h;
            let next = doc.blocks.get(i + 1).map(|n| &n.kind);
            prev_after = if b.kind.is_list() && next.is_some_and(|k| k.is_list()) { 3. * scale } else if b.kind.is_code() && next.is_some_and(|k| k.is_code()) { 0. } else { m.after };
        }
        out.height = y + 80.;
        out
    }
}

impl Layout {
    /// Top-left of the caret at `p`, relative to the document origin, and the line height.
    pub fn point_of(&self, p: Pos) -> (f32, f32, f32) {
        let b = &self.blocks[p.block.min(self.blocks.len() - 1)];
        let base_y = b.y + b.top;
        for l in b.lines.iter().rev() {
            if p.off >= l.start {
                let pt = l.shaped.position_for_index(p.off - l.start, px(b.m.lh)).unwrap_or_default();
                return (b.x + f32::from(pt.x), base_y + l.y + f32::from(pt.y), b.m.lh);
            }
        }
        (b.x, base_y, b.m.lh)
    }

    pub fn block_at(&self, y: f32) -> usize {
        for (i, b) in self.blocks.iter().enumerate() {
            let next_y = self.blocks.get(i + 1).map_or(f32::MAX, |n| n.y);
            if y < (b.y + b.h + next_y) / 2. || i + 1 == self.blocks.len() { return i; }
        }
        0
    }

    /// Document position under a point (relative to the document origin).
    pub fn pos_at(&self, x: f32, y: f32) -> Pos {
        let i = self.block_at(y);
        let b = &self.blocks[i];
        if b.lines.is_empty() { return Pos::new(i, 0); }
        let ry = y - b.y - b.top;
        let l = b.lines.iter().rev().find(|l| ry >= l.y).unwrap_or(&b.lines[0]);
        let pt = point(px(x - b.x), px((ry - l.y).max(0.)));
        let ix = l.shaped.closest_index_for_position(pt, px(b.m.lh)).unwrap_or_else(|e| e);
        Pos::new(i, l.start + ix.min(l.shaped.len()))
    }

    /// Rectangles (x, y, w, h; document coordinates) covering `a..b` of a block's text.
    pub fn rects(&self, block: usize, a: usize, b: usize, to_edge: bool) -> Vec<(f32, f32, f32, f32)> {
        let bb = &self.blocks[block];
        let mut out = Vec::new();
        let lh = bb.m.lh;
        let right = bb.x + bb.w;
        if bb.lines.is_empty() {
            if to_edge { out.push((bb.x, bb.y + bb.top, 6., lh)); }
            return out;
        }
        let (x1, y1, _) = self.point_of(Pos::new(block, a));
        let (x2, y2, _) = self.point_of(Pos::new(block, b));
        let edge = if to_edge { 6. } else { 0. };
        if (y1 - y2).abs() < 1. {
            out.push((x1, y1, (x2 - x1 + edge).max(0.), lh));
            return out;
        }
        out.push((x1, y1, (right - x1).max(0.), lh));
        let mut y = y1 + lh;
        while y < y2 - 1. { out.push((bb.x, y, right - bb.x, lh)); y += lh; }
        out.push((bb.x, y2, (x2 - bb.x + edge).max(0.), lh));
        out
    }
}
