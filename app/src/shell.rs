//! The app: note list, settings, find, colours, delete/undo, shortcuts, tray commands.

use crate::editor::{self, Editor, EditorEvent};
use crate::folder::*;
#[path = "motion.rs"]
mod motion;
#[path = "setui.rs"]
mod setui;
#[path = "emenu.rs"]
mod emenu;
use motion::{Anim, Ease, Motion, V, SEARCH, SETTINGS};
use crate::locale::{self, Language};
use crate::settings::{self, Prefs, DEFAULT_CARDS, KEYS, MAX_CARDS};
use crate::strings as ui;
use crate::system::{self, Cmd};
use crate::theme::{c, theme_kind, AppTheme};
use gpui::prelude::FluentBuilder;
use gpui::*;
use notedd_core::store::{self, Note};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use crate::platform::{self, LocalTime, Win};

actions!(
    shell,
    [NewNote, AllNotes, FindInNote, OpenSettings, TogglePin, QuitApp, NoteColor, DeleteNote, ZoomIn, ZoomOut, ZoomReset, Escape, NextPage]
);


const HF: f32 = 290.;
const LIST_TOP: f32 = 6.;
const LIST_BOTTOM: f32 = 40.;
const LIST_SC: f32 = 0.955;
const PERSP: f32 = 1700.;
const DEPTH: f32 = 26.;

thread_local! { static RSTART: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) }; static PSUM: std::cell::Cell<(f32, u32)> = const { std::cell::Cell::new((0., 0)) }; }
thread_local! { static PERF: std::cell::RefCell<(Option<std::time::Instant>, [u32; 12])> = const { std::cell::RefCell::new((None, [0; 12])) }; }
fn perf(_ms: f32) {
    if std::env::var("NOTEDD_PERF").is_err() { return; }
    PERF.with(|p| {
        let mut p = p.borrow_mut();
        let now = std::time::Instant::now();
        if let Some(t) = p.0 {
            let gap = now.duration_since(t).as_secs_f32() * 1000.;
            if gap < 48. { p.1[(gap / 4.) as usize] += 1; }
        }
        p.0 = Some(now);
        if let Some(r) = RSTART.with(|r| r.get()) { let v = now.duration_since(r).as_secs_f32() * 1000.; PSUM.with(|s| { let (a, n) = s.get(); s.set((a + v, n + 1)); }); }
        let n: u32 = p.1.iter().sum();
        if n >= 120 { let (a, c) = PSUM.with(|s| s.replace((0., 0))); dbg_log(&format!("gaps per 4ms bucket (0-48ms): {:?} render->paint avg {:.2} ms", p.1, a / c.max(1) as f32)); p.1 = [0; 12]; }
    });
}

/// With NOTEDD_PERF set, timings and traces go to notedd-dbg.txt in the temp folder.
pub fn dbg_log(s: &str) {
    use std::io::Write;
    if std::env::var_os("NOTEDD_PERF").is_none() { return; }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("notedd-dbg.txt")) { let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() % 100000); let _ = writeln!(f, "{ms} {s}"); }
}

fn plain(s: &str) -> String { notedd_core::md::strip(s) }

#[derive(Clone, Copy, PartialEq)]
enum Ctl { Close, Pin, Find, New, More, Gear, List, Trash, Dot }

#[derive(Clone, Copy, PartialEq)]
enum Mode { Note, List, Settings }

pub struct Shell {
    prefs: Prefs,
    th: AppTheme,
    dir: PathBuf,
    notes: Vec<Note>,
    cur: String,
    mode: Mode,
    /// where Esc in the settings goes back to
    back: Mode,
    body: Entity<Editor>,
    search: Entity<Editor>,
    findbox: Entity<Editor>,
    hexes: Vec<Entity<Editor>>,
    find_open: bool,
    palette: bool,
    pal_id: String,
    pal_at: (f32, f32),
    pal_t: Ease,
    pal_sc: [Ease; MAX_CARDS],
    pal_hover: Option<usize>,
    pal_prev: bool,
    sel: usize,
    page: usize,
    capture: Option<usize>,
    toast: Option<SharedString>,
    toast_gen: u32,
    deleted: Option<Note>,
    dirty: bool,
    saver: Option<Task<()>>,
    focus: FocusHandle,
    win: Win,
    st: setui::SetState,
    emenu: Option<emenu::EMenu>,
    rsz: Option<(usize, (i32, i32), (i32, i32, i32, i32))>,
    ctl_open: bool,
    hover_plus: bool,
    tab_hover: bool,
    ctl_last: Vec<Ctl>,
    ctl_prev: Vec<Ctl>,
    hover_ctl: Option<usize>,
    list_start: usize,
    edit_h: f32,
    mo: Motion,
}

// ---- helpers -----------------------------------------------------------------

fn local(ft: u64) -> LocalTime { platform::local_time(ft) }

fn when(ft: u64) -> String {
    const MONTHS_RU: [&str; 12] = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];
    const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let now = store::now_ft();
    let (d, n, y) = (local(ft), local(now), local(now.saturating_sub(864_000_000_000)));
    let same = |a: &LocalTime, b: &LocalTime| a.year == b.year && a.month == b.month && a.day == b.day;
    let hm = format!("{:02}:{:02}", d.hour, d.minute);
    let relative = if same(&d, &n) { Some(ui::TODAY.get()) } else if same(&d, &y) { Some(ui::YESTERDAY.get()) } else { None };
    if let Some(label) = relative { return format!("{label}, {hm}"); }
    let month = (d.month.clamp(1, 12) - 1) as usize;
    match locale::current() {
        Language::English => format!("{} {}", MONTHS_EN[month], d.day),
        Language::Russian => format!("{} {}", d.day, MONTHS_RU[month]),
        Language::Chinese => format!("{}月{}日", month + 1, d.day),
    }
}

fn words(n: usize) -> String {
    match locale::current() {
        Language::English => if n == 1 { "1 word".into() } else { format!("{n} words") },
        Language::Russian => {
            let w = if n % 10 == 1 && n % 100 != 11 { "слово" } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) { "слова" } else { "слов" };
            format!("{n} {w}")
        }
        Language::Chinese => format!("{n} 个词"),
    }
}

fn hexs(v: u32) -> String { format!("{v:06x}") }

fn bind_keys(cx: &mut App, p: &Prefs) {
    cx.clear_key_bindings();
    editor::bind_keys(cx);
    let s = Some("Shell");
    cx.bind_keys([
        KeyBinding::new("secondary-=", ZoomIn, s),
        KeyBinding::new("secondary-shift-=", ZoomIn, s),
        KeyBinding::new("secondary--", ZoomOut, s),
        KeyBinding::new("secondary-0", ZoomReset, s),
        KeyBinding::new("escape", Escape, s),
        KeyBinding::new("ctrl-tab", NextPage, s),
    ]);
    for (i, &(m, vk)) in p.keys.iter().enumerate().skip(2) {
        let Some(b) = system::binding(m, vk) else { continue };
        let kb = match i {
            2 => KeyBinding::new(&b, NewNote, s),
            3 => KeyBinding::new(&b, AllNotes, s),
            4 => KeyBinding::new(&b, FindInNote, s),
            5 => KeyBinding::new(&b, OpenSettings, s),
            6 => KeyBinding::new(&b, TogglePin, s),
            7 => KeyBinding::new(&b, QuitApp, s),
            8 => KeyBinding::new(&b, NoteColor, s),
            9 => KeyBinding::new(&b, DeleteNote, s),
            _ => KeyBinding::new(&b, editor::Highlight, Some("Editor")),
        };
        cx.bind_keys([kb]);
    }
}

fn single_input(shell: &mut Shell, placeholder: &str, window: &mut Window, cx: &mut Context<Shell>) -> Entity<Editor> {
    let th = shell.th;
    let e = cx.new(|cx| {
        let mut e = Editor::new(true, th, cx);
        e.pad_top = 4.;
        e.placeholder = placeholder.to_string().into();
        e
    });
    let _ = window;
    e
}

