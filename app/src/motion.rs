//! Motion: folders that glide between the editor and the drawer, the window that slides in and out,
//! the magnet at the screen edges. Same numbers and the same easing as the Direct2D app.

use super::*;
use crate::editor::Preview;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;
use crate::platform::{self, Pt, Rect};

pub const SEARCH: &str = "__search";
pub const SETTINGS: &str = "__settings";
const GLIDE: f32 = 16.;
const ZONE: f32 = 64.;

#[derive(Clone, Copy, Default)]
pub struct V { pub y: f32, pub sc: f32, pub h: f32, pub o: f32, pub lift: f32, pub rot: f32, pub sx: f32 }

fn hash(s: &str) -> u32 { s.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619)) }

/// the picked folder leans a little, a different way for each note: (degrees, shift)
pub fn tilt(id: &str) -> (f32, f32) {
    let h = hash(id);
    let sgn = if h & 1 == 0 { -1.0 } else { 1.0 };
    (sgn * (0.7 + ((h >> 1) % 100) as f32 / 100.0 * 1.1), sgn * (3.0 + ((h >> 8) % 100) as f32 / 100.0 * 7.0))
}

impl V {
    fn lerp(&mut self, to: &V, k: f32, ko: f32) {
        self.y += (to.y - self.y) * k; self.sc += (to.sc - self.sc) * k; self.h += (to.h - self.h) * k;
        self.o += (to.o - self.o) * ko; self.lift += (to.lift - self.lift) * k;
        self.rot += (to.rot - self.rot) * k; self.sx += (to.sx - self.sx) * k;
    }
    pub fn near(&self, to: &V) -> bool {
        (self.y - to.y).abs() < 0.3 && (self.sc - to.sc).abs() < 0.001 && (self.h - to.h).abs() < 0.3
            && (self.o - to.o).abs() < 0.01 && (self.lift - to.lift).abs() < 0.2 && (self.rot - to.rot).abs() < 0.02 && (self.sx - to.sx).abs() < 0.2
    }
}

#[derive(Clone, Copy, Default)]
pub struct Anim { pub cur: V, pub to: V, pub z: i32, pub delay: f32 }

#[derive(Clone, Copy)]
pub struct Ease { pub v: f32, pub to: f32 }

impl Ease {
    pub const fn at(v: f32) -> Ease { Ease { v, to: v } }
    pub fn step(&mut self, k: f32, eps: f32) -> bool {
        self.v += (self.to - self.v) * k;
        if (self.v - self.to).abs() < eps { self.v = self.to; true } else { false }
    }
}

struct Drag { c0: Pt, r0: Rect, moved: bool }

pub struct Motion {
    pub anims: HashMap<String, Anim>,
    pub colors: HashMap<String, [f32; 3]>,
    pub dark: Ease,
    pub ctl_a: Ease,
    pub cap_n: Ease,
    pub more_t: Ease,
    pub last_w: f32,
    pub ctl_fade: Ease,
    pub plus_sc: Ease,
    pub demo: bool,
    pub tab_hl: Ease,
    pub dot_sc: Ease,
    pub win_h: f32,
    pub win_h_to: f32,
    pub win_x: f32,
    pub win_y: f32,
    pub win_y_to: f32,
    pub win_x_to: f32,
    x_glide: bool,
    pub slide: (Ease, Ease),
    #[cfg(target_os = "macos")]
    pub window_alpha: Ease,
    pub hiding: bool,
    pub running: bool,
    tick: Instant,
    task: Option<Task<()>>,
    pub edit_top: i32,
    pub docks: u8,
    pub rl: Ease,
    pub rr: Ease,
    pub rb: Ease,
    drag: Option<Drag>,
    pub previews: Rc<RefCell<HashMap<String, Preview>>>,
    /// The card in front while another card comes forward from underneath it.
    pub swap_out: Option<String>,
    pub list_len: usize,
    pub region: (i32, i32, u8),
    pub shown_ids: Vec<String>,
    /// when the running animation began, and when the recent ones began (for the watchdog)
    anim_since: Instant,
    starts: Vec<Instant>,
}

impl Motion {
    pub fn dragging(&self) -> bool { self.drag.is_some() }

    pub fn new(edit_h: f32) -> Motion {
        let mut anims = HashMap::new();
        anims.insert(SEARCH.to_string(), Anim { cur: V { y: 18., sc: 0.955, h: edit_h, ..Default::default() }, ..Default::default() });
        anims.insert(SETTINGS.to_string(), Anim { cur: V { y: 18., sc: 0.955, h: edit_h, ..Default::default() }, ..Default::default() });
        Motion {
            anims, colors: HashMap::new(), dark: Ease::at(DARK), ctl_a: Ease::at(1.), cap_n: Ease::at(4.), more_t: Ease::at(0.), last_w: 800., ctl_fade: Ease::at(1.), plus_sc: Ease::at(1.), demo: false, tab_hl: Ease::at(0.), dot_sc: Ease::at(1.),
            win_h: edit_h + PADB, win_h_to: edit_h + PADB, win_x: 0., win_y: 0., win_y_to: 0., win_x_to: 0., x_glide: false,
            slide: (Ease::at(0.), Ease::at(0.)), hiding: false, running: false, tick: Instant::now(), task: None,
            #[cfg(target_os = "macos")]
            window_alpha: Ease::at(1.),
            edit_top: 0, docks: 0, rl: Ease::at(R), rr: Ease::at(R), rb: Ease::at(R), drag: None,
            previews: Rc::new(RefCell::new(HashMap::new())), swap_out: None, list_len: 0, region: (0, 0, 255), shown_ids: Vec::new(), anim_since: Instant::now(), starts: Vec::new(),
        }
    }
}

