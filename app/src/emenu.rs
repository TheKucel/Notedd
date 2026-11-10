//! The right-click menu of the editor: clipboard and formatting, like the old app popup.

use super::*;
use std::time::Instant;

const ROW: f32 = 32.;
const PADM: f32 = 6.;
const FORMAT_START: usize = 5;
const GROUP_GAP: f32 = 10.;
const ENTRY: f32 = 0.14;

const GLYPHS: [&str; 10] = [
    "M7 3L3 7l4 4M3 7h6l3 1.5 1 2.5-1 2H8",
    "M3 3h2l2 2-2 2H3L2 5zM3 9h2l2 2-2 2H3l-1-2zM6 6l7 7M6 10l7-7",
    "M6 6h7v7H6zM3 10V3h7",
    "M5 4H3v10h10V4h-2M6 2h4l1 2-1 2H6L5 4z",
    "M6 3H3v3M10 3h3v3M13 10v3h-3M6 13H3v-3M6 7h4M6 9h4",
    "M5 13V3h4l2 1v2L9 8H5M9 8l3 1v2l-2 2H5",
    "M7 3h6M3 13h6M10 3L6 13",
    "M11 4L9 3H6L4 5v1l2 2 4 1 2 1v1l-2 2H6l-2-1M2 8h12",
    "M5 5L2 8l3 3M11 5l3 3-3 3M9 4l-2 8",
    "M4 9l6-6 3 3-6 6H4zM4 12l-1 2M8 14h5",
];

pub struct EMenu {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub t0: Instant,
    pub hot: Option<usize>,
    pub pressed: Option<usize>,
    pub enabled: [bool; 10],
    pub labels: Vec<(String, String)>,
}

fn row_y(i: usize) -> f32 { PADM + i as f32 * ROW + if i >= FORMAT_START { GROUP_GAP } else { 0. } }
fn menu_h() -> f32 { 2. * PADM + 10. * ROW + GROUP_GAP }

impl EMenu {
    fn progress(&self) -> f32 {
        let t = (self.t0.elapsed().as_secs_f32() / ENTRY).min(1.);
        1. - (1. - t).powi(3)
    }
}

