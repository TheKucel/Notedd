//! Notedd setup. Every step is a note, and they all lie in one stack: at the start they land one by one,
//! the hello note on top. Each step forward throws the top note aside (left, then right, and so on).
//! It copies the app into the user's programs folder, makes shortcuts, autostart and an uninstall entry.
//! No admin rights: everything goes under the user's profile. Run with --uninstall to remove the app.

#![windows_subsystem = "windows"]

#[path = "../../locale.rs"]
mod locale;
mod shell;
mod strings;

use locale::Language;
use strings as ui;

use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::time::Instant;
use windows::core::*;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static APP_EXE: &[u8] = include_bytes!(env!("NOTEDD_EXE"));
static FONT: &[u8] = include_bytes!("../../assets/fonts/Geologica.ttf");
static ICON: &[u8] = include_bytes!("../../assets/notedd.ico");
const VERSION: &str = "1.0.0";

// ---- geometry, DIPs ----
const W: f32 = 640.0;
const H: f32 = 504.0;
const M: f32 = 64.0; // room around the card for its shadow and the finale's fan
const STACK: f32 = 44.0; // room above the card for the edges of the notes under it
const OX: f32 = M;
const OY: f32 = M + STACK;
const T: f32 = 34.0;
const R: f32 = 20.0;
const G: f32 = 6.0;
const F: f32 = 12.0;
const RT: f32 = 14.0;
const PAD: f32 = 56.0;
const TOP: f32 = T + 54.0;
const BTN_H: f32 = 44.0;
const CONTROL_GAP: f32 = 16.0;
const KEY_CHANGE_GAP: f32 = 12.0;
const X2: f32 = R + G + F;

// ---- the setup is always on paper ----
const PAPER: u32 = 0xfaf7f1;
const INK: u32 = 0x1c1b19;
const INK2: u32 = 0x6f6b63;
const LINE: u32 = 0x3d3c38;
const DARK: u32 = 0x22211f;
const ON: u32 = 0xfaf7f1;

fn split(c: u32) -> [f32; 3] { [((c >> 16) & 255) as f32, ((c >> 8) & 255) as f32, (c & 255) as f32] }
fn join(c: [f32; 3]) -> u32 { ((c[0].round() as u32) << 16) | ((c[1].round() as u32) << 8) | c[2].round() as u32 }
fn mix(a: u32, b: u32, k: f32) -> u32 { let (a, b) = (split(a), split(b)); join([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]) }
fn rgb(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: ((hex >> 16) & 255) as f32 / 255.0, g: ((hex >> 8) & 255) as f32 / 255.0, b: (hex & 255) as f32 / 255.0, a }
}
fn luma(c: u32) -> f32 { let [r, g, b] = split(c); (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0 }
fn pt(x: f32, y: f32) -> D2D_POINT_2F { D2D_POINT_2F { x, y } }
fn rect(l: f32, t: f32, r: f32, b: f32) -> D2D_RECT_F { D2D_RECT_F { left: l, top: t, right: r, bottom: b } }
fn rrect(l: f32, t: f32, r: f32, b: f32, rad: f32) -> D2D1_ROUNDED_RECT { D2D1_ROUNDED_RECT { rect: rect(l, t, r, b), radiusX: rad, radiusY: rad } }
fn circle(x: f32, y: f32, r: f32) -> D2D1_ELLIPSE { D2D1_ELLIPSE { point: pt(x, y), radiusX: r, radiusY: r } }
fn u(s: &str) -> Vec<u16> { s.encode_utf16().collect() }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }
fn wpath(p: &Path) -> Vec<u16> { wide(&p.to_string_lossy()) }
fn mat(m11: f32, m12: f32, m21: f32, m22: f32, dx: f32, dy: f32) -> Matrix3x2 { Matrix3x2 { M11: m11, M12: m12, M21: m21, M22: m22, M31: dx, M32: dy } }
fn shift(dx: f32, dy: f32) -> Matrix3x2 { mat(1.0, 0.0, 0.0, 1.0, dx, dy) }
fn mul(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    mat(a.M11 * b.M11 + a.M12 * b.M21, a.M11 * b.M12 + a.M12 * b.M22,
        a.M21 * b.M11 + a.M22 * b.M21, a.M21 * b.M12 + a.M22 * b.M22,
        a.M31 * b.M11 + a.M32 * b.M21 + b.M31, a.M31 * b.M12 + a.M32 * b.M22 + b.M32)
}
fn inside(r: &D2D_RECT_F, x: f32, y: f32) -> bool { x >= r.left && x <= r.right && y >= r.top && y <= r.bottom }

#[derive(Clone, Copy)]
struct Ease { v: f32, to: f32 }
impl Ease {
    const fn at(v: f32) -> Ease { Ease { v, to: v } }
    fn step(&mut self, k: f32, eps: f32) -> bool {
        self.v += (self.to - self.v) * k;
        if (self.v - self.to).abs() < eps { self.v = self.to; true } else { false }
    }
}

/// where a note is: the same motion the app uses for its folders
#[derive(Clone, Copy, Default)]
struct V { x: f32, y: f32, sc: f32, o: f32, rot: f32 }
impl V {
    fn lerp(&mut self, to: &V, k: f32, ko: f32) {
        self.x += (to.x - self.x) * k; self.y += (to.y - self.y) * k; self.sc += (to.sc - self.sc) * k; self.rot += (to.rot - self.rot) * k; self.o += (to.o - self.o) * ko;
    }
    fn near(&self, to: &V) -> bool {
        (self.x - to.x).abs() < 0.3 && (self.y - to.y).abs() < 0.3 && (self.sc - to.sc).abs() < 0.001 && (self.o - to.o).abs() < 0.004 && (self.rot - to.rot).abs() < 0.02
    }
}
#[derive(Clone, Copy)]
struct Card { cur: V, to: V, delay: f32 }


#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit { Primary, Secondary, Close, Theme(usize), Editor(bool), Keys, Auto, Desktop, NotesDir, AppDir, LanguageMenu, Language(usize) }

struct Fonts { big: IDWriteTextFormat, title: IDWriteTextFormat, sub: IDWriteTextFormat, subc: IDWriteTextFormat, label: IDWriteTextFormat, small: IDWriteTextFormat, btn: IDWriteTextFormat, cap: IDWriteTextFormat, tab: IDWriteTextFormat, titlec: IDWriteTextFormat, aa: IDWriteTextFormat }

struct Setup {
    hwnd: HWND,
    factory: ID2D1Factory,
    dw: IDWriteFactory,
    fonts: Fonts,
    round: ID2D1StrokeStyle,
    dash: ID2D1StrokeStyle,
    target: Option<ID2D1HwndRenderTarget>,
    scale: f32,
    uninstall: bool,
    /// pages: 0 hello, 1 theme, 2 hotkey, 3 folders, 4 installing / done (uninstall: 0 ask, 1 done)
    page: usize,
    /// one note per page, the first on top
    cards: Vec<Card>,
    /// the hello text comes up once the notes have landed
    intro: Ease,
    intro_wait: f32,
    theme: usize,
    theme0: usize,
    simple_editor: bool,
    editor_pill: Ease,
    /// the user's own palette from the app (theme 3): paper, card, text
    custom: Option<[u32; 3]>,
    /// how picked each theme tile looks
    sel_k: [Ease; 4],
    keys: (u32, u32),
    capturing: bool,
    autostart: bool,
    desktop: bool,
    sw: [Ease; 2],
    notes_dir: PathBuf,
    app_dir: PathBuf,
    progress: Ease,
    working: bool,
    done: bool,
    /// seconds since the install finished, drives the finale
    done_t: f32,
    /// the finale: the thrown notes come back and fan out behind the last one, then file in under it
    finale: bool,
    fan_closed: bool,
    error: Option<String>,
    hover: Option<Hit>,
    press: Option<Hit>,
    language: Option<Language>,
    language_menu: Option<usize>,
    hover_k: [Ease; 13 + Language::ALL.len()],
    tick: Instant,
    animating: bool,
}

static mut APP: *mut Setup = null_mut();
fn app() -> Option<&'static mut Setup> { unsafe { APP.as_mut() } }

// ---------- paths and the existing install ----------
unsafe fn known(id: &GUID) -> PathBuf {
    match SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None) {
        Ok(p) => { let s = p.to_string().unwrap_or_default(); CoTaskMemFree(Some(p.0 as _)); PathBuf::from(s) }
        Err(_) => std::env::temp_dir(),
    }
}
fn appdata() -> PathBuf { std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir()) }
fn settings_file() -> PathBuf { appdata().join("Notedd").join("settings.txt") }
fn remove_settings(file: &Path) {
    let _ = std::fs::remove_file(file);
    if let Some(dir) = file.parent() { let _ = std::fs::remove_dir(dir); }
}

