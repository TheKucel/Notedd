//! Colours of the app: the same themes as the RichEdit version (paper, night, system, custom).

use crate::settings::{self, Prefs, DEFAULT_CARDS, MAX_CARDS};
use gpui::{rgb, Hsla};

#[derive(Clone, Copy, PartialEq, Hash)]
pub struct AppTheme {
    pub night: bool,
    pub ink: u32,
    pub ink2: u32,
    pub ink3: u32,
    pub line: u32,
    /// the dark tab and the accent buttons
    pub tab_dark: u32,
    pub on_dark: u32,
    pub link: u32,
    pub cards: [u32; MAX_CARDS],
    pub n: usize,
}

pub fn c(hex: u32) -> Hsla { rgb(hex).into() }

fn split(hex: u32) -> [f32; 3] { [((hex >> 16) & 255) as f32, ((hex >> 8) & 255) as f32, (hex & 255) as f32] }
fn join(c: [f32; 3]) -> u32 { ((c[0].round() as u32) << 16) | ((c[1].round() as u32) << 8) | c[2].round() as u32 }
/// relative brightness, 0..1
pub fn lum(c: u32) -> f32 { let [r, g, b] = split(c); (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 }
/// a + (b - a) * k, per channel
pub fn mix(a: u32, b: u32, k: f32) -> u32 {
    let (a, b) = (split(a), split(b));
    join([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k])
}

/// which colour set a theme choice uses: 0 paper, 1 night, 2 custom
pub fn theme_kind(p: &Prefs) -> usize { match p.theme { 0 => 0, 1 => 1, 3 => 2, _ => if settings::system_dark() { 1 } else { 0 } } }

impl AppTheme {
    pub fn new(p: &Prefs) -> AppTheme {
        let k = theme_kind(p);
        let cards = p.cards[k];
        let bg = cards[0];
        // light or dark text follows the paper, so a recoloured paper never hides the text
        let own_bg = bg != DEFAULT_CARDS[k][0];
        let night = if k == 2 || own_bg { lum(bg) < 0.45 } else { k == 1 };
        let mut t = if night {
            AppTheme { night, ink: 0xece8dc, ink2: 0xb0ab9f, ink3: 0x6e6a61, line: 0x5a5952, tab_dark: 0xe6e2d5, on_dark: 0x151513, link: 0x8db4ec, cards, n: p.ncards }
        } else {
            AppTheme { night, ink: 0x141412, ink2: 0x56534b, ink3: 0x98938a, line: 0x4b4a46, tab_dark: 0x2b2b28, on_dark: 0xf3f0e8, link: 0x2f5fa8, cards, n: p.ncards }
        };
        if p.ink[k].is_some() || k == 2 || own_bg {
            if let Some(c) = p.ink[k] { t.ink = c; }
            t.ink2 = mix(t.ink, bg, 0.3);
            t.ink3 = mix(t.ink, bg, 0.6);
            t.line = mix(t.ink, bg, if night { 0.66 } else { 0.25 });
        }
        if let Some(c) = p.accent[k] {
            t.tab_dark = c;
            t.link = c;
            t.on_dark = if lum(c) < 0.5 { 0xf3f0e8 } else { 0x151513 };
        }
        t
    }

    pub fn salt(&self) -> u64 { use std::hash::{Hash, Hasher}; let mut h = std::collections::hash_map::DefaultHasher::new(); self.hash(&mut h); h.finish() }
    pub fn paper(&self) -> u32 { self.cards[0] }
    pub fn note(&self, i: usize) -> u32 { self.cards[i.min(MAX_CARDS - 1)] }
    /// the backdrop behind the cards
    pub fn backdrop(&self) -> u32 { if self.night { mix(self.cards[0], 0x000000, 0.45) } else { mix(self.cards[0], 0x28261e, 0.12) } }
    pub fn highlight(&self) -> u32 { if self.night { 0x57441d } else { 0xffe491 } }
}