fn split(hex: u32) -> [f32; 3] { [((hex >> 16) & 255) as f32, ((hex >> 8) & 255) as f32, (hex & 255) as f32] }
pub fn join(c: [f32; 3]) -> u32 { ((c[0].round() as u32) << 16) | ((c[1].round() as u32) << 8) | c[2].round() as u32 }

impl Shell {
    /// Exchange two cards: the incoming one peeks out below the outgoing card.
    pub fn swap_cards(&mut self, outgoing: &str, incoming: &str) {
        if outgoing == incoming { return; }
        let h = self.edit_h;
        if let Some(a) = self.mo.anims.get_mut(incoming) {
            a.cur = V { y: 14., sc: 0.93, h, o: 1., rot: 0.8, sx: 4., ..Default::default() };
            a.to = V { y: 0., sc: 1., h, o: 1., ..Default::default() };
            a.z = 1000;
            a.delay = 0.;
        }
        if let Some(a) = self.mo.anims.get_mut(outgoing) {
            a.to = V { y: 8., sc: 0.93, h, o: 0., rot: -0.8, sx: -4., ..Default::default() };
            a.z = 1500;
            a.delay = 0.;
            self.mo.swap_out = Some(outgoing.to_string());
        }
    }

    fn dpi(&self, window: &Window) -> f32 { window.scale_factor() }

    pub fn work_area(&self) -> Rect { self.win.work_area() }
    fn monitor_rect(&self) -> Rect { self.win.monitor_rect() }

    /// Dock to the actual display edges on macOS; retain the usable work area elsewhere.
    fn dock_area(&self) -> Rect {
        #[cfg(target_os = "macos")]
        { self.win.monitor_rect() }
        #[cfg(not(target_os = "macos"))]
        { self.work_area() }
    }

    fn drag_dock_area(&self) -> Rect {
        #[cfg(target_os = "macos")]
        { self.win.pointer_monitor_rect() }
        #[cfg(not(target_os = "macos"))]
        { self.dock_area() }
    }

    /// the visible window: the client area, in screen pixels (what the folder is drawn in)
    pub fn win_rect(&self) -> Rect { self.win.client_rect() }

    /// colour a folder is meant to have
    fn target_color(&self, id: &str) -> u32 {
        if id == SEARCH || id == SETTINGS { return self.th.paper(); }
        self.idx(id).map_or(self.th.paper(), |i| self.th.note(self.notes[i].color))
    }

    fn rate(&self) -> f32 { [24.0, 14.0, 8.5][self.prefs.speed.min(2) as usize] }
    pub fn fades(&self) -> bool { self.prefs.dir == 4 || !self.prefs.far }

    fn slide_offset(&self, wr: &Rect, scale: f32) -> (f32, f32) {
        let near = 90.0 * scale;
        let m = self.monitor_rect();
        let far = self.prefs.far;
        match self.prefs.dir {
            0 => (0.0, if far { (m.bottom - wr.top + 8) as f32 } else { near }),
            1 => (if far { (m.right - wr.left + 8) as f32 } else { near }, 0.0),
            2 => (if far { -((wr.right - m.left + 8) as f32) } else { -near }, 0.0),
            3 => (0.0, if far { -((wr.bottom - m.top + 8) as f32) } else { -near }),
            _ => (0.0, 0.0),
        }
    }

    /// moves and sizes the window from the animated numbers
    fn apply_window(&self, window: &Window) {
        let s = self.dpi(window);
        let r = self.win_rect();
        let (x, y) = (self.mo.win_x.round() as i32 + self.mo.slide.0.v.round() as i32, self.mo.win_y.round() as i32 + self.mo.slide.1.v.round() as i32);
        let h = (self.mo.win_h * s).round() as i32;
        let w = r.right - r.left;
        if (r.left, r.top, r.bottom - r.top) == (x, y, h) { return; }
        let t0 = Instant::now();
        self.win.set_client_rect(Rect { left: x, top: y, right: x + w, bottom: y + h });
        if std::env::var("NOTEDD_PERF").is_ok() { let ms = t0.elapsed().as_secs_f32() * 1000.; if ms > 4. { crate::shell::dbg_log(&format!("SetWindowPos {ms:.1} ms h={h}")); } }
    }

    /// follow the window when the user moved or sized it
    /// true when the user changed the folder's height
    pub fn sync_window(&mut self, window: &Window) -> bool {
        if (self.mo.running && self.rsz.is_none()) || self.mo.hiding || self.mo.drag.is_some() || !self.win.visible() { return false; }
        let s = self.dpi(window);
        let r = self.win_rect();
        self.mo.win_x = r.left as f32;
        self.mo.win_x_to = self.mo.win_x;
        self.mo.win_y = r.top as f32;
        self.mo.win_y_to = r.top as f32;
        self.mo.win_h = (r.bottom - r.top) as f32 / s;
        self.mo.win_h_to = self.mo.win_h;
        if self.mode != Mode::List {
            self.mo.edit_top = r.top;
            let eh = (self.mo.win_h - PADB).max(250.);
            if (eh - self.edit_h).abs() > 0.5 { self.edit_h = eh; return true; }
        }
        false
    }