impl Shell {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Shell {
        let prefs = settings::load();
        cx.observe_window_appearance(window, |this, window, cx| {
            settings::set_system_dark(matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark));
            if this.prefs.theme == 2 {
                this.th = AppTheme::new(&this.prefs);
                this.apply_look(cx);
                cx.notify();
            }
        }).detach();
        #[cfg(target_os = "macos")]
        cx.observe_window_activation(window, |this, window, _| {
            this.win.set_topmost(this.prefs.pinned || window.is_window_active());
        }).detach();
        locale::set(prefs.language);
        let th = AppTheme::new(&prefs);
        let dir = settings::notes_dir();
        let mut notes = store::load(&dir);
        if notes.is_empty() {
            let mut n = Note::new(store::new_id(), 0, ui::WELCOME.get().to_string(), store::now_ft());
            n.opened = store::now_ft();
            let _ = store::save(&dir, &n);
            notes.push(n);
        }
        let seen = |n: &Note| n.seen();
        let cur = store::read_state(&dir).filter(|id| notes.iter().any(|n| &n.id == id))
            .unwrap_or_else(|| notes.iter().max_by_key(|n| seen(n)).map(|n| n.id.clone()).unwrap_or_default());
        let raw = !prefs.simple_editor;
        let body = cx.new(|cx| { let mut e = Editor::new(false, th, cx); e.pad = 39.; e.pad_top = 35.5; e.raw = raw; e });
        let focus = cx.focus_handle();
        let mut me = Shell {
            prefs, th, dir, notes, cur, mode: Mode::Note, back: Mode::Note,
            body: body.clone(),
            search: body.clone(), findbox: body.clone(), hexes: Vec::new(),
            find_open: false, palette: false, pal_id: String::new(), pal_at: (0., 0.), pal_t: Ease::at(0.), pal_sc: [Ease::at(1.); MAX_CARDS], pal_hover: None, pal_prev: false, sel: 0, page: 0, capture: None,
            toast: None, toast_gen: 0, deleted: None, dirty: false, saver: None, focus,
            win: Win::of(window).unwrap_or_default(),
            st: setui::SetState::new(), emenu: None, rsz: None,
            ctl_open: false, hover_plus: false, tab_hover: false, ctl_last: Vec::new(), ctl_prev: Vec::new(), hover_ctl: None, list_start: 0, edit_h: 560.,
            mo: Motion::new(560.),
        };
        me.search = single_input(&mut me, ui::SEARCH_NOTES.get(), window, cx);
        me.findbox = single_input(&mut me, ui::FIND_IN_NOTE.get(), window, cx);
        me.findbox.update(cx, |e, _| e.scale = 0.92);
        for _ in 0..(2 + MAX_CARDS) { let e = single_input(&mut me, "rrggbb", window, cx); me.hexes.push(e); }
        // events
        cx.subscribe_in(&me.body, window, |this, _, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Changed => this.schedule_save(window, cx),
            EditorEvent::Cancel => this.escape(window, cx),
            EditorEvent::UndoEmpty => this.undo_delete(window, cx),
            _ => {}
        }).detach();
        cx.subscribe_in(&me.search, window, |this, _, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Changed => { this.sel = 0; this.list_start = 0; this.retarget(false, 0, window, cx); cx.notify(); }
            EditorEvent::Submit => this.submit_search(window, cx),
            EditorEvent::Nav(d) => {
                let n = this.shown(cx).len() as i32;
                if n > 0 { this.sel = (this.sel as i32 - d).clamp(0, n - 1) as usize; }
                this.retarget(false, 0, window, cx);
            }
            EditorEvent::Cancel => this.escape(window, cx),
            EditorEvent::UndoEmpty => this.undo_delete(window, cx),
        }).detach();
        cx.subscribe_in(&me.findbox, window, |this, e, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Changed => {
                let q = e.read(cx).text();
                this.body.update(cx, |b, cx| { b.find = q; b.goto_start(); b.find_step(false, window, cx); });
            }
            EditorEvent::Submit | EditorEvent::Nav(1) => this.body.update(cx, |b, cx| b.find_step(false, window, cx)),
            EditorEvent::Nav(_) => this.body.update(cx, |b, cx| b.find_step(true, window, cx)),
            EditorEvent::Cancel => this.escape(window, cx),
            _ => {}
        }).detach();
        for i in 0..me.hexes.len() {
            let e = me.hexes[i].clone();
            cx.subscribe_in(&e, window, move |this, e, ev: &EditorEvent, window, cx| match ev {
                EditorEvent::Changed => { let t = e.read(cx).text(); this.hex_changed(i, &t, window, cx); }
                EditorEvent::Cancel | EditorEvent::Submit => this.escape(window, cx),
                _ => {}
            }).detach();
        }
        me.load_current(window, cx);
        let r = me.win_rect();
        me.mo.edit_top = r.top;
        me.mo.win_x = r.left as f32; me.mo.win_x_to = me.mo.win_x;
        me.mo.win_y = r.top as f32; me.mo.win_y_to = me.mo.win_y;
        let front = V { y: 0., sc: 1., h: me.edit_h, o: 1., lift: 0., rot: 0., sx: 0. };
        me.mo.anims.insert(me.cur.clone(), Anim { cur: front, to: front, z: 1000, delay: 0. });
        me.mo.cap_n = Ease::at(me.controls().len() as f32);
        me.apply_look(cx);
        bind_keys(cx, &me.prefs);
        system::set_hotkeys([me.prefs.keys[0], me.prefs.keys[1]]);
        if let Some(w) = Win::of(window) {
            w.prepare();
            if me.prefs.pinned { w.set_topmost(true); }
        }
        me.focus_main(window, cx);
        me
    }

    // ---- notes ------------------------------------------------------------------

    fn idx(&self, id: &str) -> Option<usize> { self.notes.iter().position(|n| n.id == id) }

    fn order(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.notes.len()).collect();
        v.sort_by_key(|&i| std::cmp::Reverse(self.notes[i].seen()));
        v
    }

    /// indices of the notes the search shows
    fn shown(&self, cx: &App) -> Vec<usize> {
        let q = self.search.read(cx).text().trim().to_lowercase();
        self.order().into_iter().filter(|&i| q.is_empty() || self.notes[i].lower.contains(&q)).collect()
    }

    fn load_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.idx(&self.cur).map(|i| self.notes[i].text.clone()).unwrap_or_default();
        let dir = self.dir.clone();
        self.body.update(cx, |b, _| { b.set_markdown(&text, dir); b.find.clear(); });
        let _ = window;
    }

    fn save_now(&mut self, cx: &mut Context<Self>) {
        if !self.dirty { return; }
        self.dirty = false;
        let text = self.body.read(cx).markdown();
        let Some(i) = self.idx(&self.cur) else { return };
        let n = &mut self.notes[i];
        n.set_text(text);
        n.mtime = store::now_ft();
        if n.is_empty() { store::delete(&self.dir, n); } else { let _ = store::save(&self.dir, n); }
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dirty = true;
        self.saver = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(500)).await;
            let _ = this.update(cx, |s, cx| s.save_now(cx));
        }));
        cx.notify();
    }

    fn drop_empty(&mut self, keep: &str) {
        let dir = self.dir.clone();
        self.notes.retain(|n| {
            let drop = n.is_empty() && n.id != keep;
            if drop { store::delete(&dir, n); }
            !drop
        });
    }

    fn open_note(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        if id != self.cur { let leaving = self.cur.clone(); self.cur = id.to_string(); self.drop_empty(&self.cur.clone()); let _ = leaving; }
        let now = store::now_ft();
        if let Some(i) = self.idx(id) { self.notes[i].opened = now; }
        store::write_state(&self.dir, id);
        self.deleted = None;
        self.mode = Mode::Note;
        self.find_open = false;
        self.palette = false;
        self.load_current(window, cx);
        self.retarget(false, 0, window, cx);
        self.body.update(cx, |b, cx| b.request_focus(cx));
        cx.notify();
    }

    fn new_note(&mut self, title: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        let outgoing = (self.mode == Mode::Note).then(|| self.cur.clone());
        let mut n = Note::new(store::new_id(), 0, if title.is_empty() { String::new() } else { format!("{title}\n") }, store::now_ft());
        n.opened = store::now_ft();
        let id = n.id.clone();
        self.notes.push(n);
        self.open_note(&id, window, cx);
        if let Some(old) = outgoing { self.swap_cards(&old, &id); }
        self.body.update(cx, |b, cx| b.go_to_end(window, cx));
    }

    fn delete_note(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        let Some(i) = self.idx(id) else { return };
        let n = self.notes.remove(i);
        if n.is_empty() { store::delete(&self.dir, &n); } else {
            settings::trash(&self.dir, &n.id);
            self.deleted = Some(n);
        }
        if self.notes.is_empty() {
            let mut e = Note::new(store::new_id(), 0, String::new(), store::now_ft());
            e.opened = store::now_ft();
            self.notes.push(e);
        }
        if id == self.cur {
            self.cur = self.notes[self.order()[0]].id.clone();
            store::write_state(&self.dir, &self.cur);
            self.load_current(window, cx);
        }
        self.enter_list(window, cx);
    }

    fn delete_by_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = match self.mode {
            Mode::Settings => return,
            Mode::List => match self.shown(cx).get(self.sel) { Some(&i) => self.notes[i].id.clone(), None => return },
            Mode::Note => self.cur.clone(),
        };
        self.delete_note(&id, window, cx);
    }

    fn undo_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut n) = self.deleted.take() else { return };
        self.notes.retain(|x| !x.is_empty());
        n.opened = store::now_ft();
        let _ = store::save(&self.dir, &n);
        let id = n.id.clone();
        self.notes.push(n);
        self.toast = None;
        self.cur = id.clone();
        self.open_note(&id, window, cx);
        self.enter_list(window, cx);
        self.deleted = None;
    }

    fn show_toast(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.toast = Some(text.to_string().into());
        self.toast_gen += 1;
        let gen = self.toast_gen;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(4500)).await;
            let _ = this.update(cx, |s, cx| if s.toast_gen == gen { s.toast = None; cx.notify(); });
        }).detach();
    }

    // ---- modes ------------------------------------------------------------------

    fn focus_main(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Note => self.body.update(cx, |b, cx| b.request_focus(cx)),
            Mode::List => self.search.update(cx, |b, cx| b.request_focus(cx)),
            Mode::Settings => window.focus(&self.focus),
        }
        cx.notify();
    }

    fn enter_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        self.mode = Mode::List;
        self.palette = false;
        self.find_open = false;
        self.capture = None;
        self.sel = 0;
        let ph = if self.deleted.is_some() { ui::NOTE_DELETED.get() } else { ui::SEARCH_NOTES.get() };
        self.search.update(cx, |s, _| { s.set_markdown("", PathBuf::new()); s.placeholder = ph.to_string().into(); });
        self.retarget(true, 0, window, cx);
        self.focus_main(window, cx);
    }

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Settings { return self.escape(window, cx); }
        self.save_now(cx);
        self.back = self.mode;
        self.ctl_open = true;
        self.mode = Mode::Settings;
        self.palette = false;
        self.capture = None;
        self.sync_hexes(cx);
        self.retarget(false, 0, window, cx);
        if self.back == Mode::Note { self.swap_cards(&self.cur.clone(), SETTINGS); }
        self.focus_main(window, cx);
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.capture.is_some() { self.capture = None; return cx.notify(); }
        if self.emenu.is_some() { self.emenu = None; return cx.notify(); }
        if self.palette { self.palette = false; return cx.notify(); }
        if self.mode == Mode::Settings && self.st.pick.is_some() { self.st.pick = None; return cx.notify(); }
        match self.mode {
            Mode::Settings => { self.mode = self.back; self.retarget(false, 0, window, cx); if self.mode == Mode::Note { self.swap_cards(SETTINGS, &self.cur.clone()); } self.focus_main(window, cx); }
            Mode::List => { let id = self.cur.clone(); self.open_note(&id, window, cx); }
            Mode::Note => {
                if self.find_open { self.close_find(window, cx); } else { self.hide(window, cx); }
            }
        }
    }

    fn submit_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.shown(cx);
        if let Some(&i) = shown.get(self.sel) {
            let id = self.notes[i].id.clone();
            self.open_note(&id, window, cx);
        } else {
            let q = self.search.read(cx).text().trim().to_string();
            self.new_note(&q, window, cx);
        }
    }

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Note { return; }
        self.find_open = true;
        self.palette = false;
        self.findbox.update(cx, |f, cx| { f.request_focus(cx); f.select_all_text(window, cx); });
        self.mo.tab_hl.to = 0.;
        self.retarget(false, 0, window, cx);
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        self.body.update(cx, |b, cx| { b.find.clear(); b.request_focus(cx); });
        self.retarget(false, 0, window, cx);
        cx.notify();
    }

    // ---- look and prefs ---------------------------------------------------------

    fn apply_look(&mut self, cx: &mut Context<Self>) {
        let (th, k) = (self.th, self.prefs.scale as f32 / 100.);
        self.body.update(cx, |e, cx| { e.app = th; e.scale = k; cx.notify(); });
        for e in [self.search.clone(), self.findbox.clone()].into_iter().chain(self.hexes.clone()) {
            e.update(cx, |e, cx| { e.app = th; cx.notify(); });
        }
    }

    fn apply_prefs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        locale::set(self.prefs.language);
        self.th = AppTheme::new(&self.prefs);
        settings::save(&self.prefs);
        self.apply_look(cx);
        bind_keys(cx, &self.prefs);
        system::set_hotkeys([self.prefs.keys[0], self.prefs.keys[1]]);
        system::retitle_tray();
        let ph =[ui::SEARCH_NOTES.get(), ui::FIND_IN_NOTE.get()];
        self.search.update(cx, |e, _| e.placeholder = ph[0].to_string().into());
        self.findbox.update(cx, |e, _| e.placeholder = ph[1].to_string().into());
        // Markdown or Simple: the note is reopened in the other editor
        let raw = !self.prefs.simple_editor;
        if self.body.read(cx).raw != raw {
            self.save_now(cx);
            self.body.update(cx, |b, _| b.raw = raw);
            self.load_current(window, cx);
        }
        cx.notify();
    }

    fn zoom(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let s = if delta == 0 { 100 } else { (self.prefs.scale as i32 + delta).clamp(80, 160) as u32 };
        if s != self.prefs.scale { self.prefs.scale = s; self.apply_prefs(window, cx); }
    }

    fn sync_hexes(&mut self, cx: &mut Context<Self>) {
        let k = theme_kind(&self.prefs);
        let mut vals = vec![self.prefs.ink[k].unwrap_or(self.th.ink), self.prefs.accent[k].unwrap_or(self.th.tab_dark)];
        vals.extend(self.prefs.cards[k]);
        for (e, v) in self.hexes.iter().zip(vals) { e.update(cx, |e, _| e.set_markdown(&hexs(v), PathBuf::new())); }
    }

    fn hex_changed(&mut self, i: usize, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(v) = settings::hex(text) else { return };
        let k = theme_kind(&self.prefs);
        match i { 0 => self.prefs.ink[k] = Some(v), 1 => self.prefs.accent[k] = Some(v), n => self.prefs.cards[k][n - 2] = v }
        self.apply_prefs(window, cx);
    }

    fn pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prefs.pinned = !self.prefs.pinned;
        self.win.set_topmost(self.prefs.pinned || (cfg!(target_os = "macos") && window.is_window_active()));
        self.apply_prefs(window, cx);
    }

    fn move_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(ui::MOVE_NOTES.get().into()), extensions: None });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let Some(to) = paths.into_iter().next() else { return };
            let _ = this.update_in(cx, |s, window, cx| {
                s.save_now(cx);
                let from = s.dir.clone();
                let target = to.clone();
                match store::move_notes(&from, &to, move || settings::set_notes_dir(&target)) {
                    Ok(_) => {
                        s.dir = settings::notes_dir();
                        s.notes = store::load(&s.dir);
                        if s.notes.is_empty() { let n = Note::new(store::new_id(), 0, String::new(), store::now_ft()); s.notes.push(n); }
                        if s.idx(&s.cur).is_none() { s.cur = s.notes[s.order()[0]].id.clone(); }
                        s.load_current(window, cx);
                        cx.notify();
                    }
                    Err(_) => s.show_toast(ui::MOVE_FAILED.get(), window, cx),
                }
            });
        }).detach();
    }

    pub fn command(&mut self, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            Cmd::Show => self.appear(window, cx),
            Cmd::Toggle => {
                self.toggle_window(window, cx);
            }
            Cmd::NewNote => { self.appear(window, cx); self.new_note("", window, cx); }
            Cmd::OpenFolder => { system::open_folder(&self.dir); }
            Cmd::Quit => self.quit(cx),
        }
    }

    fn quit(&mut self, cx: &mut Context<Self>) {
        self.save_now(cx);
        system::remove_tray();
        cx.quit();
    }

    /// starts sizing from one edge: the window follows the pointer until the button goes up
    fn begin_resize(&mut self, hit: usize) {
        if !matches!(hit, 10 | 11 | 12 | 13 | 14 | 15 | 16 | 17) { return; }
        let (p, r) = (self.win.cursor_pos(), self.win.client_rect());
        dbg_log(&format!("begin_resize hit={hit} cur={},{} win={},{},{},{}", p.x, p.y, r.left, r.top, r.right, r.bottom));
        self.rsz = Some((hit, (p.x, p.y), (r.left, r.top, r.right, r.bottom)));
    }

    fn drag_resize(&mut self, window: &Window) {
        let Some((hit, (px0, py0), (l0, t0, r0, b0))) = self.rsz else { return };
        let p = self.win.cursor_pos();
        let (dx, dy) = (p.x - px0, p.y - py0);
        let sc = window.scale_factor();
        let (minw, minh) = ((522. * sc) as i32, (330. * sc) as i32);
        let (mut l, mut t, mut r, mut b) = (l0, t0, r0, b0);
        if matches!(hit, 10 | 13 | 16) { l = (l0 + dx).min(r0 - minw); }
        if matches!(hit, 11 | 14 | 17) { r = (r0 + dx).max(l0 + minw); }
        if matches!(hit, 12 | 13 | 14) { t = (t0 + dy).min(b0 - minh); }
        if matches!(hit, 15 | 16 | 17) { b = (b0 + dy).max(t0 + minh); }
        self.win.set_client_rect(platform::Rect { left: l, top: t, right: r, bottom: b });
    }

    // ---- views ------------------------------------------------------------------

    /// keys of the stack the search box does not use: Delete removes the picked note, Home and End jump
    fn list_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let m = &ev.keystroke.modifiers;
        if m.control || m.alt || m.shift || m.platform || self.palette { return false; }
        let n = self.shown(cx).len();
        match ev.keystroke.key.as_str() {
            "delete" => { self.delete_by_key(window, cx); true }
            "home" | "end" if n > 0 => {
                // the stack grows upwards: Home is the front note, End the one at the back
                self.sel = if ev.keystroke.key == "home" { 0 } else { n - 1 };
                self.retarget(false, 0, window, cx);
                cx.notify();
                true
            }
            _ => false,
        }
    }

    fn toggle_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::List { let id = self.cur.clone(); self.open_note(&id, window, cx); } else { self.enter_list(window, cx); }
    }

    fn note_color(&self) -> u32 {
        let col = self.idx(&self.cur).map_or(0, |i| self.notes[i].color);
        self.th.note(col)
    }

    fn controls(&self) -> Vec<Ctl> {
        let mut v = vec![Ctl::Close, Ctl::Pin];
        match self.mode { Mode::List => v.push(Ctl::New), Mode::Note => v.push(Ctl::Find), Mode::Settings => {} }
        v.push(Ctl::More);
        if self.ctl_open || self.palette {
            v.push(Ctl::Gear);
            v.push(Ctl::List);
            if self.mode == Mode::Note { v.push(Ctl::Trash); v.push(Ctl::Dot); }
        }
        v
    }

    fn press(&mut self, c: Ctl, window: &mut Window, cx: &mut Context<Self>) {
        dbg_log(&format!("press {} ctl_open={}", c as u8, self.ctl_open));
        match c {
            Ctl::Close => self.hide(window, cx),
            Ctl::Pin => self.pin(window, cx),
            Ctl::Find => self.open_find(window, cx),
            Ctl::New => self.new_note("", window, cx),
            Ctl::More => { self.ctl_open = !self.ctl_open; if !self.ctl_open { self.palette = false; } self.sync_caps(window, cx); }
            Ctl::Gear => self.open_settings(window, cx),
            Ctl::List => self.toggle_list(window, cx),
            Ctl::Trash => { let id = self.cur.clone(); self.delete_note(&id, window, cx); }
            Ctl::Dot => {
                let k = self.controls().iter().position(|c| *c == Ctl::Dot).unwrap_or(0);
                self.pal_at = (ctl_x(self.mo.last_w, k), T + 8.);
                self.palette = !self.palette;
                self.sync_caps(window, cx);
            }
        }
    }

    fn folder_band(&self, id: impl Into<ElementId>, left: f32, top: f32, w: f32, h: f32) -> Stateful<Div> {
        div().id(id).absolute().left(px(left)).top(px(top)).w(px(w)).h(px(h)).cursor_pointer()
    }
}

