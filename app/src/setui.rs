//! The settings pages, laid out and painted exactly like the old app's:
//! rows of a label and one block (choice with a sliding pill, stepper, key caps, colour dots, button),
//! help lines, a scroll thumb, the colour picker, keyboard navigation.

use super::*;
use crate::theme::mix;
use std::collections::HashMap;

const SET_TOP: f32 = EDIT_TOP - 8.0;
const SET_ROW: f32 = 50.0;
const SET_BOTTOM: f32 = 18.0;
const CTL_H: f32 = 32.0;
const SET_LABEL: f32 = 170.0;
const GRIP: f32 = 7.0;

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub enum Row { Page, Language, Editor, Theme, Scale, Dir, Far, Speed, Auto, Notes, Key(usize), Ink, Accent, Cards, Reset }

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum Pick { Ink, Accent, Card(usize) }

/// one button of a row: label, chosen, a colour dot, shown value only (not clickable)
#[derive(Clone)]
pub struct Seg { pub text: String, pub on: bool, pub swatch: Option<u32>, pub value: bool, pub glyph: u8, pub keys: Option<(u32, u32)> }

fn seg(text: impl Into<String>, on: bool) -> Seg { Seg { text: text.into(), on, swatch: None, value: false, glyph: 0, keys: None } }

#[derive(PartialEq, Clone, Copy)]
enum Style { Choice, Step, Keys, Swatches, Button }

fn style(r: Row) -> Style {
    match r { Row::Scale => Style::Step, Row::Key(_) => Style::Keys, Row::Cards => Style::Swatches, Row::Reset | Row::Notes => Style::Button, _ => Style::Choice }
}

type Rc4 = [f32; 4]; // left, top, right, bottom

#[derive(Clone)]
pub struct RowG { pub row: Row, pub label: String, pub top: f32, pub h: f32, pub block: Rc4, pub segs: Vec<(Rc4, Seg)> }

pub struct SetState {
    pub scroll: f32,
    pub scroll_to: f32,
    pub scroll_wake: bool,
    pub hover: Option<(usize, usize)>,
    pub focus: (usize, usize),
    pub kb: bool,
    pub pick: Option<Pick>,
    pub hsv: [f32; 3],
    pub hex: String,
    pub seg: HashMap<(usize, Row), [Ease; 2]>,
    pub fade: Ease,
    pub thumb_drag: Option<f32>,
    pub pick_drag: Option<u8>,
}

impl SetState {
    pub fn new() -> SetState {
        SetState { scroll: 0., scroll_to: 0., scroll_wake: false, hover: None, focus: (0, 0), kb: false, pick: None, hsv: [0., 0., 0.], hex: String::new(), seg: HashMap::new(), fade: Ease::at(1.), thumb_drag: None, pick_drag: None }
    }
}

/// everything the canvas needs to paint the page
#[derive(Clone)]
pub struct SetDraw {
    pub page_o: f32,
    pub th: AppTheme,
    pub w: f32,
    pub eh: f32,
    pub rows: Vec<RowG>,
    pub end: f32,
    pub offset: f32,
    pub body: (f32, f32),
    pub pills: HashMap<Row, (f32, f32)>,
    pub hover: Option<(usize, usize)>,
    pub focus: Option<(usize, usize)>,
    pub page: usize,
    pub pick: Option<(Pick, [f32; 3], String, u32)>,
    pub thumb: Option<(f32, f32)>,
    pub dir: String,
    pub label_w: f32,
    pub o: f32,
    pub help: [Option<&'static str>; 2],
    /// the global shortcuts another app already holds
    pub taken: [bool; 2],
}

fn key_parts(m: u32, vk: u32) -> Vec<String> {
    let mut v = vec![];
    if m & 2 != 0 { v.push("Ctrl".to_string()); }
    if m & 1 != 0 { v.push("Alt".to_string()); }
    if m & 4 != 0 { v.push("Shift".to_string()); }
    if m & 8 != 0 { v.push("Win".to_string()); }
    if vk != 0 { v.push(system::label(0, vk)); }
    v
}

pub fn hsv_rgb(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 { 0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x), 3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x) };
    let m = v - c;
    let f = |t: f32| ((t + m) * 255.0).round().clamp(0.0, 255.0) as u32;
    (f(r) << 16) | (f(g) << 8) | f(b)
}

