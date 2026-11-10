//! Preferences: the same `settings.txt` the RichEdit app uses, so both versions share it.

use crate::locale::Language;
use std::path::PathBuf;

/// key bindings: 0 show/hide (global), 1 new note (global), 2 new note, 3 all notes, 4 find, 5 settings, 6 pin, 7 quit, 8 note colour, 9 delete note, 10 highlight
pub const KEYS: usize = 11;
pub const MAX_CARDS: usize = 10;
/// (MOD_* flags, virtual key): MOD_ALT 1, MOD_CONTROL 2, MOD_SHIFT 4, MOD_WIN 8
pub const DEFAULT_KEYS: [(u32, u32); KEYS] = [(3, 0x20), (3, 0x4E), (2, 0x4E), (2, 0x50), (2, 0x46), (2, 0xBC), (2, 0x54), (2, 0x51), (6, 0x43), (6, 0x2E), (6, 0x48)];
#[cfg(target_os = "macos")]
const MAC_KEYS: [(u32, u32); KEYS] = [(3, 0x20), (3, 0x4E), (8, 0x4E), (8, 0x50), (8, 0x46), (8, 0xBC), (8, 0x54), (8, 0x51), (12, 0x43), (12, 0x2E), (12, 0x48)];
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
    pub dir: u8,
    pub far: bool,
    pub speed: u8,
    pub keys: [(u32, u32); KEYS],
    pub cards: [[u32; MAX_CARDS]; 3],
    pub ncards: usize,
    pub pinned: bool,
    pub ink: [Option<u32>; 3],
    pub accent: [Option<u32>; 3],
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs { language: None, simple_editor: true, theme: 0, scale: 100, dir: 0, far: false, speed: 1, keys: if cfg!(target_os = "macos") { mac_default_keys() } else { DEFAULT_KEYS }, cards: DEFAULT_CARDS, ncards: 6, pinned: false, ink: [None; 3], accent: [None; 3] }
    }
}

fn mac_default_keys() -> [(u32, u32); KEYS] {
    #[cfg(target_os = "macos")]
    { MAC_KEYS }
    #[cfg(not(target_os = "macos"))]
    { DEFAULT_KEYS }
}

/// where Notedd keeps its settings: %APPDATA%, ~/Library/Application Support, ~/.config
fn config_base() -> PathBuf {
    // Windows: APPDATA itself, so a test run can point it at a sandbox (and the old app reads the same file)
    #[cfg(windows)]
    if let Some(a) = std::env::var_os("APPDATA") { return PathBuf::from(a); }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir)
}

fn path() -> PathBuf {
    let base = config_base();
    let dir = base.join("Notedd");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.txt")
}

