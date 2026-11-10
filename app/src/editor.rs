//! The editor view: keyboard, mouse, IME and painting on top of `notedd_editor::Core`.

use crate::layout::{self, Layout, ShapeCache, Theme};
use crate::theme::AppTheme;
use crate::pictures::Pictures;
use gpui::prelude::FluentBuilder;
use gpui::*;
use notedd_editor::model::{Block, Doc, Kind, Pos, BOLD, CODE, HIGHLIGHT, ITALIC, STRIKE};
use notedd_editor::Core;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

actions!(
    editor,
    [
        Backspace, Delete, Enter, CtrlEnter, Left, Right, Up, Down, SelectLeft, SelectRight, SelectUp, SelectDown,
        WordLeft, WordRight, SelectWordLeft, SelectWordRight, Home, End, SelectHome, SelectEnd, DocStart, DocEnd,
        SelectDocStart, SelectDocEnd, SelectAll, Copy, Cut, Paste, Undo, Redo, Bold, Italic, Strike, Highlight, Code,
        DeleteWordBack, DeleteLineBack, Cancel,
    ]
);

pub fn bind_keys(cx: &mut App) {
    let c = Some("Editor");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, c),
        KeyBinding::new("delete", Delete, c),
        KeyBinding::new("enter", Enter, c),
        KeyBinding::new("shift-enter", Enter, c),
        KeyBinding::new("secondary-enter", CtrlEnter, c),
        KeyBinding::new("left", Left, c),
        KeyBinding::new("right", Right, c),
        KeyBinding::new("up", Up, c),
        KeyBinding::new("down", Down, c),
        KeyBinding::new("shift-left", SelectLeft, c),
        KeyBinding::new("shift-right", SelectRight, c),
        KeyBinding::new("shift-up", SelectUp, c),
        KeyBinding::new("shift-down", SelectDown, c),
        KeyBinding::new("ctrl-left", WordLeft, c),
        KeyBinding::new("ctrl-right", WordRight, c),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, c),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, c),
        KeyBinding::new("home", Home, c),
        KeyBinding::new("end", End, c),
        KeyBinding::new("shift-home", SelectHome, c),
        KeyBinding::new("shift-end", SelectEnd, c),
        KeyBinding::new("ctrl-home", DocStart, c),
        KeyBinding::new("ctrl-end", DocEnd, c),
        KeyBinding::new("ctrl-shift-home", SelectDocStart, c),
        KeyBinding::new("ctrl-shift-end", SelectDocEnd, c),
        KeyBinding::new("secondary-a", SelectAll, c),
        KeyBinding::new("secondary-c", Copy, c),
        KeyBinding::new("ctrl-insert", Copy, c),
        KeyBinding::new("secondary-x", Cut, c),
        KeyBinding::new("shift-delete", Cut, c),
        KeyBinding::new("secondary-v", Paste, c),
        KeyBinding::new("shift-insert", Paste, c),
        KeyBinding::new("secondary-z", Undo, c),
        KeyBinding::new("ctrl-y", Redo, c),
        KeyBinding::new("secondary-shift-z", Redo, c),
        KeyBinding::new("secondary-b", Bold, c),
        KeyBinding::new("secondary-i", Italic, c),
        KeyBinding::new("secondary-shift-s", Strike, c),
        KeyBinding::new("secondary-shift-x", Strike, c),
        KeyBinding::new("secondary-e", Code, c),
        KeyBinding::new("ctrl-backspace", DeleteWordBack, c),
        KeyBinding::new("escape", Cancel, c),
    ]);
    // the Mac moves by word with Option and to the line or document edge with Cmd
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("alt-left", WordLeft, c),
        KeyBinding::new("alt-right", WordRight, c),
        KeyBinding::new("alt-shift-left", SelectWordLeft, c),
        KeyBinding::new("alt-shift-right", SelectWordRight, c),
        KeyBinding::new("cmd-left", Home, c),
        KeyBinding::new("cmd-right", End, c),
        KeyBinding::new("cmd-shift-left", SelectHome, c),
        KeyBinding::new("cmd-shift-right", SelectEnd, c),
        KeyBinding::new("cmd-up", DocStart, c),
        KeyBinding::new("cmd-down", DocEnd, c),
        KeyBinding::new("cmd-shift-up", SelectDocStart, c),
        KeyBinding::new("cmd-shift-down", SelectDocEnd, c),
        KeyBinding::new("alt-backspace", DeleteWordBack, c),
        KeyBinding::new("cmd-backspace", DeleteLineBack, c),
    ]);
}

/// With NOTEDD_DEBUG set, key presses and paste results go to notedd-debug.txt in the temp folder.
pub fn debug_log(line: &str) {
    if std::env::var_os("NOTEDD_DEBUG").is_none() { return; }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("notedd-debug.txt")) { let _ = writeln!(f, "{line}"); }
}

fn al_(mut c: Hsla, o: f32) -> Hsla { c.a *= o; c }

fn floor(text: &str, mut byte: usize) -> usize {
    byte = byte.min(text.len());
    while !text.is_char_boundary(byte) { byte -= 1; }
    byte
}
fn to16(text: &str, byte: usize) -> usize { text[..floor(text, byte)].encode_utf16().count() }
fn from16(text: &str, u: usize) -> usize {
    let mut n = 0;
    for (i, c) in text.char_indices() {
        if n >= u { return i; }
        n += c.len_utf16();
    }
    text.len()
}

/// The link target written around byte `off` of a Markdown line: `[text](url)`, `![](path)`, `<url>` or a bare URL.
fn raw_url_at(text: &str, off: usize) -> Option<String> {
    let off = floor(text, off);
    let start = text[..off].rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let end = text[off..].find(char::is_whitespace).map_or(text.len(), |i| off + i);
    let word = &text[start..end];
    if let Some(i) = word.find("](") {
        let rest = &word[i + 2..];
        let url = rest.split(|c| c == ')' || c == ' ').next()?;
        return (!url.is_empty()).then(|| url.to_string());
    }
    let url = word.trim_matches(|c: char| matches!(c, '<' | '>' | '(' | ')' | '[' | ']' | '"' | '\'' | ',' | '.' | ';' | '!' | '?'));
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "www.", "mailto:"].iter().any(|p| lower.starts_with(p)).then(|| url.to_string())
}

