//! The look of Notedd: a paper folder with a tab, a dark tab segment, soft shadow, line icons.
//! The numbers are the ones of the Direct2D app (DIPs).

use gpui::*;

pub const T: f32 = 34.0;
pub const R: f32 = 20.0;
pub const G: f32 = 6.0;
pub const F: f32 = 12.0;
pub const RT: f32 = 14.0;
pub const PAD: f32 = 40.0;
pub const PADB: f32 = 26.0;
pub const DARK: f32 = 34.0;
pub const DARK_HOVER: f32 = 42.0;
pub const DARK_FIND: f32 = 136.0;
pub const DARK_SEARCH: f32 = 104.0;
pub const EDIT_TOP: f32 = T + 36.0;

/// the tab's left and right edge (x2, bx) for a folder `width` wide
pub fn tab_x(width: f32) -> (f32, f32) {
    let x2 = R + G + F;
    (x2, x2 + (width * 0.34).round().clamp(180.0, 300.0))
}

/// folder space -> window space: scale around the top centre, then move down by `dy`
#[derive(Clone, Copy)]
pub struct Place { pub x: f32, pub y: f32, pub sc: f32, pub w: f32, pub rot: f32, pub sx: f32 }

impl Place {
    /// lean about the top centre by `rot`, move by `sx`, scale about the top centre
    pub fn pt(&self, x: f32, y: f32) -> Point<Pixels> {
        let cx = self.w / 2.0;
        let (s, c) = self.rot.sin_cos();
        let dx = x - cx;
        let (rx, ry) = (dx * c - y * s, dx * s + y * c);
        point(px(self.x + cx + (rx + self.sx) * self.sc), px(self.y + ry * self.sc))
    }
    /// the same place without the lean: text is placed here and leaned by the glyph transform
    pub fn flat(&self) -> Place { Place { rot: 0.0, ..*self } }
    pub fn r(&self, r: f32) -> Point<Pixels> { point(px(r * self.sc), px(r * self.sc)) }
}

fn arc(b: &mut PathBuilder, p: &Place, r: f32, x: f32, y: f32, sweep: bool) {
    b.arc_to(p.r(r), px(0.), false, sweep, p.pt(x, y));
}

fn folder_builder(mut b: PathBuilder, p: &Place, h: f32, tab: (f32, f32), radii: (f32, f32, f32, f32)) -> PathBuilder {
    let w = p.w;
    let (x2, bx) = tab;
    let (tl, tr, bl, br) = radii;
    b.move_to(p.pt(0.0, h - bl));
    b.line_to(p.pt(0.0, T + tl)); if tl > 0.05 { arc(&mut b, p, tl, tl, T, true); }
    b.line_to(p.pt(x2 - F, T)); arc(&mut b, p, F, x2, T - F, false);
    b.line_to(p.pt(x2, RT)); arc(&mut b, p, RT, x2 + RT, 0.0, true);
    b.line_to(p.pt(bx - RT, 0.0)); arc(&mut b, p, RT, bx, RT, true);
    b.line_to(p.pt(bx, T - F)); arc(&mut b, p, F, bx + F, T, false);
    b.line_to(p.pt(w - tr, T)); if tr > 0.05 { arc(&mut b, p, tr, w, T + tr, true); }
    b.line_to(p.pt(w, h - br)); if br > 0.05 { arc(&mut b, p, br, w - br, h, true); }
    b.line_to(p.pt(bl, h)); if bl > 0.05 { arc(&mut b, p, bl, 0.0, h - bl, true); }
    b.close();
    b
}

pub fn paint_folder(window: &mut Window, p: &Place, h: f32, color: Hsla, line: Hsla, tab: (f32, f32), alpha: f32, radii: (f32, f32, f32, f32)) {
    let mut c = color; c.a *= alpha;
    let mut l = line; l.a *= alpha;
    if let Ok(path) = folder_builder(PathBuilder::fill(), p, h, tab, radii).build() { window.paint_path(path, c); }
    if let Ok(path) = folder_builder(PathBuilder::stroke(px(1.5)), p, h, tab, radii).build() { window.paint_path(path, l); }
}

