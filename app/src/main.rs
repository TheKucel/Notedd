//! Notedd for Windows (it was called Pane).
//! Win32 window + Direct2D (software) for the folders, RichEdit for the text.
//! Editor: the DWM frame is extended over the window edges only, so the area around the tab and the
//! rounded corners is transparent while RichEdit sits on opaque paper. Drawer and transitions:
//! the whole window becomes per-pixel transparent and every folder is drawn by Direct2D.

#![windows_subsystem = "windows"]

mod checkbox;
mod drop;
mod edit_menu;
mod icons;
mod images;
mod layer;
#[path = "../../locale.rs"]
mod locale;
use notedd_core::md;
mod settings;
mod shell;
mod store;
mod strings;

use icons::Icons;
use layer::Layer;
use locale::Language;
use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::time::Instant;
use settings::{Prefs, DEFAULT_CARDS, KEYS, MAX_CARDS};
use store::{Kind, Note};
use strings as ui;
use windows::core::*;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::Win32::System::Time::*;
use windows::Win32::System::Threading::Sleep;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::UI::Controls::RichEdit::{
    tomAutoColor, tomClientCoord, tomConstants, tomListPeriod, tomStart, tomUndefined,
    IRichEditOle, ITextDocument, ITextFont, ITextPara, ITextRange,
};
use md::{Block, Style as Md};
use windows::Win32::UI::Controls::{BeginBufferedPaint, BufferedPaintInit, BufferedPaintUnInit, EndBufferedPaint, BPBF_COMPATIBLEBITMAP, MARGINS};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

// ---- geometry, in DIPs (same numbers as the prototype) ----
// window width and the editor folder's height: the user can resize them, so they live in statics
static mut W_: f32 = 880.0;
static mut EH_: f32 = 560.0;
fn ww() -> f32 { unsafe { W_ } }
fn eh() -> f32 { unsafe { EH_ } }
const MIN_W: f32 = 520.0;
const MIN_H: f32 = 300.0;
// room under the editor folder for its shadow and outline
const PADB: f32 = 26.0;
// room above the drawer stack and below it (shadow, tilt)
const LIST_TOP: f32 = 6.0;
const LIST_BOTTOM: f32 = 40.0;
const LIST_LIMIT: usize = 10;
// resize grip along the folder edges
const GRIP: f32 = 7.0;
const HF: f32 = 290.0; // folder height in the drawer
const T: f32 = 34.0;
const R: f32 = 20.0;
const G: f32 = 6.0;
const F: f32 = 12.0;
const RT: f32 = 14.0;
const PAD: f32 = 40.0;
const DARK: f32 = 34.0;
const DARK_HOVER: f32 = 42.0;
const DARK_FIND: f32 = 136.0;
const DARK_SEARCH: f32 = 104.0;
const PERSP: f32 = 1700.0;
const DEPTH: f32 = 26.0;
// the drawer stack is a touch smaller than the window, so a tilted, lifted folder never meets its edges
const LIST_SC: f32 = 0.955;
const SEARCH: &str = "__search";
const SETTINGS: &str = "__settings";
// settings rows
const SET_TOP: f32 = EDIT_TOP - 8.0;
const SET_ROW: f32 = 50.0;
const SET_BOTTOM: f32 = 18.0;
const CTL_H: f32 = 32.0;
const SET_LABEL: f32 = 170.0;
// editor text box; the preview in the drawer uses the same numbers so switching shows no jump
const EDIT_TOP: f32 = T + 36.0;
const RE_DY: f32 = -1.0; // RichEdit's own offset of the first line, measured
const RE_DX: f32 = -1.0;
// palette popup
fn pal_w() -> f32 { let n = ncards() as f32; n * 32.0 + (n - 1.0) * 4.0 + 12.0 }
const PAL_H: f32 = 44.0;
const PAL_DELETE_H: f32 = 38.0;
const PAL_ML: f32 = 24.0; // room for the shadow around the box
const PAL_MT: f32 = 16.0;
const PAL_MB: f32 = 40.0;
// taskbar magnet, DIPs: inside ZONE the window follows the cursor ever more slowly toward the taskbar
// (a soft pull, no jump); released closer than GLIDE it slides down onto it
const ZONE: f32 = 64.0;
const GLIDE: f32 = 16.0;
// line boxes of the note text, DIPs (a little tighter than the prototype's 28/16 and 26/12)

// ---- colours: paper by day, night by night (the prototype's two themes), or the user's own ----
#[derive(Clone, Copy)]
struct Theme { night: bool, ink: u32, ink2: u32, ink3: u32, line: u32, tab_dark: u32, on_dark: u32, link: u32, cards: [u32; MAX_CARDS], n: usize }
static mut TH_: Theme = Theme { night: false, ink: 0x141412, ink2: 0x56534b, ink3: 0x98938a, line: 0x4b4a46, tab_dark: 0x2b2b28, on_dark: 0xf3f0e8, link: 0x2f5fa8, cards: DEFAULT_CARDS[0], n: 6 };
fn th() -> &'static Theme { unsafe { &*std::ptr::addr_of!(TH_) } }
fn night() -> bool { th().night }
fn ink() -> u32 { th().ink }
fn ink2() -> u32 { th().ink2 }
fn ink3() -> u32 { th().ink3 }
fn line_c() -> u32 { th().line }
fn tab_dark() -> u32 { th().tab_dark }
fn on_dark() -> u32 { th().on_dark }
fn link_c() -> u32 { th().link }
fn highlight_c() -> u32 { if night() { 0x57441d } else { 0xffe491 } }
fn shadow() -> u32 { if night() { 0x000000 } else { 0x28261e } }
fn shadow_k() -> f32 { if night() { 2.4 } else { 1.0 } }
fn note_rgb(i: usize) -> u32 { th().cards[i.min(MAX_CARDS - 1)] }
fn paper() -> u32 { note_rgb(0) }
fn ncards() -> usize { th().n }
/// relative brightness, 0..1
fn lum(c: u32) -> f32 { let [r, g, b] = split(c); (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 }
/// a + (b - a) * k, per channel
fn mix(a: u32, b: u32, k: f32) -> u32 { let (a, b) = (split(a), split(b)); join([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]) }
/// which colour set a theme choice uses: 0 paper, 1 night, 2 custom
fn theme_kind(p: &Prefs) -> usize { match p.theme { 0 => 0, 1 => 1, 3 => 2, _ => if settings::system_dark() { 1 } else { 0 } } }
fn make_theme(p: &Prefs) -> Theme {
    let k = theme_kind(p);
    let cards = p.cards[k];
    let bg = cards[0];
    // light or dark text follows the paper, so a recoloured paper never hides the text
    let own_bg = bg != DEFAULT_CARDS[k][0];
    let night = if k == 2 || own_bg { lum(bg) < 0.45 } else { k == 1 };
    let mut t = if night {
        Theme { night, ink: 0xece8dc, ink2: 0xb0ab9f, ink3: 0x6e6a61, line: 0x5a5952, tab_dark: 0xe6e2d5, on_dark: 0x151513, link: 0x8db4ec, cards, n: p.ncards }
    } else {
        Theme { night, ink: 0x141412, ink2: 0x56534b, ink3: 0x98938a, line: 0x4b4a46, tab_dark: 0x2b2b28, on_dark: 0xf3f0e8, link: 0x2f5fa8, cards, n: p.ncards }
    };
    // the text greys and outlines are the ink mixed toward the paper, the same steps the built-in themes use
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

// ---- text size: line boxes of the note text scale with it ----
static mut TS_: f32 = 1.0;
fn ts() -> f32 { unsafe { TS_ } }
fn lh_title() -> f32 { (27.0 * ts()).round() }
fn gap_title() -> f32 { (12.0 * ts()).round() }
fn lh_body() -> f32 { (23.0 * ts()).round() }
fn gap_body() -> f32 { (9.0 * ts()).round() }
fn gap_list() -> f32 { (3.0 * ts()).round() }
/// glyph box inside a line box: RichEdit centres the glyphs (box top, box bottom, text top)
fn hl_body() -> (f32, f32, f32) { let g = 20.0 * ts(); ((lh_body() - g) / 2.0, (lh_body() + g) / 2.0, 0.0) }
fn hl_title() -> (f32, f32, f32) { let g = 26.0 * ts(); ((lh_title() - g) / 2.0, (lh_title() + g) / 2.0, 0.0) }

static FONT: &[u8] = include_bytes!("../../assets/fonts/Geologica.ttf");
static ICON: &[u8] = include_bytes!("../../assets/notedd.ico");

// ---- RichEdit messages and structs ----
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SCROLLCARET: u32 = 0x00B7;
const EM_EMPTYUNDOBUFFER: u32 = 0x00CD;
const EM_POSFROMCHAR: u32 = 0x00D6;
const EM_EXGETSEL: u32 = WM_USER + 52;
const EM_EXLIMITTEXT: u32 = WM_USER + 53;
const EM_EXSETSEL: u32 = WM_USER + 55;
const EM_GETOLEINTERFACE: u32 = WM_USER + 60;
const EM_GETPARAFORMAT: u32 = WM_USER + 61;
const EM_SETBKGNDCOLOR: u32 = WM_USER + 67;
const EM_SETCHARFORMAT: u32 = WM_USER + 68;
const EM_SETEVENTMASK: u32 = WM_USER + 69;
const EM_SETOLECALLBACK: u32 = WM_USER + 70;
const EM_SETPARAFORMAT: u32 = WM_USER + 71;
const EM_GETTEXTEX: u32 = WM_USER + 94;
const EM_SETTYPOGRAPHYOPTIONS: u32 = WM_USER + 202;
const SCF_SELECTION: usize = 1;
const ENM_CHANGE: isize = 1;
const ENM_SCROLL: isize = 4;
const ENM_SCROLLEVENTS: isize = 8;
const EN_CHANGE: u32 = 0x0300;
const EN_VSCROLL: u32 = 0x0602;
const CFM_BOLD: u32 = 0x1;
const CFM_BACKCOLOR: u32 = 0x0400_0000;
const CFM_COLOR: u32 = 0x4000_0000;
const CFM_SIZE: u32 = 0x8000_0000;
const CFM_FACE: u32 = 0x2000_0000;
const CFE_BOLD: u32 = 0x1;
const CFM_ITALIC: u32 = 0x2;
const CFM_UNDERLINE: u32 = 0x4;
const CFM_STRIKEOUT: u32 = 0x8;
const CFE_ITALIC: u32 = 0x2;
const CFE_UNDERLINE: u32 = 0x4;
const CFE_STRIKEOUT: u32 = 0x8;
const CFM_HIDDEN: u32 = 0x100;
const CFE_HIDDEN: u32 = 0x100;
const EM_CHARFROMPOS: u32 = 0x00D7;
const EM_CANUNDO: u32 = 0x00C6;
/// task box size and the gap after it, DIPs at text size 100%
const TASK_BOX: f32 = 15.0;
const TASK_GAP: f32 = 8.0;
/// task boxes at the start of a line: "- [ ] " and "- [x] " in the file
const TASK: u16 = 0x2610;
const DONE: u16 = 0x2611;
/// "no colour of its own" in the remembered character colours
const AUTO: u32 = u32::MAX;
const CFE_AUTOBACKCOLOR: u32 = 0x0400_0000;
const PFM_STARTINDENT: u32 = 0x1;
const PFM_OFFSET: u32 = 0x4;
const PFM_NUMBERING: u32 = 0x20;
const PFM_SPACEBEFORE: u32 = 0x40;
const PFM_SPACEAFTER: u32 = 0x80;
const PFM_LINESPACING: u32 = 0x100;
const PFM_NUMBERINGSTYLE: u32 = 0x2000;
const PFM_NUMBERINGTAB: u32 = 0x4000;
const PFM_NUMBERINGSTART: u32 = 0x8000;
const TOM_SUSPEND: i32 = -9999995;
const TOM_RESUME: i32 = -9999994;
const WM_TRAY: u32 = WM_APP + 1;
const WM_TEST: u32 = WM_APP + 2;
const WM_HITS: u32 = WM_APP + 3;
const WM_APPEAR: u32 = WM_APP + 4;
/// files or pictures were dragged onto the note (see drop.rs)
const WM_DROPPED: u32 = WM_APP + 5;
/// corner radius of pictures in a note (DIP)
const PIC_RADIUS: f32 = 8.0;
const TIMER_ANIM: usize = 1;
const TIMER_SAVE: usize = 2;
const TIMER_CARET: usize = 3;
const TIMER_ECARET: usize = 4;
const TIMER_EDIT_MENU: usize = 5;
const EM_GETSCROLLPOS: u32 = WM_USER + 221;
const EM_SETSCROLLPOS: u32 = WM_USER + 222;
// glyph box relative to the line top RichEdit reports for a character: (box top, box bottom, text top).
// RichEdit centres the glyphs in an exact line box, so the box sits in the middle of it.

#[repr(C)]
#[derive(Default)]
struct CharFormat2 {
    cb_size: u32, mask: u32, effects: u32, height: i32, offset: i32, color: u32,
    charset: u8, pitch: u8, face: [u16; 32], weight: u16, spacing: i16, back: u32, lcid: u32,
    reserved: u32, style: i16, kerning: u16, underline: u8, animation: u8, rev_author: u8, underline_color: u8,
}

impl CharFormat2 {
    fn new(mask: u32, effects: u32, height: i32, color: u32, back: u32, face: &str) -> Self {
        let mut format = Self {
            cb_size: std::mem::size_of::<Self>() as u32,
            mask, effects, height,
            color: bgr(color),
            back: if back == AUTO { 0 } else { bgr(back) },
            ..Self::default()
        };
        for (slot, value) in format.face[..31].iter_mut().zip(face.encode_utf16()) { *slot = value; }
        format
    }
}

#[repr(C)]
struct ParaFormat2 {
    cb_size: u32, mask: u32, numbering: u16, effects: u16, start_indent: i32, right_indent: i32, offset: i32,
    alignment: u16, tab_count: i16, tabs: [i32; 32], space_before: i32, space_after: i32, line_spacing: i32,
    style: i16, line_spacing_rule: u8, outline_level: u8, shading_weight: u16, shading_style: u16,
    numbering_start: u16, numbering_style: u16, numbering_tab: u16, border_space: u16, border_width: u16, borders: u16,
}
/// what a paragraph shows besides its text
#[derive(Clone)]
enum PK { Text, Quote, Code, Image(PathBuf, f32, f32) }
/// a paragraph as styled: range, line box and glyph height (DIPs), kind
#[derive(Clone)]
struct PStyle { a: i32, b: i32, lh: f32, g: f32, kind: PK, block: Block, task: Option<bool> }

/// A detached TOM font changes only the attributes in this format's mask.
unsafe fn editor_font(range: &ITextRange, mask: u32, effects: u32, height: i32, color: u32, back: u32, face: &str) -> Result<ITextFont> {
    let font = range.GetFont()?.GetDuplicate()?;
    font.Reset(tomUndefined)?;
    let enabled = |flag| if effects & flag != 0 { -1 } else { 0 };
    if mask & CFM_FACE != 0 { font.SetName(&BSTR::from(face))?; }
    if mask & CFM_SIZE != 0 { font.SetSize(height as f32 / 20.0)?; }
    if mask & CFM_COLOR != 0 { font.SetForeColor(bgr(color) as i32)?; }
    if mask & CFM_BACKCOLOR != 0 { font.SetBackColor(if back == AUTO { tomAutoColor.0 } else { bgr(back) as i32 })?; }
    if mask & CFM_BOLD != 0 { font.SetBold(enabled(CFE_BOLD))?; }
    if mask & CFM_ITALIC != 0 { font.SetItalic(enabled(CFE_ITALIC))?; }
    if mask & CFM_STRIKEOUT != 0 { font.SetStrikeThrough(enabled(CFE_STRIKEOUT))?; }
    if mask & CFM_UNDERLINE != 0 { font.SetUnderline((effects & CFE_UNDERLINE != 0) as i32)?; }
    if mask & CFM_HIDDEN != 0 { font.SetHidden(enabled(CFE_HIDDEN))?; }
    Ok(font)
}

unsafe fn editor_paragraph(range: &ITextRange, pf: &ParaFormat2) -> Result<ITextPara> {
    let para = range.GetPara()?.GetDuplicate()?;
    para.Reset(tomUndefined.0)?;
    let points = |twips: i32| twips as f32 / 20.0;
    para.SetLineSpacing(pf.line_spacing_rule as i32, points(pf.line_spacing))?;
    para.SetSpaceBefore(points(pf.space_before))?;
    para.SetSpaceAfter(points(pf.space_after))?;
    para.SetIndents(points(-pf.offset), points(pf.start_indent + pf.offset), 0.0)?;
    let numbering = pf.numbering as i32;
    para.SetListType(if numbering == 0 { 0 } else { numbering | tomListPeriod.0 })?;
    if numbering != 0 {
        para.SetListStart(pf.numbering_start as i32)?;
        para.SetListTab(points(pf.numbering_tab as i32))?;
        // RichEdit right-aligns numbers by default, and wide ones ("2.", "10.") then stick out past the edge
        para.SetListAlignment(windows::Win32::UI::Controls::RichEdit::tomAlignLeft.0)?;
    }
    Ok(para)
}

#[repr(C)]
struct CharRange { min: i32, max: i32 }
#[repr(C)]
struct GetTextEx { cb: u32, flags: u32, codepage: u32, default_char: *const u8, used_default: *mut i32 }

// ---- animation ----
#[derive(Clone, Copy, Default)]
struct V { y: f32, sc: f32, h: f32, o: f32, lift: f32, rot: f32, sx: f32 }
impl V {
    fn lerp(&mut self, to: &V, k: f32, ko: f32) {
        self.y += (to.y - self.y) * k; self.sc += (to.sc - self.sc) * k; self.h += (to.h - self.h) * k;
        self.o += (to.o - self.o) * ko; self.lift += (to.lift - self.lift) * k; self.rot += (to.rot - self.rot) * k;
        self.sx += (to.sx - self.sx) * k;
    }
    fn near(&self, to: &V) -> bool {
        (self.y - to.y).abs() < 0.3 && (self.sc - to.sc).abs() < 0.001 && (self.h - to.h).abs() < 0.3
            && (self.o - to.o).abs() < 0.01 && (self.lift - to.lift).abs() < 0.2 && (self.rot - to.rot).abs() < 0.02 && (self.sx - to.sx).abs() < 0.2
    }
}
#[derive(Clone, Copy, Default)]
struct Anim { cur: V, to: V, z: i32, delay: f32 }

/// one eased number
#[derive(Clone, Copy)]
struct Ease { v: f32, to: f32 }
impl Ease {
    const fn at(v: f32) -> Ease { Ease { v, to: v } }
    fn step(&mut self, k: f32, eps: f32) -> bool {
        self.v += (self.to - self.v) * k;
        if (self.v - self.to).abs() < eps { self.v = self.to; true } else { false }
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Mode { Edit, List }
#[derive(PartialEq, Clone, Copy, Debug)]
enum Ctl { Close, Pin, Gear, List, Find, Dot, New, More, Trash }
#[derive(PartialEq, Clone, Copy, Debug)]
enum PalAction { Color(usize), Delete }
/// a colour being edited on the colours page
#[derive(PartialEq, Clone, Copy, Debug)]
enum Pick { Ink, Accent, Card(usize) }
/// settings rows
#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
enum Row { Page, Language, Editor, Theme, Scale, Dir, Far, Speed, Auto, Notes, Key(usize), Ink, Accent, Cards, Reset }
/// one button of a settings row: label, chosen, a colour dot, shown value only (not clickable)
struct Seg { text: String, on: bool, swatch: Option<u32>, value: bool, glyph: u8, keys: Option<(u32, u32)> }
fn seg(text: impl Into<String>, on: bool) -> Seg { Seg { text: text.into(), on, swatch: None, value: false, glyph: 0, keys: None } }
/// how a settings row is drawn: one block with a sliding pill, a stepper, key caps, colour dots, a button
#[derive(PartialEq, Clone, Copy)]
enum Style { Choice, Step, Keys, Swatches, Button }
fn style(r: Row) -> Style {
    match r { Row::Scale => Style::Step, Row::Key(_) => Style::Keys, Row::Cards => Style::Swatches, Row::Reset | Row::Notes => Style::Button, _ => Style::Choice }
}
struct RowG { row: Row, label: String, top: f32, h: f32, block: D2D_RECT_F, segs: Vec<(D2D_RECT_F, Seg)> }
/// the keys of a binding, one per cap
fn key_parts(m: u32, vk: u32) -> Vec<String> {
    let mut v = vec![];
    if m & 2 != 0 { v.push("Ctrl".to_string()); }
    if m & 1 != 0 { v.push("Alt".to_string()); }
    if m & 4 != 0 { v.push("Shift".to_string()); }
    if m & 8 != 0 { v.push("Win".to_string()); }
    if vk != 0 { v.push(key_name(vk)); }
    v
}

const SIMPLE_FORMATS: [(md::Format, locale::Message, (u32, u32)); 4] = [
    (md::Format::Bold, ui::BOLD, (2, b'B' as u32)),
    (md::Format::Italic, ui::ITALIC, (2, b'I' as u32)),
    (md::Format::Strike, ui::STRIKE, (6, b'S' as u32)),
    (md::Format::Code, ui::CODE, (2, b'E' as u32)),
];
unsafe fn keys_width(a: &App, s: &Seg) -> f32 {
    if !s.text.is_empty() { return a.text_w(&u(&s.text), &a.fonts.small) + 28.0; }
    let (m, vk) = s.keys.unwrap_or((0, 0));
    key_parts(m, vk).iter().map(|k| a.text_w(&u(k), &a.fonts.small).max(10.0) + 18.0 + 5.0).sum::<f32>() - 5.0
}
const LANGUAGES: [Option<Language>; 4] = [None, Some(Language::English), Some(Language::Russian), Some(Language::Chinese)];

struct Fonts {
    tab: IDWriteTextFormat, small: IDWriteTextFormat, small_r: IDWriteTextFormat, count: IDWriteTextFormat, count_l: IDWriteTextFormat,
    head: IDWriteTextFormat, body: IDWriteTextFormat, input: IDWriteTextFormat,
}

enum PreviewMarker {
    List(IDWriteTextLayout),
    Task(bool),
}

struct PreviewLine {
    x: f32,
    y: f32,
    layout: IDWriteTextLayout,
    highlights: Vec<D2D_RECT_F>,
    title: bool,
    marker: Option<PreviewMarker>,
}

type PreviewLines = std::rc::Rc<[PreviewLine]>;

unsafe fn preview_highlights(layout: &IDWriteTextLayout, marked: Vec<std::ops::Range<u32>>) -> Vec<D2D_RECT_F> {
    let mut out = Vec::new();
    for range in marked.into_iter().filter(|r| !r.is_empty()) {
        let mut count = 0;
        let len = range.end - range.start;
        let _ = layout.HitTestTextRange(range.start, len, 0.0, 0.0, None, &mut count);
        if count == 0 { continue; }
        let mut metrics = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
        if layout.HitTestTextRange(range.start, len, 0.0, 0.0, Some(&mut metrics), &mut count).is_ok() {
            out.extend(metrics.into_iter().take(count as usize).map(|m| rect(m.left, m.top, m.left + m.width, m.top + m.height)));
        }
    }
    out
}

struct EditSnapshot {
    bitmap: ID2D1Bitmap,
    bounds: D2D_RECT_F,
    scroll: POINT,
    start: Instant,
}

impl EditSnapshot {
    const FADE: std::time::Duration = std::time::Duration::from_millis(120);

    fn opacity(&self, mode: Mode) -> f32 {
        let t = (self.start.elapsed().as_secs_f32() / Self::FADE.as_secs_f32()).min(1.0);
        let progress = t * t * (3.0 - 2.0 * t);
        if mode == Mode::List { 1.0 - progress } else { progress }
    }

    fn reverse(&mut self) {
        self.start = Instant::now() - Self::FADE.saturating_sub(self.start.elapsed());
    }
}

struct App {
    hwnd: HWND,
    edit: HWND,
    doc: Option<ITextDocument>,
    factory: ID2D1Factory,
    dw: IDWriteFactory5,
    fonts: Fonts,
    icons: Icons,
    round: ID2D1StrokeStyle,
    text_rendering: IDWriteRenderingParams,
    target: Option<ID2D1HwndRenderTarget>,
    scale: f32,
    dir: PathBuf,
    notes: Vec<Note>,
    cur: String,
    mode: Mode,
    finding: bool,
    find_q: Vec<u16>,
    hits: Vec<(i32, i32)>,
    hit: usize,
    list_q: Vec<u16>,
    shown: Vec<String>,
    sel: usize,
    list_start: usize,
    list_len: usize,
    list_wheel: i32,
    anims: HashMap<String, Anim>,
    colors: HashMap<String, [f32; 3]>,
    kinds: Vec<Kind>,
    load_kinds: Option<Vec<Kind>>,
    force: Option<(usize, Kind)>,
    win_h: f32,
    win_h_to: f32,
    dark: Ease,
    rb: Ease,
    tab_hl: Ease,
    dot_sc: Ease,
    plus_sc: Ease,
    pal_t: Ease,
    sw_sc: [Ease; MAX_CARDS],
    hover_ctl: Option<usize>,
    hover_plus: bool,
    hover_tab: bool,
    hover_fbtn: Option<usize>,
    press_tab: Option<POINT>,
    /// a press on a drawer folder: a click opens it, a drag moves the window
    press_folder: Option<(POINT, String)>,
    /// resizing from a folder edge: cursor and window rect at the start, edges (1 left, 2 right, 4 bottom)
    press_size: Option<(POINT, RECT, u8)>,
    drag0: Option<(POINT, RECT)>,
    press_plus: bool,
    tracking: bool,
    palette: Option<String>,
    pal_anchor: (f32, f32),
    pal_hover: Option<PalAction>,
    pal_delete_pressed: bool,
    pal_tracking: bool,
    pinned: bool,
    /// screen edges the editor is stuck to: 1 left, 2 right, 4 bottom, 8 top
    docks: u8,
    /// left/right corner radii, eased (rb is the bottom one)
    rl: Ease,
    rr: Ease,
    win_x: f32,
    win_x_to: f32,
    /// a release near the left or right edge glides onto it
    x_glide: bool,
    prefs: Prefs,
    coll: IDWriteFontCollection1,
    /// the settings folder is up (mode stays Edit underneath)
    settings: bool,
    /// settings page: 0 general, 1 keys, 2 colours
    set_page: usize,
    set_scroll: f32,
    set_scroll_drag: Option<f32>,
    /// the key binding listening for a new combination
    capturing: Option<usize>,
    /// a global binding that could not be registered
    hk_err: Option<usize>,
    pick: Option<Pick>,
    pick_hsv: [f32; 3],
    pick_hex: Vec<u16>,
    /// dragging in the colour picker: 0 the square, 1 the hue strip
    press_pick: Option<u8>,
    /// the button capsule: expanded behind the arrow, opacity, width in buttons, arrow turn
    ctl_open: bool,
    ctl_a: Ease,
    cap_n: Ease,
    more_t: Ease,
    ctl_last: Vec<Ctl>,
    ctl_prev: Vec<Ctl>,
    ctl_fade: Ease,
    /// frames run from the message loop, paced by DWM
    animating: bool,
    last_frame: Instant,
    fps: Option<(u32, f32, f32, f32)>,
    /// sliding pills of the settings blocks: (page, row) -> left, right
    seg_anim: HashMap<(usize, Row), [Ease; 2]>,
    /// keyboard focus in the settings (row, part); drawn only once the keyboard is used
    focus: (usize, usize),
    kb_focus: bool,
    /// last cursor position seen, screen px, and whether the latest mouse move really moved it
    last_cursor: (i32, i32),
    mouse_moved: bool,
    previews: std::cell::RefCell<HashMap<String, (u64, PreviewLines)>>,
    autostart: bool,
    hover_set: Option<(usize, usize)>,
    /// hiding to show a new appearance setting: comes straight back
    demo: bool,
    /// offset from the resting place while sliding in or out, px
    slide: (Ease, Ease),
    slide_base: POINT,
    edit_visible: bool,
    edit_snapshot: Option<EditSnapshot>,
    suspended_scroll: Option<POINT>,
    hiding: bool,
    guard: bool,
    caret_on: bool,
    tick: Instant,
    tray: NOTIFYICONDATAW,
    hits_layer: Layer,
    caret_layer: Layer,
    pop: Layer,
    edit_menu: Option<edit_menu::EditMenu>,
    /// window top in screen px, eased: the drawer keeps the window on screen, a release near the taskbar glides onto it
    win_y: f32,
    win_y_to: f32,
    /// Physical top of a fixed canvas covering both the drawer and the unfolding editor.
    unfold_top: Option<i32>,
    /// where the editor window stood before the drawer opened
    edit_top: i32,
    /// the note deleted last, until another note is opened: Ctrl+Z in the drawer brings it back
    undo: Option<Note>,
    /// end of the first paragraph (the heading)
    title_end: i32,
    car: Caret,
    /// 1x1 ink pixel for the editor marks, and the caret layer's last bounds and opacity
    car_dc: HDC,
    car_rect: RECT,
    car_a: u8,
    /// last seen RichEdit state: selection, caret position, scroll, focus
    seen: (i32, i32, i32, i32, i32, bool),
    wheel_delta: i32,
    /// The open note as styled: paragraphs, links (from, to, target) and inline code.
    pstyle: Vec<PStyle>,
    styled_text: Vec<u16>,
    links: Vec<(i32, i32, String)>,
    codes: Vec<(i32, i32)>,
    imgs: images::Images,
    checkboxes: Option<checkbox::Checkboxes>,
}

/// our own caret: a thin rounded bar that glides to the new place and blinks by fading
#[derive(Default)]
struct Caret { last: Option<Instant>, x: f32, y: f32, h: f32, tx: f32, ty: f32, th: f32, t: f32, alpha: f32, on: bool }

static mut APP: *mut App = null_mut();
fn app() -> Option<&'static mut App> { unsafe { APP.as_mut() } }

// ---- small helpers ----
fn rgb(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((hex >> 16) & 255) as f32 / 255.0, g: ((hex >> 8) & 255) as f32 / 255.0, b: (hex & 255) as f32 / 255.0, a }
}
fn bgr(hex: u32) -> u32 { ((hex & 0xff) << 16) | (hex & 0xff00) | ((hex >> 16) & 0xff) }
fn split(hex: u32) -> [f32; 3] { [((hex >> 16) & 255) as f32, ((hex >> 8) & 255) as f32, (hex & 255) as f32] }
fn join(c: [f32; 3]) -> u32 { ((c[0].round() as u32) << 16) | ((c[1].round() as u32) << 8) | c[2].round() as u32 }
fn pt(x: f32, y: f32) -> D2D_POINT_2F { D2D_POINT_2F { x, y } }
fn rect(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F { D2D_RECT_F { left: l, top: t, right: r, bottom: b } }
fn rrect(l: f32, t: f32, r: f32, b: f32, rad: f32) -> D2D1_ROUNDED_RECT { D2D1_ROUNDED_RECT { rect: rect(l, t, r, b), radiusX: rad, radiusY: rad } }
fn circle(x: f32, y: f32, r: f32) -> D2D1_ELLIPSE { D2D1_ELLIPSE { point: pt(x, y), radiusX: r, radiusY: r } }
fn u(s: &str) -> Vec<u16> { s.encode_utf16().collect() }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn mat(m11: f32, m12: f32, m21: f32, m22: f32, dx: f32, dy: f32) -> Matrix3x2 { Matrix3x2 { M11: m11, M12: m12, M21: m21, M22: m22, M31: dx, M32: dy } }
fn shift(dx: f32, dy: f32) -> Matrix3x2 { mat(1.0, 0.0, 0.0, 1.0, dx, dy) }
fn mul(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    mat(a.M11 * b.M11 + a.M12 * b.M21, a.M11 * b.M12 + a.M12 * b.M22,
        a.M21 * b.M11 + a.M22 * b.M21, a.M21 * b.M12 + a.M22 * b.M22,
        a.M31 * b.M11 + a.M32 * b.M21 + b.M31, a.M31 * b.M12 + a.M32 * b.M22 + b.M32)
}
fn hash(s: &str) -> u32 { s.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619)) }
/// each folder leans its own way when pulled up: (degrees, px)
fn tilt(id: &str) -> (f32, f32) {
    let h = hash(id);
    let sgn = if h & 1 == 0 { -1.0 } else { 1.0 };
    (sgn * (0.7 + ((h >> 1) % 100) as f32 / 100.0 * 1.1), sgn * (3.0 + ((h >> 8) % 100) as f32 / 100.0 * 7.0))
}
fn width() -> f32 { ww() - 2.0 }
fn tab_x() -> (f32, f32) { let x2 = R + G + F; (x2, x2 + (width() * 0.34).round().clamp(180.0, 300.0)) }
fn tab_text_x() -> f32 { tab_x().0 + RT - 2.0 }
fn ctl_x(k: usize) -> f32 { width() - 16.0 - 34.0 * k as f32 }
fn plural(n: usize, f: [&str; 3]) -> String {
    let (m10, m100) = (n % 10, n % 100);
    let w = if m10 == 1 && m100 != 11 { f[0] } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) { f[1] } else { f[2] };
    format!("{n} {w}")
}
fn local(ft: u64) -> SYSTEMTIME {
    unsafe {
        let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
        let mut utc = SYSTEMTIME::default();
        let mut loc = SYSTEMTIME::default();
        let _ = FileTimeToSystemTime(&f, &mut utc);
        let _ = SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc);
        loc
    }
}
fn when(ft: u64) -> String {
    const MONTHS_RU: [&str; 12] = ["января", "февраля", "марта", "апреля", "мая", "июня", "июля", "августа", "сентября", "октября", "ноября", "декабря"];
    const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let now = unsafe { GetSystemTimeAsFileTime() };
    let now = ((now.dwHighDateTime as u64) << 32) | now.dwLowDateTime as u64;
    let (d, n, y) = (local(ft), local(now), local(now.saturating_sub(864_000_000_000)));
    let same = |a: &SYSTEMTIME, b: &SYSTEMTIME| a.wYear == b.wYear && a.wMonth == b.wMonth && a.wDay == b.wDay;
    let hm = format!("{:02}:{:02}", d.wHour, d.wMinute);
    let relative = if same(&d, &n) { Some(ui::TODAY.get()) } else if same(&d, &y) { Some(ui::YESTERDAY.get()) } else { None };
    if let Some(label) = relative { return format!("{label}, {hm}"); }
    let month = (d.wMonth.clamp(1, 12) - 1) as usize;
    match locale::current() {
        Language::English => format!("{} {}", MONTHS_EN[month], d.wDay),
        Language::Russian => format!("{} {}", d.wDay, MONTHS_RU[month]),
        Language::Chinese => format!("{}月{}日", month + 1, d.wDay),
    }
}