pub fn rgb_hsv(c: u32) -> [f32; 3] {
    let [r, g, b] = [(c >> 16) & 255, (c >> 8) & 255, c & 255].map(|v| v as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d < 1e-6 { 0.0 } else if mx == r { 60.0 * ((g - b) / d).rem_euclid(6.0) } else if mx == g { 60.0 * ((b - r) / d + 2.0) } else { 60.0 * ((r - g) / d + 4.0) };
    [h, if mx < 1e-6 { 0.0 } else { d / mx }, mx]
}

fn tw(window: &mut Window, s: &str) -> f32 { text_width(window, s, 13., FontWeight::NORMAL) }

fn al(mut c: Hsla, o: f32) -> Hsla { c.a *= o; c }

// ---- painting primitives, placed by the folder's transform ----------------------

fn rq(window: &mut Window, p: &Place, r: Rc4, rad: f32, fill_c: Option<Hsla>, line: Option<(f32, Hsla)>, dashed: bool) {
    let lw = line.map_or(0., |l| l.0);
    let (l, t, rr, b) = (r[0] - lw / 2., r[1] - lw / 2., r[2] + lw / 2., r[3] + lw / 2.);
    let a = p.pt(l, t);
    let bounds = Bounds::new(a, size(px((rr - l) * p.sc), px((b - t) * p.sc)));
    window.paint_quad(quad(bounds, px((rad + lw / 2.) * p.sc), fill_c.unwrap_or(transparent_black()), px(lw * p.sc),
        line.map_or(transparent_black(), |l| l.1), if dashed { BorderStyle::Dashed } else { BorderStyle::Solid }));
}

fn circ(window: &mut Window, p: &Place, x: f32, y: f32, r: f32, fill_c: Option<Hsla>, line: Option<(f32, Hsla)>) {
    let a = p.pt(x, y);
    circle(window, f32::from(a.x), f32::from(a.y), r * p.sc, line.map(|(w, c)| (w * p.sc, c)), fill_c);
}

/// one line of text, vertically centred on `cy`
fn txt(window: &mut Window, cx: &mut App, p: &Place, s: &str, left: f32, cy: f32, size_px: f32, weight: FontWeight, color: Hsla, max: f32) {
    text(window, cx, s, p.pt(left, cy - size_px * 0.7), size_px * p.sc, weight, color, max * p.sc);
}

fn txt_mid(window: &mut Window, cx: &mut App, p: &Place, s: &str, mid: f32, cy: f32, size_px: f32, weight: FontWeight, color: Hsla) {
    let wd = text_width(window, s, size_px, weight);
    txt(window, cx, p, s, mid - wd / 2., cy, size_px, weight, color, wd + 4.);
}

fn stroke_line(window: &mut Window, p: &Place, a: (f32, f32), b: (f32, f32), w: f32, color: Hsla) {
    let mut pb = PathBuilder::stroke(px(w * p.sc));
    pb.move_to(p.pt(a.0, a.1));
    pb.line_to(p.pt(b.0, b.1));
    if let Ok(path) = pb.build() { window.paint_path(path, color); }
}

fn grad(window: &mut Window, p: &Place, r: Rc4, radii: [f32; 4], angle: f32, a: Hsla, b: Hsla) {
    let at = p.pt(r[0], r[1]);
    let bounds = Bounds::new(at, size(px((r[2] - r[0]) * p.sc), px((r[3] - r[1]) * p.sc)));
    let corners = Corners { top_left: px(radii[0] * p.sc), top_right: px(radii[1] * p.sc), bottom_right: px(radii[2] * p.sc), bottom_left: px(radii[3] * p.sc) };
    window.paint_quad(fill(bounds, linear_gradient(angle, linear_color_stop(a, 0.), linear_color_stop(b, 1.))).corner_radii(corners));
}

fn pick_geom(sd_w: f32, label_w: f32, end: f32) -> (Rc4, Rc4, f32) {
    let y = end + 6.0;
    let x = PAD + label_w;
    let w = (sd_w - PAD - x - 150.0).clamp(140.0, 260.0);
    ([x, y, x + w, y + 110.0], [x, y + 120.0, x + w, y + 134.0], x + w + 18.0)
}

fn paint_rows(sd: &SetDraw, rows: &[RowG], first: usize, p: &Place, window: &mut Window, cx: &mut App) {
    let th = &sd.th;
    let o = sd.o;
    let (ink, ink2, ink3, line) = (c(th.ink), c(th.ink2), c(th.ink3), c(th.line));
    let w = sd.w;
    for (i, g) in rows.iter().enumerate() {
        let r = first + i;
        let cyr = g.top + g.h / 2.;
        if !g.label.is_empty() { txt(window, cx, p, &g.label, PAD, cyr, 15., FontWeight::MEDIUM, al(ink2, o), sd.label_w - 10.); }
        let st = style(g.row);
        let b = g.block;
        let hh = b[3] - b[1];
        if matches!(st, Style::Choice | Style::Step) {
            rq(window, p, b, hh / 2., Some(al(ink, 0.05 * o)), None, false);
            rq(window, p, b, hh / 2., None, Some((1., al(line, 0.35 * o))), false);
        }
        let pill = if st == Style::Choice { sd.pills.get(&g.row).copied() } else { None };
        if let Some((l, rr)) = pill {
            if g.segs.iter().any(|(_, s)| s.on) {
                rq(window, p, [l + 3., b[1] + 3., rr - 3., b[3] - 3.], (hh - 6.) / 2., Some(al(c(th.tab_dark), o)), None, false);
            }
        }
        for (k, (rc, s)) in g.segs.iter().enumerate() {
            let hover = sd.hover == Some((r, k));
            let (mx, my) = ((rc[0] + rc[2]) / 2., (rc[1] + rc[3]) / 2.);
            match st {
                Style::Choice => {
                    let cover = pill.map_or(0.0, |(l, rr)| ((rr.min(rc[2]) - l.max(rc[0])) / (rc[2] - rc[0])).clamp(0.0, 1.0));
                    let base = if hover { th.ink } else { th.ink2 };
                    let col = c(mix(base, th.on_dark, cover));
                    let mut tx = rc[0] + 15.;
                    if let Some(sw) = s.swatch {
                        circ(window, p, rc[0] + 21., my, 7., Some(al(c(sw), o)), Some((1., al(c(mix(th.line, th.on_dark, cover)), 0.7 * o))));
                        tx = rc[0] + 35.;
                    }
                    txt(window, cx, p, &s.text, tx, my, 13., FontWeight::NORMAL, al(col, o), rc[2] + 4. - tx);
                }
                Style::Step => {
                    if s.value { txt_mid(window, cx, p, &s.text, mx, my, 13., FontWeight::MEDIUM, al(ink, o)); }
                    else {
                        if hover { rq(window, p, [rc[0] + 3., rc[1] + 3., rc[2] - 3., rc[3] - 3.], (hh - 6.) / 2., Some(al(ink, 0.08 * o)), None, false); }
                        stroke_line(window, p, (mx - 5., my), (mx + 5., my), 1.6, al(ink, o));
                        if s.glyph == 2 { stroke_line(window, p, (mx, my - 5.), (mx, my + 5.), 1.6, al(ink, o)); }
                    }
                }
                Style::Keys => paint_keys(sd, *rc, s, p, window, cx),
                Style::Swatches => {
                    if let Some(sw) = s.swatch {
                        circ(window, p, mx, my, 11., Some(al(c(sw), o)), Some((1., al(line, 0.6 * o))));
                        if s.on { circ(window, p, mx, my, 15., None, Some((1.6, al(ink, o)))); }
                        else if hover { circ(window, p, mx, my, 15., None, Some((1.2, al(ink, 0.3 * o)))); }
                    } else {
                        if hover { circ(window, p, mx, my, 13., Some(al(ink, 0.08 * o)), None); }
                        circ(window, p, mx, my, 13., None, Some((1., al(line, 0.45 * o))));
                        stroke_line(window, p, (mx - 5., my), (mx + 5., my), 1.6, al(ink2, o));
                        if s.glyph == 2 { stroke_line(window, p, (mx, my - 5.), (mx, my + 5.), 1.6, al(ink2, o)); }
                    }
                }
                Style::Button => {
                    if hover { rq(window, p, *rc, hh / 2., Some(al(ink, 0.08 * o)), None, false); }
                    rq(window, p, *rc, hh / 2., None, Some((1., al(line, 0.4 * o))), false);
                    txt(window, cx, p, &s.text, rc[0] + 15., my, 13., FontWeight::NORMAL, al(ink, o), rc[2] + 4. - rc[0] - 15.);
                }
            }
            if sd.focus == Some((r, k)) {
                let rad = if st == Style::Swatches { 18. } else { hh / 2. + 3. };
                rq(window, p, [rc[0] - 3., rc[1] - 3., rc[2] + 3., rc[3] + 3.], rad, None, Some((1.8, al(c(th.tab_dark), 0.85 * o))), false);
            }
        }
        if let Row::Key(k @ (0 | 1)) = g.row {
            if sd.taken[k] {
                txt(window, cx, p, ui::SHORTCUT_TAKEN.get(), b[2] + 12., (b[1] + b[3]) / 2., 13., FontWeight::NORMAL, al(ink3, o), w - PAD - b[2] - 12.);
            }
        }
        if g.row == Row::Notes {
            let room = w - PAD - b[2] - 12.;
            let chars: Vec<char> = sd.dir.chars().collect();
            let mut shown = sd.dir.clone();
            let mut cut = 0;
            while cut < chars.len() && tw(window, &shown) > room {
                cut += 1;
                shown = format!("…{}", chars[cut..].iter().collect::<String>());
            }
            txt(window, cx, p, &shown, b[2] + 12., (b[1] + b[3]) / 2., 13., FontWeight::NORMAL, al(ink3, o), room + 4.);
        }
    }
}

fn paint_keys(sd: &SetDraw, rc: Rc4, s: &Seg, p: &Place, window: &mut Window, cx: &mut App) {
    let th = &sd.th;
    let o = sd.o;
    let my = (rc[1] + rc[3]) / 2.;
    if !s.text.is_empty() {
        let rr = [rc[0], rc[1] + 2., rc[2], rc[3] - 2.];
        let col = if s.on { c(th.tab_dark) } else { c(th.line) };
        if s.on { rq(window, p, rr, 8., Some(al(c(th.tab_dark), 0.08 * o)), None, false); }
        rq(window, p, rr, 8., None, Some((1.2, al(col, if s.on { o } else { 0.6 * o }))), true);
        txt(window, cx, p, &s.text, rc[0] + 14., my, 13., FontWeight::NORMAL, al(if s.on { c(th.ink) } else { c(th.ink3) }, o), rc[2] + 4. - rc[0] - 14.);
        return;
    }
    let (m, vk) = s.keys.unwrap_or((0, 0));
    let mut x = rc[0];
    for k in key_parts(m, vk) {
        let w = tw(window, &k).max(10.) + 18.;
        let (top, bot) = (rc[1] + 2., rc[3] - 4.);
        rq(window, p, [x, top + 2., x + w, bot + 2.], 7., Some(al(c(th.line), 0.45 * o)), None, false);
        rq(window, p, [x, top, x + w, bot], 7., Some(al(c(mix(th.paper(), th.ink, 0.07)), o)), None, false);
        rq(window, p, [x + 0.5, top + 0.5, x + w - 0.5, bot - 0.5], 6.5, None, Some((1., al(c(th.line), 0.35 * o))), false);
        txt_mid(window, cx, p, &k, x + w / 2., (top + bot) / 2., 13., FontWeight::MEDIUM, al(c(th.ink), o));
        x += w + 5.;
    }
}

fn paint_picker(sd: &SetDraw, p: &Place, window: &mut Window, cx: &mut App) {
    let Some((_, [h, s, v], hex, cur)) = sd.pick.clone() else { return };
    let th = &sd.th;
    let o = sd.o;
    let (sv, hue, cx0) = pick_geom(sd.w, sd.label_w, sd.end);
    let line = c(th.line);
    let white = hsla(0., 0., 1., 1.);
    let white0 = hsla(0., 0., 1., 0.);
    let black0 = hsla(0., 0., 0., 0.);
    let blk = hsla(0., 0., 0., 1.);
    rq(window, p, sv, 8., Some(al(c(hsv_rgb(h, 1., 1.)), o)), None, false);
    grad(window, p, sv, [8.; 4], 90., al(white, o), al(white0, o));
    grad(window, p, sv, [8.; 4], 180., al(black0, o), al(blk, o));
    rq(window, p, sv, 8., None, Some((1., al(line, 0.5 * o))), false);
    let n = 6;
    for i in 0..n {
        let (x0, x1) = (hue[0] + (hue[2] - hue[0]) * i as f32 / n as f32, hue[0] + (hue[2] - hue[0]) * (i + 1) as f32 / n as f32);
        let rad = [if i == 0 { 7. } else { 0. }, if i == n - 1 { 7. } else { 0. }, if i == n - 1 { 7. } else { 0. }, if i == 0 { 7. } else { 0. }];
        grad(window, p, [x0, hue[1], x1 + 0.5, hue[3]], rad, 90., al(c(hsv_rgb(i as f32 * 60., 1., 1.)), o), al(c(hsv_rgb((i + 1) as f32 * 60., 1., 1.)), o));
    }
    rq(window, p, hue, 7., None, Some((1., al(line, 0.5 * o))), false);
    let (kx, ky) = (sv[0] + s * (sv[2] - sv[0]), sv[1] + (1. - v) * (sv[3] - sv[1]));
    circ(window, p, kx, ky, 7., Some(al(c(hsv_rgb(h, s, v)), o)), Some((2., al(white, o))));
    circ(window, p, kx, ky, 8.5, None, Some((1., al(hsla(0., 0., 0., 0.35), o))));
    let hx = hue[0] + h / 360. * (hue[2] - hue[0]);
    let knob = [hx - 4., hue[1] - 3., hx + 4., hue[3] + 3.];
    rq(window, p, knob, 4., Some(al(c(hsv_rgb(h, 1., 1.)), o)), None, false);
    rq(window, p, knob, 4., None, Some((2., al(white, o))), false);
    rq(window, p, [cx0, sv[1], cx0 + 70., sv[1] + 40.], 10., Some(al(c(cur), o)), None, false);
    rq(window, p, [cx0, sv[1], cx0 + 70., sv[1] + 40.], 10., None, Some((1., al(line, 0.6 * o))), false);
    let code = if hex.is_empty() { format!("#{cur:06X}") } else { format!("#{}", hex.to_uppercase()) };
    txt(window, cx, p, &code, cx0, sv[1] + 59., 15., FontWeight::MEDIUM, al(if hex.is_empty() { c(th.ink) } else { c(th.ink2) }, o), 140.);
    txt(window, cx, p, ui::COLOR_CODE.get(), cx0, sv[1] + 82., 13., FontWeight::NORMAL, al(c(th.ink3), o), 160.);
    txt(window, cx, p, ui::COLOR_DONE.get(), cx0, sv[1] + 102., 13., FontWeight::NORMAL, al(c(th.ink3), o), 160.);
    if let Some((fr, _)) = sd.focus.map(|f| (f, 0)) {
        if fr.0 >= sd.rows.len() {
            rq(window, p, [sv[0] - 5., sv[1] - 5., sv[2] + 5., hue[3] + 7.], 12., None, Some((1.8, al(c(th.tab_dark), 0.85 * o))), false);
            txt(window, cx, p, ui::COLOR_KEYS.get(), sv[0], hue[3] + 22., 13., FontWeight::NORMAL, al(c(th.ink3), o), sd.w - PAD - sv[0]);
        }
    }
}

pub fn paint_settings(sd: &SetDraw, p: &Place, window: &mut Window, cx: &mut App) {
    if sd.rows.is_empty() { return; }
    paint_rows(sd, &sd.rows[..1], 0, p, window, cx);
    let (top, bottom) = sd.body;
    let tl = p.pt(0., top);
    let clip = Bounds::new(tl, size(px(sd.w * p.sc), px((bottom - top) * p.sc)));
    let moved = Place { y: p.y - sd.offset * p.sc, ..*p };
    let faded;
    let sd = if sd.page_o < 0.999 { faded = SetDraw { o: sd.o * sd.page_o, ..sd.clone() }; &faded } else { sd };
    let o = sd.o;
    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
        paint_rows(sd, &sd.rows[1..], 1, &moved, window, cx);
        if sd.pick.is_some() && sd.page == 2 { paint_picker(sd, &moved, window, cx); }
        for (k, line) in sd.help.iter().flatten().enumerate() {
            txt(window, cx, &moved, line, PAD, sd.end + 10. + k as f32 * 22. + 10., 13., FontWeight::NORMAL, al(c(sd.th.ink3), o), sd.w - 2. * PAD);
        }
    });
    if let Some((y, t)) = sd.thumb {
        let x = sd.w - 12.;
        rq(window, p, [x, y, x + 4., y + t], 2., Some(al(c(sd.th.ink2), 0.4 * o)), None, false);
    }
}