pub struct Editor {
    core: Core,
    focus: FocusHandle,
    scroll: ScrollHandle,
    cache: ShapeCache,
    pics: Pictures,
    layout: Rc<Layout>,
    /// Origin of the document in window coordinates (captured while painting).
    origin: Point<Pixels>,
    goal_x: Option<f32>,
    marked: Option<(usize, Range<usize>)>,
    selecting: bool,
    /// dragging a picture's corner: block, pointer x, width and height at the start, changed yet
    img_drag: Option<(usize, f32, f32, f32, bool)>,
    hover_img: Option<usize>,
    reveal: bool,
    caret_visible: bool,
    blink: Option<Task<()>>,
    pub single: bool,
    /// Markdown mode: the note's source, one plain paragraph per line, markup shown as typed.
    pub raw: bool,
    pub app: AppTheme,
    pub scale: f32,
    pub pad: f32,
    pub pad_top: f32,
    /// the width the shell gives this editor, so a resize does not wait for last frame's bounds
    pub avail: Option<f32>,
    pub find: String,
    pub placeholder: SharedString,
    pub autofocus: bool,
}

#[derive(Clone, Debug)]
pub enum EditorEvent { Changed, Submit, Cancel, Nav(i32), UndoEmpty }
impl EventEmitter<EditorEvent> for Editor {}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle { self.focus.clone() }
}

pub(crate) struct PaintData {
    layout: Rc<Layout>,
    th: Theme,
    sel: (Pos, Pos),
    collapsed: bool,
    head: Pos,
    caret: bool,
    /// the editor has the keyboard (the caret may be in its off phase)
    focused: bool,
    markers: Vec<Option<String>>,
    kinds: Vec<Kind>,
    width: f32,
    scale: f32,
    finds: Vec<(usize, usize, usize)>,
    hover_img: Option<usize>,
    /// the find match the selection sits on: its index in `finds` and its text
    cur: Option<(usize, String)>,
    placeholder: Option<ShapedLine>,
    /// blocks starting below this (document y) are not drawn: they are out of sight
    vis_bottom: f32,
}

impl Editor {
    pub fn new(single: bool, app: AppTheme, cx: &mut Context<Self>) -> Editor {
        Editor {
            core: if single { Core::new(Doc { blocks: vec![Block::para("")] }) } else { Core::from_markdown("") },
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            cache: ShapeCache::default(),
            pics: Pictures::new(PathBuf::new()),
            layout: Rc::new(Layout::default()),
            origin: Point::default(),
            goal_x: None,
            marked: None,
            selecting: false, img_drag: None, hover_img: None,
            reveal: false,
            caret_visible: true,
            blink: None,
            single,
            raw: false,
            app,
            scale: 1.,
            pad: 0.,
            pad_top: 0.,
            avail: None,
            find: String::new(),
            placeholder: SharedString::default(),
            autofocus: false,
        }
    }

    pub fn set_markdown(&mut self, src: &str, dir: PathBuf) {
        self.core = if self.single { Core::new(Doc { blocks: vec![Block::para(src)] }) }
            else if self.raw { Core::new(Doc { blocks: src.replace("\r\n", "\n").split('\n').map(Block::para).collect() }) }
            else { Core::from_markdown(src) };
        self.pics = Pictures::new(dir);
        self.marked = None;
        self.goal_x = None;
        self.scroll.set_offset(point(px(0.), px(0.)));
    }
    pub fn markdown(&self) -> String {
        if self.single { self.core.doc.blocks[0].text() }
        else if self.raw { self.core.doc.blocks.iter().map(|b| b.text()).collect::<Vec<_>>().join("\n") }
        else { self.core.markdown() }
    }

    /// Text typed or pasted in Markdown mode: no shortcuts, newlines split lines.
    fn type_raw(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        for (k, seg) in text.split('\n').enumerate() {
            if k > 0 { self.core.enter(); }
            self.core.insert_raw(seg);
        }
    }

    /// Ctrl+B and friends in Markdown mode: the selection gets wrapped in the markup.
    fn wrap_raw(&mut self, d: &str) {
        let (s, e) = self.core.sel();
        if s.block != e.block { return; }
        let t = self.core.doc.blocks[s.block].text();
        let inner = &t[s.off..e.off];
        let undo = inner.len() >= 2 * d.len() && inner.starts_with(d) && inner.ends_with(d);
        let new = if undo { inner[d.len()..inner.len() - d.len()].to_string() } else { format!("{d}{inner}{d}") };
        self.core.insert_raw(&new);
        let b = s.block;
        if s == e { self.core.set_selection(Pos::new(b, s.off + d.len()), Pos::new(b, s.off + d.len())); }
        else { self.core.set_selection(Pos::new(b, s.off), Pos::new(b, s.off + new.len())); }
    }

    fn mark(&mut self, bit: u8, window: &mut Window, cx: &mut Context<Self>) {
        if self.single { return; }
        if self.raw {
            let d = match bit { BOLD => "**", ITALIC => "*", STRIKE => "~~", HIGHLIGHT => "==", _ => "`" };
            self.wrap_raw(d);
        } else { self.core.toggle_mark(bit); }
        self.changed(window, cx);
    }

    /// Ctrl+Enter in Markdown mode: the line's task box ticks or unticks.
    fn toggle_task_raw(&mut self) {
        let b = self.core.head.block;
        let t = self.core.doc.blocks[b].text();
        let lead = t.len() - t.trim_start().len();
        let rest = &t[lead..];
        let (from, to) = if rest.starts_with("- [ ] ") { ("- [ ] ", "- [x] ") }
            else if rest.starts_with("- [x] ") || rest.starts_with("- [X] ") { ("- [x] ", "- [ ] ") }
            else { return };
        let head = self.core.head;
        self.core.set_selection(Pos::new(b, lead), Pos::new(b, lead + from.len()));
        self.core.insert_raw(to);
        self.core.set_selection(head, head);
    }

