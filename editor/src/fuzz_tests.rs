//! Random edit sequences: invariants must hold after every operation,
//! undo/redo must restore exactly, and Markdown must round-trip.

use crate::model::*;
use crate::{markdown, Core};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize { (self.next() % n as u64) as usize }
}

const WORDS: [&str; 8] = ["a", "bc", " ", "é", "😀", "x y", "  ", "слово"];

fn rand_pos(c: &Core, r: &mut Rng) -> Pos {
    let i = r.below(c.doc.blocks.len());
    let b = &c.doc.blocks[i];
    let t = b.text();
    let mut off = if b.kind.is_image() { 0 } else { r.below(t.len() + 1) };
    while !t.is_char_boundary(off) { off -= 1; }
    Pos::new(i, off)
}

fn step(c: &mut Core, r: &mut Rng) -> &'static str {
    match r.below(22) {
        0..=4 => { c.insert_text(WORDS[r.below(WORDS.len())]); "insert" }
        5 => { c.enter(); "enter" }
        6 | 7 => { c.backspace(); "backspace" }
        8 => { c.delete_forward(); "delete" }
        9 => { let (a, b) = (rand_pos(c, r), rand_pos(c, r)); c.set_selection(a, b); "select" }
        10 => { let a = rand_pos(c, r); c.move_to(a, false); "move" }
        11 => { c.toggle_mark([BOLD, ITALIC, STRIKE, HIGHLIGHT, CODE][r.below(5)]); "mark" }
        12 => { let k = [Kind::Heading(1), Kind::Quote, Kind::Bullet, Kind::Number, Kind::Todo(false), Kind::Code(String::new())][r.below(6)].clone(); c.toggle_kind(k); "kind" }
        13 => { c.insert_image("a.png"); "image" }
        14 => { c.insert_text(["- ", "1. ", "# ", "> ", "[ ] ", "**b**", "*i*", "`c`", "~~s~~", "==h=="][r.below(10)]); "shortcut" }
        15 => { c.insert_text("p\nq\n"); "multiline" }
        16 => { let b = c.copy_blocks(); c.insert_blocks(b); "paste" }
        17 => { c.paste_markdown(["- a\n- b", "x **y**", "![](b.png)\nz", "1. n\n2. m", "```\ncode\n```", "> q"][r.below(6)]); "paste md" }
        18 => { c.delete_word_back(); "wordback" }
        19 => { let p = c.next_pos(c.head); c.move_to(p, r.below(2) == 0); "next" }
        20 => { c.ctrl_enter(); "ctrl-enter" }
        _ => { c.select_all(); c.toggle_mark(BOLD); "all" }
    }
}

fn plain(doc: &Doc) -> Vec<(String, String)> {
    doc.blocks.iter().map(|b| {
        let k = match &b.kind { Kind::Number => "num".to_string(), k => format!("{k:?}") };
        let t = b.text();
        (k, t)
    }).collect()
}

fn check(c: &Core, ctx: &str) {
    if let Err(e) = c.doc.check() { panic!("{ctx}: {e}\n{:#?}", c.doc); }
    assert!(c.doc.valid(c.anchor) && c.doc.valid(c.head), "{ctx}: bad selection {:?} {:?}\n{:#?}", c.anchor, c.head, c.doc);
}