// ---- state, geometry, input ---------------------------------------------------

impl Shell {
    fn set_rows(&self) -> Vec<(String, Row, Vec<Seg>)> {
        let p = &self.prefs;
        let k = theme_kind(p);
        let page = self.page;
        let mut rows = vec![(String::new(), Row::Page, vec![seg(ui::GENERAL.get(), page == 0), seg(ui::SHORTCUTS.get(), page == 1), seg(ui::COLORS.get(), page == 2)])];
        let theme = || (ui::THEME.get().to_string(), Row::Theme, vec![seg(ui::PAPER.get(), p.theme == 0), seg(ui::NIGHT.get(), p.theme == 1), seg(ui::SYSTEM_THEME.get(), p.theme == 2), seg(ui::CUSTOM_THEME.get(), p.theme == 3)]);
        let glyph = |g: u8| Seg { glyph: g, ..seg("", false) };
        match page {
            0 => {
                let mut languages = vec![seg(ui::SYSTEM_LANGUAGE.get(), p.language.is_none())];
                languages.extend(Language::ALL.iter().map(|l| seg(l.name(), p.language == Some(*l))));
                rows.push((ui::LANGUAGE.get().into(), Row::Language, languages));
                rows.push((ui::EDITOR.get().into(), Row::Editor, vec![seg("Markdown", !p.simple_editor), seg(ui::SIMPLE.get(), p.simple_editor)]));
                rows.push(theme());
                rows.push((ui::TEXT_SIZE.get().into(), Row::Scale, vec![glyph(1), Seg { value: true, ..seg(format!("{}%", p.scale), false) }, glyph(2)]));
                rows.push((ui::APPEARANCE.get().into(), Row::Dir, vec![seg(ui::BOTTOM.get(), p.dir == 0), seg(ui::RIGHT.get(), p.dir == 1), seg(ui::LEFT.get(), p.dir == 2), seg(ui::TOP.get(), p.dir == 3), seg(ui::IN_PLACE.get(), p.dir == 4)]));
                rows.push((ui::SLIDE_DISTANCE.get().into(), Row::Far, vec![seg(ui::NEAR.get(), !p.far), seg(ui::OFF_SCREEN.get(), p.far)]));
                rows.push((ui::SPEED.get().into(), Row::Speed, vec![seg(ui::FAST.get(), p.speed == 0), seg(ui::NORMAL.get(), p.speed == 1), seg(ui::SLOW.get(), p.speed == 2)]));
                let auto = settings::autostart();
                rows.push((ui::START_WITH_WINDOWS.get().into(), Row::Auto, vec![seg(ui::ON.get(), auto), seg(ui::OFF.get(), !auto)]));
                rows.push((ui::NOTES_FOLDER.get().into(), Row::Notes, vec![seg(ui::OPEN.get(), false), seg(ui::MOVE.get(), false)]));
            }
            1 => {
                for i in 0..KEYS {
                    let text = if self.capture == Some(i) { ui::PRESS_SHORTCUT.get().to_string() } else if p.keys[i].1 == 0 { ui::NOT_SET.get().to_string() } else { String::new() };
                    rows.push((ui::KEY_NAMES[i].get().to_string(), Row::Key(i), vec![Seg { keys: Some(p.keys[i]), ..seg(text, self.capture == Some(i)) }]));
                }
            }
            _ => {
                rows.push(theme());
                let sw = |c: u32, text: &str, on: bool| Seg { swatch: Some(c), ..seg(text, on) };
                let own_ink = p.ink[k].is_some() || k == 2;
                rows.push((ui::TEXT.get().into(), Row::Ink, vec![sw(self.th.ink, ui::CUSTOM_COLOR.get(), own_ink), seg(ui::FROM_THEME.get(), !own_ink)]));
                rows.push((ui::ACCENT.get().into(), Row::Accent, vec![sw(self.th.tab_dark, ui::CUSTOM_COLOR.get(), p.accent[k].is_some()), seg(ui::FROM_THEME.get(), p.accent[k].is_none())]));
                let n = p.ncards;
                let mut cards: Vec<Seg> = (0..n).map(|i| sw(self.th.note(i), "", self.st.pick == Some(Pick::Card(i)))).collect();
                if n < MAX_CARDS { cards.push(glyph(2)); }
                if n > 1 { cards.push(glyph(1)); }
                rows.push((ui::CARDS.get().into(), Row::Cards, cards));
                rows.push((String::new(), Row::Reset, vec![seg(ui::RESET_COLORS.get(), false)]));
            }
        }
        rows
    }

