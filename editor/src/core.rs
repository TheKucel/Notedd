//! The edit kernel. Every document change goes through `Core`:
//! checkpoint -> operation -> (UI) redraw. Positions are `Pos` (block, byte).
//!
//! Images are blocks without text. The single position `(i, 0)` of an image
//! block means "the image is selected"; Backspace/Delete beside an image first
//! move onto it, and the next press removes it. Text never glues onto an image.

use crate::markdown;
use crate::model::*;
use notedd_core::md;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind { Typing, Delete, Other }

#[derive(Clone)]
struct Snap { doc: Doc, anchor: Pos, head: Pos }

pub struct Core {
    pub doc: Doc,
    pub anchor: Pos,
    pub head: Pos,
    /// Marks the next typed text gets (toggled with nothing selected).
    pub typing: Option<Marks>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last: Option<EditKind>,
}

const UNDO_LIMIT: usize = 500;

fn prev_boundary(text: &str, off: usize) -> usize {
    text.grapheme_indices(true).take_while(|(i, _)| *i < off).last().map_or(0, |(i, _)| i)
}
fn next_boundary(text: &str, off: usize) -> usize {
    text[off..].graphemes(true).next().map_or(off, |g| off + g.len())
}
fn blank(w: &str) -> bool { w.chars().all(char::is_whitespace) }

impl Core {
    pub fn new(doc: Doc) -> Core {
        let mut doc = doc;
        doc.fix();
        let end = doc.end();
        Core { doc, anchor: end, head: end, typing: None, undo: Vec::new(), redo: Vec::new(), last: None }
    }
    pub fn from_markdown(src: &str) -> Core { Core::new(markdown::parse(src)) }
    pub fn markdown(&self) -> String { markdown::write(&self.doc) }

    // ---- selection and movement -------------------------------------------------

