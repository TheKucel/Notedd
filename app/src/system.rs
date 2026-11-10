//! The system around the app. Tray icon, global shortcuts and the single instance go through
//! cross-platform crates; the main window itself is in `platform`.

use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton as TrayButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd { Show, Toggle, NewNote, OpenFolder, Quit }

static TX: OnceLock<async_channel::Sender<Cmd>> = OnceLock::new();
static ICO: &[u8] = include_bytes!("../../assets/notedd.ico");

fn send(c: Cmd) { if let Some(tx) = TX.get() { let _ = tx.try_send(c); } }

// ---- single instance ---------------------------------------------------------

/// true when this is the first instance; a second one wakes the first and gets false
#[cfg(windows)]
pub fn single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE};
    unsafe {
        // the handle stays open for the life of the process
        if CreateMutexW(None, true, w!("Local\\NoteddInstance")).is_err() { return true; }
        let ev = CreateEventW(None, false, false, w!("Local\\NoteddShow"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(ev) = ev { let _ = SetEvent(ev); }
            return false;
        }
        // the first instance waits for the others' knock
        let Ok(ev) = ev else { return true };
        let raw = ev.0 as usize;
        std::thread::spawn(move || loop {
            if WaitForSingleObject(HANDLE(raw as *mut _), INFINITE).0 != 0 { break; }
            send(Cmd::Show);
        });
        // the installer sets this to replace or remove the exe
        if let Ok(quit) = CreateEventW(None, false, false, w!("Local\\NoteddQuit")) {
            let raw = quit.0 as usize;
            std::thread::spawn(move || {
                if WaitForSingleObject(HANDLE(raw as *mut _), INFINITE).0 == 0 { send(Cmd::Quit); }
            });
        }
        true
    }
}

/// true when this is the first instance; a second one wakes the first and gets false
#[cfg(unix)]
pub fn single_instance() -> bool {
    use std::os::unix::net::{UnixListener, UnixStream};
    let user = std::env::var("USER").unwrap_or_default();
    let path = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join(format!("notedd-{user}.sock"));
    // somebody answers: the app runs already, and the knock brings it up
    if UnixStream::connect(&path).is_ok() { return false; }
    // nobody does: a socket file still there was left by a crash
    let _ = std::fs::remove_file(&path);
    if let Ok(l) = UnixListener::bind(&path) {
        std::thread::spawn(move || for c in l.incoming() { if c.is_ok() { send(Cmd::Show); } });
    }
    true
}

// ---- tray icon -----------------------------------------------------------------

thread_local! {
    /// the icon and its menu items (kept to rename them when the language changes); dropping it removes the icon
    static TRAY: RefCell<Option<(TrayIcon, [MenuItem; 3])>> = const { RefCell::new(None) };
    static HOTKEYS: RefCell<Option<(GlobalHotKeyManager, Vec<HotKey>)>> = const { RefCell::new(None) };
    /// the shortcuts registered last: every settings change calls set_hotkeys, most change something else
    static LAST_KEYS: std::cell::Cell<Option<[(u32, u32); 2]>> = const { std::cell::Cell::new(None) };
}