    // ---- targets ----------------------------------------------------------------

    pub fn start_anim(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mo.running { return; }
        self.mo.running = true;
        self.mo.tick = Instant::now();
        self.mo.anim_since = self.mo.tick;
        // animations that keep restarting themselves draw every frame too: note it once in a while
        let now = self.mo.tick;
        self.mo.starts.retain(|t| now - *t < Duration::from_secs(2));
        self.mo.starts.push(now);
        if self.mo.starts.len() == 40 { self.anim_report("restarted 40 times in 2 s", window); }
        self.mo.task = Some(cx.spawn_in(window, async move |this, cx| {
            // one step per drawn frame: a timer here only ticks every 15 ms on Windows
            loop {
                let (tx, rx) = async_channel::bounded::<()>(1);
                let queued = this.update_in(cx, |_, window, cx| {
                    cx.notify();
                    window.on_next_frame(move |_, _| { let _ = tx.try_send(()); });
                });
                if queued.is_err() { break; }
                let _ = rx.recv().with_timeout(Duration::from_millis(100), cx.background_executor()).await;
                match this.update_in(cx, |s, window, cx| s.step_anim(window, cx)) {
                    Ok(false) => {}
                    _ => break,
                }
            }
        }));
    }

    fn eh(&self) -> f32 { self.edit_h }

    /// the folders, the window and the tab go where the mode wants them; `scroll` moves the drawer's window
    pub fn retarget(&mut self, stagger: bool, scroll: isize, window: &mut Window, cx: &mut Context<Self>) {
        let s = self.dpi(window);
        let work = self.work_area();
        let work_h = (work.bottom - work.top) as f32 / s;
        let eh = self.eh();
        let listing = self.mode == Mode::List;
        let shown: Vec<String> = if listing { self.shown(cx).iter().map(|&i| self.notes[i].id.clone()).collect() } else { Vec::new() };
        let base_h = HF + LIST_TOP + LIST_BOTTOM;
        let min_step = T + 6.;
        let capacity = (((work_h - base_h) / min_step) as usize).clamp(1, 10);
        let old_start = self.list_start;
        let count = shown.len();
        self.mo.list_len = count.min(capacity);
        self.sel = self.sel.min(count.saturating_sub(1));
        let max_start = count.saturating_sub(self.mo.list_len);
        self.list_start = if scroll != 0 {
            self.list_start.saturating_add_signed(scroll).min(max_start)
        } else {
            self.list_start.min(max_start).clamp(self.sel.saturating_add(1).saturating_sub(capacity), self.sel)
        };
        if self.mo.list_len != 0 { self.sel = self.sel.clamp(self.list_start, self.list_start + self.mo.list_len - 1); }
        let visible: Vec<String> = shown[self.list_start.min(count)..(self.list_start + self.mo.list_len).min(count)].to_vec();
        self.mo.shown_ids = visible.clone();
        let direction = (self.list_start as isize - old_start as isize).signum() as f32;
        let m = visible.len() as f32;
        let max_h = (work_h * 0.86).max(480.).min(work_h);
        let step = ((max_h - base_h) / m.max(1.)).clamp(min_step, 50.);
        let list_h = m * step + base_h;
        self.mo.win_h_to = if listing { list_h } else { eh + PADB };
        self.mo.win_y_to = if listing {
            (self.mo.edit_top as f32).min(work.bottom as f32 - (list_h * s).round()).max(work.top as f32)
        } else { self.mo.edit_top as f32 };
        if self.mo.drag.is_some() { self.mo.win_y_to = self.mo.win_y; }
        let cur = self.cur.clone();
        let anims = &mut self.mo.anims;
        let window_changed = listing && (visible.iter().any(|id| anims.get(id).is_none_or(|a| a.to.o == 0.))
            || anims.iter().any(|(id, a)| id != SEARCH && id != SETTINGS && a.to.o > 0. && !visible.contains(id)));
        if window_changed { anims.retain(|id, a| id == SEARCH || id == SETTINGS || a.to.o > 0. || visible.contains(id)); }
        let hidden = V { y: 18., sc: LIST_SC, h: eh, ..Default::default() };
        for (id, a) in anims.iter_mut() {
            if id == SEARCH || id == SETTINGS || visible.contains(id) { continue; }
            if !listing && *id == cur { continue; }
            if listing && a.to.o == 0. { continue; }
            a.to = if listing { V { y: (a.cur.y + step * direction).max(LIST_TOP), lift: 0., o: 0., rot: 0., sx: 0., ..a.cur } } else { hidden };
            a.delay = 0.;
            if listing && direction <= 0. { a.z = -1; }
            if !listing { a.z = 999; }
        }
        for (i, id) in visible.iter().enumerate() {
            let sf = i as f32;
            let picked = self.list_start + i == self.sel;
            let (rot, sx) = if picked { tilt(id) } else { (0., 0.) };
            let to = V { y: (m - sf) * step + LIST_TOP, sc: LIST_SC * PERSP / (PERSP + sf * DEPTH), h: HF, o: 1., lift: if picked { 14. } else { 0. }, rot, sx };
            let from = if direction != 0. { V { y: (to.y - step * direction).max(LIST_TOP + 8.), o: 0., ..to } } else { hidden };
            let a = anims.entry(id.clone()).or_insert(Anim { cur: from, ..Default::default() });
            a.to = to;
            a.z = 100 - i as i32;
            if stagger { a.delay = (m - sf) * 0.018; } else if window_changed { a.delay = 0.; }
        }
        if !listing {
            let a = anims.entry(cur.clone()).or_insert(Anim { cur: hidden, ..Default::default() });
            a.to = V { y: 0., sc: 1., h: eh, o: if self.mode == Mode::Settings { 0. } else { 1. }, ..Default::default() };
            a.z = 1000;
            a.delay = 0.;
        }
        let sc_search = if listing { LIST_SC * PERSP / (PERSP + m * DEPTH) } else { 0.955 };
        let st = anims.entry(SEARCH.to_string()).or_insert_with(|| Anim { cur: V { y: 18., sc: 0.955, h: eh, ..Default::default() }, ..Default::default() });
        st.to = if listing { V { y: LIST_TOP, sc: sc_search, h: HF, o: 1., ..Default::default() } } else { V { y: 18., sc: 0.955, h: eh, o: 0., ..Default::default() } };
        st.z = 0;
        let up = self.mode == Mode::Settings;
        let se = anims.entry(SETTINGS.to_string()).or_insert_with(|| Anim { cur: V { y: 18., sc: 0.955, h: eh, ..Default::default() }, ..Default::default() });
        se.to = if up { V { y: 0., sc: 1., h: eh, o: 1., ..Default::default() } } else { V { y: eh * 0.35, sc: 1., h: eh, o: 0., ..Default::default() } };
        se.z = if up || se.cur.o > 0.01 { 1500 } else { -2 };
        self.mo.colors.retain(|id, _| self.mo.anims.contains_key(id));
        self.mo.dark.to = if self.find_open && self.mode == Mode::Note { DARK_FIND } else if self.hover_plus { DARK_HOVER } else { DARK };
        self.mo.cap_n.to = self.controls().len() as f32;
        self.mo.more_t.to = if self.ctl_open || self.palette { 1. } else { 0. };
        self.dock_radii();
        self.start_anim(window, cx);
    }