// ---- painting the folders ---------------------------------------------------------

struct Item {
    kind: u8, // 0 note, 1 search, 2 settings
    id: String,
    v: V,
    color: u32,
    title: String,
    info: String,
    ctx: String,
    ctx_hl: Option<(usize, usize)>,
    count: String,
    text: String,
    front: bool,
    editor: bool,
    dark_w: f32,
}

struct Draw {
    th: AppTheme,
    w: f32,
    mode: Mode,
    find_open: bool,
    find_label: String,
    items: Vec<Item>,
    controls: Vec<(Ctl, bool)>,
    ctl_prev: Vec<Ctl>,
    ctl_fade: f32,
    query: String,
    pinned: bool,
    plus_sc: f32,
    cap_n: f32,
    ctl_a: f32,
    more_t: f32,
    dot_sc: f32,
    tab_hl: f32,
    dot_color: u32,
    radii: (f32, f32, f32),
    dir: PathBuf,
    scale: f32,
    set: Option<setui::SetDraw>,
    pal: Option<PalDraw>,
    previews: Rc<std::cell::RefCell<std::collections::HashMap<String, crate::editor::Preview>>>,
}

#[derive(Clone)]
struct PalDraw { x0: f32, y0: f32, pw: f32, ph: f32, t: f32, chosen: usize, cols: Vec<u32>, sc: Vec<f32>, listing: bool, del_hover: bool }