    pub fn sel(&self) -> (Pos, Pos) { if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) } }
    pub fn collapsed(&self) -> bool { self.anchor == self.head }
    fn image(&self, i: usize) -> bool { self.doc.blocks[i].kind.is_image() }
    fn blen(&self, i: usize) -> usize { self.doc.blocks[i].len() }

    pub fn clamp(&self, p: Pos) -> Pos {
        let i = p.block.min(self.doc.blocks.len() - 1);
        if self.image(i) { return Pos::new(i, 0); }
        let text = self.doc.blocks[i].text();
        let mut off = p.off.min(text.len());
        while !text.is_char_boundary(off) { off -= 1; }
        Pos::new(i, off)
    }
    pub fn set_selection(&mut self, anchor: Pos, head: Pos) {
        self.anchor = self.clamp(anchor);
        self.head = self.clamp(head);
        self.moved();
    }
    pub fn move_to(&mut self, p: Pos, extend: bool) {
        self.head = self.clamp(p);
        if !extend { self.anchor = self.head; }
        self.moved();
    }
    pub fn select_all(&mut self) {
        self.anchor = Pos::new(0, 0);
        self.head = self.doc.end();
        self.moved();
    }
    fn moved(&mut self) { self.typing = None; self.last = None; }

    pub fn prev_pos(&self, p: Pos) -> Pos {
        if !self.image(p.block) && p.off > 0 {
            return Pos::new(p.block, prev_boundary(&self.doc.blocks[p.block].text(), p.off));
        }
        if p.block == 0 { return p; }
        Pos::new(p.block - 1, self.blen(p.block - 1))
    }
    pub fn next_pos(&self, p: Pos) -> Pos {
        if !self.image(p.block) && p.off < self.blen(p.block) {
            return Pos::new(p.block, next_boundary(&self.doc.blocks[p.block].text(), p.off));
        }
        if p.block + 1 >= self.doc.blocks.len() { return p; }
        Pos::new(p.block + 1, 0)
    }
    pub fn word_left(&self, p: Pos) -> Pos {
        if self.image(p.block) || p.off == 0 { return self.prev_pos(p); }
        let text = self.doc.blocks[p.block].text();
        for (i, w) in text.split_word_bound_indices().rev() {
            if i < p.off && !blank(w) { return Pos::new(p.block, i); }
        }
        Pos::new(p.block, 0)
    }
    pub fn word_right(&self, p: Pos) -> Pos {
        if self.image(p.block) || p.off >= self.blen(p.block) { return self.next_pos(p); }
        let text = self.doc.blocks[p.block].text();
        let mut after = false;
        for (i, w) in text.split_word_bound_indices() {
            let end = i + w.len();
            if end <= p.off { continue; }
            if after { if !blank(w) { return Pos::new(p.block, i); } } else if blank(w) { after = true; if end >= text.len() { break; } } else { after = true; }
        }
        Pos::new(p.block, text.len())
    }
    pub fn block_start(&self, p: Pos) -> Pos { Pos::new(p.block, 0) }
    pub fn block_end(&self, p: Pos) -> Pos { Pos::new(p.block, self.blen(p.block)) }

    // ---- history ----------------------------------------------------------------

    fn snap(&self) -> Snap { Snap { doc: self.doc.clone(), anchor: self.anchor, head: self.head } }
    fn push_undo(&mut self) {
        let s = self.snap();
        self.undo.push(s);
        if self.undo.len() > UNDO_LIMIT { self.undo.remove(0); }
        self.redo.clear();
    }
    /// Call before a change. Consecutive typing (or deleting) shares one step.
    pub fn checkpoint(&mut self, kind: EditKind) {
        let join = kind != EditKind::Other && self.last == Some(kind) && self.collapsed();
        if !join { self.push_undo(); }
        self.last = Some(kind);
    }
    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else { return false };
        let cur = self.snap();
        self.redo.push(cur);
        self.restore(s);
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        let cur = self.snap();
        self.undo.push(cur);
        self.restore(s);
        true
    }
    fn restore(&mut self, s: Snap) {
        self.doc = s.doc;
        self.anchor = s.anchor;
        self.head = s.head;
        self.moved();
    }
    /// Drop history, e.g. after a conversion that must not be undoable.
    pub fn clear_history(&mut self) { self.undo.clear(); self.redo.clear(); self.last = None; }

    // ---- deleting ---------------------------------------------------------------

    /// Remove `a..b`, return where the caret goes. Does not touch history/selection.
    fn delete_range(&mut self, a: Pos, b: Pos) -> Pos {
        if a >= b { return a; }
        if a.block == b.block {
            self.doc.blocks[a.block].replace(a.off, b.off, Vec::new());
            return a;
        }
        let a_img = self.image(a.block);
        let b_img = self.image(b.block);
        let a_end = !a_img && a.off == self.blen(a.block);
        // Only images between the end of one paragraph and the start of the next:
        // remove the images, do not glue the paragraphs.
        if a_end && !b_img && b.off == 0 && b.block > a.block + 1 && (a.block + 1..b.block).all(|i| self.image(i)) {
            self.doc.blocks.drain(a.block + 1..b.block); self.doc.fix();
            return a;
        }
        let a_code = !a_img && self.doc.blocks[a.block].kind.is_code();
        if b_img || (!a_code && self.doc.blocks[b.block].kind.is_code()) {
            // Images and code never merge into a paragraph: block `b` stays (minus the
            // deleted head of its text) and `a` keeps only its own head.
            if !b_img { self.doc.blocks[b.block].replace(0, b.off, Vec::new()); }
            if a_img {
                self.doc.blocks.drain(a.block..b.block); self.doc.fix();
                return Pos::new(a.block, 0);
            }
            let n = self.blen(a.block);
            self.doc.blocks[a.block].replace(a.off, n, Vec::new());
            self.doc.blocks.drain(a.block + 1..b.block); self.doc.fix();
            return a;
        }
        let head = if a_img { Vec::new() } else { self.doc.blocks[a.block].slice(0, a.off) };
        let head_len: usize = head.iter().map(|r| r.text.len()).sum();
        let tb = &self.doc.blocks[b.block];
        let tail = tb.slice(b.off, tb.len());
        let (kind, indent) = if a_img { (tb.kind.clone(), tb.indent) } else { (self.doc.blocks[a.block].kind.clone(), self.doc.blocks[a.block].indent) };
        let mut runs = head;
        runs.extend(tail);
        normalize(&mut runs);
        self.doc.blocks.splice(a.block..=b.block, [Block { kind, runs, indent }]);
        self.doc.fix();
        Pos::new(a.block, head_len)
    }

    /// Delete the selection; true if there was one.
    fn remove_selection(&mut self) -> bool {
        if self.collapsed() { return false; }
        let (s, e) = self.sel();
        let p = self.delete_range(s, e);
        self.anchor = p;
        self.head = p;
        self.typing = None;
        true
    }
    pub fn delete_selection(&mut self) -> bool {
        if self.collapsed() { return false; }
        self.checkpoint(EditKind::Other);
        self.remove_selection()
    }

    pub fn backspace(&mut self) {
        if !self.collapsed() { self.checkpoint(EditKind::Other); self.remove_selection(); return; }
        let p = self.head;
        let i = p.block;
        if self.image(i) {
            self.checkpoint(EditKind::Other);
            self.doc.blocks.remove(i); self.doc.fix();
            let q = self.clamp(Pos::new(i - 1, usize::MAX));
            self.anchor = q; self.head = q; self.typing = None;
            return;
        }
        if p.off > 0 {
            self.checkpoint(EditKind::Delete);
            let from = prev_boundary(&self.doc.blocks[i].text(), p.off);
            self.doc.blocks[i].replace(from, p.off, Vec::new());
            self.anchor = Pos::new(i, from); self.head = self.anchor; self.typing = None;
            return;
        }
        if i == 0 { return; }
        // At the start of a block.
        if self.doc.blocks[i].kind != Kind::Para {
            self.checkpoint(EditKind::Other);
            let level = levels(&self.doc.blocks)[i];
            if level > 0 {
                // a nested item steps out one level before it stops being an item
                self.doc.blocks[i].indent = level - 1;
                self.doc.fix();
            } else {
                self.set_kind_range(i, i, Kind::Para);
            }
            return;
        }
        let prev = i - 1;
        if self.image(prev) {
            // First press selects the picture (an empty line below it is dropped).
            self.checkpoint(EditKind::Other);
            if self.doc.blocks[i].is_empty() && i + 1 < self.doc.blocks.len() { self.doc.blocks.remove(i); self.doc.fix(); }
            self.anchor = Pos::new(prev, 0); self.head = self.anchor; self.typing = None;
            return;
        }
        if self.doc.blocks[prev].kind.is_code() {
            self.checkpoint(EditKind::Other);
            if self.doc.blocks[i].is_empty() { self.doc.blocks.remove(i); self.doc.fix(); }
            let q = Pos::new(prev, self.blen(prev));
            self.anchor = q; self.head = q; self.typing = None;
            return;
        }
        self.checkpoint(EditKind::Other);
        let q = self.join_with_next(prev);
        self.anchor = q; self.head = q; self.typing = None;
    }

    /// Append block `i + 1` to block `i`; returns the join point.
    fn join_with_next(&mut self, i: usize) -> Pos {
        let next = self.doc.blocks.remove(i + 1);
        let at = self.blen(i);
        let b = &mut self.doc.blocks[i];
        let mut runs = next.runs;
        if b.kind.is_code() { for r in &mut runs { r.marks = Marks::default(); } }
        b.runs.extend(runs);
        normalize(&mut b.runs);
        self.doc.fix();
        Pos::new(i, at)
    }

    pub fn delete_forward(&mut self) {
        if !self.collapsed() { self.checkpoint(EditKind::Other); self.remove_selection(); return; }
        let p = self.head;
        let i = p.block;
        if self.image(i) {
            self.checkpoint(EditKind::Other);
            self.doc.blocks.remove(i); self.doc.fix();
            let q = if i < self.doc.blocks.len() { self.clamp(Pos::new(i, 0)) } else { self.clamp(Pos::new(i - 1, usize::MAX)) };
            self.anchor = q; self.head = q; self.typing = None;
            return;
        }
        let len = self.blen(i);
        if p.off < len {
            self.checkpoint(EditKind::Delete);
            let to = next_boundary(&self.doc.blocks[i].text(), p.off);
            self.doc.blocks[i].replace(p.off, to, Vec::new());
            self.typing = None;
            return;
        }
        if i + 1 >= self.doc.blocks.len() { return; }
        let next = i + 1;
        self.checkpoint(EditKind::Other);
        if self.image(next) {
            self.anchor = Pos::new(next, 0); self.head = self.anchor; self.typing = None;
            return;
        }
        if self.doc.blocks[next].kind.is_code() {
            // Do not pull code into a paragraph; an empty line makes way for it.
            if self.doc.blocks[i].is_empty() && i > 0 {
                self.doc.blocks.remove(i); self.doc.fix();
                self.anchor = Pos::new(i, 0); self.head = self.anchor;
            }
            return;
        }
        if self.doc.blocks[i].is_empty() && i > 0 && self.doc.blocks[i].kind == Kind::Para {
            self.doc.blocks.remove(i); self.doc.fix();
            self.anchor = Pos::new(i, 0); self.head = self.anchor; self.typing = None;
            return;
        }
        let q = self.join_with_next(i);
        self.anchor = q; self.head = q; self.typing = None;
    }

    pub fn delete_word_back(&mut self) {
        if !self.collapsed() { self.backspace(); return; }
        let from = self.word_left(self.head);
        if from == self.head { return; }
        self.checkpoint(EditKind::Other);
        let p = self.delete_range(from, self.head);
        self.anchor = p; self.head = p; self.typing = None;
    }
    /// Tab / Shift+Tab: move the selected list items one level deeper or shallower, with the
    /// items nested under them. Returns false (and changes nothing) when there is nothing to move.
    pub fn change_indent(&mut self, deeper: bool) -> bool {
        let (s, e) = self.sel();
        let lv = levels(&self.doc.blocks);
        let list = |i: usize| self.doc.blocks[i].kind.is_list();
        let mut moving = std::collections::BTreeSet::new();
        for i in s.block..=e.block {
            if !list(i) || (!deeper && lv[i] == 0) { continue; }
            moving.insert(i);
            let mut j = i + 1;
            while j < lv.len() && list(j) && lv[j] > lv[i] { moving.insert(j); j += 1; }
        }
        if moving.is_empty() { return false; }
        for &i in &moving {
            if deeper && lv[i] >= MAX_INDENT { return false; }
            // an item can sit at most one level below the item above it
            if deeper && !moving.contains(&i.wrapping_sub(1)) && !(i > 0 && list(i - 1) && lv[i] <= lv[i - 1]) { return false; }
        }
        self.checkpoint(EditKind::Other);
        for &i in &moving { self.doc.blocks[i].indent = if deeper { lv[i] + 1 } else { lv[i] - 1 }; }
        self.doc.fix();
        self.typing = None;
        true
    }

    /// Ctrl+Delete: the word after the caret (at the end of a block it behaves like Delete).
    pub fn delete_word_forward(&mut self) {
        if !self.collapsed() { self.backspace(); return; }
        let h = self.head;
        if self.image(h.block) || h.off >= self.blen(h.block) { self.delete_forward(); return; }
        let to = self.word_right(h);
        if to == h { return; }
        self.checkpoint(EditKind::Other);
        self.delete_range(h, to);
        self.anchor = h; self.head = h; self.typing = None;
    }
    pub fn delete_to_block_start(&mut self) {
        if !self.collapsed() { self.backspace(); return; }
        let h = self.head;
        if self.image(h.block) || h.off == 0 { self.backspace(); return; }
        self.checkpoint(EditKind::Other);
        let p = self.delete_range(Pos::new(h.block, 0), h);
        self.anchor = p; self.head = p; self.typing = None;
    }

    // ---- typing -----------------------------------------------------------------

    /// Typed or IME-committed text. Newlines become paragraph breaks.
    pub fn insert_text(&mut self, s: &str) {
        if s.is_empty() { return; }
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        let single = !s.contains('\n');
        self.checkpoint(if single && self.collapsed() { EditKind::Typing } else { EditKind::Other });
        self.remove_selection();
        for (k, seg) in s.split('\n').enumerate() {
            if k > 0 { self.enter_inner(); }
            if !seg.is_empty() { self.insert_plain(seg); }
        }
        if single { self.after_typing(&s); }
    }

    /// Text from an IME composition: typed like `insert_text`, without Markdown shortcuts.
    pub fn insert_raw(&mut self, s: &str) {
        if s.is_empty() && self.collapsed() { return; }
        self.checkpoint(if self.collapsed() { EditKind::Typing } else { EditKind::Other });
        self.remove_selection();
        if !s.is_empty() { self.insert_plain(s); }
    }

    fn insert_plain(&mut self, seg: &str) {
        let p = self.head;
        let i = p.block;
        if self.image(i) {
            // Typing with a picture selected starts a paragraph below it.
            self.doc.blocks.insert(i + 1, Block::new(Kind::Para));
            self.anchor = Pos::new(i + 1, 0);
            self.head = self.anchor;
            return self.insert_plain(seg);
        }
        let mut marks = if self.doc.blocks[i].kind.is_code() { Marks::default() }
            else { self.typing.take().unwrap_or_else(|| self.doc.blocks[i].marks_near(p.off)) };
        // A backtick cannot live inside a code span.
        if marks.has(CODE) && seg.contains('`') { marks = marks.with(CODE, false); }
        self.doc.blocks[i].replace(p.off, p.off, vec![Run::new(seg, marks)]);
        self.anchor = Pos::new(i, p.off + seg.len());
        self.head = self.anchor;
    }

    fn after_typing(&mut self, s: &str) {
        if s == " " { self.block_shortcut(); return; }
        let mut it = s.chars();
        if let (Some(c), None) = (it.next(), it.next()) {
            if matches!(c, '`' | '*' | '_' | '~' | '=') { self.inline_shortcut(c); }
        }
    }

    /// "- ", "1. ", "# ", "> ", "[ ] " typed at the start of a line.
    fn block_shortcut(&mut self) {
        let p = self.head;
        let i = p.block;
        if i == 0 || self.image(i) { return; }
        let b = &self.doc.blocks[i];
        if !matches!(b.kind, Kind::Para | Kind::Bullet) { return; }
        let text = b.text();
        let prefix = &text[..p.off];
        let new = if b.kind == Kind::Para {
            match prefix {
                "- " | "* " | "+ " => Some(Kind::Bullet),
                "> " => Some(Kind::Quote),
                "[ ] " | "[] " => Some(Kind::Todo(false)),
                "[x] " => Some(Kind::Todo(true)),
                _ => {
                    let d = prefix.bytes().take_while(u8::is_ascii_digit).count();
                    let hashes = prefix.bytes().take_while(|&c| c == b'#').count();
                    if (1..4).contains(&d) && (&prefix[d..] == ". " || &prefix[d..] == ") ") { Some(Kind::Number) }
                    else if (1..=6).contains(&hashes) && &prefix[hashes..] == " " { Some(Kind::Heading(hashes.min(3) as u8)) }
                    else { None }
                }
            }
        } else {
            match prefix { "[ ] " | "[] " => Some(Kind::Todo(false)), "[x] " => Some(Kind::Todo(true)), _ => None }
        };
        let Some(kind) = new else { return };
        // The literal text stays one undo step away.
        self.push_undo();
        self.last = None;
        let b = &mut self.doc.blocks[i];
        b.replace(0, p.off, Vec::new());
        b.kind = kind;
        self.anchor = Pos::new(i, 0);
        self.head = self.anchor;
        self.typing = None;
    }

    /// `code`, **bold**, *italic*, ~~strike~~, ==highlight== closed by typing the delimiter.
    fn inline_shortcut(&mut self, c: char) {
        let p = self.head;
        let i = p.block;
        if self.image(i) || self.doc.blocks[i].kind.is_code() || p.off == 0 { return; }
        if self.doc.blocks[i].marks_near(p.off).has(CODE) && c != '`' { return; }
        let text = self.doc.blocks[i].text();
        let upto = &text[..p.off]; // ends with the typed delimiter
        let body = &upto[..upto.len() - 1];
        let double = matches!(c, '~' | '*' | '_' | '=') && body.ends_with(c);
        let width = if double { 2 } else { 1 };
        let (bit, delim_end) = match (c, double) {
            ('`', _) => (CODE, upto.len() - 1),
            ('~', true) => (STRIKE, upto.len() - 2),
            ('=', true) => (HIGHLIGHT, upto.len() - 2),
            ('*' | '_', true) => (BOLD, upto.len() - 2),
            ('*' | '_', false) => (ITALIC, upto.len() - 1),
            _ => return,
        };
        if matches!(c, '~' | '=') && !double { return; }
        // The opener: the nearest earlier delimiter that is not part of a longer run.
        let before = &text[..delim_end];
        let dl = c.to_string().repeat(width);
        let mut from = before.len();
        let open = loop {
            let Some(j) = before[..from].rfind(&dl) else { return };
            let prev = before[..j].chars().next_back();
            let next = before[j + width..].chars().next();
            let run_ok = prev != Some(c) && next != Some(c);
            let word_ok = c != '_' || !prev.is_some_and(char::is_alphanumeric);
            if run_ok && word_ok { break j; }
            if j == 0 { return; }
            from = j;
        };
        let content = &before[open + width..];
        if content.is_empty() || content.starts_with(char::is_whitespace) || content.ends_with(char::is_whitespace) { return; }
        if bit == CODE && content.contains('`') { return; }
        // Convert: drop the closer and the opener, style the content.
        self.push_undo();
        self.last = None;
        let b = &mut self.doc.blocks[i];
        b.replace(delim_end, p.off, Vec::new());
        b.replace(open, open + width, Vec::new());
        let (a, z) = (open, delim_end - width);
        apply_marks(b, a, z, |m| m.with(bit, true));
        self.anchor = Pos::new(i, z);
        self.head = self.anchor;
        self.typing = Some(self.doc.blocks[i].marks_near(z).with(bit, false));
    }

    // ---- enter ------------------------------------------------------------------

    pub fn enter(&mut self) {
        self.checkpoint(EditKind::Other);
        self.remove_selection();
        self.enter_inner();
    }

    fn enter_inner(&mut self) {
        let p = self.head;
        let i = p.block;
        self.typing = None;
        let kind = self.doc.blocks[i].kind.clone();
        if kind.is_image() {
            self.doc.blocks.insert(i + 1, Block::new(Kind::Para));
            self.anchor = Pos::new(i + 1, 0);
            self.head = self.anchor;
            return;
        }
        let len = self.blen(i);
        if let Kind::Code(_) = kind {
            let text = self.doc.blocks[i].text();
            // A second Enter on an empty last line leaves the block.
            if p.off == len && text.ends_with('\n') {
                self.doc.blocks[i].replace(len - 1, len, Vec::new());
                self.doc.blocks.insert(i + 1, Block::new(Kind::Para));
                self.anchor = Pos::new(i + 1, 0);
                self.head = self.anchor;
                return;
            }
            self.doc.blocks[i].replace(p.off, p.off, vec![Run::new("\n", Marks::default())]);
            self.anchor = Pos::new(i, p.off + 1);
            self.head = self.anchor;
            return;
        }
        // Enter on an empty list item or quote ends it; a nested item first steps out one level.
        if len == 0 && i > 0 && (kind.is_list() || kind == Kind::Quote) {
            let level = levels(&self.doc.blocks)[i];
            if kind.is_list() && level > 0 {
                self.doc.blocks[i].indent = level - 1;
            } else {
                self.doc.blocks[i].kind = Kind::Para;
                self.doc.blocks[i].indent = 0;
            }
            self.doc.fix();
            return;
        }
        if i > 0 && kind == Kind::Para && p.off == len {
            let text = self.doc.blocks[i].text();
            // ```lang + Enter opens a code block.
            if let Some(lang) = text.strip_prefix("```") {
                if !lang.contains('`') {
                    self.doc.blocks[i] = Block::new(Kind::Code(lang.trim().to_string()));
                    self.anchor = Pos::new(i, 0);
                    self.head = self.anchor;
                    return;
                }
            }
            // A typed ![](path) line turns into the picture.
            if let Some(path) = md::image_path(&text.encode_utf16().collect::<Vec<_>>()) {
                self.doc.blocks[i] = Block::new(Kind::Image(path));
                self.doc.blocks.insert(i + 1, Block::new(Kind::Para));
                self.anchor = Pos::new(i + 1, 0);
                self.head = self.anchor;
                return;
            }
        }
        let b = &mut self.doc.blocks[i];
        let tail = b.slice(p.off, len);
        b.replace(p.off, len, Vec::new());
        let next_kind = match kind {
            Kind::Bullet => Kind::Bullet,
            Kind::Number => Kind::Number,
            Kind::Todo(_) => Kind::Todo(false),
            Kind::Quote => Kind::Quote,
            _ => Kind::Para,
        };
        let indent = if next_kind.is_list() { b.indent } else { 0 };
        self.doc.blocks.insert(i + 1, Block { kind: next_kind, runs: tail, indent });
        self.anchor = Pos::new(i + 1, 0);
        self.head = self.anchor;
    }

    /// Ctrl+Enter: tick a task, or make the line one.
    pub fn ctrl_enter(&mut self) {
        let i = self.head.block;
        if i == 0 || self.image(i) { return; }
        self.checkpoint(EditKind::Other);
        match self.doc.blocks[i].kind {
            Kind::Todo(d) => self.doc.blocks[i].kind = Kind::Todo(!d),
            Kind::Para | Kind::Bullet | Kind::Number | Kind::Quote | Kind::Heading(_) => self.doc.blocks[i].kind = Kind::Todo(false),
            _ => {}
        }
    }
    pub fn toggle_todo(&mut self, i: usize) {
        if let Some(Kind::Todo(d)) = self.doc.blocks.get(i).map(|b| b.kind.clone()) {
            self.checkpoint(EditKind::Other);
            self.doc.blocks[i].kind = Kind::Todo(!d);
        }
    }

    // ---- formatting -------------------------------------------------------------

    fn text_blocks(&self, s: Pos, e: Pos) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
        (s.block..=e.block).filter_map(move |i| {
            let b = &self.doc.blocks[i];
            if b.kind.is_image() || b.kind.is_code() { return None; }
            let a = if i == s.block { s.off } else { 0 };
            let z = if i == e.block { e.off } else { b.len() };
            (a < z).then_some((i, a, z))
        })
    }

    pub fn toggle_mark(&mut self, bit: u8) {
        let (s, e) = self.sel();
        if s == e {
            if self.image(s.block) || self.doc.blocks[s.block].kind.is_code() { return; }
            let cur = self.typing.clone().unwrap_or_else(|| self.doc.blocks[s.block].marks_near(s.off));
            self.typing = Some(cur.with(bit, !cur.has(bit)));
            return;
        }
        let ranges: Vec<_> = self.text_blocks(s, e).collect();
        if ranges.is_empty() { return; }
        let ticks = ranges.iter().any(|&(i, a, z)| self.doc.blocks[i].text()[a..z].contains('`'));
        if bit == CODE && ticks { return; }
        let all = ranges.iter().all(|&(i, a, z)| cut(&self.doc.blocks[i].runs, a, z).iter().all(|r| r.marks.has(bit)));
        self.checkpoint(EditKind::Other);
        for (i, a, z) in ranges { apply_marks(&mut self.doc.blocks[i], a, z, |m| m.with(bit, !all)); }
    }

    pub fn set_link(&mut self, url: Option<&str>) {
        let (s, e) = self.sel();
        let ranges: Vec<_> = self.text_blocks(s, e).collect();
        if ranges.is_empty() { return; }
        self.checkpoint(EditKind::Other);
        let link = url.filter(|u| !u.is_empty()).map(|u| u.into());
        for (i, a, z) in ranges {
            apply_marks(&mut self.doc.blocks[i], a, z, |m| Marks { bits: m.bits, link: link.clone() });
        }
    }

    /// Turn the selected blocks into `kind`, or back to paragraphs if they all already are.
    pub fn toggle_kind(&mut self, kind: Kind) {
        let (s, e) = self.sel();
        let same = |k: &Kind| match (k, &kind) {
            (Kind::Heading(a), Kind::Heading(b)) => a == b,
            (Kind::Todo(_), Kind::Todo(_)) | (Kind::Code(_), Kind::Code(_)) => true,
            (a, b) => a == b,
        };
        let idx: Vec<_> = (s.block.max(1)..=e.block).filter(|&i| !self.image(i)).collect();
        if idx.is_empty() { return; }
        let all = idx.iter().all(|&i| same(&self.doc.blocks[i].kind));
        self.checkpoint(EditKind::Other);
        self.set_kind_range(s.block.max(1), e.block, if all { Kind::Para } else { kind });
    }

    /// Set the kind of blocks `a..=b` (images and the title are skipped).
    fn set_kind_range(&mut self, a: usize, b: usize, kind: Kind) {
        for i in (a.max(1)..=b).rev() {
            if self.image(i) { continue; }
            let was_code = self.doc.blocks[i].kind.is_code();
            if was_code && !kind.is_code() {
                // Code lines become blocks of the new kind.
                let text = self.doc.blocks[i].text();
                let lines: Vec<&str> = text.split('\n').collect();
                let remap = |p: Pos| -> Pos {
                    if p.block > i { return Pos::new(p.block + lines.len() - 1, p.off); }
                    if p.block < i { return p; }
                    let mut at = 0;
                    for (k, l) in lines.iter().enumerate() {
                        if p.off <= at + l.len() { return Pos::new(i + k, p.off - at); }
                        at += l.len() + 1;
                    }
                    p
                };
                self.anchor = remap(self.anchor);
                self.head = remap(self.head);
                let blocks: Vec<Block> = lines.iter().map(|l| {
                    let mut nb = Block::new(kind.clone());
                    if !l.is_empty() { nb.runs.push(Run::new(*l, Marks::default())); }
                    nb
                }).collect();
                self.doc.blocks.splice(i..=i, blocks);
            } else {
                self.doc.blocks[i].kind = kind.clone();
            }
        }
        self.doc.fix();
        self.anchor = self.clamp(self.anchor);
        self.head = self.clamp(self.head);
        self.typing = None;
    }

    // ---- objects and clipboard --------------------------------------------------

    /// Insert a picture at the caret; text after the caret moves below it.
    pub fn insert_image(&mut self, path: &str) {
        self.checkpoint(EditKind::Other);
        self.remove_selection();
        let p = self.head;
        let mut i = p.block;
        let mut tail = Vec::new();
        // Code is never split by a picture: it goes below the code block.
        if !self.image(i) && !self.doc.blocks[i].kind.is_code() {
            let b = &mut self.doc.blocks[i];
            let len = b.len();
            tail = b.slice(p.off, len);
            b.replace(p.off, len, Vec::new());
            if b.is_empty() && i > 0 && b.kind == Kind::Para && tail.is_empty() {
                // An empty line is replaced by the picture.
                self.doc.blocks.remove(i); self.doc.fix();
                i -= 1;
            }
        }
        self.doc.blocks.insert(i + 1, Block::new(Kind::Image(path.to_string())));
        let after_ok = self.doc.blocks.get(i + 2).is_some_and(|b| !b.kind.is_image()) && tail.is_empty();
        if !after_ok { self.doc.blocks.insert(i + 2, Block { kind: Kind::Para, runs: tail, indent: 0 }); }
        self.doc.fix();
        self.anchor = Pos::new(i + 2, 0);
        self.head = self.anchor;
        self.typing = None;
    }

    /// Sets the displayed width of the picture block `i` (None: natural size).
    pub fn set_image_width(&mut self, i: usize, w: Option<f32>, checkpoint: bool) {
        let Some(Kind::Image(p)) = self.doc.blocks.get(i).map(|b| b.kind.clone()) else { return };
        if checkpoint { self.checkpoint(EditKind::Other); }
        let path = crate::model::image_parts(&p).0.to_string();
        self.doc.blocks[i].kind = Kind::Image(match w { Some(w) => format!("{path}\x01{}", w.round() as i32), None => path });
    }

    /// The selection as blocks (partial first and last block).
    pub fn copy_blocks(&self) -> Vec<Block> {
        let (s, e) = self.sel();
        if s == e { return Vec::new(); }
        let b = &self.doc.blocks;
        if s.block == e.block {
            let blk = &b[s.block];
            return vec![Block { kind: blk.kind.clone(), runs: blk.slice(s.off, e.off), indent: blk.indent }];
        }
        let mut out = Vec::new();
        let first = &b[s.block];
        out.push(if first.kind.is_image() { first.clone() } else { Block { kind: first.kind.clone(), runs: first.slice(s.off, first.len()), indent: first.indent } });
        out.extend(b[s.block + 1..e.block].iter().cloned());
        let last = &b[e.block];
        if !last.kind.is_image() { out.push(Block { kind: last.kind.clone(), runs: last.slice(0, e.off), indent: last.indent }); }
        out
    }
    /// Markdown for the clipboard: one block is copied as its inline text.
    pub fn selection_markdown(&self) -> String {
        let mut blocks = self.copy_blocks();
        // the copy starts at the top level, however deep it sat
        let (all, first) = (crate::model::levels(&self.doc.blocks), self.sel().0.block);
        let base = blocks.iter().enumerate().filter(|(_, b)| b.kind.is_list()).map(|(k, _)| all[first + k]).min().unwrap_or(0);
        for (k, b) in blocks.iter_mut().enumerate() { b.indent = all[first + k].saturating_sub(base); }
        match blocks.as_slice() {
            [b] if b.kind.is_image() => markdown::write_blocks(&blocks),
            [b] if b.kind.is_code() => b.text(),
            [b] => markdown::write_inline(&b.runs),
            _ => markdown::write_blocks(&blocks),
        }
    }
    pub fn cut_blocks(&mut self) -> Vec<Block> {
        let v = self.copy_blocks();
        if !v.is_empty() { self.delete_selection(); }
        v
    }

    /// Paste Markdown text.
    pub fn paste_markdown(&mut self, text: &str) {
        let i = self.head.block;
        if !self.image(i) && self.doc.blocks[i].kind.is_code() { return self.insert_text(&text.replace("\r\n", "\n")); }
        self.insert_blocks(markdown::parse_blocks(text));
    }

    /// Paste blocks at the caret, merging the first and last into the surrounding text.
    pub fn insert_blocks(&mut self, mut blocks: Vec<Block>) {
        if blocks.is_empty() { return; }
        self.checkpoint(EditKind::Other);
        self.remove_selection();
        self.typing = None;
        let p = self.head;
        let mut i = p.block;
        if self.image(i) {
            // Below the selected picture.
            self.doc.blocks.insert(i + 1, Block::new(Kind::Para));
            i += 1;
            self.anchor = Pos::new(i, 0);
            self.head = self.anchor;
            return self.insert_blocks_at_text(blocks, i, 0);
        }
        if self.doc.blocks[i].kind.is_code() {
            let text: Vec<String> = blocks.iter().map(Block::text).collect();
            let t = text.join("\n");
            self.insert_plain(&t);
            return;
        }
        let _ = &mut blocks;
        self.insert_blocks_at_text(blocks, i, p.off)
    }

    fn insert_blocks_at_text(&mut self, mut blocks: Vec<Block>, i: usize, off: usize) {
        let plain = |runs: &mut Vec<Run>| for r in runs.iter_mut() { r.marks = Marks::default(); };
        let len = self.blen(i);
        let tail = self.doc.blocks[i].slice(off, len);
        self.doc.blocks[i].replace(off, len, Vec::new());
        // A single text block merges into the line.
        if blocks.len() == 1 && !blocks[0].kind.is_image() && !blocks[0].kind.is_code() {
            let runs = std::mem::take(&mut blocks[0].runs);
            let n: usize = runs.iter().map(|r| r.text.len()).sum();
            let b = &mut self.doc.blocks[i];
            let mut ins = runs;
            b.replace(off, off, std::mem::take(&mut ins));
            b.replace(off + n, off + n, tail);
            self.anchor = Pos::new(i, off + n);
            self.head = self.anchor;
            return;
        }
        let first_text = !blocks[0].kind.is_image() && !blocks[0].kind.is_code();
        let mut at = i;
        if first_text {
            let f = blocks.remove(0);
            let b = &mut self.doc.blocks[i];
            if b.is_empty() && off == 0 && i > 0 && b.kind == Kind::Para { b.kind = f.kind; }
            let mut runs = f.runs;
            if b.kind.is_code() { plain(&mut runs); }
            b.runs.extend(runs);
            normalize(&mut b.runs);
        }
        // Blocks after the first, then the tail joined to the last one.
        let last_text = blocks.last().is_some_and(|b| !b.kind.is_image());
        let mut caret = Pos::new(i, self.blen(i));
        let n = blocks.len();
        for (k, mut blk) in blocks.into_iter().enumerate() {
            let final_text = k + 1 == n && last_text;
            if final_text {
                let l: usize = blk.runs.iter().map(|r| r.text.len()).sum();
                blk.runs.extend(tail.iter().cloned());
                normalize(&mut blk.runs);
                caret = Pos::new(at + 1, l);
            } else if blk.kind.is_image() { caret = Pos::new(at + 1, 0); }
            self.doc.blocks.insert(at + 1, blk);
            at += 1;
        }
        if n > 0 && !last_text {
            self.doc.blocks.insert(at + 1, Block { kind: Kind::Para, runs: tail, indent: 0 });
            caret = Pos::new(at + 1, 0);
        } else if n == 0 {
            let b = &mut self.doc.blocks[i];
            b.runs.extend(tail);
            normalize(&mut b.runs);
        }
        self.doc.fix();
        self.anchor = caret;
        self.head = caret;
    }
}