    // ---- one frame --------------------------------------------------------------

    fn step_anim(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let dt = (now - self.mo.tick).as_secs_f32().min(0.05);
        self.mo.tick = now;
        let rate = self.rate();
        let sliding = self.mo.hiding || self.mo.slide.0.v != self.mo.slide.0.to || self.mo.slide.1.v != self.mo.slide.1.to;
        let k = 1. - (-dt * if sliding { rate } else { 17. }).exp();
        let ko = 1. - (-dt * if sliding { rate * 1.3 } else { 30. }).exp();
        let mut settled = true;
        #[cfg(target_os = "macos")]
        {
            settled &= self.mo.window_alpha.step(1. - (-dt * rate * 1.3).exp(), 0.004);
            self.win.set_alpha(self.mo.window_alpha.v);
        }
        if sliding {
            let ks = 1. - (-dt * rate).exp();
            let (a, b) = (self.mo.slide.0.step(ks, 0.5), self.mo.slide.1.step(ks, 0.5));
            settled &= a && b;
        }
        let kc = 1. - (-dt * 14.).exp();
        let kf = 1. - (-dt * 26.).exp();
        settled &= self.mo.ctl_a.step(if sliding { ko } else { kf }, 0.004);
        settled &= self.mo.cap_n.step(1. - (-dt * 20.).exp(), 0.004);
        settled &= self.mo.more_t.step(kf, 0.004);
        settled &= self.mo.tab_hl.step(kf, 0.01);
        settled &= self.mo.dot_sc.step(kf, 0.003);
        settled &= self.mo.ctl_fade.step(kf, 0.004);
        settled &= self.mo.plus_sc.step(kf, 0.003);
        if self.palette || self.pal_t.v != 0. { settled &= self.pal_t.step(1. - (-dt * 24.).exp(), 0.005); if !self.palette { self.pal_t.v = 0.; self.pal_t.to = 0.; } }
        for e in self.pal_sc.iter_mut() { settled &= e.step(kf, 0.003); }
        {
            let max_to = self.st.scroll_to;
            let d = max_to - self.st.scroll;
            if d.abs() > 0.3 { self.st.scroll += d * (1. - (-dt * 22.).exp()); settled = false; } else { self.st.scroll = max_to; }
        }
        settled &= self.st.fade.step(1. - (-dt * 16.).exp(), 0.004);
        for e in self.st.seg.values_mut() { settled &= e[0].step(1. - (-dt * 18.).exp(), 0.2); settled &= e[1].step(1. - (-dt * 18.).exp(), 0.2); }
        // a note that makes way for another stays whole until the newcomer is opaque: no see-through flash
        let hold = self.mo.swap_out.is_none() && self.mode == Mode::Note && self.mo.anims.get(&self.cur).is_some_and(|c| !c.cur.near(&c.to) && c.to.o > 0.5);
        let cur_id = self.cur.clone();
        for (id, a) in self.mo.anims.iter_mut() {
            if a.delay > 0. { a.delay -= dt; settled = false; continue; }
            let to = a.to;
            let keep = hold && *id != cur_id && id != SEARCH && id != SETTINGS && to.o == 0. && a.cur.o > 0.5;
            if keep { settled = false; continue; }
            let swap = self.mo.swap_out.as_deref() == Some(id.as_str());
            a.cur.lerp(&to, k, if swap { k } else { ko });
            if a.cur.near(&to) { a.cur = to; } else { settled = false; }
        }
        let ids: Vec<String> = self.mo.anims.keys().filter(|id| *id != SEARCH && *id != SETTINGS).cloned().collect();
        for id in ids {
            let to = split(self.target_color(&id));
            let c = self.mo.colors.entry(id).or_insert(to);
            for i in 0..3 {
                if (c[i] - to[i]).abs() > 0.4 { c[i] += (to[i] - c[i]) * kc; settled = false; } else { c[i] = to[i]; }
            }
        }
        settled &= self.mo.dark.step(k, 0.2);
        settled &= self.mo.rb.step(kf, 0.1);
        settled &= self.mo.rl.step(kf, 0.1);
        settled &= self.mo.rr.step(kf, 0.1);
        let before = self.mo.win_h;
        self.mo.win_h += (self.mo.win_h_to - self.mo.win_h) * k;
        // the window never shrinks faster than the folders inside it (their start is staggered)
        let need = self.mo.anims.iter().filter(|(_, a)| a.cur.o > 0.05).map(|(_, a)| 1. + (a.cur.y - a.cur.lift + a.cur.h) * a.cur.sc + 14.).fold(0., f32::max);
        if self.mo.win_h < need && self.mo.win_h_to < before { self.mo.win_h = need.min(before); settled = false; }
        if (self.mo.win_h - self.mo.win_h_to).abs() < 0.5 { self.mo.win_h = self.mo.win_h_to; } else { settled = false; }
        if self.mo.drag.is_some() { self.mo.win_y_to = self.mo.win_y; }
        self.mo.win_y += (self.mo.win_y_to - self.mo.win_y) * k;
        if (self.mo.win_y - self.mo.win_y_to).abs() < 0.5 { self.mo.win_y = self.mo.win_y_to; } else { settled = false; }
        if self.mo.x_glide && self.mo.drag.is_none() {
            self.mo.win_x += (self.mo.win_x_to - self.mo.win_x) * k;
            if (self.mo.win_x - self.mo.win_x_to).abs() < 0.5 { self.mo.win_x = self.mo.win_x_to; self.mo.x_glide = false; } else { settled = false; }
        }
        if self.mo.drag.is_none() { self.apply_window(window); }
        cx.notify();
        // a fold, fade or slide is over in well under a second: one still going after 3 s never settles
        if !settled && self.mo.drag.is_none() && self.rsz.is_none() && now - self.mo.anim_since > Duration::from_secs(3) {
            self.anim_report("ran 3 s without settling", window);
            self.snap_anims();
            self.apply_window(window);
            settled = true;
        }
        if settled {
            self.mo.running = false;
            self.on_settled(window, cx);
            return true;
        }
        false
    }

