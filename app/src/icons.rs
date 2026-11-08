//! The prototype's SVG icons, drawn by Direct2D from the same path data (16×16 units).
//! Only the commands those icons use: M L H V Z, absolute and relative.

use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;

pub const CLOSE: &str = "M4.5 4.5l7 7M11.5 4.5l-7 7";
pub const PIN: &str = "M9.8 2.5l3.7 3.7-2.3.8-2.4 2.4.3 2.8-1 1-2.1-2.1L3 14l-.1-.1L6 11 3.9 8.9l1-1 2.8.3 2.4-2.4z";
pub const FOLDERS: &str = "M2.5 5.5h4l1.2-1.6h5.8v8.6h-11zM2.5 8h11";
pub const FIND: &str = "M10.5 10.5L14 14"; // plus a circle at (7,7) r 4.5
pub const NEW: &str = "M8 3v10M3 8h10";
pub const PLUS: &str = "M8 2.5v11M2.5 8h11";
pub const UP: &str = "M4 10l4-4 4 4";
pub const DOWN: &str = "M4 6l4 4 4-4";
pub const MORE: &str = "M9.5 4.5L6 8l3.5 3.5"; // points left: more buttons that way
pub const TRASH: &str = "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5L5.2 13h5.6l.7-8.5";
pub const GEAR: &str = "M2.5 5h11M2.5 11h11"; // two sliders; the knobs are drawn on top

enum Tok { C(char), N(f32) }

fn tokens(d: &str) -> Vec<Tok> {
    let b = d.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() { out.push(Tok::C(c)); i += 1; }
        else if c == '-' || c == '.' || c.is_ascii_digit() {
            // "-2.3.8" is two numbers: a second dot starts the next one
            let s = i;
            if c == '-' { i += 1; }
            let mut dot = false;
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() { i += 1; } else if ch == '.' && !dot { dot = true; i += 1; } else { break; }
            }
            out.push(Tok::N(d[s..i].parse().unwrap_or(0.0)));
        } else { i += 1; }
    }
    out
}

pub unsafe fn path(f: &ID2D1Factory, d: &str) -> Result<ID2D1PathGeometry> { path_as(f, d, false) }
/// filled: closed figures can be filled (hollow ones only take a stroke)
pub unsafe fn path_as(f: &ID2D1Factory, d: &str, filled: bool) -> Result<ID2D1PathGeometry> {
    let g = f.CreatePathGeometry()?;
    let s = g.Open()?;
    let toks = tokens(d);
    let (mut x, mut y, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut open = false;
    let mut cmd = 'M';
    let mut j = 0;
    let p = |x: f32, y: f32| D2D_POINT_2F { x, y };
    while j < toks.len() {
        match toks[j] {
            Tok::C(c) => {
                cmd = c;
                j += 1;
                if c == 'z' || c == 'Z' {
                    if open { s.EndFigure(D2D1_FIGURE_END_CLOSED); open = false; }
                    x = sx; y = sy;
                }
            }
            Tok::N(_) => {
                let n = match cmd { 'H' | 'h' | 'V' | 'v' => 1, _ => 2 };
                let mut a = [0.0f32; 2];
                for k in 0..n { if let Some(Tok::N(v)) = toks.get(j + k) { a[k] = *v; } }
                j += n;
                let rel = cmd.is_ascii_lowercase();
                match cmd.to_ascii_uppercase() {
                    'M' => {
                        if open { s.EndFigure(D2D1_FIGURE_END_OPEN); }
                        if rel { x += a[0]; y += a[1]; } else { x = a[0]; y = a[1]; }
                        sx = x; sy = y;
                        s.BeginFigure(p(x, y), if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
                        open = true;
                        cmd = if rel { 'l' } else { 'L' };
                    }
                    'L' => { if rel { x += a[0]; y += a[1]; } else { x = a[0]; y = a[1]; } s.AddLine(p(x, y)); }
                    'H' => { if rel { x += a[0]; } else { x = a[0]; } s.AddLine(p(x, y)); }
                    'V' => { if rel { y += a[0]; } else { y = a[0]; } s.AddLine(p(x, y)); }
                    _ => {}
                }
            }
        }
    }
    if open { s.EndFigure(D2D1_FIGURE_END_OPEN); }
    s.Close()?;
    Ok(g)
}

pub struct Icons {
    pub close: ID2D1PathGeometry, pub pin: ID2D1PathGeometry, pub folders: ID2D1PathGeometry,
    pub find: ID2D1PathGeometry, pub new: ID2D1PathGeometry, pub plus: ID2D1PathGeometry, pub up: ID2D1PathGeometry, pub down: ID2D1PathGeometry, pub gear: ID2D1PathGeometry, pub more: ID2D1PathGeometry, pub trash: ID2D1PathGeometry, pub pin_fill: ID2D1PathGeometry,
}
impl Icons {
    pub unsafe fn new(f: &ID2D1Factory) -> Result<Icons> {
        Ok(Icons {
            close: path(f, CLOSE)?, pin: path(f, PIN)?, folders: path(f, FOLDERS)?, find: path(f, FIND)?,
            new: path(f, NEW)?, plus: path(f, PLUS)?, up: path(f, UP)?, down: path(f, DOWN)?, gear: path(f, GEAR)?, more: path(f, MORE)?, trash: path(f, TRASH)?, pin_fill: path_as(f, PIN, true)?,
        })
    }
}