fn tray_image() -> Option<tray_icon::Icon> {
    let img = image::load_from_memory_with_format(ICO, image::ImageFormat::Ico).ok()?;
    let img = img.resize(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn make_tray() {
    use crate::strings as ui;
    let items = [
        MenuItem::with_id("show", ui::SHOW.get(), true, None),
        MenuItem::with_id("folder", ui::OPEN_NOTES_FOLDER.get(), true, None),
        MenuItem::with_id("quit", ui::QUIT.get(), true, None),
    ];
    let menu = Menu::new();
    let _ = menu.append_items(&[&items[0], &items[1], &items[2]]);
    // a click shows the notes; the menu is on the right button
    let mut b = TrayIconBuilder::new().with_tooltip("Notedd").with_menu(Box::new(menu)).with_menu_on_left_click(false);
    if let Some(i) = tray_image() { b = b.with_icon(i); }
    if let Ok(t) = b.build() { TRAY.with(|c| *c.borrow_mut() = Some((t, items))); }
}

/// The tray menu follows the language picked in the settings.
pub fn retitle_tray() {
    use crate::strings as ui;
    TRAY.with(|c| if let Some((_, it)) = &*c.borrow() {
        it[0].set_text(ui::SHOW.get());
        it[1].set_text(ui::OPEN_NOTES_FOLDER.get());
        it[2].set_text(ui::QUIT.get());
    });
}

/// Takes the tray icon away before the process ends.
pub fn remove_tray() { TRAY.with(|c| drop(c.borrow_mut().take())); }

// ---- global shortcuts -------------------------------------------------------------

/// bit i: global shortcut i could not be registered (another app holds it)
static TAKEN: AtomicU8 = AtomicU8::new(0);
/// the ids of the two registered shortcuts (0: none)
static IDS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

pub fn hotkey_taken(i: usize) -> bool { TAKEN.load(Ordering::Relaxed) & (1 << i) != 0 }

/// a shortcut as the settings keep it (Windows modifier bits, virtual-key code), for the hotkey crate
fn hotkey(mods: u32, vk: u32) -> Option<HotKey> {
    let mut m = HkMods::empty();
    if mods & 1 != 0 { m |= HkMods::ALT; }
    if mods & 2 != 0 { m |= HkMods::CONTROL; }
    if mods & 4 != 0 { m |= HkMods::SHIFT; }
    if mods & 8 != 0 { m |= HkMods::META; }
    const LETTERS: [Code; 26] = [Code::KeyA, Code::KeyB, Code::KeyC, Code::KeyD, Code::KeyE, Code::KeyF, Code::KeyG, Code::KeyH, Code::KeyI, Code::KeyJ, Code::KeyK, Code::KeyL, Code::KeyM,
        Code::KeyN, Code::KeyO, Code::KeyP, Code::KeyQ, Code::KeyR, Code::KeyS, Code::KeyT, Code::KeyU, Code::KeyV, Code::KeyW, Code::KeyX, Code::KeyY, Code::KeyZ];
    const DIGITS: [Code; 10] = [Code::Digit0, Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4, Code::Digit5, Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9];
    const FKEYS: [Code; 24] = [Code::F1, Code::F2, Code::F3, Code::F4, Code::F5, Code::F6, Code::F7, Code::F8, Code::F9, Code::F10, Code::F11, Code::F12,
        Code::F13, Code::F14, Code::F15, Code::F16, Code::F17, Code::F18, Code::F19, Code::F20, Code::F21, Code::F22, Code::F23, Code::F24];
    let code = match vk {
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x70..=0x87 => FKEYS[(vk - 0x70) as usize],
        0x20 => Code::Space, 0x0D => Code::Enter, 0x09 => Code::Tab, 0x1B => Code::Escape, 0x08 => Code::Backspace,
        0x2E => Code::Delete, 0x2D => Code::Insert, 0x24 => Code::Home, 0x23 => Code::End, 0x21 => Code::PageUp, 0x22 => Code::PageDown,
        0x25 => Code::ArrowLeft, 0x26 => Code::ArrowUp, 0x27 => Code::ArrowRight, 0x28 => Code::ArrowDown,
        0xBC => Code::Comma, 0xBE => Code::Period, 0xBD => Code::Minus, 0xBB => Code::Equal, 0xBF => Code::Slash, 0xC0 => Code::Backquote,
        0xDB => Code::BracketLeft, 0xDD => Code::BracketRight, 0xDC => Code::Backslash, 0xBA => Code::Semicolon, 0xDE => Code::Quote,
        _ => return None,
    };
    Some(HotKey::new(Some(m), code))
}

/// The global shortcuts changed in the settings (or at start). Call on the main thread.
pub fn set_hotkeys(keys: [(u32, u32); 2]) {
    if LAST_KEYS.with(|l| l.replace(Some(keys))) == Some(keys) { return; }
    HOTKEYS.with(|h| {
        let mut h = h.borrow_mut();
        let Some((m, held)) = h.as_mut() else { TAKEN.store(0b11, Ordering::Relaxed); return };
        let _ = m.unregister_all(held);
        held.clear();
        let mut taken = 0u8;
        for (i, &(mods, vk)) in keys.iter().enumerate() {
            IDS[i].store(0, Ordering::Relaxed);
            if vk == 0 { continue; }
            match hotkey(mods, vk) {
                Some(k) if m.register(k).is_ok() => { IDS[i].store(k.id(), Ordering::Relaxed); held.push(k); }
                _ => taken |= 1 << i,
            }
        }
        TAKEN.store(taken, Ordering::Relaxed);
    });
}

/// Puts up the tray icon and the global shortcuts; their commands arrive on `tx`.
/// Call on the main thread once the app runs: Windows and the Mac deliver both through its event loop.
pub fn start(keys: [(u32, u32); 2], tx: async_channel::Sender<Cmd>) {
    let _ = TX.set(tx);
    TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: TrayButton::Left, button_state: MouseButtonState::Up, .. } = e { send(Cmd::Show); }
    }));
    MenuEvent::set_event_handler(Some(|e: MenuEvent| match e.id.0.as_str() {
        "show" => send(Cmd::Show),
        "folder" => send(Cmd::OpenFolder),
        "quit" => send(Cmd::Quit),
        _ => {}
    }));
    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state != HotKeyState::Pressed || e.id == 0 { return; }
        if e.id == IDS[0].load(Ordering::Relaxed) { send(Cmd::Toggle); } else if e.id == IDS[1].load(Ordering::Relaxed) { send(Cmd::NewNote); }
    }));
    // GTK owns the tray on Linux: it gets a thread with its own main loop
    #[cfg(target_os = "linux")]
    std::thread::spawn(|| { if gtk::init().is_ok() { make_tray(); gtk::main(); } });
    #[cfg(not(target_os = "linux"))]
    make_tray();
    if let Ok(m) = GlobalHotKeyManager::new() { HOTKEYS.with(|h| *h.borrow_mut() = Some((m, Vec::new()))); }
    set_hotkeys(keys);
}