    fn label_w(&self, w: f32) -> f32 {
        let base = match self.page { 1 => 250.0, 2 => 130.0, _ => SET_LABEL };
        if w < 640.0 { base * 0.8 } else { base }
    }
    fn row_h(&self) -> f32 { if self.page == 1 { 37.0 } else { SET_ROW } }
    fn body_top(&self) -> f32 { SET_TOP + SET_ROW + 14.0 }
    fn body_bottom(&self) -> f32 { self.edit_h - SET_BOTTOM }
    fn scroll_max(&self, end: f32) -> f32 {
        let extra = match self.page { 1 => 52.0, 2 if self.st.pick.is_some() => 172.0, _ => 30.0 };
        (end + extra - self.body_bottom()).max(0.0)
    }
    pub fn offset(&self, end: f32) -> f32 { self.st.scroll.min(self.scroll_max(end)) }
    fn thumb(&self, end: f32) -> Option<(f32, f32)> {
        let max = self.scroll_max(end);
        if max == 0.0 { return None; }
        let h = self.body_bottom() - self.body_top();
        let size = (h * h / (h + max)).max(18.0);
        Some((self.body_top() + (h - size) * self.offset(end) / max, size))
    }

    /// the rows laid out: label, block, each part's box; and where the rows end.
    /// A block that does not fit beside its label moves under it.
    pub fn set_geom(&self, window: &mut Window, w: f32) -> (Vec<RowG>, f32) {
        let mut out = vec![];
        let mut y = SET_TOP;
        let rh0 = self.row_h();
        let lw_all = self.label_w(w);
        for (label, row, segs) in self.set_rows() {
            let st = style(row);
            let rh = if row == Row::Page { SET_ROW } else { rh0 };
            let widths: Vec<f32> = segs.iter().map(|s| match st {
                Style::Choice => tw(window, &s.text) + 30.0 + if s.swatch.is_some() { 20.0 } else { 0.0 },
                Style::Step => if s.value { tw(window, &s.text) + 24.0 } else { 36.0 },
                Style::Keys => {
                    if !s.text.is_empty() { tw(window, &s.text) + 28.0 }
                    else {
                        let (m, vk) = s.keys.unwrap_or((0, 0));
                        key_parts(m, vk).iter().map(|k| tw(window, k).max(10.0) + 18.0 + 5.0).sum::<f32>() - 5.0
                    }
                }
                Style::Swatches => 30.0,
                Style::Button => tw(window, &s.text) + 30.0,
            }).collect();
            let gap = match st { Style::Swatches => 8.0, Style::Choice | Style::Step => 0.0, _ => 6.0 };
            let total: f32 = widths.iter().sum::<f32>() + gap * (widths.len().max(1) - 1) as f32;
            let lw = if label.is_empty() { 0.0 } else { lw_all };
            let mut x = PAD + lw;
            let mut top = y;
            let mut h = rh;
            if x + total > w - PAD && lw > 0.0 { x = PAD; top = y + 34.0; h = rh + 34.0; }
            let by = top + (rh - CTL_H) / 2.0;
            let block = [x, by, x + total, by + CTL_H];
            let mut parts = vec![];
            for (s, wd) in segs.into_iter().zip(widths) {
                parts.push(([x, by, x + wd, by + CTL_H], s));
                x += wd + gap;
            }
            out.push(RowG { row, label, top: y, h: rh, block, segs: parts });
            y += h + if row == Row::Page { 14.0 } else { 0.0 };
        }
        (out, y)
    }

