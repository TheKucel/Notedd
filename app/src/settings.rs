//! Preferences: a small key=value file in %APPDATA%\Pane, plus the Windows bits they touch
//! (start with Windows, the system light/dark choice).

use std::path::PathBuf;
use crate::locale::Language;
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::*;

/// key bindings: 0 show/hide (global), 1 new note (global), 2 new note, 3 all notes, 4 find, 5 settings, 6 pin, 7 quit, 8 note colour, 9 delete note, 10 highlight
pub const KEYS: usize = 11;
pub const MAX_CARDS: usize = 10;
pub const DEFAULT_KEYS: [(u32, u32); KEYS] = [(3, 0x20), (3, 0x4E), (2, 0x4E), (2, 0x50), (2, 0x46), (2, 0xBC), (2, 0x54), (2, 0x51), (6, 0x43), (6, 0x2E), (6, 0x48)];
/// card colours per theme kind: 0 paper, 1 night, 2 custom
pub const DEFAULT_CARDS: [[u32; MAX_CARDS]; 3] = [
    [0xefebe3, 0xdde3d2, 0xd8e0e2, 0xe2dbe8, 0xefdcd4, 0xeee1c5, 0xd9e4ea, 0xe6e6dc, 0xe9dde3, 0xe0e7dc],
    [0x141412, 0x1a1f19, 0x171d1f, 0x1d1a22, 0x221a17, 0x221e15, 0x161c22, 0x1c1c18, 0x201a1d, 0x1a1f1a],
    [0xefebe3, 0xdde3d2, 0xd8e0e2, 0xe2dbe8, 0xefdcd4, 0xeee1c5, 0xd9e4ea, 0xe6e6dc, 0xe9dde3, 0xe0e7dc],
];

#[derive(Clone, Copy, PartialEq)]
pub struct Prefs {
    /// None follows the Windows display language.
    pub language: Option<Language>,
    pub simple_editor: bool,
    /// 0 paper, 1 night, 2 follow Windows, 3 custom
    pub theme: u8,
    /// text size, percent
    pub scale: u32,
    /// where the window comes from: 0 bottom, 1 right, 2 left, 3 top, 4 in place (fade)
    pub dir: u8,
    /// slide in from beyond the screen edge instead of a short way
    pub far: bool,
    /// 0 fast, 1 normal, 2 slow
    pub speed: u8,
    /// (MOD_* flags, virtual key); vk 0 = no binding
    pub keys: [(u32, u32); KEYS],
    /// card colours per theme kind (0 paper, 1 night, 2 custom); slot 0 is the theme's paper
    pub cards: [[u32; MAX_CARDS]; 3],
    /// how many card colours the palette offers
    pub ncards: usize,
    /// the window stays above other windows
    pub pinned: bool,
    /// text and accent overrides per theme kind (None = the theme's own)
    pub ink: [Option<u32>; 3],
    pub accent: [Option<u32>; 3],
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { language: None, simple_editor: false, theme: 0, scale: 100, dir: 0, far: false, speed: 1, keys: DEFAULT_KEYS, cards: DEFAULT_CARDS, ncards: 6, pinned: false, ink: [None; 3], accent: [None; 3] }
    }
}

fn path() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join("Notedd");
    let file = dir.join("settings.txt");
    // the app was called Pane before: its settings carry over once
    if !file.exists() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::copy(base.join("Pane").join("settings.txt"), &file);
    }
    file
}

/// the folder the notes live in, if one was chosen (installer or settings)
pub fn notes_dir_pref() -> Option<PathBuf> {
    let s = std::fs::read_to_string(path()).ok()?;
    s.lines().find_map(|l| l.strip_prefix("notes_dir=")).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).map(PathBuf::from)
}
pub fn set_notes_dir(dir: &std::path::Path) -> std::io::Result<()> {
    let file = path();
    let s = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let mut out: String = s.lines().filter(|l| !l.starts_with("notes_dir=")).map(|l| format!("{l}\n")).collect();
    out += &format!("notes_dir={}\n", dir.display());
    crate::store::replace_file(&file, out.as_bytes())
}