    /// everything at its target at once
    fn snap_anims(&mut self) {
        for a in self.mo.anims.values_mut() { a.cur = a.to; a.delay = 0.; }
        let ids: Vec<String> = self.mo.colors.keys().cloned().collect();
        for id in ids { let to = split(self.target_color(&id)); self.mo.colors.insert(id, to); }
        let m = &mut self.mo;
        for e in [&mut m.dark, &mut m.ctl_a, &mut m.cap_n, &mut m.more_t, &mut m.ctl_fade, &mut m.plus_sc, &mut m.tab_hl, &mut m.dot_sc, &mut m.rl, &mut m.rr, &mut m.rb, &mut m.slide.0, &mut m.slide.1] { e.v = e.to; }
        #[cfg(target_os = "macos")]
        { m.window_alpha.v = m.window_alpha.to; }
        m.win_h = m.win_h_to;
        m.win_y = m.win_y_to;
        m.win_x = m.win_x_to;
        m.x_glide = false;
        self.pal_t.v = self.pal_t.to;
        for e in self.pal_sc.iter_mut() { e.v = e.to; }
        self.st.scroll = self.st.scroll_to;
        self.st.fade.v = self.st.fade.to;
        for e in self.st.seg.values_mut() { e[0].v = e[0].to; e[1].v = e[1].to; }
    }