    /// the sliding part of each choice block follows the chosen option
    pub fn sync_segs(&mut self, rows: &[RowG]) -> bool {
        let page = self.page;
        let mut moved = false;
        for g in rows {
            if style(g.row) != Style::Choice { continue; }
            let Some((rc, _)) = g.segs.iter().find(|(_, s)| s.on) else { continue };
            let e = self.st.seg.entry((page, g.row)).or_insert([Ease::at(rc[0]), Ease::at(rc[2])]);
            if e[0].to != rc[0] || e[1].to != rc[2] { e[0].to = rc[0]; e[1].to = rc[2]; moved = true; }
        }
        moved
    }

    pub fn pick_color(&self) -> u32 {
        match self.st.pick { Some(Pick::Ink) => self.th.ink, Some(Pick::Accent) => self.th.tab_dark, Some(Pick::Card(i)) => self.th.note(i), None => 0 }
    }

    pub fn set_draw(&mut self, window: &mut Window, cx: &mut Context<Self>, w: f32, o: f32) -> SetDraw {
        let (rows, end) = self.set_geom(window, w);
        if self.sync_segs(&rows) || self.st.fade.v != self.st.fade.to { self.start_anim(window, cx); }
        let pills = self.st.seg.iter().filter(|((pg, _), _)| *pg == self.page).map(|((_, r), e)| (*r, (e[0].v, e[1].v))).collect();
        let help = match self.page {
            0 => [Some(ui::GENERAL_HELP.get()), None],
            1 => [Some(ui::SHORTCUT_HELP.get()), Some(ui::GLOBAL_HELP.get())],
            _ if self.st.pick.is_none() => [Some(ui::COLOR_HELP.get()), None],
            _ => [None, None],
        };
        let offset = self.offset(end);
        let cur = self.pick_color();
        SetDraw {
            page_o: self.st.fade.v.clamp(0., 1.), th: self.th, w, eh: self.edit_h, end, offset, body: (self.body_top(), self.body_bottom()), pills,
            hover: self.st.hover, focus: if self.st.kb { Some(self.st.focus) } else { None }, page: self.page,
            pick: self.st.pick.map(|p| (p, self.st.hsv, self.st.hex.clone(), cur)),
            thumb: self.thumb(end), dir: self.dir.to_string_lossy().to_string(), label_w: self.label_w(w), o, help, rows,
            taken: [system::hotkey_taken(0), system::hotkey_taken(1)],
        }
    }