/// the folder the notes live in, if one was chosen (installer or settings)
pub fn notes_dir_pref() -> Option<PathBuf> {
    let s = std::fs::read_to_string(path()).ok()?;
    s.lines().find_map(|l| l.strip_prefix("notes_dir=")).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn set_notes_dir(dir: &std::path::Path) -> std::io::Result<()> {
    let file = path();
    let s = std::fs::read_to_string(&file).unwrap_or_default();
    let mut out: String = s.lines().filter(|l| !l.starts_with("notes_dir=")).map(|l| format!("{l}\n")).collect();
    out += &format!("notes_dir={}\n", dir.display());
    notedd_core::store::replace_file(&file, out.as_bytes())
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
    // Older Mac builds saved the Windows defaults. Keep customized shortcuts intact.
    #[cfg(target_os = "macos")]
    if !s.lines().any(|line| line.trim() == "mac_keys=1") {
        for i in 2..KEYS { if p.keys[i] == DEFAULT_KEYS[i] { p.keys[i] = MAC_KEYS[i]; } }
    }
    p
}

pub fn save(p: &Prefs) {
    let language = p.language.map_or("system", Language::tag);
    let editor = if p.simple_editor { "simple" } else { "markdown" };
    let mut s = format!("language={language}\neditor={editor}\ntheme={}\nscale={}\ndir={}\nfar={}\nspeed={}\n", p.theme, p.scale, p.dir, p.far as u8, p.speed);
    #[cfg(target_os = "macos")]
    { s += "mac_keys=1\n"; }
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

// ---- start with the system ---------------------------------------------------------
// Windows: the Run key (the value name the old app used too); the Mac: a LaunchAgent;
// Linux: an XDG autostart entry. All start the app hidden (--background).

fn exe() -> String { std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default() }

#[cfg(windows)]
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

#[cfg(windows)]
pub fn autostart() -> bool {
    windows_registry::CURRENT_USER.open(RUN).ok().and_then(|k| k.get_string("Notedd").ok()).is_some()
}

#[cfg(windows)]
pub fn set_autostart(on: bool) {
    let Ok(key) = windows_registry::CURRENT_USER.create(RUN) else { return };
    if on { let _ = key.set_string("Notedd", &format!("\"{}\" --background", exe())); } else { let _ = key.remove_value("Notedd"); }
}

#[cfg(not(windows))]
fn autostart_file() -> Option<PathBuf> {
    if cfg!(target_os = "macos") { dirs::home_dir().map(|h| h.join("Library/LaunchAgents/app.notedd.plist")) }
    else { dirs::config_dir().map(|c| c.join("autostart/notedd.desktop")) }
}

#[cfg(not(windows))]
pub fn autostart() -> bool { autostart_file().is_some_and(|f| f.exists()) }

#[cfg(not(windows))]
pub fn set_autostart(on: bool) {
    let Some(f) = autostart_file() else { return };
    if !on { let _ = std::fs::remove_file(&f); return; }
    let exe = exe();
    let text = if cfg!(target_os = "macos") {
        let esc = exe.replace('&', "&amp;").replace('<', "&lt;");
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
            <plist version=\"1.0\"><dict>\n<key>Label</key><string>app.notedd</string>\n\
            <key>ProgramArguments</key><array><string>{esc}</string><string>--background</string></array>\n\
            <key>RunAtLoad</key><true/>\n</dict></plist>\n")
    } else {
        format!("[Desktop Entry]\nType=Application\nName=Notedd\nExec=\"{}\" --background\nX-GNOME-Autostart-enabled=true\n", exe.replace('"', "\\\""))
    };
    if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
    let _ = std::fs::write(&f, text);
}

// ---- dark mode ---------------------------------------------------------------------

/// what the system said at start (GPUI reads it the same way on every system)
static SYSTEM_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn set_system_dark(on: bool) { SYSTEM_DARK.store(on, std::sync::atomic::Ordering::Relaxed); }

/// true when apps should be dark
pub fn system_dark() -> bool {
    // Windows: read live, so the switch in Settings is picked up without a restart
    #[cfg(windows)]
    if let Some(v) = windows_registry::CURRENT_USER
        .open("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").ok()
        .and_then(|k| k.get_u32("AppsUseLightTheme").ok()) { return v == 0; }
    SYSTEM_DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// the chosen notes folder, or Documents\Notedd (Documents\Pane if the notes are still there from before the rename)
pub fn notes_dir() -> PathBuf {
    if let Some(d) = notes_dir_pref() { let _ = std::fs::create_dir_all(&d); return d; }
    let docs = dirs_documents();
    let (new, old) = (docs.join("Notedd"), docs.join("Pane"));
    let dir = if !new.exists() && old.exists() { old } else { new };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn dirs_documents() -> PathBuf {
    dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents"))).unwrap_or_else(std::env::temp_dir)
}

/// a note the user deletes goes to the Recycle Bin / Trash; where there is none the file is removed
pub fn trash(dir: &std::path::Path, id: &str) {
    let path = dir.join(format!("{id}.md"));
    if !path.exists() { return; }
    if trash::delete(&path).is_err() || path.exists() { let _ = std::fs::remove_file(&path); }
}