fn paint_palette(pd: &PalDraw, th: &AppTheme, window: &mut Window, cx: &mut App) {
    let t = pd.t.clamp(0., 1.);
    if t < 0.01 { return; }
    let (x0, y0) = (pd.x0, pd.y0 - 6. * (1. - t));
    let sh = if th.night { black() } else { c(0x28261e) };
    let k = if th.night { 2.4 } else { 1.0 };
    let n = 12;
    for i in 0..n {
        let e = -24. * 0.35 + 24. * 0.9 * i as f32 / (n as f32 - 1.);
        let a = (0.16 * t * k / n as f32 * 1.7).min(0.5);
        let b = Bounds::new(point(px(x0 - e), px(y0 + 12. - e)), size(px((pd.pw + 2. * e).max(0.)), px((pd.ph + 2. * e).max(0.))));
        window.paint_quad(fill(b, Hsla { a, ..sh }).corner_radii(px((14. + e).max(0.))));
    }
    let (paper, line, ink) = (c(th.paper()), c(th.line), c(th.ink));
    let bounds = Bounds::new(point(px(x0), px(y0)), size(px(pd.pw), px(pd.ph)));
    window.paint_quad(quad(bounds, px(14.), al(paper, t), px(1.5), al(line, t), BorderStyle::Solid));
    let n = pd.cols.len();
    let pal_w = n as f32 * 32. + (n as f32 - 1.) * 4. + 12.;
    for i in 0..n {
        let (cx0, cy0) = (x0 + (pd.pw - pal_w) / 2. + 22. + 36. * i as f32, y0 + 22.);
        let s = pd.sc[i];
        circle(window, cx0, cy0, 10. * s, None, Some(al(c(pd.cols[i]), t)));
        circle(window, cx0, cy0, 9.25 * s, Some((1.5, al(line, t))), None);
        if i == pd.chosen { circle(window, cx0, cy0, 15.25, Some((1.5, al(line, t))), None); }
    }
    if pd.listing {
        let y = y0 + 44.;
        let mut sep = ink; sep.a = 0.14 * t;
        let mut ln = PathBuilder::stroke(px(1.));
        ln.move_to(point(px(x0 + 12.), px(y)));
        ln.line_to(point(px(x0 + pd.pw - 12.), px(y)));
        if let Ok(p) = ln.build() { window.paint_path(p, sep); }
        if pd.del_hover {
            let b = Bounds::new(point(px(x0 + 5.), px(y + 4.)), size(px(pd.pw - 10.), px(pd.ph - 44. - 9.)));
            window.paint_quad(fill(b, al(ink, 0.09 * t)).corner_radii(px(8.)));
        }
        glyph(window, TRASH, x0 + 22., y + 19., 16., 1.4, al(ink, t), false);
        text(window, cx, ui::DELETE_NOTE.get(), point(px(x0 + 39.), px(y + 19. - 13. * 0.7)), 13., FontWeight::NORMAL, al(ink, t), pd.pw - 49.);
    }
}

fn ctl_x(w: f32, k: usize) -> f32 { w - 16. - 34. * k as f32 }

fn al(mut c: Hsla, o: f32) -> Hsla { c.a *= o; c }

/// the found part of a line gets the highlighter band behind it. The line's text sits at the folder's
/// local point (lx, ly), `size` and `max` are on-screen; the band leans with the folder like the glyphs do.
fn mark(window: &mut Window, th: &AppTheme, p: &Place, s: &str, (lx, ly): (f32, f32), size: f32, weight: FontWeight, q: &str, range: Option<(usize, usize)>, o: f32, max: f32) {
    let r = range.or_else(|| { let l = s.to_lowercase(); if l.len() != s.len() || q.is_empty() { None } else { l.find(q).map(|p| (p, q.len())) } });
    let Some((a, n)) = r else { return };
    if n == 0 || a + n > s.len() || !s.is_char_boundary(a) || !s.is_char_boundary(a + n) { return; }
    let x0 = text_width(window, &s[..a], size, weight);
    // the line is cut with an ellipsis at `max`: no band past what is drawn
    let full = text_width(window, s, size, weight);
    let end = if full > max { max - text_width(window, "…", size, weight) } else { max };
    if x0 >= end { return; }
    let w = text_width(window, &s[a..a + n], size, weight).min(end - x0);
    let k = p.sc.max(0.01);
    // text() centres the glyphs in a 1.4×size line: the band is centred the same way
    band(window, p, lx + (x0 - 1.) / k, ly + size * 0.06 / k, lx + (x0 + w + 1.) / k, ly + size * 1.34 / k, 3. / k, al(c(th.highlight()), o));
}

/// a rounded rectangle in folder-local coordinates, turned with the folder
fn band(window: &mut Window, p: &Place, l: f32, t: f32, r: f32, b: f32, rad: f32, color: Hsla) {
    let rad = rad.min((b - t) / 2.).min((r - l) / 2.).max(0.);
    let mut pb = PathBuilder::fill();
    pb.move_to(p.pt(l + rad, t));
    pb.line_to(p.pt(r - rad, t));
    pb.arc_to(p.r(rad), px(0.), false, true, p.pt(r, t + rad));
    pb.line_to(p.pt(r, b - rad));
    pb.arc_to(p.r(rad), px(0.), false, true, p.pt(r - rad, b));
    pb.line_to(p.pt(l + rad, b));
    pb.arc_to(p.r(rad), px(0.), false, true, p.pt(l, b - rad));
    pb.line_to(p.pt(l, t + rad));
    pb.arc_to(p.r(rad), px(0.), false, true, p.pt(l + rad, t));
    pb.close();
    if let Ok(path) = pb.build() { window.paint_path(path, color); }
}