    fn end_capture(&mut self) { self.capture = None; }

    pub fn scroll_settings(&mut self, dy: f32, end: f32, cx: &mut Context<Self>) {
        self.st.scroll_to = (self.st.scroll_to + dy).clamp(0.0, self.scroll_max(end));
        self.st.hover = None;
        self.st.scroll_wake = true;
        cx.notify();
    }

    fn show_setting(&mut self, top: f32, bottom: f32, end: f32) {
        let offset = self.st.scroll_to.min(self.scroll_max(end));
        let target = if top - offset < self.body_top() { top - self.body_top() }
            else if bottom - offset > self.body_bottom() { bottom - self.body_bottom() }
            else { offset };
        self.st.scroll_to = target.clamp(0.0, self.scroll_max(end));
        self.st.scroll_wake = true;
    }

    pub fn turn_page(&mut self, d: isize, cx: &mut Context<Self>) {
        self.page = ((self.page as isize + d).rem_euclid(3)) as usize;
        self.st.fade = Ease::at(0.); self.st.fade.to = 1.;
        self.st.scroll = 0.0;
        self.st.scroll_to = 0.0;
        self.st.pick = None;
        self.end_capture();
        self.st.focus = (0, self.page);
        cx.notify();
    }

    fn start_pick(&mut self, p: Pick, window: &mut Window, w: f32) {
        if self.st.pick == Some(p) { self.st.pick = None; return; }
        self.st.pick = Some(p);
        self.st.hsv = rgb_hsv(self.pick_color());
        self.st.hex.clear();
        let end = self.set_geom(window, w).1;
        self.show_setting(end, end + 147.0, end);
    }

    /// the edited colour changes; while dragging only the look updates, the rest happens on release
    pub fn set_pick_color(&mut self, col: u32, done: bool, window: &mut Window, cx: &mut Context<Self>) {
        let k = theme_kind(&self.prefs);
        let mut p = self.prefs;
        match self.st.pick { Some(Pick::Ink) => p.ink[k] = Some(col), Some(Pick::Accent) => p.accent[k] = Some(col), Some(Pick::Card(i)) => p.cards[k][i] = col, None => return }
        self.prefs = p;
        if done { self.apply_prefs(window, cx); } else { self.th = AppTheme::new(&self.prefs); cx.notify(); }
    }