/// Re-style `a..b` of a block with `f` applied to the marks.
pub fn apply_marks(b: &mut Block, a: usize, z: usize, f: impl Fn(&Marks) -> Marks) {
    let len = b.len();
    let mut mid = b.slice(a, z);
    for r in &mut mid { r.marks = f(&r.marks); }
    b.replace(a, z.min(len), mid);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(c: &mut Core, b: usize, o: usize) { c.set_selection(Pos::new(b, o), Pos::new(b, o)); }
    fn texts(c: &Core) -> Vec<String> { c.doc.blocks.iter().map(Block::text).collect() }

    #[test]
    fn typing_shortcuts_and_undo() {
        let mut c = Core::from_markdown("T\n");
        at(&mut c, 1, 0);
        for ch in "- item".chars() { c.insert_text(&ch.to_string()); }
        assert_eq!(c.doc.blocks[1].kind, Kind::Bullet);
        assert_eq!(c.doc.blocks[1].text(), "item");
        c.enter();
        for ch in "**b**".chars() { c.insert_text(&ch.to_string()); }
        assert_eq!(c.doc.blocks[2].text(), "b");
        assert!(c.doc.blocks[2].runs[0].marks.has(BOLD));
        c.insert_text("x");
        assert_eq!(c.doc.blocks[2].runs.last().unwrap().marks.bits, 0, "typing after a closed **bold** is plain");
        assert_eq!(c.markdown(), "T\n- item\n- **b**x");
        // Undo of the conversion shows the literal text again.
        let mut d = Core::from_markdown("T\n");
        at(&mut d, 1, 0);
        d.insert_text("-");
        d.insert_text(" ");
        assert_eq!(d.doc.blocks[1].kind, Kind::Bullet);
        d.undo();
        assert_eq!(d.doc.blocks[1].kind, Kind::Para);
        assert_eq!(d.doc.blocks[1].text(), "- ");
    }

    #[test]
    fn picture_is_selected_first_then_removed_and_never_glued() {
        let mut c = Core::from_markdown("T\n![](a.png)\ntext");
        at(&mut c, 2, 0);
        c.backspace();
        assert_eq!((c.head, c.doc.blocks.len()), (Pos::new(1, 0), 3), "first Backspace only selects the picture");
        c.backspace();
        assert_eq!(texts(&c), vec!["T", "text"]);
        assert_eq!(c.head, Pos::new(0, 1));
        // Selecting across the picture removes it without joining the paragraphs.
        let mut c = Core::from_markdown("T\nab\n![](a.png)\ncd");
        c.set_selection(Pos::new(1, 2), Pos::new(3, 0));
        c.backspace();
        assert_eq!(c.markdown(), "T\nab\ncd");
        // Typing while a picture is selected starts a paragraph below it.
        let mut c = Core::from_markdown("T\n![](a.png)\n");
        at(&mut c, 1, 0);
        c.insert_text("x");
        assert_eq!(c.markdown(), "T\n![](a.png)\nx\n");
    }

    #[test]
    fn enter_and_backspace_in_lists() {
        let mut c = Core::from_markdown("T\n- a\n- ");
        at(&mut c, 2, 0);
        c.enter();
        assert_eq!(c.doc.blocks[2].kind, Kind::Para, "Enter on an empty item ends the list");
        let mut c = Core::from_markdown("T\n- a");
        at(&mut c, 1, 0);
        c.backspace();
        assert_eq!(c.doc.blocks[1].kind, Kind::Para);
        c.backspace();
        assert_eq!(c.markdown(), "Ta");
    }

    #[test]
    fn code_fence_and_image_line_on_enter() {
        let mut c = Core::from_markdown("T\n");
        at(&mut c, 1, 0);
        c.insert_text("```rs");
        c.enter();
        assert_eq!(c.doc.blocks[1].kind, Kind::Code("rs".into()));
        c.insert_text("x");
        c.enter();
        c.enter();
        assert_eq!(c.markdown(), "T\n```rs\nx\n```\n");
        let mut c = Core::from_markdown("T\n");
        at(&mut c, 1, 0);
        c.insert_text("![](p.png)");
        c.enter();
        assert_eq!(c.doc.blocks[1].kind, Kind::Image("p.png".into()));
    }

    #[test]
    fn copy_and_paste_keep_formatting() {
        let mut c = Core::from_markdown("T\nhello **big** world");
        c.set_selection(Pos::new(1, 6), Pos::new(1, 9));
        assert_eq!(c.selection_markdown(), "**big**");
        let blocks = c.copy_blocks();
        at(&mut c, 0, 1);
        c.insert_blocks(blocks);
        assert_eq!(c.markdown(), "T**big**\nhello **big** world");
    }
}

#[cfg(test)]
mod delete_word_forward_tests {
    use super::*;

    fn at(c: &mut Core, block: usize, off: usize) { c.set_selection(Pos::new(block, off), Pos::new(block, off)); }

    #[test]
    fn deletes_the_next_word_and_keeps_the_caret() {
        let mut c = Core::from_markdown("T\nhello brave world");
        at(&mut c, 1, 0);
        c.delete_word_forward();
        assert_eq!(c.doc.blocks[1].text(), "brave world");
        assert_eq!(c.head, Pos::new(1, 0));
        c.undo();
        assert_eq!(c.doc.blocks[1].text(), "hello brave world");
    }

    #[test]
    fn at_the_end_of_a_block_it_joins_the_next_one() {
        let mut c = Core::from_markdown("T\nab\ncd");
        at(&mut c, 1, 2);
        c.delete_word_forward();
        assert_eq!(c.doc.blocks[1].text(), "abcd");
    }

    #[test]
    fn deletes_a_selection_like_backspace() {
        let mut c = Core::from_markdown("T\nhello world");
        c.set_selection(Pos::new(1, 0), Pos::new(1, 5));
        c.delete_word_forward();
        assert_eq!(c.doc.blocks[1].text(), " world");
    }
}