fn paint_all(d: &Draw, window: &mut Window, cx: &mut App) {
    let th = &d.th;
    let (x2, bx) = tab_x(d.w);
    let tx = x2 + RT - 2.;
    let (rl, rr, rb) = d.radii;
    // In the stack each folder hides most of the ones behind it: below the body edge of an opaque
    // folder drawn later, nothing of an earlier one shows. Their previews are clipped to what is left,
    // so a frame of the moving stack does not lay out and draw every note's text and pictures in full.
    let mut covers = vec![f32::MAX; d.items.len()];
    let mut cover = f32::MAX;
    for (k, it) in d.items.iter().enumerate().rev() {
        covers[k] = cover;
        let v = it.v;
        if it.kind == 0 && v.o > 0.95 {
            // a leaning folder lifts one side of its edge
            let lean = d.w / 2. * v.sc * v.rot.to_radians().sin().abs() + 2.;
            cover = cover.min(1. + (v.y - v.lift + T) * v.sc - lean);
        }
    }
    for (ix, it) in d.items.iter().enumerate() {
        set_glyph_rotation(None);
        let o = it.v.o.clamp(0., 1.);
        if o < 0.01 { continue; }
        let v = it.v;
        let p = Place { x: 1., y: 1. + (v.y - v.lift) * v.sc, sc: v.sc, w: d.w, rot: v.rot.to_radians(), sx: v.sx };
        let pf = p.flat();
        if v.rot.abs() > 0.001 { let pv = pf.pt(d.w / 2., 0.); set_glyph_rotation(Some((p.rot, f32::from(pv.x), f32::from(pv.y)))); }
        let shaped = it.front;
        let radii = if shaped { (rl, rr, rb.min(rl), rb.min(rr)) } else { (R, R, R, R) };
        if it.dark_w > 0. { paint_dark(window, &p, bx, it.dark_w, al(c(th.tab_dark), o)); }
        paint_folder(window, &p, v.h, c(it.color), c(th.line), (x2, bx), o, radii);
        match it.kind {
            1 => {
                let at = p.pt(tx, T / 2.);
                find_icon(window, f32::from(at.x) + 7.5 * v.sc, f32::from(at.y) + 0.5, 15. * v.sc, 1.6, al(c(th.ink2), o));
                let wd = text_width(window, &it.count, 12.5 * v.sc, FontWeight::SEMIBOLD);
                let at = pf.pt(bx + DARK_SEARCH / 2., 8.);
                text(window, cx, &it.count, point(at.x - px(wd / 2.), at.y), 12.5 * v.sc, FontWeight::SEMIBOLD, al(c(th.on_dark), o), DARK_SEARCH);
                if !it.text.is_empty() {
                    text(window, cx, &it.text, pf.pt(PAD, T + 50.), 13. * v.sc, FontWeight::NORMAL, al(c(th.ink2), o), (d.w - 2. * PAD) * v.sc);
                }
            }
            2 => {
                let at = pf.pt(tx, 7.);
                text(window, cx, &it.title, at, 14.5 * v.sc, FontWeight::SEMIBOLD, al(c(th.ink), o), (bx - 6. - tx) * v.sc);
                if let Some(sd) = &d.set { setui::paint_settings(sd, &p, window, cx); }
            }
            _ => {
                if it.front && d.find_open && d.mode == Mode::Note {
                    let cx0 = 1. + bx + it.dark_w;
                    find_icon(window, 1. + tx + 7.5, 1. + T / 2. + 0.5, 15., 1.6, al(c(th.ink2), o));
                    if it.dark_w > 100. {
                        text(window, cx, &d.find_label, point(px(1. + bx + 8.), px(8.)), 12.5, FontWeight::SEMIBOLD, al(c(th.on_dark), o), it.dark_w - 90.);
                        glyph(window, UP, cx0 - 69., 1. + T / 2., 15., 1.7, al(c(th.on_dark), o), false);
                        glyph(window, DOWN, cx0 - 43., 1. + T / 2., 15., 1.7, al(c(th.on_dark), o), false);
                        glyph(window, CLOSE, cx0 - 17., 1. + T / 2., 15., 1.7, al(c(th.on_dark), o), false);
                    }
                } else {
                    if it.front && d.tab_hl > 0.01 && d.mode == Mode::Note {
                        let a = p.pt(x2 + 5., 5.);
                        window.paint_quad(fill(Bounds::new(a, size(px(bx - x2 - 10.), px(T - 9.))), al(c(th.ink), 0.07 * d.tab_hl * o)).corner_radii(px(9.)));
                    }
                    let at = pf.pt(tx, 7.);
                    if d.mode == Mode::List { mark(window, th, &p, &it.title, (tx, 7.), 14.5 * v.sc, FontWeight::SEMIBOLD, &d.query, None, o, (bx - 6. - tx) * v.sc); }
                    text(window, cx, &it.title, at, 14.5 * v.sc, FontWeight::SEMIBOLD, al(c(th.ink), o), (bx - 6. - tx) * v.sc);
                    if it.front && it.dark_w > 0. {
                        let at = p.pt(bx + it.dark_w / 2., T / 2.);
                        glyph(window, PLUS, f32::from(at.x), f32::from(at.y), 15. * v.sc * d.plus_sc, 2.8, al(c(th.on_dark), o), false);
                    }
                }
                if d.mode == Mode::List {
                    let iw = text_width(window, &it.info, 12.5 * v.sc, FontWeight::NORMAL);
                    let at = pf.pt(d.w - PAD, T + 9.);
                    text(window, cx, &it.info, point(at.x - px(iw), at.y), 12.5 * v.sc, FontWeight::NORMAL, al(c(th.ink3), o), iw + 4.);
                    if !it.ctx.is_empty() {
                        let at = pf.pt(bx + 40., T + 9.);
                        let max = (d.w - PAD - iw - 16. - bx - 40.) * v.sc;
                        mark(window, th, &p, &it.ctx, (bx + 40., T + 9.), 12.5 * v.sc, FontWeight::NORMAL, &d.query, it.ctx_hl, o, max);
                        text(window, cx, &it.ctx, at, 12.5 * v.sc, FontWeight::NORMAL, al(c(th.ink2), o), max);
                    }
                }
                if !it.editor {
                    let at = pf.pt(39., T + 35.5);
                    let tl = pf.pt(0., T + 1.);
                    let bottom = (f32::from(tl.y) + (v.h - T - 4.) * v.sc).min(covers[ix]);
                    let ht = bottom - f32::from(tl.y);
                    if ht < 1. { continue; }
                    let clip = Bounds::new(tl, size(px(d.w * v.sc), px(ht)));
                    d.previews.borrow_mut().entry(it.id.clone()).or_insert_with(crate::editor::Preview::new)
                        // the hit is marked once, on the snippet line: not again in the body below it
                        .paint(&it.text, &d.dir, "", at, (d.w - 78.) * v.sc, d.scale * v.sc, th, o, clip, window, cx);
                }
            }
        }
    }
    set_glyph_rotation(None);
    if let Some(pd) = &d.pal { paint_palette(pd, th, window, cx); }
    // the controls: a paper capsule with line icons
    let a = d.ctl_a.clamp(0., 1.);
    if a < 0.01 { return; }
    let (line, paper) = (c(th.line), c(th.paper()));
    let n = d.cap_n.max(1.);
    let (l, r) = (ctl_x(d.w, 0) - 34. * (n - 1.) - 16. + 1., ctl_x(d.w, 0) + 16. + 1.);
    let y = 1. + T / 2.;
    // the capsule keeps its small soft shadow (the folders have none)
    let (sh, k) = if th.night { (black(), 2.4) } else { (c(0x28261e), 1.0) };
    for i in 0..6 {
        let e = i as f32 * 1.5;
        let mut s = sh; s.a = 0.02 * k * a;
        window.paint_quad(fill(Bounds::new(point(px(l - e), px(y - 14. + 2. - e)), size(px(r - l + 2. * e), px(28. + 2. * e))), s).corner_radii(px(14. + e)));
    }
    window.paint_quad(quad(Bounds::new(point(px(l), px(y - 14.)), size(px(r - l), px(28.))), px(14.), al(paper, 0.94 * a), px(1.), al(line, 0.22 * a), BorderStyle::Solid));
    let f = d.ctl_fade.clamp(0., 1.);
    let on_of = |ctl: Ctl| d.controls.iter().find(|(c, _)| *c == ctl).map_or(false, |(_, on)| *on);
    for k in 0..d.controls.len().max(d.ctl_prev.len()) {
        let slot = (n - k as f32).clamp(0., 1.) * a;
        if slot < 0.01 { continue; }
        let cur = d.controls.get(k).map(|(c, on)| (*c, *on));
        let prev = d.ctl_prev.get(k).copied();
        match (prev, cur) {
            (Some(p), Some((c, on))) if p == c => paint_ctl(window, d, c, on, k, slot, paper, line),
            (p, cur) => {
                if let Some(p) = p { paint_ctl(window, d, p, on_of(p), k, slot * (1. - f), paper, line); }
                if let Some((c, on)) = cur { paint_ctl(window, d, c, on, k, slot * f, paper, line); }
            }
        }
    }
}

fn paint_ctl(window: &mut Window, d: &Draw, ctl: Ctl, on: bool, k: usize, slot: f32, paper: Hsla, line: Hsla) {
    if slot < 0.01 { return; }
    let th = &d.th;
    let y = 1. + T / 2.;
    let x = 1. + ctl_x(d.w, k);
    let col = al(c(if on { th.ink } else { th.ink2 }), slot);
    match ctl {
        Ctl::Close => glyph(window, CLOSE, x, y, 16., 1.5, col, false),
        Ctl::Gear => gear_icon(window, x, y, 16., 1.5, col, al(paper, slot)),
        Ctl::Pin => {
            if d.pinned { glyph(window, I_PIN, x, y, 16., 1.5, col, true); }
            glyph(window, I_PIN, x, y, 16., 1.5, col, false);
        }
        Ctl::List => glyph(window, FOLDERS, x, y, 16., 1.5, col, false),
        Ctl::Find => find_icon(window, x, y, 16., 1.5, col),
        Ctl::New => glyph(window, I_NEW, x, y, 16., 1.6, col, false),
        Ctl::Trash => glyph(window, TRASH, x, y, 16., 1.5, col, false),
        Ctl::More => glyph_x(window, MORE, x, y, 16., 1.6, col, false, (std::f32::consts::PI * d.more_t).cos()),
        Ctl::Dot => circle(window, x, y, 7. * d.dot_sc, Some((1.5, al(line, slot))), Some(al(c(d.dot_color), slot))),
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t_render = std::time::Instant::now();
        RSTART.with(|r| r.set(Some(t_render)));
        let el = self.render_inner(window, cx);
        let ms = t_render.elapsed().as_secs_f32() * 1000.;
        if ms > 4. && (self.rsz.is_some() || std::env::var("NOTEDD_PERF").is_ok()) { dbg_log(&format!("render {ms:.1} ms")); }
        el
    }
}