fn word_count(n: usize) -> String {
    match locale::current() {
        Language::English => if n == 1 { "1 word".into() } else { format!("{n} words") },
        Language::Russian => plural(n, ["слово", "слова", "слов"]),
        Language::Chinese => format!("{n} 个词"),
    }
}
fn para_ranges(text: &[u16]) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    let mut a = 0;
    for (i, &c) in text.iter().enumerate() { if c == 13 { out.push((a as i32, i as i32)); a = i + 1; } }
    out.push((a as i32, text.len() as i32));
    out
}

fn editor_markdown(text: &str) -> (Vec<u16>, Vec<Kind>) {
    let mut kinds = Vec::new();
    let mut contents = Vec::with_capacity(text.len() + 1);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 { contents.push(13); }
        let (kind, body) = if i == 0 { (Kind::Plain, line) } else { store::marker(line) };
        if let Some((done, rest)) = store::task(kind, body) {
            kinds.push(Kind::Plain);
            contents.extend([if done { DONE } else { TASK }, 32]);
            contents.extend(rest.encode_utf16());
        } else {
            kinds.push(kind);
            contents.extend(body.encode_utf16());
        }
    }
    contents.push(0);
    (contents, kinds)
}

// ---- geometry ----
unsafe fn arc(s: &ID2D1GeometrySink, x: f32, y: f32, r: f32, clockwise: bool) {
    s.AddArc(&D2D1_ARC_SEGMENT {
        point: pt(x, y), size: D2D_SIZE_F { width: r, height: r }, rotationAngle: 0.0,
        sweepDirection: if clockwise { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
        arcSize: D2D1_ARC_SIZE_SMALL,
    });
}
unsafe fn folder_geometry(f: &ID2D1Factory, w: f32, h: f32, tl: f32, tr: f32, bl: f32, br: f32) -> Result<ID2D1PathGeometry> {
    let (x2, bx) = tab_x();
    let g = f.CreatePathGeometry()?;
    let s = g.Open()?;
    s.BeginFigure(pt(0.0, h - bl), D2D1_FIGURE_BEGIN_FILLED);
    s.AddLine(pt(0.0, T + tl)); if tl > 0.05 { arc(&s, tl, T, tl, true); }
    s.AddLine(pt(R + G, T)); arc(&s, x2, T - F, F, false);
    s.AddLine(pt(x2, RT)); arc(&s, x2 + RT, 0.0, RT, true);
    s.AddLine(pt(bx - RT, 0.0)); arc(&s, bx, RT, RT, true);
    s.AddLine(pt(bx, T - F)); arc(&s, bx + F, T, F, false);
    s.AddLine(pt(w - tr, T)); if tr > 0.05 { arc(&s, w, T + tr, tr, true); }
    s.AddLine(pt(w, h - br)); if br > 0.05 { arc(&s, w - br, h, br, true); }
    s.AddLine(pt(bl, h)); if bl > 0.05 { arc(&s, 0.0, h - bl, bl, true); }
    s.EndFigure(D2D1_FIGURE_END_CLOSED);
    s.Close()?;
    Ok(g)
}
unsafe fn dark_geometry(f: &ID2D1Factory, bx: f32, w: f32) -> Result<ID2D1PathGeometry> {
    let c = bx + w;
    let g = f.CreatePathGeometry()?;
    let s = g.Open()?;
    s.BeginFigure(pt(bx - RT, T + 2.0), D2D1_FIGURE_BEGIN_FILLED);
    s.AddLine(pt(bx - RT, 0.0));
    s.AddLine(pt(c - RT, 0.0)); arc(&s, c, RT, RT, true);
    s.AddLine(pt(c, T - F)); arc(&s, c + F, T, F, false);
    s.AddLine(pt(c + F, T + 2.0));
    s.EndFigure(D2D1_FIGURE_END_CLOSED);
    s.Close()?;
    Ok(g)
}
/// local folder space -> window space: lean around the top centre, move, then perspective toward the top
fn fold_matrix(v: &V) -> Matrix3x2 {
    let w = width();
    let (cx, oy) = (w / 2.0, 0.0);
    let (s, c) = (v.rot.to_radians().sin(), v.rot.to_radians().cos());
    let rot = mul(&mul(&shift(-cx, 0.0), &mat(c, s, -s, c, 0.0, 0.0)), &shift(cx, 0.0));
    let mv = shift(v.sx, v.y - v.lift);
    let per = mul(&mul(&shift(-cx, -oy), &mat(v.sc, 0.0, 0.0, v.sc, 0.0, 0.0)), &shift(cx, oy));
    mul(&mul(&rot, &mv), &per)
}
fn fold_rect(v: &V) -> (f32, f32, f32, f32) {
    let w = width();
    let (cx, oy) = (w / 2.0, 0.0);
    let top = oy + (v.y - v.lift - oy) * v.sc;
    let left = cx + (v.sx - cx) * v.sc;
    (left, top, left + w * v.sc, top + v.h * v.sc)
}
/// a soft drop shadow made of a few stacked translucent rectangles (a real blur is too slow in software)
unsafe fn soft_shadow(rt: &ID2D1RenderTarget, r: D2D_RECT_F, rad: f32, dy: f32, blur: f32, alpha: f32) -> Result<()> {
    let n = 12;
    let b = rt.CreateSolidColorBrush(&rgb(shadow(), (alpha * shadow_k()).min(0.9) / n as f32 * 1.7), None)?;
    for i in 0..n {
        let e = -blur * 0.35 + blur * 0.9 * i as f32 / (n - 1) as f32;
        rt.FillRoundedRectangle(&rrect(r.left - e, r.top + dy - e, r.right + e, r.bottom + dy + e, (rad + e).max(0.0)), &b);
    }
    Ok(())
}

impl App {
    fn note(&self, id: &str) -> Option<&Note> { self.notes.iter().find(|n| n.id == id) }
    fn note_mut(&mut self, id: &str) -> Option<&mut Note> { self.notes.iter_mut().find(|n| n.id == id) }
    fn cur_note(&self) -> &Note { self.note(&self.cur).unwrap_or(&self.notes[0]) }
    fn target_color(&self, id: &str) -> u32 { if id == SEARCH || id == SETTINGS { paper() } else { self.note(id).map(|n| note_rgb(n.color)).unwrap_or(paper()) } }
    fn shown_color(&self, id: &str) -> u32 { self.colors.get(id).map(|c| join(*c)).unwrap_or_else(|| self.target_color(id)) }
    fn input_mode(&self) -> bool { self.finding || self.mode == Mode::List }
    /// the buttons, right to left; colour, drawer and settings hide behind the arrow
    fn controls(&self) -> Vec<Ctl> {
        let edit = self.mode == Mode::Edit && !self.settings;
        let mut v = vec![Ctl::Close, Ctl::Pin];
        if self.mode == Mode::List { v.push(Ctl::New); } else if edit { v.push(Ctl::Find); }
        v.push(Ctl::More);
        if self.ctl_open || self.palette.is_some() {
            v.push(Ctl::Gear);
            v.push(Ctl::List);
            if edit { v.push(Ctl::Trash); v.push(Ctl::Dot); }
        }
        v
    }

    // ---------- RichEdit ----------
    /// the styled paragraph holding a character position
    fn para_at(&self, cp: i32) -> Option<usize> {
        let i = self.pstyle.partition_point(|p| p.b < cp);
        self.pstyle.get(i).filter(|p| p.a <= cp).map(|_| i)
    }
    /// glyph box inside the line box at a position: (box top, box bottom, text top)
    fn hl_at(&self, cp: i32) -> (f32, f32, f32) {
        match self.para_at(cp).map(|i| &self.pstyle[i]) {
            Some(p) => ((p.lh - p.g) / 2.0, (p.lh + p.g) / 2.0, 0.0),
            None => if cp <= self.title_end { hl_title() } else { hl_body() },
        }
    }
    /// a picture's file: relative to the notes folder, or an absolute path; web pictures are not fetched
    fn image_file(&self, path: &str) -> Option<PathBuf> {
        if path.contains("://") && !path.starts_with("file://") { return None; }
        let path = path.strip_prefix("file:///").unwrap_or(path).replace("%20", " ");
        let p = PathBuf::from(path.replace('/', "\\"));
        let p = if p.is_absolute() { p } else { self.dir.join(p) };
        if images::is_image(&p) && p.exists() { Some(p) } else { None }
    }
    unsafe fn edit_text(&self) -> Vec<u16> {
        let len = GetWindowTextLengthW(self.edit).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let gte = GetTextEx { cb: ((len + 1) * 2) as u32, flags: 0, codepage: 1200, default_char: std::ptr::null(), used_default: null_mut() };
        let n = SendMessageW(self.edit, EM_GETTEXTEX, WPARAM(&gte as *const _ as usize), LPARAM(buf.as_mut_ptr() as isize)).0 as usize;
        buf.truncate(n.min(len));
        buf
    }
    unsafe fn select(&self, a: i32, b: i32) {
        let cr = CharRange { min: a, max: b };
        SendMessageW(self.edit, EM_EXSETSEL, WPARAM(0), LPARAM(&cr as *const _ as isize));
    }
    unsafe fn get_sel(&self) -> (i32, i32) {
        let mut cr = CharRange { min: 0, max: 0 };
        SendMessageW(self.edit, EM_EXGETSEL, WPARAM(0), LPARAM(&mut cr as *mut _ as isize));
        (cr.min, cr.max)
    }

    unsafe fn edit_scroll(&self) -> POINT {
        let mut scroll = POINT::default();
        SendMessageW(self.edit, EM_GETSCROLLPOS, WPARAM(0), LPARAM(&mut scroll as *mut _ as isize));
        scroll
    }

    unsafe fn set_edit_scroll(&self, scroll: POINT) {
        SendMessageW(self.edit, EM_SETSCROLLPOS, WPARAM(0), LPARAM(&scroll as *const _ as isize));
    }

    unsafe fn scroll_wheel(&mut self, wp: WPARAM) {
        self.wheel_delta += ((wp.0 >> 16) as i16) as i32;
        let ticks = self.wheel_delta / WHEEL_DELTA as i32;
        self.wheel_delta %= WHEEL_DELTA as i32;
        if ticks == 0 { return; }

        let mut lines = 3u32;
        let _ = SystemParametersInfoW(SPI_GETWHEELSCROLLLINES, 0, Some((&mut lines as *mut u32).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        if lines == u32::MAX {
            let direction = if ticks > 0 { SB_PAGEUP } else { SB_PAGEDOWN };
            for _ in 0..ticks.unsigned_abs() {
                SendMessageW(self.edit, WM_VSCROLL, WPARAM(direction.0 as usize), LPARAM(0));
            }
        } else if lines != 0 {
            // by pixels, not lines: a picture is drawn in one line's spacing, and line steps would jump over it whole
            let step = (lh_body() * self.scale).round() as i32 * lines as i32;
            let mut sp = self.edit_scroll();
            sp.y = (sp.y - ticks * step).max(0);
            self.set_edit_scroll(sp);
        }
        if self.finding { self.update_hits(); }
    }
    unsafe fn editor_kind(&self, at: i32) -> Kind {
        if let Some(list) = self.doc.as_ref().and_then(|d| d.Range(at, at).ok())
            .and_then(|r| r.GetPara().ok()).and_then(|p| p.GetListType().ok()) {
            return match list & 0xffff { 1 => Kind::Bullet, 2..=6 => Kind::Number, _ => Kind::Plain };
        }
        self.select(at, at);
        let mut pf: ParaFormat2 = std::mem::zeroed();
        pf.cb_size = std::mem::size_of::<ParaFormat2>() as u32;
        pf.mask = PFM_NUMBERING;
        SendMessageW(self.edit, EM_GETPARAFORMAT, WPARAM(0), LPARAM(&mut pf as *mut _ as isize));
        match pf.numbering { 1 => Kind::Bullet, 2..=6 => Kind::Number, _ => Kind::Plain }
    }

    /// Unchanged paragraphs keep the formatting RichEdit already holds. Leave the edges
    /// of an edit to be formatted again because RichEdit can merge paragraph attributes.
    fn unchanged_paragraphs(&self, text: &[u16], paras: &[(i32, i32)], caret: i32) -> Vec<Option<usize>> {
        let old = &self.pstyle;
        let matches = |(a, b): (i32, i32), p: &PStyle| {
            text[a as usize..b as usize] == self.styled_text[p.a as usize..p.b as usize]
        };
        let mut same = vec![None; paras.len()];
        if text == self.styled_text {
            for (i, slot) in same.iter_mut().take(old.len()).enumerate() { *slot = Some(i); }
            return same;
        }
        let prefix = paras.iter().zip(old).take_while(|((_, b), _)| *b < caret)
            .take_while(|(span, p)| matches(**span, p)).count();
        let suffix = paras[prefix..].iter().rev().zip(old[prefix..].iter().rev())
            .take_while(|((a, _), _)| *a > caret)
            .take_while(|(span, p)| matches(**span, p)).count();
        for (i, slot) in same.iter_mut().take(prefix.saturating_sub(1)).enumerate() { *slot = Some(i); }
        let end = paras.len();
        for i in 0..suffix.saturating_sub(1) { same[end - 1 - i] = Some(old.len() - 1 - i); }
        same
    }

    unsafe fn restyle(&mut self) {
        self.style_edit(false);
    }

    /// Heading on the first line, body and lists below. Kept out of the undo history.
    unsafe fn style_edit(&mut self, incremental: bool) {
        self.edit_snapshot = None;
        self.guard = true;
        let frozen = self.doc.as_ref().is_some_and(|d| d.Freeze().is_ok());
        if let Some(d) = &self.doc { let _ = d.Undo(TOM_SUSPEND); }
        let keep = self.get_sel();
        // WM_SETREDRAW(TRUE) also makes a hidden window visible, so only use it while RichEdit is shown
        let vis = self.edit_visible && IsWindowVisible(self.edit).as_bool();
        let redraw = vis && (!incremental || !frozen);
        if redraw { SendMessageW(self.edit, WM_SETREDRAW, WPARAM(0), LPARAM(0)); }
        let text = self.edit_text();
        let paras = para_ranges(&text);
        self.title_end = paras[0].1;
        let mut unchanged = if incremental { self.unchanged_paragraphs(&text, &paras, keep.1) } else { vec![None; paras.len()] };
        let mut kinds = match self.load_kinds.take() {
            Some(mut k) => { k.resize(paras.len(), Kind::Plain); k }
            None => paras.iter().zip(&unchanged).map(|(&(a, _), old)| {
                old.map_or_else(|| self.editor_kind(a), |i| self.kinds[i])
            }).collect(),
        };
        if let Some((i, k)) = self.force.take() { if i < kinds.len() { kinds[i] = k; } }
        kinds[0] = Kind::Plain;
        let lines: Vec<&[u16]> = paras.iter().map(|&(a, b)| &text[a as usize..b as usize]).collect();
        let blocks = md::blocks(&lines);
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 && !matches!(b, Block::Plain | Block::Quote) { kinds[i] = Kind::Plain; }
        }
        let edit = self.edit;
        let paper_c = self.target_color(&self.cur.clone());
        for (i, old) in unchanged.iter_mut().enumerate() {
            let Some(j) = *old else { continue };
            let p = &self.pstyle[j];
            let code = |b: Option<&Block>| matches!(b, Some(Block::Code | Block::Fence));
            let changed = (i == 0) != (j == 0) || (i <= 1) != (j <= 1)
                || p.block != blocks[i] || self.kinds[j] != kinds[i]
                || (self.kinds.get(j + 1) != kinds.get(i + 1))
                || code(self.pstyle.get(j + 1).map(|p| &p.block)) != code(blocks.get(i + 1));
            if changed { *old = None; }
        }
        // Only the attributes in the mask change.
        let range = self.doc.as_ref().and_then(|d| d.Range(0, 0).ok());
        let mut font_cache = HashMap::new();
        let mut para_cache = HashMap::new();
        let mut fmt = |a: i32, b: i32, mask: u32, effects: u32, px: f32, color: u32, back: u32, face: &'static str| {
            if a >= b { return; }
            let height = (px * 15.0) as i32;
            let styled = range.as_ref().is_some_and(|r| {
                let font = font_cache.entry((mask, effects, height, color, back, face))
                    .or_insert_with(|| editor_font(r, mask, effects, height, color, back, face).ok());
                font.as_ref().is_some_and(|f| r.SetRange(a, b).and_then(|()| r.SetFont(f)).is_ok())
            });
            if !styled {
                let cr = CharRange { min: a, max: b };
                SendMessageW(edit, EM_EXSETSEL, WPARAM(0), LPARAM(&cr as *const _ as isize));
                let cf = CharFormat2::new(mask, effects, height, color, back, face);
                SendMessageW(edit, EM_SETCHARFORMAT, WPARAM(SCF_SELECTION), LPARAM(&cf as *const _ as isize));
            }
        };
        let all = text.len() as i32 + 1;
        let simple = self.prefs.simple_editor;
        let (mark_mask, mark_effect) = if simple { (CFM_HIDDEN, CFE_HIDDEN) } else { (CFM_COLOR, 0) };
        let base = CFM_FACE | CFM_SIZE | CFM_COLOR | CFM_BOLD | CFM_ITALIC | CFM_STRIKEOUT | CFM_UNDERLINE | CFM_BACKCOLOR | CFM_HIDDEN;
        let reset_all = unchanged.iter().all(Option::is_none);
        if reset_all {
            fmt(0, all, base, CFE_AUTOBACKCOLOR, 16.0 * ts(), ink2(), AUTO, "Geologica");
        }
        let s = ts();
        let title_size = 21.0 * s;
        let tw = |v: f32| (v * 15.0) as i32;
        let avail = ww() - 2.0 * PAD - 4.0;
        let mut pstyle = Vec::with_capacity(paras.len());
        let mut links = Vec::new();
        let mut codes = Vec::new();
        let mut task_widths = [None; 8];
        for (i, &(a, b)) in paras.iter().enumerate() {
            if let Some(j) = unchanged[i] {
                let mut p = self.pstyle[j].clone();
                let delta = a - p.a;
                let in_line = |start| start >= p.a && start <= p.b;
                let first_link = self.links.partition_point(|l| l.0 < p.a);
                let first_code = self.codes.partition_point(|c| c.0 < p.a);
                links.extend(self.links[first_link..].iter().take_while(|l| in_line(l.0)).map(|l| (l.0 + delta, l.1 + delta, l.2.clone())));
                codes.extend(self.codes[first_code..].iter().take_while(|c| in_line(c.0)).map(|c| (c.0 + delta, c.1 + delta)));
                p.a = a;
                p.b = b;
                pstyle.push(p);
                continue;
            }
            if !reset_all { fmt(a, b + 1, base, CFE_AUTOBACKCOLOR, 16.0 * s, ink2(), AUTO, "Geologica"); }
            let blk = if i == 0 { Block::Plain } else { blocks[i].clone() };
            let line = lines[i];
            let task = i > 0 && kinds[i] == Kind::Plain && matches!(line.first(), Some(&TASK) | Some(&DONE));
            let code_like = |j: usize| j > 0 && matches!(blocks.get(j), Some(Block::Code) | Some(Block::Fence));
            let (lh, g, size) = match &blk {
                _ if i == 0 => (lh_title(), 26.0 * s, title_size),
                Block::Heading(1) => (lh_title(), 26.0 * s, title_size),
                Block::Heading(2) => ((25.0 * s).round(), 23.0 * s, 18.5 * s),
                Block::Heading(_) => (lh_body(), 20.0 * s, 16.0 * s),
                Block::Code | Block::Fence => ((21.0 * s).round(), 18.0 * s, 14.5 * s),
                Block::Image(_) => ((18.0 * s).round(), 15.0 * s, 12.0 * s),
                _ => (lh_body(), 20.0 * s, 16.0 * s),
            };
            // a picture: the line keeps its link and the picture is drawn in the gap below it
            let mut kind = match &blk { Block::Quote => PK::Quote, Block::Code | Block::Fence => PK::Code, _ => PK::Text };
            let mut pic_h = 0.0;
            if let Block::Image(path) = &blk {
                if let Some(p) = self.image_file(path) {
                    if let Some((w, h)) = self.imgs.size(&p) {
                        let mut dw = (w as f32 / self.scale).min(avail);
                        let mut dh = dw * h as f32 / w as f32;
                        let max_h = 380.0 * s;
                        if dh > max_h { dh = max_h; dw = dh * w as f32 / h as f32; }
                        pic_h = dh.round();
                        kind = PK::Image(p, dw.round(), pic_h);
                    }
                }
            }
            // Simple mode hides a shown picture's link, and its line shrinks to a hairline above the picture
            let hide_link = simple && pic_h > 0.0;
            let (lh, g, size) = if hide_link { ((2.0 * s).round().max(1.0), (2.0 * s).round().max(1.0), 1.0) } else { (lh, g, size) };
            let mut pf: ParaFormat2 = std::mem::zeroed();
            pf.cb_size = std::mem::size_of::<ParaFormat2>() as u32;
            pf.mask = PFM_SPACEAFTER | PFM_SPACEBEFORE | PFM_LINESPACING | PFM_NUMBERING | PFM_STARTINDENT | PFM_OFFSET | PFM_NUMBERINGSTYLE | PFM_NUMBERINGTAB | PFM_NUMBERINGSTART;
            pf.line_spacing_rule = 4;
            pf.line_spacing = tw(lh);
            let list = kinds[i] != Kind::Plain;
            pf.space_after = if i == 0 { tw(gap_title()) }
                else if list && kinds.get(i + 1).map_or(false, |k| *k != Kind::Plain) { tw(gap_list()) }
                else if code_like(i) && code_like(i + 1) { 0 }
                else if pic_h > 0.0 { tw(gap_body() + pic_h + 6.0 * s) }
                else { tw(gap_body()) };
            if matches!(blk, Block::Heading(_)) && i > 1 { pf.space_before = tw(gap_body()); }
            if list {
                pf.numbering = if kinds[i] == Kind::Bullet { 1 } else { 2 };
                pf.numbering_style = 0x200; // "1."
                pf.numbering_start = 1;
                pf.numbering_tab = (20.0 * s * 15.0) as u16;
                pf.start_indent = (4.0 * s * 15.0) as i32;
                pf.offset = (20.0 * s * 15.0) as i32;
            } else if matches!(kind, PK::Quote) { pf.start_indent = tw(16.0 * s); }
            else if matches!(kind, PK::Code) { pf.start_indent = tw(10.0 * s); }
            let styled = range.as_ref().is_some_and(|r| {
                let key = (pf.numbering, pf.space_before, pf.space_after, pf.line_spacing, pf.start_indent);
                let para = para_cache.entry(key).or_insert_with(|| editor_paragraph(r, &pf).ok());
                para.as_ref().is_some_and(|p| r.SetRange(a, b).and_then(|()| r.SetPara(p)).is_ok())
            });
            if !styled {
                self.select(a, b);
                SendMessageW(edit, EM_SETPARAFORMAT, WPARAM(0), LPARAM(&pf as *const _ as isize));
            }

            // characters: the paragraph's own look, then the inline marks over it
            let head = i == 0 || matches!(blk, Block::Heading(_));
            match &blk {
                _ if head => fmt(a, b + 1, CFM_SIZE | CFM_COLOR | CFM_BOLD, CFE_BOLD, size, ink(), 0, ""),
                Block::Code => fmt(a, b, CFM_FACE | CFM_SIZE | CFM_COLOR, 0, size, ink(), 0, "Consolas"),
                Block::Fence => fmt(a, b, CFM_FACE | CFM_SIZE | CFM_COLOR, 0, size, ink3(), 0, "Consolas"),
                Block::Image(_) if hide_link => fmt(a, b, CFM_SIZE | CFM_HIDDEN, CFE_HIDDEN, size, 0, 0, ""),
                Block::Image(_) => fmt(a, b, CFM_SIZE | CFM_COLOR, 0, size, ink3(), 0, ""),
                Block::Quote => fmt(a, b, CFM_ITALIC, CFE_ITALIC, size, 0, 0, ""),
                _ => {}
            }
            let mut from = a;
            let mark_blk = if i == 0 { &blocks[0] } else { &blk };
            if matches!(mark_blk, Block::Heading(_) | Block::Quote) {
                let m = md::mark_len(line, mark_blk) as i32;
                fmt(a, a + m, mark_mask, mark_effect, 0.0, ink3(), 0, "");
                from = a + m;
            }
            if simple && (blk == Block::Fence || pic_h > 0.0) {
                fmt(a, b, mark_mask, mark_effect, 0.0, 0, 0, "");
            }
            if task {
                let space = line.get(1) == Some(&32);
                let text_at = a + 1 + i32::from(space);
                let width = &mut task_widths[usize::from(line[0] == DONE)
                    | (usize::from(space) << 1) | (usize::from(blk == Block::Code) << 2)];
                let base = 16.0 * s;
                fmt(a, a + 1, CFM_FACE | CFM_COLOR | CFM_SIZE, 0, width.unwrap_or(base), paper_c, 0, "Segoe UI Symbol");
                // Measure each kind of placeholder once per styling pass.
                if width.is_none() {
                    let pos = |cp: i32| {
                        let mut p = POINT::default();
                        SendMessageW(edit, EM_POSFROMCHAR, WPARAM(&mut p as *mut _ as usize), LPARAM(cp as isize));
                        p
                    };
                    let (start, end, body) = (pos(a), pos(a + 1), pos(text_at));
                    if start.y == end.y && start.y == body.y && end.x > start.x + 1 {
                        let gap = (body.x - end.x) as f32;
                        let want = (TASK_BOX + TASK_GAP) * s * self.scale;
                        let px = if want > gap { (base * (want - gap) / (end.x - start.x) as f32).clamp(8.0, 60.0) } else { base };
                        *width = Some(px);
                        if px != base { fmt(a, a + 1, CFM_SIZE, 0, px, paper_c, 0, ""); }
                    }
                }
                if line[0] == DONE { fmt(text_at, b, CFM_STRIKEOUT | CFM_COLOR, CFE_STRIKEOUT, 0.0, ink3(), 0, ""); }
                from = a + 1;
            }
            let missing_picture = simple && matches!(blk, Block::Image(_)) && pic_h == 0.0;
            if (matches!(blk, Block::Plain | Block::Quote | Block::Heading(_)) || missing_picture) && from < b {
                let seg = &text[from as usize..b as usize];
                let (spans, ls) = md::inline(seg);
                let at = |k: usize| from + k as i32;
                let ordered = spans.iter().filter(|x| x.style != Md::Mark).chain(spans.iter().filter(|x| x.style == Md::Mark));
                for sp in ordered {
                    let (x0, x1) = (at(sp.a), at(sp.b));
                    match sp.style {
                        Md::Mark => fmt(x0, x1, mark_mask, mark_effect, 0.0, ink3(), 0, ""),
                        Md::Bold => fmt(x0, x1, CFM_BOLD | CFM_COLOR, CFE_BOLD, 0.0, ink(), 0, ""),
                        Md::Italic => fmt(x0, x1, CFM_ITALIC, CFE_ITALIC, 0.0, 0, 0, ""),
                        Md::Strike => fmt(x0, x1, CFM_STRIKEOUT, CFE_STRIKEOUT, 0.0, 0, 0, ""),
                        Md::Highlight => fmt(x0, x1, CFM_BACKCOLOR, 0, 0.0, 0, highlight_c(), ""),
                        Md::Code => { fmt(x0, x1, CFM_FACE | CFM_SIZE | CFM_COLOR, 0, size * 0.9, ink(), 0, "Consolas"); codes.push((x0, x1)); }
                        Md::Link => fmt(x0, x1, CFM_COLOR, 0, 0.0, link_c(), 0, ""),
                        Md::Url => {
                            let in_link = sp.a >= 2 && seg[sp.a - 1] == b'(' as u16 && seg[sp.a - 2] == b']' as u16;
                            let empty_label = sp.a >= 3 && seg[sp.a - 3] == b'[' as u16;
                            if in_link && !(simple && empty_label) { fmt(x0, x1, mark_mask, mark_effect, 0.0, ink3(), 0, ""); }
                            else { fmt(x0, x1, CFM_COLOR | CFM_UNDERLINE, CFE_UNDERLINE, 0.0, link_c(), 0, ""); }
                        }
                    }
                }
                for l in ls { links.push((at(l.a), at(l.b), l.url)); }
            }
            if let Block::Image(p) = &blk { links.push((a, b, p.clone())); }
            pstyle.push(PStyle { a, b, lh, g, kind, block: blocks[i].clone(), task: task.then(|| line[0] == DONE) });
        }
        self.pstyle = pstyle;
        self.styled_text = text;
        links.sort_by_key(|l| l.0);
        codes.sort_unstable();
        self.links = links;
        self.codes = codes;
        self.kinds = kinds;
        let hit = self.hits.get(self.hit).filter(|_| self.finding).map(|&(a, _)| a);
        if let Some(a) = hit { self.select(a, a); } else { self.select(keep.0, keep.1); }
        if self.styled_text.is_empty() {
            // The paragraph mark's font does not set RichEdit's empty insertion point.
            let cf = CharFormat2::new(base, CFE_BOLD | CFE_AUTOBACKCOLOR, tw(title_size), ink(), AUTO, "Geologica");
            SendMessageW(self.edit, EM_SETCHARFORMAT, WPARAM(SCF_SELECTION), LPARAM(&cf as *const _ as isize));
        }
        if let Some(d) = &self.doc {
            let _ = d.Undo(TOM_RESUME);
            if frozen { let _ = d.Unfreeze(); }
        }
        if redraw {
            SendMessageW(self.edit, WM_SETREDRAW, WPARAM(1), LPARAM(0));
        }
        if vis {
            if hit.is_some() { SendMessageW(self.edit, EM_SCROLLCARET, WPARAM(0), LPARAM(0)); }
            let _ = InvalidateRect(self.edit, None, false);
        }
        self.guard = false;
        self.seen.0 = -1;
        if vis || !self.edit_visible { self.update_hits(); }
    }
    /// what RichEdit shows, written back as Markdown (list markers included)
    fn markdown(&self) -> String {
        let text = String::from_utf16_lossy(&self.styled_text);
        let mut out = String::with_capacity(text.len());
        let mut n = 0;
        for (i, line) in text.split('\r').enumerate() {
            if i > 0 { out.push('\n'); }
            match self.kinds.get(i).copied().unwrap_or(Kind::Plain) {
                Kind::Bullet => { n = 0; out.push_str("- "); out.push_str(line); }
                Kind::Number => { n += 1; out.push_str(&n.to_string()); out.push_str(". "); out.push_str(line); }
                Kind::Plain => {
                    n = 0;
                    // task boxes go back to "- [ ] " / "- [x] "
                    let mut ch = line.chars();
                    match ch.next() {
                        Some(c @ ('\u{2610}' | '\u{2611}')) => {
                            out.push_str(if c == '\u{2611}' { "- [x] " } else { "- [ ] " });
                            out.push_str(ch.as_str().strip_prefix(' ').unwrap_or(ch.as_str()));
                        }
                        _ => out.push_str(line),
                    }
                }
            }
        }
        out
    }
    unsafe fn load_editor(&mut self) {
        self.guard = true;
        let (w, kinds) = editor_markdown(&self.cur_note().text);
        let _ = SetWindowTextW(self.edit, PCWSTR(w.as_ptr()));
        let c = self.shown_color(&self.cur.clone());
        SendMessageW(self.edit, EM_SETBKGNDCOLOR, WPARAM(0), LPARAM(bgr(c) as isize));
        self.guard = false;
        self.load_kinds = Some(kinds);
        self.restyle();
        SendMessageW(self.edit, EM_EMPTYUNDOBUFFER, WPARAM(0), LPARAM(0));
        let n = self.edit_text().len() as i32;
        self.select(n, n);
    }
    /// "- " or "1. " typed at the start of a line turns it into a list item
    unsafe fn auto_list(&mut self) {
        let (s0, s1) = self.get_sel();
        if s0 != s1 { return; }
        let text = self.edit_text();
        let paras = para_ranges(&text);
        let Some(i) = paras.iter().position(|&(a, b)| s0 >= a && s0 <= b) else { return };
        if i == 0 { return; }
        let (a, b) = paras[i];
        let line = String::from_utf16_lossy(&text[a as usize..b as usize]);
        // "[ ] " or "[x] " typed at the start of a line or a bullet makes a task box
        for (p, done) in [("[ ] ", false), ("[] ", false), ("[x] ", true)] {
            if line.starts_with(p) && s0 == a + p.len() as i32 {
                self.guard = true;
                self.select(a, s0);
                let g = [if done { DONE } else { TASK }, 32, 0];
                SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(g.as_ptr() as isize));
                self.guard = false;
                if self.kinds.get(i).copied().unwrap_or(Kind::Plain) != Kind::Plain { self.force = Some((i, Kind::Plain)); }
                return;
            }
        }
        if self.kinds.get(i).copied().unwrap_or(Kind::Plain) != Kind::Plain { return; }
        let (k, rest) = store::marker(&line);
        if k == Kind::Plain { return; }
        let mlen = (line.encode_utf16().count() - rest.encode_utf16().count()) as i32;
        if s0 != a + mlen { return; }
        self.guard = true;
        self.select(a, a + mlen);
        let empty = [0u16];
        SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(empty.as_ptr() as isize));
        self.guard = false;
        self.force = Some((i, k));
    }
    /// Enter on an empty list item or Backspace at its start ends the list
    unsafe fn list_key(&mut self, vk: u16) -> bool {
        if vk != VK_RETURN.0 && vk != VK_BACK.0 { return false; }
        if vk == VK_RETURN.0 && GetKeyState(VK_SHIFT.0 as i32) < 0 { return false; }
        let (s0, s1) = self.get_sel();
        let text = self.edit_text();
        let paras = para_ranges(&text);
        let Some(i) = paras.iter().position(|&(a, b)| s0 >= a && s0 <= b) else { return false };
        let (a, b) = paras[i];
        let task = i > 0 && self.kinds.get(i).copied().unwrap_or(Kind::Plain) == Kind::Plain && matches!(text.get(a as usize), Some(&TASK) | Some(&DONE));
        // Ctrl+Enter ticks a task, or makes the line one
        if vk == VK_RETURN.0 && GetKeyState(VK_CONTROL.0 as i32) < 0 {
            if i == 0 { return false; }
            if task { self.toggle_task(a); }
            else {
                if self.kinds.get(i).copied().unwrap_or(Kind::Plain) != Kind::Plain { self.force = Some((i, Kind::Plain)); }
                self.select(a, a);
                let g = [TASK, 32, 0];
                SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(g.as_ptr() as isize));
                self.select(s0 + 2, s1 + 2);
            }
            return true;
        }
        if s0 != s1 { return false; }
        if task {
            let body = if text.get(a as usize + 1) == Some(&32) { a + 2 } else { a + 1 };
            let rep = |this: &Self, x: i32, y: i32, w: &[u16]| { this.select(x, y); SendMessageW(this.edit, EM_REPLACESEL, WPARAM(1), LPARAM(w.as_ptr() as isize)); };
            if vk == VK_BACK.0 && s0 > a && s0 <= body { rep(self, a, body, &[0]); return true; }
            if vk == VK_RETURN.0 && s0 >= body {
                // Enter on an empty task ends the tasks, otherwise the next line is a task too
                if b == body { rep(self, a, b, &[0]); } else { rep(self, s0, s0, &[13, TASK, 32, 0]); }
                return true;
            }
            return false;
        }
        if self.kinds.get(i).copied().unwrap_or(Kind::Plain) == Kind::Plain { return false; }
        let end = if vk == VK_RETURN.0 { a == b } else { s0 == a };
        if !end { return false; }
        self.force = Some((i, Kind::Plain));
        self.restyle();
        self.after_change();
        true
    }
    /// ticks or unticks the task box at a position (kept in the undo history)
    unsafe fn toggle_task(&mut self, at: i32) {
        let text = self.edit_text();
        let Some(&c) = text.get(at as usize) else { return };
        if c != TASK && c != DONE { return; }
        let keep = self.get_sel();
        self.select(at, at + 1);
        let g = [if c == TASK { DONE } else { TASK }, 0];
        SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(g.as_ptr() as isize));
        self.select(keep.0, keep.1);
    }
    unsafe fn on_edit_change(&mut self) {
        if self.guard { return; }
        self.auto_list();
        if self.finding { self.find_hits(false); } else { self.style_edit(true); }
        self.after_change();
    }

    /// Keep delimiters paired when a visible selection replaces part of a formatted span.
    fn simple_selection(&self, start: i32, end: i32, erase: i32) -> Option<(i32, i32, Vec<u16>, Vec<u16>)> {
        if !self.prefs.simple_editor { return None; }
        let text = &self.styled_text;
        let (mut a, mut b) = (start.max(0) as usize, (end.max(0) as usize).min(text.len()));
        if a == 0 && b == text.len() { return None; }
        if a == b && erase == 0 { return None; }
        let first = self.pstyle.partition_point(|p| (p.b as usize) < a);
        let mut pairs = Vec::new();
        for p in self.pstyle[first..].iter().take_while(|p| p.a as usize <= b) {
            if p.a != 0 && matches!(p.block, Block::Fence | Block::Code | Block::Image(_)) { continue; }
            let line = &text[p.a as usize..p.b as usize];
            let offset = p.a as usize + md::mark_len(line, &p.block);
            pairs.extend(md::delimiters(&text[offset..p.b as usize]).into_iter()
                .map(|(l, r)| (offset + l.start..offset + l.end, offset + r.start..offset + r.end)));
        }
        if pairs.is_empty() { return None; }
        if a == b {
            let mut cp = if erase < 0 { a.checked_sub(1)? } else { b };
            while let Some(r) = pairs.iter().flat_map(|(l, r)| [l, r]).find(|r| r.contains(&cp)) {
                cp = if erase < 0 { r.start.checked_sub(1)? } else { r.end };
            }
            let ch = *text.get(cp)?;
            a = cp - usize::from((0xdc00..=0xdfff).contains(&ch) && cp > 0);
            b = cp + 1 + usize::from((0xd800..=0xdbff).contains(&ch) && cp + 1 < text.len());
        }
        md::preserve_delimiters(text, a..b, pairs, erase != 0)
            .map(|(range, before, after)| (range.start as i32, range.end as i32, before, after))
    }

    unsafe fn simple_edit_key(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if !self.prefs.simple_editor || self.guard { return None; }
        let erase = match (msg, wp.0) {
            (WM_KEYDOWN, k) if k == VK_BACK.0 as usize && cur_mods() & !4 == 0 => -1,
            (WM_KEYDOWN, k) if k == VK_DELETE.0 as usize && cur_mods() == 0 => 1,
            (WM_CLEAR | WM_CUT, _) => 1,
            (WM_CHAR, c) if c >= 32 || matches!(c, 9 | 13) => 0,
            _ => return None,
        };
        let (a, b) = self.get_sel();
        if a == b && matches!(msg, WM_CUT | WM_CLEAR) { return None; }
        let (start, end, before, after) = self.simple_selection(a, b, erase)?;
        let old: WNDPROC = std::mem::transmute(OLD_EDIT);
        let doc = self.doc.clone();
        let undo = doc.as_ref().is_some_and(|d| d.BeginEditCollection().is_ok());
        self.guard = true;
        if msg == WM_CUT { CallWindowProcW(old, self.edit, WM_COPY, WPARAM(0), LPARAM(0)); }
        let pos = start + before.len() as i32;
        let replacement: Vec<_> = before.into_iter().chain(after).chain([0]).collect();
        self.select(start, end);
        SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(replacement.as_ptr() as isize));
        self.select(pos, pos);
        let result = if erase == 0 { CallWindowProcW(old, self.edit, msg, wp, lp) } else { LRESULT(0) };
        if let Some(d) = &doc { if undo { let _ = d.EndEditCollection(); } }
        self.guard = false;
        self.on_edit_change();
        SendMessageW(self.edit, EM_SCROLLCARET, WPARAM(0), LPARAM(0));
        Some(result)
    }

    unsafe fn toggle_format(&mut self, format: md::Format) {
        if !self.edit_visible || GetFocus() != self.edit { return; }
        let (a, b) = self.get_sel();
        if a >= b { return; }
        let text = &self.styled_text;
        let mut edits = Vec::new();
        let first = self.pstyle.partition_point(|p| p.b <= a);
        for p in self.pstyle[first..].iter().take_while(|p| p.a < b) {
            if matches!(p.block, Block::Fence | Block::Code | Block::Image(_)) { continue; }
            let line = &text[p.a as usize..p.b as usize];
            let prefix = if p.task.is_some() { 1 } else { md::mark_len(line, &p.block) };
            let start = ((a - p.a).max(0) as usize).max(prefix);
            let end = (b.min(p.b) - p.a) as usize;
            if start >= end { continue; }
            if let Some((range, marker)) = md::format_edit(line, start..end, format, self.prefs.simple_editor) {
                edits.push((p.a as usize + range.start..p.a as usize + range.end, marker));
            }
        }
        if edits.is_empty() { return; }
        let unwrap = edits.iter().all(|(_, marker)| marker.is_none());
        edits.retain(|(_, marker)| marker.is_none() == unwrap);
        let width = format.width();
        let start = edits[0].0.start;
        let end = edits.last().unwrap().0.end;
        let mut out = Vec::new();
        let mut at = start;
        for (range, marker) in edits {
            out.extend_from_slice(&text[at..range.start]);
            if let Some(marker) = marker {
                out.extend_from_slice(marker);
                out.extend_from_slice(&text[range.clone()]);
                out.extend_from_slice(marker);
            } else {
                out.extend_from_slice(&text[range.start + width..range.end - width]);
            }
            at = range.end;
        }
        let end_after = start + out.len();

        // Keep native list formats if this replacement spans multiple paragraphs.
        self.load_kinds = Some(self.kinds.clone());
        self.guard = true;
        let (start, end) = (start as i32, end as i32);
        // RichEdit snaps the visual selection across adjacent hidden markers.
        let replaced = self.prefs.simple_editor && self.doc.as_ref().is_some_and(|d| {
            d.Range(start, end).and_then(|range| BSTR::from_wide(&out).and_then(|text| range.SetText(&text))).is_ok()
        });
        if !replaced {
            self.select(start, end);
            if self.get_sel() != (start, end) {
                self.load_kinds = None;
                self.select(a, b);
                self.guard = false;
                return;
            }
            out.push(0);
            SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(out.as_ptr() as isize));
        }
        self.select(start, end_after as i32);
        self.guard = false;
        if self.finding { self.find_hits(false); } else { self.style_edit(true); }
        self.after_change();
    }
    unsafe fn after_change(&mut self) {
        let md = self.markdown();
        let id = self.cur.clone();
        if let Some(n) = self.note_mut(&id) { n.set_text(md); n.mtime = store::now_ft(); }
        SetTimer(self.hwnd, TIMER_SAVE, 400, None);
        self.invalidate();
    }
    fn save_now(&mut self) {
        unsafe { let _ = KillTimer(self.hwnd, TIMER_SAVE); }
        let n = self.cur_note().clone();
        if !n.is_empty() { let _ = store::save(&self.dir, &n); }
    }
    /// leaving an empty note removes it, like the prototype
    fn drop_empty(&mut self) {
        let n = self.cur_note().clone();
        if n.is_empty() && self.notes.len() > 1 {
            store::delete(&self.dir, &n);
            self.notes.retain(|x| x.id != n.id);
            self.anims.remove(&n.id);
        }
    }

    // ---------- pictures, paste and drop ----------
    fn assets(&self) -> PathBuf { self.dir.join("assets") }
    /// a free file name in the assets folder: name.ext, name-2.ext, ...
    fn free_asset(&self, stem: &str, ext: &str) -> PathBuf {
        let d = self.assets();
        let _ = std::fs::create_dir_all(&d);
        let clean: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
        let clean = if clean.is_empty() { "image".to_string() } else { clean };
        (1..).map(|k| d.join(if k == 1 { format!("{clean}.{ext}") } else { format!("{clean}-{k}.{ext}") })).find(|p| !p.exists()).unwrap()
    }
    fn pic_link(p: &std::path::Path) -> String { format!("![](assets/{})", p.file_name().unwrap_or_default().to_string_lossy()) }
    fn pic_stem(&self) -> String {
        let t = local(store::now_ft());
        format!("{}-{:04}{:02}{:02}-{:02}{:02}{:02}", self.cur, t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }
    /// an image file brought in from outside: copied into the assets folder (unless it is there already)
    fn import_file(&self, src: &std::path::Path) -> Option<String> {
        if !images::is_image(src) || !src.is_file() { return None; }
        if src.parent() == Some(self.assets().as_path()) { return Some(Self::pic_link(src)); }
        let stem = src.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let ext = src.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        let to = self.free_asset(&stem, &ext);
        std::fs::copy(src, &to).ok()?;
        Some(Self::pic_link(&to))
    }
    /// a picture that exists only as bytes (dragged out of a browser): saved into the assets folder
    fn import_bytes(&mut self, name: &str, bytes: &[u8]) -> Option<String> {
        let name = std::path::Path::new(name);
        if !images::is_image(name) { return None; }
        let stem = name.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let ext = name.extension().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        let to = self.free_asset(&stem, &ext);
        std::fs::write(&to, bytes).ok()?;
        if self.imgs.size(&to).is_none() { let _ = std::fs::remove_file(&to); return None; }
        Some(Self::pic_link(&to))
    }
    unsafe fn drop_paths(&self, drop: HDROP) -> (Vec<String>, Vec<String>) {
        let (mut pics, mut other) = (Vec::new(), Vec::new());
        for i in 0..DragQueryFileW(drop, u32::MAX, None) {
            let n = DragQueryFileW(drop, i, None) as usize;
            let mut b = vec![0u16; n + 1];
            DragQueryFileW(drop, i, Some(&mut b));
            let p = PathBuf::from(String::from_utf16_lossy(&b[..n]));
            match self.import_file(&p) { Some(l) => pics.push(l), None => other.push(p.display().to_string()) }
        }
        (pics, other)
    }
    /// Ctrl+V in the note: pictures become files in assets plus a Markdown link, everything else plain text
    unsafe fn edit_paste(&mut self) {
        if OpenClipboard(self.hwnd).is_err() { return; }
        let mut pics = Vec::new();
        let mut text = None;
        let png = RegisterClipboardFormatW(w!("PNG"));
        if IsClipboardFormatAvailable(15).is_ok() {
            if let Ok(h) = GetClipboardData(15) {
                let (p, o) = self.drop_paths(HDROP(h.0));
                pics = p;
                if !o.is_empty() { text = Some(o.join("\n")); }
            }
        } else if IsClipboardFormatAvailable(13).is_ok() {
            if let Ok(h) = GetClipboardData(13) {
                let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
                if !p.is_null() {
                    let mut n = 0; while *p.add(n) != 0 { n += 1; }
                    text = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                    let _ = GlobalUnlock(HGLOBAL(h.0));
                }
            }
        } else if png != 0 && IsClipboardFormatAvailable(png).is_ok() {
            if let Ok(h) = GetClipboardData(png) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u8;
                let n = GlobalSize(g);
                if !p.is_null() && n > 8 {
                    let to = self.free_asset(&self.pic_stem(), "png");
                    if std::fs::write(&to, std::slice::from_raw_parts(p, n)).is_ok() { pics.push(Self::pic_link(&to)); }
                }
                if !p.is_null() { let _ = GlobalUnlock(g); }
            }
        } else if IsClipboardFormatAvailable(2).is_ok() {
            if let Ok(h) = GetClipboardData(2) {
                let to = self.free_asset(&self.pic_stem(), "png");
                if self.imgs.save_bitmap(HBITMAP(h.0), &to) { pics.push(Self::pic_link(&to)); }
            }
        }
        let _ = CloseClipboard();
        if !pics.is_empty() { self.insert_pictures(&pics); }
        if let Some(t) = text { self.insert_text(&t); }
    }
    /// files dropped on the note
    unsafe fn drop_files(&mut self, drop: HDROP) {
        let mut pt = POINT::default();
        let _ = DragQueryPoint(drop, &mut pt);
        let (pics, other) = self.drop_paths(drop);
        DragFinish(drop);
        self.drop_at(pt, &pics, &other);
    }
    /// files or pictures dragged onto the note, handed over by RichEdit's OLE callback
    unsafe fn dropped(&mut self) {
        let Some(d) = drop::take() else { return };
        let (mut pics, mut other) = (Vec::new(), Vec::new());
        for item in d.items {
            match item {
                drop::Item::File(p) => match self.import_file(&p) { Some(l) => pics.push(l), None => other.push(p.display().to_string()) },
                drop::Item::Data(name, bytes) => if let Some(l) = self.import_bytes(&name, &bytes) { pics.push(l) },
            }
        }
        let mut pt = d.at;
        let _ = ScreenToClient(self.edit, &mut pt);
        self.drop_at(pt, &pics, &other);
    }
    /// pictures and file names put where they were dropped (`pt` in the editor's client coordinates)
    unsafe fn drop_at(&mut self, pt: POINT, pics: &[String], other: &[String]) {
        if pics.is_empty() && other.is_empty() { return; }
        let pl = POINTL { x: pt.x, y: pt.y };
        let cp = SendMessageW(self.edit, EM_CHARFROMPOS, WPARAM(0), LPARAM(&pl as *const _ as isize)).0 as i32;
        let _ = SetForegroundWindow(self.hwnd);
        let _ = SetFocus(self.edit);
        self.select(cp, cp);
        if !pics.is_empty() { self.insert_pictures(&pics); }
        if !other.is_empty() { self.insert_text(&other.join("\n")); }
    }
    /// picture lines go on lines of their own, never into the heading
    unsafe fn insert_pictures(&mut self, links: &[String]) {
        let (mut s0, s1) = self.get_sel();
        if s0 <= self.title_end { s0 = self.title_end; self.select(s0, s0.max(s1)); }
        let text = self.edit_text();
        let paras = para_ranges(&text);
        let i = paras.iter().position(|&(a, b)| s0 >= a && s0 <= b).unwrap_or(0);
        let before = if i == 0 || s0 > paras[i].0 { "\r" } else { "" };
        // at the end of a line that has one below, that line follows the picture: no empty line in between
        let s1 = s1.max(s0);
        let k = paras.iter().position(|&(a, b)| s1 >= a && s1 <= b).unwrap_or(i);
        let joined = s1 == paras[k].1 && k + 1 < paras.len();
        let after = if joined { "" } else { "\r" };
        let w = wide(&format!("{before}{}{after}", links.join("\r")));
        SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(w.as_ptr() as isize));
        if joined { let (c, _) = self.get_sel(); self.select(c + 1, c + 1); }
        // the empty line after the pictures must not carry on a list it was split from
        let (c, _) = self.get_sel();
        if let Some(j) = self.para_at(c) {
            if self.kinds.get(j).map_or(false, |k| *k != Kind::Plain) && self.pstyle[j].a == self.pstyle[j].b {
                self.force = Some((j, Kind::Plain));
                self.restyle();
                self.after_change();
            }
        }
    }
    /// plain text; list markers and task boxes on the pasted lines become lists, as if typed
    unsafe fn insert_text(&mut self, t: &str) {
        let (mut start, end) = self.get_sel();
        let selection = self.simple_selection(start, end, 0);
        let text = t.replace("\r\n", "\n").replace('\r', "\n");
        let redraw = self.edit_visible && IsWindowVisible(self.edit).as_bool();
        if redraw { SendMessageW(self.edit, WM_SETREDRAW, WPARAM(0), LPARAM(0)); }
        let doc = self.doc.clone();
        let frozen = doc.as_ref().is_some_and(|d| d.Freeze().is_ok());
        let undo_group = doc.as_ref().is_some_and(|d| d.BeginEditCollection().is_ok());
        self.guard = true;
        if let Some((a, b, before, after)) = selection {
            start = a + before.len() as i32;
            let replacement: Vec<_> = before.into_iter().chain(after).chain([0]).collect();
            self.select(a, b);
            SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(replacement.as_ptr() as isize));
            self.select(start, start);
        }
        // Select All includes RichEdit's final paragraph mark.
        let kinds = if start == 0 && end as usize >= self.styled_text.len() && !text.contains('\0') {
            let (w, kinds) = editor_markdown(&text);
            SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(w.as_ptr() as isize));
            self.pstyle.clear();
            kinds
        } else {
            self.insert_partial_markdown(&text, start)
        };
        if let Some(d) = &doc { if undo_group { let _ = d.EndEditCollection(); } }
        self.guard = false;
        self.load_kinds = Some(kinds);
        if self.finding { self.find_hits(false); } else { self.style_edit(true); }
        if let Some(d) = &doc { if frozen { let _ = d.Unfreeze(); } }
        if redraw {
            SendMessageW(self.edit, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            let _ = InvalidateRect(self.edit, None, true);
        }
        SendMessageW(self.edit, EM_SCROLLCARET, WPARAM(0), LPARAM(0));
        self.update_hits();
        self.after_change();
    }

    /// Partial pastes inherit native paragraph formatting at the insertion point.
    unsafe fn insert_partial_markdown(&mut self, text: &str, s0: i32) -> Vec<Kind> {
        let w: Vec<u16> = text.replace('\n', "\r").encode_utf16().chain(Some(0)).collect();
        SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(w.as_ptr() as isize));
        let end = self.get_sel().1;
        let text = self.edit_text();
        let paras = para_ranges(&text);
        let unchanged = self.unchanged_paragraphs(&text, &paras, s0);
        let mut kinds: Vec<_> = paras.iter().zip(unchanged).map(|(&(a, _), old)| {
            old.map_or_else(|| self.editor_kind(a), |i| self.kinds[i])
        }).collect();
        let mut converted = Vec::new();
        let mut from = s0;
        for i in 1..paras.len() {
            let (a, b) = paras[i];
            if a >= end { break; }
            if a < s0 || kinds[i] != Kind::Plain { continue; }
            let line = String::from_utf16_lossy(&text[a as usize..b.min(end) as usize]);
            let (k, rest) = store::marker(&line);
            if k == Kind::Plain { continue; }
            let len = |x: &str| x.encode_utf16().count() as i32;
            let (nk, n, task) = match store::task(k, rest) {
                Some((done, r)) => (Kind::Plain, len(&line) - len(r), Some(if done { DONE } else { TASK })),
                None => (k, len(&line) - len(rest), None),
            };
            converted.extend_from_slice(&text[from as usize..a as usize]);
            if let Some(c) = task { converted.extend([c, 32]); }
            from = a + n;
            kinds[i] = nk;
        }
        if from != s0 {
            converted.extend_from_slice(&text[from as usize..end as usize]);
            converted.push(0);
            self.select(s0, end);
            SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(converted.as_ptr() as isize));
        } else {
            self.select(end, end);
        }
        // Match the editor's typed-prefix behaviour for short pasted task/list markers.
        if w.len() <= 6 { self.auto_list(); }
        kinds
    }
    /// where paragraph `i`'s picture is drawn (editor client px): under its link line, narrowed to fit
    unsafe fn picture_box(&self, i: usize, rc: RECT) -> Option<(i32, i32, i32, i32)> {
        let p = self.pstyle.get(i)?;
        let PK::Image(_, w, h) = &p.kind else { return None };
        let s = self.scale;
        let pos = |cp: i32| { let mut q = POINT::default(); SendMessageW(self.edit, EM_POSFROMCHAR, WPARAM(&mut q as *mut _ as usize), LPARAM(cp as isize)); q };
        let (pa, pb) = (pos(p.a), pos(p.b));
        let top = pb.y + (p.lh * s).round() as i32 + (3.0 * ts() * s).round() as i32;
        let (mut pw, mut ph) = ((w * s).round() as i32, (h * s).round() as i32);
        let room = rc.right - pa.x;
        if pw > room && room > 0 { ph = ph * room / pw; pw = room; }
        Some((pa.x, top, pw, ph))
    }
    /// In Simple mode a shown picture's link line is hidden and only a hairline tall, so the caret
    /// never rests there: a caret that lands on it goes on to the line above or below, the way it was moving.
    fn past_picture(&self, cp: i32) -> Option<i32> {
        let i = self.para_at(cp).filter(|&i| i > 0 && matches!(self.pstyle[i].kind, PK::Image(..)))?;
        let p = &self.pstyle[i];
        let (was0, was1) = (self.seen.0, self.seen.1);
        let up = if was0 >= p.a && was1 <= p.b { cp == p.a } else { was0 > p.b };
        let last = p.b as usize >= self.styled_text.len();
        Some(if (up || last) && p.a > 0 { p.a - 1 } else { p.b + 1 })
    }
    /// the paragraph whose picture is under a point of the editor
    unsafe fn picture_at(&self, x: i32, y: i32) -> Option<usize> {
        let mut rc = RECT::default();
        let _ = GetClientRect(self.edit, &mut rc);
        self.visible_paragraphs(rc).find(|&i| self.picture_box(i, rc).is_some_and(|(l, t, w, h)| x >= l && x < l + w && y >= t && y < t + h))
    }
    /// Backspace after a picture or Delete before it selects the picture; with the picture selected
    /// either key removes its line. Joining text onto a picture's line would turn the picture back into text.
    unsafe fn picture_key(&mut self, vk: u16) -> bool {
        let back = vk == VK_BACK.0;
        if (!back && vk != VK_DELETE.0) || GetKeyState(VK_CONTROL.0 as i32) < 0 { return false; }
        let (s0, s1) = self.get_sel();
        let Some(i) = self.para_at(s0) else { return false };
        let is_pic = |j: usize| j > 0 && self.pstyle.get(j).is_some_and(|p| matches!(p.block, Block::Image(_)));
        let p = &self.pstyle[i];
        if s0 != s1 {
            if !(is_pic(i) && s0 == p.a && s1 == p.b) { return false; }
            let len = self.styled_text.len() as i32;
            let (a, b) = if p.b < len { (p.a, p.b + 1) } else { (p.a - 1, p.b) };
            self.select(a, b);
            let empty = [0u16];
            SendMessageW(self.edit, EM_REPLACESEL, WPARAM(1), LPARAM(empty.as_ptr() as isize));
            return true;
        }
        // the key would erase into a picture's line: from the line after it (empty or not), from the
        // line before it, or from the edge of the picture's own line
        let (toward, away) = if back { (p.a, p.b) } else { (p.b, p.a) };
        let neighbour = if back { i.wrapping_sub(1) } else { i + 1 };
        let candidates: &[usize] = if s0 == toward { &[neighbour, i] } else if s0 == away { &[i] } else { &[] };
        let Some(j) = candidates.iter().copied().find(|&j| is_pic(j)) else { return false };
        let (a, b) = (self.pstyle[j].a, self.pstyle[j].b);
        self.select(a, b);
        let _ = InvalidateRect(self.edit, None, false);
        true
    }
    /// what a press at a point of the note hits: a task box (its position) or, with Ctrl, a link or picture
    unsafe fn edit_target(&self, x: i32, y: i32, ctrl: bool) -> Option<std::result::Result<i32, String>> {
        let s = self.scale;
        let pos = |cp: i32| { let mut p = POINT::default(); SendMessageW(self.edit, EM_POSFROMCHAR, WPARAM(&mut p as *mut _ as usize), LPARAM(cp as isize)); p };
        if ctrl {
            if let Some(PK::Image(path, ..)) = self.picture_at(x, y).map(|i| &self.pstyle[i].kind) { return Some(Err(path.to_string_lossy().to_string())); }
        }
        let pl = POINTL { x, y };
        let cp = SendMessageW(self.edit, EM_CHARFROMPOS, WPARAM(0), LPARAM(&pl as *const _ as isize)).0 as i32;
        let i = self.para_at(cp)?;
        let p = &self.pstyle[i];
        if p.task.is_some() {
            let (p0, p1) = (pos(p.a), pos(p.a + 1));
            let pad = (3.0 * s) as i32;
            if x >= p0.x - pad && x <= p1.x + pad && y >= p0.y && y < p0.y + (p.lh * s) as i32 { return Some(Ok(p.a)); }
        }
        if !ctrl { return None; }
        let (_, cy) = { let q = pos(cp); (q.x, q.y) };
        if y < cy || y > cy + (p.lh * s) as i32 { return None; }
        self.links.iter().find(|l| cp >= l.0 && cp < l.1).map(|l| Err(l.2.clone()))
    }
    unsafe fn open_link(&self, url: &str) {
        let lower = url.to_ascii_lowercase();
        let target = if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") { url.to_string() }
            else if lower.starts_with("www.") { format!("https://{url}") }
            else if std::path::Path::new(url).is_absolute() && images::is_image(std::path::Path::new(url)) { url.to_string() }
            else if let Some(p) = self.image_file(url) { p.to_string_lossy().to_string() }
            else { return };
        let w = wide(&target);
        ShellExecuteW(self.hwnd, w!("open"), PCWSTR(w.as_ptr()), None, None, SW_SHOWNORMAL);
    }
    /// a press in the note: ticks a task box, Ctrl opens a link or a picture. true when handled here
    unsafe fn edit_click(&mut self, lp: LPARAM) -> bool {
        let (x, y) = ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32);
        let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
        match self.edit_target(x, y, ctrl) {
            Some(Ok(at)) => { let _ = SetFocus(self.edit); self.toggle_task(at); true }
            Some(Err(url)) => { self.open_link(&url); true }
            None => match self.picture_at(x, y).filter(|_| !ctrl) {
                // a click on a picture selects it, so Delete or Backspace removes it
                Some(i) => {
                    let _ = SetFocus(self.edit);
                    let (a, b) = (self.pstyle[i].a, self.pstyle[i].b);
                    self.select(a, b);
                    let _ = InvalidateRect(self.edit, None, false);
                    true
                }
                None => false,
            },
        }
    }
    unsafe fn edit_hand(&self) -> bool {
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = ScreenToClient(self.edit, &mut p);
        self.edit_target(p.x, p.y, GetKeyState(VK_CONTROL.0 as i32) < 0).is_some()
    }
    /// Vector checkboxes for the animated folder preview.
    unsafe fn draw_task_box(&self, rt: &ID2D1RenderTarget, bounds: D2D_RECT_F, done: bool, scale: f32, opacity: f32) {
        let (x, y, size) = (bounds.left, bounds.top, bounds.right - bounds.left);
        let color = if done { tab_dark() } else { ink3() };
        let Ok(brush) = rt.CreateSolidColorBrush(&rgb(color, opacity), None) else { return };
        if done {
            rt.FillRoundedRectangle(&rrect(x, y, x + size, y + size, checkbox::CORNER * scale), &brush);
            brush.SetColor(&rgb(on_dark(), opacity));
            let q = |(fx, fy)| pt(x + size * fx, y + size * fy);
            for segment in checkbox::TICK.windows(2) {
                rt.DrawLine(q(segment[0]), q(segment[1]), &brush, checkbox::TICK_WIDTH * scale, &self.round);
            }
        } else {
            let stroke = checkbox::OUTLINE_WIDTH * scale;
            let inset = stroke / 2.0;
            let box_ = rrect(x + inset, y + inset, x + size - inset, y + size - inset, checkbox::OUTLINE_CORNER * scale);
            rt.DrawRoundedRectangle(&box_, &brush, stroke, None);
        }
    }

    /// Include a neighbouring paragraph at each edge for decorations that extend into its gap.
    unsafe fn visible_paragraphs(&self, rc: RECT) -> std::ops::Range<usize> {
        if self.pstyle.is_empty() || rc.right <= 0 || rc.bottom <= 0 { return 0..0; }
        let at = |x, y| {
            let point = POINTL { x, y };
            SendMessageW(self.edit, EM_CHARFROMPOS, WPARAM(0), LPARAM(&point as *const _ as isize)).0 as i32
        };
        let a = self.para_at(at(0, 0)).unwrap_or(0);
        let b = self.para_at(at(rc.right - 1, rc.bottom - 1)).unwrap_or(self.pstyle.len() - 1);
        a.min(b).saturating_sub(1)..(a.max(b) + 2).min(self.pstyle.len())
    }

    /// Task boxes are drawn over the invisible box character.
    unsafe fn draw_tasks(&mut self, dc: HDC, rc: RECT, visible: std::ops::Range<usize>) {
        let s = self.scale;
        let size = (TASK_BOX * ts() * s).round() as i32;
        let colors = [ink3(), tab_dark(), on_dark()];
        for p in &self.pstyle[visible] {
            let Some(done) = p.task else { continue };
            let mut at = POINT::default();
            SendMessageW(self.edit, EM_POSFROMCHAR, WPARAM(&mut at as *mut _ as usize), LPARAM(p.a as isize));
            let hl = self.hl_at(p.a);
            // centred on the lowercase letters, which sit a little below the middle of the glyph box
            let cy = at.y as f32 + ((hl.0 + hl.1) / 2.0 + 2.0 * ts()) * s;
            let y = (cy - size as f32 / 2.0).round() as i32;
            if y + size <= 0 || y >= rc.bottom { continue; }
            if !self.checkboxes.as_ref().is_some_and(|b| b.matches(size, s, colors)) {
                self.checkboxes = checkbox::Checkboxes::new(size, s, colors);
            }
            if let Some(b) = &self.checkboxes { b.draw(dc, at.x, y, done); }
        }
    }
    /// Draw the decorations into the same surface as the text.
    unsafe fn draw_marks_on(&mut self, dc: HDC) {
        let edit = self.edit;
        let s = self.scale;
        let mut rc = RECT::default();
        let _ = GetClientRect(edit, &mut rc);
        let pos = |cp: i32| { let mut p = POINT::default(); SendMessageW(edit, EM_POSFROMCHAR, WPARAM(&mut p as *mut _ as usize), LPARAM(cp as isize)); p };
        let ink1 = self.car_dc;
        let tint = |l: i32, t: i32, r: i32, b: i32, k: f32| {
            let (l, t, r, b) = (l.max(0), t.max(0), r.min(rc.right), b.min(rc.bottom));
            if r <= l || b <= t { return; }
            let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: (k * 255.0) as u8, AlphaFormat: 0 };
            let _ = AlphaBlend(dc, l, t, r - l, b - t, ink1, 0, 0, 1, 1, blend);
        };
        let d = |v: f32| (v * ts() * s).round() as i32;
        let visible = self.visible_paragraphs(rc);
        if visible.is_empty() { return; }
        let n = visible.end;
        let mut i = visible.start;
        while i < n {
            let quote = matches!(self.pstyle[i].kind, PK::Quote);
            if !(quote || matches!(self.pstyle[i].kind, PK::Code)) { i += 1; continue; }
            let mut j = i;
            while j + 1 < n && std::mem::discriminant(&self.pstyle[j + 1].kind) == std::mem::discriminant(&self.pstyle[i].kind) { j += 1; }
            let (first, last) = (&self.pstyle[i], &self.pstyle[j]);
            let (p0, p1) = (pos(first.a), pos(last.b));
            let (top, bottom) = (p0.y, p1.y + (last.lh * s).round() as i32);
            if quote { let x = p0.x - d(14.0); tint(x, top + d(2.0), x + d(3.0).max(2), bottom - d(2.0), 0.3); }
            else { tint(p0.x - d(8.0), top - d(3.0), rc.right - d(2.0), bottom + d(3.0), if night() { 0.1 } else { 0.06 }); }
            i = j + 1;
        }
        let first = self.pstyle[visible.start].a;
        let last = self.pstyle[visible.end - 1].b;
        let inline = self.codes.partition_point(|&(_, b)| b <= first);
        for &(a, b) in self.codes[inline..].iter().take_while(|&&(a, _)| a <= last) {
            let hl = self.hl_at(a);
            let mut k = a;
            while k < b {
                let p = pos(k);
                let mut j = b;
                if pos(b - 1).y != p.y {
                    let mut lo = k + 1;
                    while lo < j {
                        let mid = lo + (j - lo) / 2;
                        if pos(mid).y == p.y { lo = mid + 1; } else { j = mid; }
                    }
                }
                let pe = pos(j);
                let x1 = if pe.y == p.y { pe.x } else { pos(j - 1).x + d(8.0) };
                tint(p.x - d(2.0), p.y + (hl.0 * s) as i32, x1 + d(2.0), p.y + (hl.1 * s) as i32, if night() { 0.12 } else { 0.07 });
                k = j;
            }
        }
        let (s0, s1) = self.get_sel();
        for i in visible.clone() {
            let Some((x, y, w, h)) = self.picture_box(i, rc) else { continue };
            if y >= rc.bottom || y + h <= 0 { continue; }
            let p = &self.pstyle[i];
            // a selected picture (its whole line) shows faded, the way a selection covers text
            let selected = s0 < s1 && s0 <= p.a && s1 >= p.b;
            let PK::Image(f, ..) = &p.kind else { continue };
            let f = f.clone();
            if selected {
                // RichEdit's selection covers the whole line box, picture gap included: only the link line keeps it
                let (link, below) = (pos(p.b), if (p.b as usize) < self.styled_text.len() { pos(p.b + 1).y } else { rc.bottom });
                let gap = RECT { left: x.min(link.x), top: link.y + (p.lh * s).round() as i32, right: (x + w).max(link.x + d(4.0)), bottom: below };
                let paper = CreateSolidBrush(COLORREF(bgr(self.shown_color(&self.cur))));
                FillRect(dc, &gap, paper);
                let _ = DeleteObject(paper);
            }
            self.imgs.draw(dc, &f, x, y, w, h, (PIC_RADIUS * s).round() as i32, if selected { 120 } else { 255 });
        }
        self.draw_tasks(dc, rc, visible);
    }

    /// Paint the RichEdit surface once so it can move with the folder in either direction.
    unsafe fn capture_edit(&mut self) -> Result<EditSnapshot> {
        let target = self.ensure_target()?;
        let mut rc = RECT::default();
        GetClientRect(self.edit, &mut rc)?;
        let (w, h) = (rc.right, rc.bottom);
        if w <= 0 || h <= 0 { return Err(Error::from(E_FAIL)); }
        let mut origin = [POINT::default()];
        MapWindowPoints(self.edit, self.hwnd, &mut origin);
        // Folder drawing has a 1 DIP inset in addition to its animation transform.
        let s = self.scale;
        let (x, y) = (origin[0].x as f32 / s - 1.0, origin[0].y as f32 / s - 1.0);
        let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default()
        }, ..Default::default() };
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() { return Err(Error::from_win32()); }
        let mut bits = null_mut();
        let result = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0).and_then(|dib| {
            let old = SelectObject(dc, dib);
            // GDI leaves the alpha channel undefined. The editor surface is opaque paper.
            std::slice::from_raw_parts_mut(bits.cast::<u32>(), w as usize * h as usize).fill(self.shown_color(&self.cur));
            SendMessageW(self.edit, WM_PRINTCLIENT, WPARAM(dc.0 as usize), LPARAM((PRF_CLIENT | PRF_ERASEBKGND) as isize));
            self.draw_marks_on(dc);
            let _ = GdiFlush();
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0 * s, dpiY: 96.0 * s,
            };
            let bitmap = target.CreateBitmap(D2D_SIZE_U { width: w as u32, height: h as u32 }, Some(bits), w as u32 * 4, &props);
            SelectObject(dc, old);
            let _ = DeleteObject(dib);
            bitmap
        });
        let _ = DeleteDC(dc);
        Ok(EditSnapshot { bitmap: result?, bounds: rect(x, y, x + w as f32 / s, y + h as f32 / s), scroll: self.edit_scroll(), start: Instant::now() })
    }

    /// Use the full editor image on both sides of the hide/show animation.
    unsafe fn capture_suspended_edit(&mut self, scroll: POINT) {
        self.set_edit_scroll(scroll);
        self.edit_snapshot = self.capture_edit().ok().map(|mut snapshot| {
            snapshot.start = Instant::now() - EditSnapshot::FADE;
            snapshot
        });
    }

    // ---------- find highlights: a layered child above RichEdit ----------
    // RichEdit paints a background colour over the whole line box (paragraph gap included), which looks like tall bars.
    unsafe fn pos_of(&self, i: i32) -> (f32, f32) {
        let mut p = POINT::default();
        SendMessageW(self.edit, EM_POSFROMCHAR, WPARAM(&mut p as *mut _ as usize), LPARAM(i as isize));
        (p.x as f32 / self.scale, p.y as f32 / self.scale)
    }
    /// one box per visual line a range spans: (title, from, to, x0, line top, x1)
    unsafe fn line_boxes(&self, text: &[u16], a: i32, b: i32, out: &mut Vec<(bool, i32, i32, f32, f32, f32)>) {
        let mut i = a;
        while i < b {
            let title = i < self.title_end;
            let (x0, y0) = self.pos_of(i);
            let mut j = i + 1;
            while j < b && (self.pos_of(j).1 - y0).abs() < 1.0 { j += 1; }
            let (x1, y1) = self.pos_of(j);
            let fmt = if title { &self.fonts.head } else { &self.fonts.body };
            let x1 = if (y1 - y0).abs() < 1.0 && x1 > x0 { x1 } else { x0 + self.text_w(&text[(j - 1) as usize..j as usize], fmt) };
            out.push((title, i, j, x0, y0, x1));
            i = j;
        }
    }
    /// find matches, drawn on a layered child above RichEdit (only while finding)
    unsafe fn update_hits(&mut self) {
        if !(self.finding && self.edit_visible && !self.hits.is_empty()) { self.hits_layer.hide(); return; }
        let mut rc = RECT::default();
        let _ = GetClientRect(self.edit, &mut rc);
        let mut wr = RECT::default();
        let _ = GetWindowRect(self.edit, &mut wr);
        let mut p0 = POINT { x: wr.left, y: wr.top };
        let _ = ScreenToClient(self.hwnd, &mut p0);
        let text = &self.styled_text;
        let h = rc.bottom as f32 / self.scale;
        let visible = self.visible_paragraphs(rc);
        if visible.is_empty() { self.hits_layer.hide(); return; }
        let first = self.pstyle[visible.start].a;
        let last = self.pstyle[visible.end - 1].b;
        let mut boxes = Vec::new();
        let mut tmp = Vec::new();
        let start = self.hits.partition_point(|&(_, b)| b <= first);
        for (k, &(a, b)) in self.hits.iter().enumerate().skip(start).take_while(|(_, (a, _))| *a <= last) {
            if a < 0 || b as usize > text.len() { continue; }
            tmp.clear();
            self.line_boxes(text, a, b, &mut tmp);
            boxes.extend(tmp.iter().map(|&(t, i, j, x0, y0, x1)| (k == self.hit, t, i, j, x0, y0, x1, self.hl_at(i))));
        }
        boxes.retain(|b| b.5 > -40.0 && b.5 < h);
        let fonts = (self.fonts.head.clone(), self.fonts.body.clone());
        self.hits_layer.present(p0.x, p0.y, rc.right, rc.bottom, 1.0, |rt| {
            let soft = rt.CreateSolidColorBrush(&rgb(ink(), 0.13), None);
            let dark = rt.CreateSolidColorBrush(&rgb(tab_dark(), 1.0), None);
            let on = rt.CreateSolidColorBrush(&rgb(on_dark(), 1.0), None);
            let (Ok(soft), Ok(dark), Ok(on)) = (soft, dark, on) else { return };
            for &(cur, title, i, j, x0, y0, x1, hl) in &boxes {
                let r = rrect(x0 - 1.5, y0 + hl.0, x1 + 1.5, y0 + hl.1, 3.0);
                if cur {
                    rt.FillRoundedRectangle(&r, &dark);
                    let fmt = if title { &fonts.0 } else { &fonts.1 };
                    rt.DrawText(&text[i as usize..j as usize], fmt, &rect(x0, y0 + hl.2, x1 + 40.0, y0 + 80.0), &on, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                } else { rt.FillRoundedRectangle(&r, &soft); }
            }
        });
    }

    // ---------- caret: a click-through sibling of RichEdit, outside its scrolling surface ----------
    /// Called after input. RichEdit owns the selection, including drag autoscroll.
    unsafe fn sync_edit(&mut self) {
        let focus = self.edit_visible && !self.hiding && GetFocus() == self.edit;
        let (s0, s1) = self.get_sel();
        if focus && s0 == s1 && self.prefs.simple_editor {
            if let Some(to) = self.past_picture(s0) {
                self.seen.0 = s0;
                self.seen.1 = s1;
                self.select(to, to);
                return self.sync_edit();
            }
        }
        let mut cp = POINT::default();
        let mut sp = POINT::default();
        if focus {
            let _ = GetCaretPos(&mut cp);
            sp = self.edit_scroll();
            let _ = HideCaret(self.edit); // RichEdit's own caret never shows; hiding is cumulative, so extra calls are harmless
        }
        let key = (s0, s1, cp.x, cp.y, sp.y, focus);
        if key == self.seen { return; }
        self.seen = key;
        if !focus || s0 != s1 { self.caret_off(); return; }
        let hl = self.hl_at(s0);
        let s = self.scale;
        let tx = cp.x as f32;
        let th = (hl.1 - hl.0 - 1.0) * s;
        let mut ty = cp.y as f32 + (hl.0 + 0.5) * s;
        if self.para_at(s0).is_some_and(|i| matches!(self.pstyle[i].block, Block::Heading(_))) {
            if let Some(selection) = self.doc.as_ref().and_then(|d| d.GetSelection().ok()) {
                // The text baseline includes paragraph spacing. Keep the native caret's
                // visual line if TOM points to the other side of a wrap.
                let mut x = i32::MIN;
                let mut baseline = i32::MIN;
                let flags = tomConstants(tomStart.0 | tomClientCoord.0 | TA_BASELINE.0 as i32);
                if selection.GetPoint(flags, &mut x, &mut baseline).is_ok()
                    && baseline != i32::MIN && (x.abs_diff(cp.x) as f32) <= 2.0 * s {
                    ty = baseline as f32 + 2.0 * ts() * s - th;
                }
            }
        }
        let c = &mut self.car;
        c.tx = tx; c.ty = ty; c.th = th;
        // appear in place; scrolling moves with the text instead of gliding
        c.x = tx; c.y = ty; c.h = th;
        c.on = true;
        c.t = 0.0;
        c.last = Some(Instant::now());
        self.tick_caret(0.0);
    }
    unsafe fn caret_off(&mut self) {
        if !self.car.on { return; }
        self.car.on = false;
        let _ = KillTimer(self.hwnd, TIMER_ECARET);
        self.caret_layer.hide();
        self.car_rect = RECT::default();
        self.car_a = 0;
    }
    /// the caret's box in RichEdit client px
    fn caret_box(&self) -> RECT {
        let c = &self.car;
        let w = (2.0 * self.scale).round().max(1.0) as i32;
        let x = c.x.round() as i32 - w / 2;
        RECT { left: x, top: c.y.round() as i32, right: x + w, bottom: (c.y + c.h).round() as i32 }
    }
    unsafe fn tick_caret(&mut self, dt: f32) {
        if !self.car.on { return; }
        let c = &mut self.car;
        let k = 1.0 - (-dt * 30.0).exp();
        c.x += (c.tx - c.x) * k; c.y += (c.ty - c.y) * k; c.h += (c.th - c.h) * k;
        let moving = (c.tx - c.x).abs() > 0.4 || (c.ty - c.y).abs() > 0.4 || (c.th - c.h).abs() > 0.4;
        if !moving { c.x = c.tx; c.y = c.ty; c.h = c.th; }
        c.t += dt;
        // solid for half a second after any move, then a soft blink: on .45s, fade .15s, off .3s, fade .15s
        let smooth = |v: f32| { let v = v.clamp(0.0, 1.0); v * v * (3.0 - 2.0 * v) };
        let (alpha, next) = if moving || c.t < 0.5 { (1.0, 0.5 - c.t) } else {
            let p = (c.t - 0.5) % 1.05;
            if p < 0.45 { (1.0, 0.45 - p) } else if p < 0.6 { (1.0 - smooth((p - 0.45) / 0.15), 0.0) }
            else if p < 0.9 { (0.0, 0.9 - p) } else { (smooth((p - 0.9) / 0.15), 0.0) }
        };
        c.alpha = alpha;
        let interval = if moving || next < 0.02 { 16 } else { (next * 1000.0) as u32 + 1 };
        let mut r = self.caret_box();
        let mut editor = RECT::default();
        let _ = GetClientRect(self.edit, &mut editor);
        r.left = r.left.max(editor.left);
        r.top = r.top.max(editor.top);
        r.right = r.right.min(editor.right);
        r.bottom = r.bottom.min(editor.bottom);
        let a8 = if r.left < r.right && r.top < r.bottom { (alpha * 255.0) as u8 } else { 0 };
        let mut origin = [POINT::default()];
        MapWindowPoints(self.edit, self.hwnd, &mut origin);
        let _ = OffsetRect(&mut r, origin[0].x, origin[0].y);
        if r != self.car_rect || a8 != self.car_a {
            self.car_rect = r;
            self.car_a = a8;
            if a8 == 0 { self.caret_layer.hide(); }
            else {
                self.caret_layer.present(r.left, r.top, r.right - r.left, r.bottom - r.top, a8 as f32 / 255.0, |rt| {
                    rt.Clear(Some(&rgb(ink(), 1.0)));
                });
            }
        }
        SetTimer(self.hwnd, TIMER_ECARET, interval, None);
    }

    // ---------- palette: an owned popup so it can lie over RichEdit ----------
    unsafe fn open_palette(&mut self, id: String, ax: f32, ay: f32) {
        self.close_edit_menu();
        self.palette = Some(id);
        self.pal_anchor = (ax, ay);
        self.pal_t = Ease::at(0.0);
        self.pal_t.to = 1.0;
        self.pal_hover = None;
        self.pal_delete_pressed = false;
        self.sw_sc = [Ease::at(1.0); MAX_CARDS];
        self.draw_palette();
        self.start_anim();
    }
    unsafe fn close_palette(&mut self) {
        self.pal_delete_pressed = false;
        if self.palette.take().is_some() { self.pop.hide(); self.invalidate(); }
    }
    fn pal_dims(&self) -> (f32, f32) {
        if self.mode == Mode::List { (pal_w().max(184.0), PAL_H + PAL_DELETE_H) } else { (pal_w(), PAL_H) }
    }
    unsafe fn draw_palette(&mut self) {
        let Some(id) = self.palette.clone() else { return };
        let chosen = self.note(&id).map(|n| n.color).unwrap_or(0);
        let (pw, ph) = self.pal_dims();
        let left = (self.pal_anchor.0 - pw / 2.0).clamp(0.0, width() - pw);
        let mut top = self.pal_anchor.1;
        let listing = self.mode == Mode::List;
        if listing {
            let work = self.work_area();
            let mut origin = POINT::default();
            let _ = ClientToScreen(self.hwnd, &mut origin);
            let min = (work.top - origin.y) as f32 / self.scale + 6.0;
            let max = (work.bottom - origin.y) as f32 / self.scale - ph - 1.0;
            top = top.clamp(min, max.max(min));
        }
        let mut p = POINT { x: ((left + 1.0 - PAL_ML) * self.scale).round() as i32, y: ((top + 1.0 - PAL_MT) * self.scale).round() as i32 };
        let _ = ClientToScreen(self.hwnd, &mut p);
        let (w, h) = (((pw + 2.0 * PAL_ML) * self.scale).ceil() as i32, ((ph + PAL_MT + PAL_MB) * self.scale).ceil() as i32);
        let t = self.pal_t.v;
        let dy = -6.0 * (1.0 - t);
        let sc = self.sw_sc;
        let delete_hover = self.pal_hover == Some(PalAction::Delete);
        let (trash, font, round) = (&self.icons.trash, &self.fonts.small, &self.round);
        self.pop.present(p.x, p.y, w, h, t, |rt| {
            let (x0, y0) = (PAL_ML, PAL_MT + dy);
            let _ = soft_shadow(rt, rect(x0, y0, x0 + pw, y0 + ph), 14.0, 12.0, 24.0, 0.16);
            let (Ok(paper), Ok(line)) = (rt.CreateSolidColorBrush(&rgb(paper(), 1.0), None), rt.CreateSolidColorBrush(&rgb(line_c(), 1.0), None)) else { return };
            rt.FillRoundedRectangle(&rrect(x0, y0, x0 + pw, y0 + ph, 14.0), &paper);
            rt.DrawRoundedRectangle(&rrect(x0 + 0.75, y0 + 0.75, x0 + pw - 0.75, y0 + ph - 0.75, 13.25), &line, 1.5, None);
            for i in 0..ncards() {
                let (cx, cy) = (x0 + (pw - pal_w()) / 2.0 + 22.0 + 36.0 * i as f32, y0 + 22.0);
                let k = sc[i].v;
                if let Ok(b) = rt.CreateSolidColorBrush(&rgb(note_rgb(i), 1.0), None) { rt.FillEllipse(&circle(cx, cy, 10.0 * k), &b); }
                rt.DrawEllipse(&circle(cx, cy, 9.25 * k), &line, 1.5, None);
                if i == chosen { rt.DrawEllipse(&circle(cx, cy, 15.25), &line, 1.5, None); }
            }
            if listing {
                let Ok(b) = rt.CreateSolidColorBrush(&rgb(ink(), 0.14), None) else { return };
                let y = y0 + PAL_H;
                rt.DrawLine(pt(x0 + 12.0, y), pt(x0 + pw - 12.0, y), &b, 1.0, None);
                if delete_hover {
                    b.SetColor(&rgb(ink(), 0.09));
                    rt.FillRoundedRectangle(&rrect(x0 + 5.0, y + 4.0, x0 + pw - 5.0, y0 + ph - 5.0, 8.0), &b);
                }
                b.SetColor(&rgb(ink(), 1.0));
                rt.SetTransform(&shift(x0 + 14.0, y + (PAL_DELETE_H - 16.0) / 2.0));
                rt.DrawGeometry(trash, &b, 1.4, round);
                rt.SetTransform(&shift(0.0, 0.0));
                rt.DrawText(&u(ui::DELETE_NOTE.get()), font, &rect(x0 + 39.0, y, x0 + pw - 10.0, y0 + ph),
                    &b, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            }
        });
    }
    /// Palette coordinates use the same offset as the opening animation.
    fn pal_hit(&self, lp: LPARAM) -> (Option<PalAction>, bool) {
        let x = (lp.0 & 0xffff) as i16 as f32 / self.scale - PAL_ML;
        let y = ((lp.0 >> 16) & 0xffff) as i16 as f32 / self.scale - PAL_MT + 6.0 * (1.0 - self.pal_t.v);
        let (w, h) = self.pal_dims();
        let inside = (0.0..w).contains(&x) && (0.0..h).contains(&y);
        if self.mode == Mode::List && (5.0..w - 5.0).contains(&x) && (PAL_H + 4.0..h - 5.0).contains(&y) {
            return (Some(PalAction::Delete), true);
        }
        let color = (0..ncards()).find(|&i| {
            let cx = (w - pal_w()) / 2.0 + 22.0 + 36.0 * i as f32;
            (x - cx).hypot(y - 22.0) < 16.0
        });
        (color.map(PalAction::Color), inside)
    }

    // ---------- layout targets ----------
    fn order(&self) -> Vec<String> {
        let mut v: Vec<&Note> = self.notes.iter().collect();
        v.sort_by(|a, b| b.seen().cmp(&a.seen()));
        v.into_iter().map(|n| n.id.clone()).collect()
    }
    unsafe fn work_area(&self) -> RECT {
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
        mi.rcWork
    }
    unsafe fn retarget(&mut self) { self.retarget_with(false); }
    unsafe fn retarget_with(&mut self, stagger: bool) {
        self.shown.clear();
        if self.mode == Mode::List {
            let query = String::from_utf16_lossy(&self.list_q).to_lowercase();
            let q = query.trim();
            let mut matches: Vec<_> = self.notes.iter().filter(|n| q.is_empty() || n.lower.contains(q)).collect();
            matches.sort_by_key(|n| std::cmp::Reverse(n.seen()));
            self.shown.extend(matches.into_iter().map(|n| n.id.clone()));
        }
        self.layout_stack(stagger, 0);
    }
    unsafe fn layout_stack(&mut self, stagger: bool, scroll: isize) {
        let work = self.work_area();
        let work_h = (work.bottom - work.top) as f32 / self.scale;
        let base_h = HF + LIST_TOP + LIST_BOTTOM;
        let min_step = T + 6.0;
        let capacity = (((work_h - base_h) / min_step) as usize).clamp(1, LIST_LIMIT);
        let old_start = self.list_start;
        let count = self.shown.len();
        self.list_len = count.min(capacity);
        self.sel = self.sel.min(count.saturating_sub(1));
        let max_start = count.saturating_sub(self.list_len);
        self.list_start = if scroll != 0 {
            self.list_start.saturating_add_signed(scroll).min(max_start)
        } else {
            self.list_start.min(max_start).clamp(self.sel.saturating_add(1).saturating_sub(capacity), self.sel)
        };
        if self.list_len != 0 {
            self.sel = self.sel.clamp(self.list_start, self.list_start + self.list_len - 1);
        }
        let visible = &self.shown[self.list_start..self.list_start + self.list_len];
        let direction = (self.list_start as isize - old_start as isize).signum() as f32;
        let m = visible.len() as f32;
        let max_h = (work_h * 0.86).max(480.0).min(work_h);
        let step = ((max_h - base_h) / m.max(1.0)).clamp(min_step, 50.0);
        let list_h = m * step + base_h;
        self.win_h_to = if self.mode == Mode::List { list_h } else { eh() + PADB };
        self.win_y_to = if self.mode == Mode::List {
            self.edit_top.min(work.bottom - (list_h * self.scale).round() as i32).max(work.top) as f32
        } else { self.edit_top as f32 };
        if self.drag0.is_some() { self.win_y_to = self.win_y; }
        let listing = self.mode == Mode::List;
        let window_changed = listing && (visible.iter().any(|id| self.anims.get(id).is_none_or(|a| a.to.o == 0.0))
            || self.anims.iter().any(|(id, a)| id != SEARCH && id != SETTINGS && a.to.o > 0.0 && !visible.contains(id)));
        if window_changed {
            // Keep just the current and previous window, even when scroll events arrive mid-fade.
            self.anims.retain(|id, a| id == SEARCH || id == SETTINGS || a.to.o > 0.0 || visible.contains(id));
        }
        let hidden = V { y: 18.0, sc: LIST_SC, h: eh(), ..Default::default() };
        for (id, a) in &mut self.anims {
            if id == SEARCH || id == SETTINGS || visible.contains(id) { continue; }
            if !listing && id == &self.cur && !self.settings { continue; }
            if listing && a.to.o == 0.0 { continue; }
            a.to = if listing {
                V { y: (a.cur.y + step * direction).max(LIST_TOP), lift: 0.0, o: 0.0, ..a.cur }
            } else { hidden };
            a.delay = 0.0;
            if listing && direction <= 0.0 { a.z = -1; }
        }
        for (i, id) in visible.iter().enumerate() {
            let sf = i as f32;
            let picked = self.list_start + i == self.sel;
            let (rot, sx) = if picked { tilt(id) } else { (0.0, 0.0) };
            let to = V { y: (m - sf) * step + LIST_TOP, sc: LIST_SC * PERSP / (PERSP + sf * DEPTH), h: HF, o: 1.0, lift: if picked { 14.0 } else { 0.0 }, rot, sx };
            let from = if direction != 0.0 {
                V { y: (to.y - step * direction).max(LIST_TOP + 8.0), o: 0.0, ..to }
            } else { hidden };
            let a = self.anims.entry(id.clone()).or_insert(Anim { cur: from, ..Default::default() });
            a.to = to;
            a.z = 100 - i as i32;
            if stagger { a.delay = (m - sf) * 0.018; }
            else if window_changed { a.delay = 0.0; }
        }
        if !listing && !self.settings {
            let a = self.anims.entry(self.cur.clone()).or_insert(Anim { cur: hidden, ..Default::default() });
            a.to = V { y: 0.0, sc: 1.0, h: eh(), o: 1.0, ..Default::default() };
            a.z = 1000;
            a.delay = 0.0;
        }
        let s = self.anims.entry(SEARCH.to_string()).or_insert_with(|| Anim { cur: V { y: 18.0, sc: 0.955, h: eh(), ..Default::default() }, ..Default::default() });
        s.to = if self.mode == Mode::List { V { y: LIST_TOP, sc: LIST_SC * PERSP / (PERSP + m * DEPTH), h: HF, o: 1.0, ..Default::default() } } else { V { y: 18.0, sc: 0.955, h: eh(), o: 0.0, ..Default::default() } };
        s.z = 0;
        let up = self.settings && self.mode == Mode::Edit;
        let st = self.anims.entry(SETTINGS.to_string()).or_insert_with(|| Anim { cur: V { y: 18.0, sc: 0.955, h: eh(), ..Default::default() }, ..Default::default() });
        st.to = if up { V { y: 0.0, sc: 1.0, h: eh(), o: 1.0, ..Default::default() } } else { V { y: 18.0, sc: 0.955, h: eh(), o: 0.0, ..Default::default() } };
        st.z = if up { 1500 } else { -2 };
        self.trim_stack_cache();
        self.dark.to = if self.finding { DARK_FIND } else if self.hover_plus { DARK_HOVER } else { DARK };
        self.dock_radii();
        self.start_anim();
    }
    fn trim_stack_cache(&mut self) {
        self.colors.retain(|id, _| self.anims.contains_key(id));
        self.previews.get_mut().retain(|id, _| self.anims.contains_key(id));
    }
    unsafe fn scroll_list(&mut self, wp: WPARAM) {
        self.list_wheel += i32::from((wp.0 >> 16) as i16);
        let steps = -(self.list_wheel / 120) as isize;
        self.list_wheel %= 120;
        if steps == 0 || self.list_start.saturating_add_signed(steps).min(self.shown.len().saturating_sub(self.list_len)) == self.list_start { return; }
        self.close_palette();
        self.layout_stack(false, steps);
    }
    /// frames run from the message loop, one per screen refresh (see main); the timer only keeps
    /// them going inside modal loops (window drag, menus), where our loop does not run
    unsafe fn start_anim(&mut self) {
        // restarting the clock mid-animation would shorten the next step and make motion stutter
        if !self.animating { self.tick = Instant::now(); self.last_frame = self.tick; }
        self.animating = true;
        SetTimer(self.hwnd, TIMER_ANIM, 15, None);
    }
    /// one animation frame: step, draw straight away (no WM_PAINT round trip)
    unsafe fn frame(&mut self) {
        let t0 = Instant::now();
        let gap = (t0 - self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = t0;
        self.step_anim();
        let _ = self.render();
        let _ = ValidateRect(self.hwnd, None);
        if let Some(f) = &mut self.fps {
            f.0 += 1; f.1 += gap; f.2 = f.2.max(gap); f.3 += (Instant::now() - t0).as_secs_f32() * 1000.0;
            if !self.animating && f.0 > 3 {
                let line = format!("frames {} · avg gap {:.1} ms · worst {:.1} ms · draw {:.2} ms
", f.0, f.1 / f.0 as f32, f.2, f.3 / f.0 as f32);
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("pane-fps.txt")) { let _ = file.write_all(line.as_bytes()); }
                *f = (0, 0.0, 0.0, 0.0);
            }
        }
    }
    unsafe fn step_anim(&mut self) {
        let now = Instant::now();
        let dt = (now - self.tick).as_secs_f32().min(0.05);
        self.tick = now;
        let sliding = self.hiding || self.slide.0.v != self.slide.0.to || self.slide.1.v != self.slide.1.to;
        let k = 1.0 - (-dt * if sliding { self.rate() } else { 17.0 }).exp();
        let ko = 1.0 - (-dt * if sliding { self.rate() * 1.3 } else { 30.0 }).exp(); // opacity settles faster than motion, like the prototype (.18s vs .3s)
        let mut slide_done = true;
        if sliding {
            let ks = 1.0 - (-dt * self.rate()).exp();
            let (a, b) = (self.slide.0.step(ks, 0.5), self.slide.1.step(ks, 0.5));
            self.place_slide();
            slide_done = a && b;
        }
        let kc = 1.0 - (-dt * 14.0).exp(); // colour, .25s
        let kf = 1.0 - (-dt * 26.0).exp(); // small hover reactions, .15s
        let mut settled = slide_done && self.edit_snapshot.as_ref().is_none_or(|edit| edit.start.elapsed() >= EditSnapshot::FADE);
        settled &= self.ctl_a.step(if sliding { ko } else { kf }, 0.004);
        settled &= self.cap_n.step(1.0 - (-dt * 20.0).exp(), 0.004);
        settled &= self.more_t.step(kf, 0.004);
        settled &= self.ctl_fade.step(kf, 0.004);
        let kp = 1.0 - (-dt * 18.0).exp();
        for e in self.seg_anim.values_mut() { settled &= e[0].step(kp, 0.2); settled &= e[1].step(kp, 0.2); }
        for a in self.anims.values_mut() {
            if a.delay > 0.0 { a.delay -= dt; settled = false; continue; }
            a.cur.lerp(&a.to.clone(), k, ko);
            if a.cur.near(&a.to) { a.cur = a.to; } else { settled = false; }
        }
        // Only cards participating in this frame need colour updates.
        for id in self.anims.keys().filter(|id| *id != SEARCH && *id != SETTINGS) {
            let to = split(self.target_color(id));
            let c = self.colors.entry(id.clone()).or_insert(to);
            let mut moved = false;
            for i in 0..3 { if (c[i] - to[i]).abs() > 0.4 { c[i] += (to[i] - c[i]) * kc; moved = true; } else { c[i] = to[i]; } }
            if moved { settled = false; }
            let now_c = join(*c);
            if id == &self.cur && (moved || now_c == join(to)) && self.edit_visible {
                SendMessageW(self.edit, EM_SETBKGNDCOLOR, WPARAM(0), LPARAM(bgr(now_c) as isize));
            }
        }
        settled &= self.dark.step(k, 0.2);
        settled &= self.rb.step(kf, 0.1);
        settled &= self.rl.step(kf, 0.1);
        settled &= self.rr.step(kf, 0.1);
        settled &= self.tab_hl.step(kf, 0.01);
        settled &= self.dot_sc.step(kf, 0.003);
        settled &= self.plus_sc.step(kf, 0.003);
        let mut pal_moved = !self.pal_t.step(1.0 - (-dt * 24.0).exp(), 0.005);
        for s in self.sw_sc.iter_mut() { pal_moved |= !s.step(kf, 0.003); }
        if pal_moved { settled = false; if self.palette.is_some() { self.draw_palette(); } }
        let old = self.win_h;
        self.win_h += (self.win_h_to - self.win_h) * k;
        if (self.win_h - self.win_h_to).abs() < 0.5 { self.win_h = self.win_h_to; } else { settled = false; }
        let old_y = self.win_y;
        if self.drag0.is_some() { self.win_y_to = self.win_y; }
        self.win_y += (self.win_y_to - self.win_y) * k;
        if (self.win_y - self.win_y_to).abs() < 0.5 { self.win_y = self.win_y_to; } else { settled = false; }
        if (self.win_h - old).abs() > 0.01 || (self.win_y - old_y).abs() > 0.01 { self.resize_window(); }
        if self.x_glide && self.drag0.is_none() {
            self.win_x += (self.win_x_to - self.win_x) * k;
            if (self.win_x - self.win_x_to).abs() < 0.5 { self.win_x = self.win_x_to; self.x_glide = false; } else { settled = false; }
            let mut wr = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut wr);
            let _ = SetWindowPos(self.hwnd, None, self.win_x.round() as i32, wr.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.invalidate();
        if settled { let _ = KillTimer(self.hwnd, TIMER_ANIM); self.animating = false; self.on_settled(); }
    }
    unsafe fn on_settled(&mut self) {
        self.finish_unfold();
        if self.mode == Mode::List { self.edit_snapshot = None; }
        if self.hiding {
            // the window slid away: hide it and put the folder back in place for next time
            self.hiding = false;
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            self.slide = (Ease::at(0.0), Ease::at(0.0));
            self.place_slide();
            if self.mode == Mode::Edit {
                let id = self.front_id();
                if let Some(a) = self.anims.get_mut(&id) { a.to = V { y: 0.0, sc: 1.0, h: eh(), o: 1.0, ..Default::default() }; a.cur = a.to; }
            }
            if self.demo { self.demo = false; self.appear(); }
            return;
        }
        self.anims.retain(|id, a| id == SEARCH || id == SETTINGS || a.cur.o > 0.0 || a.to.o > 0.0);
        self.trim_stack_cache();
        if self.mode == Mode::Edit && !self.edit_visible && self.press_size.is_none() && !self.settings
            && IsWindowVisible(self.hwnd).as_bool() { self.reveal_edit(); }
        else { self.set_region(); }
    }
    unsafe fn reveal_edit(&mut self) {
        if !self.edit_visible {
            self.set_glass(false);
            let c = self.shown_color(&self.cur.clone());
            SendMessageW(self.edit, EM_SETBKGNDCOLOR, WPARAM(0), LPARAM(bgr(c) as isize));
            self.edit_visible = true;
            let _ = ShowWindow(self.edit, SW_SHOW);
            if !self.finding { let _ = SetFocus(self.edit); }
            let snapshot_scroll = self.edit_snapshot.take().map(|edit| edit.scroll);
            if let Some(scroll) = self.suspended_scroll.take().or(snapshot_scroll) { self.set_edit_scroll(scroll); }
            let _ = RedrawWindow(self.edit, None, None, RDW_INVALIDATE | RDW_ERASE | RDW_UPDATENOW);
            self.update_hits();
        }
        self.set_region();
    }
    unsafe fn hide_edit(&mut self) {
        self.close_edit_menu();
        if self.edit_visible {
            let _ = ShowWindow(self.edit, SW_HIDE);
            self.edit_visible = false;
            self.update_hits();
            self.caret_off();
            self.seen = (0, 0, 0, 0, 0, false);
            self.set_glass(true);
            self.set_region();
        }
    }
    unsafe fn resize_window(&mut self) {
        if self.unfold_top.is_none() {
            self.place_window(self.win_y.round() as i32, (self.win_h * self.scale).round() as i32);
        }
    }

    unsafe fn place_window(&mut self, top: i32, height: i32) {
        let mut wr = RECT::default();
        let _ = GetWindowRect(self.hwnd, &mut wr);
        let width = (ww() * self.scale).round() as i32;
        if top != wr.top || height != wr.bottom - wr.top || width != wr.right - wr.left {
            let _ = SetWindowPos(self.hwnd, None, wr.left, top, width, height, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        if let Some(t) = &self.target {
            let size = D2D_SIZE_U { width: width as u32, height: height as u32 };
            let old = t.GetPixelSize();
            if (old.width, old.height) != (size.width, size.height) { let _ = t.Resize(&size); }
        }
    }

    /// Near an edge, move the drawing within one window instead of resizing its surface each frame.
    unsafe fn begin_unfold(&mut self) {
        if self.docks & 7 == 0 || self.x_glide || self.drag0.is_some() || self.hiding
            || self.slide.0.v != 0.0 || self.slide.1.v != 0.0 { return; }
        let mut wr = RECT::default();
        if GetWindowRect(self.hwnd, &mut wr).is_err() { return; }
        let edit_top = self.win_y_to.round() as i32;
        let edit_bottom = edit_top + (self.win_h_to * self.scale).round() as i32;
        let top = wr.top.min(edit_top);
        self.unfold_top = Some(top);
        self.place_window(top, wr.bottom.max(edit_bottom) - top);
    }

    unsafe fn finish_unfold(&mut self) {
        if self.unfold_top.take().is_some() { self.resize_window(); }
    }

    fn canvas_offset(&self) -> f32 {
        self.unfold_top.map_or(0.0, |top| (self.win_y - top as f32) / self.scale)
    }
    unsafe fn set_glass(&self, full: bool) {
        let px = |v: f32| (v * self.scale).ceil() as i32;
        let m = if full { MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 } }
            else { MARGINS { cxLeftWidth: px(R + 2.0), cxRightWidth: px(R + 2.0), cyTopHeight: px(EDIT_TOP - 6.0), cyBottomHeight: px(R + 2.0 + PADB) } };
        let _ = DwmExtendFrameIntoClientArea(self.hwnd, &m);
    }
    /// clicks only land on the folder and the controls; the rest of the window lets them through.
    /// The room under the folder stays in, or its outline and shadow would be clipped.
    unsafe fn set_region(&self) {
        let px = |v: f32| (v * self.scale).round() as i32;
        if self.mode != Mode::Edit || !self.edit_visible { SetWindowRgn(self.hwnd, HRGN::default(), true); return; }
        let (x2, bx) = tab_x();
        let body = CreateRectRgn(0, px(T), px(ww()), px(eh() + PADB));
        let tab = CreateRectRgn(px(x2 - 2.0), 0, px(bx + DARK_FIND + F + 4.0), px(T) + 2);
        // the flare where the tab meets the folder reaches F to the left of the tab: without it the region cut the curve off
        let flare = CreateRectRgn(px(x2 - F - 3.0), px(T - F - 3.0), px(x2), px(T) + 2);
        CombineRgn(body, body, flare, RGN_OR);
        let _ = DeleteObject(flare);
        let ctl = CreateRectRgn(px(ctl_x(self.controls().len() - 1) - 18.0), 0, px(ww()), px(T) + 2);
        CombineRgn(body, body, tab, RGN_OR);
        CombineRgn(body, body, ctl, RGN_OR);
        let _ = DeleteObject(tab);
        let _ = DeleteObject(ctl);
        SetWindowRgn(self.hwnd, body, true);
    }
    unsafe fn invalidate(&self) { let _ = InvalidateRect(self.hwnd, None, false); }
    /// corners square off on the screen edges the editor is stuck to
    unsafe fn dock_radii(&mut self) {
        let d = if self.mode == Mode::Edit { self.docks } else { 0 };
        self.rb.to = if d & 4 != 0 { 0.0 } else { R };
        self.rl.to = if d & 1 != 0 { 0.0 } else { R };
        self.rr.to = if d & 2 != 0 { 0.0 } else { R };
        self.start_anim();
    }
    unsafe fn set_docks(&mut self, d: u8) {
        if d == self.docks { return; }
        self.docks = d;
        self.dock_radii();
    }
    fn docks_of(&self, left: i32, top: i32, work: &RECT) -> u8 {
        let s = self.scale;
        let (w, hf) = ((ww() * s).round() as i32, (eh() * s).round() as i32);
        let mut d = 0;
        if left <= work.left { d |= 1; }
        if left + w >= work.right { d |= 2; }
        if top + hf >= work.bottom - 1 { d |= 4; }
        if top <= work.top { d |= 8; }
        d
    }
    unsafe fn update_docked(&mut self) {
        let mut wr = RECT::default();
        let _ = GetWindowRect(self.hwnd, &mut wr);
        let work = self.work_area();
        let top = if self.mode == Mode::Edit && self.drag0.is_none() { self.edit_top } else { wr.top };
        let d = if self.mode == Mode::Edit { self.docks_of(wr.left, top, &work) } else { 0 };
        self.set_docks(d);
    }
    /// a release close to a screen edge slides the rest of the way onto it
    unsafe fn end_drag(&mut self) {
        let mut wr = RECT::default();
        let _ = GetWindowRect(self.hwnd, &mut wr);
        let start = self.drag0.take().map(|(_, r)| r.top).unwrap_or(wr.top);
        self.win_y = wr.top as f32;
        self.win_y_to = self.win_y;
        self.win_x = wr.left as f32;
        self.win_x_to = self.win_x;
        if self.mode != Mode::Edit {
            self.edit_top += wr.top - start;
            self.layout_stack(false, 0);
            return;
        }
        let work = self.work_area();
        let s = self.scale;
        let (w, hf) = ((ww() * s).round() as i32, (eh() * s).round() as i32);
        let near = |gap: i32| gap > 0 && (gap as f32) < GLIDE * s;
        if near(work.bottom - (wr.top + hf)) { self.win_y_to = (work.bottom - hf) as f32; }
        else if near(wr.top - work.top) { self.win_y_to = work.top as f32; }
        if near(wr.left - work.left) { self.win_x_to = work.left as f32; }
        else if near(work.right - (wr.left + w)) { self.win_x_to = (work.right - w) as f32; }
        self.x_glide = self.win_x_to != self.win_x;
        self.docks = self.docks_of(self.win_x_to as i32, self.win_y_to as i32, &work);
        self.dock_radii();
        self.edit_top = self.win_y_to as i32;
    }
    /// near a screen edge the window lags behind the cursor more and more (d*d/zone), so it settles onto the edge
    /// smoothly and lifts off only after a pull: no jump either way. Works on all four edges.
    unsafe fn on_moving(&mut self, r: &mut RECT) {
        if self.mode != Mode::Edit { return; }
        // the move loop hands us rects built from the last (possibly pinned) one, so rebuild it from the cursor
        if let Some((c0, r0)) = self.drag0 {
            let mut c = POINT::default();
            let _ = GetCursorPos(&mut c);
            let (dx, dy) = (c.x - c0.x, c.y - c0.y);
            *r = RECT { left: r0.left + dx, top: r0.top + dy, right: r0.right + dx, bottom: r0.bottom + dy };
        }
        let work = self.work_area();
        let s = self.scale;
        let (hw, w) = (r.bottom - r.top, r.right - r.left);
        let hf = (eh() * s).round() as i32;
        let zone = ZONE * s;
        let pull = |d: f32| if d <= 0.0 { 0.0 } else if d < zone { d * d / zone } else { d };
        let (dt, db) = ((r.top - work.top) as f32, (work.bottom - (r.top + hf)) as f32);
        let (dl, dr) = ((r.left - work.left) as f32, (work.right - r.right) as f32);
        let (pv, top) = if db < dt { let p = pull(db); (p, work.bottom - p.round() as i32 - hf) } else { let p = pull(dt); (p, work.top + p.round() as i32) };
        let (ph, left) = if dl < dr { let p = pull(dl); (p, work.left + p.round() as i32) } else { let p = pull(dr); (p, work.right - p.round() as i32 - w) };
        *r = RECT { left, top, right: left + w, bottom: top + hw };
        self.docks = self.docks_of(left, top, &work);
        // the corners round off as it lifts from an edge
        let k = |v: f32| R * (v / (12.0 * s)).min(1.0);
        let (kb, kl, kr) = (if db < dt { k(pv) } else { R }, if dl < dr { k(ph) } else { R }, if dl >= dr { k(ph) } else { R });
        if (self.rb.to - kb).abs() + (self.rl.to - kl).abs() + (self.rr.to - kr).abs() > 0.01 {
            self.rb.to = kb; self.rl.to = kl; self.rr.to = kr;
            self.start_anim();
        }
        self.win_y = top as f32;
        self.win_y_to = self.win_y;
        self.win_x = left as f32;
        self.win_x_to = self.win_x;
        self.close_palette();
    }

    // ---------- modes ----------
    unsafe fn enter_list(&mut self) {
        self.finish_unfold();
        self.suspended_scroll = None;
        self.save_now();
        if self.mode == Mode::Edit {
            if self.edit_visible {
                // Keep the live editor's exact text and scroll position on the moving folder.
                self.edit_snapshot = self.capture_edit().ok();
            } else if let Some(snapshot) = &mut self.edit_snapshot {
                snapshot.reverse();
            }
        }
        self.finding = false;
        self.close_palette();
        if self.mode == Mode::Edit && self.drag0.is_none() {
            let mut wr = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut wr);
            self.edit_top = wr.top;
            self.win_y = wr.top as f32;
        }
        self.settings = false;
        self.end_capture();
        self.mode = Mode::List;
        self.list_q.clear();
        self.sel = 0;
        self.list_start = 0;
        self.list_wheel = 0;
        self.hover_tab = false;
        self.tab_hl.to = 0.0;
        self.hide_edit();
        let _ = SetFocus(self.hwnd);
        SetTimer(self.hwnd, TIMER_CARET, 530, None);
        self.retarget_with(true);
    }
    unsafe fn open_note(&mut self, id: &str) {
        let from_list = self.mode == Mode::List;
        self.finish_unfold();
        self.suspended_scroll = None;
        self.save_now();
        self.undo = None;
        if id != self.cur { self.drop_empty(); }
        self.cur = id.to_string();
        self.settings = false;
        self.end_capture();
        let now = store::now_ft();
        if let Some(n) = self.note_mut(id) { n.opened = now; }
        store::write_state(&self.dir, id);
        self.close_palette();
        self.mode = Mode::Edit;
        let _ = KillTimer(self.hwnd, TIMER_CARET);
        if let Some(a) = self.anims.get_mut(id) { a.z = 1000; }
        self.hide_edit();
        self.load_editor();
        if from_list {
            // The caret is at the end. Prepare its viewport before the hidden editor is focused.
            SendMessageW(self.edit, WM_VSCROLL, WPARAM(SB_BOTTOM.0 as usize), LPARAM(0));
            self.edit_snapshot = self.capture_edit().ok();
        }
        self.update_docked();
        self.retarget();
        if from_list { self.begin_unfold(); }
    }
    unsafe fn new_note(&mut self, title: &str) {
        self.save_now();
        self.drop_empty();
        let mut n = Note::new(store::new_id(), 0, if title.is_empty() { String::new() } else { format!("{title}\n") }, store::now_ft());
        n.opened = store::now_ft();
        let id = n.id.clone();
        self.notes.push(n);
        // the new folder slides up out of the drawer
        self.anims.insert(id.clone(), Anim { cur: V { y: 80.0, sc: 1.0, h: eh(), o: 0.0, ..Default::default() }, to: V::default(), z: 1000, delay: 0.0 });
        self.finding = false;
        self.open_note(&id);
    }
    /// deletes a note: the file goes to the Recycle Bin, the drawer shows what is left,
    /// and Ctrl+Z there brings the note back until another note is opened
    unsafe fn delete_note(&mut self, id: &str) {
        if self.settings || id == SEARCH || id == SETTINGS { return; }
        let Some(n) = self.note(id).cloned() else { return };
        if id == self.cur { let _ = KillTimer(self.hwnd, TIMER_SAVE); }
        self.close_palette();
        if n.is_empty() { store::delete(&self.dir, &n); } else { store::trash(&self.dir, &n); self.undo = Some(n); }
        self.notes.retain(|x| x.id != id);
        self.anims.remove(id);
        self.colors.remove(id);
        // the window is never without a note: the last one gives way to an empty one
        if self.notes.is_empty() {
            let mut e = Note::new(store::new_id(), 0, String::new(), store::now_ft());
            e.opened = store::now_ft();
            self.notes.push(e);
        }
        if id == self.cur {
            self.cur = self.order()[0].clone();
            store::write_state(&self.dir, &self.cur);
        }
        if self.mode == Mode::Edit {
            self.enter_list();
        } else {
            self.retarget();
        }
    }
    /// the delete binding: the open note, or the picked folder in the drawer
    unsafe fn delete_by_key(&mut self) {
        if self.settings { return; }
        let id = if self.mode == Mode::List { match self.shown.get(self.sel) { Some(id) => id.clone(), None => return } } else { self.cur.clone() };
        self.delete_note(&id);
    }
    /// Ctrl+Z in the drawer: the note deleted last comes back on top
    unsafe fn undo_delete(&mut self) -> bool {
        let Some(mut n) = self.undo.take() else { return false };
        // the empty note that stood in for the last one is not needed any more
        self.notes.retain(|x| !x.is_empty());
        n.opened = store::now_ft();
        let _ = store::save(&self.dir, &n);
        self.cur = n.id.clone();
        store::write_state(&self.dir, &self.cur);
        self.notes.push(n);
        self.list_q.clear();
        self.sel = 0;
        self.list_start = 0;
        self.list_wheel = 0;
        self.retarget_with(true);
        true
    }
    unsafe fn open_find(&mut self) {
        if self.settings { self.settings = false; self.pick = None; self.end_capture(); }
        if self.mode != Mode::Edit { let id = self.cur.clone(); self.open_note(&id); }
        self.finding = true;
        self.find_q.clear();
        self.close_palette();
        self.hover_tab = false;
        self.tab_hl.to = 0.0;
        self.find_hits(true);
        let _ = SetFocus(self.hwnd);
        SetTimer(self.hwnd, TIMER_CARET, 530, None);
        self.retarget();
    }
    unsafe fn close_find(&mut self) {
        self.finding = false;
        self.hits.clear();
        self.restyle();
        let _ = KillTimer(self.hwnd, TIMER_CARET);
        let _ = SetFocus(self.edit);
        self.retarget();
    }
    unsafe fn find_hits(&mut self, reset: bool) {
        let text: Vec<u16> = String::from_utf16_lossy(&self.edit_text()).to_lowercase().encode_utf16().collect();
        let q: Vec<u16> = String::from_utf16_lossy(&self.find_q).to_lowercase().trim().encode_utf16().collect();
        self.hits.clear();
        if !q.is_empty() && text.len() >= q.len() {
            let mut i = 0;
            while i + q.len() <= text.len() {
                if text[i..i + q.len()] == q[..] { self.hits.push((i as i32, (i + q.len()) as i32)); i += q.len(); } else { i += 1; }
            }
        }
        if reset || self.hit >= self.hits.len() { self.hit = 0; }
        self.style_edit(true);
    }
    unsafe fn step_hit(&mut self, d: i32) {
        if self.hits.is_empty() { return; }
        let n = self.hits.len() as i32;
        self.hit = ((self.hit as i32 + d + n) % n) as usize;
        self.style_edit(true);
        self.invalidate();
    }
    unsafe fn set_color(&mut self, id: &str, c: usize) {
        if let Some(n) = self.note_mut(id) { n.color = c; }
        let n = self.note(id).unwrap().clone();
        if !n.is_empty() { let _ = store::save(&self.dir, &n); }
        self.draw_palette();
        self.start_anim();
    }
    /// show from the tray or the hotkey: the folder rises into place
    unsafe fn appear(&mut self) {
        let shown = IsWindowVisible(self.hwnd).as_bool();
        if !shown || self.hiding {
            self.hiding = false;
            if !shown {
                let mut wr = RECT::default();
                let _ = GetWindowRect(self.hwnd, &mut wr);
                self.slide_base = POINT { x: wr.left, y: wr.top };
                let (dx, dy) = self.slide_offset(&wr);
                self.slide.0 = Ease::at(dx);
                self.slide.1 = Ease::at(dy);
                // the buttons fade in with the folder instead of popping up
                if self.fades() { self.ctl_a = Ease::at(0.0); }
            }
            self.ctl_a.to = 1.0;
            self.slide.0.to = 0.0;
            self.slide.1.to = 0.0;
            if self.mode == Mode::Edit && (self.fades() || self.edit_visible == false) {
                if self.edit_snapshot.is_none() {
                    if let Some(scroll) = self.suspended_scroll { self.capture_suspended_edit(scroll); }
                }
                self.hide_edit();
                let id = self.front_id();
                let in_place = self.prefs.dir == 4;
                if let Some(a) = self.anims.get_mut(&id) {
                    if !shown { a.cur.o = 0.0; if in_place { a.cur.y = 22.0; a.cur.sc = 0.985; } }
                    if !self.prefs.far || in_place {} else { a.cur.o = 1.0; }
                }
            }
            self.place_slide();
        }
        let _ = ShowWindow(self.hwnd, SW_SHOW);
        let _ = SetForegroundWindow(self.hwnd);
        if self.input_mode() || !self.edit_visible { let _ = SetFocus(self.hwnd); } else { let _ = SetFocus(self.edit); }
        self.retarget();
    }
    unsafe fn toggle_window(&mut self) {
        if IsWindowVisible(self.hwnd).as_bool() && GetForegroundWindow() == self.hwnd { self.hide(); } else { self.appear(); }
    }
    /// slides (or fades) away the way it came, then hides
    unsafe fn hide(&mut self) {
        self.finish_unfold();
        self.save_now();
        self.close_palette();
        self.close_edit_menu();
        self.end_capture();
        if !IsWindowVisible(self.hwnd).as_bool() { return; }
        if self.mode == Mode::List && self.fades() { let _ = ShowWindow(self.hwnd, SW_HIDE); self.ctl_a = Ease::at(0.0); return; }
        let scroll = (self.mode == Mode::Edit && self.edit_visible).then(|| self.edit_scroll());
        if self.finding { self.close_find(); }
        self.ctl_open = false;
        if self.fades() { self.ctl_a.to = 0.0; }
        if !self.hiding {
            let mut wr = RECT::default();
            let _ = GetWindowRect(self.hwnd, &mut wr);
            self.slide_base = POINT { x: wr.left - self.slide.0.v.round() as i32, y: wr.top - self.slide.1.v.round() as i32 };
            let (dx, dy) = self.slide_offset(&RECT { left: self.slide_base.x, top: self.slide_base.y, right: self.slide_base.x + (wr.right - wr.left), bottom: self.slide_base.y + (wr.bottom - wr.top) });
            self.slide.0.to = dx;
            self.slide.1.to = dy;
        }
        if let Some(scroll) = scroll {
            self.suspended_scroll = Some(scroll);
            self.capture_suspended_edit(scroll);
            self.hide_edit();
        }
        if self.mode == Mode::Edit && self.fades() {
            let id = self.front_id();
            let in_place = self.prefs.dir == 4;
            if let Some(a) = self.anims.get_mut(&id) { a.to = V { y: if in_place { 26.0 } else { 0.0 }, sc: if in_place { 0.985 } else { 1.0 }, h: eh(), o: 0.0, ..Default::default() }; }
        }
        self.hiding = true;
        self.start_anim();
    }

    // ---------- typing into the tab fields ----------
    fn query(&mut self) -> &mut Vec<u16> { if self.finding { &mut self.find_q } else { &mut self.list_q } }
    unsafe fn query_changed(&mut self) {
        self.caret_on = true;
        if self.finding { self.find_hits(true); }
        else { self.sel = 0; self.list_start = 0; self.list_wheel = 0; self.retarget(); }
        self.invalidate();
    }
    unsafe fn paste(&mut self) {
        if OpenClipboard(self.hwnd).is_err() { return; }
        if let Ok(h) = GetClipboardData(13) {
            let p = GlobalLock(HGLOBAL(h.0)) as *const u16;
            if !p.is_null() {
                let mut n = 0; while *p.add(n) != 0 { n += 1; }
                let s: Vec<u16> = std::slice::from_raw_parts(p, n).iter().copied().filter(|&c| c != 13 && c != 10).collect();
                self.query().extend(s);
                let _ = GlobalUnlock(HGLOBAL(h.0));
            }
        }
        let _ = CloseClipboard();
        self.query_changed();
    }

    // ---------- drawing ----------
    unsafe fn ensure_target(&mut self) -> Result<ID2D1HwndRenderTarget> {
        if self.target.is_none() {
            let mut rc = RECT::default();
            GetClientRect(self.hwnd, &mut rc)?;
            // the GPU draws the folders (their stacked shadows are far too slow for the CPU at 120+ Hz);
            // PANE_SOFTWARE=1 falls back to the old software target
            let soft = std::env::var("PANE_SOFTWARE").is_ok();
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: if soft { D2D1_RENDER_TARGET_TYPE_SOFTWARE } else { D2D1_RENDER_TARGET_TYPE_DEFAULT },
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0 * self.scale, dpiY: 96.0 * self.scale,
                usage: D2D1_RENDER_TARGET_USAGE_NONE, minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd, pixelSize: D2D_SIZE_U { width: rc.right as u32, height: rc.bottom as u32 }, presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };
            let target = self.factory.CreateHwndRenderTarget(&props, &hp)?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            target.SetTextRenderingParams(&self.text_rendering);
            self.target = Some(target);
        }
        Ok(self.target.clone().unwrap())
    }
    unsafe fn brush(&self, t: &ID2D1HwndRenderTarget, hex: u32, o: f32) -> Result<ID2D1SolidColorBrush> { t.CreateSolidColorBrush(&rgb(hex, o), None) }
    unsafe fn text(&self, t: &ID2D1HwndRenderTarget, s: &[u16], f: &IDWriteTextFormat, r: D2D_RECT_F, hex: u32, o: f32) -> Result<()> {
        if s.is_empty() { return Ok(()); }
        t.DrawText(s, f, &r, &self.brush(t, hex, o)?, D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_NO_SNAP, DWRITE_MEASURING_MODE_NATURAL);
        Ok(())
    }
    unsafe fn text_w(&self, s: &[u16], f: &IDWriteTextFormat) -> f32 {
        let Ok(l) = self.dw.CreateTextLayout(s, f, 2000.0, 100.0) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = l.GetMetrics(&mut m);
        m.widthIncludingTrailingWhitespace
    }
    /// an icon from the prototype: 16-unit path data drawn at `size` px centred on (cx, cy), under `base`
    unsafe fn icon(&self, t: &ID2D1HwndRenderTarget, base: &Matrix3x2, g: &ID2D1PathGeometry, cx: f32, cy: f32, size: f32, stroke: f32, hex: u32, o: f32) -> Result<()> {
        let k = size / 16.0;
        t.SetTransform(&mul(&mat(k, 0.0, 0.0, k, cx - 8.0 * k, cy - 8.0 * k), base));
        let b = self.brush(t, hex, o)?;
        t.DrawGeometry(g, &b, stroke, &self.round);
        if std::ptr::eq(g, &self.icons.find) { t.DrawEllipse(&circle(7.0, 7.0, 4.5), &b, stroke, None); }
        if std::ptr::eq(g, &self.icons.gear) {
            let bg = self.brush(t, paper(), o)?;
            for (cx, cy) in [(5.5, 5.0), (10.5, 11.0)] { t.FillEllipse(&circle(cx, cy, 1.9), &bg); t.DrawEllipse(&circle(cx, cy, 1.9), &b, stroke, None); }
        }
        t.SetTransform(base);
        Ok(())
    }
    /// a text field living inside a tab: icon, text or placeholder, caret
    unsafe fn tab_input(&self, t: &ID2D1HwndRenderTarget, base: &Matrix3x2, q: &[u16], placeholder: &str, o: f32) -> Result<()> {
        let (_, bx) = tab_x();
        let x0 = tab_text_x();
        self.icon(t, base, &self.icons.find, x0 + 7.5, T / 2.0 + 0.5, 15.0, 1.6, ink2(), o)?;
        let tx = x0 + 23.0;
        let r = rect(tx, 1.0, bx - 6.0, T + 1.0);
        if q.is_empty() { self.text(t, &u(placeholder), &self.fonts.input, r, ink3(), o)?; } else { self.text(t, q, &self.fonts.input, r, ink(), o)?; }
        if self.caret_on {
            let cx = (tx + self.text_w(q, &self.fonts.input) + 1.0).min(bx - 8.0);
            t.DrawLine(pt(cx, 9.0), pt(cx, T - 8.0), &self.brush(t, ink(), o)?, 1.3, None);
        }
        Ok(())
    }
    /// Cache the note's layout so that folder animation only changes its drawing transform.
    unsafe fn preview_lines(&self, n: &Note) -> Result<PreviewLines> {
        let w = width();
        let key = hash(&n.text) as u64 ^ ((ts().to_bits() as u64) << 32) ^ ((w.to_bits() as u64) << 7);
        if let Some((k, v)) = self.previews.borrow().get(&n.id) { if *k == key { return Ok(v.clone()); } }
        let mut out = Vec::new();
        let tw = w - 2.0 * PAD;
        let mut y = EDIT_TOP;
        let px0 = PAD - 1.0; // RichEdit's text starts one pixel left of its box
        let lay = |s: &[u16], f: &IDWriteTextFormat, w: f32| self.dw.CreateTextLayout(s, f, w, 2000.0);
        let (title, marked) = md::preview(n.title());
        if !title.is_empty() {
            let l = lay(&title, &self.fonts.head, tw)?;
            let mut m = DWRITE_TEXT_METRICS::default(); l.GetMetrics(&mut m)?;
            let highlights = preview_highlights(&l, marked);
            out.push(PreviewLine { x: px0, y, layout: l, highlights, title: true, marker: None });
            y += m.height.max(lh_title()) + gap_title();
        } else { y += lh_title() + gap_title(); }
        let lines = n.body_lines();
        let mut num = 0;
        let limit = eh().max(HF) + 40.0;
        let mut fence = false;
        for (i, line) in lines.iter().enumerate() {
            if y > limit { break; }
            if line.trim_start().starts_with("```") { fence = !fence; continue; }
            if md::image_path(&u(line)).is_some() { continue; }
            let (k, txt) = store::marker(line);
            let task = store::task(k, txt);
            let txt = task.map_or(txt, |(_, rest)| rest);
            let (text, marked) = if fence { (u(txt), Vec::new()) } else { md::preview(txt) };
            let next_list = lines.get(i + 1).is_some_and(|line| {
                let (kind, text) = store::marker(line);
                kind != Kind::Plain && store::task(kind, text).is_none()
            });
            let indent = if task.is_some() { (TASK_BOX + TASK_GAP) * ts() }
                else if k != Kind::Plain { 24.0 * ts() } else { 0.0 };
            if k == Kind::Number && task.is_none() { num += 1; } else { num = 0; }
            let marker = match (k, task) {
                (_, Some((done, _))) => Some(PreviewMarker::Task(done)),
                (Kind::Bullet, None) => Some(PreviewMarker::List(lay(&u("•"), &self.fonts.body, 26.0 * ts())?)),
                (Kind::Number, None) => Some(PreviewMarker::List(lay(&u(&format!("{num}.")), &self.fonts.body, 26.0 * ts())?)),
                (Kind::Plain, None) => None,
            };
            let l = lay(&text, &self.fonts.body, tw - indent)?;
            if matches!(marker, Some(PreviewMarker::Task(true))) && !text.is_empty() {
                l.SetStrikethrough(true, DWRITE_TEXT_RANGE { startPosition: 0, length: text.len() as u32 })?;
            }
            let mut m = DWRITE_TEXT_METRICS::default(); l.GetMetrics(&mut m)?;
            let highlights = preview_highlights(&l, marked);
            out.push(PreviewLine { x: px0 + indent, y, layout: l, highlights, title: false, marker });
            y += m.height.max(lh_body()) + if k != Kind::Plain && task.is_none() && next_list { gap_list() } else { gap_body() };
        }
        let v: PreviewLines = out.into();
        self.previews.borrow_mut().insert(n.id.clone(), (key, v.clone()));
        Ok(v)
    }
    unsafe fn preview(&self, t: &ID2D1HwndRenderTarget, n: &Note, h: f32, o: f32) -> Result<()> {
        let w = width();
        let lines = self.preview_lines(n)?;
        let (head, body) = (self.brush(t, ink(), o)?, self.brush(t, ink2(), o)?);
        let done = self.brush(t, ink3(), o)?;
        let highlight = self.brush(t, highlight_c(), o)?;
        let px0 = PAD + RE_DX;
        let size = TASK_BOX * ts();
        let opts = D2D1_DRAW_TEXT_OPTIONS_NO_SNAP;
        t.PushAxisAlignedClip(&rect(1.0, T + 1.0, w - 1.0, h - 4.0), D2D1_ANTIALIAS_MODE_ALIASED);
        for line in lines.iter().take_while(|line| line.y <= h) {
            for r in &line.highlights {
                t.FillRectangle(&rect(line.x + r.left, line.y + r.top, line.x + r.right, line.y + r.bottom), &highlight);
            }
            let brush = match &line.marker {
                Some(PreviewMarker::Task(checked)) => {
                    let top = line.y + lh_body() / 2.0 + 2.0 * ts() - size / 2.0;
                    self.draw_task_box(t, rect(px0, top, px0 + size, top + size), *checked, 1.0, o);
                    if *checked { &done } else { &body }
                }
                Some(PreviewMarker::List(mark)) => {
                    t.DrawTextLayout(pt(px0 + 4.0 * ts(), line.y), mark, &body, opts);
                    &body
                }
                None if line.title => &head,
                None => &body,
            };
            t.DrawTextLayout(pt(line.x, line.y), &line.layout, brush, opts);
        }
        t.PopAxisAlignedClip();
        Ok(())
    }
    /// a fading folder is drawn whole into a layer and faded as one picture: drawing each part translucent
    /// let the dark "+" tab, which lies partly under the folder, show through it while the note went away
    unsafe fn draw_folder(&self, t: &ID2D1HwndRenderTarget, id: &str, v: &V) -> Result<()> {
        let o = v.o.clamp(0.0, 1.0);
        if o < 0.01 { return Ok(()); }
        if o > 0.995 { return self.draw_folder_solid(t, id, v); }
        let layer = t.CreateLayer(None)?;
        t.SetTransform(&shift(0.0, 0.0));
        let big = 1.0e6;
        t.PushLayer(&D2D1_LAYER_PARAMETERS {
            contentBounds: rect(-big, -big, big, big), geometricMask: std::mem::ManuallyDrop::new(None),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, maskTransform: shift(0.0, 0.0), opacity: o,
            opacityBrush: std::mem::ManuallyDrop::new(None), layerOptions: D2D1_LAYER_OPTIONS_NONE,
        }, &layer);
        let solid = V { o: 1.0, ..*v };
        let res = self.draw_folder_solid(t, id, &solid);
        t.PopLayer();
        res
    }
    unsafe fn draw_folder_solid(&self, t: &ID2D1HwndRenderTarget, id: &str, v: &V) -> Result<()> {
        let o = v.o.clamp(0.0, 1.0);
        if o < 0.01 { return Ok(()); }
        let w = width();
        let (x2, bx) = tab_x();
        let base = mul(&fold_matrix(v), &shift(1.0, 1.0 + self.canvas_offset()));
        t.SetTransform(&base);
        let is_search = id == SEARCH;
        let note = self.note(id);
        let color = self.shown_color(id);
        let current_edit = !is_search && id == self.cur && self.mode == Mode::Edit;
        let shaped = current_edit || (id == SETTINGS && self.mode == Mode::Edit);
        let (tl, tr, bl, br) = if shaped { (self.rl.v, self.rr.v, self.rb.v.min(self.rl.v), self.rb.v.min(self.rr.v)) } else { (R, R, R, R) };
        let rb = if shaped { self.rb.v } else { R };
        soft_shadow(t, rect(0.0, T, w, v.h), bl.max(br).max(4.0), 10.0, 18.0, 0.16 * o * if shaped { rb / R } else { 1.0 })?;
        let dark_w = if is_search { DARK_SEARCH } else if current_edit { self.dark.v } else { 0.0 };
        if dark_w > 0.0 { t.FillGeometry(&dark_geometry(&self.factory, bx, dark_w)?, &self.brush(t, tab_dark(), o)?, None); }
        let g = folder_geometry(&self.factory, w, v.h, tl, tr, bl, br)?;
        t.FillGeometry(&g, &self.brush(t, color, o)?, None);
        t.DrawGeometry(&g, &self.brush(t, line_c(), o)?, 1.5, None);

        if is_search {
            self.tab_input(t, &base, &self.list_q, if self.undo.is_some() { ui::NOTE_DELETED.get() } else { ui::SEARCH_NOTES.get() }, o)?;
            let total = self.notes.len();
            let count = self.shown.len();
            let label = if count > self.list_len {
                format!("{}–{}/{count}", self.list_start + 1, self.list_start + self.list_len)
            } else if self.list_q.is_empty() { total.to_string() } else { format!("{count}/{total}") };
            self.text(t, &u(&label), &self.fonts.count, rect(bx, 0.0, bx + DARK_SEARCH, T + 1.0), on_dark(), o)?;
            if self.shown.is_empty() && !self.list_q.is_empty() {
                let msg = ui::NO_RESULTS.get().replace("{query}", String::from_utf16_lossy(&self.list_q).trim());
                self.text(t, &u(&msg), &self.fonts.small, rect(PAD, T + 50.0, w - PAD, T + 80.0), ink2(), o)?;
            }
            return Ok(());
        }
        if id == SETTINGS {
            self.text(t, &u(ui::SETTINGS.get()), &self.fonts.tab, rect(tab_text_x(), 1.0, bx - 6.0, T + 1.0), ink(), o)?;
            return self.draw_settings(t, &base, o);
        }
        let Some(n) = note else { return Ok(()) };
        if current_edit && self.finding {
            self.tab_input(t, &base, &self.find_q, ui::FIND_IN_NOTE.get(), o)?;
            if self.dark.v > 100.0 {
                let c = bx + self.dark.v;
                let label = if self.find_q.is_empty() { String::new() } else if self.hits.is_empty() { "0".into() } else { format!("{}/{}", self.hit + 1, self.hits.len()) };
                self.text(t, &u(&label), &self.fonts.count_l, rect(bx + 8.0, 0.0, c - 80.0, T + 1.0), on_dark(), o)?;
                let op = |i: usize| o * if self.hover_fbtn == Some(i) { 1.0 } else { 0.7 };
                self.icon(t, &base, &self.icons.up, c - 69.0, T / 2.0, 15.0, 1.7, on_dark(), op(2))?;
                self.icon(t, &base, &self.icons.down, c - 43.0, T / 2.0, 15.0, 1.7, on_dark(), op(1))?;
                self.icon(t, &base, &self.icons.close, c - 17.0, T / 2.0, 15.0, 1.7, on_dark(), op(0))?;
            }
        } else {
            if current_edit && self.tab_hl.v > 0.01 {
                // the tab reacts to the pointer: a click opens the drawer, a drag moves the window
                t.FillRoundedRectangle(&rrect(x2 + 5.0, 5.0, bx - 5.0, T - 4.0, 9.0), &self.brush(t, ink(), 0.07 * self.tab_hl.v * o)?);
            }
            let title = if self.prefs.simple_editor { md::preview(n.title()).0 } else { u(n.title()) };
            let title = if title.is_empty() { u(ui::UNTITLED.get()) } else { title };
            self.text(t, &title, &self.fonts.tab, rect(tab_text_x(), 1.0, bx - 6.0, T + 1.0), ink(), o)?;
            if current_edit {
                self.icon(t, &base, &self.icons.plus, bx + self.dark.v / 2.0, T / 2.0, 15.0 * self.plus_sc.v, 2.8, on_dark(), o)?;
            }
        }
        if self.mode == Mode::List {
            let info = u(&format!("{} · {}", when(n.mtime), word_count(n.words())));
            let iw = self.text_w(&info, &self.fonts.small_r);
            self.text(t, &info, &self.fonts.small_r, rect(bx + 40.0, T + 9.0, w - PAD, T + 29.0), ink3(), o)?;
            let q = String::from_utf16_lossy(&self.list_q).to_lowercase();
            let q = q.trim();
            if !q.is_empty() {
                let flat = n.body_flat();
                if let Some(i) = flat.to_lowercase().find(q) {
                    let start = flat[..i.min(flat.len())].char_indices().rev().nth(27).map(|(p, _)| p).unwrap_or(0);
                    let ctx = format!("{}{}", if start > 0 { "…" } else { "" }, &flat[start..]);
                    self.text(t, &u(&ctx), &self.fonts.small, rect(bx + 40.0, T + 9.0, w - PAD - iw - 16.0, T + 29.0), ink2(), o)?;
                }
            }
        }
        if !(current_edit && self.edit_visible) {
            let snapshot = self.edit_snapshot.as_ref().filter(|_| id == self.cur);
            let opacity = snapshot.map_or(0.0, |edit| edit.opacity(self.mode));
            if opacity < 1.0 { self.preview(t, n, v.h, o)?; }
            if let Some(edit) = snapshot.filter(|_| opacity > 0.0) {
                t.PushAxisAlignedClip(&rect(1.0, T + 1.0, w - 1.0, v.h - 4.0), D2D1_ANTIALIAS_MODE_ALIASED);
                t.DrawBitmap(&edit.bitmap, Some(&edit.bounds), opacity, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                t.PopAxisAlignedClip();
            }
        }
        Ok(())
    }
    unsafe fn draw_ctl(&self, t: &ID2D1HwndRenderTarget, base: &Matrix3x2, c: Ctl, k: usize, o: f32) -> Result<()> {
        if o < 0.01 { return Ok(()); }
        let (x, y) = (ctl_x(k), T / 2.0);
        let on = self.hover_ctl == Some(k) || (c == Ctl::Pin && self.pinned) || (c == Ctl::List && self.mode == Mode::List) || (c == Ctl::Find && self.finding) || (c == Ctl::Gear && self.settings) || (c == Ctl::More && self.ctl_open);
        let col = if on { ink() } else { ink2() };
        match c {
            Ctl::Close => self.icon(t, base, &self.icons.close, x, y, 16.0, 1.5, col, o)?,
            Ctl::Gear => self.icon(t, base, &self.icons.gear, x, y, 16.0, 1.5, col, o)?,
            Ctl::Pin => {
                if self.pinned {
                    // pinned: the pin is filled in
                    t.SetTransform(&mul(&shift(x - 8.0, y - 8.0), base));
                    t.FillGeometry(&self.icons.pin_fill, &self.brush(t, col, o)?, None);
                    t.SetTransform(base);
                }
                self.icon(t, base, &self.icons.pin, x, y, 16.0, 1.5, col, o)?
            }
            Ctl::List => self.icon(t, base, &self.icons.folders, x, y, 16.0, 1.5, col, o)?,
            Ctl::Find => self.icon(t, base, &self.icons.find, x, y, 16.0, 1.5, col, o)?,
            Ctl::New => self.icon(t, base, &self.icons.new, x, y, 16.0, 1.6, col, o)?,
            Ctl::Trash => self.icon(t, base, &self.icons.trash, x, y, 16.0, 1.5, col, o)?,
            Ctl::More => {
                // the arrow turns over as the hidden buttons come out
                let a = (180.0 * self.more_t.v).to_radians();
                let (sn, cs) = (a.sin(), a.cos());
                let turn = mul(&mul(&shift(-x, -y), &mat(cs, sn, -sn, cs, 0.0, 0.0)), &shift(x, y));
                self.icon(t, &mul(&turn, base), &self.icons.more, x, y, 16.0, 1.6, col, o)?;
                t.SetTransform(base);
            }
            Ctl::Dot => {
                let r = 7.0 * self.dot_sc.v;
                t.FillEllipse(&circle(x, y, r), &self.brush(t, self.shown_color(&self.cur), o)?);
                t.DrawEllipse(&circle(x, y, r - 0.75), &self.brush(t, line_c(), o)?, 1.5, None);
            }
        }
        Ok(())
    }
    unsafe fn draw_controls(&self, t: &ID2D1HwndRenderTarget) -> Result<()> {
        let a = self.ctl_a.v.clamp(0.0, 1.0);
        if a < 0.01 { return Ok(()); }
        let base = shift(1.0, 1.0 + self.canvas_offset());
        t.SetTransform(&base);
        let y = T / 2.0;
        // a paper capsule under the buttons keeps them readable on a dark desktop; its width eases
        let n = self.cap_n.v.max(1.0);
        let (l, r) = (ctl_x(0) - 34.0 * (n - 1.0) - 16.0, ctl_x(0) + 16.0);
        soft_shadow(t, rect(l, y - 14.0, r, y + 14.0), 14.0, 2.0, 8.0, 0.12 * a)?;
        let cap = rrect(l, y - 14.0, r, y + 14.0, 14.0);
        t.FillRoundedRectangle(&cap, &self.brush(t, paper(), 0.94 * a)?);
        t.DrawRoundedRectangle(&cap, &self.brush(t, line_c(), 0.22 * a)?, 1.0, None);
        // a button that changes crossfades; ones past the capsule's edge fade with it
        let (cur, prev) = (&self.ctl_last, &self.ctl_prev);
        let f = self.ctl_fade.v.clamp(0.0, 1.0);
        for k in 0..cur.len().max(prev.len()) {
            let slot = (n - k as f32).clamp(0.0, 1.0) * a;
            match (prev.get(k), cur.get(k)) {
                (Some(p), Some(c)) if p == c => self.draw_ctl(t, &base, *c, k, slot)?,
                (p, c) => {
                    if let Some(p) = p { self.draw_ctl(t, &base, *p, k, slot * (1.0 - f))?; }
                    if let Some(c) = c { self.draw_ctl(t, &base, *c, k, slot * f)?; }
                }
            }
        }
        Ok(())
    }
    /// notices a change in the set of buttons and starts its animation
    unsafe fn sync_controls(&mut self) {
        let c = self.controls();
        if c == self.ctl_last { return; }
        let old = std::mem::replace(&mut self.ctl_last, c);
        self.ctl_prev = old;
        self.ctl_fade = Ease::at(0.0);
        self.ctl_fade.to = 1.0;
        self.cap_n.to = self.ctl_last.len() as f32;
        self.more_t.to = if self.ctl_open || self.palette.is_some() { 1.0 } else { 0.0 };
        if self.hover_ctl.map_or(false, |k| k >= self.ctl_last.len()) { self.hover_ctl = None; }
        self.set_region();
        self.start_anim();
    }
    unsafe fn render(&mut self) -> Result<()> {
        self.sync_controls();
        self.sync_segs();
        let t = self.ensure_target()?;
        t.BeginDraw();
        t.Clear(Some(&rgb(0, 0.0)));
        let mut items: Vec<(i32, String)> = self.anims.iter().map(|(k, a)| (a.z, k.clone())).collect();
        items.sort();
        for (_, id) in items { let v = self.anims[&id].cur; self.draw_folder(&t, &id, &v)?; }
        self.draw_controls(&t)?;
        if let Err(e) = t.EndDraw(None, None) {
            if e.code() == D2DERR_RECREATE_TARGET {
                self.edit_snapshot = None;
                self.target = None;
            }
        }
        Ok(())
    }

    // ---------- settings: a folder of its own, drawn like the notes; three pages ----------
    fn set_rows(&self) -> Vec<(String, Row, Vec<Seg>)> {
        let p = &self.prefs;
        let k = theme_kind(p);
        let mut rows = vec![(String::new(), Row::Page, vec![seg(ui::GENERAL.get(), self.set_page == 0), seg(ui::SHORTCUTS.get(), self.set_page == 1), seg(ui::COLORS.get(), self.set_page == 2)])];
        let theme = || (ui::THEME.get().to_string(), Row::Theme, vec![seg(ui::PAPER.get(), p.theme == 0), seg(ui::NIGHT.get(), p.theme == 1), seg(ui::SYSTEM_THEME.get(), p.theme == 2), seg(ui::CUSTOM_THEME.get(), p.theme == 3)]);
        let glyph = |g: u8| Seg { glyph: g, ..seg("", false) };
        match self.set_page {
            0 => {
                let languages = LANGUAGES.into_iter().map(|lang| seg(lang.map_or_else(|| ui::SYSTEM_LANGUAGE.get(), Language::name), lang == p.language)).collect();
                rows.push((ui::LANGUAGE.get().into(), Row::Language, languages));
                rows.push((ui::EDITOR.get().into(), Row::Editor, vec![seg("Markdown", !p.simple_editor), seg(ui::SIMPLE.get(), p.simple_editor)]));
                rows.push(theme());
                rows.push((ui::TEXT_SIZE.get().into(), Row::Scale, vec![glyph(1), Seg { value: true, ..seg(format!("{}%", p.scale), false) }, glyph(2)]));
                rows.push((ui::APPEARANCE.get().into(), Row::Dir, vec![seg(ui::BOTTOM.get(), p.dir == 0), seg(ui::RIGHT.get(), p.dir == 1), seg(ui::LEFT.get(), p.dir == 2), seg(ui::TOP.get(), p.dir == 3), seg(ui::IN_PLACE.get(), p.dir == 4)]));
                rows.push((ui::SLIDE_DISTANCE.get().into(), Row::Far, vec![seg(ui::NEAR.get(), !p.far), seg(ui::OFF_SCREEN.get(), p.far)]));
                rows.push((ui::SPEED.get().into(), Row::Speed, vec![seg(ui::FAST.get(), p.speed == 0), seg(ui::NORMAL.get(), p.speed == 1), seg(ui::SLOW.get(), p.speed == 2)]));
                rows.push((ui::START_WITH_WINDOWS.get().into(), Row::Auto, vec![seg(ui::ON.get(), self.autostart), seg(ui::OFF.get(), !self.autostart)]));
                rows.push((ui::NOTES_FOLDER.get().into(), Row::Notes, vec![seg(ui::OPEN.get(), false), seg(ui::MOVE.get(), false)]));
            }
            1 => {
                for i in 0..KEYS {
                    let text = if self.capturing == Some(i) { ui::PRESS_SHORTCUT.get().to_string() } else if self.prefs.keys[i].1 == 0 { ui::NOT_SET.get().to_string() } else { String::new() };
                    rows.push((ui::KEY_NAMES[i].get().to_string(), Row::Key(i), vec![Seg { keys: Some(p.keys[i]), ..seg(text, self.capturing == Some(i)) }]));
                }
            }
            _ => {
                rows.push(theme());
                let sw = |c: u32, text: &str, on: bool| Seg { swatch: Some(c), ..seg(text, on) };
                let own_ink = p.ink[k].is_some() || k == 2;
                rows.push((ui::TEXT.get().into(), Row::Ink, vec![sw(ink(), ui::CUSTOM_COLOR.get(), own_ink), seg(ui::FROM_THEME.get(), !own_ink)]));
                rows.push((ui::ACCENT.get().into(), Row::Accent, vec![sw(tab_dark(), ui::CUSTOM_COLOR.get(), p.accent[k].is_some()), seg(ui::FROM_THEME.get(), p.accent[k].is_none())]));
                let mut cards: Vec<Seg> = (0..ncards()).map(|i| sw(note_rgb(i), "", self.pick == Some(Pick::Card(i)))).collect();
                if ncards() < MAX_CARDS { cards.push(glyph(2)); }
                if ncards() > 1 { cards.push(glyph(1)); }
                rows.push((ui::CARDS.get().into(), Row::Cards, cards));
                rows.push((String::new(), Row::Reset, vec![seg(ui::RESET_COLORS.get(), false)]));
            }
        }
        rows
    }
    fn set_label_w(&self) -> f32 {
        let base = match self.set_page { 1 => 250.0, 2 => 130.0, _ => SET_LABEL };
        if width() < 640.0 { base * 0.8 } else { base }
    }
    fn set_row_height(&self) -> f32 { if self.set_page == 1 { 37.0 } else { SET_ROW } }
    fn set_body_top(&self) -> f32 { SET_TOP + self.set_row_height() + 14.0 }
    fn set_body_bottom(&self) -> f32 { eh() - SET_BOTTOM }
    fn set_scroll_max(&self, end: f32) -> f32 {
        let extra = match self.set_page {
            1 => 52.0,
            2 if self.pick.is_some() => 172.0,
            _ => 30.0,
        };
        (end + extra - self.set_body_bottom()).max(0.0)
    }
    fn set_offset(&self, end: f32) -> f32 { self.set_scroll.min(self.set_scroll_max(end)) }
    fn set_thumb(&self, end: f32) -> Option<(f32, f32)> {
        let max = self.set_scroll_max(end);
        if max == 0.0 { return None; }
        let h = self.set_body_bottom() - self.set_body_top();
        let size = (h * h / (h + max)).max(18.0);
        Some((self.set_body_top() + (h - size) * self.set_offset(end) / max, size))
    }
    unsafe fn begin_settings_scroll(&mut self, x: f32, y: f32) -> bool {
        if !self.set_visible() || !(width() - 20.0..width() - GRIP).contains(&x)
            || !(self.set_body_top()..self.set_body_bottom()).contains(&y) { return false; }
        let Some((top, size)) = self.set_thumb(self.set_geom().1) else { return false; };
        let grab = if (top..top + size).contains(&y) { y - top } else { size / 2.0 };
        self.set_scroll_drag = Some(grab);
        self.drag_settings_scroll(y, grab);
        true
    }
    unsafe fn drag_settings_scroll(&mut self, y: f32, grab: f32) {
        let end = self.set_geom().1;
        let Some((_, size)) = self.set_thumb(end) else { return };
        let top = self.set_body_top();
        let travel = self.set_body_bottom() - top - size;
        self.set_scroll = ((y - grab - top) / travel).clamp(0.0, 1.0) * self.set_scroll_max(end);
        self.hover_set = None;
        self.invalidate();
    }
    unsafe fn scroll_settings(&mut self, dy: f32) {
        let end = self.set_geom().1;
        self.set_scroll = (self.set_offset(end) + dy).clamp(0.0, self.set_scroll_max(end));
        self.hover_set = None;
        self.invalidate();
    }
    fn show_setting(&mut self, top: f32, bottom: f32, end: f32) {
        let offset = self.set_offset(end);
        let target = if top - offset < self.set_body_top() { top - self.set_body_top() }
            else if bottom - offset > self.set_body_bottom() { bottom - self.set_body_bottom() }
            else { offset };
        self.set_scroll = target.clamp(0.0, self.set_scroll_max(end));
    }
    /// the rows laid out: label, controls block, each part's box; and where the rows end.
    /// A block that does not fit beside its label moves under it.
    unsafe fn set_geom(&self) -> (Vec<RowG>, f32) {
        let mut out = vec![];
        let mut y = SET_TOP;
        // The shortcut rows are shorter so they fit in the default window.
        let rh = self.set_row_height();
        let small = |s: &str| self.text_w(&u(s), &self.fonts.small);
        for (label, row, segs) in self.set_rows() {
            let st = style(row);
            let widths: Vec<f32> = segs.iter().map(|s| match st {
                Style::Choice => small(&s.text) + 30.0 + if s.swatch.is_some() { 20.0 } else { 0.0 },
                Style::Step => if s.value { small(&s.text) + 24.0 } else { 36.0 },
                Style::Keys => keys_width(self, s),
                Style::Swatches => 30.0,
                Style::Button => small(&s.text) + 30.0,
            }).collect();
            let gap = match st { Style::Swatches => 8.0, Style::Choice | Style::Step => 0.0, _ => 6.0 };
            let total: f32 = widths.iter().sum::<f32>() + gap * (widths.len().max(1) - 1) as f32;
            let lw = if label.is_empty() { 0.0 } else { self.set_label_w() };
            let mut x = PAD + lw;
            let mut top = y;
            let mut h = rh;
            if x + total > width() - PAD && lw > 0.0 { x = PAD; top = y + 34.0; h = rh + 34.0; }
            let by = top + (rh - CTL_H) / 2.0;
            let block = rect(x, by, x + total, by + CTL_H);
            let mut parts = vec![];
            for (s, w) in segs.into_iter().zip(widths) {
                parts.push((rect(x, by, x + w, by + CTL_H), s));
                x += w + gap;
            }
            out.push(RowG { row, label, top: y, h: rh, block, segs: parts });
            y += h + if row == Row::Page { 14.0 } else { 0.0 };
        }
        (out, y)
    }
    fn set_visible(&self) -> bool { self.settings && self.mode == Mode::Edit && self.anims.get(SETTINGS).map_or(false, |a| a.cur.o > 0.6) }
    unsafe fn set_hit(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        if !self.set_visible() { return None; }
        let (rows, end) = self.set_geom();
        let body = (self.set_body_top()..self.set_body_bottom()).contains(&y);
        for (r, g) in rows.iter().enumerate() {
            if (r != 0) != body { continue; }
            let y = if body { y + self.set_offset(end) } else { y };
            for (i, (rc, s)) in g.segs.iter().enumerate() {
                if x >= rc.left && x <= rc.right && y >= rc.top - 4.0 && y <= rc.bottom + 4.0 && !s.value { return Some((r, i)); }
            }
        }
        None
    }
    /// the sliding part of each choice block follows the chosen option
    unsafe fn sync_segs(&mut self) {
        if !self.settings { return; }
        let page = self.set_page;
        let mut moved = false;
        for g in self.set_geom().0 {
            if style(g.row) != Style::Choice { continue; }
            let Some((rc, _)) = g.segs.iter().find(|(_, s)| s.on) else { continue };
            let e = self.seg_anim.entry((page, g.row)).or_insert([Ease::at(rc.left), Ease::at(rc.right)]);
            if e[0].to != rc.left || e[1].to != rc.right { e[0].to = rc.left; e[1].to = rc.right; moved = true; }
        }
        if moved { self.start_anim(); }
    }
    unsafe fn draw_settings(&self, t: &ID2D1HwndRenderTarget, base: &Matrix3x2, o: f32) -> Result<()> {
        let (rows, end) = self.set_geom();
        self.draw_setting_rows(t, &rows[..1], 0, o)?;
        let (top, bottom) = (self.set_body_top(), self.set_body_bottom());
        let offset = self.set_offset(end);
        t.PushAxisAlignedClip(&rect(0.0, top, width(), bottom), D2D1_ANTIALIAS_MODE_ALIASED);
        t.SetTransform(&mul(&shift(0.0, -offset), base));
        let result = (|| -> Result<()> {
            self.draw_setting_rows(t, &rows[1..], 1, o)?;
            if self.set_page == 2 { self.draw_picker(t, end, o)?; }
            let keys = match self.set_page {
                0 => [Some(ui::GENERAL_HELP.get()), None],
                1 => [Some(ui::SHORTCUT_HELP.get()), Some(ui::GLOBAL_HELP.get())],
                2 if self.pick.is_none() => [Some(ui::COLOR_HELP.get()), None],
                _ => [None, None],
            };
            for (k, line) in keys.into_iter().flatten().enumerate() {
                let y = end + 10.0 + k as f32 * 22.0;
                self.text(t, &u(line), &self.fonts.small, rect(PAD, y, width() - PAD, y + 20.0), ink3(), o)?;
            }
            Ok(())
        })();
        t.SetTransform(base);
        t.PopAxisAlignedClip();
        result?;
        if let Some((y, thumb)) = self.set_thumb(end) {
            let x = width() - 12.0;
            t.FillRoundedRectangle(&rrect(x, y, x + 4.0, y + thumb, 2.0), &self.brush(t, ink2(), 0.4 * o)?);
        }
        Ok(())
    }
    unsafe fn draw_setting_rows(&self, t: &ID2D1HwndRenderTarget, rows: &[RowG], first: usize, o: f32) -> Result<()> {
        let w = width();
        let lw = self.set_label_w();
        let focus = if self.kb_focus { Some(self.focus) } else { None };
        for (i, g) in rows.iter().enumerate() {
            let r = first + i;
            if !g.label.is_empty() { self.text(t, &u(&g.label), &self.fonts.input, rect(PAD, g.top, PAD + lw - 10.0, g.top + g.h), ink2(), o)?; }
            let st = style(g.row);
            let b = g.block;
            let hh = b.bottom - b.top;
            if matches!(st, Style::Choice | Style::Step) {
                // one block; the chosen part is a pill that slides inside it
                let shell = rrect(b.left, b.top, b.right, b.bottom, hh / 2.0);
                t.FillRoundedRectangle(&shell, &self.brush(t, ink(), 0.05 * o)?);
                t.DrawRoundedRectangle(&shell, &self.brush(t, line_c(), 0.35 * o)?, 1.0, None);
            }
            let pill = if st == Style::Choice { self.seg_anim.get(&(self.set_page, g.row)).map(|e| (e[0].v, e[1].v)) } else { None };
            if let Some((l, rr)) = pill {
                if g.segs.iter().any(|(_, s)| s.on) {
                    t.FillRoundedRectangle(&rrect(l + 3.0, b.top + 3.0, rr - 3.0, b.bottom - 3.0, (hh - 6.0) / 2.0), &self.brush(t, tab_dark(), o)?);
                }
            }
            for (i, (rc, s)) in g.segs.iter().enumerate() {
                let hover = self.hover_set == Some((r, i));
                let (cx, cy) = ((rc.left + rc.right) / 2.0, (rc.top + rc.bottom) / 2.0);
                match st {
                    Style::Choice => {
                        // text turns light where the pill lies under it
                        let cover = pill.map_or(0.0, |(l, rr)| ((rr.min(rc.right) - l.max(rc.left)) / (rc.right - rc.left)).clamp(0.0, 1.0));
                        let base = if hover { ink() } else { ink2() };
                        let col = mix(base, on_dark(), cover);
                        let mut tx = rc.left + 15.0;
                        if let Some(c) = s.swatch {
                            t.FillEllipse(&circle(rc.left + 21.0, cy, 7.0), &self.brush(t, c, o)?);
                            t.DrawEllipse(&circle(rc.left + 21.0, cy, 7.0), &self.brush(t, mix(line_c(), on_dark(), cover), 0.7 * o)?, 1.0, None);
                            tx = rc.left + 35.0;
                        }
                        self.text(t, &u(&s.text), &self.fonts.small, rect(tx, rc.top, rc.right + 4.0, rc.bottom), col, o)?;
                    }
                    Style::Step => {
                        if s.value { self.text(t, &u(&s.text), &self.fonts.count, *rc, ink(), o)?; }
                        else {
                            if hover { t.FillRoundedRectangle(&rrect(rc.left + 3.0, rc.top + 3.0, rc.right - 3.0, rc.bottom - 3.0, (hh - 6.0) / 2.0), &self.brush(t, ink(), 0.08 * o)?); }
                            self.glyph(t, s.glyph, cx, cy, ink(), o)?;
                        }
                    }
                    Style::Keys => self.draw_keys(t, rc, s, o)?,
                    Style::Swatches => {
                        if let Some(c) = s.swatch {
                            t.FillEllipse(&circle(cx, cy, 11.0), &self.brush(t, c, o)?);
                            t.DrawEllipse(&circle(cx, cy, 11.0), &self.brush(t, line_c(), 0.6 * o)?, 1.0, None);
                            if s.on { t.DrawEllipse(&circle(cx, cy, 15.0), &self.brush(t, ink(), o)?, 1.6, None); }
                            else if hover { t.DrawEllipse(&circle(cx, cy, 15.0), &self.brush(t, ink(), 0.3 * o)?, 1.2, None); }
                        } else {
                            if hover { t.FillEllipse(&circle(cx, cy, 13.0), &self.brush(t, ink(), 0.08 * o)?); }
                            t.DrawEllipse(&circle(cx, cy, 13.0), &self.brush(t, line_c(), 0.45 * o)?, 1.0, None);
                            self.glyph(t, s.glyph, cx, cy, ink2(), o)?;
                        }
                    }
                    Style::Button => {
                        let rr = rrect(rc.left, rc.top, rc.right, rc.bottom, hh / 2.0);
                        if hover { t.FillRoundedRectangle(&rr, &self.brush(t, ink(), 0.08 * o)?); }
                        t.DrawRoundedRectangle(&rr, &self.brush(t, line_c(), 0.4 * o)?, 1.0, None);
                        self.text(t, &u(&s.text), &self.fonts.small, rect(rc.left + 15.0, rc.top, rc.right + 4.0, rc.bottom), ink(), o)?;
                    }
                }
                if focus == Some((r, i)) {
                    let rad = if st == Style::Swatches { 18.0 } else { hh / 2.0 + 3.0 };
                    t.DrawRoundedRectangle(&rrect(rc.left - 3.0, rc.top - 3.0, rc.right + 3.0, rc.bottom + 3.0, rad), &self.brush(t, tab_dark(), 0.85 * o)?, 1.8, None);
                }
            }
            if g.row == Row::Notes {
                // where the notes are, shortened from the left so the end of the path stays readable
                let full = self.dir.to_string_lossy().to_string();
                let room = w - PAD - b.right - 12.0;
                let mut shown = full.clone();
                let chars: Vec<char> = full.chars().collect();
                let mut cut = 0;
                while cut < chars.len() && self.text_w(&u(&shown), &self.fonts.small) > room {
                    cut += 1;
                    shown = format!("…{}", chars[cut..].iter().collect::<String>());
                }
                self.text(t, &u(&shown), &self.fonts.small, rect(b.right + 12.0, b.top, w - PAD, b.bottom), ink3(), o)?;
            }
            if let Row::Key(i) = g.row {
                if self.hk_err == Some(i) { self.text(t, &u(ui::SHORTCUT_TAKEN.get()), &self.fonts.small, rect(b.right + 12.0, b.top, w - PAD, b.bottom), ink3(), o)?; }
            }
        }
        Ok(())
    }
    /// a minus (1) or plus (2) drawn as lines, so no dash character is needed
    unsafe fn glyph(&self, t: &ID2D1HwndRenderTarget, g: u8, cx: f32, cy: f32, hex: u32, o: f32) -> Result<()> {
        let b = self.brush(t, hex, o)?;
        t.DrawLine(pt(cx - 5.0, cy), pt(cx + 5.0, cy), &b, 1.6, &self.round);
        if g == 2 { t.DrawLine(pt(cx, cy - 5.0), pt(cx, cy + 5.0), &b, 1.6, &self.round); }
        Ok(())
    }
    /// a key binding as keyboard keys: one cap per key
    unsafe fn draw_keys(&self, t: &ID2D1HwndRenderTarget, rc: &D2D_RECT_F, s: &Seg, o: f32) -> Result<()> {
        let h = rc.bottom - rc.top;
        if !s.text.is_empty() {
            // listening, or nothing bound: a dashed slot
            let rr = rrect(rc.left, rc.top + 2.0, rc.right, rc.bottom - 2.0, 8.0);
            let col = if s.on { tab_dark() } else { line_c() };
            if s.on { t.FillRoundedRectangle(&rr, &self.brush(t, tab_dark(), 0.08 * o)?); }
            let dash = self.factory.CreateStrokeStyle(&D2D1_STROKE_STYLE_PROPERTIES { dashStyle: D2D1_DASH_STYLE_DASH, ..Default::default() }, None)?;
            t.DrawRoundedRectangle(&rr, &self.brush(t, col, if s.on { o } else { 0.6 * o })?, 1.2, &dash);
            self.text(t, &u(&s.text), &self.fonts.small, rect(rc.left + 14.0, rc.top, rc.right + 4.0, rc.bottom), if s.on { ink() } else { ink3() }, o)?;
            return Ok(());
        }
        let (m, vk) = s.keys.unwrap_or((0, 0));
        let mut x = rc.left;
        for k in key_parts(m, vk) {
            let w = self.text_w(&u(&k), &self.fonts.small).max(10.0) + 18.0;
            let (top, bot) = (rc.top + 2.0, rc.bottom - 4.0);
            // the cap: a lighter face over a darker edge, like a real key
            t.FillRoundedRectangle(&rrect(x, top + 2.0, x + w, bot + 2.0, 7.0), &self.brush(t, line_c(), 0.45 * o)?);
            t.FillRoundedRectangle(&rrect(x, top, x + w, bot, 7.0), &self.brush(t, mix(paper(), ink(), 0.07), o)?);
            t.DrawRoundedRectangle(&rrect(x + 0.5, top + 0.5, x + w - 0.5, bot - 0.5, 6.5), &self.brush(t, line_c(), 0.35 * o)?, 1.0, None);
            self.text(t, &u(&k), &self.fonts.count, rect(x, top, x + w, bot), ink(), o)?;
            x += w + 5.0;
        }
        let _ = h;
        Ok(())
    }
    /// keyboard in the settings: arrows and Tab move, Enter or Space presses, Ctrl+Tab turns the page
    unsafe fn settings_key(&mut self, vk: u16, mods: u32) -> bool {
        if !self.set_visible() || self.capturing.is_some() { return false; }
        let (ctrl, shift) = (mods & 2 != 0, mods & 4 != 0);
        let rows = self.set_geom().0;
        let picker = self.pick.is_some() && self.set_page == 2;
        let n = rows.len() + picker as usize;
        let on_picker = picker && self.focus.0 >= rows.len();
        let chosen = |r: usize| rows.get(r).and_then(|g| g.segs.iter().position(|(_, s)| s.on)).unwrap_or(0);
        let goto = |a: &mut App, r: usize| { a.focus = (r, if r < rows.len() && style(rows[r].row) == Style::Choice { chosen(r) } else { 0 }); };
        match VIRTUAL_KEY(vk) {
            VK_TAB if ctrl => { self.turn_page(if shift { -1 } else { 1 }); }
            VK_NEXT if ctrl => self.turn_page(1),
            VK_PRIOR if ctrl => self.turn_page(-1),
            VK_NEXT | VK_PRIOR => {
                let distance = self.set_body_bottom() - self.set_body_top() - CTL_H;
                self.scroll_settings(if vk == VK_NEXT.0 { distance } else { -distance });
                self.kb_focus = false;
                return true;
            }
            VK_UP | VK_DOWN if on_picker && ctrl => {
                let d = if vk == VK_UP.0 { 0.04 } else { -0.04 };
                self.pick_hsv[2] = (self.pick_hsv[2] + d).clamp(0.0, 1.0);
                self.pick_apply();
            }
            VK_TAB | VK_UP | VK_DOWN => {
                let d: isize = if vk == VK_UP.0 || (vk == VK_TAB.0 && shift) { -1 } else { 1 };
                let r = if self.kb_focus { ((self.focus.0 as isize + d).rem_euclid(n as isize)) as usize } else { 0 };
                goto(self, r);
            }
            VK_LEFT | VK_RIGHT => {
                let d: isize = if vk == VK_LEFT.0 { -1 } else { 1 };
                if !self.kb_focus { goto(self, 0); }
                else if on_picker {
                    if ctrl { self.pick_hsv[1] = (self.pick_hsv[1] + d as f32 * 0.04).clamp(0.0, 1.0); }
                    else { self.pick_hsv[0] = (self.pick_hsv[0] + d as f32 * if shift { 1.0 } else { 6.0 }).rem_euclid(360.0); }
                    self.pick_apply();
                } else if let Some(g) = rows.get(self.focus.0) {
                    let len = g.segs.len() as isize;
                    match style(g.row) {
                        Style::Choice => {
                            let i = (chosen(self.focus.0) as isize + d).clamp(0, len - 1) as usize;
                            let r = self.focus.0;
                            self.set_click(r, i);
                            self.focus = (r, i);
                        }
                        Style::Step => { let r = self.focus.0; self.set_click(r, if d < 0 { 0 } else { 2 }); }
                        _ => self.focus.1 = (self.focus.1 as isize + d).clamp(0, len - 1) as usize,
                    }
                }
            }
            VK_RETURN | VK_SPACE => {
                if !self.kb_focus { goto(self, 0); }
                else if on_picker { self.pick = None; self.focus.0 = rows.len().saturating_sub(1); }
                else if rows.get(self.focus.0).is_some() {
                    let (r, i) = self.focus;
                    let before = self.pick;
                    self.set_click(r, i);
                    // a colour opened for editing: the arrows now work in the picker
                    if self.pick.is_some() && self.pick != before { self.focus = (self.set_geom().0.len(), 0); }
                }
            }
            _ => return false,
        }
        // keep the focus on something that exists after the rows changed
        let (rows, end) = self.set_geom();
        let n = rows.len() + (self.pick.is_some() && self.set_page == 2) as usize;
        if self.focus.0 >= n { self.focus.0 = n.saturating_sub(1); }
        if let Some(g) = rows.get(self.focus.0) { self.focus.1 = self.focus.1.min(g.segs.len().saturating_sub(1)); }
        if let Some(g) = rows.get(self.focus.0).filter(|g| g.row != Row::Page) {
            self.show_setting(g.top, g.block.bottom + 4.0, end);
        } else if self.focus.0 >= rows.len() {
            self.show_setting(end, end + 172.0, end);
        }
        self.kb_focus = true;
        self.invalidate();
        true
    }
    unsafe fn turn_page(&mut self, d: isize) {
        self.set_page = ((self.set_page as isize + d).rem_euclid(3)) as usize;
        self.set_scroll = 0.0;
        self.pick = None;
        self.end_capture();
        self.focus = (0, self.set_page);
    }
    unsafe fn pick_apply(&mut self) {
        self.pick_hex.clear();
        let [h, s, v] = self.pick_hsv;
        self.set_pick_color(hsv_rgb(h, s, v), true);
    }

    // ---------- colour picker: a saturation/brightness square, a hue strip and the code ----------
    /// (square, hue strip, x of the code column)
    unsafe fn pick_geom(&self) -> Option<(D2D_RECT_F, D2D_RECT_F, f32)> {
        if self.set_page != 2 || self.pick.is_none() || !self.settings { return None; }
        let y = self.set_geom().1 + 6.0;
        let x = PAD + self.set_label_w();
        let w = (width() - PAD - x - 150.0).clamp(140.0, 260.0);
        Some((rect(x, y, x + w, y + 110.0), rect(x, y + 120.0, x + w, y + 134.0), x + w + 18.0))
    }
    unsafe fn draw_picker(&self, t: &ID2D1HwndRenderTarget, _end: f32, o: f32) -> Result<()> {
        let Some((sv, hue, cx)) = self.pick_geom() else { return Ok(()) };
        let [h, s, v] = self.pick_hsv;
        let stop = |p: f32, c: u32, a: f32| D2D1_GRADIENT_STOP { position: p, color: rgb(c, a) };
        let grad = |stops: &[D2D1_GRADIENT_STOP], a: D2D_POINT_2F, b: D2D_POINT_2F| -> Result<ID2D1LinearGradientBrush> {
            let coll = t.CreateGradientStopCollection(stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            t.CreateLinearGradientBrush(&D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: a, endPoint: b },
                Some(&D2D1_BRUSH_PROPERTIES { opacity: o, transform: mat(1.0, 0.0, 0.0, 1.0, 0.0, 0.0) }), &coll)
        };
        let box_ = rrect(sv.left, sv.top, sv.right, sv.bottom, 8.0);
        t.FillRoundedRectangle(&box_, &self.brush(t, hsv_rgb(h, 1.0, 1.0), o)?);
        t.FillRoundedRectangle(&box_, &grad(&[stop(0.0, 0xffffff, 1.0), stop(1.0, 0xffffff, 0.0)], pt(sv.left, 0.0), pt(sv.right, 0.0))?);
        t.FillRoundedRectangle(&box_, &grad(&[stop(0.0, 0, 0.0), stop(1.0, 0, 1.0)], pt(0.0, sv.top), pt(0.0, sv.bottom))?);
        t.DrawRoundedRectangle(&box_, &self.brush(t, line_c(), 0.5 * o)?, 1.0, None);
        let hs: Vec<D2D1_GRADIENT_STOP> = (0..=6).map(|i| stop(i as f32 / 6.0, hsv_rgb(i as f32 * 60.0, 1.0, 1.0), 1.0)).collect();
        let strip = rrect(hue.left, hue.top, hue.right, hue.bottom, 7.0);
        t.FillRoundedRectangle(&strip, &grad(&hs, pt(hue.left, 0.0), pt(hue.right, 0.0))?);
        t.DrawRoundedRectangle(&strip, &self.brush(t, line_c(), 0.5 * o)?, 1.0, None);
        // knobs
        let (kx, ky) = (sv.left + s * (sv.right - sv.left), sv.top + (1.0 - v) * (sv.bottom - sv.top));
        let c = hsv_rgb(h, s, v);
        t.FillEllipse(&circle(kx, ky, 7.0), &self.brush(t, c, o)?);
        t.DrawEllipse(&circle(kx, ky, 7.0), &self.brush(t, 0xffffff, o)?, 2.0, None);
        t.DrawEllipse(&circle(kx, ky, 8.5), &self.brush(t, 0, 0.35 * o)?, 1.0, None);
        let hx = hue.left + h / 360.0 * (hue.right - hue.left);
        let (hy, hb) = (hue.top - 3.0, hue.bottom + 3.0);
        t.FillRoundedRectangle(&rrect(hx - 4.0, hy, hx + 4.0, hb, 4.0), &self.brush(t, hsv_rgb(h, 1.0, 1.0), o)?);
        t.DrawRoundedRectangle(&rrect(hx - 4.0, hy, hx + 4.0, hb, 4.0), &self.brush(t, 0xffffff, o)?, 2.0, None);
        // the colour and its code; typing a code replaces it
        let cur = self.pick_color();
        t.FillRoundedRectangle(&rrect(cx, sv.top, cx + 70.0, sv.top + 40.0, 10.0), &self.brush(t, cur, o)?);
        t.DrawRoundedRectangle(&rrect(cx, sv.top, cx + 70.0, sv.top + 40.0, 10.0), &self.brush(t, line_c(), 0.6 * o)?, 1.0, None);
        let code = if self.pick_hex.is_empty() { format!("#{cur:06X}") } else { format!("#{}", String::from_utf16_lossy(&self.pick_hex).to_uppercase()) };
        self.text(t, &u(&code), &self.fonts.input, rect(cx, sv.top + 46.0, cx + 140.0, sv.top + 72.0), if self.pick_hex.is_empty() { ink() } else { ink2() }, o)?;
        self.text(t, &u(ui::COLOR_CODE.get()), &self.fonts.small, rect(cx, sv.top + 72.0, cx + 160.0, sv.top + 92.0), ink3(), o)?;
        self.text(t, &u(ui::COLOR_DONE.get()), &self.fonts.small, rect(cx, sv.top + 92.0, cx + 160.0, sv.top + 112.0), ink3(), o)?;
        if self.kb_focus && self.focus.0 >= self.set_geom().0.len() {
            t.DrawRoundedRectangle(&rrect(sv.left - 5.0, sv.top - 5.0, sv.right + 5.0, hue.bottom + 7.0, 12.0), &self.brush(t, tab_dark(), 0.85 * o)?, 1.8, None);
            self.text(t, &u(ui::COLOR_KEYS.get()), &self.fonts.small, rect(sv.left, hue.bottom + 12.0, width() - PAD, hue.bottom + 32.0), ink3(), o)?;
        }
        Ok(())
    }
    unsafe fn pick_hit(&self, x: f32, y: f32) -> Option<u8> {
        let (sv, hue, _) = self.pick_geom()?;
        if !self.set_visible() || !(self.set_body_top()..self.set_body_bottom()).contains(&y) { return None; }
        let y = y + self.set_offset(self.set_geom().1);
        let inr = |r: &D2D_RECT_F, m: f32| x >= r.left - m && x <= r.right + m && y >= r.top - m && y <= r.bottom + m;
        if inr(&sv, 6.0) { Some(0) } else if inr(&hue, 6.0) { Some(1) } else { None }
    }
    unsafe fn pick_drag(&mut self, part: u8, x: f32, y: f32) {
        let Some((sv, hue, _)) = self.pick_geom() else { return };
        let y = y + self.set_offset(self.set_geom().1);
        let fx = |r: &D2D_RECT_F| ((x - r.left) / (r.right - r.left)).clamp(0.0, 1.0);
        if part == 0 { self.pick_hsv[1] = fx(&sv); self.pick_hsv[2] = 1.0 - ((y - sv.top) / (sv.bottom - sv.top)).clamp(0.0, 1.0); }
        else { self.pick_hsv[0] = fx(&hue) * 359.9; }
        self.pick_hex.clear();
        let [h, s, v] = self.pick_hsv;
        self.set_pick_color(hsv_rgb(h, s, v), false);
    }
    fn pick_color(&self) -> u32 {
        match self.pick { Some(Pick::Ink) => ink(), Some(Pick::Accent) => tab_dark(), Some(Pick::Card(i)) => note_rgb(i), None => 0 }
    }
    unsafe fn start_pick(&mut self, p: Pick) {
        if self.pick == Some(p) { self.pick = None; return; }
        self.pick = Some(p);
        self.pick_hsv = rgb_hsv(self.pick_color());
        self.pick_hex.clear();
        let end = self.set_geom().1;
        self.show_setting(end, end + 147.0, end);
    }
    /// the edited colour changes; while dragging only the look updates, the rest happens on release
    unsafe fn set_pick_color(&mut self, c: u32, done: bool) {
        let k = theme_kind(&self.prefs);
        let mut p = self.prefs;
        match self.pick { Some(Pick::Ink) => p.ink[k] = Some(c), Some(Pick::Accent) => p.accent[k] = Some(c), Some(Pick::Card(i)) => p.cards[k][i] = c, None => return }
        if done { self.apply_prefs(p); } else { self.prefs = p; TH_ = make_theme(&self.prefs); self.start_anim(); }
        self.invalidate();
    }
    /// a character typed while the picker is open: a hex code
    unsafe fn pick_char(&mut self, c: u16) {
        if c == 8 { self.pick_hex.pop(); }
        else if (c as u8 as char).is_ascii_hexdigit() && c < 128 {
            if self.pick_hex.len() >= 6 { self.pick_hex.clear(); }
            self.pick_hex.push(c);
        } else { return; }
        if self.pick_hex.len() == 6 {
            if let Some(v) = settings::hex(&String::from_utf16_lossy(&self.pick_hex)) {
                self.pick_hsv = rgb_hsv(v);
                self.set_pick_color(v, true);
            }
        }
        self.invalidate();
    }

    unsafe fn set_click(&mut self, r: usize, i: usize) {
        let rows = self.set_rows();
        let Some((_, row, segs)) = rows.get(r) else { return };
        let glyph = segs.get(i).map(|s| s.glyph).unwrap_or(0);
        let mut p = self.prefs;
        let k = theme_kind(&p);
        let mut demo = false;
        match *row {
            Row::Page => { self.set_page = i; self.set_scroll = 0.0; self.pick = None; self.end_capture(); self.focus = (0, i); }
            Row::Language => p.language = LANGUAGES[i],
            Row::Editor => p.simple_editor = i == 1,
            Row::Theme => { p.theme = i as u8; self.pick = None; }
            Row::Scale => p.scale = if i == 0 { p.scale.saturating_sub(10).max(80) } else { (p.scale + 10).min(160) },
            Row::Dir => { p.dir = i as u8; demo = true; }
            Row::Far => { p.far = i == 1; demo = true; }
            Row::Speed => { p.speed = i as u8; demo = true; }
            Row::Auto => { settings::set_autostart(i == 0); self.autostart = settings::autostart(); }
            Row::Key(n) => { if self.capturing == Some(n) { self.end_capture(); } else { self.start_capture(n); } }
            Row::Ink => if i == 0 { self.start_pick(Pick::Ink); } else { p.ink[k] = None; if self.pick == Some(Pick::Ink) { self.pick = None; } },
            Row::Accent => if i == 0 { self.start_pick(Pick::Accent); } else { p.accent[k] = None; if self.pick == Some(Pick::Accent) { self.pick = None; } },
            Row::Cards => {
                if i < p.ncards { self.start_pick(Pick::Card(i)); }
                else if glyph == 2 { p.ncards += 1; }
                else if glyph == 1 { p.ncards -= 1; if self.pick == Some(Pick::Card(p.ncards)) { self.pick = None; } }
            }
            Row::Notes => {
                if i == 0 { let d = wide(&self.dir.to_string_lossy()); ShellExecuteW(self.hwnd, w!("open"), PCWSTR(d.as_ptr()), None, None, SW_SHOWNORMAL); }
                else if let Some(to) = shell::pick_folder(self.hwnd, ui::MOVE_NOTES.get(), Some(&self.dir)) { self.move_notes(to); }
                return;
            }
            Row::Reset => { p.cards[k] = DEFAULT_CARDS[k]; p.ink[k] = None; p.accent[k] = None; self.pick = None; }
        }
        self.apply_prefs(p);
        // show the new way in and out right away
        if demo { self.demo = true; self.hide(); }
        self.invalidate();
    }
    unsafe fn step_text(&mut self, d: i32) {
        let mut p = self.prefs;
        p.scale = if d == 0 { 100 } else { (p.scale as i32 + d).clamp(80, 160) as u32 };
        self.apply_prefs(p);
    }
    /// new preferences: applied and saved right away
    unsafe fn apply_prefs(&mut self, p: Prefs) {
        let old = self.prefs;
        self.prefs = p;
        settings::save(&p);
        if old.language != p.language {
            locale::set(p.language);
            self.seg_anim.clear();
            self.hover_set = None;
        }
        if old.theme != p.theme || old.cards != p.cards || old.ink != p.ink || old.accent != p.accent || old.ncards != p.ncards { self.apply_theme(); }
        if old.scale != p.scale { self.apply_scale(); }
        if old.simple_editor != p.simple_editor { self.restyle(); self.invalidate(); }
    }
    unsafe fn apply_theme(&mut self) {
        TH_ = make_theme(&self.prefs);
        let old = std::mem::replace(&mut self.car_dc, ink_dc());
        let _ = DeleteDC(old);
        self.car_a = 0;
        self.caret_layer.hide();
        self.restyle();
        if self.palette.is_some() { self.draw_palette(); }
        self.update_hits();
        // note colours ease to the new theme's
        self.start_anim();
        self.invalidate();
    }
    unsafe fn apply_scale(&mut self) {
        TS_ = self.prefs.scale as f32 / 100.0;
        if let Ok((h, b)) = text_fonts(&self.dw, &self.coll) { self.fonts.head = h; self.fonts.body = b; }
        self.restyle();
        self.seen.0 = -1;
        self.invalidate();
    }
    unsafe fn open_settings(&mut self) {
        self.finish_unfold();
        self.suspended_scroll = None;
        self.edit_snapshot = None;
        self.save_now();
        self.close_palette();
        if self.finding { self.close_find(); }
        if self.mode == Mode::List { self.mode = Mode::Edit; let _ = KillTimer(self.hwnd, TIMER_CARET); self.update_docked(); }
        self.settings = true;
        self.set_scroll = 0.0;
        self.ctl_open = true;
        self.end_capture();
        self.hk_err = None;
        self.pick = None;
        self.autostart = settings::autostart();
        self.hide_edit();
        let _ = SetFocus(self.hwnd);
        self.retarget();
    }
    unsafe fn close_settings(&mut self) {
        self.settings = false;
        self.pick = None;
        self.end_capture();
        self.retarget();
    }
    /// an in-window binding fired (2.. in KEY_NAMES order)
    unsafe fn action(&mut self, i: usize) {
        match i {
            2 => self.new_note(""),
            3 => { if self.mode == Mode::List { let id = self.cur.clone(); self.open_note(&id); } else { self.enter_list(); } }
            4 => { if self.finding { self.close_find(); } else { self.open_find(); } }
            5 => { if self.settings { self.close_settings(); } else { self.open_settings(); } }
            6 => self.toggle_pin(),
            7 => { let _ = DestroyWindow(self.hwnd); }
            8 => self.palette_by_key(),
            9 => self.delete_by_key(),
            10 => self.toggle_format(md::Format::Highlight),
            _ => {}
        }
    }
    /// the colour palette from the keyboard: for the open note, or the picked folder in the drawer
    unsafe fn palette_by_key(&mut self) {
        if self.palette.is_some() { self.close_palette(); return; }
        if self.settings { return; }
        if self.mode == Mode::List {
            let Some(id) = self.shown.get(self.sel).cloned() else { return };
            let top = self.anims.get(&id).map_or(T, |a| fold_rect(&a.cur).1);
            self.open_palette(id, width() / 2.0, top + T + 8.0);
        } else {
            let id = self.cur.clone();
            // the dot is the last button once the palette is open
            self.palette = Some(id.clone());
            let k = self.controls().len() - 1;
            self.palette = None;
            self.open_palette(id, ctl_x(k), T + 8.0);
        }
    }
    /// keys while the palette is open: arrows pick the colour, digits jump to one, Enter closes
    unsafe fn palette_key(&mut self, vk: u16) -> bool {
        let Some(id) = self.palette.clone() else { return false };
        let n = ncards();
        let c = self.note(&id).map(|n| n.color).unwrap_or(0).min(n - 1);
        let to = match VIRTUAL_KEY(vk) {
            VK_LEFT | VK_UP => (c + n - 1) % n,
            VK_RIGHT | VK_DOWN => (c + 1) % n,
            VK_HOME => 0,
            VK_END => n - 1,
            VK_RETURN | VK_SPACE => { self.close_palette(); return true; }
            _ if (0x31..=0x39).contains(&vk) && ((vk - 0x31) as usize) < n => (vk - 0x31) as usize,
            _ => return false,
        };
        self.set_color(&id, to);
        true
    }
    unsafe fn move_notes(&mut self, to: PathBuf) {
        let from = &self.dir;
        let result = (|| {
            let note = self.cur_note();
            if !note.is_empty() { store::save(from, note)?; }
            store::move_notes(from, &to, || settings::set_notes_dir(&to))
        })();
        match result {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                let title = wide(ui::MOVE_FAILED.get());
                let message = wide(&format!("{}\n\n{error}", ui::MOVE_RETRY.get()));
                MessageBoxW(self.hwnd, PCWSTR(message.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR);
                return;
            }
        }
        let _ = KillTimer(self.hwnd, TIMER_SAVE);
        for f in [".pane-state", ".pane-size"] { if std::fs::copy(self.dir.join(f), to.join(f)).is_ok() { let _ = std::fs::remove_file(self.dir.join(f)); } }
        self.dir = to;
        // notes that were already in the new folder join in
        let mut loaded = store::load(&self.dir);
        for n in &self.notes { if let Some(l) = loaded.iter_mut().find(|l| l.id == n.id) { l.opened = n.opened; } }
        if loaded.is_empty() { loaded = self.notes.clone(); }
        self.notes = loaded;
        if self.note(&self.cur).is_none() { self.cur = self.notes[0].id.clone(); }
        self.invalidate();
    }
    unsafe fn toggle_pin(&mut self) {
        self.pinned = !self.pinned;
        // remembered: the next start opens pinned or not, as it was left
        let mut p = self.prefs;
        p.pinned = self.pinned;
        self.apply_prefs(p);
        let _ = SetWindowPos(self.hwnd, if self.pinned { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        self.invalidate();
    }
    /// the global hotkeys (show/hide, new note); returns which ones failed
    unsafe fn register_globals(&mut self) -> [bool; 2] {
        let mut bad = [false; 2];
        for i in 0..2 {
            let _ = UnregisterHotKey(self.hwnd, i as i32 + 1);
            let (m, vk) = self.prefs.keys[i];
            if vk != 0 { bad[i] = RegisterHotKey(self.hwnd, i as i32 + 1, HOT_KEY_MODIFIERS(m | MOD_NOREPEAT.0), vk).is_err(); }
        }
        bad
    }
    /// a binding listens for a new combination; the global ones step aside meanwhile so they can be typed
    unsafe fn start_capture(&mut self, i: usize) {
        self.capturing = Some(i);
        self.hk_err = None;
        let _ = UnregisterHotKey(self.hwnd, 1);
        let _ = UnregisterHotKey(self.hwnd, 2);
    }
    unsafe fn end_capture(&mut self) {
        if self.capturing.take().is_some() { self.register_globals(); }
    }
    /// a key pressed while a binding listens
    unsafe fn capture_key(&mut self, vk: u32) {
        if matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5) { return; }
        let Some(i) = self.capturing else { return };
        if vk == VK_ESCAPE.0 as u32 { self.end_capture(); self.invalidate(); return; }
        let mods = cur_mods();
        let mut p = self.prefs;
        let old = p.keys[i];
        if mods == 0 && (vk == VK_BACK.0 as u32 || vk == VK_DELETE.0 as u32) { p.keys[i] = (0, 0); }
        else if mods == 0 && !(0x70..=0x87).contains(&vk) { return; } // a plain key would steal typing: keep listening
        else {
            p.keys[i] = (mods, vk);
            // one combination, one action
            for j in 0..KEYS { if j != i && p.keys[j] == (mods, vk) { p.keys[j] = (0, 0); } }
        }
        self.capturing = None;
        self.apply_prefs(p);
        let bad = self.register_globals();
        if i < 2 && bad[i] {
            let mut p = self.prefs;
            p.keys[i] = old;
            self.apply_prefs(p);
            self.register_globals();
            self.hk_err = Some(i);
        }
        self.update_tray_tip();
        self.invalidate();
    }
    unsafe fn update_tray_tip(&mut self) {
        self.tray.szTip = [0; 128];
        let (m, vk) = self.prefs.keys[0];
        for (i, c) in format!("Notedd · {}", combo_name(m, vk)).encode_utf16().take(127).enumerate() { self.tray.szTip[i] = c; }
        let _ = Shell_NotifyIconW(NIM_MODIFY, &self.tray);
    }

    // ---------- showing and hiding: slide in from a side, or fade in place ----------
    fn rate(&self) -> f32 { [24.0, 14.0, 8.5][self.prefs.speed.min(2) as usize] }
    unsafe fn monitor_rect(&self) -> RECT {
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
        mi.rcMonitor
    }
    /// how far from its resting place the window starts (and ends when hiding), px
    unsafe fn slide_offset(&self, wr: &RECT) -> (f32, f32) {
        let near = 90.0 * self.scale;
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
    fn fades(&self) -> bool { self.prefs.dir == 4 || !self.prefs.far }
    fn front_id(&self) -> String { if self.settings { SETTINGS.to_string() } else { self.cur.clone() } }
    unsafe fn place_slide(&self) {
        let _ = SetWindowPos(self.hwnd, None, self.slide_base.x + self.slide.0.v.round() as i32, self.slide_base.y + self.slide.1.v.round() as i32, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }

    // ---------- hit testing ----------
    fn dips(&self, lp: LPARAM, screen: bool) -> (f32, f32) {
        let mut p = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
        if screen { unsafe { let _ = ScreenToClient(self.hwnd, &mut p); } }
        (p.x as f32 / self.scale - 1.0, p.y as f32 / self.scale - 1.0 - self.canvas_offset())
    }
    fn control_at(&self, x: f32, y: f32) -> Option<usize> {
        if !(0.0..T).contains(&y) || x > width() { return None; }
        let k = ((width() - x + 1.0) / 34.0).floor();
        if k >= 0.0 && (k as usize) < self.controls().len() { Some(k as usize) } else { None }
    }
    /// The tab and the two shoulders form the visible upper edge of the folder.
    fn edge_at(&self, x: f32, y: f32) -> u8 {
        if self.mode != Mode::Edit || (!self.edit_visible && !self.set_visible()) || self.finding { return 0; }
        let (w, h) = (width(), eh());
        if y < -1.0 || y > h + 1.0 || x < -1.0 || x > w + 1.0 { return 0; }
        let (tab, tab_end) = tab_x();
        let tab_end = tab_end + if self.settings { 0.0 } else { self.dark.v };
        let corner = (T - 1.0..T + R + 1.0).contains(&y);
        let upper_left = corner && x < R + GRIP;
        let upper_right = corner && x > w - R - GRIP;
        let tab_top = y < GRIP && (tab..tab_end).contains(&x);
        let shoulder = (T - 1.0..T + GRIP).contains(&y) && (x < tab - F || x > tab_end + F);
        let mut e = 0;
        if (upper_left || (y >= T + R && x < GRIP)) && self.docks & 1 == 0 { e |= 1; }
        if (upper_right || (y >= T + R && x > w - GRIP)) && self.docks & 2 == 0 { e |= 2; }
        if y > h - GRIP && self.docks & 4 == 0 { e |= 4; }
        if tab_top || shoulder || upper_left || upper_right { e |= 8; }
        e
    }
    unsafe fn resize_drag(&mut self, origin: POINT, bounds: RECT, edge: u8, pointer: POINT) {
        let work = self.work_area();
        let s = self.scale;
        let dx = (pointer.x - origin.x) as f32 / s;
        let dy = (pointer.y - origin.y) as f32 / s;
        let w0 = (bounds.right - bounds.left) as f32 / s;
        let h0 = (bounds.bottom - bounds.top) as f32 / s - PADB;
        let w = if edge & 3 == 0 { w0 } else {
            let delta = if edge & 1 != 0 { -dx } else { dx };
            (w0 + delta).clamp(MIN_W, ((work.right - work.left) as f32 / s).max(MIN_W))
        };
        let h = if edge & 8 != 0 {
            (h0 - dy).clamp(MIN_H, ((bounds.bottom - work.top) as f32 / s - PADB).max(h0).max(MIN_H))
        } else if edge & 4 != 0 {
            (h0 + dy).clamp(MIN_H, ((work.bottom - bounds.top) as f32 / s).max(h0).max(MIN_H))
        } else { h0 };
        let left = if edge & 1 != 0 { bounds.right - (w * s).round() as i32 } else { bounds.left };
        let top = if edge & 8 != 0 { bounds.bottom - ((h + PADB) * s).round() as i32 } else { bounds.top };
        self.resize_to(left, top, w, h);
    }
    unsafe fn resize_to(&mut self, left: i32, top: i32, w: f32, h: f32) {
        self.finish_unfold();
        self.edit_snapshot = None;
        let pics = (w - ww()).abs() > 0.5 && self.pstyle.iter().any(|p| matches!(p.kind, PK::Image(..)));
        W_ = w; EH_ = h;
        let s = self.scale;
        let px = |v: f32| (v * s).round() as i32;
        let _ = MoveWindow(self.edit, px(PAD + RE_DX), px(EDIT_TOP + RE_DY), px(w - 2.0 * PAD), px(h - EDIT_TOP - 24.0), true);
        if pics { self.restyle(); }
        let id = self.cur.clone();
        if let Some(a) = self.anims.get_mut(&id) { a.to.h = h; a.cur.h = h; }
        if let Some(a) = self.anims.get_mut(SETTINGS) { a.to.h = h; a.cur.h = h; }
        self.win_h = h + PADB;
        self.win_h_to = self.win_h;
        self.win_y = top as f32;
        self.win_y_to = self.win_y;
        self.edit_top = top;
        let _ = SetWindowPos(self.hwnd, None, left, top, px(w), px(self.win_h), SWP_NOZORDER | SWP_NOACTIVATE);
        if let Some(t) = &self.target { let _ = t.Resize(&D2D_SIZE_U { width: px(w) as u32, height: px(self.win_h) as u32 }); }
        let _ = RedrawWindow(self.hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
    }
    fn on_tab(&self, x: f32, y: f32) -> bool {
        let (x2, bx) = tab_x();
        self.mode == Mode::Edit && !self.finding && (0.0..T).contains(&y) && x >= x2 && x < bx
    }
    fn on_plus(&self, x: f32, y: f32) -> bool {
        let (_, bx) = tab_x();
        self.mode == Mode::Edit && !self.finding && (0.0..T).contains(&y) && x >= bx && x < bx + self.dark.v + 4.0
    }
    /// find buttons in the dark tab: 0 close, 1 next, 2 previous
    fn find_btn(&self, x: f32, y: f32) -> Option<usize> {
        if !(self.mode == Mode::Edit && self.finding && (0.0..T).contains(&y)) { return None; }
        let d = tab_x().1 + self.dark.v - x;
        if (4.0..30.0).contains(&d) { Some(0) } else if (30.0..56.0).contains(&d) { Some(1) } else if (56.0..82.0).contains(&d) { Some(2) } else { None }
    }
    fn folder_at(&self, x: f32, y: f32) -> Option<String> {
        let (x2, bx) = tab_x();
        let mut items: Vec<(i32, &String)> = self.anims.iter().filter(|(_, a)| a.cur.o > 0.5 && a.to.o > 0.0).map(|(k, a)| (a.z, k)).collect();
        items.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, id) in items {
            let v = &self.anims[id].cur;
            let (l, tp, r, b) = fold_rect(v);
            let lx = (x - l) / v.sc;
            let in_tab = lx >= x2 && lx <= bx + F;
            let top = tp + if in_tab { 0.0 } else { T * v.sc };
            if x >= l && x <= r && y >= top && y <= b { return Some(id.clone()); }
        }
        None
    }
    unsafe fn on_hover(&mut self, x: f32, y: f32) {
        let hc = self.control_at(x, y);
        if hc != self.hover_ctl { self.hover_ctl = hc; self.invalidate(); }
        let dot = hc.map_or(false, |k| self.controls()[k] == Ctl::Dot);
        let dot_to = if dot { 1.15 } else { 1.0 };
        let mut changed = dot_to != self.dot_sc.to;
        self.dot_sc.to = dot_to;
        if self.settings {
            let h = self.set_hit(x, y);
            if h != self.hover_set { self.hover_set = h; self.invalidate(); }
        }
        if self.mode == Mode::Edit {
            let hp = self.on_plus(x, y);
            if hp != self.hover_plus { self.hover_plus = hp; self.dark.to = if self.finding { DARK_FIND } else if hp { DARK_HOVER } else { DARK }; changed = true; }
            let ht = self.on_tab(x, y);
            if ht != self.hover_tab { self.hover_tab = ht; self.tab_hl.to = if ht { 1.0 } else { 0.0 }; changed = true; }
            let fb = self.find_btn(x, y);
            if fb != self.hover_fbtn { self.hover_fbtn = fb; self.invalidate(); }
        } else if let Some(id) = self.folder_at(x, y).filter(|_| self.mouse_moved) {
            if let Some(i) = self.shown[self.list_start..self.list_start + self.list_len].iter().position(|s| *s == id) {
                let i = i + self.list_start;
                if i != self.sel { self.sel = i; self.layout_stack(false, 0); }
            }
        }
        if changed { self.start_anim(); }
    }
    fn hand_at(&self, x: f32, y: f32) -> bool {
        (unsafe { self.set_hit(x, y).is_some() || self.pick_hit(x, y).is_some() }) || self.control_at(x, y).is_some() || self.on_tab(x, y) || self.on_plus(x, y) || self.find_btn(x, y).is_some()
            || (self.mode == Mode::List && self.folder_at(x, y).map_or(false, |id| id != SEARCH))
    }
}

/// modifier keys held now, as MOD_* flags
unsafe fn cur_mods() -> u32 {
    let down = |k: VIRTUAL_KEY| GetKeyState(k.0 as i32) < 0;
    let mut m = 0;
    if down(VK_CONTROL) { m |= 2; }
    if down(VK_MENU) { m |= 1; }
    if down(VK_SHIFT) { m |= 4; }
    if down(VK_LWIN) || down(VK_RWIN) { m |= 8; }
    m
}
/// window-wide keys, checked before the focused control sees them
unsafe fn shortcut(a: &mut App, k: usize, mods: u32) -> bool {
    if a.edit_menu_key(k as u16) { return true; }
    if k == VK_ESCAPE.0 as usize {
        if a.palette.is_some() { a.close_palette(); }
        else if a.pick.is_some() { a.pick = None; a.invalidate(); }
        else if a.finding { a.close_find(); }
        else if a.settings { a.close_settings(); }
        else if a.mode == Mode::List { if a.list_q.is_empty() { let id = a.cur.clone(); a.open_note(&id); } else { a.list_q.clear(); a.query_changed(); } }
        else { a.hide(); }
        return true;
    }
    if mods == 2 {
        match k as u8 {
            0xBB | 0x6B => { a.step_text(10); return true; }  // Ctrl+= / numpad +
            0xBD | 0x6D => { a.step_text(-10); return true; } // Ctrl+- / numpad -
            b'0' | 0x60 => { a.step_text(0); return true; }
            _ => {}
        }
    }
    // the in-window bindings (the first two are global hotkeys, Windows handles them)
    for i in 2..KEYS {
        let (m, vk) = a.prefs.keys[i];
        if vk != 0 && vk as usize == k && m == mods { a.action(i); return true; }
    }
    if a.prefs.simple_editor && a.edit_visible && GetFocus() == a.edit {
        if (mods, k as u16) == (0, VK_APPS.0) || (mods, k as u16) == (4, VK_F10.0) {
            a.edit_menu(LPARAM(-1));
            return true;
        }
        if (mods, k as u32) == (2, b'X' as u32) || (mods, k as u32) == (4, VK_DELETE.0 as u32) {
            SendMessageW(a.edit, WM_CUT, WPARAM(0), LPARAM(0));
            return true;
        }
        if let Some((format, _, _)) = SIMPLE_FORMATS.into_iter().find(|&(_, _, binding)| binding == (mods, k as u32)) {
            a.toggle_format(format);
            return true;
        }
    }
    false
}

unsafe fn click_control(a: &mut App, k: usize) {
    match a.controls()[k] {
        Ctl::Close => a.hide(),
        Ctl::Gear => { if a.settings { a.close_settings(); } else { a.open_settings(); } }
        Ctl::Pin => a.toggle_pin(),
        Ctl::More => { a.ctl_open = !a.ctl_open; if !a.ctl_open { a.close_palette(); } a.invalidate(); }
        Ctl::List => { if a.mode == Mode::List { let id = a.cur.clone(); a.open_note(&id); } else { a.enter_list(); } }
        Ctl::Find => { if a.finding { a.close_find(); } else { a.open_find(); } }
        Ctl::New => a.new_note(""),
        Ctl::Trash => { let id = a.cur.clone(); a.delete_note(&id); }
        Ctl::Dot => { if a.palette.is_some() { a.close_palette(); } else { let id = a.cur.clone(); a.open_palette(id, ctl_x(k), T + 8.0); } }
    }
}
fn key_name(vk: u32) -> String {
    match vk {
        0x20 => "Space".into(), 0x0D => "Enter".into(), 0x09 => "Tab".into(), 0x08 => "Backspace".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        // punctuation by its US name: the keyboard layout would turn Ctrl+, into Ctrl+Б
        0xBC => ",".into(), 0xBE => ".".into(), 0xBF => "/".into(), 0xBA => ";".into(), 0xDE => "'".into(),
        0xDB => "[".into(), 0xDD => "]".into(), 0xDC => "\\".into(), 0xC0 => "`".into(), 0xBD => "-".into(), 0xBB => "=".into(),
        0x2E => "Delete".into(), 0x2D => "Insert".into(), 0x24 => "Home".into(), 0x23 => "End".into(), 0x21 => "PgUp".into(), 0x22 => "PgDn".into(),
        0x25 => "←".into(), 0x26 => "↑".into(), 0x27 => "→".into(), 0x28 => "↓".into(),
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        _ => unsafe {
            let sc = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
            let mut buf = [0u16; 32];
            let n = GetKeyNameTextW((sc << 16) as i32, &mut buf);
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
    }
}
fn combo_name(m: u32, vk: u32) -> String {
    if vk == 0 { return ui::NOT_SET.get().into(); }
    let mut s = String::new();
    if m & 2 != 0 { s += "Ctrl+"; }
    if m & 1 != 0 { s += "Alt+"; }
    if m & 4 != 0 { s += "Shift+"; }
    if m & 8 != 0 { s += "Win+"; }
    s + &key_name(vk)
}
fn hsv_rgb(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 { 0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x), 3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x) };
    let m = v - c;
    join([(r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0])
}
fn rgb_hsv(c: u32) -> [f32; 3] {
    let [r, g, b] = split(c).map(|v| v / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d < 1e-6 { 0.0 } else if mx == r { 60.0 * ((g - b) / d).rem_euclid(6.0) } else if mx == g { 60.0 * ((b - r) / d + 2.0) } else { 60.0 * ((r - g) / d + 4.0) };
    [h, if mx < 1e-6 { 0.0 } else { d / mx }, mx]
}
fn lp_from(p: POINT) -> LPARAM { LPARAM(((p.y as u16 as isize) << 16) | p.x as u16 as isize) }

static mut OLD_EDIT: isize = 0;
/// RichEdit subclass for editor input and Markdown decorations.
extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let old: WNDPROC = std::mem::transmute(OLD_EDIT);
        if let Some(a) = app() {
            if matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL | WM_KILLFOCUS) {
                a.close_edit_menu();
            }
            if !(msg == WM_KEYDOWN && wp.0 == VK_BACK.0 as usize) {
                if let Some(result) = a.simple_edit_key(msg, wp, lp) { return result; }
            }
            match msg {
                WM_RBUTTONUP if a.prefs.simple_editor && a.edit_visible => {
                    if GetCapture() == hwnd { let _ = ReleaseCapture(); }
                    let mut pt = POINT { x: lp.0 as i16 as i32, y: (lp.0 >> 16) as i16 as i32 };
                    let _ = ClientToScreen(hwnd, &mut pt);
                    a.edit_menu(lp_from(pt));
                    return LRESULT(0);
                }
                WM_CONTEXTMENU if a.prefs.simple_editor && a.edit_visible => { a.edit_menu(lp); return LRESULT(0); }
                WM_PASTE => { a.edit_paste(); return LRESULT(0); }
                WM_DROPFILES => { a.drop_files(HDROP(wp.0 as _)); return LRESULT(0); }
                WM_MOUSEWHEEL if wp.0 & 0x0008 == 0 => { a.scroll_wheel(wp); return LRESULT(0); }
                WM_LBUTTONDOWN if a.edit_click(lp) => return LRESULT(0),
                WM_ERASEBKGND if a.edit_visible => return LRESULT(1),
                WM_PAINT if a.edit_visible => {
                    let mut ps = PAINTSTRUCT::default();
                    let target = BeginPaint(hwnd, &mut ps);
                    if !target.is_invalid() && ps.rcPaint.left < ps.rcPaint.right && ps.rcPaint.top < ps.rcPaint.bottom {
                        let mut dc = HDC::default();
                        let buffer = BeginBufferedPaint(target, &ps.rcPaint, BPBF_COMPATIBLEBITMAP, None, &mut dc);
                        if buffer == 0 { dc = target; }
                        let paper = CreateSolidBrush(COLORREF(bgr(a.shown_color(&a.cur))));
                        FillRect(dc, &ps.rcPaint, paper);
                        let _ = DeleteObject(paper);
                        CallWindowProcW(old, hwnd, WM_PRINTCLIENT, WPARAM(dc.0 as usize), LPARAM(PRF_CLIENT as isize));
                        a.draw_marks_on(dc);
                        if buffer != 0 { let _ = EndBufferedPaint(buffer, true); }
                    }
                    let _ = EndPaint(hwnd, &ps);
                    return LRESULT(0);
                }
                WM_SETCURSOR if a.edit_hand() => {
                    if let Ok(c) = LoadCursorW(None, IDC_HAND) { SetCursor(c); }
                    return LRESULT(1);
                }
                _ => {}
            }
        }
        CallWindowProcW(old, hwnd, msg, wp, lp)
    }
}
unsafe fn ink_dc() -> HDC {
    let dc = CreateCompatibleDC(None);
    let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: 1, biHeight: 1, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
    let mut bits = std::ptr::null_mut();
    if let Ok(b) = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
        *(bits as *mut u32) = ink();
        SelectObject(dc, b);
    }
    dc
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(a) = app() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        match msg {
            WM_MOUSEWHEEL if a.mode == Mode::List => { a.scroll_list(wp); LRESULT(0) }
            WM_MOUSEWHEEL if a.settings => {
                a.scroll_settings(-f32::from((wp.0 >> 16) as i16) * 0.4);
                LRESULT(0)
            }
            WM_MOUSEWHEEL if a.edit_visible && wp.0 & 0x0008 == 0 => {
                a.scroll_wheel(wp);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                // while animating the loop draws every frame itself
                if !a.animating { let _ = a.render(); }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SETCURSOR => {
                if wp.0 as isize == hwnd.0 as isize && (lp.0 & 0xffff) as u32 == HTCLIENT {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    let (x, y) = a.dips(lp_from(p), true);
                    let e = if a.press_size.is_some() { a.press_size.as_ref().unwrap().2 } else { a.edge_at(x, y) };
                    let id = match e { 1 | 2 => IDC_SIZEWE, 4 | 8 => IDC_SIZENS, 5 | 10 => IDC_SIZENESW, 6 | 9 => IDC_SIZENWSE, _ => if a.hand_at(x, y) { IDC_HAND } else { IDC_ARROW } };
                    if let Ok(c) = LoadCursorW(None, id) { SetCursor(c); }
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_MOVING => { a.on_moving(&mut *(lp.0 as *mut RECT)); LRESULT(1) }
            WM_ENTERSIZEMOVE => {
                a.finish_unfold();
                let (mut c, mut r) = (POINT::default(), RECT::default());
                let _ = GetCursorPos(&mut c);
                let _ = GetWindowRect(hwnd, &mut r);
                a.drag0 = Some((c, r));
                a.win_y = r.top as f32;
                a.win_y_to = a.win_y;
                LRESULT(0)
            }
            WM_EXITSIZEMOVE => { a.end_drag(); LRESULT(0) }
            WM_DISPLAYCHANGE => {
                a.finish_unfold();
                a.drag0 = None;
                let mut wr = RECT::default();
                let _ = GetWindowRect(hwnd, &mut wr);
                if a.mode == Mode::Edit { a.edit_top = wr.top; }
                a.win_y = wr.top as f32;
                a.win_y_to = a.win_y;
                a.update_docked();
                if a.mode == Mode::List { a.layout_stack(false, 0); }
                LRESULT(0)
            }
            WM_ACTIVATE => { if wp.0 & 0xffff == 0 { a.close_palette(); a.close_edit_menu(); } LRESULT(0) }
            WM_MOUSEMOVE => {
                if !a.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    a.tracking = true;
                }
                if let Some(part) = a.press_pick { let (x, y) = a.dips(lp, false); a.pick_drag(part, x, y); return LRESULT(0); }
                if let Some(grab) = a.set_scroll_drag {
                    let (_, y) = a.dips(lp, false);
                    a.drag_settings_scroll(y, grab);
                    return LRESULT(0);
                }
                // a press on the tab that starts moving becomes a window drag
                if let Some((c0, r0, e)) = a.press_size {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    a.resize_drag(c0, r0, e, p);
                    return LRESULT(0);
                }
                if let Some((p0, _)) = &a.press_folder {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    if (p.x - p0.x).abs() > 4 || (p.y - p0.y).abs() > 4 {
                        a.press_folder = None;
                        let _ = ReleaseCapture();
                        SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), lp_from(p));
                        return LRESULT(0);
                    }
                }
                if let Some(p0) = a.press_tab {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    if (p.x - p0.x).abs() > 4 || (p.y - p0.y).abs() > 4 {
                        a.press_tab = None;
                        let _ = ReleaseCapture();
                        a.hover_tab = false;
                        a.tab_hl.to = 0.0;
                        a.start_anim();
                        SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), lp_from(p));
                        return LRESULT(0);
                    }
                }
                // Windows also sends a move when the folders slide under a still cursor;
                // only a real move may pick a folder, or it steals the keyboard's choice
                let mut cp = POINT::default();
                let _ = GetCursorPos(&mut cp);
                a.mouse_moved = (cp.x, cp.y) != a.last_cursor;
                a.last_cursor = (cp.x, cp.y);
                let (x, y) = a.dips(lp, false);
                a.on_hover(x, y);
                LRESULT(0)
            }
            0x02A3 => { // WM_MOUSELEAVE
                a.tracking = false;
                // the extra buttons tuck back behind the arrow once the pointer leaves
                if a.ctl_open && a.palette.is_none() && !a.settings { a.ctl_open = false; a.invalidate(); }
                a.on_hover(-100.0, -100.0);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                a.close_edit_menu();
                let (x, y) = a.dips(lp, false);
                let e = a.edge_at(x, y);
                if e != 0 {
                    a.finish_unfold();
                    a.x_glide = false;
                    let (mut c, mut r) = (POINT::default(), RECT::default());
                    let _ = GetCursorPos(&mut c);
                    let _ = GetWindowRect(hwnd, &mut r);
                    a.press_size = Some((c, r, e));
                    a.close_palette();
                    a.hide_edit();
                    SetCapture(hwnd);
                    return LRESULT(0);
                }
                if a.begin_settings_scroll(x, y) { SetCapture(hwnd); return LRESULT(0); }
                if let Some(k) = a.control_at(x, y) {
                    if a.controls()[k] != Ctl::Dot { a.close_palette(); }
                    click_control(a, k);
                    return LRESULT(0);
                }
                a.close_palette();
                if let Some(part) = a.pick_hit(x, y) { a.press_pick = Some(part); a.pick_drag(part, x, y); SetCapture(hwnd); return LRESULT(0); }
                if let Some((r, i)) = a.set_hit(x, y) { a.kb_focus = false; a.focus = (r, i); a.set_click(r, i); return LRESULT(0); }
                if a.on_tab(x, y) {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    a.press_tab = Some(p);
                    SetCapture(hwnd);
                    return LRESULT(0);
                }
                if a.on_plus(x, y) { a.press_plus = true; a.plus_sc.to = 0.85; a.start_anim(); SetCapture(hwnd); return LRESULT(0); }
                if let Some(b) = a.find_btn(x, y) { match b { 0 => a.close_find(), 1 => a.step_hit(1), _ => a.step_hit(-1) } return LRESULT(0); }
                if a.mode == Mode::List {
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    a.press_folder = Some((p, a.folder_at(x, y).unwrap_or_else(|| SEARCH.to_string())));
                    SetCapture(hwnd);
                } else if !a.finding { let _ = SetFocus(a.edit); }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let (x, y) = a.dips(lp, false);
                if a.set_scroll_drag.take().is_some() { let _ = ReleaseCapture(); return LRESULT(0); }
                if a.press_tab.take().is_some() { let _ = ReleaseCapture(); a.enter_list(); }
                if a.press_pick.take().is_some() { let _ = ReleaseCapture(); let c = a.pick_color(); a.set_pick_color(c, true); }
                if a.press_size.take().is_some() {
                    let _ = ReleaseCapture();
                    let mut wr = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut wr);
                    a.edit_top = wr.top;
                    a.win_y = wr.top as f32;
                    a.win_y_to = a.win_y;
                    a.update_docked();
                    store::write_size(&a.dir, ww(), eh());
                    if !a.settings { a.reveal_edit(); }
                }
                if let Some((_, id)) = a.press_folder.take() {
                    let _ = ReleaseCapture();
                    if id != SEARCH && a.folder_at(x, y).as_deref() == Some(id.as_str()) { a.open_note(&id); }
                }
                if a.press_plus {
                    a.press_plus = false;
                    let _ = ReleaseCapture();
                    a.plus_sc.to = 1.0;
                    a.start_anim();
                    if a.on_plus(x, y) { a.new_note(""); }
                }
                LRESULT(0)
            }
            WM_CAPTURECHANGED => { a.set_scroll_drag = None; DefWindowProcW(hwnd, msg, wp, lp) }
            WM_RBUTTONDOWN => {
                a.close_edit_menu();
                let (x, y) = a.dips(lp, false);
                if a.mode == Mode::List { if let Some(id) = a.folder_at(x, y) { if id != SEARCH { a.open_palette(id, x, y); } } }
                LRESULT(0)
            }
            WM_MOVE => { a.close_edit_menu(); if a.palette.is_some() { a.draw_palette(); } LRESULT(0) }
            WM_CHAR => {
                if a.settings && a.pick.is_some() { a.pick_char(wp.0 as u16); return LRESULT(0); }
                if !a.input_mode() || a.settings { return LRESULT(0); }
                let c = wp.0 as u16;
                match c {
                    8 => { a.query().pop(); a.query_changed(); }
                    127 => { // ctrl+backspace: drop the last word
                        let q = a.query();
                        while q.last() == Some(&32) { q.pop(); }
                        while q.last().map(|&c| c != 32).unwrap_or(false) { q.pop(); }
                        a.query_changed();
                    }
                    c if c >= 32 => { a.query().push(c); a.query_changed(); }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                let k = wp.0 as u16;
                if a.input_mode() && ctrl && k == 'V' as u16 { a.paste(); return LRESULT(0); }
                if a.finding && k == VK_RETURN.0 { a.step_hit(if shift { -1 } else { 1 }); }
                if a.mode == Mode::List {
                    if k == VK_DELETE.0 && !ctrl && !shift && a.palette.is_none() { a.delete_by_key(); return LRESULT(0); }
                    if k == 'Z' as u16 && ctrl && !shift { a.undo_delete(); return LRESULT(0); }
                    let last = a.shown.len().saturating_sub(1);
                    let selected = match VIRTUAL_KEY(k) {
                        VK_UP => (a.sel + 1).min(last),
                        VK_DOWN => a.sel.saturating_sub(1),
                        VK_HOME => 0,
                        VK_END => last,
                        _ => a.sel,
                    };
                    if selected != a.sel { a.sel = selected; a.layout_stack(false, 0); }
                    if k == VK_RETURN.0 {
                        if let Some(id) = a.shown.get(a.sel).cloned() { a.open_note(&id); }
                        else if !a.list_q.is_empty() { let q = String::from_utf16_lossy(&a.list_q).trim().to_string(); a.new_note(&q); }
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let code = ((wp.0 >> 16) & 0xffff) as u32;
                if code == EN_CHANGE { a.on_edit_change(); }
                if code == EN_VSCROLL && a.finding { a.update_hits(); }
                LRESULT(0)
            }
            // wheel over RichEdit: redraw the highlights once the scroll has happened
            WM_NOTIFY => { if a.finding { let _ = PostMessageW(hwnd, WM_HITS, WPARAM(0), LPARAM(0)); } LRESULT(0) }
            WM_HITS => { a.update_hits(); LRESULT(0) }
            WM_APPEAR => { a.appear(); LRESULT(0) }
            WM_DROPPED => { a.dropped(); LRESULT(0) }
            WM_TIMER => {
                match wp.0 {
                    // only when the loop is stuck in a modal one (window drag, menu)
                    TIMER_ANIM => { if a.last_frame.elapsed().as_millis() > 12 { a.frame(); } }
                    TIMER_SAVE => a.save_now(),
                    TIMER_CARET => { a.caret_on = !a.caret_on; a.invalidate(); }
                    TIMER_EDIT_MENU => a.draw_edit_menu(),
                    TIMER_ECARET => {
                        let now = Instant::now();
                        let dt = a.car.last.map(|l| (now - l).as_secs_f32()).unwrap_or(0.0).min(0.1);
                        a.car.last = Some(now);
                        a.tick_caret(dt);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_HOTKEY => { if wp.0 == 2 { a.action(2); a.appear(); } else { a.toggle_window(); } LRESULT(0) }
            WM_SYSCHAR if a.capturing.is_some() => LRESULT(0),
            WM_SETTINGCHANGE => { if a.prefs.theme == 2 { a.apply_theme(); } DefWindowProcW(hwnd, msg, wp, lp) }
            // lets a script drive the window without global key input: wParam = key, lParam = ctrl
            WM_TEST => { shortcut(a, wp.0, if lp.0 != 0 { 2 } else { 0 }); LRESULT(0) }
            WM_TRAY => {
                let ev = (lp.0 & 0xffff) as u32;
                if ev == WM_LBUTTONUP { a.toggle_window(); }
                if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                    let menu = CreatePopupMenu().unwrap();
                    let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(wide(ui::SHOW.get()).as_ptr()));
                    let _ = AppendMenuW(menu, MF_STRING, 2, PCWSTR(wide(ui::OPEN_NOTES_FOLDER.get()).as_ptr()));
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
                    let _ = AppendMenuW(menu, MF_STRING, 3, PCWSTR(wide(ui::QUIT.get()).as_ptr()));
                    let mut p = POINT::default();
                    let _ = GetCursorPos(&mut p);
                    let _ = SetForegroundWindow(hwnd);
                    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, None).0;
                    let _ = DestroyMenu(menu);
                    match cmd {
                        1 => a.appear(),
                        2 => { let d = wide(&a.dir.to_string_lossy()); ShellExecuteW(hwnd, w!("open"), PCWSTR(d.as_ptr()), None, None, SW_SHOWNORMAL); }
                        3 => { let _ = DestroyWindow(hwnd); }
                        _ => {}
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                a.save_now();
                let _ = Shell_NotifyIconW(NIM_DELETE, &a.tray);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// the palette popup
extern "system" fn pop_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(a) = app() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        if let Some(result) = a.edit_menu_mouse(hwnd, msg, lp) { return result; }
        match msg {
            WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
            WM_SETCURSOR => { if let Ok(c) = LoadCursorW(None, IDC_HAND) { SetCursor(c); } LRESULT(1) }
            WM_MOUSEMOVE => {
                if !a.pal_tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    a.pal_tracking = true;
                }
                let (action, _) = a.pal_hit(lp);
                if action != a.pal_hover {
                    a.pal_hover = action;
                    for (i, s) in a.sw_sc.iter_mut().enumerate() { s.to = if action == Some(PalAction::Color(i)) { 1.12 } else { 1.0 }; }
                    a.draw_palette();
                    a.start_anim();
                }
                LRESULT(0)
            }
            0x02A3 => {
                a.pal_tracking = false;
                a.pal_hover = None;
                a.pal_delete_pressed = false;
                for s in a.sw_sc.iter_mut() { s.to = 1.0; }
                a.draw_palette();
                a.start_anim();
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (action, inside) = a.pal_hit(lp);
                a.pal_delete_pressed = action == Some(PalAction::Delete);
                if let (Some(PalAction::Color(i)), Some(id)) = (action, a.palette.clone()) { a.set_color(&id, i); }
                else if !inside { a.close_palette(); }
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                if std::mem::take(&mut a.pal_delete_pressed) && a.pal_hit(lp).0 == Some(PalAction::Delete) {
                    if let Some(id) = a.palette.clone() { a.delete_note(&id); }
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
extern "system" fn plain_proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT { unsafe { DefWindowProcW(h, m, w, l) } }

/// heading and body formats of the note text at the current size: CSS-like line boxes, glyphs in the middle
unsafe fn text_fonts(dw: &IDWriteFactory5, coll: &IDWriteFontCollection1) -> Result<(IDWriteTextFormat, IDWriteTextFormat)> {
    let lead = DWRITE_TEXT_ALIGNMENT_LEADING;
    let head = make_format(dw, coll, 21.0 * ts(), DWRITE_FONT_WEIGHT_BOLD, lead, false, true, false)?;
    let body = make_format(dw, coll, 16.0 * ts(), DWRITE_FONT_WEIGHT_NORMAL, lead, false, true, false)?;
    let mut fm = DWRITE_FONT_METRICS::default();
    coll.GetFontFamily(0)?.GetFirstMatchingFont(DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL)?.GetMetrics(&mut fm);
    let (asc, desc) = (fm.ascent as f32 / fm.designUnitsPerEm as f32, fm.descent as f32 / fm.designUnitsPerEm as f32);
    let base = |size: f32, lh: f32| (lh - (asc + desc) * size) / 2.0 + asc * size;
    head.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, lh_title(), base(21.0 * ts(), lh_title()))?;
    body.SetLineSpacing(DWRITE_LINE_SPACING_METHOD_UNIFORM, lh_body(), base(16.0 * ts(), lh_body()))?;
    Ok((head, body))
}
unsafe fn make_format(dw: &IDWriteFactory5, coll: &IDWriteFontCollection1, size: f32, weight: DWRITE_FONT_WEIGHT,
    align: DWRITE_TEXT_ALIGNMENT, center: bool, wrap: bool, ellipsis: bool) -> Result<IDWriteTextFormat> {
    let f = dw.CreateTextFormat(w!("Geologica"), coll, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("ru-ru"))?;
    f.cast::<IDWriteTextFormat1>()?.SetFontFallback(&dw.GetSystemFontFallback()?)?;
    f.SetTextAlignment(align)?;
    if center { f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?; }
    if !wrap { f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?; }
    if ellipsis {
        let sign = dw.CreateEllipsisTrimmingSign(&f)?;
        f.SetTrimming(&DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 }, &sign)?;
    }
    Ok(f)
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // the folder picker is a COM object
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        // one instance only: a second launch just shows the running one
        if let Ok(h) = FindWindowW(w!("NoteddFolder"), None) {
            if !h.0.is_null() { let _ = PostMessageW(h, WM_APPEAR, WPARAM(0), LPARAM(0)); return Ok(()); }
        }

        let instance = GetModuleHandleW(None)?;
        LoadLibraryW(w!("Msftedit.dll"))?;
        let buffered_paint = BufferedPaintInit().is_ok();

        let tmp = std::env::temp_dir();
        let font_path = tmp.join("notedd-geologica.ttf");
        if std::fs::metadata(&font_path).map(|m| m.len() as usize != FONT.len()).unwrap_or(true) { let _ = std::fs::write(&font_path, FONT); }
        let icon_path = tmp.join("notedd.ico");
        if std::fs::read(&icon_path).as_deref().ok() != Some(ICON) {
            let _ = std::fs::write(&icon_path, ICON);
        }
        let iw = wide(&icon_path.to_string_lossy());
        let app_icon = |size: i32| {
            // The Explorer icon group is also the source for the live window and tray.
            let resource = PCWSTR(1usize as *const u16);
            LoadImageW(HINSTANCE::from(instance), resource, IMAGE_ICON, size, size, LR_DEFAULTSIZE | LR_SHARED)
                .or_else(|_| LoadImageW(None, PCWSTR(iw.as_ptr()), IMAGE_ICON, size, size, LR_DEFAULTSIZE | LR_LOADFROMFILE))
                .map(|handle| HICON(handle.0)).unwrap_or_default()
        };
        let font_w = wide(&font_path.to_string_lossy());
        AddFontResourceExW(PCWSTR(font_w.as_ptr()), FR_PRIVATE, None);

        let prefs = settings::load();
        locale::set(prefs.language);
        TS_ = prefs.scale as f32 / 100.0;
        TH_ = make_theme(&prefs);
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dw: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        // Fractional folder motion needs antialiasing along both axes, including between rows of pixels.
        let defaults = dw.CreateRenderingParams()?;
        let text_rendering = IDWriteFactory::CreateCustomRenderingParams(&dw, defaults.GetGamma(), defaults.GetEnhancedContrast(),
            0.0, DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC)?;
        let builder = dw.CreateFontSetBuilder()?;
        builder.AddFontFile(&dw.CreateFontFileReference(PCWSTR(font_w.as_ptr()), None)?)?;
        let coll = dw.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)?;
        let lead = DWRITE_TEXT_ALIGNMENT_LEADING;
        let fonts = Fonts {
            tab: make_format(&dw, &coll, 15.0, DWRITE_FONT_WEIGHT_MEDIUM, lead, true, false, true)?,
            input: make_format(&dw, &coll, 15.0, DWRITE_FONT_WEIGHT_MEDIUM, lead, true, false, false)?,
            small: make_format(&dw, &coll, 13.0, DWRITE_FONT_WEIGHT_NORMAL, lead, true, false, true)?,
            small_r: make_format(&dw, &coll, 13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING, true, false, false)?,
            count: make_format(&dw, &coll, 13.0, DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_TEXT_ALIGNMENT_CENTER, true, false, false)?,
            count_l: make_format(&dw, &coll, 13.0, DWRITE_FONT_WEIGHT_MEDIUM, lead, true, false, false)?,
            head: text_fonts(&dw, &coll)?.0,
            body: text_fonts(&dw, &coll)?.1,
        };
        let round = factory.CreateStrokeStyle(&D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, dashCap: D2D1_CAP_STYLE_FLAT,
            lineJoin: D2D1_LINE_JOIN_ROUND, miterLimit: 10.0, dashStyle: D2D1_DASH_STYLE_SOLID, dashOffset: 0.0,
        }, None)?;
        let icons = Icons::new(&factory)?;

        let class = w!("NoteddFolder");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: class, hIcon: app_icon(0), hCursor: LoadCursorW(None, IDC_ARROW)?, ..Default::default() });
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(plain_proc), hInstance: instance.into(), lpszClassName: w!("NoteddHits"), ..Default::default() });
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(pop_proc), hInstance: instance.into(), lpszClassName: w!("NoteddPalette"), ..Default::default() });
        let scale = GetDpiForSystem() as f32 / 96.0;
        if let Some((w, h)) = store::read_size(&store::notes_dir()) { W_ = w.max(MIN_W); EH_ = h.max(MIN_H); }
        let (pw, ph) = ((ww() * scale) as i32, ((eh() + PADB) * scale) as i32);
        let x = (GetSystemMetrics(SM_CXSCREEN) - pw) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - ph) / 3;
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | if prefs.pinned { WS_EX_TOPMOST } else { WINDOW_EX_STYLE(0) }, class, w!("Notedd"), WS_POPUP | WS_CLIPCHILDREN, x, y, pw, ph, None, None, instance, None)?;
        let pref = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as _, 4);

        let px = |v: f32| (v * scale).round() as i32;
        let edit = CreateWindowExW(WINDOW_EX_STYLE(0), w!("RICHEDIT50W"), PCWSTR::null(),
            WS_CHILD | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL) as u32),
            px(PAD + RE_DX), px(EDIT_TOP + RE_DY), px(ww() - 2.0 * PAD), px(eh() - EDIT_TOP - 24.0), hwnd, None, instance, None)?;
        SendMessageW(edit, EM_EXLIMITTEXT, WPARAM(0), LPARAM(i32::MAX as isize));
        SendMessageW(edit, EM_SETEVENTMASK, WPARAM(0), LPARAM(ENM_CHANGE | ENM_SCROLL | ENM_SCROLLEVENTS));
        SendMessageW(edit, EM_SETTYPOGRAPHYOPTIONS, WPARAM(1), LPARAM(1)); // TO_ADVANCEDTYPOGRAPHY: kerning, better spacing
        let doc = {
            let mut raw: *mut std::ffi::c_void = null_mut();
            SendMessageW(edit, EM_GETOLEINTERFACE, WPARAM(0), LPARAM(&mut raw as *mut _ as isize));
            if raw.is_null() { None } else { IRichEditOle::from_raw(raw).cast::<ITextDocument>().ok() }
        };
        let hits = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE, w!("NoteddHits"), PCWSTR::null(), WS_CHILD,
            0, 0, 1, 1, hwnd, None, instance, None)?;
        let caret = CreateWindowExW(WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE, w!("NoteddHits"), PCWSTR::null(), WS_CHILD,
            0, 0, 1, 1, hwnd, None, instance, None)?;
        OLD_EDIT = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as isize);
        DragAcceptFiles(edit, true);
        // real drags go to RichEdit's own OLE drop target; its callback passes files and pictures to us.
        // RichEdit keeps a reference; ours lives as long as the app.
        let ole_callback = drop::Callback::new(hwnd, WM_DROPPED);
        SendMessageW(edit, EM_SETOLECALLBACK, WPARAM(0), LPARAM(ole_callback.as_raw() as isize));
        std::mem::forget(ole_callback);
        let pop = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST, w!("NoteddPalette"), PCWSTR::null(), WS_POPUP,
            0, 0, 1, 1, hwnd, None, instance, None)?;

        // notes from disk; a first run gets one note so the window is never empty
        let dir = store::notes_dir();
        let mut notes = store::load(&dir);
        if notes.is_empty() {
            let n = Note::new(store::new_id(), 0, ui::WELCOME.get().into(), store::now_ft());
            let _ = store::save(&dir, &n);
            notes.push(n);
        }
        let cur = store::read_state(&dir).filter(|id| notes.iter().any(|n| &n.id == id))
            .unwrap_or_else(|| notes.iter().max_by_key(|n| n.mtime).unwrap().id.clone());

        let mut tray = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP, uCallbackMessage: WM_TRAY, ..Default::default() };
        tray.hIcon = app_icon(16);
        let _ = Shell_NotifyIconW(NIM_ADD, &tray);

        let state = Box::new(App {
            hwnd, edit, doc, hits_layer: Layer::new(&factory, hits, scale)?, caret_layer: Layer::new(&factory, caret, scale)?, pop: Layer::new(&factory, pop, scale)?, edit_menu: None,
            factory, dw, fonts, icons, round, text_rendering, target: None, scale, dir, notes, cur: cur.clone(),
            mode: Mode::Edit, finding: false, find_q: vec![], hits: vec![], hit: 0, list_q: vec![], shown: vec![], sel: 0,
            list_start: 0, list_len: 0, list_wheel: 0,
            anims: HashMap::new(), colors: HashMap::new(), kinds: vec![], load_kinds: None, force: None,
            win_h: eh() + PADB, win_h_to: eh() + PADB, dark: Ease::at(DARK), rb: Ease::at(R), tab_hl: Ease::at(0.0), dot_sc: Ease::at(1.0),
            plus_sc: Ease::at(1.0), pal_t: Ease::at(0.0), sw_sc: [Ease::at(1.0); MAX_CARDS],
            hover_ctl: None, hover_plus: false, hover_tab: false, hover_fbtn: None, press_tab: None, press_folder: None, press_size: None, drag0: None, press_plus: false, tracking: false,
            palette: None, pal_anchor: (0.0, 0.0), pal_hover: None, pal_delete_pressed: false, pal_tracking: false,
            pinned: prefs.pinned, docks: 0, rl: Ease::at(R), rr: Ease::at(R), win_x: 0.0, win_x_to: 0.0, x_glide: false, prefs, coll: coll.clone(), settings: false, set_page: 0, set_scroll: 0.0, set_scroll_drag: None, capturing: None, hk_err: None, pick: None, pick_hsv: [0.0; 3], pick_hex: vec![], press_pick: None,
            ctl_open: false, ctl_a: Ease::at(1.0), cap_n: Ease::at(4.0), more_t: Ease::at(0.0), ctl_last: vec![], ctl_prev: vec![], ctl_fade: Ease::at(1.0),
            animating: false, last_frame: Instant::now(), fps: std::env::var("PANE_FPS").ok().map(|_| (0, 0.0, 0.0, 0.0)), previews: Default::default(), seg_anim: HashMap::new(), focus: (0, 0), kb_focus: false, last_cursor: (i32::MIN, 0), mouse_moved: false,
            autostart: false, hover_set: None, demo: false, slide: (Ease::at(0.0), Ease::at(0.0)), slide_base: POINT::default(), edit_visible: false, edit_snapshot: None, suspended_scroll: None, hiding: false, guard: false, caret_on: true, tick: Instant::now(), tray,
            win_y: y as f32, win_y_to: y as f32, unfold_top: None, edit_top: y, undo: None, title_end: 0, car: Caret::default(), car_dc: ink_dc(), car_rect: RECT::default(), car_a: 0, seen: (0, 0, 0, 0, 0, false), wheel_delta: 0,
            pstyle: vec![], styled_text: vec![], links: vec![], codes: vec![], imgs: images::Images::new(), checkboxes: None,
        });
        APP = Box::into_raw(state);
        let a = app().unwrap();
        a.anims.insert(cur.clone(), Anim { cur: V { y: 0.0, sc: 1.0, h: eh(), o: 1.0, ..Default::default() }, to: V::default(), z: 1000, delay: 0.0 });
        a.set_glass(true);
        a.open_note(&cur);

        a.register_globals();
        a.update_tray_tip();
        a.appear();
        a.update_docked();

        // Sleep(1) really sleeps ~1 ms (only used to pace frames if DWM returns early)
        timeBeginPeriod(1);
        let mut msg = MSG::default();
        'run: loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT { break 'run; }
                if (msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN) && app().map_or(false, |a| a.capturing.is_some()) {
                    app().unwrap().capture_key(msg.wParam.0 as u32);
                    continue;
                }
                if msg.message == WM_MOUSEWHEEL && GetKeyState(VK_CONTROL.0 as i32) < 0 {
                    // Ctrl+wheel sizes the text our way (RichEdit's own zoom would break the line boxes)
                    let d = ((msg.wParam.0 >> 16) & 0xffff) as i16;
                    app().unwrap().step_text(if d > 0 { 10 } else { -10 });
                    continue;
                }
                if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
                    let a = app().unwrap();
                    let vk = msg.wParam.0 as u16;
                    if a.palette.is_some() && a.palette_key(vk) { continue; }
                    if a.settings_key(vk, cur_mods()) { continue; }
                    if shortcut(a, msg.wParam.0, cur_mods()) { continue; }
                    if msg.message == WM_KEYDOWN && msg.hwnd == a.edit {
                        let (ctrl, shift) = (GetKeyState(VK_CONTROL.0 as i32) < 0, GetKeyState(VK_SHIFT.0 as i32) < 0);
                        if (ctrl && vk == 'V' as u16) || (shift && !ctrl && vk == VK_INSERT.0) { a.edit_paste(); a.sync_edit(); continue; }
                    }
                    if msg.message == WM_KEYDOWN && msg.hwnd == a.edit {
                        // RichEdit erases on keydown. Repair the range before it processes Backspace.
                        if a.list_key(vk) || a.picture_key(vk) || (vk == VK_BACK.0 && a.simple_edit_key(msg.message, msg.wParam, msg.lParam).is_some()) {
                            a.sync_edit();
                            continue;
                        }
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                if let Some(a) = app() { if a.edit_visible || a.car.on { a.sync_edit(); } }
            }
            // animating: one frame per screen refresh. DwmFlush waits for the next composition,
            // so motion follows the display (60/120/144 Hz) instead of the coarse 15.6 ms timer.
            match app() {
                Some(a) if a.animating => {
                    let t0 = Instant::now();
                    a.frame();
                    let _ = DwmFlush();
                    // DWM can return at once (nothing to compose): never spin faster than ~250 Hz
                    while t0.elapsed().as_micros() < 4000 { Sleep(1); }
                }
                _ => { let _ = WaitMessage(); }
            }
        }
        timeEndPeriod(1);
        if buffered_paint { let _ = BufferedPaintUnInit(); }
    }
    Ok(())
}