/// the dark segment right of the tab, `dw` wide
pub fn paint_dark(window: &mut Window, p: &Place, bx: f32, dw: f32, color: Hsla) {
    let c = bx + dw;
    let mut b = PathBuilder::fill();
    b.move_to(p.pt(bx - RT, T + 2.0));
    b.line_to(p.pt(bx - RT, 0.0));
    b.line_to(p.pt(c - RT, 0.0)); arc(&mut b, p, RT, c, RT, true);
    b.line_to(p.pt(c, T - F)); arc(&mut b, p, F, c + F, T, false);
    b.line_to(p.pt(c + F, T + 2.0));
    b.close();
    if let Ok(path) = b.build() { window.paint_path(path, color); }
}

// ---- icons: 16x16 SVG path data, commands M L H V Z (absolute and relative) -------------

pub const CLOSE: &str = "M4.5 4.5l7 7M11.5 4.5l-7 7";
pub const I_PIN: &str = "M9.8 2.5l3.7 3.7-2.3.8-2.4 2.4.3 2.8-1 1-2.1-2.1L3 14l-.1-.1L6 11 3.9 8.9l1-1 2.8.3 2.4-2.4z";
pub const FOLDERS: &str = "M2.5 5.5h4l1.2-1.6h5.8v8.6h-11zM2.5 8h11";
pub const FIND: &str = "M10.5 10.5L14 14";
pub const I_NEW: &str = "M8 3v10M3 8h10";
pub const PLUS: &str = "M8 2.5v11M2.5 8h11";
pub const UP: &str = "M4 10l4-4 4 4";
pub const DOWN: &str = "M4 6l4 4 4-4";
pub const MORE: &str = "M9.5 4.5L6 8l3.5 3.5";
pub const TRASH: &str = "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5L5.2 13h5.6l.7-8.5";
pub const GEAR: &str = "M2.5 5h11M2.5 11h11";

fn numbers(d: &str) -> Vec<(Option<char>, f32)> {
    // tokens: a command letter (value NaN) or a number
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() { out.push((Some(c), f32::NAN)); i += 1; }
        else if c == '-' || c == '.' || c.is_ascii_digit() {
            let s = i;
            if c == '-' { i += 1; }
            let mut dot = false;
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() { i += 1; } else if ch == '.' && !dot { dot = true; i += 1; } else { break; }
            }
            out.push((None, d[s..i].parse().unwrap_or(0.0)));
        } else { i += 1; }
    }
    out
}

/// draws path data with its 16x16 box centred on (cx, cy), `size` px wide
pub fn glyph(window: &mut Window, d: &str, cx: f32, cy: f32, size: f32, stroke: f32, color: Hsla, filled: bool) {
    glyph_x(window, d, cx, cy, size, stroke, color, filled, 1.0);
}

/// `sx` squeezes the icon horizontally around its centre (-1 mirrors it)
pub fn glyph_x(window: &mut Window, d: &str, cx: f32, cy: f32, size: f32, stroke: f32, color: Hsla, filled: bool, sx: f32) {
    let k = size / 16.0;
    let (ox, oy) = (cx - 8.0 * k, cy - 8.0 * k);
    let p = |x: f32, y: f32| point(px(ox + (8.0 + (x - 8.0) * sx) * k), px(oy + y * k));
    let mut b = if filled { PathBuilder::fill() } else { PathBuilder::stroke(px(stroke)) };
    let toks = numbers(d);
    let (mut x, mut y, mut sx, mut sy) = (0f32, 0f32, 0f32, 0f32);
    let mut cmd = 'M';
    let mut open = false;
    let mut j = 0;
    while j < toks.len() {
        if let (Some(c), _) = toks[j] {
            cmd = c;
            j += 1;
            if c == 'z' || c == 'Z' { if open { b.close(); open = false; } x = sx; y = sy; }
            continue;
        }
        let n = match cmd { 'H' | 'h' | 'V' | 'v' => 1, _ => 2 };
        let mut a = [0f32; 2];
        for t in 0..n { if let Some((None, v)) = toks.get(j + t) { a[t] = *v; } }
        j += n;
        let rel = cmd.is_ascii_lowercase();
        match cmd.to_ascii_uppercase() {
            'M' => {
                if rel { x += a[0]; y += a[1]; } else { x = a[0]; y = a[1]; }
                sx = x; sy = y;
                b.move_to(p(x, y));
                open = true;
                cmd = if rel { 'l' } else { 'L' };
            }
            'L' => { if rel { x += a[0]; y += a[1]; } else { x = a[0]; y = a[1]; } b.line_to(p(x, y)); }
            'H' => { if rel { x += a[0]; } else { x = a[0]; } b.line_to(p(x, y)); }
            'V' => { if rel { y += a[0]; } else { y = a[0]; } b.line_to(p(x, y)); }
            _ => {}
        }
    }
    if let Ok(path) = b.build() { window.paint_path(path, color); }
}