    pub fn pick_apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.st.hex.clear();
        let [h, s, v] = self.st.hsv;
        self.set_pick_color(hsv_rgb(h, s, v), true, window, cx);
    }

    pub fn pick_drag(&mut self, part: u8, x: f32, y: f32, w: f32, end: f32, window: &mut Window, cx: &mut Context<Self>, done: bool) {
        let (sv, hue, _) = pick_geom(w, self.label_w(w), end);
        let fx = |r: &Rc4| ((x - r[0]) / (r[2] - r[0])).clamp(0.0, 1.0);
        if part == 0 { self.st.hsv[1] = fx(&sv); self.st.hsv[2] = 1.0 - ((y - sv[1]) / (sv[3] - sv[1])).clamp(0.0, 1.0); }
        else { self.st.hsv[0] = fx(&hue) * 359.9; }
        self.st.hex.clear();
        let [h, s, v] = self.st.hsv;
        self.set_pick_color(hsv_rgb(h, s, v), done, window, cx);
    }

    /// a character typed while the picker is open: a hex code
    pub fn pick_char(&mut self, ch: Option<char>, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if back { self.st.hex.pop(); }
        else if let Some(ch) = ch.filter(|c| c.is_ascii_hexdigit()) {
            if self.st.hex.len() >= 6 { self.st.hex.clear(); }
            self.st.hex.push(ch);
        } else { return false; }
        if self.st.hex.len() == 6 {
            if let Some(v) = settings::hex(&self.st.hex) {
                self.st.hsv = rgb_hsv(v);
                self.set_pick_color(v, true, window, cx);
            }
        }
        cx.notify();
        true
    }

    pub fn set_click(&mut self, r: usize, i: usize, window: &mut Window, w: f32, cx: &mut Context<Self>) {
        let rows = self.set_rows();
        let Some((_, row, segs)) = rows.get(r) else { return };
        let glyph = segs.get(i).map(|s| s.glyph).unwrap_or(0);
        let mut p = self.prefs;
        let k = theme_kind(&p);
        let mut demo = false;
        match *row {
            Row::Page => { self.page = i; self.st.fade = Ease::at(0.); self.st.fade.to = 1.; self.st.scroll = 0.0; self.st.scroll_to = 0.0; self.st.pick = None; self.end_capture(); self.st.focus = (0, i); cx.notify(); return; }
            Row::Language => p.language = if i == 0 { None } else { Some(Language::ALL[i - 1]) },
            Row::Editor => p.simple_editor = i == 1,
            Row::Theme => { p.theme = i as u8; self.st.pick = None; }
            Row::Scale => p.scale = if i == 0 { p.scale.saturating_sub(10).max(80) } else { (p.scale + 10).min(160) },
            Row::Dir => { p.dir = i as u8; demo = true; }
            Row::Far => { p.far = i == 1; demo = true; }
            Row::Speed => { p.speed = i as u8; demo = true; }
            Row::Auto => { settings::set_autostart(i == 0); cx.notify(); return; }
            Row::Key(n) => {
                if self.capture == Some(n) { self.capture = None; } else { self.capture = Some(n); window.focus(&self.focus); }
                cx.notify();
                return;
            }
            Row::Ink => if i == 0 { self.start_pick(Pick::Ink, window, w); } else { p.ink[k] = None; if self.st.pick == Some(Pick::Ink) { self.st.pick = None; } },
            Row::Accent => if i == 0 { self.start_pick(Pick::Accent, window, w); } else { p.accent[k] = None; if self.st.pick == Some(Pick::Accent) { self.st.pick = None; } },
            Row::Cards => {
                if i < p.ncards { self.start_pick(Pick::Card(i), window, w); }
                else if glyph == 2 { p.ncards += 1; }
                else if glyph == 1 { p.ncards -= 1; if self.st.pick == Some(Pick::Card(p.ncards)) { self.st.pick = None; } }
            }
            Row::Notes => {
                if i == 0 { let _ = std::process::Command::new("explorer").arg(&self.dir).spawn(); } else { self.move_folder(window, cx); }
                return;
            }
            Row::Reset => { p.cards[k] = DEFAULT_CARDS[k]; p.ink[k] = None; p.accent[k] = None; self.st.pick = None; }
        }
        self.prefs = p;
        self.apply_prefs(window, cx);
        // show the new way in and out right away
        if demo { self.mo.demo = true; self.hide(window, cx); }
    }

    /// keyboard in the settings: arrows and Tab move, Enter or Space presses, Ctrl+Tab turns the page
    pub fn settings_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let m = &ev.keystroke.modifiers;
        let (ctrl, shift) = (m.control, m.shift);
        let key = ev.keystroke.key.as_str();
        let w = f32::from(window.viewport_size().width) - 2.;
        let (rows, end) = self.set_geom(window, w);
        let picker = self.st.pick.is_some() && self.page == 2;
        let n = rows.len() + picker as usize;
        let on_picker = picker && self.st.focus.0 >= rows.len();
        let chosen = |r: usize| rows.get(r).and_then(|g| g.segs.iter().position(|(_, s)| s.on)).unwrap_or(0);
        let goto = |s: &mut Shell, r: usize| { s.st.focus = (r, if r < rows.len() && style(rows[r].row) == Style::Choice { chosen(r) } else { 0 }); };
        match key {
            "tab" if ctrl => self.turn_page(if shift { -1 } else { 1 }, cx),
            "pagedown" if ctrl => self.turn_page(1, cx),
            "pageup" if ctrl => self.turn_page(-1, cx),
            "pagedown" | "pageup" => {
                let distance = self.body_bottom() - self.body_top() - CTL_H;
                self.scroll_settings(if key == "pagedown" { distance } else { -distance }, end, cx);
                self.st.kb = false;
                return true;
            }
            "up" | "down" if on_picker && ctrl => {
                let d = if key == "up" { 0.04 } else { -0.04 };
                self.st.hsv[2] = (self.st.hsv[2] + d).clamp(0.0, 1.0);
                self.pick_apply(window, cx);
            }
            "tab" | "up" | "down" => {
                let d: isize = if key == "up" || (key == "tab" && shift) { -1 } else { 1 };
                let r = if self.st.kb { ((self.st.focus.0 as isize + d).rem_euclid(n as isize)) as usize } else { 0 };
                goto(self, r);
            }
            "left" | "right" => {
                let d: isize = if key == "left" { -1 } else { 1 };
                if !self.st.kb { goto(self, 0); }
                else if on_picker {
                    if ctrl { self.st.hsv[1] = (self.st.hsv[1] + d as f32 * 0.04).clamp(0.0, 1.0); }
                    else { self.st.hsv[0] = (self.st.hsv[0] + d as f32 * if shift { 1.0 } else { 6.0 }).rem_euclid(360.0); }
                    self.pick_apply(window, cx);
                } else if let Some(g) = rows.get(self.st.focus.0) {
                    let len = g.segs.len() as isize;
                    match style(g.row) {
                        Style::Choice => {
                            let i = (chosen(self.st.focus.0) as isize + d).clamp(0, len - 1) as usize;
                            let r = self.st.focus.0;
                            self.set_click(r, i, window, w, cx);
                            self.st.focus = (r, i);
                        }
                        Style::Step => { let r = self.st.focus.0; self.set_click(r, if d < 0 { 0 } else { 2 }, window, w, cx); }
                        _ => self.st.focus.1 = (self.st.focus.1 as isize + d).clamp(0, len - 1) as usize,
                    }
                }
            }
            "enter" | "space" => {
                if !self.st.kb { goto(self, 0); }
                else if on_picker { self.st.pick = None; self.st.focus.0 = rows.len().saturating_sub(1); }
                else if rows.get(self.st.focus.0).is_some() {
                    let (r, i) = self.st.focus;
                    let before = self.st.pick;
                    self.set_click(r, i, window, w, cx);
                    if self.st.pick.is_some() && self.st.pick != before { self.st.focus = (self.set_geom(window, w).0.len(), 0); }
                }
            }
            _ => {
                // anything else typed while a colour is open is its hex code
                if self.st.pick.is_some() && self.page == 2 && !ctrl && !m.alt {
                    let ch = ev.keystroke.key_char.as_deref().and_then(|s| s.chars().next());
                    return self.pick_char(ch, key == "backspace", window, cx);
                }
                return false;
            }
        }
        let (rows, end) = self.set_geom(window, w);
        let n = rows.len() + (self.st.pick.is_some() && self.page == 2) as usize;
        if self.st.focus.0 >= n { self.st.focus.0 = n.saturating_sub(1); }
        if let Some(g) = rows.get(self.st.focus.0) { self.st.focus.1 = self.st.focus.1.min(g.segs.len().saturating_sub(1)); }
        if let Some(g) = rows.get(self.st.focus.0).filter(|g| g.row != Row::Page) {
            self.show_setting(g.top, g.block[3] + 4.0, end);
        } else if self.st.focus.0 >= rows.len() {
            self.show_setting(end, end + 172.0, end);
        }
        self.st.kb = true;
        self.start_anim(window, cx);
        cx.notify();
        true
    }

    /// the clickable parts of the page: one box per button, the scroll thumb, the picker areas
    pub fn set_hits(&mut self, sd: &SetDraw, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = vec![];
        let w = sd.w;
        let (top, bottom) = sd.body;
        let mut body = div().absolute().left(px(0.)).top(px(1. + top)).w(px(w + 2.)).h(px(bottom - top)).overflow_hidden();
        for (r, g) in sd.rows.iter().enumerate() {
            for (i, (rc, s)) in g.segs.iter().enumerate() {
                if s.value { continue; }
                let el = div().id(("set", r * 64 + i)).absolute().w(px(rc[2] - rc[0])).h(px(rc[3] - rc[1] + 8.)).cursor_pointer()
                    .on_hover(cx.listener(move |s, h: &bool, _, cx| {
                        let now = if *h { Some((r, i)) } else if s.st.hover == Some((r, i)) { None } else { s.st.hover };
                        if now != s.st.hover { s.st.hover = now; cx.notify(); }
                    }))
                    .on_mouse_down(MouseButton::Left, cx.listener(move |s, _, window, cx| {
                        let w = f32::from(window.viewport_size().width) - 2.;
                        s.st.kb = false;
                        s.set_click(r, i, window, w, cx);
                        s.focus_main(window, cx);
                    }));
                if r == 0 {
                    out.push(el.left(px(1. + rc[0])).top(px(1. + rc[1] - 4.)).into_any_element());
                } else {
                    body = body.child(el.left(px(1. + rc[0])).top(px(rc[1] - 4. - top - sd.offset)));
                }
            }
        }
        if sd.pick.is_some() && sd.page == 2 {
            let (sv, hue, _) = pick_geom(w, sd.label_w, sd.end);
            for (k, rc) in [sv, hue].into_iter().enumerate() {
                let part = k as u8;
                body = body.child(div().id(("pick", k)).absolute().left(px(1. + rc[0] - 6.)).top(px(rc[1] - 6. - top - sd.offset)).w(px(rc[2] - rc[0] + 12.)).h(px(rc[3] - rc[1] + 12.)).cursor_pointer()
                    .on_mouse_down(MouseButton::Left, cx.listener(move |s, ev: &MouseDownEvent, window, cx| {
                        let w = f32::from(window.viewport_size().width) - 2.;
                        let end = s.set_geom(window, w).1;
                        s.st.pick_drag = Some(part);
                        let (x, y) = (f32::from(ev.position.x) - 1., f32::from(ev.position.y) - 1. + s.offset(end));
                        s.pick_drag(part, x, y, w, end, window, cx, false);
                    })));
            }
        }
        out.push(body.into_any_element());
        if let Some((y, t)) = sd.thumb {
            out.push(div().id("thumb").absolute().left(px(1. + w - 20.)).top(px(1. + sd.body.0)).w(px(20. - GRIP)).h(px(sd.body.1 - sd.body.0)).cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(move |s, ev: &MouseDownEvent, window, cx| {
                    let w = f32::from(window.viewport_size().width) - 2.;
                    let end = s.set_geom(window, w).1;
                    let my = f32::from(ev.position.y) - 1.;
                    let Some((top, size)) = s.thumb(end) else { return };
                    let grab = if (top..top + size).contains(&my) { my - top } else { size / 2. };
                    s.st.thumb_drag = Some(grab);
                    s.drag_thumb(my, grab, end, cx);
                    let _ = (y, t);
                })).into_any_element());
        }
        out
    }

    pub fn drag_thumb(&mut self, y: f32, grab: f32, end: f32, cx: &mut Context<Self>) {
        let Some((_, size)) = self.thumb(end) else { return };
        let top = self.body_top();
        let travel = self.body_bottom() - top - size;
        self.st.scroll = ((y - grab - top) / travel).clamp(0.0, 1.0) * self.scroll_max(end);
        self.st.scroll_to = self.st.scroll;
        self.st.hover = None;
        cx.notify();
    }

    /// pointer moves and releases that belong to a drag started on the page
    pub fn set_pointer(&mut self, pos: Point<Pixels>, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Settings { return; }
        let w = f32::from(window.viewport_size().width) - 2.;
        let (x, y) = (f32::from(pos.x) - 1., f32::from(pos.y) - 1.);
        if let Some(grab) = self.st.thumb_drag {
            let end = self.set_geom(window, w).1;
            if up { self.st.thumb_drag = None; } else { self.drag_thumb(y, grab, end, cx); }
        }
        if let Some(part) = self.st.pick_drag {
            let end = self.set_geom(window, w).1;
            let yy = y + self.offset(end);
            if up { self.st.pick_drag = None; }
            self.pick_drag(part, x, yy, w, end, window, cx, up);
        }
    }
}