impl Shell {
    fn render_inner(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let th = self.th;
        // sizing follows the pointer once per frame, not once per mouse event
        if self.rsz.is_some() { self.drag_resize(window); }
        let (me, rsz_on) = (cx.entity(), self.rsz.is_some());
        if self.sync_window(window) {
            self.retarget(false, 0, window, cx);
            if self.rsz.is_some() { for a in self.mo.anims.values_mut() { a.cur = a.to; } }
            for a in self.mo.anims.values_mut() { if a.to.o > 0. && a.to.sc == 1. { a.cur.h = a.to.h; } }
        }
        let vs = window.viewport_size();
        if std::env::var("NOTEDD_PERF").is_ok() { dbg_log(&format!("vp {:?} win_h {} to {} mode {}", vs, self.mo.win_h, self.mo.win_h_to, self.mode as u8)); }
        let (vw, vh) = (f32::from(vs.width), f32::from(vs.height));
        let w = (vw - 2.).max(300.);
        self.mo.last_w = w;
        self.body.update(cx, |b, _| b.avail = Some(w));
        let (x2, bx) = tab_x(w);
        let tx = x2 + RT - 2.;
        let in_note = self.mode == Mode::Note;
        let shown_editor = self.editor_shown();
        let eh = self.edit_h;
        if (!self.mo.running || self.rsz.is_some() || (self.mo.region.0, self.mo.region.1) != (vw as i32, vh as i32)) && self.mo.region != (vw as i32, vh as i32, self.mode as u8 + 10 * shown_editor as u8) {
            self.mo.region = (vw as i32, vh as i32, self.mode as u8 + 10 * shown_editor as u8);
            self.set_region(window);
        }

        // ---- folders from the animation state
        let q = self.search.read(cx).text().trim().to_lowercase();
        let listing = self.mode == Mode::List;
        let total = self.notes.len();
        let nshown = if listing { self.shown(cx).len() } else { 0 };
        let mut ids: Vec<(i32, String)> = self.mo.anims.iter().map(|(k, a)| (a.z, k.clone())).collect();
        ids.sort();
        let mut items = Vec::new();
        for (_, id) in ids {
            let a = self.mo.anims[&id];
            let kind = if id == SEARCH { 1 } else if id == SETTINGS { 2 } else { 0 };
            let color = if kind == 0 { self.mo.colors.get(&id).map_or_else(|| self.th.paper(), |c| motion::join(*c)) } else { th.paper() };
            let mut it = Item { kind, id: id.clone(), v: a.cur, color, title: String::new(), info: String::new(), ctx: String::new(), ctx_hl: None, count: String::new(), text: String::new(), front: false, editor: false, dark_w: 0. };
            match kind {
                1 => {
                    let m = self.mo.list_len;
                    it.count = if nshown > m { format!("{}–{}/{}", self.list_start + 1, self.list_start + m, nshown) } else if q.is_empty() { total.to_string() } else { format!("{nshown}/{total}") };
                    it.dark_w = DARK_SEARCH;
                    // nothing found: say so, and that Enter makes a note of the query
                    if listing && nshown == 0 && !q.is_empty() {
                        it.text = ui::NO_RESULTS.get().replace("{query}", self.search.read(cx).text().trim());
                    }
                }
                2 => { it.title = ui::SETTINGS.get().to_string(); it.front = true; }
                _ => {
                    let Some(i) = self.idx(&id) else { continue };
                    let n = &self.notes[i];
                    let live = if id == self.cur && self.mode == Mode::Note && !self.mo.running {
                        let b = self.body.read(cx);
                        if b.raw { plain(&b.text()) } else { b.text() }
                    } else { String::new() };
                    it.title = if id == self.cur && self.mode == Mode::Note && !self.mo.running { if live.trim().is_empty() { ui::UNTITLED.get().to_string() } else { live.trim().to_string() } }
                        else if n.title().is_empty() { ui::UNTITLED.get().to_string() } else { plain(n.title()) };
                    it.text = if id == self.cur && !self.mo.running && self.dirty { self.body.read(cx).markdown() } else { n.text.clone() };
                    it.info = format!("{} · {}", when(n.mtime), words(n.words()));
                    if !q.is_empty() {
                        let flat = n.body_flat();
                        if let Some(p) = flat.to_lowercase().find(&q) {
                            if flat.is_char_boundary(p) {
                                let start = flat[..p].char_indices().rev().nth(27).map(|(p, _)| p).unwrap_or(0);
                                let pre = if start > 0 { "…" } else { "" };
                                it.ctx = format!("{}{}", pre, &flat[start..]);
                                if p + q.len() <= flat.len() { it.ctx_hl = Some((pre.len() + p - start, q.len())); }
                            }
                        }
                    }
                    if id == self.cur && self.mode != Mode::List {
                        it.front = true;
                        it.dark_w = self.mo.dark.v;
                        it.editor = shown_editor;
                    }
                }
            }
            items.push(it);
        }
        let find_label = {
            let q = self.findbox.read(cx).text();
            if q.is_empty() { String::new() } else {
                let m = self.body.read(cx).matches();
                if m.is_empty() { "0".to_string() } else {
                    let (s, _) = self.body.read(cx).core().sel();
                    let at = m.iter().position(|(b, a, _)| (*b, *a) >= (s.block, s.off)).unwrap_or(0);
                    format!("{}/{}", at + 1, m.len())
                }
            }
        };
        let ctls = self.controls();
        if self.ctl_last.is_empty() { self.ctl_last = ctls.clone(); self.ctl_prev = ctls.clone(); }
        if ctls != self.ctl_last {
            self.ctl_prev = std::mem::replace(&mut self.ctl_last, ctls.clone());
            self.mo.ctl_fade = Ease::at(0.);
            self.mo.ctl_fade.to = 1.;
            self.start_anim(window, cx);
        }
        if window.focused(cx).is_none() { window.focus(&self.focus); }
        if self.mode != Mode::Note || self.find_open { self.tab_hover = false; }
        let want_hl = if self.tab_hover { 1. } else { 0. };
        if self.mo.tab_hl.to != want_hl { self.mo.tab_hl.to = want_hl; self.start_anim(window, cx); }
        let controls: Vec<(Ctl, bool)> = ctls.iter().enumerate().map(|(k, c)| {
            let on = self.hover_ctl == Some(k) || (*c == Ctl::Pin && self.prefs.pinned)
                || matches!((c, self.mode), (Ctl::List, Mode::List) | (Ctl::Gear, Mode::Settings))
                || (*c == Ctl::Find && self.find_open) || (*c == Ctl::More && self.ctl_open) || (*c == Ctl::Dot && self.palette);
            (*c, on)
        }).collect();
        let set_o = self.mo.anims.get(SETTINGS).map_or(0., |a| a.cur.o.clamp(0., 1.));
        let set = if set_o > 0.05 { Some(self.set_draw(window, cx, w, set_o)) } else { None };
        if self.palette != self.pal_prev {
            self.pal_prev = self.palette;
            if self.palette {
                self.pal_t = Ease::at(0.); self.pal_t.to = 1.;
                self.pal_sc = [Ease::at(1.); MAX_CARDS];
                self.pal_hover = None;
                self.start_anim(window, cx);
            }
        }
        let listing_now = self.mode == Mode::List;
        let pal_n = self.prefs.ncards;
        let pal_w = pal_n as f32 * 32. + (pal_n as f32 - 1.) * 4. + 12.;
        let (pw, ph) = if listing_now { (pal_w.max(184.), 88.) } else { (pal_w, 44.) };
        let dot_k = ctls.iter().position(|c| *c == Ctl::Dot);
        let pal_open = self.palette && (listing_now || (self.mode == Mode::Note && dot_k.is_some()));
        let pal_id = if listing_now { self.pal_id.clone() } else { self.cur.clone() };
        let pal_left = {
            let ax = self.pal_at.0;
            (ax - pw / 2.).clamp(0., (w - pw).max(0.))
        };
        let pal_top = if listing_now { self.pal_at.1.clamp(6., (vh - ph - 10.).max(6.)) } else { T + 8. };
        let pal = if pal_open {
            let chosen = self.idx(&pal_id).map_or(0, |i| self.notes[i].color);
            Some(PalDraw { x0: 1. + pal_left, y0: 1. + pal_top, pw, ph, t: self.pal_t.v, chosen, cols: (0..pal_n).map(|i| self.th.note(i)).collect(),
                sc: (0..pal_n).map(|i| self.pal_sc[i].v).collect(), listing: listing_now, del_hover: self.pal_hover == Some(100) })
        } else { None };
        let search_a = self.mo.anims.get(SEARCH).copied().unwrap_or_default();
        let draw = Draw {
            th, w, mode: self.mode, find_open: self.find_open, find_label, items, controls, ctl_prev: self.ctl_prev.clone(), ctl_fade: self.mo.ctl_fade.v, query: if listing { q.clone() } else { String::new() }, pinned: self.prefs.pinned, plus_sc: self.mo.plus_sc.v, cap_n: self.mo.cap_n.v, ctl_a: self.mo.ctl_a.v, more_t: self.mo.more_t.v, dot_sc: self.mo.dot_sc.v, tab_hl: self.mo.tab_hl.v,
            dot_color: self.note_color(), radii: (self.mo.rl.v, self.mo.rr.v, self.mo.rb.v), dir: self.dir.clone(),
            scale: self.prefs.scale as f32 / 100., previews: self.mo.previews.clone(), set: set.clone(), pal: pal.clone(),
        };

        let mut root = div()
            .id("shell")
            .size_full()
            .relative()
            .on_mouse_down(MouseButton::Left, cx.listener(|s, _, w, cx| { if !s.focus.contains_focused(w, cx) { w.focus(&s.focus); } }))
            .key_context("Shell")
            .track_focus(&self.focus)
            .text_color(c(th.ink))
            .font_family(crate::layout::BODY)
            .capture_key_down(cx.listener(|s, ev: &KeyDownEvent, w, cx| {
                let Some(i) = s.capture else {
                    if s.mode == Mode::Settings && s.settings_key(ev, w, cx) { cx.stop_propagation(); }
                    if s.mode == Mode::List && s.list_key(ev, w, cx) { cx.stop_propagation(); }
                    return;
                };
                cx.stop_propagation();
                let key = ev.keystroke.key.as_str();
                let mods = system::mods_of(&ev.keystroke.modifiers);
                if key == "escape" { s.capture = None; return cx.notify(); }
                if key == "backspace" && mods == 0 { s.prefs.keys[i] = (0, 0); s.capture = None; return s.apply_prefs(w, cx); }
                let Some((mods, vk)) = system::shortcut_key(key, mods) else { return };
                let fkey = (0x70..=0x87).contains(&vk);
                if mods == 0 && (i < 2 || !fkey) { return; }
                s.prefs.keys[i] = (mods, vk);
                for j in 0..KEYS { if j != i && s.prefs.keys[j] == (mods, vk) { s.prefs.keys[j] = (0, 0); } }
                s.capture = None;
                s.apply_prefs(w, cx);
            }))
            .on_scroll_wheel(cx.listener(|s, ev: &ScrollWheelEvent, w, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                if ev.modifiers.control {
                    // zoom is handled in the capture phase (see the canvas below)
                } else if s.mode == Mode::Settings && dy != 0. {
                    let end = s.set_geom(w, f32::from(w.viewport_size().width) - 2.).1;
                    s.scroll_settings(-dy * 2.5, end, cx);
                    s.start_anim(w, cx);
                } else if s.mode == Mode::List && dy != 0. {
                    s.palette = false;
                    s.retarget(false, if dy > 0. { -1 } else { 1 }, w, cx);
                }
            }))
            .on_action(cx.listener(|s, _: &NewNote, w, cx| s.new_note("", w, cx)))
            .on_action(cx.listener(|s, _: &AllNotes, w, cx| s.toggle_list(w, cx)))
            .on_action(cx.listener(|s, _: &FindInNote, w, cx| s.open_find(w, cx)))
            .on_action(cx.listener(|s, _: &OpenSettings, w, cx| s.open_settings(w, cx)))
            .on_action(cx.listener(|s, _: &TogglePin, w, cx| s.pin(w, cx)))
            .on_action(cx.listener(|s, _: &QuitApp, _, cx| s.quit(cx)))
            .on_action(cx.listener(|s, _: &NoteColor, w, cx| if s.mode == Mode::Note { s.pal_at = (s.mo.last_w / 2., T + 8.); s.palette = !s.palette; s.sync_caps(w, cx); }))
            .on_action(cx.listener(|s, _: &DeleteNote, w, cx| s.delete_by_key(w, cx)))
            .on_action(cx.listener(|s, _: &ZoomIn, w, cx| s.zoom(10, w, cx)))
            .on_action(cx.listener(|s, _: &ZoomOut, w, cx| s.zoom(-10, w, cx)))
            .on_action(cx.listener(|s, _: &ZoomReset, w, cx| s.zoom(0, w, cx)))
            .on_action(cx.listener(|s, _: &Escape, w, cx| s.escape(w, cx)))
            .on_action(cx.listener(|s, _: &NextPage, w, cx| if s.mode == Mode::Settings { s.turn_page(1, cx); s.focus_main(w, cx); }))
            .on_mouse_move(cx.listener(|s, ev: &MouseMoveEvent, w, cx| if s.rsz.is_some() { } else if s.st.thumb_drag.is_some() || s.st.pick_drag.is_some() { s.set_pointer(ev.position, false, w, cx); }))
            .on_mouse_up(MouseButton::Left, cx.listener(|s, ev: &MouseUpEvent, w, cx| {
                if s.rsz.take().is_some() { s.update_docked(w, cx); }
                if s.mo.plus_sc.to != 1. { s.mo.plus_sc.to = 1.; s.start_anim(w, cx); }
                if s.st.thumb_drag.is_some() || s.st.pick_drag.is_some() { s.set_pointer(ev.position, true, w, cx); }
            }))
            .child(canvas(|_, _, _| (), move |_, _, window, cx| {
                let t0 = std::time::Instant::now();
                // Ctrl+wheel zooms everywhere, before the note's own scroll area can take the wheel
                let m = me.clone();
                window.on_mouse_event(move |ev: &ScrollWheelEvent, phase, w, cx| {
                    if phase != DispatchPhase::Capture || !ev.modifiers.control { return; }
                    let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                    cx.stop_propagation();
                    if dy != 0. { m.update(cx, |s, cx| s.zoom(if dy > 0. { 10 } else { -10 }, w, cx)); }
                });
                if rsz_on {
                    // sizing goes on while the pointer is outside the window too: listen at window level
                    let m = me.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, _, w, cx| {
                        m.update(cx, |s, cx| if s.rsz.is_some() {
                            if ev.pressed_button != Some(MouseButton::Left) { s.rsz = None; s.update_docked(w, cx); }
                            cx.notify();
                        });
                    });
                    let m = me.clone();
                    // sizing is over: the edges the window now touches square off (and lose their grips)
                    window.on_mouse_event(move |_: &MouseUpEvent, _, w, cx| { m.update(cx, |s, cx| { if s.rsz.take().is_some() { s.update_docked(w, cx); } cx.notify(); }); });
                }
                paint_all(&draw, window, cx);
                perf(t0.elapsed().as_secs_f32() * 1000.);
            }).absolute().size_full());

        // the tab drags the window; a click on it opens the drawer
        if self.mode != Mode::List && !(self.find_open && in_note) {
            root = root.child(div().id("tab").absolute().left(px(1. + x2)).top(px(1.)).w(px(bx - x2)).h(px(T)).cursor_pointer()
                .on_hover(cx.listener(|s, h: &bool, _, cx| { s.tab_hover = *h; cx.notify(); }))
                .on_mouse_down(MouseButton::Left, cx.listener(|s, _, w, cx| s.start_drag(w, cx))));
        }
        match self.mode {
            Mode::Note => {
                if self.find_open && self.mo.dark.v > 100. {
                    let c0 = 1. + bx + DARK_FIND;
                    root = root.child(div().absolute().left(px(1. + tx + 23.)).top(px(1.)).w(px(bx - 6. - tx - 23.)).h(px(T)).child(self.findbox.clone()));
                    for (i, (dx, up)) in [(69., Some(true)), (43., Some(false)), (17., None)].into_iter().enumerate() {
                        root = root.child(div().id(("fbtn", i)).absolute().left(px(c0 - dx - 13.)).top(px(1.)).w(px(26.)).h(px(T)).cursor_pointer()
                            .on_mouse_down(MouseButton::Left, cx.listener(move |s, _, w, cx| match up {
                                Some(back) => s.body.update(cx, |b, cx| b.find_step(back, w, cx)),
                                None => s.close_find(w, cx),
                            })));
                    }
                } else if !self.find_open {
                    root = root.child(div().id("plus").absolute().left(px(1. + bx)).top(px(1.)).w(px(DARK_HOVER)).h(px(T)).cursor_pointer()
                        .on_hover(cx.listener(|s, h: &bool, w, cx| { if s.hover_plus != *h { s.hover_plus = *h; s.mo.dark.to = if *h { DARK_HOVER } else { DARK }; s.start_anim(w, cx); } }))
                        
                        .on_mouse_down(MouseButton::Left, cx.listener(|s, _, w, cx| { s.mo.plus_sc.to = 0.85; s.start_anim(w, cx); }))
                        .on_mouse_up(MouseButton::Left, cx.listener(|s, _, w, cx| { s.mo.plus_sc.to = 1.; s.new_note("", w, cx); })));
                }
                if shown_editor {
                    root = root.child(div().id("body-box").absolute().left(px(1.)).top(px(1. + T)).w(px(w)).h(px(eh - T - 10.))
                        .on_mouse_down(MouseButton::Right, cx.listener(|s, ev: &MouseDownEvent, w, cx| {
                            if s.mode == Mode::Note {
                                let p = point(ev.position.x - px(1.), ev.position.y - px(1.));
                                s.open_emenu(p, w, cx);
                            }
                        }))
                        .child(self.body.clone()));
                }
            }
            Mode::Settings => {
                let settled = self.mo.anims.get(SETTINGS).is_some_and(|a| a.cur.near(&a.to)) && !self.mo.hiding;
                if settled {
                    if let Some(sd) = &set { for e in self.set_hits(sd, cx) { root = root.child(e); } }
                }
            }
            Mode::List => {
                let sc = search_a.cur.sc;
                let (st, k) = (1. + search_a.cur.y * sc, 0.92 * sc);
                self.search.update(cx, |e, _| e.scale = k);
                let x0 = 1. + w / 2. + (tx + 23. - w / 2.) * sc;
                if search_a.cur.o > 0.02 {
                    root = root.child(div().absolute().left(px(x0)).top(px(st)).w(px((bx - 6. - tx - 23.) * sc)).h(px(T * sc)).opacity(search_a.cur.o.clamp(0., 1.).powi(4)).child(self.search.clone()));
                }
                let n_vis = self.mo.shown_ids.len();
                let step = if n_vis > 1 {
                    let a0 = self.mo.anims.get(&self.mo.shown_ids[0]).map_or(0., |a| a.cur.y);
                    let a1 = self.mo.anims.get(&self.mo.shown_ids[1]).map_or(0., |a| a.cur.y);
                    (a0 - a1).max(20.)
                } else { 40. };
                for (i, id) in self.mo.shown_ids.clone().into_iter().enumerate() {
                    let Some(a) = self.mo.anims.get(&id) else { continue };
                    let (left, top, bw) = (1. + w / 2. * (1. - a.cur.sc), 1. + (a.cur.y - a.cur.lift) * a.cur.sc, w * a.cur.sc);
                    let bh = if i == 0 { HF * a.cur.sc } else { step * a.cur.sc };
                    let pos = self.list_start + i;
                    root = root.child(self.folder_band(("band", pos), left, top, bw, bh)
                        .on_hover(cx.listener(move |s, h: &bool, w, cx| if *h && s.sel != pos { s.sel = pos; s.retarget(false, 0, w, cx); }))
                        .on_mouse_down(MouseButton::Left, cx.listener(move |s, _, w, cx| {
                            let shown = s.shown(cx);
                            if let Some(&i) = shown.get(pos) { let id = s.notes[i].id.clone(); s.open_note(&id, w, cx); }
                        }))
                        .on_mouse_down(MouseButton::Right, cx.listener(move |s, ev: &MouseDownEvent, w, cx| {
                            let shown = s.shown(cx);
                            if let Some(&i) = shown.get(pos) {
                                s.pal_id = s.notes[i].id.clone();
                                s.pal_at = (f32::from(ev.position.x) - 1., f32::from(ev.position.y) - 1.);
                                s.palette = true;
                                s.sync_caps(w, cx);
                            }
                        })));
                }
            }
        }
        // the buttons
        for (k, ctl) in ctls.iter().copied().enumerate() {
            root = root.child(div().id(("ctl", k)).absolute().left(px(1. + ctl_x(w, k) - 17.)).top(px(1. + T / 2. - 14.)).w(px(34.)).h(px(28.))
                .cursor_pointer()
                .on_hover(cx.listener(move |s, h: &bool, w, cx| {
                    let nv = if *h { Some(k) } else if s.hover_ctl == Some(k) { None } else { s.hover_ctl };
                    if nv != s.hover_ctl {
                        s.hover_ctl = nv;
                        let dot = nv.is_some_and(|k| s.controls().get(k) == Some(&Ctl::Dot));
                        s.mo.dot_sc.to = if dot { 1.15 } else { 1. };
                        s.start_anim(w, cx);
                        cx.notify();
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(move |s, _, w, cx| { cx.stop_propagation(); s.press(ctl, w, cx); })));
        }
        if pal_open {
            let dot_x = dot_k.map(|k| 1. + ctl_x(w, k));
            let mut boxd = div().id("palette").absolute().left(px(1. + pal_left)).top(px(1. + pal_top)).w(px(pw)).h(px(ph))
                .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.stop_propagation()))
                .on_mouse_down_out(cx.listener(move |s, ev: &MouseDownEvent, w, cx| {
                    if let Some(dx) = dot_x {
                        if (f32::from(ev.position.x) - dx).abs() < 17. && f32::from(ev.position.y) < 1. + T { return; }
                    }
                    s.palette = false; s.sync_caps(w, cx);
                }));
            for i in 0..pal_n {
                let left = (pw - pal_w) / 2. + 6.;
                boxd = boxd.child(div().id(("pal", i)).absolute().left(px(left + 36. * i as f32)).top(px(6.)).w(px(32.)).h(px(32.)).cursor_pointer()
                    .on_hover(cx.listener(move |s, h: &bool, w, cx| {
                        s.pal_sc[i].to = if *h { 1.12 } else { 1. };
                        s.start_anim(w, cx);
                    }))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |s, _, w, cx| {
                        let id = if s.mode == Mode::List { s.pal_id.clone() } else { s.cur.clone() };
                        if let Some(j) = s.idx(&id) {
                            s.notes[j].color = i;
                            if !s.notes[j].is_empty() { let _ = store::save(&s.dir, &s.notes[j]); }
                        }
                        s.palette = false;
                        s.sync_caps(w, cx);
                    })));
            }
            if listing_now {
                boxd = boxd.child(div().id("pal-del").absolute().left(px(5.)).top(px(48.)).w(px(pw - 10.)).h(px(ph - 53.)).cursor_pointer()
                    .on_hover(cx.listener(|s, h: &bool, _, cx| { let v = if *h { Some(100) } else { None }; if s.pal_hover != v { s.pal_hover = v; cx.notify(); } }))
                    .on_mouse_down(MouseButton::Left, cx.listener(|s, _, w, cx| {
                        let id = s.pal_id.clone();
                        s.palette = false;
                        s.delete_note(&id, w, cx);
                    })));
            }
            root = root.child(boxd);
        }
        // the edges that resize the window
        let settings_up = self.mode == Mode::Settings && self.mo.anims.get(SETTINGS).is_some_and(|a| a.cur.near(&a.to)) && !self.mo.hiding;
        if (shown_editor || settings_up) && !self.find_open {
            let (docks, g) = (self.mo.docks, 7.);
            let edge = |id: &'static str, l: f32, t: f32, wd: f32, ht: f32, cur: CursorStyle, hit: usize, cx: &mut Context<Self>| {
                div().id(id).absolute().left(px(1. + l)).top(px(1. + t)).w(px(wd)).h(px(ht)).cursor(cur)
                    .on_mouse_down(MouseButton::Left, cx.listener(move |s, _, _, cx| { cx.stop_propagation(); s.begin_resize(hit); cx.notify(); }))
            };
            let (hh, ww) = (eh, w);
            if docks & 1 == 0 { root = root.child(edge("e-l", 0., T + R, g, hh - T - R - g, CursorStyle::ResizeLeftRight, 10, cx)); }
            if docks & 2 == 0 { root = root.child(edge("e-r", ww - g, T + R, g, hh - T - R - g, CursorStyle::ResizeLeftRight, 11, cx)); }
            if docks & 4 == 0 {
                root = root.child(edge("e-b", g, hh - g, ww - 2. * g, g, CursorStyle::ResizeUpDown, 15, cx));
                let k = 18.;
                if docks & 1 == 0 { root = root.child(edge("e-bl", 0., hh - k, k, k, CursorStyle::ResizeUpRightDownLeft, 16, cx)); }
                if docks & 2 == 0 { root = root.child(edge("e-br", ww - k, hh - k, k, k, CursorStyle::ResizeUpLeftDownRight, 17, cx)); }
            }
            // the folder's top edge on both sides of the tab
            if docks & 8 == 0 && (self.mode == Mode::Note || settings_up) {
                let k = 18.;
                if x2 > k + 8. { root = root.child(edge("e-tl", k, T - 3., x2 - k - 8., g, CursorStyle::ResizeUpDown, 12, cx)); }
                if ww - bx - DARK_FIND > 2. * k { root = root.child(edge("e-tr", bx + DARK_FIND, T - 3., ww - bx - DARK_FIND - k, g, CursorStyle::ResizeUpDown, 12, cx)); }
                if docks & 1 == 0 { root = root.child(edge("e-ctl", 0., T - 3., k, k + 3., CursorStyle::ResizeUpLeftDownRight, 13, cx)); }
                if docks & 2 == 0 { root = root.child(edge("e-ctr", ww - k, T - 3., k, k + 3., CursorStyle::ResizeUpRightDownLeft, 14, cx)); }
            }
            if self.mode == Mode::Note || settings_up { root = root.child(edge("e-t", x2, 0., bx - x2, g, CursorStyle::ResizeUpDown, 12, cx)); }
        }
        if self.mode != Mode::Note { self.emenu = None; }
        if let Some(e) = self.emenu_el(window, cx) { root = root.child(e); }
        if let Some(t) = &self.toast {
            let has_undo = self.deleted.is_some() && self.mode != Mode::List;
            root = root.child(div().absolute().bottom(px(32.)).w_full().flex().justify_center().child(
                div().id("toast").px(px(16.)).h(px(36.)).flex().items_center().rounded(px(18.)).bg(c(th.tab_dark)).text_color(c(th.on_dark)).text_size(px(13.))
                    .when(has_undo, |d| d.cursor_pointer().on_mouse_down(MouseButton::Left, cx.listener(|s, _, w, cx| s.undo_delete(w, cx))))
                    .child(t.clone()),
            ));
        }
        root
    }
}