    /// what the animation was doing, to notedd-anim.txt in the temp folder (kept small)
    fn anim_report(&self, why: &str, window: &Window) {
        use std::io::Write;
        let f = std::env::temp_dir().join("notedd-anim.txt");
        if std::fs::metadata(&f).is_ok_and(|m| m.len() > 256 * 1024) { let _ = std::fs::remove_file(&f); }
        let r = self.win_rect();
        let m = &self.mo;
        let mut s = format!("{:?} {why}
  mode {} hiding {} window {},{} {}x{} scale {} edit_h {:.2} win_h {:.2}->{:.2} win_y {:.1}->{:.1}
",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()), self.mode as u8, m.hiding,
            r.left, r.top, r.right - r.left, r.bottom - r.top, window.scale_factor(), self.edit_h, m.win_h, m.win_h_to, m.win_y, m.win_y_to);
        let eases = [("dark", m.dark), ("ctl_a", m.ctl_a), ("cap_n", m.cap_n), ("more_t", m.more_t), ("ctl_fade", m.ctl_fade), ("plus_sc", m.plus_sc),
            ("tab_hl", m.tab_hl), ("dot_sc", m.dot_sc), ("rl", m.rl), ("rr", m.rr), ("rb", m.rb), ("slide_x", m.slide.0), ("slide_y", m.slide.1), ("pal_t", self.pal_t)];
        for (n, e) in eases { if e.v != e.to { s += &format!("  {n} {:.3}->{:.3}
", e.v, e.to); } }
        for (id, a) in &m.anims {
            if !a.cur.near(&a.to) || a.delay > 0. {
                s += &format!("  folder {id}: y {:.1}->{:.1} h {:.1}->{:.1} sc {:.3}->{:.3} o {:.2}->{:.2} rot {:.3}->{:.3} delay {:.2}
",
                    a.cur.y, a.to.y, a.cur.h, a.to.h, a.cur.sc, a.to.sc, a.cur.o, a.to.o, a.cur.rot, a.to.rot, a.delay);
            }
        }
        if self.st.scroll != self.st.scroll_to { s += &format!("  settings scroll {:.1}->{:.1}
", self.st.scroll, self.st.scroll_to); }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&f) { let _ = file.write_all(s.as_bytes()); }
    }

    fn on_settled(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mo.swap_out = None;
        if self.mo.hiding {
            self.mo.hiding = false;
            self.win.hide();
            #[cfg(target_os = "macos")]
            { self.win.set_alpha(1.); self.mo.window_alpha = Ease::at(1.); }
            self.mo.slide = (Ease::at(0.), Ease::at(0.));
            self.apply_window(window);
            if self.mode != Mode::List {
                let id = if self.mode == Mode::Settings { SETTINGS.to_string() } else { self.cur.clone() };
                let eh = self.eh();
                if let Some(a) = self.mo.anims.get_mut(&id) { a.to = V { y: 0., sc: 1., h: eh, o: 1., ..Default::default() }; a.cur = a.to; }
            }
            if self.mo.demo { self.mo.demo = false; self.appear(window, cx); }
            return;
        }
        self.mo.anims.retain(|id, a| id == SEARCH || id == SETTINGS || a.cur.o > 0. || a.to.o > 0.);
        self.mo.colors.retain(|id, _| self.mo.anims.contains_key(id));
        let alive: Vec<String> = self.mo.anims.keys().cloned().collect();
        self.mo.previews.borrow_mut().retain(|id, _| alive.contains(id));
        self.set_region(window);
        cx.notify();
    }

    /// clicks land on the folder and the buttons only; the rest of the window lets them through
    pub fn set_region(&mut self, window: &Window) {
        let s = self.dpi(window);
        let px = |v: f32| (v * s).round() as i32;
        if self.mode == Mode::List || !self.editor_shown() { self.win.set_shape(None); return; }
        let r = self.win_rect();
        let width = r.w() as f32 / s - 2.;
        let eh = self.eh();
        let (x2, bx) = tab_x(width);
        let n = self.controls().len().max(self.mo.cap_n.v.ceil() as usize);
        let rc = |left: i32, top: i32, right: i32, bottom: i32| Rect { left, top, right, bottom };
        self.win.set_shape(Some(&[
            // the body, the tab, the flare where the tab meets the body, the buttons
            rc(0, px(T), px(width + 2.), px(eh + PADB)),
            rc(px(x2 - 2.), 0, px(bx + DARK_FIND + F + 4.), px(T) + 2),
            rc(px(x2 - F - 3.), px(T - F - 3.), px(x2), px(T) + 2),
            rc(px(1. + width - 16. - 34. * (n as f32 - 1.) - 18.), 0, px(width + 2.), px(T) + 2),
        ]));
    }

    /// the live editor is on show once its folder has settled in place
    pub fn editor_shown(&self) -> bool {
        if self.mode != Mode::Note || std::env::var("NOTEDD_NOEDIT").is_ok() { return false; }
        #[cfg(not(target_os = "macos"))]
        if self.mo.hiding { return false; }
        let moving = self.mo.slide.0.v != 0. || self.mo.slide.1.v != 0.;
        (!moving || cfg!(target_os = "macos")) && self.mo.anims.get(&self.cur).is_some_and(|a| a.cur.near(&a.to) && a.to.o > 0.99 && a.delay <= 0.)
            && (self.mo.win_h - self.mo.win_h_to).abs() < 0.5
    }

    // ---- showing and hiding -----------------------------------------------------

    pub fn appear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.win.visible();
        #[cfg(target_os = "macos")]
        if !self.win.foreground() {
            self.win.move_to_pointer_screen();
            let r = self.win_rect();
            self.mo.win_x = r.left as f32; self.mo.win_x_to = self.mo.win_x;
            self.mo.win_y = r.top as f32; self.mo.win_y_to = self.mo.win_y;
            self.mo.edit_top = r.top;
        }
        if !shown || self.mo.hiding {
            self.mo.hiding = false;
            if !shown {
                let s = self.dpi(window);
                let r = self.win_rect();
                self.mo.win_x = r.left as f32;
                self.mo.win_y = r.top as f32;
                let (dx, dy) = self.slide_offset(&r, s);
                self.mo.slide = (Ease::at(dx), Ease::at(dy));
                #[cfg(not(target_os = "macos"))]
                if self.fades() { self.mo.ctl_a = Ease::at(0.); }
                #[cfg(target_os = "macos")]
                if self.fades() { self.mo.window_alpha = Ease::at(0.); self.win.set_alpha(0.); }
            }
            #[cfg(target_os = "macos")]
            { self.mo.window_alpha.to = 1.; }
            self.mo.ctl_a.to = 1.;
            self.mo.slide.0.to = 0.;
            self.mo.slide.1.to = 0.;
            #[cfg(not(target_os = "macos"))]
            if self.mode != Mode::List && self.fades() {
                let id = if self.mode == Mode::Settings { SETTINGS.to_string() } else { self.cur.clone() };
                let in_place = self.prefs.dir == 4;
                if let Some(a) = self.mo.anims.get_mut(&id) {
                    if !shown { a.cur.o = 0.; if in_place { a.cur.y = 22.; a.cur.sc = 0.985; } }
                    if self.prefs.far && !in_place { a.cur.o = 1.; }
                }
            }
            self.apply_window(window);
        }
        self.win.show();
        #[cfg(not(target_os = "macos"))]
        window.activate_window();
        self.focus_main(window, cx);
        self.retarget(false, 0, window, cx);
    }

    pub fn toggle_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.win.visible() && self.win.foreground() { self.hide(window, cx); } else { self.appear(window, cx); }
    }

    pub fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_now(cx);
        self.palette = false;
        self.capture = None;
        if !self.win.visible() { return; }
        if self.mode == Mode::List && self.fades() { self.win.hide(); self.mo.ctl_a = Ease::at(0.); return; }
        if self.mode == Mode::Settings {
            // Finish any exchange before fading, so the previous note cannot show through.
            self.mo.swap_out = None;
            for (id, a) in self.mo.anims.iter_mut() {
                if id != SETTINGS { a.cur.o = 0.; a.to.o = 0.; }
            }
            if let Some(a) = self.mo.anims.get_mut(SETTINGS) {
                a.cur = V { y: 0., sc: 1., h: self.edit_h, o: 1., ..Default::default() };
            }
        }
        if self.find_open { self.close_find(window, cx); }
        self.ctl_open = false;
        #[cfg(not(target_os = "macos"))]
        if self.fades() { self.mo.ctl_a.to = 0.; }
        #[cfg(target_os = "macos")]
        if self.fades() { self.mo.window_alpha.to = 0.; }
        if !self.mo.hiding {
            let s = self.dpi(window);
            let r = self.win_rect();
            self.mo.win_x = (r.left as f32 - self.mo.slide.0.v.round()).round();
            self.mo.win_y = (r.top as f32 - self.mo.slide.1.v.round()).round();
            let base = Rect { left: self.mo.win_x as i32, top: self.mo.win_y as i32, right: self.mo.win_x as i32 + (r.right - r.left), bottom: self.mo.win_y as i32 + (r.bottom - r.top) };
            let (dx, dy) = self.slide_offset(&base, s);
            self.mo.slide.0.to = dx;
            self.mo.slide.1.to = dy;
        }
        #[cfg(not(target_os = "macos"))]
        if self.mode != Mode::List && self.fades() {
            let id = if self.mode == Mode::Settings { SETTINGS.to_string() } else { self.cur.clone() };
            let in_place = self.prefs.dir == 4;
            let eh = self.eh();
            if let Some(a) = self.mo.anims.get_mut(&id) { a.to = V { y: if in_place { 26. } else { 0. }, sc: if in_place { 0.985 } else { 1. }, h: eh, o: 0., ..Default::default() }; }
        }
        self.mo.hiding = true;
        self.start_anim(window, cx);
    }

    // ---- docking, dragging ------------------------------------------------------

    fn docks_of(&self, left: i32, top: i32, w: i32, h: i32, work: &Rect, s: f32) -> u8 {
        let hf = h - (PADB * s).round() as i32;
        let mut d = 0;
        if left <= work.left { d |= 1; }
        if left + w >= work.right { d |= 2; }
        if top + hf >= work.bottom - 1 { d |= 4; }
        if top <= work.top { d |= 8; }
        d
    }

    /// the capsule and the arrow follow the set of buttons
    pub fn sync_caps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mo.cap_n.to = self.controls().len() as f32;
        self.mo.more_t.to = if self.ctl_open || self.palette { 1. } else { 0. };
        self.set_region(window);
        self.start_anim(window, cx);
    }

    pub fn dock_radii(&mut self) {
        let d = if self.mode != Mode::List { self.mo.docks } else { 0 };
        self.mo.rb.to = if d & 4 != 0 { 0. } else { R };
        self.mo.rl.to = if d & 1 != 0 { 0. } else { R };
        self.mo.rr.to = if d & 2 != 0 { 0. } else { R };
    }

    pub fn update_docked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let r = self.win_rect();
        let work = self.dock_area();
        let s = self.dpi(window);
        let top = if self.mode != Mode::List { self.mo.edit_top } else { r.top };
        self.mo.docks = if self.mode != Mode::List { self.docks_of(r.left, top, r.right - r.left, r.bottom - r.top, &work, s) } else { 0 };
        self.dock_radii();
        self.start_anim(window, cx);
    }