// ---- key names ---------------------------------------------------------------

const NAMED: &[(u32, &str, &str)] = &[
    (0x20, "space", "Space"), (0x0D, "enter", "Enter"), (0x09, "tab", "Tab"), (0x1B, "escape", "Esc"), (0x08, "backspace", "Backspace"),
    (0x2E, "delete", "Delete"), (0x2D, "insert", "Insert"), (0x24, "home", "Home"), (0x23, "end", "End"), (0x21, "pageup", "PgUp"),
    (0x22, "pagedown", "PgDn"), (0x25, "left", "←"), (0x26, "up", "↑"), (0x27, "right", "→"), (0x28, "down", "↓"),
    (0xBC, ",", ","), (0xBE, ".", "."), (0xBD, "-", "-"), (0xBB, "=", "="), (0xBF, "/", "/"), (0xC0, "`", "`"),
    (0xDB, "[", "["), (0xDD, "]", "]"), (0xDC, "\\", "\\"), (0xBA, ";", ";"), (0xDE, "'", "'"),
];

pub fn vk_to_key(vk: u32) -> Option<String> {
    if let Some((_, k, _)) = NAMED.iter().find(|n| n.0 == vk) { return Some(k.to_string()); }
    match vk {
        0x41..=0x5A | 0x30..=0x39 => Some((vk as u8 as char).to_ascii_lowercase().to_string()),
        0x70..=0x87 => Some(format!("f{}", vk - 0x6F)),
        _ => None,
    }
}

pub fn key_to_vk(key: &str) -> Option<u32> {
    if let Some(n) = NAMED.iter().find(|n| n.1 == key) { return Some(n.0); }
    let mut cs = key.chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c.to_ascii_uppercase() as u32),
        (Some('f'), Some(_)) => key[1..].parse::<u32>().ok().filter(|n| (1..=24).contains(n)).map(|n| 0x6F + n),
        _ => None,
    }
}

/// gpui binding string such as "ctrl-alt-n"
pub fn binding(mods: u32, vk: u32) -> Option<String> {
    let key = vk_to_key(vk)?;
    let mut s = String::new();
    if mods & 2 != 0 { s += "ctrl-"; }
    if mods & 1 != 0 { s += "alt-"; }
    if mods & 4 != 0 { s += "shift-"; }
    if mods & 8 != 0 { s += "super-"; }
    Some(s + &key)
}

pub fn mods_of(m: &gpui::Modifiers) -> u32 { (m.alt as u32) | (m.control as u32) << 1 | (m.shift as u32) << 2 | (m.platform as u32) << 3 }

/// modifier names in label order (Ctrl, Alt, Shift, the system key) as each system writes them
#[cfg(target_os = "macos")]
const MOD_NAMES: [&str; 4] = ["⌃", "⌥", "⇧", "⌘"];
#[cfg(target_os = "windows")]
const MOD_NAMES: [&str; 4] = ["Ctrl", "Alt", "Shift", "Win"];
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const MOD_NAMES: [&str; 4] = ["Ctrl", "Alt", "Shift", "Super"];

fn join(parts: Vec<String>) -> String { if cfg!(target_os = "macos") { parts.concat() } else { parts.join("+") } }

/// "Ctrl+Alt+Space" (on the Mac "⌃⌥Space")
pub fn label(mods: u32, vk: u32) -> String {
    if vk == 0 { return String::new(); }
    let mut parts: Vec<String> = Vec::new();
    for (bit, name) in [(2, MOD_NAMES[0]), (1, MOD_NAMES[1]), (4, MOD_NAMES[2]), (8, MOD_NAMES[3])] {
        if mods & bit != 0 { parts.push(name.into()); }
    }
    let k = NAMED.iter().find(|n| n.0 == vk).map(|n| n.2.to_string())
        .or_else(|| vk_to_key(vk).map(|k| k.to_uppercase())).unwrap_or_else(|| format!("#{vk}"));
    parts.push(k);
    join(parts)
}

/// an editor shortcut on the command key: "Ctrl+B" here, "⌘B" on the Mac; `rest` is like "Shift+S"
pub fn cmd_label(rest: &str) -> String {
    let mut parts = vec![(if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" }).to_string()];
    for p in rest.split('+') { parts.push(if p == "Shift" { MOD_NAMES[2].to_string() } else { p.to_string() }); }
    join(parts)
}