impl Shell {
    /// opens the menu at a window position (client coordinates)
    pub fn open_emenu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = false;
        let sel = !self.body.read(cx).core().collapsed();
        let hl = {
            let (m, vk) = self.prefs.keys[10];
            if vk == 0 { String::new() } else { system::label(m, vk) }
        };
        let labels = vec![
            (ui::UNDO.get().to_string(), system::cmd_label("Z")),
            (ui::CUT.get().to_string(), system::cmd_label("X")),
            (ui::COPY.get().to_string(), system::cmd_label("C")),
            (ui::PASTE.get().to_string(), system::cmd_label("V")),
            (ui::SELECT_ALL.get().to_string(), system::cmd_label("A")),
            (ui::BOLD.get().to_string(), system::cmd_label("B")),
            (ui::ITALIC.get().to_string(), system::cmd_label("I")),
            (ui::STRIKE.get().to_string(), system::cmd_label("Shift+S")),
            (ui::CODE.get().to_string(), system::cmd_label("E")),
            (ui::KEY_NAMES[10].get().to_string(), hl),
        ];
        let lw = labels.iter().map(|l| text_width(window, &l.0, 13., FontWeight::NORMAL)).fold(0., f32::max);
        let kw = labels.iter().map(|l| text_width(window, &l.1, 12., FontWeight::NORMAL)).fold(0., f32::max);
        let w = (42. + lw + 24. + kw + 16.).max(244.).ceil();
        let vs = window.viewport_size();
        let (vw, vh) = (f32::from(vs.width), f32::from(vs.height));
        let (mut x, mut y) = (f32::from(at.x) + 2., f32::from(at.y) + 4.);
        if x + w > vw { x = f32::from(at.x) - w - 2.; }
        if y + menu_h() > vh { y = f32::from(at.y) - menu_h() - 4.; }
        x = x.min(vw - w).max(0.);
        y = y.min(vh - menu_h()).max(0.);
        let mut enabled = [true; 10];
        for i in [1, 2, 5, 6, 7, 8, 9] { enabled[i] = sel; }
        self.emenu = Some(EMenu { x, y, w, t0: Instant::now(), hot: None, pressed: None, enabled, labels });
        cx.notify();
    }

    pub fn close_emenu(&mut self, cx: &mut Context<Self>) {
        if self.emenu.take().is_some() { cx.notify(); }
    }

    fn run_emenu(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let ok = self.emenu.as_ref().is_some_and(|m| m.enabled[i]);
        self.emenu = None;
        cx.notify();
        if !ok { return; }
        self.body.read(cx).focus(window);
        let a: Box<dyn Action> = match i {
            0 => Box::new(editor::Undo),
            1 => Box::new(editor::Cut),
            2 => Box::new(editor::Copy),
            3 => Box::new(editor::Paste),
            4 => Box::new(editor::SelectAll),
            5 => Box::new(editor::Bold),
            6 => Box::new(editor::Italic),
            7 => Box::new(editor::Strike),
            8 => Box::new(editor::Code),
            _ => Box::new(editor::Highlight),
        };
        window.dispatch_action(a, cx);
    }

    pub fn emenu_el(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.emenu.as_ref()?;
        let t = m.progress();
        if t < 1. { window.request_animation_frame(); }
        let th = self.th;
        let tint = self.idx(&self.cur).map_or(th.paper(), |i| th.note(self.notes[i].color));
        let (x, y, w, h) = (m.x, m.y, m.w, menu_h());
        let off = (t - 1.) * 5.;
        let (hot, pressed, enabled) = (m.hot, m.pressed, m.enabled);
        let labels = m.labels.clone();
        let paint = canvas(|_, _, _| (), move |b, _, window, cx| {
            let (ox, oy) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let o = t.max(0.06);
            let sh = if th.night { black() } else { c(0x28261e) };
            let k = if th.night { 2.4 } else { 1. };
            for i in 0..12 {
                let e = -15. * 0.35 + 15. * 0.9 * i as f32 / 11.;
                let a = (0.18 * o * k / 12. * 1.7).min(0.5);
                let r = Bounds::new(point(px(ox - e), px(oy + 5. - e)), size(px((w + 2. * e).max(0.)), px((h + 2. * e).max(0.))));
                window.paint_quad(fill(r, Hsla { a, ..sh }).corner_radii(px((13. + e).max(0.))));
            }
            let bounds = Bounds::new(point(px(ox), px(oy)), size(px(w), px(h)));
            window.paint_quad(quad(bounds, px(13.), al(c(tint), o), px(1.), al(c(th.line), 0.55 * o), BorderStyle::Solid));
            let ink = c(th.ink);
            let sep = oy + row_y(FORMAT_START) - GROUP_GAP / 2.;
            let mut ln = PathBuilder::stroke(px(1.));
            ln.move_to(point(px(ox + 12.), px(sep)));
            ln.line_to(point(px(ox + w - 12.), px(sep)));
            if let Ok(p) = ln.build() { window.paint_path(p, al(ink, 0.15 * o)); }
            for i in 0..10 {
                let ry = oy + row_y(i);
                if hot == Some(i) && enabled[i] {
                    let r = Bounds::new(point(px(ox + PADM), px(ry + 1.)), size(px(w - 2. * PADM), px(ROW - 2.)));
                    window.paint_quad(fill(r, al(ink, if pressed == Some(i) { 0.15 } else { 0.09 } * o)).corner_radii(px(7.)));
                }
                let a = if enabled[i] { 1. } else { 0.34 };
                glyph(window, GLYPHS[i], ox + 23., ry + ROW / 2., 16., 1.25, al(ink, a * o), false);
                text(window, cx, &labels[i].0, point(px(ox + 42.), px(ry + (ROW - 18.2) / 2.)), 13., FontWeight::NORMAL, al(ink, a * o), w - 58.);
                let kw = text_width(window, &labels[i].1, 12., FontWeight::NORMAL);
                text(window, cx, &labels[i].1, point(px(ox + w - 16. - kw), px(ry + (ROW - 16.8) / 2.)), 12., FontWeight::NORMAL, al(c(th.ink2), 0.85 * a * o), kw + 4.);
            }
        }).absolute().size_full();
        let mut menu = div().id("emenu").absolute().left(px(1. + x)).top(px(1. + y + off)).w(px(w)).h(px(h)).occlude()
            .on_mouse_down_out(cx.listener(|s, _, _, cx| s.close_emenu(cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|s, _, _, cx| { cx.stop_propagation(); s.close_emenu(cx); }))
            .on_scroll_wheel(cx.listener(|s, _, _, cx| s.close_emenu(cx)))
            .on_hover(cx.listener(|s, h: &bool, _, cx| { if !*h { if let Some(m) = &mut s.emenu { m.hot = None; m.pressed = None; cx.notify(); } } }))
            .child(paint);
        for i in 0..10 {
            menu = menu.child(div().id(("emi", i)).absolute().left(px(PADM)).top(px(row_y(i))).w(px(w - 2. * PADM)).h(px(ROW))
                .when(enabled[i], |d| d.cursor_pointer())
                .on_mouse_move(cx.listener(move |s, _, _, cx| {
                    if let Some(m) = &mut s.emenu {
                        let want = if m.enabled[i] { Some(i) } else { None };
                        if m.hot != want { m.hot = want; cx.notify(); }
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(move |s, _, _, cx| { cx.stop_propagation(); if let Some(m) = &mut s.emenu { m.pressed = Some(i); cx.notify(); } }))
                .on_mouse_up(MouseButton::Left, cx.listener(move |s, _, w, cx| {
                    let hit = s.emenu.as_ref().is_some_and(|m| m.pressed == Some(i));
                    if hit { s.run_emenu(i, w, cx); } else if let Some(m) = &mut s.emenu { m.pressed = None; cx.notify(); }
                })));
        }
        Some(menu.into_any_element())
    }
}