#[test]
fn random_edits() {
    let seeds: u64 = std::env::var("NOTEDD_FUZZ_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
    for seed in 1..=seeds {
        let mut r = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let mut c = Core::from_markdown("Title\nline one\n- item\n![](p.png)\ntail");
        let initial = c.doc.clone();
        let mut log = Vec::new();
        for n in 0..60 {
            let before = c.doc.clone();
            let name = step(&mut c, &mut r);
            log.push(name);
            let ctx = format!("seed {seed} step {n} {name} ({log:?})");
            check(&c, &ctx);
            // Markdown round-trips: the written file reads back to the same text and kinds.
            let md = markdown::write(&c.doc);
            let back = markdown::parse(&md);
            assert_eq!(plain(&back), plain(&c.doc), "{ctx}: markdown round trip\n{md:?}");
            assert_eq!(markdown::write(&back), md, "{ctx}: markdown not stable");
            // Undo and redo restore exactly.
            if c.doc != before {
                let now = c.doc.clone();
                let (a, h) = (c.anchor, c.head);
                if c.undo() {
                    check(&c, &format!("{ctx} undo"));
                    assert!(c.redo(), "{ctx}: redo missing");
                    assert_eq!(c.doc, now, "{ctx}: redo differs");
                    assert_eq!((c.anchor, c.head), (a, h), "{ctx}: redo selection");
                }
            }
        }
        while c.undo() { check(&c, &format!("seed {seed} unwinding")); }
        assert_eq!(c.doc, initial, "seed {seed}: full undo differs ({log:?})");
    }
}

fn todos(c: &Core) -> usize { c.doc.blocks.iter().filter(|b| matches!(b.kind, Kind::Todo(_))).count() }

/// Deleting, joining and splitting never turn a numbered or plain item into a task:
/// tasks appear only from an explicit command, a `[ ] ` shortcut or pasted Markdown.
#[test]
fn deleting_never_creates_tasks() {
    for seed in 1..=2000u64 {
        let mut r = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
        let mut c = Core::from_markdown("T\n1. one\n2. two\n- [ ] task\n3. three\n- bullet\n4. four\nplain");
        for n in 0..80 {
            let before = c.doc.blocks.iter().map(|b| b.kind.clone()).collect::<Vec<_>>();
            let todos_before = todos(&c);
            let op = r.below(9);
            // Enter on a task continues the list with a task: that one is expected.
            let (sel_a, _) = c.sel();
            let on_task = matches!(c.doc.blocks[sel_a.block].kind, Kind::Todo(_));
            match op {
                0 => c.backspace(),
                1 => c.delete_forward(),
                2 => c.delete_word_back(),
                3 => c.enter(),
                4 => c.insert_text(WORDS[r.below(WORDS.len())]),
                5 => { let (a, b) = (rand_pos(&c, &mut r), rand_pos(&c, &mut r)); c.set_selection(a, b); }
                6 => { let a = rand_pos(&c, &mut r); c.move_to(a, false); }
                7 => { c.toggle_mark(BOLD); }
                _ => { c.delete_selection(); }
            }
            check(&c, &format!("seed {seed} step {n} op {op}"));
            let after = c.doc.blocks.iter().map(|b| b.kind.clone()).collect::<Vec<_>>();
            if todos(&c) > todos_before && !(op == 3 && on_task) {
                panic!("seed {seed} step {n} op {op}: a task appeared\nbefore {before:?}\nafter  {after:?}");
            }
            // A joined block keeps the kind of the block it joined into, never a later one:
            // the kind sequence after an edit is a subsequence-with-changes only via these ops.
        }
    }
}

/// Enter and Backspace in a numbered list behave like a list in any editor.
#[test]
fn numbered_list_editing() {
    let mut c = Core::from_markdown("T\n1. one\n2. two");
    // Backspace at the start of an item: first unwraps it, the second joins it upwards.
    c.set_selection(Pos::new(2, 0), Pos::new(2, 0));
    c.backspace();
    assert_eq!(c.doc.blocks[2].kind, Kind::Para);
    c.backspace();
    assert_eq!(c.markdown(), "T\n1. onetwo");
    // Enter on an empty item ends the list; the following items stay numbered.
    let mut c = Core::from_markdown("T\n1. a\n2. \n3. c");
    c.set_selection(Pos::new(2, 0), Pos::new(2, 0));
    c.enter();
    assert_eq!(c.markdown(), "T\n1. a\n\n1. c");
    // A task above never changes the kind of a numbered item below it when its text is deleted.
    let mut c = Core::from_markdown("T\n- [ ] task\n1. num");
    c.set_selection(Pos::new(2, 0), Pos::new(2, 3));
    c.backspace();
    assert_eq!(c.doc.blocks[2].kind, Kind::Number);
    c.backspace();
    assert_eq!(c.doc.blocks[2].kind, Kind::Para);
    assert!(matches!(c.doc.blocks[1].kind, Kind::Todo(false)));
}