pub fn hex(s: &str) -> Option<u32> {
    let s = s.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 { return None; }
    u32::from_str_radix(s, 16).ok().map(|v| v & 0xffffff)
}

pub fn load() -> Prefs {
    let mut p = Prefs::default();
    let Ok(s) = std::fs::read_to_string(path()) else { return p };
    for line in s.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        let n = v.parse::<u32>().unwrap_or(0);
        let idx = |pre: &str, max: usize| k.strip_prefix(pre).and_then(|r| r.parse::<usize>().ok()).filter(|&i| i < max);
        match k {
            "language" => p.language = Language::from_tag(v),
            "editor" => p.simple_editor = v == "simple",
            "theme" => p.theme = (n as u8).min(3),
            "scale" => p.scale = n.clamp(80, 160),
            "dir" => p.dir = (n as u8).min(4),
            "far" => p.far = n != 0,
            "speed" => p.speed = (n as u8).min(2),
            "hk_mods" => p.keys[0].0 = n,
            "hk_vk" => if n != 0 { p.keys[0].1 = n },
            "pinned" => p.pinned = n != 0,
            "ncards" => p.ncards = n.clamp(1, MAX_CARDS as u32) as usize,
            _ => {
                if let Some(i) = idx("key", KEYS) {
                    if let Some((m, vk)) = v.split_once(',') {
                        if let (Ok(m), Ok(vk)) = (m.trim().parse(), vk.trim().parse()) { p.keys[i] = (m, vk); }
                    }
                } else if let Some(i) = idx("cards", 3) {
                    for (j, c) in v.split(',').enumerate().take(MAX_CARDS) { if let Some(c) = hex(c) { p.cards[i][j] = c; } }
                } else if let Some(i) = idx("ink", 3) { p.ink[i] = hex(v); }
                else if let Some(i) = idx("accent", 3) { p.accent[i] = hex(v); }
            }
        }
    }
    p
}

pub fn save(p: &Prefs) {
    let language = p.language.map_or("system", Language::tag);
    let editor = if p.simple_editor { "simple" } else { "markdown" };
    let mut s = format!("language={language}\neditor={editor}\ntheme={}\nscale={}\ndir={}\nfar={}\nspeed={}\n", p.theme, p.scale, p.dir, p.far as u8, p.speed);
    for (i, (m, vk)) in p.keys.iter().enumerate() { s += &format!("key{i}={m},{vk}\n"); }
    s += &format!("ncards={}\npinned={}\n", p.ncards, p.pinned as u8);
    let h = |c: Option<u32>| c.map(|c| format!("{c:06x}")).unwrap_or_default();
    for t in 0..3 {
        s += &format!("cards{t}={}\n", p.cards[t].iter().map(|c| format!("{c:06x}")).collect::<Vec<_>>().join(","));
        s += &format!("ink{t}={}\naccent{t}={}\n", h(p.ink[t]), h(p.accent[t]));
    }
    if let Some(d) = notes_dir_pref() { s += &format!("notes_dir={}\n", d.display()); }
    let _ = std::fs::write(path(), s);
}

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

pub fn autostart() -> bool {
    unsafe {
        let mut size = 0u32;
        RegGetValueW(HKEY_CURRENT_USER, RUN, w!("Notedd"), RRF_RT_REG_SZ, None, None, Some(&mut size)).is_ok()
    }
}

pub fn set_autostart(on: bool) {
    unsafe {
        if on {
            let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let v: Vec<u16> = format!("\"{exe}\" --background").encode_utf16().chain(Some(0)).collect();
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, RUN, w!("Notedd"), REG_SZ.0, Some(v.as_ptr() as _), (v.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, w!("Notedd"));
        }
        // the entry from when the app was called Pane
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, w!("Pane"));
    }
}

/// Windows app mode: true when apps should be dark
pub fn system_dark() -> bool {
    unsafe {
        let mut v = 1u32;
        let mut size = 4u32;
        let ok = RegGetValueW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as _), Some(&mut size)).is_ok();
        ok && v == 0
    }
}