    /// a press on the tab: drag the window with the magnet, or a plain click opens the drawer
    pub fn start_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mo.drag.is_some() { return; }
        let c0 = self.win.cursor_pos();
        let r0 = self.win_rect();
        self.mo.drag = Some(Drag { c0, r0, moved: false });
        self.palette = false;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(6)).await;
                let down = platform::left_button_down();
                let r = this.update_in(cx, |s, window, cx| { if down { s.drag_move(window, cx); } else { s.end_drag(window, cx); } !down });
                if !matches!(r, Ok(false)) { break; }
            }
        }).detach();
    }

    /// near a screen edge the window lags behind the cursor more and more (d*d/zone), so it settles onto the edge smoothly
    fn drag_move(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.mo.drag else { return };
        let (c0, r0) = (d.c0, d.r0);
        let c = self.win.cursor_pos();
        let (dx, dy) = (c.x - c0.x, c.y - c0.y);
        if dx.abs() + dy.abs() > 3 { if let Some(d) = &mut self.mo.drag { d.moved = true; } }
        let moved = self.mo.drag.as_ref().is_some_and(|d| d.moved);
        if !moved { return; }
        let s = self.dpi(window);
        let mut r = Rect { left: r0.left + dx, top: r0.top + dy, right: r0.right + dx, bottom: r0.bottom + dy };
        if self.mode != Mode::List {
            let work = self.drag_dock_area();
            let (hw, w) = (r.bottom - r.top, r.right - r.left);
            let hf = ((self.eh()) * s).round() as i32;
            let zone = ZONE * s;
            let pull = |d: f32| if d <= 0. { 0. } else if d < zone { d * d / zone } else { d };
            let (dt, db) = ((r.top - work.top) as f32, (work.bottom - (r.top + hf)) as f32);
            let (dl, dr) = ((r.left - work.left) as f32, (work.right - r.right) as f32);
            let (pv, top) = if db < dt { let p = pull(db); (p, work.bottom - p.round() as i32 - hf) } else { let p = pull(dt); (p, work.top + p.round() as i32) };
            let (ph, left) = if dl < dr { let p = pull(dl); (p, work.left + p.round() as i32) } else { let p = pull(dr); (p, work.right - p.round() as i32 - w) };
            r = Rect { left, top, right: left + w, bottom: top + hw };
            self.mo.docks = self.docks_of(left, top, w, hw, &work, s);
            let k = |v: f32| R * (v / (12. * s)).min(1.);
            let (kb, kl, kr) = (if db < dt { k(pv) } else { R }, if dl < dr { k(ph) } else { R }, if dl >= dr { k(ph) } else { R });
            self.mo.rb.to = kb; self.mo.rl.to = kl; self.mo.rr.to = kr;
            self.start_anim(window, cx);
        }
        self.mo.win_x = r.left as f32; self.mo.win_x_to = self.mo.win_x;
        self.mo.win_y = r.top as f32; self.mo.win_y_to = self.mo.win_y;
        self.win.move_client_to(r.left, r.top);
        cx.notify();
    }

    fn end_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.mo.drag.take() else { return };
        if !d.moved {
            if self.mode == Mode::Note { self.toggle_list(window, cx); }
            return;
        }
        let r = self.win_rect();
        let s = self.dpi(window);
        self.mo.win_y = r.top as f32; self.mo.win_y_to = self.mo.win_y;
        self.mo.win_x = r.left as f32; self.mo.win_x_to = self.mo.win_x;
        if self.mode == Mode::List {
            self.mo.edit_top += r.top - d.r0.top;
            self.retarget(false, 0, window, cx);
            return;
        }
        let work = self.drag_dock_area();
        let (w, hf) = (r.right - r.left, (self.eh() * s).round() as i32);
        let near = |gap: i32| gap > 0 && (gap as f32) < GLIDE * s;
        if near(work.bottom - (r.top + hf)) { self.mo.win_y_to = (work.bottom - hf) as f32; }
        else if near(r.top - work.top) { self.mo.win_y_to = work.top as f32; }
        if near(r.left - work.left) { self.mo.win_x_to = work.left as f32; }
        else if near(work.right - (r.left + w)) { self.mo.win_x_to = (work.right - w) as f32; }
        self.mo.x_glide = self.mo.win_x_to != self.mo.win_x;
        self.mo.docks = self.docks_of(self.mo.win_x_to as i32, self.mo.win_y_to as i32, w, r.bottom - r.top, &work, s);
        self.dock_radii();
        self.mo.edit_top = self.mo.win_y_to as i32;
        self.start_anim(window, cx);
    }
}