#[cfg(test)]
#[test]
fn removing_settings_preserves_notes_in_the_same_tree() {
    let profile = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("notedd-uninstall-test-{}", std::process::id()));
    let settings = profile.join("Notedd").join("settings.txt");
    let folder = settings.parent().unwrap();
    let note = folder.join("notes").join("note.md");
    let asset = folder.join("notes").join("assets").join("image.png");
    std::fs::create_dir_all(asset.parent().unwrap()).unwrap();
    std::fs::write(&note, "Title\nBody").unwrap();
    std::fs::write(&asset, [1, 2, 3]).unwrap();
    std::fs::write(&settings, "theme=0").unwrap();

    remove_settings(&settings);

    assert!(!settings.exists());
    assert_eq!(std::fs::read_to_string(&note).unwrap(), "Title\nBody");
    assert_eq!(std::fs::read(&asset).unwrap(), [1, 2, 3]);
    std::fs::remove_dir_all(profile).unwrap();
}
/// the settings the app already has (also the ones from when it was called Pane)
fn old_settings() -> String {
    std::fs::read_to_string(settings_file()).or_else(|_| std::fs::read_to_string(appdata().join("Pane").join("settings.txt"))).unwrap_or_default()
}
fn setting(s: &str, key: &str) -> Option<String> { s.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).map(|v| v.trim().to_string()) }
const UNINSTALL_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Notedd");
const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
unsafe fn reg_str(key: PCWSTR, name: PCWSTR) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    if RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as _), Some(&mut size)).is_err() { return None; }
    let n = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n]))
}
unsafe fn reg_set(key: PCWSTR, name: PCWSTR, v: &str) {
    let w = wide(v);
    let _ = RegSetKeyValueW(HKEY_CURRENT_USER, key, name, REG_SZ.0, Some(w.as_ptr() as _), (w.len() * 2) as u32);
}
unsafe fn reg_dword(key: PCWSTR, name: PCWSTR, v: u32) {
    let _ = RegSetKeyValueW(HKEY_CURRENT_USER, key, name, REG_DWORD.0, Some(&v as *const u32 as _), 4);
}
fn has_notes(d: &Path) -> bool {
    std::fs::read_dir(d).map(|r| r.flatten().any(|e| e.path().extension().map_or(false, |x| x == "md"))).unwrap_or(false)
}
unsafe fn default_dirs() -> (PathBuf, PathBuf) {
    let app = reg_str(UNINSTALL_KEY, w!("InstallLocation")).map(PathBuf::from)
        .unwrap_or_else(|| known(&FOLDERID_LocalAppData).join("Programs").join("Notedd"));
    let docs = known(&FOLDERID_Documents);
    let s = old_settings();
    let notes = setting(&s, "notes_dir").filter(|v| !v.is_empty()).map(PathBuf::from)
        .unwrap_or_else(|| if !has_notes(&docs.join("Notedd")) && has_notes(&docs.join("Pane")) { docs.join("Pane") } else { docs.join("Notedd") });
    (app, notes)
}

fn key_name(vk: u32) -> String {
    match vk {
        0x20 => "Space".into(), 0x0D => "Enter".into(), 0x09 => "Tab".into(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0xBC => ",".into(), 0xBE => ".".into(), 0xBF => "/".into(), 0xBA => ";".into(), 0xDE => "'".into(),
        0xDB => "[".into(), 0xDD => "]".into(), 0xC0 => "`".into(), 0xBD => "-".into(), 0xBB => "=".into(),
        _ => format!("#{vk}"),
    }
}
fn key_parts(m: u32, vk: u32) -> Vec<String> {
    let mut v = vec![];
    if m & 2 != 0 { v.push("Ctrl".into()); }
    if m & 1 != 0 { v.push("Alt".into()); }
    if m & 4 != 0 { v.push("Shift".into()); }
    if m & 8 != 0 { v.push("Win".into()); }
    if vk != 0 { v.push(key_name(vk)); }
    v
}

// ---------- install and uninstall ----------
/// asks a running Notedd (or the old Pane) to close, so its exe can be replaced or removed
unsafe fn close_running() {
    for class in [w!("NoteddFolder"), w!("PaneFolder")] {
        let Ok(h) = FindWindowW(class, None) else { continue };
        if h.0.is_null() { continue; }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let proc = OpenProcess(PROCESS_SYNCHRONIZE, false, pid);
        let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
        if let Ok(p) = proc { let _ = WaitForSingleObject(p, 5000); let _ = CloseHandle(p); }
    }
}
unsafe fn shortcut(lnk: &Path, target: &Path, icon: &Path) -> Result<()> {
    let l: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
    l.SetPath(PCWSTR(wpath(target).as_ptr()))?;
    if let Some(d) = target.parent() { l.SetWorkingDirectory(PCWSTR(wpath(d).as_ptr()))?; }
    l.SetIconLocation(PCWSTR(wpath(icon).as_ptr()), 0)?;
    l.SetDescription(PCWSTR(wide(ui::QUICK_NOTES.get()).as_ptr()))?;
    let pf: IPersistFile = l.cast()?;
    pf.Save(PCWSTR(wpath(lnk).as_ptr()), true)?;
    Ok(())
}
/// the settings file keeps everything the app wrote; only the chosen bits change
fn write_settings(theme: Option<usize>, simple_editor: bool, keys: (u32, u32), notes: &Path, language: Option<Language>) -> std::io::Result<()> {
    let file = settings_file();
    std::fs::create_dir_all(file.parent().unwrap())?;
    let old = old_settings();
    let mut out: String = old.lines()
        .filter(|l| !["key0=", "hk_mods=", "hk_vk=", "notes_dir="].iter().any(|k| l.starts_with(k))
            && !(theme.is_some() && l.starts_with("theme="))
            && !(language.is_some() && l.starts_with("language="))
            && !l.split_once('=').is_some_and(|(key, _)| key.trim() == "editor"))
        .map(|l| format!("{l}\n")).collect();
    if let Some(t) = theme { out += &format!("theme={t}\n"); }
    if let Some(lang) = language { out += &format!("language={}\n", lang.tag()); }
    let editor = if simple_editor { "simple" } else { "markdown" };
    out += &format!("editor={editor}\n");
    out += &format!("key0={},{}\nnotes_dir={}\n", keys.0, keys.1, notes.display());
    std::fs::write(file, out)
}
unsafe fn install(s: &Setup) -> std::result::Result<(), String> {
    close_running();
    let dir = &s.app_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", ui::CREATE_APP_DIR.get()))?;
    let exe = dir.join("notedd.exe");
    let tmp = dir.join("notedd.exe.new");
    std::fs::write(&tmp, APP_EXE).map_err(|e| format!("{}: {e}", ui::WRITE_APP.get()))?;
    let mut tries = 0;
    while let Err(e) = std::fs::rename(&tmp, &exe) {
        // the old one may still be closing
        tries += 1;
        if tries > 20 { let _ = std::fs::remove_file(&tmp); return Err(format!("{}: {e}", ui::APP_RUNNING.get())); }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let icon = dir.join("notedd.ico");
    let _ = std::fs::write(&icon, ICON);
    // the uninstaller is this program, kept next to the app
    let me = std::env::current_exe().unwrap_or_default();
    let un = dir.join("uninstall.exe");
    if me != un { let _ = std::fs::copy(&me, &un); }

    // notes: the folder exists; notes from the old place come along (copied, never deleted)
    std::fs::create_dir_all(&s.notes_dir).map_err(|e| format!("{}: {e}", ui::CREATE_NOTES_DIR.get()))?;
    let (_, before) = default_dirs();
    if before != s.notes_dir && has_notes(&before) && !has_notes(&s.notes_dir) {
        if let Ok(rd) = std::fs::read_dir(&before) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map_or(false, |x| x == "md") { let _ = std::fs::copy(&p, s.notes_dir.join(e.file_name())); }
            }
        }
    }
    write_settings((s.theme != s.theme0).then_some(s.theme), s.simple_editor, s.keys, &s.notes_dir, s.language)
        .map_err(|e| format!("{}: {e}", ui::SAVE_SETTINGS.get()))?;

    let programs = known(&FOLDERID_Programs);
    let _ = shortcut(&programs.join("Notedd.lnk"), &exe, &icon);
    let desk = known(&FOLDERID_Desktop).join("Notedd.lnk");
    if s.desktop { let _ = shortcut(&desk, &exe, &icon); } else { let _ = std::fs::remove_file(&desk); }
    if s.autostart { reg_set(RUN, w!("Notedd"), &format!("\"{}\"", exe.display())); } else { let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, w!("Notedd")); }
    let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, w!("Pane"));

    reg_set(UNINSTALL_KEY, w!("DisplayName"), "Notedd");
    reg_set(UNINSTALL_KEY, w!("DisplayVersion"), VERSION);
    reg_set(UNINSTALL_KEY, w!("Publisher"), "Notedd");
    reg_set(UNINSTALL_KEY, w!("DisplayIcon"), &icon.to_string_lossy());
    reg_set(UNINSTALL_KEY, w!("InstallLocation"), &dir.to_string_lossy());
    reg_set(UNINSTALL_KEY, w!("UninstallString"), &format!("\"{}\" --uninstall", un.display()));
    reg_dword(UNINSTALL_KEY, w!("NoModify"), 1);
    reg_dword(UNINSTALL_KEY, w!("NoRepair"), 1);
    reg_dword(UNINSTALL_KEY, w!("EstimatedSize"), (APP_EXE.len() / 1024) as u32 + 64);
    Ok(())
}
unsafe fn uninstall(s: &Setup) {
    close_running();
    let dir = &s.app_dir;
    let _ = std::fs::remove_file(dir.join("notedd.exe"));
    let _ = std::fs::remove_file(dir.join("notedd.ico"));
    let _ = std::fs::remove_file(known(&FOLDERID_Programs).join("Notedd.lnk"));
    let _ = std::fs::remove_file(known(&FOLDERID_Desktop).join("Notedd.lnk"));
    let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, w!("Notedd"));
    let _ = RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY);
    remove_settings(&settings_file());
}
/// a running exe cannot delete itself: a hidden cmd does it a moment after we exit
fn delete_self_later(dir: &Path) {
    use std::os::windows::process::CommandExt;
    let me = std::env::current_exe().unwrap_or_default();
    if me.parent() != Some(dir) { return; }
    let cmd = format!("ping 127.0.0.1 -n 3 > nul & del /f /q \"{}\" & rmdir \"{}\"", me.display(), dir.display());
    let _ = std::process::Command::new("cmd").args(["/c", &cmd]).creation_flags(0x0800_0000).spawn();
}