pub fn circle(window: &mut Window, cx: f32, cy: f32, r: f32, stroke: Option<(f32, Hsla)>, fill_color: Option<Hsla>) {
    let ring = |window: &mut Window, r: f32, mut b: PathBuilder, color: Hsla| {
        b.move_to(point(px(cx + r), px(cy)));
        b.arc_to(point(px(r), px(r)), px(0.), false, true, point(px(cx - r), px(cy)));
        b.arc_to(point(px(r), px(r)), px(0.), false, true, point(px(cx + r), px(cy)));
        b.close();
        if let Ok(p) = b.build() { window.paint_path(p, color); }
    };
    if let Some(c) = fill_color { ring(window, r, PathBuilder::fill(), c); }
    if let Some((w, c)) = stroke { ring(window, r - w / 2.0, PathBuilder::stroke(px(w)), c); }
}

/// the magnifier: circle at (7,7) r 4.5 plus the handle
pub fn find_icon(window: &mut Window, cx: f32, cy: f32, size: f32, stroke: f32, color: Hsla) {
    let k = size / 16.0;
    glyph(window, FIND, cx, cy, size, stroke, color, false);
    circle(window, cx - 8.0 * k + 7.0 * k, cy - 8.0 * k + 7.0 * k, 4.5 * k + stroke / 2.0, Some((stroke, color)), None);
}

pub fn gear_icon(window: &mut Window, cx: f32, cy: f32, size: f32, stroke: f32, color: Hsla, paper: Hsla) {
    let k = size / 16.0;
    glyph(window, GEAR, cx, cy, size, stroke, color, false);
    for (x, y) in [(5.5, 5.0), (10.5, 11.0)] {
        circle(window, cx - 8.0 * k + x * k, cy - 8.0 * k + y * k, 1.9 * k + stroke / 2.0, Some((stroke, color)), Some(paper));
    }
}

/// text on one line, cut with an ellipsis when wider than `max`
pub fn text(window: &mut Window, cx: &mut App, s: &str, at: Point<Pixels>, size: f32, weight: FontWeight, color: Hsla, max: f32) {
    if s.is_empty() || max < 4.0 { return; }
    let mut t: String = s.to_string();
    let mut cut = false;
    loop {
        let shown: SharedString = if cut { format!("{t}…").into() } else { t.clone().into() };
        let run = TextRun {
            len: shown.len(),
            font: Font { family: crate::layout::BODY.into(), features: FontFeatures::default(), fallbacks: None, weight, style: FontStyle::Normal },
            color, background_color: None, underline: None, strikethrough: None,
        };
        let line = window.text_system().shape_line(shown, px(size), &[run], None);
        if f32::from(line.width) <= max || t.chars().count() <= 1 {
            let _ = line.paint(at, px(size * 1.4), window, cx);
            return;
        }
        let n = t.chars().count();
        let drop = ((f32::from(line.width) - max) / (f32::from(line.width) / n as f32)).ceil().max(1.0) as usize;
        t = t.chars().take(n.saturating_sub(drop + usize::from(!cut))).collect();
        cut = true;
    }
}

pub fn text_width(window: &mut Window, s: &str, size: f32, weight: FontWeight) -> f32 {
    let shown: SharedString = s.to_string().into();
    let run = TextRun {
        len: shown.len(),
        font: Font { family: crate::layout::BODY.into(), features: FontFeatures::default(), fallbacks: None, weight, style: FontStyle::Normal },
        color: black(), background_color: None, underline: None, strikethrough: None,
    };
    f32::from(window.text_system().shape_line(shown, px(size), &[run], None).width)
}
