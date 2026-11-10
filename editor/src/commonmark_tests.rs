//! A standard CommonMark parser (pulldown-cmark) as the reference: what Notedd writes
//! must mean the same thing to it.

use crate::model::*;
use crate::markdown;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// (depth, ordered, text) of every list item, the way a CommonMark reader sees them.
fn reference_items(md: &str) -> Vec<(usize, bool, String)> {
    let mut out: Vec<(usize, bool, String)> = Vec::new();
    let mut lists: Vec<bool> = Vec::new();
    let mut open: Option<usize> = None;
    for ev in Parser::new(md) {
        match ev {
            Event::Start(Tag::List(start)) => lists.push(start.is_some()),
            Event::End(TagEnd::List(_)) => { lists.pop(); }
            Event::Start(Tag::Item) => { out.push((lists.len() - 1, *lists.last().unwrap(), String::new())); open = Some(out.len() - 1); }
            Event::End(TagEnd::Item) => open = None,
            Event::Text(t) | Event::Code(t) => { if let Some(i) = open { out[i].2.push_str(&t); } }
            _ => {}
        }
    }
    out
}

/// The items of our own model: (level, numbered, text).
fn model_items(blocks: &[Block]) -> Vec<(usize, bool, String)> {
    let lv = levels(blocks);
    blocks.iter().zip(lv)
        .filter(|(b, _)| matches!(b.kind, Kind::Bullet | Kind::Number))
        .map(|(b, l)| (l as usize, b.kind == Kind::Number, b.text()))
        .collect()
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

#[test]
fn nested_lists_mean_the_same_to_a_commonmark_reader() {
    for seed in 1..=500u64 {
        let mut r = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let mut blocks = vec![Block::para("Title")];
        let mut prev: Option<u8> = None;
        for n in 0..r.below(12) + 1 {
            if r.below(6) == 0 { blocks.push(Block::new(Kind::Para)); prev = None; continue; }
            let mut b = Block::para(&format!("item{n}"));
            b.kind = if r.below(2) == 0 { Kind::Bullet } else { Kind::Number };
            b.indent = r.below(prev.map_or(0, |p| p as usize + 1).min(MAX_INDENT as usize) + 1) as u8;
            prev = Some(b.indent);
            blocks.push(b);
        }
        let md = markdown::write_blocks(&blocks[1..]);
        let ours = model_items(&blocks);
        let theirs = reference_items(&md);
        assert_eq!(theirs, ours, "seed {seed}\n{md}");
        // and reading it back gives the same items
        assert_eq!(model_items(&markdown::parse_blocks(&md)), ours, "seed {seed}: round trip\n{md}");
    }
}

#[test]
fn files_written_by_other_editors_read_the_same_way() {
    // two-space, four-space and tab nesting, as other editors write them
    for src in ["- a\n  - b\n    - c\n- d", "- a\n    - b\n        - c\n- d", "- a\n\t- b\n\t\t- c\n- d"] {
        let theirs = reference_items(src);
        let ours = model_items(&markdown::parse_blocks(src));
        assert_eq!(ours, theirs, "{src:?}");
    }
    let src = "1. a\n   1. b\n   2. c\n2. d";
    assert_eq!(model_items(&markdown::parse_blocks(src)), reference_items(src));
}