// ---------- the window ----------
/// note `i`, `d` levels down the stack: higher and a touch smaller, so its edge and tab show,
/// and a little askew, each note its own way, like sheets dropped on a desk; the top one lies straight
fn stack_v(i: usize, d: usize) -> V {
    const ROT: [f32; 5] = [-1.6, 1.4, -1.1, 1.8, -1.3];
    const DX: [f32; 5] = [-3.0, 3.0, -2.0, 3.0, -2.0];
    if d == 0 { return V { x: 0.0, y: 0.0, sc: 1.0, o: 1.0, rot: 0.0 }; }
    let k = i % ROT.len();
    let d = d as f32;
    // the turn is about a point low on the card, which would swing the tabs sideways onto each other:
    // shift back so the tabs stay where they belong and the lean shows lower down
    let swing = H * 0.9 * ROT[k].to_radians().sin();
    V { x: DX[k] - swing, y: -13.0 * d, sc: 1.0 - 0.012 * d, o: 1.0, rot: ROT[k] }
}
/// a note that was thrown aside: even pages go left, odd ones right
fn gone_v(i: usize) -> V {
    let s = if i % 2 == 0 { -1.0 } else { 1.0 };
    V { x: s * W * 0.62, y: 46.0, sc: 0.97, o: 0.0, rot: s * 9.0 }
}
/// the thrown notes in the finale, fanned behind the last one like a hand of cards
fn fan_v(i: usize) -> V {
    let rot = [-5.0, -2.5, 2.5, 5.0][i.min(3)];
    let y = [-6.0, -16.0, -16.0, -6.0][i.min(3)];
    V { x: 0.0, y, sc: 0.985, o: 1.0, rot }
}
fn card_matrix(v: &V) -> Matrix3x2 {
    let (cx, cy) = (W / 2.0, H * 0.9);
    let (s, c) = v.rot.to_radians().sin_cos();
    mul(&mul(&shift(-cx, -cy), &mat(c * v.sc, s * v.sc, -s * v.sc, c * v.sc, 0.0, 0.0)), &shift(cx + OX + v.x, cy + OY + v.y))
}
const CARD: u32 = 0xffffff;
const TILE_LIGHT: (u32, u32) = (0xf4f1ea, 0x1c1b19);
const TILE_DARK: (u32, u32) = (0x1e1e1c, 0xece8dc);