    /// The link (or picture) under a document position, ready to open.
    fn link_at(&self, p: Pos) -> Option<String> {
        let b = self.core.doc.blocks.get(p.block)?;
        let link: String = if let Kind::Image(path) = &b.kind {
            notedd_editor::model::image_parts(path).0.to_string()
        } else if self.raw {
            raw_url_at(&b.text(), p.off)?
        } else {
            let m = b.marks_of(p.off).filter(|m| m.link.is_some()).or_else(|| p.off.checked_sub(1).and_then(|o| b.marks_of(o)))?;
            m.link.as_deref()?.to_string()
        };
        let lower = link.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") { return Some(link); }
        if lower.starts_with("www.") { return Some(format!("https://{link}")); }
        self.pics.resolve(&link).map(|f| f.to_string_lossy().to_string())
    }
    pub fn text(&self) -> String { self.core.doc.blocks[0].text() }
    pub fn is_empty(&self) -> bool { self.core.doc.blocks.iter().all(|b| b.is_empty() && !b.kind.is_image()) }
    pub fn focus(&self, window: &mut Window) { window.focus(&self.focus); }
    pub fn select_all_text(&mut self, window: &mut Window, cx: &mut Context<Self>) { self.core.select_all(); self.moved(window, cx); }
    pub fn go_to_end(&mut self, window: &mut Window, cx: &mut Context<Self>) { let e = self.core.doc.end(); self.core.set_selection(e, e); self.moved(window, cx); }
    pub fn is_focused(&self, window: &Window) -> bool { self.focus.is_focused(window) }
    pub fn request_focus(&mut self, cx: &mut Context<Self>) { self.autofocus = true; cx.notify(); }
    pub fn goto_start(&mut self) { self.core.set_selection(Pos::new(0, 0), Pos::new(0, 0)); }
    pub fn core(&self) -> &Core { &self.core }
    pub fn word_count(&self) -> usize { self.core.doc.blocks.iter().map(|b| b.text().split_whitespace().count()).sum() }
    pub fn do_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) { self.marked = None; self.core.undo(); self.changed(window, cx); }
    pub fn do_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) { self.marked = None; self.core.redo(); self.changed(window, cx); }
    pub fn do_mark(&mut self, bit: u8, window: &mut Window, cx: &mut Context<Self>) { self.core.toggle_mark(bit); self.changed(window, cx); }
    pub fn insert(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) { self.core.insert_raw(text); self.changed(window, cx); }

    /// Matches of the find text: (block, start, end), in document order.
    pub fn matches(&self) -> Vec<(usize, usize, usize)> {
        let q = self.find.to_lowercase();
        let mut out = Vec::new();
        if q.is_empty() { return out; }
        for (i, b) in self.core.doc.blocks.iter().enumerate() {
            if b.kind.is_image() { continue; }
            let t = b.text();
            let l = t.to_lowercase();
            if l.len() != t.len() { continue; }
            let mut from = 0;
            while let Some(k) = l[from..].find(&q) {
                let a = from + k;
                out.push((i, a, a + q.len()));
                from = a + q.len().max(1);
            }
        }
        out
    }

    /// Select the next (or previous) match after the caret.
    pub fn find_step(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.matches();
        if m.is_empty() { return; }
        let (s, e) = self.core.sel();
        let hit = if back {
            m.iter().rev().find(|(b, a, _)| Pos::new(*b, *a) < s).or(m.last())
        } else {
            m.iter().find(|(b, a, _)| (*b, *a) >= (e.block, e.off)).or(m.first())
        };
        if let Some(&(b, a, z)) = hit { self.core.set_selection(Pos::new(b, a), Pos::new(b, z)); self.moved(window, cx); }
    }

    // ---- bookkeeping ------------------------------------------------------------

    fn restart_blink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.caret_visible = true;
        self.blink = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(530)).await;
                let ok = this.update_in(cx, |e, window, cx| {
                    if e.focus.is_focused(window) { e.caret_visible = !e.caret_visible; cx.notify(); }
                });
                if ok.is_err() { break; }
            }
        }));
    }

    /// The document changed: repaint, keep the caret visible, save soon.
    fn changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.goal_x = None;
        self.moved(window, cx);
        cx.emit(EditorEvent::Changed);
    }

    /// Only the selection changed.
    fn moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reveal = true;
        self.restart_blink(window, cx);
        cx.notify();
    }

    fn head_text(&self) -> String { self.core.doc.blocks[self.core.head.block].text() }

    fn caret_xy(&self) -> (f32, f32, f32) { self.layout.point_of(self.core.head) }

    /// The layout of the last frame still matches the document (events can arrive between a change and the next frame).
    fn fresh(&self) -> bool { self.layout.blocks.len() == self.core.doc.blocks.len() }

    fn to_doc(&self, p: Point<Pixels>) -> (f32, f32) { (f32::from(p.x - self.origin.x), f32::from(p.y - self.origin.y)) }

    // ---- movement ---------------------------------------------------------------

    fn set_head(&mut self, p: Pos, extend: bool, keep_goal: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !keep_goal { self.goal_x = None; }
        self.core.move_to(p, extend);
        self.moved(window, cx);
    }

    fn horizontal(&mut self, back: bool, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (s, e) = self.core.sel();
        let p = if !extend && !self.core.collapsed() { if back { s } else { e } } else if back { self.core.prev_pos(self.core.head) } else { self.core.next_pos(self.core.head) };
        self.set_head(p, extend, false, window, cx);
    }

    fn word(&mut self, back: bool, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let p = if back { self.core.word_left(self.core.head) } else { self.core.word_right(self.core.head) };
        self.set_head(p, extend, false, window, cx);
    }

    fn vertical(&mut self, up: bool, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.single { cx.emit(EditorEvent::Nav(if up { -1 } else { 1 })); return; }
        if !self.fresh() { return; }
        let head = self.core.head;
        let (x, y, lh) = self.caret_xy();
        let gx = *self.goal_x.get_or_insert(x);
        let ty = if up { y - lh * 0.5 } else { y + lh * 1.5 };
        let mut p = self.layout.pos_at(gx, ty);
        if up && ty < 0. { p = Pos::new(0, 0); }
        if !up && p == head && ty > y { p = self.core.doc.end(); }
        self.set_head(p, extend, true, window, cx);
    }

    fn line_edge(&mut self, end: bool, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.fresh() { return; }
        let (_, y, lh) = self.caret_xy();
        let p = self.layout.pos_at(if end { 1e6 } else { -1e6 }, y + lh * 0.5);
        let p = if p.block != self.core.head.block { self.core.head } else { p };
        self.set_head(p, extend, false, window, cx);
    }

    fn select_word_at(&mut self, p: Pos, window: &mut Window, cx: &mut Context<Self>) {
        let b = &self.core.doc.blocks[p.block];
        if b.kind.is_image() { return; }
        let text = b.text();
        let (mut a, mut z) = (0, text.len());
        for (i, w) in text.split_word_bound_indices() {
            if p.off >= i && p.off <= i + w.len() { a = i; z = i + w.len(); if p.off < i + w.len() || w.chars().any(char::is_alphanumeric) { break; } }
        }
        self.core.set_selection(Pos::new(p.block, a), Pos::new(p.block, z));
        self.moved(window, cx);
    }

    // ---- mouse ------------------------------------------------------------------

    fn on_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        if !self.fresh() { return; }
        let (x, y) = self.to_doc(ev.position);
        let p = self.layout.pos_at(x, y);
        for (i, bb) in self.layout.blocks.iter().enumerate() {
            if let Some((_, w, h)) = &bb.img {
                if x >= w - 24. && x <= *w && y >= bb.y + h - 24. && y <= bb.y + h {
                    self.img_drag = Some((i, f32::from(ev.position.x), *w, *h, false));
                    return;
                }
            }
        }
        // Ctrl+click opens a link or a picture
        if ev.modifiers.control && ev.click_count == 1 && !self.single {
            if let Some(url) = self.link_at(p) { cx.open_url(&url); return; }
        }
        let b = &self.layout.blocks[p.block];
        if let Kind::Todo(_) = self.core.doc.blocks[p.block].kind {
            if x >= b.x - 26. && x < b.x - 4. && y >= b.y && y < b.y + b.m.lh {
                self.core.toggle_todo(p.block);
                self.changed(window, cx);
                return;
            }
        }
        self.marked = None;
        match ev.click_count {
            1 => {
                self.selecting = true;
                if ev.modifiers.shift { self.set_head(p, true, false, window, cx); }
                else { self.core.set_selection(p, p); self.goal_x = None; self.moved(window, cx); }
            }
            2 => self.select_word_at(p, window, cx),
            _ => {
                let len = self.core.doc.blocks[p.block].len();
                self.core.set_selection(Pos::new(p.block, 0), Pos::new(p.block, len));
                self.moved(window, cx);
            }
        }
    }

    fn on_move(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((i, x0, w0, h0, moved)) = self.img_drag {
            if ev.pressed_button != Some(MouseButton::Left) { self.img_drag = None; return; }
            let max = (self.layout.width).max(120.).min(w0 * 450. / h0.max(1.));
            let nw = (w0 + f32::from(ev.position.x) - x0).clamp(60., max.max(60.));
            self.core.set_image_width(i, Some(nw), !moved);
            self.img_drag = Some((i, x0, w0, h0, true));
            self.changed(window, cx);
            return;
        }
        // a release outside the note never reached on_up: no button down means no drag-select
        if ev.pressed_button != Some(MouseButton::Left) { self.selecting = false; }
        if self.fresh() && !self.selecting {
            let (x, y) = self.to_doc(ev.position);
            let hv = self.layout.blocks.iter().position(|b| b.img.as_ref().is_some_and(|(_, w, h)| x >= 0. && x <= *w && y >= b.y && y <= b.y + *h));
            if hv != self.hover_img { self.hover_img = hv; cx.notify(); }
        }
        if !self.selecting || !self.fresh() || ev.pressed_button != Some(MouseButton::Left) { return; }
        let (x, y) = self.to_doc(ev.position);
        let p = self.layout.pos_at(x, y);
        if p != self.core.head { self.set_head(p, true, false, window, cx); }
    }

    fn on_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) { self.selecting = false; self.img_drag = None; }

    // ---- clipboard --------------------------------------------------------------

    fn selection_text(&self) -> String {
        if self.raw {
            // the source as it stands: no escaping
            let (s, e) = self.core.sel();
            return (s.block..=e.block).map(|i| {
                let t = self.core.doc.blocks[i].text();
                let a = if i == s.block { s.off } else { 0 };
                let z = if i == e.block { e.off } else { t.len() };
                t[a.min(z)..z].to_string()
            }).collect::<Vec<_>>().join("\n");
        }
        let h = self.core.head;
        if self.core.collapsed() {
            if let Kind::Image(p) = &self.core.doc.blocks[h.block].kind { { let (path, w) = notedd_editor::model::image_parts(p); return match w { Some(w) => format!("![]({path} \"notedd-width={}\")", w.round() as i32), None => format!("![]({path})") }; } }
            return String::new();
        }
        self.core.selection_markdown()
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let t = self.selection_text();
        if !t.is_empty() { cx.write_to_clipboard(ClipboardItem::new_string(t)); }
    }
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        let t = self.selection_text();
        if t.is_empty() { return; }
        cx.write_to_clipboard(ClipboardItem::new_string(t));
        if self.core.collapsed() { self.core.backspace(); } else { self.core.delete_selection(); }
        self.changed(window, cx);
    }
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else { debug_log("paste: clipboard unreadable"); return };
        debug_log(&format!("paste: {} entries", item.entries().len()));
        self.marked = None;
        if self.single {
            if let Some(t) = item.text() { self.core.insert_raw(&t.replace(['\r', '\n'], " ")); self.changed(window, cx); }
            return;
        }
        // Copied files and screenshots become pictures of the note.
        let mut links: Vec<String> = Vec::new();
        for entry in item.entries() {
            if let ClipboardEntry::Image(img) = entry {
                let ext = match img.format { ImageFormat::Jpeg => "jpg", ImageFormat::Gif => "gif", ImageFormat::Webp => "webp", _ => "png" };
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                if let Some(l) = self.pics.import_bytes(&format!("image-{stamp}"), ext, &img.bytes) { links.push(l); }
            }
        }
        let text = item.text();
        if links.is_empty() {
            if let Some(t) = &text {
                let files: Vec<_> = t.lines().filter(|l| !l.trim().is_empty()).map(|l| PathBuf::from(l.trim())).collect();
                if !files.is_empty() {
                    links = files.iter().filter_map(|f| self.pics.import_file(f)).collect();
                    if links.len() != files.len() { links.clear(); }
                }
            }
        }
        if self.raw {
            let t = if !links.is_empty() { links.iter().map(|l| format!("![]({l})")).collect::<Vec<_>>().join("\n") } else if let Some(t) = text { t } else { return };
            self.type_raw(&t);
        } else if !links.is_empty() {
            for l in &links { self.core.insert_image(l); }
        } else if let Some(t) = text {
            self.core.paste_markdown(&t);
        } else { return; }
        self.changed(window, cx);
    }

    fn dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        if self.single { return; }
        let mut any = false;
        for p in paths.paths() {
            let Some(l) = self.pics.import_file(p) else { continue };
            if self.raw { self.type_raw(&format!("![]({l})\n")); } else { self.core.insert_image(&l); }
            any = true;
        }
        if any { self.changed(window, cx); }
    }

    // ---- painting ---------------------------------------------------------------

    pub(crate) fn paint_doc(d: &PaintData, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let o = bounds.origin;
        let th = &d.th;
        let at = |x: f32, y: f32| point(o.x + px(x), o.y + px(y));
        let (s, e) = d.sel;
        // Backgrounds, selection, markers.
        for (i, b) in d.layout.blocks.iter().enumerate() {
            if b.y > d.vis_bottom { break; }
            let kind = &d.kinds[i];
            let full = |x: f32, w: f32| Bounds::new(at(x, b.y), size(px(w), px(b.h)));
            let pic_w = b.img.as_ref().map_or(d.width, |p| p.1);
            match kind {
                Kind::Code(_) => fill_r(window, full(0., d.width), 6., th.code_bg),
                Kind::Quote => fill_r(window, full(4., 3.), 1.5, th.line),
                _ => {}
            }
            if i >= s.block && i <= e.block && !d.collapsed {
                match kind {
                    Kind::Image(_) => {
                        // Selected when the range reaches past it.
                        if (i > s.block || s.off == 0) && i < e.block {
                            fill_r(window, full(0., pic_w), 6., th.selection);
                        }
                    }
                    _ => {
                        let len = b.lines.last().map_or(0, |l| l.start + l.shaped.len());
                        let a = if i == s.block { s.off } else { 0 };
                        let z = if i == e.block { e.off } else { len };
                        for (x, y, w, h) in d.layout.rects(i, a, z, i < e.block) {
                            let bh = (b.m.size * 1.3).min(h);
                            fill_r(window, Bounds::new(at(x, y + (h - bh) / 2.), size(px(w), px(bh))), 0., th.selection);
                        }
                    }
                }
            }
            if d.collapsed && d.head.block == i && kind.is_image() && d.focused {
                ring_r(window, full(0., pic_w), 6., 2., th.accent);
            }
            if let Kind::Image(path) = kind {
                let path = &notedd_editor::model::image_parts(path).0.to_string();
                match &b.img {
                    Some((p, w, h)) => {
                        let r = Bounds::new(at(0., b.y), size(px(*w), px(*h)));
                        let _ = window.paint_image(r, Corners::all(px(6.)), p.image.clone(), 0, false);
                        // the grip shows on the picked picture (steady, not with the caret's blink) and under the pointer
                        if (d.collapsed && d.head.block == i && d.focused) || d.hover_img == Some(i) {
                            let hd = Bounds::new(at(*w - 21., b.y + *h - 21.), size(px(18.), px(18.)));
                            window.paint_quad(fill(hd, al_(th.dark, 0.78)).corner_radii(px(5.)));
                            let mut pb = PathBuilder::stroke(px(1.4));
                            let q = |x: f32, y: f32| at(*w - 21. + x, b.y + *h - 21. + y);
                            pb.move_to(q(5., 5.)); pb.line_to(q(13., 13.));
                            pb.move_to(q(5., 9.)); pb.line_to(q(5., 5.)); pb.line_to(q(9., 5.));
                            pb.move_to(q(9., 13.)); pb.line_to(q(13., 13.)); pb.line_to(q(13., 9.));
                            if let Ok(pp) = pb.build() { window.paint_path(pp, th.on_dark); }
                        }
                    }
                    None => {
                        let r = Bounds::new(at(0., b.y + 2.), size(px(d.width), px(b.h - 4.)));
                        ring_r(window, r, 6., 1., th.line);
                        let run = TextRun { len: path.len(), font: layout::font(layout::BODY, false, false), color: th.dim, background_color: None, underline: None, strikethrough: None };
                        let line = window.text_system().shape_line(path.clone().into(), px(14.), &[run], None);
                        let _ = line.paint(at(14., b.y + b.h / 2. - 10.), px(20.), window, cx);
                    }
                }
            }
            // List markers.
            let mx = b.x - 21. * d.scale;
            let my = b.y + b.top;
            match (&d.markers[i], kind) {
                (Some(t), _) => {
                    let run = TextRun { len: t.len(), font: layout::font(layout::BODY, false, false), color: th.dim, background_color: None, underline: None, strikethrough: None };
                    let line = window.text_system().shape_line(t.clone().into(), px(b.m.size), &[run], None);
                    let _ = line.paint(at(mx, my), px(b.m.lh), window, cx);
                }
                (None, Kind::Todo(done)) => {
                    let sz = 15. * d.scale;
                    let (x0, y0) = (f32::from(o.x) + mx, f32::from(o.y) + my + (b.m.lh - sz) / 2.);
                    let k = d.scale;
                    if *done {
                        fill_r(window, Bounds::new(point(px(x0), px(y0)), size(px(sz), px(sz))), 4. * k, th.dark);
                        let mut pb = PathBuilder::stroke(px(1.7 * k));
                        let q = |fx: f32, fy: f32| turn(point(px(x0 + sz * fx), px(y0 + sz * fy)));
                        pb.move_to(q(0.26, 0.52));
                        pb.line_to(q(0.43, 0.69));
                        pb.line_to(q(0.75, 0.33));
                        if let Ok(p) = pb.build() { window.paint_path(p, th.on_dark); }
                    } else {
                        ring_r(window, Bounds::new(point(px(x0), px(y0)), size(px(sz), px(sz))), 3.5 * k + 0.75 * k, 1.5 * k, th.ink3);
                    }
                }
                _ => {}
            }
        }
        for (k, &(bi, a, z)) in d.finds.iter().enumerate() {
            if bi < d.layout.blocks.len() && !d.layout.blocks[bi].lines.is_empty() {
                // the current hit is painted over the text further down
                if d.cur.as_ref().is_some_and(|c| c.0 == k) { continue; }
                for (x, y, w, h) in d.layout.rects(bi, a, z, false) {
                    let bh = (d.layout.blocks[bi].m.size * 1.3).min(h);
                    let r = Bounds::new(at(x - 1.5, y + (h - bh) / 2.), size(px(w + 3.), px(bh)));
                    fill_r(window, r, 3., th.find);
                }
            }
        }
        // Text.
        for b in d.layout.blocks.iter() {
            if b.y > d.vis_bottom { break; }
            for l in &b.lines {
                let origin = at(b.x, b.y + b.top + l.y);
                let lh = px(b.m.lh);
                let _ = l.shaped.paint_background(origin, lh, TextAlign::Left, None, window, cx);
                let _ = l.shaped.paint(origin, lh, TextAlign::Left, None, window, cx);
            }
        }
        if let Some((k, t)) = &d.cur {
            if let Some(&(bi, a, z)) = d.finds.get(*k) {
                if bi < d.layout.blocks.len() && !d.layout.blocks[bi].lines.is_empty() && z <= t.len() + a {
                    let bb = &d.layout.blocks[bi];
                    let rs = d.layout.rects(bi, a, z, false);
                    // a solid band over the text, so the dark-on-light glyphs underneath do not show
                    // through as a second, offset copy of the word
                    // (a hit wrapped over two lines gets no light copy: a see-through band keeps it readable)
                    let col = if rs.len() == 1 { th.dark } else { al_(th.dark, 0.3) };
                    for &(x, y, w, h) in &rs {
                        let bh = (bb.m.size * 1.3).min(h);
                        let r = Bounds::new(at(x - 1.5, y + (h - bh) / 2.), size(px(w + 3.), px(bh)));
                        fill_r(window, r, 3., col);
                    }
                    if rs.len() == 1 {
                        let run = TextRun { len: t.len(), font: layout::font(if bb.m.mono { layout::MONO } else { layout::BODY }, bb.m.bold, bb.m.italic), color: th.on_dark, background_color: None, underline: None, strikethrough: None };
                        let line = window.text_system().shape_line(t.clone().into(), px(bb.m.size), &[run], None);
                        let _ = line.paint(at(rs[0].0, rs[0].1), px(bb.m.lh), window, cx);
                    }
                }
            }
        }
        if let Some(pl) = &d.placeholder {
            let b = &d.layout.blocks[0];
            let _ = pl.paint(at(b.x, b.y + b.top), px(b.m.lh), window, cx);
        }
        // Caret, painted in the same pass as the text.
        if d.caret && d.collapsed && !d.kinds[d.head.block].is_image() {
            let (x, y, lh) = d.layout.point_of(d.head);
            window.paint_quad(fill(Bounds::new(at(x, y + 3.), size(px(1.5), px(lh - 6.))), th.ink));
        }
    }
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.autofocus {
            self.autofocus = false;
            window.focus(&self.focus);
            self.restart_blink(window, cx);
        }
        let th = Theme::from_app(&self.app);
        let salt = self.app.salt();
        let bw = f32::from(self.scroll.bounds().size.width);
        let avail = self.avail.unwrap_or(if bw > 0. { bw } else { f32::from(window.viewport_size().width) });
        let width = (avail - 2. * self.pad).max(120.);
        self.layout = Rc::new(self.cache.layout(&self.core.doc, width, &th, salt, self.scale, self.single, &mut self.pics, window));
        // Keep the caret in view after an edit or a caret move.
        if self.reveal {
            self.reveal = false;
            let vb = self.scroll.bounds();
            let vh = f32::from(vb.size.height);
            if vh > 0. {
                let (_, y, lh) = self.layout.point_of(self.core.head);
                let off = f32::from(self.scroll.offset().y);
                let (top, bottom) = (-off, -off + vh);
                let m = 40.;
                let new = if y - m < top { Some(-(y - m).max(0.)) } else if y + lh + m > bottom { Some(-(y + lh + m - vh)) } else { None };
                if let Some(n) = new { self.scroll.set_offset(point(px(0.), px(n.min(0.)))); }
            }
        }
        let mut n = 0;
        let markers: Vec<Option<String>> = self.core.doc.blocks.iter().map(|b| match b.kind {
            Kind::Bullet => { n = 0; Some("•".to_string()) }
            Kind::Number => { n += 1; Some(format!("{n}.")) }
            _ => { n = 0; None }
        }).collect();
        let finds = self.matches();
        let cur = {
            let (s, z) = self.core.sel();
            finds.iter().position(|&(b, a, e)| s.block == b && z.block == b && s.off == a && z.off == e && !self.core.collapsed())
                .map(|k| (k, self.core.doc.blocks[finds[k].0].text()[finds[k].1..finds[k].2].to_string()))
        };
        let data = PaintData {
            layout: self.layout.clone(),
            th,
            sel: self.core.sel(),
            collapsed: self.core.collapsed(),
            head: self.core.head,
            caret: self.focus.is_focused(window) && self.caret_visible,
            focused: self.focus.is_focused(window),
            markers,
            kinds: self.core.doc.blocks.iter().map(|b| b.kind.clone()).collect(),
            width,
            scale: self.scale,
            finds: finds.clone(),
            hover_img: self.hover_img,
            cur,
            placeholder: if self.is_empty() && !self.placeholder.is_empty() && self.marked.is_none() {
                let run = TextRun { len: self.placeholder.len(), font: layout::font(layout::BODY, false, false), color: th.dim, background_color: None, underline: None, strikethrough: None };
                let m = self.layout.blocks[0].m;
                Some(window.text_system().shape_line(self.placeholder.clone(), px(m.size), &[run], None))
            } else { None },
            vis_bottom: f32::MAX,
        };
        let entity = cx.entity();
        let focus = self.focus.clone();
        let pad_top = self.pad_top;
        let single = self.single;
        let height = (if single { self.layout.blocks[0].h } else { self.layout.height }) + pad_top;
        let pics: Vec<(f32, f32, f32)> = self.layout.blocks.iter().filter_map(|b| b.img.as_ref().map(|(_, w, h)| (b.y, *w, *h))).collect();
        let sizing_pic = self.img_drag.is_some();
        let l = |f: fn(&mut Self, &mut Window, &mut Context<Self>)| move |this: &mut Self, _: &dyn Action, w: &mut Window, cx: &mut Context<Self>| f(this, w, cx);
        let _ = l;
        div()
            .id("editor")
            .size_full()
            .key_context("Editor")
            .track_focus(&self.focus)
                        .on_action(cx.listener(|e, _: &Backspace, w, cx| { e.marked = None; e.core.backspace(); e.changed(w, cx); }))
            .on_action(cx.listener(|e, _: &Delete, w, cx| { e.core.delete_forward(); e.changed(w, cx); }))
            .on_action(cx.listener(|e, _: &DeleteWordBack, w, cx| { e.core.delete_word_back(); e.changed(w, cx); }))
            .on_action(cx.listener(|e, _: &DeleteLineBack, w, cx| {
                if e.core.collapsed() {
                    if !e.fresh() { return; }
                    e.line_edge(false, true, w, cx);
                }
                e.marked = None;
                if !e.core.delete_selection() { e.core.backspace(); }
                e.changed(w, cx);
            }))
            .on_action(cx.listener(|e, _: &Enter, w, cx| { if e.single { cx.emit(EditorEvent::Submit); return; } e.marked = None; e.core.enter(); e.changed(w, cx); }))
            .on_action(cx.listener(|_, _: &Cancel, _, cx| cx.emit(EditorEvent::Cancel)))
            .on_action(cx.listener(|e, _: &CtrlEnter, w, cx| {
                if e.single { cx.emit(EditorEvent::Submit); return; }
                if e.raw { e.toggle_task_raw(); } else { e.core.ctrl_enter(); }
                e.changed(w, cx);
            }))
            .on_action(cx.listener(|e, _: &Left, w, cx| e.horizontal(true, false, w, cx)))
            .on_action(cx.listener(|e, _: &Right, w, cx| e.horizontal(false, false, w, cx)))
            .on_action(cx.listener(|e, _: &SelectLeft, w, cx| e.horizontal(true, true, w, cx)))
            .on_action(cx.listener(|e, _: &SelectRight, w, cx| e.horizontal(false, true, w, cx)))
            .on_action(cx.listener(|e, _: &Up, w, cx| e.vertical(true, false, w, cx)))
            .on_action(cx.listener(|e, _: &Down, w, cx| e.vertical(false, false, w, cx)))
            .on_action(cx.listener(|e, _: &SelectUp, w, cx| e.vertical(true, true, w, cx)))
            .on_action(cx.listener(|e, _: &SelectDown, w, cx| e.vertical(false, true, w, cx)))
            .on_action(cx.listener(|e, _: &WordLeft, w, cx| e.word(true, false, w, cx)))
            .on_action(cx.listener(|e, _: &WordRight, w, cx| e.word(false, false, w, cx)))
            .on_action(cx.listener(|e, _: &SelectWordLeft, w, cx| e.word(true, true, w, cx)))
            .on_action(cx.listener(|e, _: &SelectWordRight, w, cx| e.word(false, true, w, cx)))
            .on_action(cx.listener(|e, _: &Home, w, cx| e.line_edge(false, false, w, cx)))
            .on_action(cx.listener(|e, _: &End, w, cx| e.line_edge(true, false, w, cx)))
            .on_action(cx.listener(|e, _: &SelectHome, w, cx| e.line_edge(false, true, w, cx)))
            .on_action(cx.listener(|e, _: &SelectEnd, w, cx| e.line_edge(true, true, w, cx)))
            .on_action(cx.listener(|e, _: &DocStart, w, cx| e.set_head(Pos::new(0, 0), false, false, w, cx)))
            .on_action(cx.listener(|e, _: &DocEnd, w, cx| { let p = e.core.doc.end(); e.set_head(p, false, false, w, cx) }))
            .on_action(cx.listener(|e, _: &SelectDocStart, w, cx| e.set_head(Pos::new(0, 0), true, false, w, cx)))
            .on_action(cx.listener(|e, _: &SelectDocEnd, w, cx| { let p = e.core.doc.end(); e.set_head(p, true, false, w, cx) }))
            .on_action(cx.listener(|e, _: &SelectAll, w, cx| { e.core.select_all(); e.moved(w, cx); }))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(|e, _: &Undo, w, cx| { e.marked = None; if e.core.undo() { e.changed(w, cx); } else { cx.emit(EditorEvent::UndoEmpty); } }))
            .on_action(cx.listener(|e, _: &Redo, w, cx| { e.marked = None; e.core.redo(); e.changed(w, cx); }))
            .on_action(cx.listener(|e, _: &Bold, w, cx| e.mark(BOLD, w, cx)))
            .on_action(cx.listener(|e, _: &Italic, w, cx| e.mark(ITALIC, w, cx)))
            .on_action(cx.listener(|e, _: &Strike, w, cx| e.mark(STRIKE, w, cx)))
            .on_action(cx.listener(|e, _: &Highlight, w, cx| e.mark(HIGHLIGHT, w, cx)))
            .on_action(cx.listener(|e, _: &Code, w, cx| e.mark(CODE, w, cx)))
            .on_drop(cx.listener(Self::dropped))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_hover(cx.listener(|e, h: &bool, _, cx| if !*h && e.hover_img.is_some() { e.hover_img = None; cx.notify(); }))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .child(
                div()
                    .id("scroll")
                    .size_full()
                    .when(!single, |d| d.overflow_y_scroll())
                    .track_scroll(&self.scroll)
                    .cursor_text()
                    .child(
                        div().w(px(width)).mx_auto().pt(px(pad_top)).h(px(height)).child(
                            canvas(
                                move |bounds, window, _| {
                                    // pictures: an arrow over the picture, the sizing cursor over its corner grip
                                    let o = bounds.origin;
                                    pics.iter().map(|&(y, w, h)| {
                                        let pic = window.insert_hitbox(Bounds::new(point(o.x, o.y + px(y)), size(px(w), px(h))), HitboxBehavior::Normal);
                                        let grip = window.insert_hitbox(Bounds::new(point(o.x + px(w - 24.), o.y + px(y + h - 24.)), size(px(24.), px(24.))), HitboxBehavior::Normal);
                                        (pic, grip)
                                    }).collect::<Vec<_>>()
                                },
                                move |bounds, hits, window, cx| {
                                    window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                                    entity.update(cx, |e, _| e.origin = bounds.origin);
                                    for (pic, grip) in &hits {
                                        window.set_cursor_style(CursorStyle::Arrow, pic);
                                        window.set_cursor_style(CursorStyle::ResizeUpLeftDownRight, grip);
                                    }
                                    if sizing_pic { window.set_window_cursor_style(CursorStyle::ResizeUpLeftDownRight); }
                                    Self::paint_doc(&data, bounds, window, cx);
                                },
                            )
                            .size_full(),
                        ),
                    ),
            )
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(&mut self, range: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let t = self.head_text();
        let r = from16(&t, range.start)..from16(&t, range.end);
        actual.replace(to16(&t, r.start)..to16(&t, r.end));
        Some(t[r].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let t = self.head_text();
        let (a, h) = (self.core.anchor, self.core.head);
        let (x, y) = if a.block == h.block { (a.off, h.off) } else { (h.off, h.off) };
        let (s, e) = (x.min(y), x.max(y));
        Some(UTF16Selection { range: to16(&t, s)..to16(&t, e), reversed: x > y })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let (b, r) = self.marked.as_ref()?;
        if *b != self.core.head.block { return None; }
        let t = self.head_text();
        if r.end > t.len() { return None; }
        Some(to16(&t, r.start)..to16(&t, r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) { self.marked = None; }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let head = self.core.head.block;
        if let Some((b, r)) = self.marked.take() {
            self.core.set_selection(Pos::new(b, r.start), Pos::new(b, r.end));
        } else if let Some(r16) = range {
            if !self.core.doc.blocks[head].kind.is_image() {
                let t = self.head_text();
                let r = from16(&t, r16.start)..from16(&t, r16.end);
                let (s, e) = self.core.sel();
                if s.block != head || e.block != head || (s.off..e.off) != r { self.core.set_selection(Pos::new(head, r.start), Pos::new(head, r.end)); }
            }
        }
        if self.single { self.core.insert_raw(&text.replace(['\r', '\n'], " ")); }
        else if self.raw { self.type_raw(text); }
        else { self.core.insert_text(text); }
        self.changed(window, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, new_text: &str, new_sel: Option<Range<usize>>, window: &mut Window, cx: &mut Context<Self>) {
        let (s, e) = self.core.sel();
        if self.core.doc.blocks[self.core.head.block].kind.is_image() || s.block != e.block {
            if !self.core.collapsed() { self.core.delete_selection(); }
            self.marked = None;
        }
        let head = self.core.head.block;
        let t = self.head_text();
        let r = match (&self.marked, range) {
            (Some((b, r)), _) if *b == head => r.clone(),
            (_, Some(r16)) if !self.core.doc.blocks[head].kind.is_image() => from16(&t, r16.start)..from16(&t, r16.end),
            _ => { let (s, e) = self.core.sel(); if s.block == head && e.block == head { s.off..e.off } else { self.core.head.off..self.core.head.off } }
        };
        self.core.set_selection(Pos::new(head, r.start), Pos::new(head, r.end));
        self.core.insert_raw(new_text);
        self.marked = (!new_text.is_empty()).then(|| (head, r.start..r.start + new_text.len()));
        if let Some(sel) = new_sel {
            let a = r.start + from16(new_text, sel.start);
            let z = r.start + from16(new_text, sel.end);
            self.core.set_selection(Pos::new(head, a), Pos::new(head, z));
        }
        self.changed(window, cx);
    }

    fn bounds_for_range(&mut self, range: Range<usize>, element_bounds: Bounds<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        if !self.fresh() { return None; }
        let t = self.head_text();
        let p = Pos::new(self.core.head.block, from16(&t, range.start));
        let (x, y, lh) = self.layout.point_of(p);
        Some(Bounds::new(element_bounds.origin + point(px(x), px(y)), size(px(2.), px(lh))))
    }

    fn character_index_for_point(&mut self, pt: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        if !self.fresh() { return None; }
        let (x, y) = self.to_doc(pt);
        let p = self.layout.pos_at(x, y);
        if p.block != self.core.head.block { return None; }
        Some(to16(&self.head_text(), p.off))
    }
}


/// A note drawn without an editor: the picture of a folder in the drawer.
pub struct Preview {
    cache: ShapeCache,
    pics: Pictures,
    dir: PathBuf,
    key: u64,
    doc: Doc,
}

impl Preview {
    pub fn new() -> Preview {
        Preview { cache: ShapeCache::default(), pics: Pictures::new(PathBuf::new()), dir: PathBuf::new(), key: u64::MAX, doc: Doc { blocks: vec![Block::para("")] } }
    }

    /// Draws `src` with its text box's top-left at `at`, `width` wide, text sized by `scale`, inside `clip`.
    pub fn paint(&mut self, src: &str, dir: &std::path::Path, query: &str, at: Point<Pixels>, width: f32, scale: f32, app: &AppTheme, alpha: f32, clip: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        src.hash(&mut h);
        if self.key != h.finish() { self.key = h.finish(); self.doc = Core::from_markdown(src).doc; }
        if self.dir != dir { self.dir = dir.to_path_buf(); self.pics = Pictures::new(dir.to_path_buf()); }
        let th = Theme::from_app(app).faded(alpha);
        let salt = app.salt() ^ (alpha.to_bits() as u64);
        let layout = Rc::new(self.cache.layout(&self.doc, width, &th, salt, scale, false, &mut self.pics, window));
        let mut n = 0;
        let markers = self.doc.blocks.iter().map(|b| match b.kind {
            Kind::Bullet => { n = 0; Some("•".to_string()) }
            Kind::Number => { n += 1; Some(format!("{n}.")) }
            _ => { n = 0; None }
        }).collect();
        let mut finds = Vec::new();
        if !query.is_empty() {
            for (i, b) in self.doc.blocks.iter().enumerate() {
                if b.kind.is_image() { continue; }
                let t = b.text();
                let l = t.to_lowercase();
                if l.len() != t.len() { continue; }
                let mut from = 0;
                while let Some(k) = l[from..].find(query) { let a = from + k; finds.push((i, a, a + query.len())); from = a + query.len().max(1); }
            }
        }
        let data = PaintData {
            layout, th, sel: (Pos::new(0, 0), Pos::new(0, 0)), collapsed: true, head: Pos::new(0, 0), caret: false, focused: false, markers,
            kinds: self.doc.blocks.iter().map(|b| b.kind.clone()).collect(), width, scale, finds, hover_img: None, cur: None, placeholder: None,
            // what the clip leaves (with room for a leaning folder)
            vis_bottom: f32::from(clip.origin.y + clip.size.height - at.y) + 40.,
        };
        window.with_content_mask(Some(ContentMask { bounds: clip }), |window| Editor::paint_doc(&data, Bounds::new(at, size(px(width), px(4000.))), window, cx));
    }
}

// ---- shapes that lean with the glyphs ---------------------------------------------
// A preview on a tilted folder in the stack is painted with `set_glyph_rotation` on: text and
// pictures turn on the GPU, these turn their own corner points. Without a lean they are plain quads.

/// a point turned by the current glyph lean
fn turn(p: Point<Pixels>) -> Point<Pixels> {
    let Some((a, cx, cy)) = gpui::glyph_rotation() else { return p };
    let (s, c) = a.sin_cos();
    let (dx, dy) = (f32::from(p.x) - cx, f32::from(p.y) - cy);
    point(px(cx + dx * c - dy * s), px(cy + dx * s + dy * c))
}

fn rrect_path(mut pb: PathBuilder, b: Bounds<Pixels>, r: f32) -> PathBuilder {
    let (l, t) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (rt, bt) = (l + f32::from(b.size.width), t + f32::from(b.size.height));
    let r = r.min((rt - l) / 2.).min((bt - t) / 2.).max(0.);
    let p = |x: f32, y: f32| turn(point(px(x), px(y)));
    let rad = point(px(r), px(r));
    pb.move_to(p(l + r, t));
    pb.line_to(p(rt - r, t));
    if r > 0. { pb.arc_to(rad, px(0.), false, true, p(rt, t + r)); }
    pb.line_to(p(rt, bt - r));
    if r > 0. { pb.arc_to(rad, px(0.), false, true, p(rt - r, bt)); }
    pb.line_to(p(l + r, bt));
    if r > 0. { pb.arc_to(rad, px(0.), false, true, p(l, bt - r)); }
    pb.line_to(p(l, t + r));
    if r > 0. { pb.arc_to(rad, px(0.), false, true, p(l + r, t)); }
    pb.close();
    pb
}

fn fill_r(window: &mut Window, b: Bounds<Pixels>, r: f32, color: Hsla) {
    if gpui::glyph_rotation().is_none() { window.paint_quad(fill(b, color).corner_radii(px(r))); return; }
    if let Ok(p) = rrect_path(PathBuilder::fill(), b, r).build() { window.paint_path(p, color); }
}

/// an outline `w` wide inside `b`
fn ring_r(window: &mut Window, b: Bounds<Pixels>, r: f32, w: f32, color: Hsla) {
    if gpui::glyph_rotation().is_none() { window.paint_quad(quad(b, px(r), gpui::transparent_black(), px(w), color, BorderStyle::Solid)); return; }
    let h = px(w / 2.);
    let inner = Bounds::new(point(b.origin.x + h, b.origin.y + h), size(b.size.width - h * 2., b.size.height - h * 2.));
    if let Ok(p) = rrect_path(PathBuilder::stroke(px(w)), inner, (r - w / 2.).max(0.)).build() { window.paint_path(p, color); }
}