impl Setup {
    fn pages(&self) -> usize { if self.uninstall { 2 } else { 5 } }
    fn card_color(&self, _p: usize) -> u32 { CARD }
    fn card_label(&self, p: usize) -> &'static str {
        if self.uninstall { return if p == 0 { ui::UNINSTALL.get() } else { ui::DONE.get() }; }
        match p { 0 => "Notedd", 1 => ui::THEME.get(), 2 => ui::SHORTCUT.get(), 3 => ui::FOLDERS.get(), _ => if self.done && self.error.is_none() { ui::DONE.get() } else { ui::INSTALLATION.get() } }
    }
    unsafe fn text_w(&self, s: &str, f: &IDWriteTextFormat) -> f32 {
        let Ok(l) = self.dw.CreateTextLayout(&u(s), f, 4000.0, 100.0) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        let _ = l.GetMetrics(&mut m);
        m.widthIncludingTrailingWhitespace
    }
    unsafe fn tab_w(&self, p: usize) -> f32 { self.text_w(self.card_label(p), &self.fonts.tab) + 2.0 * RT + 10.0 }
    /// the tabs sit side by side like dividers, so the whole stack reads as the list of steps
    unsafe fn tab_x(&self, p: usize) -> f32 { X2 + (0..p).map(|j| self.tab_w(j) + 4.0).sum::<f32>() }
    unsafe fn invalidate(&self) { let _ = InvalidateRect(self.hwnd, None, false); }
    unsafe fn start_anim(&mut self) {
        if !self.animating { self.tick = Instant::now(); }
        self.animating = true;
    }
    fn theme_names(&self) -> Vec<&'static str> {
        vec![ui::LIGHT.get(), ui::DARK.get(), ui::WINDOWS.get()]
    }

    // ---------- layout, in card coordinates ----------
    fn btn_y(&self) -> f32 { H - 36.0 - BTN_H }
    unsafe fn tiles(&self) -> (Vec<D2D_RECT_F>, f32) {
        let n = 3;
        let gap = 26.0;
        let tw = (W - 2.0 * PAD - gap * (n - 1) as f32) / n as f32;
        let y = TOP + 112.0;
        ((0..n).map(|i| { let x = PAD + i as f32 * (tw + gap); rect(x, y, x + tw, y + 100.0) }).collect(), 1.0)
    }
    unsafe fn editor_choices(&self) -> [(bool, &'static str, D2D_RECT_F); 2] {
        let labels = ["Markdown", ui::SIMPLE.get()];
        let cell = labels.iter().map(|s| self.text_w(s, &self.fonts.btn)).fold(0.0, f32::max).ceil() + 38.0;
        let top = self.btn_y() - BTN_H - 20.0;
        let mid = W - PAD - cell;
        [(false, labels[0], rect(mid - cell, top, mid, top + BTN_H)),
         (true, labels[1], rect(mid, top, mid + cell, top + BTN_H))]
    }
    fn language_button(&self) -> D2D_RECT_F {
        let y = self.btn_y();
        rect(PAD, y, PAD + BTN_H, y + BTN_H)
    }
    fn language_panel(&self) -> D2D_RECT_F {
        let bottom = self.btn_y() - 10.0;
        rect(PAD, bottom - 12.0 - 36.0 * Language::ALL.len() as f32, PAD + 166.0, bottom)
    }
    fn language_rows(&self) -> [D2D_RECT_F; Language::ALL.len()] {
        let panel = self.language_panel();
        std::array::from_fn(|i| {
            let y = panel.top + 6.0 + i as f32 * 36.0;
            rect(panel.left + 6.0, y, panel.right - 6.0, y + 36.0)
        })
    }
    unsafe fn change_w(&self) -> f32 { self.text_w(ui::CHANGE.get(), &self.fonts.btn) + 44.0 }
    unsafe fn caps_width(&self) -> f32 {
        key_parts(self.keys.0, self.keys.1).iter().map(|k| self.text_w(k, &self.fonts.cap).max(16.0) + 30.0 + 8.0).sum::<f32>()
    }
    fn row_y(i: usize) -> f32 { TOP + 110.0 + 80.0 * i as f32 }

    /// what can be pressed on a page
    unsafe fn hits(&self, page: usize) -> Vec<(Hit, D2D_RECT_F)> {
        let mut v = vec![(Hit::Close, rect(W - 44.0, 3.0, W - 10.0, T - 3.0))];
        let by = self.btn_y();
        let primary = self.primary_label(page);
        if !primary.is_empty() {
            let w = self.text_w(&primary, &self.fonts.btn) + 56.0;
            v.push((Hit::Primary, rect(W - PAD - w + 8.0, by, W - PAD + 8.0, by + BTN_H)));
        }
        let second = self.secondary_label(page);
        if !second.is_empty() {
            let w = self.text_w(&second, &self.fonts.btn) + 40.0;
            v.push((Hit::Secondary, rect(PAD - 20.0, by, PAD - 20.0 + w, by + BTN_H)));
        }
        if self.uninstall { return v; }
        match page {
            0 => {
                v.push((Hit::LanguageMenu, self.language_button()));
                if self.page == 0 && self.language_menu.is_some() {
                    v.extend(self.language_rows().into_iter().enumerate().map(|(i, r)| (Hit::Language(i), r)));
                }
            }
            1 => {
                let (tiles, _) = self.tiles();
                for (i, r) in tiles.into_iter().enumerate() {
                    v.push((Hit::Theme(i), rect(r.left - 4.0, r.top - 4.0, r.right + 4.0, r.bottom + 36.0)));
                }
                v.extend(self.editor_choices().map(|(simple, _, r)| (Hit::Editor(simple), r)));
            }
            2 => {
                let x = PAD + self.caps_width() + KEY_CHANGE_GAP;
                v.push((Hit::Keys, rect(x, TOP + 110.0, x + self.change_w(), TOP + 154.0)));
                v.push((Hit::Auto, rect(PAD - 8.0, TOP + 196.0, W - PAD + 8.0, TOP + 246.0)));
            }
            3 => {
                let bw = self.change_w() + 4.0;
                for (i, h) in [Hit::NotesDir, Hit::AppDir].into_iter().enumerate() {
                    let y = Self::row_y(i);
                    v.push((h, rect(W - PAD - bw, y + 4.0, W - PAD, y + 4.0 + BTN_H)));
                }
                v.push((Hit::Desktop, rect(PAD - 8.0, Self::row_y(2) + 2.0, W - PAD + 8.0, Self::row_y(2) + 46.0)));
            }
            _ => {}
        }
        v
    }
    fn primary_label(&self, page: usize) -> String {
        if self.uninstall { return if page == 0 { ui::REMOVE.get().into() } else { ui::CLOSE.get().into() }; }
        match page {
            0 => ui::GET_STARTED.get().into(), 1 | 2 => ui::NEXT.get().into(), 3 => ui::INSTALL.get().into(),
            _ => if self.error.is_some() { ui::CLOSE.get().into() } else if self.done { ui::OPEN_APP.get().into() } else { String::new() },
        }
    }
    fn secondary_label(&self, page: usize) -> String {
        if self.uninstall { return if page == 0 { ui::CANCEL.get().into() } else { String::new() }; }
        match page { 1..=3 => ui::BACK.get().into(), 4 if self.done && self.error.is_none() => ui::CLOSE.get().into(), _ => String::new() }
    }
    unsafe fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        if self.working { return None; }
        self.hits(self.page).into_iter().find(|(_, r)| x < r.right && inside(r, x, y)).map(|(h, _)| h)
    }

    // ---------- actions ----------
    /// notes above the page are thrown aside, the rest stay stacked under it
    unsafe fn layout(&mut self) {
        let page = self.page;
        for (i, c) in self.cards.iter_mut().enumerate() {
            c.delay = 0.0;
            c.to = if i < page { gone_v(i) } else { stack_v(i, i - page) };
        }
        self.start_anim();
    }
    unsafe fn go(&mut self, page: usize) {
        self.page = page.min(self.pages() - 1);
        self.capturing = false;
        self.language_menu = None;
        self.hover = None;
        self.layout();
    }
    unsafe fn press(&mut self, h: Hit) {
        match h {
            Hit::Close => { if !self.working { let _ = DestroyWindow(self.hwnd); } }
            Hit::Primary => self.primary(),
            Hit::Secondary => self.secondary(),
            Hit::LanguageMenu => {
                self.language_menu = if self.language_menu.is_some() { None } else { Some(locale::current() as usize) };
            }
            Hit::Language(i) => {
                self.language = Some(Language::ALL[i]);
                locale::set(self.language);
                self.language_menu = None;
                let title = wide(ui::INSTALL_TITLE.get());
                let _ = SetWindowTextW(self.hwnd, PCWSTR(title.as_ptr()));
            }
            Hit::Theme(i) => { self.theme = i; self.start_anim(); }
            Hit::Editor(simple) => {
                self.simple_editor = simple;
                self.editor_pill.to = f32::from(u8::from(simple));
                self.start_anim();
            }
            Hit::Keys => { self.capturing = !self.capturing; }
            Hit::Auto => { self.autostart = !self.autostart; self.sw[0].to = self.autostart as u8 as f32; self.start_anim(); }
            Hit::Desktop => { self.desktop = !self.desktop; self.sw[1].to = self.desktop as u8 as f32; self.start_anim(); }
            Hit::NotesDir => { if let Some(p) = shell::pick_folder(self.hwnd, ui::CHOOSE_NOTES.get(), Some(&self.notes_dir)) { self.notes_dir = p; } }
            Hit::AppDir => { if let Some(p) = shell::pick_folder(self.hwnd, ui::CHOOSE_INSTALL.get(), self.app_dir.parent()) { self.app_dir = if p.ends_with("Notedd") { p } else { p.join("Notedd") }; } }
        }
        self.invalidate();
    }
    unsafe fn primary(&mut self) {
        if self.uninstall {
            if self.page == 0 { uninstall(self); self.go(1); } else { delete_self_later(&self.app_dir); let _ = DestroyWindow(self.hwnd); }
            return;
        }
        match self.page {
            0..=2 => self.go(self.page + 1),
            3 => {
                self.go(4);
                self.working = true;
                self.progress = Ease::at(0.0);
                self.progress.to = 0.55;
                // let the note fly off first, then do the work
                SetTimer(self.hwnd, 7, 450, None);
            }
            _ => {
                if self.done && self.error.is_none() {
                    let exe = wpath(&self.app_dir.join("notedd.exe"));
                    ShellExecuteW(None, w!("open"), PCWSTR(exe.as_ptr()), None, None, SW_SHOWNORMAL);
                }
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
    unsafe fn secondary(&mut self) {
        if self.uninstall || (self.page == 4 && self.done) { let _ = DestroyWindow(self.hwnd); return; }
        if self.page > 0 && self.page < 4 { self.go(self.page - 1); }
    }
    unsafe fn run_install(&mut self) {
        // NOTEDD_SETUP_DRY=1 walks through the whole setup without touching anything (for trying it out)
        let r = if std::env::var_os("NOTEDD_SETUP_DRY").is_some() { Ok(()) } else { install(self) };
        self.error = r.err();
        self.progress.to = 1.0;
        self.start_anim();
    }
    /// a key pressed while the hotkey listens
    unsafe fn capture(&mut self, vk: u32) {
        if matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5) { return; }
        if vk == VK_ESCAPE.0 as u32 { self.capturing = false; self.invalidate(); return; }
        let down = |k: VIRTUAL_KEY| GetKeyState(k.0 as i32) < 0;
        let mut m = 0;
        if down(VK_CONTROL) { m |= 2; }
        if down(VK_MENU) { m |= 1; }
        if down(VK_SHIFT) { m |= 4; }
        if down(VK_LWIN) || down(VK_RWIN) { m |= 8; }
        if m == 0 && !(0x70..=0x87).contains(&vk) { return; }
        self.keys = (m, vk);
        self.capturing = false;
        self.invalidate();
    }
    unsafe fn key(&mut self, vk: u32) {
        if self.capturing { self.capture(vk); return; }
        if self.working { return; }
        if let Some(i) = self.language_menu {
            let n = Language::ALL.len();
            match VIRTUAL_KEY(vk as u16) {
                VK_ESCAPE | VK_TAB => self.language_menu = None,
                VK_UP => self.language_menu = Some((i + n - 1) % n),
                VK_DOWN => self.language_menu = Some((i + 1) % n),
                VK_RETURN | VK_SPACE => self.press(Hit::Language(i)),
                _ => return,
            }
            self.hover = None;
            self.invalidate();
            return;
        }
        match VIRTUAL_KEY(vk as u16) {
            VK_RETURN => self.primary(),
            VK_ESCAPE => { if !self.uninstall && (1..4).contains(&self.page) { self.secondary(); } else { let _ = DestroyWindow(self.hwnd); } }
            VK_LEFT | VK_RIGHT if self.page == 1 && !self.uninstall => {
                let i = if vk == VK_LEFT.0 as u32 { self.theme.saturating_sub(1) } else { (self.theme + 1).min(self.theme_names().len() - 1) };
                self.press(Hit::Theme(i));
            }
            VK_UP | VK_DOWN if self.page == 1 && !self.uninstall => self.press(Hit::Editor(vk == VK_DOWN.0 as u32)),
            VK_TAB if self.page == 0 && !self.uninstall => self.press(Hit::LanguageMenu),
            _ => {}
        }
        self.invalidate();
    }

    // ---------- animation ----------
    unsafe fn step(&mut self) {
        let now = Instant::now();
        let dt = (now - self.tick).as_secs_f32().min(0.05);
        self.tick = now;
        let k = 1.0 - (-dt * 15.0).exp();
        let ko = 1.0 - (-dt * 11.0).exp();
        let kf = 1.0 - (-dt * 22.0).exp();
        let mut settled = true;
        for c in self.cards.iter_mut() {
            if c.delay > 0.0 { c.delay -= dt; settled = false; continue; }
            // a note coming in shows up quickly; one leaving stays visible while it flies
            let kk = if c.to.o > c.cur.o { 1.0 - (-dt * 22.0).exp() } else { ko };
            c.cur.lerp(&c.to, k, kk);
            if c.cur.near(&c.to) { c.cur = c.to; } else { settled = false; }
        }
        if self.intro_wait > 0.0 { self.intro_wait -= dt; settled = false; }
        else { self.intro.to = 1.0; settled &= self.intro.step(1.0 - (-dt * 11.0).exp(), 0.002); }
        for s in self.sw.iter_mut() { settled &= s.step(kf, 0.003); }
        settled &= self.editor_pill.step(kf, 0.003);
        let theme = self.theme;
        for (i, e) in self.sel_k.iter_mut().enumerate() { e.to = (i == theme) as u8 as f32; settled &= e.step(kf, 0.003); }
        for (i, e) in self.hover_k.iter_mut().enumerate() { e.to = if self.hover.map_or(false, |h| hit_index(h) == i) { 1.0 } else { 0.0 }; settled &= e.step(kf, 0.01); }
        if self.working {
            self.progress.step(1.0 - (-dt * 4.0).exp(), 0.004);
            settled = false;
            if self.progress.to >= 1.0 && self.progress.v > 0.995 {
                self.working = false;
                self.done = true;
                if self.error.is_none() {
                    // the thrown notes come back from both sides, one after another, and fan out behind
                    self.done_t = 0.0;
                    self.finale = true;
                    let page = self.page;
                    for (i, c) in self.cards.iter_mut().enumerate().take(page) {
                        c.to = fan_v(i);
                        c.delay = 0.06 * (page - 1 - i) as f32;
                    }
                    if let Some(c) = self.cards.get_mut(page) { c.cur.y += 10.0; c.cur.sc = 0.985; }
                }
            }
        }
        if self.finale && self.done_t < 1.6 {
            self.done_t += dt;
            settled = false;
            // then the fan closes and they file in under the last note: the stack from the start, done
            if !self.fan_closed && self.done_t > 0.95 {
                self.fan_closed = true;
                let page = self.page;
                for (i, c) in self.cards.iter_mut().enumerate().take(page) { c.to = stack_v(i, page - i); c.delay = 0.04 * (page - 1 - i) as f32; }
            }
        }
        if settled { self.animating = false; }
    }

    // ---------- drawing ----------
    unsafe fn ensure_target(&mut self) -> Result<ID2D1HwndRenderTarget> {
        if self.target.is_none() {
            let mut rc = RECT::default();
            GetClientRect(self.hwnd, &mut rc)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0 * self.scale, dpiY: 96.0 * self.scale, usage: D2D1_RENDER_TARGET_USAGE_NONE, minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES { hwnd: self.hwnd, pixelSize: D2D_SIZE_U { width: rc.right as u32, height: rc.bottom as u32 }, presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY };
            self.target = Some(self.factory.CreateHwndRenderTarget(&props, &hp)?);
        }
        Ok(self.target.clone().unwrap())
    }
    unsafe fn brush(&self, t: &ID2D1HwndRenderTarget, hex: u32, o: f32) -> Result<ID2D1SolidColorBrush> { t.CreateSolidColorBrush(&rgb(hex, o), None) }
    unsafe fn text(&self, t: &ID2D1HwndRenderTarget, s: &str, f: &IDWriteTextFormat, r: D2D_RECT_F, hex: u32, o: f32) -> Result<()> {
        if s.is_empty() || o < 0.01 { return Ok(()); }
        t.DrawText(&u(s), f, &r, &self.brush(t, hex, o)?, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        Ok(())
    }
    unsafe fn render(&mut self) -> Result<()> {
        let t = self.ensure_target()?;
        t.BeginDraw();
        t.Clear(Some(&rgb(0, 0.0)));
        // bottom of the stack first; the notes already thrown are above everything
        let order: Vec<usize> = if self.finale { (0..self.cards.len()).collect() } else { (0..self.cards.len()).rev().collect() };
        for i in order {
            let v = self.cards[i].cur;
            if v.o < 0.004 { continue; }
            let m = card_matrix(&v);
            if v.o < 0.995 {
                // a fading note fades as one picture, so its outline does not show through its paper
                let layer = t.CreateLayer(None)?;
                t.SetTransform(&shift(0.0, 0.0));
                let big = 1.0e6;
                t.PushLayer(&D2D1_LAYER_PARAMETERS {
                    contentBounds: rect(-big, -big, big, big), geometricMask: std::mem::ManuallyDrop::new(None),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, maskTransform: shift(0.0, 0.0), opacity: v.o,
                    opacityBrush: std::mem::ManuallyDrop::new(None), layerOptions: D2D1_LAYER_OPTIONS_NONE,
                }, &layer);
                t.SetTransform(&m);
                let r = self.draw_card(&t, &m, i);
                t.PopLayer();
                r?;
            } else {
                t.SetTransform(&m);
                self.draw_card(&t, &m, i)?;
            }
        }
        t.SetTransform(&shift(0.0, 0.0));
        if let Err(e) = t.EndDraw(None, None) { if e.code() == D2DERR_RECREATE_TARGET { self.target = None; } }
        Ok(())
    }
    unsafe fn soft_shadow(&self, t: &ID2D1HwndRenderTarget, r: D2D_RECT_F, rad: f32, dy: f32, blur: f32, alpha: f32) -> Result<()> {
        let n = 12;
        let b = self.brush(t, 0x2a2620, alpha / n as f32 * 1.7)?;
        for i in 0..n {
            let e = -blur * 0.35 + blur * 0.9 * i as f32 / (n - 1) as f32;
            t.FillRoundedRectangle(&rrect(r.left - e, r.top + dy - e, r.right + e, r.bottom + dy + e, (rad + e).max(0.0)), &b);
        }
        Ok(())
    }
    unsafe fn folder(&self, w: f32, h: f32, tx: f32, tabw: f32) -> Result<ID2D1PathGeometry> {
        let bx = tx + tabw;
        let g = self.factory.CreatePathGeometry()?;
        let s = g.Open()?;
        let arc = |x: f32, y: f32, r: f32, cw: bool| s.AddArc(&D2D1_ARC_SEGMENT { point: pt(x, y), size: D2D_SIZE_F { width: r, height: r }, rotationAngle: 0.0,
            sweepDirection: if cw { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE }, arcSize: D2D1_ARC_SIZE_SMALL });
        s.BeginFigure(pt(0.0, h - R), D2D1_FIGURE_BEGIN_FILLED);
        s.AddLine(pt(0.0, T + R)); arc(R, T, R, true);
        s.AddLine(pt(tx - F, T)); arc(tx, T - F, F, false);
        s.AddLine(pt(tx, RT)); arc(tx + RT, 0.0, RT, true);
        s.AddLine(pt(bx - RT, 0.0)); arc(bx, RT, RT, true);
        s.AddLine(pt(bx, T - F)); arc(bx + F, T, F, false);
        s.AddLine(pt(w - R, T)); arc(w, T + R, R, true);
        s.AddLine(pt(w, h - R)); arc(w - R, h, R, true);
        s.AddLine(pt(R, h)); arc(0.0, h - R, R, true);
        s.EndFigure(D2D1_FIGURE_END_CLOSED);
        s.Close()?;
        Ok(g)
    }
    unsafe fn draw_card(&self, t: &ID2D1HwndRenderTarget, m: &Matrix3x2, p: usize) -> Result<()> {
        let bg = self.card_color(p);
        let (tx, tabw) = (self.tab_x(p), self.tab_w(p));
        // one shadow under the whole stack, plus one under the note on top and the ones flying
        let alone = p <= self.page || p == self.cards.len() - 1 || self.cards[p].cur.o < 0.995;
        self.soft_shadow(t, rect(0.0, T, W, H), R, 10.0, 22.0, if alone { 0.10 } else { 0.025 })?;
        let g = self.folder(W, H, tx, tabw)?;
        t.FillGeometry(&g, &self.brush(t, bg, 1.0)?, None);
        t.DrawGeometry(&g, &self.brush(t, LINE, 0.9)?, 1.4, None);
        self.text(t, self.card_label(p), &self.fonts.tab, rect(tx + RT - 2.0, 1.0, tx + tabw, T + 1.0), INK, if p >= self.page { 1.0 } else { 0.9 })?;
        // under the top note nothing else shows
        if p != self.page && ((self.cards[p].cur.o > 0.995 && p > self.page) || self.finale) { return Ok(()); }
        // close: a little round capsule on the tab row, like the app's controls
        let hk = self.hk(p, Hit::Close);
        let (cx, cy) = (W - 27.0, T / 2.0);
        t.FillEllipse(&circle(cx, cy, 13.0), &self.brush(t, mix(bg, 0xffffff, 0.5), 1.0)?);
        t.DrawEllipse(&circle(cx, cy, 13.0), &self.brush(t, LINE, 0.9)?, 1.4, None);
        if hk > 0.01 { t.FillEllipse(&circle(cx, cy, 12.0), &self.brush(t, INK, 0.07 * hk)?); }
        let cb = self.brush(t, mix(INK2, INK, hk), 1.0)?;
        t.DrawLine(pt(cx - 4.5, cy - 4.5), pt(cx + 4.5, cy + 4.5), &cb, 1.5, &self.round);
        t.DrawLine(pt(cx + 4.5, cy - 4.5), pt(cx - 4.5, cy + 4.5), &cb, 1.5, &self.round);
        self.draw_page(t, m, p, bg)
    }
    /// hover belongs to the note on top only
    fn hk(&self, p: usize, h: Hit) -> f32 { if p == self.page { self.hover_k[hit_index(h)].v } else { 0.0 } }
    unsafe fn button(&self, t: &ID2D1HwndRenderTarget, p: usize, h: Hit, r: D2D_RECT_F, label: &str, primary: bool, o: f32) -> Result<()> {
        let hk = self.hk(p, h);
        let down = p == self.page && self.press == Some(h) && self.hover == Some(h);
        let s = if down { 1.5 } else { 0.0 };
        let rr = rrect(r.left + s, r.top + s, r.right - s, r.bottom - s, (r.bottom - r.top) / 2.0 - s);
        if primary {
            t.FillRoundedRectangle(&rr, &self.brush(t, mix(DARK, 0x3a3935, hk), o)?);
            self.text(t, label, &self.fonts.btn, r, ON, o)?;
        } else {
            if hk > 0.01 { t.FillRoundedRectangle(&rr, &self.brush(t, INK, 0.06 * hk * o)?); }
            self.text(t, label, &self.fonts.btn, r, mix(INK2, INK, hk), o)?;
        }
        Ok(())
    }
    /// an outlined pill, the quiet button
    unsafe fn pill(&self, t: &ID2D1HwndRenderTarget, p: usize, h: Hit, r: D2D_RECT_F, label: &str, bg: u32) -> Result<()> {
        let hk = self.hk(p, h);
        let rad = (r.bottom - r.top) / 2.0;
        t.FillRoundedRectangle(&rrect(r.left, r.top, r.right, r.bottom, rad), &self.brush(t, mix(mix(bg, 0xffffff, 0.55), bg, hk * 0.5), 1.0)?);
        t.DrawRoundedRectangle(&rrect(r.left + 0.5, r.top + 0.5, r.right - 0.5, r.bottom - 0.5, rad - 0.5), &self.brush(t, LINE, 0.22 + 0.25 * hk)?, 1.0, None);
        self.text(t, label, &self.fonts.btn, r, INK, 1.0)
    }
    unsafe fn draw_language_menu(&self, t: &ID2D1HwndRenderTarget, r: D2D_RECT_F, o: f32) -> Result<()> {
        let open = self.language_menu;
        let hk = self.hk(0, Hit::LanguageMenu);
        let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
        t.FillEllipse(&circle(cx, cy, BTN_H / 2.0), &self.brush(t, if open.is_some() { DARK } else { INK }, if open.is_some() { o } else { (0.04 + 0.06 * hk) * o })?);
        t.DrawEllipse(&circle(cx, cy, BTN_H / 2.0 - 0.5), &self.brush(t, LINE, (0.24 + 0.25 * hk) * o)?, 1.0, None);
        let globe = self.brush(t, if open.is_some() { ON } else { INK }, o)?;
        t.DrawEllipse(&circle(cx, cy, 9.0), &globe, 1.35, None);
        t.DrawEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: 4.3, radiusY: 9.0 }, &globe, 1.2, None);
        t.DrawLine(pt(cx - 8.5, cy), pt(cx + 8.5, cy), &globe, 1.2, None);
        if let Some(focus) = open {
            let p = self.language_panel();
            let panel = rrect(p.left, p.top, p.right, p.bottom, 13.0);
            self.soft_shadow(t, p, 13.0, 5.0, 12.0, 0.14)?;
            t.FillRoundedRectangle(&panel, &self.brush(t, PAPER, 1.0)?);
            t.DrawRoundedRectangle(&panel, &self.brush(t, LINE, 0.24)?, 1.0, None);
            for (i, (lang, row)) in Language::ALL.into_iter().zip(self.language_rows()).enumerate() {
                let current = locale::current() == lang;
                let hot = if matches!(self.hover, Some(Hit::Language(_))) { self.hover == Some(Hit::Language(i)) } else { i == focus };
                if hot {
                    t.FillRoundedRectangle(&rrect(row.left, row.top, row.right, row.bottom, 8.0), &self.brush(t, INK, 0.08)?);
                }
                self.text(t, lang.name(), &self.fonts.btn, rect(row.left, row.top, row.right - 20.0, row.bottom), INK, 1.0)?;
                if current {
                    let x = row.right - 16.0;
                    let y = (row.top + row.bottom) / 2.0;
                    let b = self.brush(t, INK, 1.0)?;
                    t.DrawLine(pt(x - 5.0, y), pt(x - 1.5, y + 3.5), &b, 1.5, &self.round);
                    t.DrawLine(pt(x - 1.5, y + 3.5), pt(x + 5.0, y - 4.0), &b, 1.5, &self.round);
                }
            }
        }
        Ok(())
    }
    unsafe fn switch(&self, t: &ID2D1HwndRenderTarget, p: usize, r: D2D_RECT_F, label: &str, note: &str, i: usize, h: Hit, bg: u32) -> Result<()> {
        let v = self.sw[i].v;
        let hk = self.hk(p, h);
        let cy = (r.top + r.bottom) / 2.0;
        if note.is_empty() {
            self.text(t, label, &self.fonts.label, rect(r.left + 8.0, r.top, r.right - 70.0, r.bottom), INK, 1.0)?;
        } else {
            self.text(t, label, &self.fonts.label, rect(r.left + 8.0, cy - 22.0, r.right - 70.0, cy + 1.0), INK, 1.0)?;
            self.text(t, note, &self.fonts.small, rect(r.left + 8.0, cy + 1.0, r.right - 70.0, cy + 22.0), INK2, 1.0)?;
        }
        let (x1, x0) = (r.right - 8.0, r.right - 56.0);
        t.FillRoundedRectangle(&rrect(x0, cy - 14.0, x1, cy + 14.0, 14.0), &self.brush(t, mix(mix(bg, INK, 0.12 + 0.05 * hk), DARK, v), 1.0)?);
        t.FillEllipse(&circle(x0 + 14.0 + v * 20.0, cy, 11.0), &self.brush(t, 0xffffff, 1.0)?);
        Ok(())
    }
    /// keys as flat keycaps, the same height as the buttons beside them
    unsafe fn keycaps(&self, t: &ID2D1HwndRenderTarget, x: f32, y: f32, bg: u32) -> Result<f32> {
        let mut x = x;
        for k in key_parts(self.keys.0, self.keys.1) {
            let w = self.text_w(&k, &self.fonts.cap).max(16.0) + 30.0;
            t.FillRoundedRectangle(&rrect(x, y, x + w, y + BTN_H, 11.0), &self.brush(t, mix(bg, 0xffffff, 0.7), 1.0)?);
            t.DrawRoundedRectangle(&rrect(x + 0.5, y + 0.5, x + w - 0.5, y + BTN_H - 0.5, 10.5), &self.brush(t, LINE, 0.22)?, 1.0, None);
            self.text(t, &k, &self.fonts.cap, rect(x, y, x + w, y + BTN_H), INK, 1.0)?;
            x += w + 8.0;
        }
        Ok(x)
    }
    unsafe fn draw_page(&self, t: &ID2D1HwndRenderTarget, _m: &Matrix3x2, p: usize, bg: u32) -> Result<()> {
        let title_r = rect(PAD, TOP, W - PAD, TOP + 40.0);
        let sub_r = rect(PAD, TOP + 48.0, W - PAD, TOP + 96.0);
        let mut bo = 1.0; // the buttons' opacity: on the hello note they come with the text
        if self.uninstall {
            if p == 0 {
                self.text(t, ui::REMOVE_PROMPT.get(), &self.fonts.title, title_r, INK, 1.0)?;
                self.text(t, &ui::REMOVE_DETAILS.get().replace("{path}", &self.notes_dir.to_string_lossy()), &self.fonts.sub, rect(PAD, TOP + 48.0, W - PAD, TOP + 140.0), INK2, 1.0)?;
            } else {
                self.text(t, ui::REMOVED.get(), &self.fonts.title, title_r, INK, 1.0)?;
                self.text(t, ui::REMOVED_DETAILS.get(), &self.fonts.sub, sub_r, INK2, 1.0)?;
            }
        } else {
            match p {
                0 => {
                    let k = self.intro.v;
                    let dy = (1.0 - k) * 18.0;
                    self.text(t, "Notedd", &self.fonts.big, rect(0.0, T + 112.0 + dy, W, T + 196.0 + dy), INK, k)?;
                    let k2 = ((k - 0.25) / 0.75).clamp(0.0, 1.0);
                    self.text(t, ui::WELCOME.get(), &self.fonts.subc, rect(60.0, T + 206.0 + dy * 1.4, W - 60.0, T + 266.0 + dy * 1.4), INK2, k2)?;
                    bo = ((k - 0.5) / 0.5).clamp(0.0, 1.0);
                }
                1 => {
                    self.text(t, ui::THEME.get(), &self.fonts.title, title_r, INK, 1.0)?;
                    self.text(t, ui::THEME_HELP.get(), &self.fonts.sub, sub_r, INK2, 1.0)?;
                    let (tiles, _) = self.tiles();
                    for (i, (r, name)) in tiles.iter().zip(self.theme_names()).enumerate() {
                        let sel = self.sel_k[i].v;
                        let hk = self.hk(p, Hit::Theme(i));
                        match i {
                            0 => self.tile(t, *r, TILE_LIGHT)?,
                            1 => self.tile(t, *r, TILE_DARK)?,
                            _ => {
                                // Windows decides: half light, half dark
                                self.tile(t, *r, TILE_LIGHT)?;
                                t.PushAxisAlignedClip(&rect((r.left + r.right) / 2.0, r.top, r.right, r.bottom), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                                self.tile(t, *r, TILE_DARK)?;
                                t.PopAxisAlignedClip();
                            }
                        }
                        let ring = rrect(r.left - 4.5, r.top - 4.5, r.right + 4.5, r.bottom + 4.5, 18.5);
                        if sel > 0.01 { t.DrawRoundedRectangle(&ring, &self.brush(t, DARK, sel)?, 2.0, None); }
                        if hk > 0.01 && sel < 0.99 { t.DrawRoundedRectangle(&ring, &self.brush(t, DARK, 0.25 * hk * (1.0 - sel))?, 2.0, None); }
                        let fnt = &self.fonts.btn;
                        self.text(t, name, fnt, rect(r.left - 8.0, r.bottom + 10.0, r.right + 8.0, r.bottom + 34.0), mix(INK2, INK, sel.max(hk)), 1.0)?;
                    }
                    let choices = self.editor_choices();
                    let r = choices[0].2;
                    let right = r.left - CONTROL_GAP;
                    self.text(t, ui::EDITOR.get(), &self.fonts.label, rect(PAD, r.top, right, r.top + 23.0), INK, 1.0)?;
                    let hint = if self.simple_editor { ui::SIMPLE_HELP.get() } else { ui::MARKDOWN_HELP.get() };
                    self.text(t, hint, &self.fonts.small, rect(PAD, r.top + 23.0, right, r.bottom), INK2, 1.0)?;
                    let shell = rrect(r.left, r.top, choices[1].2.right, r.bottom, BTN_H / 2.0);
                    t.FillRoundedRectangle(&shell, &self.brush(t, INK, 0.05)?);
                    t.DrawRoundedRectangle(&shell, &self.brush(t, LINE, 0.35)?, 1.0, None);
                    let slide = self.editor_pill.v;
                    let x = r.left + (r.right - r.left) * slide;
                    t.FillRoundedRectangle(&rrect(x + 3.0, r.top + 3.0, x + r.right - r.left - 3.0, r.bottom - 3.0, BTN_H / 2.0 - 3.0), &self.brush(t, DARK, 1.0)?);
                    for (simple, label, r) in choices {
                        let base = mix(INK2, INK, self.hk(p, Hit::Editor(simple)));
                        self.text(t, label, &self.fonts.btn, r, mix(base, ON, if simple { slide } else { 1.0 - slide }), 1.0)?;
                    }
                }
                2 => {
                    self.text(t, ui::SHORTCUT.get(), &self.fonts.title, title_r, INK, 1.0)?;
                    self.text(t, ui::SHORTCUT_HELP.get(), &self.fonts.sub, sub_r, INK2, 1.0)?;
                    let y = TOP + 110.0;
                    if self.capturing && p == self.page {
                        let w = self.caps_width().max(200.0);
                        let r = rrect(PAD + 0.5, y + 0.5, PAD + w - 0.5, y + BTN_H - 0.5, 11.0);
                        t.FillRoundedRectangle(&r, &self.brush(t, mix(bg, 0xffffff, 0.5), 1.0)?);
                        t.DrawRoundedRectangle(&r, &self.brush(t, DARK, 0.8)?, 1.3, &self.dash);
                        self.text(t, ui::PRESS_SHORTCUT.get(), &self.fonts.label, rect(PAD + 18.0, y, PAD + w, y + BTN_H), INK, 1.0)?;
                        self.text(t, ui::ESC_CANCEL.get(), &self.fonts.small, rect(PAD + w + 16.0, y, W - PAD, y + BTN_H), INK2, 1.0)?;
                    } else {
                        let x = self.keycaps(t, PAD, y, bg)? + KEY_CHANGE_GAP;
                        self.pill(t, p, Hit::Keys, rect(x, y, x + self.change_w(), y + BTN_H), ui::CHANGE.get(), bg)?;
                    }
                    self.switch(t, p, rect(PAD - 8.0, TOP + 196.0, W - PAD + 8.0, TOP + 246.0), ui::AUTOSTART.get(), ui::AUTOSTART_HELP.get(), 0, Hit::Auto, bg)?;
                }
                3 => {
                    self.text(t, ui::FOLDERS.get(), &self.fonts.title, title_r, INK, 1.0)?;
                    self.text(t, ui::FOLDERS_HELP.get(), &self.fonts.sub, sub_r, INK2, 1.0)?;
                    let bw = self.change_w() + 4.0;
                    for (i, (label, path, h)) in [(ui::NOTES.get(), &self.notes_dir, Hit::NotesDir), (ui::APP.get(), &self.app_dir, Hit::AppDir)].into_iter().enumerate() {
                        let y = Self::row_y(i);
                        let room = W - PAD - bw - 20.0 - PAD;
                        self.text(t, label, &self.fonts.small, rect(PAD, y - 2.0, PAD + room, y + 20.0), INK2, 1.0)?;
                        self.text(t, &self.ellipsize(&path.to_string_lossy(), room), &self.fonts.label, rect(PAD, y + 20.0, PAD + room, y + 48.0), INK, 1.0)?;
                        self.pill(t, p, h, rect(W - PAD - bw, y + 4.0, W - PAD, y + 4.0 + BTN_H), ui::CHANGE.get(), bg)?;
                    }
                    self.switch(t, p, rect(PAD - 8.0, Self::row_y(2) + 2.0, W - PAD + 8.0, Self::row_y(2) + 46.0), ui::DESKTOP_SHORTCUT.get(), "", 1, Hit::Desktop, bg)?;
                }
                _ => {
                    if let Some(e) = &self.error {
                        self.text(t, ui::INSTALL_FAILED.get(), &self.fonts.title, title_r, INK, 1.0)?;
                        self.text(t, e, &self.fonts.sub, rect(PAD, TOP + 48.0, W - PAD, TOP + 150.0), INK2, 1.0)?;
                    } else if self.done {
                        let k = ((self.done_t - 0.25) / 0.4).clamp(0.0, 1.0);
                        let dy = (1.0 - k) * 12.0;
                        self.text(t, ui::DONE.get(), &self.fonts.big, rect(0.0, T + 86.0 + dy, W, T + 170.0 + dy), INK, k)?;
                        self.text(t, ui::READY_HELP.get(), &self.fonts.subc, rect(70.0, T + 178.0 + dy, W - 70.0, T + 224.0 + dy), INK2, k)?;
                        let k2 = ((self.done_t - 0.45) / 0.4).clamp(0.0, 1.0);
                        if k2 > 0.0 {
                            let x = (W - self.caps_width() + 8.0) / 2.0;
                            t.PushLayer(&D2D1_LAYER_PARAMETERS {
                                contentBounds: rect(-1.0e6, -1.0e6, 1.0e6, 1.0e6), geometricMask: std::mem::ManuallyDrop::new(None),
                                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, maskTransform: shift(0.0, 0.0), opacity: k2,
                                opacityBrush: std::mem::ManuallyDrop::new(None), layerOptions: D2D1_LAYER_OPTIONS_NONE,
                            }, &t.CreateLayer(None)?);
                            self.keycaps(t, x, T + 246.0 + (1.0 - k2) * 10.0, bg)?;
                            t.PopLayer();
                        }
                    } else {
                        self.text(t, ui::INSTALLING.get(), &self.fonts.title, title_r, INK, 1.0)?;
                        self.text(t, ui::WAIT.get(), &self.fonts.sub, sub_r, INK2, 1.0)?;
                        let (x0, x1, y) = (PAD, W - PAD, TOP + 120.0);
                        t.FillRoundedRectangle(&rrect(x0, y, x1, y + 8.0, 4.0), &self.brush(t, INK, 0.08)?);
                        t.FillRoundedRectangle(&rrect(x0, y, x0 + (x1 - x0) * self.progress.v.max(0.02), y + 8.0, 4.0), &self.brush(t, DARK, 1.0)?);
                    }
                }
            }
        }
        for (h, r) in self.hits(p) {
            match h {
                Hit::Primary => self.button(t, p, h, r, &self.primary_label(p), true, if p == 4 && self.done { ((self.done_t - 0.7) / 0.4).clamp(0.0, 1.0) } else { bo })?,
                Hit::Secondary => self.button(t, p, h, r, &self.secondary_label(p), false, if p == 4 && self.done { ((self.done_t - 0.7) / 0.4).clamp(0.0, 1.0) } else { bo })?,
                Hit::LanguageMenu => self.draw_language_menu(t, r, bo)?,
                _ => {}
            }
        }
        Ok(())
    }
    /// a theme as a plain block of its paper with its text on it
    unsafe fn tile(&self, t: &ID2D1HwndRenderTarget, r: D2D_RECT_F, c: (u32, u32)) -> Result<()> {
        let (paper, ink) = c;
        t.FillRoundedRectangle(&rrect(r.left, r.top, r.right, r.bottom, 16.0), &self.brush(t, paper, 1.0)?);
        self.text(t, "Aa", &self.fonts.aa, r, ink, 1.0)?;
        t.DrawRoundedRectangle(&rrect(r.left + 0.5, r.top + 0.5, r.right - 0.5, r.bottom - 0.5, 15.5), &self.brush(t, LINE, 0.16)?, 1.0, None);
        Ok(())
    }
    unsafe fn ellipsize(&self, s: &str, room: f32) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = s.to_string();
        let mut cut = 0;
        while cut < chars.len() && self.text_w(&out, &self.fonts.label) > room {
            cut += 1;
            out = format!("…{}", chars[cut..].iter().collect::<String>());
        }
        out
    }
}
fn hit_index(h: Hit) -> usize {
    match h { Hit::Primary => 0, Hit::Secondary => 1, Hit::Close => 2, Hit::Theme(i) => 3 + i.min(3), Hit::Keys => 7, Hit::Auto | Hit::NotesDir => 8, Hit::Desktop | Hit::AppDir => 9, Hit::Language(i) => 10 + i, Hit::Editor(simple) => 10 + Language::ALL.len() + usize::from(simple), Hit::LanguageMenu => 12 + Language::ALL.len() }
}

fn dips(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xffff) as i16 as f32 / scale - OX, ((lp.0 >> 16) & 0xffff) as i16 as f32 / scale - OY)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let Some(a) = app() else { return DefWindowProcW(hwnd, msg, wp, lp) };
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                if !a.animating { let _ = a.render(); }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_NCHITTEST => {
                // the notes drag the window, the room around them lets clicks through
                let mut p = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
                let _ = ScreenToClient(hwnd, &mut p);
                let (x, y) = (p.x as f32 / a.scale - OX, p.y as f32 / a.scale - OY);
                let last = a.pages() - 1;
                let tabs_end = a.tab_x(last) + a.tab_w(last) + F;
                let on_card = (x >= 0.0 && x <= W && y >= T && y <= H) || (y >= -STACK + 4.0 && y < T && ((x >= X2 && x <= tabs_end) || x >= W - 44.0));
                if !on_card { return LRESULT(HTTRANSPARENT as isize); }
                if a.language_menu.is_some() || a.hit_at(x, y).is_some() { LRESULT(HTCLIENT as isize) } else { LRESULT(HTCAPTION as isize) }
            }
            WM_SETCURSOR => {
                if (lp.0 & 0xffff) as u32 == HTCLIENT {
                    if let Ok(c) = LoadCursorW(None, IDC_HAND) { SetCursor(c); }
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_MOUSEMOVE => {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
                let (x, y) = dips(lp, a.scale);
                let h = a.hit_at(x, y);
                if h != a.hover { a.hover = h; a.start_anim(); }
                LRESULT(0)
            }
            0x02A3 | WM_NCMOUSEMOVE => { if a.hover.is_some() { a.hover = None; a.start_anim(); } DefWindowProcW(hwnd, msg, wp, lp) }
            WM_LBUTTONDOWN => {
                let (x, y) = dips(lp, a.scale);
                a.press = a.hit_at(x, y);
                if !matches!(a.press, Some(Hit::LanguageMenu | Hit::Language(_))) { a.language_menu = None; }
                SetCapture(hwnd);
                a.invalidate();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let (x, y) = dips(lp, a.scale);
                if let Some(h) = a.press.take() { if a.hit_at(x, y) == Some(h) { a.press(h); } }
                a.invalidate();
                LRESULT(0)
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => { a.key(wp.0 as u32); LRESULT(0) }
            WM_ACTIVATE if wp.0 & 0xffff == 0 => { a.language_menu = None; a.invalidate(); LRESULT(0) }
            WM_SYSCHAR | WM_CHAR => LRESULT(0),
            WM_TIMER => {
                if wp.0 == 7 { let _ = KillTimer(hwnd, 7); a.run_install(); }
                if wp.0 == 8 && a.animating { a.step(); let _ = a.render(); }
                LRESULT(0)
            }
            WM_DESTROY => { PostQuitMessage(0); LRESULT(0) }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

unsafe fn make_format(dw: &IDWriteFactory5, coll: &IDWriteFontCollection, size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT, vcenter: bool, wrap: bool) -> Result<IDWriteTextFormat> {
    let f = dw.CreateTextFormat(w!("Geologica"), coll, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("ru-ru"))?;
    f.cast::<IDWriteTextFormat1>()?.SetFontFallback(&dw.GetSystemFontFallback()?)?;
    f.SetTextAlignment(align)?;
    if vcenter { f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?; }
    if !wrap { f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?; }
    Ok(f)
}
fn hex_of(s: &str) -> Option<u32> { u32::from_str_radix(s.trim().trim_start_matches('#'), 16).ok() }

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let uninstall = std::env::args().any(|a| a == "--uninstall");
        if let Ok(h) = FindWindowW(w!("NoteddSetup"), None) { if !h.0.is_null() { let _ = SetForegroundWindow(h); return Ok(()); } }
        let s = old_settings();
        let language = setting(&s, "language").as_deref().and_then(Language::from_tag);
        let simple_editor = s.lines().rev().filter_map(|line| line.split_once('='))
            .find(|(key, _)| key.trim() == "editor").is_some_and(|(_, value)| value.trim() == "simple");
        locale::set(language);
        let instance = GetModuleHandleW(None)?;

        let font_path = std::env::temp_dir().join("notedd-setup-geologica.ttf");
        let _ = std::fs::write(&font_path, FONT);
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let dw5: IDWriteFactory5 = dw.cast()?;
        let builder = dw5.CreateFontSetBuilder()?;
        builder.AddFontFile(&dw5.CreateFontFileReference(PCWSTR(wpath(&font_path).as_ptr()), None)?)?;
        let coll: IDWriteFontCollection = dw5.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?)?.cast()?;
        let lead = DWRITE_TEXT_ALIGNMENT_LEADING;
        let center = DWRITE_TEXT_ALIGNMENT_CENTER;
        let fonts = Fonts {
            big: make_format(&dw5, &coll, 68.0, DWRITE_FONT_WEIGHT_BOLD, center, true, false)?,
            title: make_format(&dw5, &coll, 30.0, DWRITE_FONT_WEIGHT_BOLD, lead, false, false)?,
            sub: make_format(&dw5, &coll, 15.5, DWRITE_FONT_WEIGHT_NORMAL, lead, false, true)?,
            subc: make_format(&dw5, &coll, 16.5, DWRITE_FONT_WEIGHT_NORMAL, center, false, true)?,
            label: make_format(&dw5, &coll, 15.0, DWRITE_FONT_WEIGHT_MEDIUM, lead, true, false)?,
            small: make_format(&dw5, &coll, 13.0, DWRITE_FONT_WEIGHT_NORMAL, lead, true, false)?,
            btn: make_format(&dw5, &coll, 15.0, DWRITE_FONT_WEIGHT_MEDIUM, center, true, false)?,
            cap: make_format(&dw5, &coll, 15.0, DWRITE_FONT_WEIGHT_MEDIUM, center, true, false)?,
            tab: make_format(&dw5, &coll, 15.0, DWRITE_FONT_WEIGHT_MEDIUM, lead, true, false)?,
            titlec: make_format(&dw5, &coll, 30.0, DWRITE_FONT_WEIGHT_BOLD, center, true, false)?,
            aa: make_format(&dw5, &coll, 34.0, DWRITE_FONT_WEIGHT_BOLD, center, true, false)?,
        };
        let round = factory.CreateStrokeStyle(&D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, dashCap: D2D1_CAP_STYLE_FLAT,
            lineJoin: D2D1_LINE_JOIN_ROUND, miterLimit: 10.0, dashStyle: D2D1_DASH_STYLE_SOLID, dashOffset: 0.0,
        }, None)?;
        let dash = factory.CreateStrokeStyle(&D2D1_STROKE_STYLE_PROPERTIES { dashStyle: D2D1_DASH_STYLE_DASH, miterLimit: 10.0, ..Default::default() }, None)?;

        let class = w!("NoteddSetup");
        let icon = std::env::temp_dir().join("notedd-setup.ico");
        let _ = std::fs::write(&icon, ICON);
        let hicon = LoadImageW(None, PCWSTR(wpath(&icon).as_ptr()), IMAGE_ICON, 0, 0, LR_LOADFROMFILE | LR_DEFAULTSIZE).map(|h| HICON(h.0)).unwrap_or_default();
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance.into(), lpszClassName: class, hIcon: hicon, hCursor: LoadCursorW(None, IDC_ARROW)?, ..Default::default() });
        let scale = GetDpiForSystem() as f32 / 96.0;
        let (pw, ph) = (((W + 2.0 * M) * scale).ceil() as i32, ((H + 2.0 * M + STACK) * scale).ceil() as i32);
        let x = (GetSystemMetrics(SM_CXSCREEN) - pw) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - ph) * 2 / 5;
        let title = wide(if uninstall { ui::UNINSTALL_TITLE.get() } else { ui::INSTALL_TITLE.get() });
        let hwnd = CreateWindowExW(WS_EX_APPWINDOW, class, PCWSTR(title.as_ptr()), WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU, x, y, pw, ph, None, None, instance, None)?;
        let pref = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as _, 4);
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });

        let theme = setting(&s, "theme").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0).min(3);
        // the user's own palette: its paper, its first card colour and its text
        let custom = setting(&s, "cards2").and_then(|v| {
            let mut it = v.split(',').filter_map(hex_of);
            let (d, c) = (it.next()?, it.next()?);
            let ink = setting(&s, "ink2").and_then(|v| hex_of(&v)).unwrap_or(if luma(d) < 0.5 { 0xece8dc } else { 0x1c1b19 });
            Some([d, c, ink])
        });
        let custom = if theme == 3 { custom.or(Some([0xefebe3, 0xdde3d2, 0x1c1b19])) } else { None };
        let keys = setting(&s, "key0").and_then(|v| { let (m, k) = v.split_once(',')?; Some((m.parse().ok()?, k.parse().ok()?)) })
            .or_else(|| Some((setting(&s, "hk_mods")?.parse().ok()?, setting(&s, "hk_vk")?.parse().ok()?)))
            .filter(|k: &(u32, u32)| k.1 != 0).unwrap_or((3, 0x20));
        let (app_dir, notes_dir) = default_dirs();
        let app_dir = if uninstall { std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or(app_dir) } else { app_dir };

        // the opening: the notes land one after another, the bottom one first and the hello note last
        let pages = if uninstall { 2 } else { 5 };
        let tilt = [3.0, -4.0, 5.0, -3.0, 4.0];
        let cards: Vec<Card> = (0..pages).map(|i| {
            let to = stack_v(i, i);
            Card { cur: V { y: to.y + 70.0, o: 0.0, rot: tilt[i], ..to }, to, delay: 0.08 + 0.065 * (pages - 1 - i) as f32 }
        }).collect();
        let land = 0.08 + 0.065 * (pages - 1) as f32;
        let mut st = Box::new(Setup {
            hwnd, factory, dw, fonts, round, dash, target: None, scale, uninstall, page: 0, cards,
            intro: Ease::at(if uninstall { 1.0 } else { 0.0 }), intro_wait: land + 0.12, theme, theme0: theme, simple_editor,
            editor_pill: Ease::at(f32::from(u8::from(simple_editor))), custom,
            sel_k: [0, 1, 2, 3].map(|i| Ease::at((i == theme) as u8 as f32)), keys, capturing: false, autostart: true, desktop: false,
            sw: [Ease::at(1.0), Ease::at(0.0)], notes_dir, app_dir, progress: Ease::at(0.0), working: false, done: false, error: None,
            hover: None, press: None, language, language_menu: None, hover_k: [Ease::at(0.0); 13 + Language::ALL.len()], tick: Instant::now(), animating: true,
            done_t: 0.0, finale: false, fan_closed: false,
        });
        st.tick = Instant::now();
        APP = Box::into_raw(st);

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        SetTimer(hwnd, 8, 15, None);
        timeBeginPeriod(1);
        let mut msg = MSG::default();
        'run: loop {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT { break 'run; }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            match app() {
                Some(a) if a.animating => {
                    let t0 = Instant::now();
                    a.step();
                    let _ = a.render();
                    let _ = ValidateRect(a.hwnd, None);
                    let _ = DwmFlush();
                    while t0.elapsed().as_micros() < 4000 { Sleep(1); }
                }
                _ => { let _ = WaitMessage(); }
            }
        }
        timeEndPeriod(1);
    }
    Ok(())
}
