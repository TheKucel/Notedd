//! macOS installer for Notedd, built with the same GPUI and Rust toolchain as the app.
#![cfg(target_os = "macos")]
#![allow(dead_code)]
#[path = "../folder.rs"] mod folder;
#[path = "../settings.rs"] mod settings;
#[path = "../system.rs"] mod system;
#[path = "../strings.rs"] mod strings;
#[path = "../../../locale.rs"] mod locale;
#[path = "../../../setup/src/strings.rs"] mod ui;
mod layout { pub const BODY: &str = "Geologica"; }

use gpui::*;
use folder::{paint_folder, Place};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};
use cocoa::{base::{id, NO}, foundation::NSString};
use objc::{class, msg_send, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const W: f32 = 768.;
const H: f32 = 676.;
const CARD_W: f32 = 640.;
const CARD_H: f32 = 504.;
const OX: f32 = 64.;
const OY: f32 = 108.;
const PAD: f32 = 56.;
const TOP: f32 = 88.;
const BUTTON_Y: f32 = OY + CARD_H - 80.;
const INTRO_DURATION: f32 = 0.9;
const PAGE_DURATION: f32 = 0.48;
const DONE_DURATION: f32 = 1.04;
const DONE_ACTIONS_DELAY: f32 = 0.50;
const DARK: u32 = 0x22211f;
const PAPER: u32 = 0xfaf7f1;
const SYSTEM_THEME: locale::Message = locale::Message("Follow macOS", "Как в macOS", "跟随 macOS");
const AUTOSTART: locale::Message = locale::Message("Open at login", "Запускать при входе", "登录时启动");
const AUTOSTART_HELP: locale::Message = locale::Message("Notedd will be ready in the menu bar", "Notedd будет ждать в строке меню", "Notedd 将在菜单栏中待命");
const READY_HELP: locale::Message = locale::Message("Use this shortcut to open Notedd from anywhere.", "Это сочетание открывает Notedd из любого места.", "随时按此快捷键即可打开 Notedd。");
const FONTS: [&[u8]; 4] = [include_bytes!("../../../assets/fonts/Geologica-Regular.ttf"), include_bytes!("../../../assets/fonts/Geologica-Medium.ttf"), include_bytes!("../../../assets/fonts/Geologica-SemiBold.ttf"), include_bytes!("../../../assets/fonts/Geologica-Bold.ttf")];

fn tr(ru: &'static str, en: &'static str) -> &'static str { if locale::current() == locale::Language::Russian { ru } else { en } }
fn ink() -> Hsla { rgb(0x1c1b19).into() }
fn muted() -> Hsla { rgb(0x6f6b63).into() }
fn file_error(op: &str, e: impl std::fmt::Display) -> String { format!("{op}: {e}") }

fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("NOTEDD_SETUP_TEST_DIR") { return PathBuf::from(p).join("settings.txt"); }
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("Notedd/settings.txt")
}
fn login_path() -> PathBuf {
    if let Some(p) = std::env::var_os("NOTEDD_SETUP_TEST_DIR") { return PathBuf::from(p).join("login.plist"); }
    dirs::home_dir().unwrap_or_default().join("Library/LaunchAgents/app.notedd.plist")
}
fn payload_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| file_error("Installer", e))?;
    Ok(exe.parent().and_then(Path::parent).ok_or("Invalid installer bundle")?.join("Resources/Notedd-payload.zip"))
}
fn bundle_id(path: &Path) -> Option<String> {
    let output = std::process::Command::new("/usr/bin/plutil").args(["-extract", "CFBundleIdentifier", "raw"]).arg(path.join("Contents/Info.plist")).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
fn running() -> bool { unsafe {
    let s = cocoa::foundation::NSString::alloc(cocoa::base::nil).init_str("com.thekucel.notedd");
    let apps: id = msg_send![class!(NSRunningApplication), runningApplicationsWithBundleIdentifier: s];
    let n: usize = msg_send![apps, count];
    let _: () = msg_send![s, release];
    n != 0
} }
fn install_payload(payload: &Path, target_dir: &Path, notes: &Path, test: bool) -> Result<PathBuf, String> {
    if !test && running() { return Err(tr("Сначала закройте Notedd и попробуйте снова.", "Quit Notedd and try again.").into()); }
    std::fs::create_dir_all(target_dir).map_err(|e| file_error("Applications", e))?;
    let destination = target_dir.join("Notedd.app");
    if std::fs::symlink_metadata(&destination).is_ok() && (destination.is_symlink() || bundle_id(&destination).as_deref() != Some("com.thekucel.notedd")) {
        return Err(tr("В выбранной папке уже есть другой файл Notedd.app.", "Another file named Notedd.app already exists there.").into());
    }
    let stage = target_dir.join(format!(".notedd-setup-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    std::fs::create_dir(&stage).map_err(|e| file_error("Staging", e))?;
    let result = (|| {
        let out = std::process::Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(payload).arg(&stage).output().map_err(|e| file_error("Unpack", e))?;
        if !out.status.success() { return Err(file_error("Unpack", String::from_utf8_lossy(&out.stderr))); }
        let candidate = stage.join("Notedd.app");
        if bundle_id(&candidate).as_deref() != Some("com.thekucel.notedd") || !candidate.join("Contents/MacOS/Notedd").is_file() { return Err("Invalid Notedd bundle".into()); }
        std::fs::create_dir_all(notes).map_err(|e| file_error("Notes folder", e))?;
        if destination.exists() {
            // Exchange atomically; the old bundle remains in staging until cleanup.
            use std::os::unix::ffi::OsStrExt;
            let a = std::ffi::CString::new(candidate.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
            let b = std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
            extern "C" { fn renameatx_np(fd1: i32, path1: *const i8, fd2: i32, path2: *const i8, flags: u32) -> i32; }
            if unsafe { renameatx_np(-2, a.as_ptr(), -2, b.as_ptr(), 0x00000002) } != 0 { return Err(file_error("Update", std::io::Error::last_os_error())); }
        } else { std::fs::rename(&candidate, &destination).map_err(|e| file_error("Install", e))?; }
        Ok(destination.clone())
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result
}
fn write_options(p: &settings::Prefs, notes: &Path, destination: &Path, login: bool, change_login: bool) -> Result<(), String> {
    let path = config_path();
    let notes = notes.to_str().ok_or("Invalid notes path")?;
    if notes.contains(['\n', '\r']) { return Err("Invalid notes path".into()); }
    let language = p.language.map_or("system", locale::Language::tag);
    let changes = [("theme", p.theme.to_string()), ("editor", if p.simple_editor {"simple"} else {"markdown"}.into()), ("language", language.into()), ("key0", format!("{},{}", p.keys[0].0, p.keys[0].1)), ("notes_dir", notes.into())];
    let old = match std::fs::read_to_string(&path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(file_error("Settings", e)),
    };
    let mut lines: Vec<String> = old.lines().filter(|line| !line.split_once('=').is_some_and(|(key,_)| changes.iter().any(|(k,_)| *k == key.trim()))).map(str::to_string).collect();
    lines.extend(changes.iter().map(|(k,v)| format!("{k}={v}")));
    std::fs::create_dir_all(path.parent().ok_or("Invalid settings path")?).map_err(|e| e.to_string())?;
    notedd_core::store::replace_file(&path, (lines.join("\n") + "\n").as_bytes()).map_err(|e| file_error("Settings", e))?;
    let agent = login_path();
    if change_login || login {
        if login {
            std::fs::create_dir_all(agent.parent().ok_or("Invalid login path")?).map_err(|e| e.to_string())?;
            let executable = destination.join("Contents/MacOS/Notedd");
            let esc = executable.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            let plist = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>app.notedd</string><key>ProgramArguments</key><array><string>{esc}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>");
            notedd_core::store::replace_file(&agent, plist.as_bytes()).map_err(|e| file_error("Login", e))?;
        } else if agent.exists() { std::fs::remove_file(&agent).map_err(|e| file_error("Login", e))?; }
    }
    Ok(())
}

fn desktop_shortcut(destination: &Path) -> Result<(), String> {
    let desktop = dirs::desktop_dir().ok_or("Desktop folder unavailable")?;
    let shortcut = desktop.join("Notedd");
    if std::fs::read_link(&shortcut).as_deref().ok() == Some(destination) {
        return Ok(());
    }
    std::os::unix::fs::symlink(destination, shortcut).map_err(|e| file_error(ui::DESKTOP_SHORTCUT.get(), e))
}

struct Installer {
    page: usize,
    previous: usize,
    prefs: settings::Prefs,
    notes: PathBuf,
    app_dir: PathBuf,
    login: bool,
    change_login: bool,
    desktop: bool,
    language_menu: bool,
    capture: bool,
    working: bool,
    error: Option<String>,
    done: Option<PathBuf>,
    started: Instant,
    focus: FocusHandle,
}
impl Installer {
    fn new(cx: &mut Context<Self>) -> Self {
        let prefs = settings::load(); locale::set(prefs.language);
        let home = dirs::home_dir().unwrap_or_default();
        let notes = settings::notes_dir_pref().unwrap_or_else(|| dirs::document_dir().unwrap_or_else(|| home.clone()).join("Notedd"));
        Self { page: 0, previous: 0, prefs, notes, app_dir: home.join("Applications"), login: login_path().exists(), change_login: false, desktop: false, language_menu: false, capture: false, working: false, error: None, done: None, started: Instant::now(), focus: cx.focus_handle() }
    }
    fn go(&mut self, page: usize, cx: &mut Context<Self>) {
        self.previous = self.page;
        self.page = page.min(4);
        self.capture = false;
        self.language_menu = false;
        self.started = Instant::now();
        cx.notify();
    }
    fn pick(&mut self, notes: bool, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(tr("Выберите папку", "Choose a folder").into()), extensions: None });
        cx.spawn_in(window, async move |this, cx| { if let Ok(Ok(Some(mut paths))) = rx.await { if let Some(path) = paths.pop() { let _ = this.update(cx, |s, cx| { if notes { s.notes = path; } else { s.app_dir = path; } cx.notify(); }); } } }).detach();
    }
    fn install(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.working { return; }
        self.error = None;
        if running() { self.error = Some(tr("Сначала закройте Notedd и попробуйте снова.", "Quit Notedd and try again.").into()); cx.notify(); return; }
        let (app_dir, notes, prefs, login, change_login, desktop) = (self.app_dir.clone(), self.notes.clone(), self.prefs, self.login, self.change_login, self.desktop);
        self.working = true;
        self.go(4, cx);
        let started = self.started;
        let task = cx.background_spawn(async move {
            let payload = payload_path()?;
            let destination = install_payload(&payload, &app_dir, &notes, true)?;
            write_options(&prefs, &notes, &destination, login, change_login)?;
            let warning = if desktop { desktop_shortcut(&destination).err() } else { None };
            Ok::<_, String>((destination, warning))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            // A fast install can finish while the last card is still sliding in.
            if result.is_ok() {
                let remaining = Duration::from_secs_f32(PAGE_DURATION).saturating_sub(started.elapsed());
                if !remaining.is_zero() { cx.background_executor().timer(remaining).await; }
            }
            let _ = this.update_in(cx, |s, _, cx| {
                s.working = false;
                match result {
                    Ok((destination, warning)) => {
                        s.done = Some(destination);
                        s.error = warning;
                        s.started = Instant::now();
                    }
                    Err(e) => s.error = Some(e),
                }
                cx.notify();
            });
        }).detach();
    }
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.page {
            0..=2 => self.go(self.page + 1, cx),
            3 => self.install(window, cx),
            _ => {
                if let Some(path) = &self.done {
                    if self.started.elapsed().as_secs_f32() < DONE_ACTIONS_DELAY { return; }
                    match std::process::Command::new("/usr/bin/open").arg(path).spawn() {
                        Ok(_) => cx.quit(),
                        Err(error) => { self.error = Some(file_error(ui::OPEN_APP.get(), error)); cx.notify(); }
                    }
                } else if self.error.is_some() {
                    self.go(3, cx);
                }
            }
        }
    }
}

fn positioned(x: f32, y: f32, width: f32, height: f32) -> Div {
    div().absolute().left(px(OX + x)).top(px(OY + y)).w(px(width)).h(px(height))
}

fn label(text: impl Into<SharedString>, x: f32, y: f32, width: f32, font_size: f32, weight: FontWeight, color: Hsla) -> Div {
    positioned(x, y, width, 52.).text_size(px(font_size)).font_weight(weight).text_color(color).child(text.into())
}

#[derive(Clone, Copy)]
enum Button { Primary, Quiet, Outline }

fn button(
    id: &'static str, title: &'static str, x: f32, y: f32, width: f32, style: Button,
    action: impl Fn(&mut Installer, &mut Window, &mut Context<Installer>) + 'static,
    cx: &mut Context<Installer>,
) -> Stateful<Div> {
    let base = positioned(x, y, width, 44.).id(id).rounded(px(22.)).flex().items_center().justify_center()
        .cursor_pointer().text_size(px(15.)).font_weight(FontWeight::MEDIUM);
    let base = match style {
        Button::Primary => base.bg(rgb(DARK)).text_color(rgb(PAPER)).hover(|s| s.bg(rgb(0x3a3935))),
        Button::Quiet => base.text_color(muted()).hover(|s| s.bg(rgba(0x1c1b190f)).text_color(ink())),
        Button::Outline => base.bg(white()).border_1().border_color(rgba(0x3d3c3838)).hover(|s| s.bg(rgb(PAPER))),
    };
    base.child(title).on_click(cx.listener(move |s, _, w, cx| action(s, w, cx)))
}

fn centered(text: &'static str, y: f32, height: f32, size: f32, weight: FontWeight, color: Hsla) -> Div {
    positioned(PAD, y, CARD_W - 2. * PAD, height).text_center().text_size(px(size))
        .font_weight(weight).text_color(color).child(text)
}

fn switch(on: bool) -> Div {
    let track = div().w(px(48.)).h(px(28.)).rounded(px(14.)).bg(rgb(if on { DARK } else { 0xe4e3e1 }))
        .flex().items_center().px(px(3.));
    let track = if on { track.justify_end() } else { track.justify_start() };
    track.child(div().size(px(22.)).rounded(px(11.)).bg(white()))
}

fn keycaps(keys: (u32, u32), window: &mut Window) -> (Div, f32) {
    let mut row = div().flex().gap(px(8.));
    let mut width = 0.;
    let (mods, key) = keys;
    let mut names: Vec<String> = [(2, "⌃"), (1, "⌥"), (4, "⇧"), (8, "⌘")].into_iter()
        .filter(|(bit, _)| mods & bit != 0).map(|(_, name)| name.into()).collect();
    names.push(system::label(0, key));
    for name in names {
        let cap_width = folder::text_width(window, &name, 16., FontWeight::MEDIUM).max(16.) + 30.;
        width += cap_width + 8.;
        row = row.child(div().w(px(cap_width)).h(px(44.)).rounded(px(11.)).border_1()
            .border_color(rgba(0x3d3c3838)).bg(white()).flex().items_center().justify_center()
            .text_size(px(16.)).font_weight(FontWeight::MEDIUM).child(name));
    }
    (row, width - 8.)
}

fn theme_preview(index: usize) -> Div {
    fn half(dark: bool) -> Div {
        div().size_full().flex().items_center().justify_center().text_size(px(34.))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(if dark { 0xece8dc } else { 0x1c1b19 })).child("Aa")
    }
    let base = div().w_full().h(px(100.)).relative().rounded(px(16.))
        .bg(rgb(if index == 1 { 0x1e1e1c } else { 0xf4f1ea }));
    if index == 2 {
        base.child(half(false)).child(div().absolute().top_0().right_0().w(relative(0.5)).h_full()
            .overflow_hidden().child(canvas(|_, _, _| (), |bounds, _, window, _| {
                window.paint_quad(fill(bounds, rgb(0x1e1e1c)).corner_radii(Corners {
                    top_right: px(16.), bottom_right: px(16.), ..Default::default()
                }));
            }).absolute().size_full())
            .child(div().absolute().right_0().top_0().w(relative(2.)).h_full().child(half(true))))
    } else { base.child(half(index == 1)) }
}

#[derive(Clone, Copy)]
struct Pose { x: f32, y: f32, scale: f32, angle: f32, alpha: f32 }

impl Pose {
    fn to(self, other: Self, amount: f32) -> Self {
        let mix = |a, b| a + (b - a) * amount;
        Self {
            x: mix(self.x, other.x), y: mix(self.y, other.y), scale: mix(self.scale, other.scale),
            angle: mix(self.angle, other.angle), alpha: mix(self.alpha, other.alpha),
        }
    }

    fn place(self) -> Place {
        let pivot_y = CARD_H * 0.9;
        Place {
            x: OX + self.x + pivot_y * self.angle.sin() * self.scale,
            y: OY + self.y + pivot_y * (1. - self.angle.cos() * self.scale),
            sc: self.scale, w: CARD_W, rot: self.angle, sx: 0.,
        }
    }
}

fn stack(index: usize, depth: usize) -> Pose {
    if depth == 0 { return Pose { x: 0., y: 0., scale: 1., angle: 0., alpha: 1. }; }
    let angle = [-1.6_f32, 1.4, -1.1, 1.8, -1.3][index].to_radians();
    let x = [-3., 3., -2., 3., -2.][index] - CARD_H * 0.9 * angle.sin();
    Pose { x, y: -13. * depth as f32, scale: 1. - 0.012 * depth as f32, angle, alpha: 1. }
}

fn thrown(index: usize) -> Pose {
    let direction = if index % 2 == 0 { -1. } else { 1. };
    Pose { x: direction * CARD_W * 0.62, y: 46., scale: 0.97, angle: direction * 9_f32.to_radians(), alpha: 0. }
}

fn ease(t: f32) -> f32 { 1. - (1. - t.clamp(0., 1.)).powi(3) }

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn card_pose(index: usize, page: usize, previous: usize, elapsed: f32, done: bool) -> Pose {
    if done {
        let rest = stack(index, page - index);
        if index == page { return rest; }
        let fan = Pose {
            x: [-25., -12., 12., 25.][index],
            y: rest.y - 17.,
            angle: [-5_f32, -2.5, 2.5, 5.][index].to_radians(),
            ..rest
        };
        let half = DONE_DURATION / 2.;
        return if elapsed < half {
            Pose { alpha: 0., ..rest }.to(fan, smooth(elapsed / half))
        } else {
            fan.to(rest, smooth((elapsed - half) / half))
        };
    }
    let target = if index < page { thrown(index) } else { stack(index, index - page) };
    let start = if page == 0 && previous == 0 {
        Pose { y: target.y + 70., alpha: 0., ..target }
    } else if index < previous { thrown(index) } else { stack(index, index - previous) };
    let amount = if page == 0 && previous == 0 {
        ease((elapsed - 0.08 - 0.065 * (4 - index) as f32) / 0.5)
    } else { ease(elapsed / PAGE_DURATION) };
    start.to(target, amount)
}

fn draw_cards(window: &mut Window, cx: &mut App, titles: [&str; 5], page: usize, previous: usize, elapsed: f32, done: bool) {
    let mut tab_left = 38.;
    let tabs = titles.map(|title| {
        let width = folder::text_width(window, title, 15., FontWeight::MEDIUM) + 38.;
        let tab = (tab_left, tab_left + width);
        tab_left += width + 4.;
        tab
    });
    let order = if done { [0, 1, 2, 3, 4] } else { [4, 3, 2, 1, 0] };
    for index in order {
        let pose = card_pose(index, page, previous, elapsed, done);
        if pose.alpha < 0.004 { continue; }
        let place = pose.place();
        let (tab, right) = tabs[index];
        if index == page {
            let shadow_bounds = Bounds::new(place.pt(0., folder::T), size(px(CARD_W * pose.scale), px((CARD_H - folder::T) * pose.scale)));
            let shadow = BoxShadow {
                color: Hsla::from(rgb(0x2a2620)).opacity(pose.alpha * 0.10),
                offset: point(px(0.), px(10.)), blur_radius: px(22.), spread_radius: px(0.),
            };
            window.paint_shadows(shadow_bounds, Corners::all(px(20.)), &[shadow]);
        }
        paint_folder(window, &place, CARD_H, white(), rgba(0x3d3c38e6).into(), (tab, right), pose.alpha, (20., 20., 20., 20.));
        let mut color = ink();
        color.a = pose.alpha;
        folder::text(window, cx, titles[index], place.pt(tab + 12., 6.), 15. * pose.scale, FontWeight::MEDIUM, color, right - tab - 12.);
    }
}

impl Render for Installer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if window.focused(cx).is_none() { window.focus(&self.focus); }
        let page = self.page;
        let previous = self.previous;
        let elapsed = self.started.elapsed().as_secs_f32();
        let done = self.done.is_some();
        let intro = page == 0 && previous == 0;
        let duration = if done { DONE_DURATION } else if intro { INTRO_DURATION } else { PAGE_DURATION };
        if self.working || elapsed < duration { window.request_animation_frame(); }
        let titles = ["Notedd", ui::THEME.get(), ui::SHORTCUT.get(), ui::FOLDERS.get(), if done { ui::DONE.get() } else { ui::INSTALLATION.get() }];
        let mut root = div().id("setup").size_full().relative().font_family("Geologica").track_focus(&self.focus).text_color(ink())
            .capture_key_down(cx.listener(|s, ev: &KeyDownEvent, w, cx| {
                if s.working { cx.stop_propagation(); return; }
                let key = &ev.keystroke.key;
                if s.capture {
                    cx.stop_propagation();
                    if key == "escape" {
                        s.capture = false;
                        cx.notify();
                    } else if let Some((mods, code)) = system::shortcut_key(key, system::mods_of(&ev.keystroke.modifiers)) {
                        if mods != 0 {
                            s.prefs.keys[0] = (mods, code);
                            s.capture = false;
                            cx.notify();
                        }
                    }
                } else if key == "escape" && s.language_menu {
                    s.language_menu = false;
                    cx.notify();
                } else if key == "escape" {
                    if (1..=3).contains(&s.page) { s.go(s.page - 1, cx); } else { cx.quit(); }
                } else if key == "enter" {
                    s.advance(w, cx);
                }
            }))
            .child(canvas(|_, _, _| (), move |_, _, window, cx| draw_cards(window, cx, titles, page, previous, elapsed, done)).absolute().size_full());

        let reveal = if done { 1. } else if intro {
            ease((elapsed - 0.42) / 0.38)
        } else { ease((elapsed - 0.04) / 0.28) };
        let mut content = div().absolute().size_full().opacity(reveal);
        let heading = |title, hint| {
            div().absolute().size_full()
                .child(label(title, PAD, TOP, 528., 30., FontWeight::BOLD, ink()))
                .child(label(hint, PAD, TOP + 48., 528., 15., FontWeight::NORMAL, muted()))
        };

        match page {
            0 => {
                let lift = (1. - reveal) * 18.;
                content = content.child(centered("Notedd", 146. + lift, 84., 68., FontWeight::BOLD, ink()))
                    .child(centered(ui::WELCOME.get(), 240. + lift, 60., 16., FontWeight::NORMAL, muted()));
                content = content.child(button("language", "", PAD, 424., 44., Button::Outline, |s, _, cx| {
                    s.language_menu = !s.language_menu; cx.notify();
                }, cx).child(canvas(|_,_,_| (), |bounds, _, window, _| {
                    let center = bounds.center();
                    let mut path = PathBuilder::stroke(px(1.2));
                    for (x, y) in [(7., 7.), (3., 7.)] {
                        path.move_to(point(center.x - px(x), center.y));
                        path.arc_to(point(px(x), px(y)), px(0.), false, true, point(center.x + px(x), center.y));
                        path.arc_to(point(px(x), px(y)), px(0.), false, true, point(center.x - px(x), center.y));
                    }
                    path.move_to(point(center.x - px(7.), center.y));
                    path.line_to(point(center.x + px(7.), center.y));
                    if let Ok(path) = path.build() { window.paint_path(path, ink()); }
                }).absolute().size_full()));
                if self.language_menu {
                    let mut menu = positioned(PAD, 292., 166., 122.).id("languages").bg(white()).rounded(px(12.))
                        .border_1().border_color(rgba(0x3d3c3838)).p(px(6.)).flex().flex_col();
                    for language in locale::Language::ALL {
                        let selected = language == locale::current();
                        let mut row = div().id(language.tag()).h(px(36.)).rounded(px(8.)).px(px(10.)).flex()
                            .items_center().justify_between().cursor_pointer().hover(|s| s.bg(rgb(0xf4f1ea)))
                            .child(language.name());
                        if selected {
                            row = row.child(canvas(|_, _, _| (), |bounds, _, window, _| {
                                let center = bounds.center();
                                let mut check = PathBuilder::stroke(px(1.5));
                                check.move_to(point(center.x - px(5.), center.y));
                                check.line_to(point(center.x - px(1.5), center.y + px(3.5)));
                                check.line_to(point(center.x + px(5.), center.y - px(4.)));
                                if let Ok(path) = check.build() { window.paint_path(path, ink()); }
                            }).size(px(16.)));
                        }
                        menu = menu.child(row.on_click(cx.listener(move |s, _, _, cx| {
                            s.prefs.language = Some(language);
                            locale::set(Some(language));
                            s.language_menu = false;
                            cx.notify();
                        })));
                    }
                    content = content.child(menu);
                }
            }
            1 => {
                content = content.child(heading(ui::THEME.get(), ui::THEME_HELP.get()));
                let tile_width = (CARD_W - 2. * PAD - 52.) / 3.;
                for (index, title) in [ui::LIGHT.get(), ui::DARK.get(), SYSTEM_THEME.get()].into_iter().enumerate() {
                    let selected = self.prefs.theme == index as u8;
                    let x = PAD + index as f32 * (tile_width + 26.);
                    content = content.child(positioned(x - 5., 195., tile_width + 10., 140.).id(("theme", index))
                        .cursor_pointer().flex().flex_col().items_center().gap(px(10.))
                        .child(div().w_full().p(px(3.)).rounded(px(19.)).border_2()
                            .border_color(if selected { ink() } else { transparent_black() }).child(theme_preview(index)))
                        .child(div().text_size(px(15.)).text_color(if selected { ink() } else { muted() }).child(title))
                        .on_click(cx.listener(move |s, _, _, cx| { s.prefs.theme = index as u8; cx.notify(); })));
                }
                content = content.child(label(ui::EDITOR.get(), PAD, 355., 230., 16., FontWeight::MEDIUM, ink()))
                    .child(label(if self.prefs.simple_editor { ui::SIMPLE_HELP.get() } else { ui::MARKDOWN_HELP.get() },
                        PAD, 380., 235., 13., FontWeight::NORMAL, muted()));
                let mut choices = positioned(334., 360., 250., 44.).rounded(px(22.)).bg(rgba(0x1c1b190d))
                    .border_1().border_color(rgba(0x3d3c3859)).p(px(2.)).flex();
                for (simple, name) in [(false, "Markdown"), (true, ui::SIMPLE.get())] {
                    let selected = self.prefs.simple_editor == simple;
                    choices = choices.child(div().id(("editor", usize::from(simple))).flex_1().h_full().rounded(px(19.))
                        .flex().items_center().justify_center().cursor_pointer().text_size(px(15.))
                        .bg(if selected { rgb(DARK).into() } else { transparent_black() })
                        .text_color(if selected { rgb(PAPER).into() } else { muted() }).child(name)
                        .on_click(cx.listener(move |s, _, _, cx| { s.prefs.simple_editor = simple; cx.notify(); })));
                }
                content = content.child(choices);
            }
            2 => {
                content = content.child(heading(ui::SHORTCUT.get(), ui::SHORTCUT_HELP.get()));
                if self.capture {
                    content = content.child(positioned(PAD, 198., 230., 44.).rounded(px(11.)).border_1().border_dashed()
                        .border_color(ink()).flex().items_center().px(px(16.)).child(ui::PRESS_SHORTCUT.get()))
                        .child(label(ui::ESC_CANCEL.get(), 306., 210., 240., 13., FontWeight::NORMAL, muted()));
                } else {
                    let (keys, width) = keycaps(self.prefs.keys[0], window);
                    content = content.child(positioned(PAD, 198., width, 44.).child(keys))
                        .child(button("shortcut", ui::CHANGE.get(), PAD + width + 16., 198., 125., Button::Outline,
                            |s, w, cx| { s.capture = true; w.focus(&s.focus); cx.notify(); }, cx));
                }
                content = content.child(positioned(PAD, 284., 528., 50.).id("login").cursor_pointer()
                    .flex().items_center().justify_between().child(div().flex().flex_col()
                        .child(AUTOSTART.get()).child(div().text_size(px(13.)).text_color(muted()).child(AUTOSTART_HELP.get())))
                    .child(switch(self.login))
                    .on_click(cx.listener(|s, _, _, cx| { s.login = !s.login; s.change_login = true; cx.notify(); })));
            }
            3 => {
                content = content.child(heading(ui::FOLDERS.get(), ui::FOLDERS_HELP.get()));
                for (i, (id, title, path)) in [("notes", ui::NOTES.get(), &self.notes), ("appdir", ui::APP.get(), &self.app_dir)].into_iter().enumerate() {
                    let y = 198. + 80. * i as f32;
                    content = content.child(label(title, PAD, y - 2., 370., 13., FontWeight::NORMAL, muted()))
                        .child(positioned(PAD, y + 20., 380., 28.).truncate().text_size(px(15.)).child(path.to_string_lossy().into_owned()))
                        .child(button(id, ui::CHANGE.get(), 454., y + 4., 130., Button::Outline, move |s, w, cx| s.pick(i == 0, w, cx), cx));
                }
                content = content.child(positioned(PAD, 360., 528., 44.).id("desktop").cursor_pointer().flex()
                    .items_center().justify_between().child(ui::DESKTOP_SHORTCUT.get()).child(switch(self.desktop))
                    .on_click(cx.listener(|s, _, _, cx| { s.desktop = !s.desktop; cx.notify(); })));
                if let Some(error) = &self.error {
                    content = content.child(label(error.clone(), PAD, 400., 390., 12., FontWeight::NORMAL, rgb(0xa03d35).into()));
                }
            }
            _ if done => {
                let (keys, width) = keycaps(self.prefs.keys[0], window);
                let copy = smooth((elapsed - 0.12) / 0.32);
                let lift = 8. * (1. - copy);
                content = content.child(centered(ui::DONE.get(), 120. + lift, 84., 68., FontWeight::BOLD, ink()).opacity(copy))
                    .child(centered(READY_HELP.get(), 212. + lift, 46., 16., FontWeight::NORMAL, muted()).opacity(copy))
                    .child(positioned((CARD_W - width) / 2., 280., width, 44.).opacity(smooth((elapsed - 0.26) / 0.32)).child(keys));
                if let Some(error) = &self.error {
                    content = content.child(label(error.clone(), PAD, 358., 528., 13., FontWeight::NORMAL, rgb(0xa03d35).into()).opacity(copy));
                }
            }
            _ if self.working => {
                let sweep = 0.5 - 0.5 * (elapsed * 5.).cos();
                content = content.child(heading(ui::INSTALLING.get(), ui::WAIT.get()))
                    .child(positioned(PAD, TOP + 120., 528., 8.).rounded(px(4.)).bg(rgba(0x1c1b1914))
                        .child(div().absolute().left(px(sweep * 400.)).w(px(128.)).h_full().rounded(px(4.)).bg(rgb(DARK))));
            }
            _ => {
                content = content.child(label(ui::INSTALL_FAILED.get(), PAD, TOP, 528., 30., FontWeight::BOLD, ink()))
                    .child(label(self.error.clone().unwrap_or_default(), PAD, TOP + 48., 528., 15., FontWeight::NORMAL, muted()));
            }
        }
        if !self.working && (!done || elapsed >= DONE_ACTIONS_DELAY) {
            let actions = if done { smooth((elapsed - DONE_ACTIONS_DELAY) / 0.32) } else { 1. };
            let secondary = if done { ui::CLOSE.get() } else { ui::BACK.get() };
            if page > 0 {
                content = content.child(button("back", secondary, 36., BUTTON_Y - OY, 108., Button::Quiet, |s, _, cx| {
                    if s.done.is_some() { cx.quit(); } else { s.go(s.page - 1, cx); }
                }, cx).opacity(actions));
            }
            let next = match page {
                0 => ui::GET_STARTED.get(), 1 | 2 => ui::NEXT.get(), 3 => ui::INSTALL.get(),
                _ if done => ui::OPEN_APP.get(), _ => ui::BACK.get(),
            };
            let width = folder::text_width(window, next, 15., FontWeight::MEDIUM).ceil() + 56.;
            content = content.child(button("next", next, CARD_W - PAD + 8. - width, BUTTON_Y - OY, width, Button::Primary,
                |s, w, cx| s.advance(w, cx), cx).opacity(actions));
            content = content.child(positioned(CARD_W - 40., 4., 26., 26.).id("close").rounded(px(13.))
                .border_1().border_color(rgba(0x3d3c38e6)).bg(white()).hover(|s| s.bg(rgb(0xf4f1ea)))
                .flex().items_center().justify_center().cursor_pointer().text_color(muted()).child("×")
                .opacity(actions).on_click(|_, _, cx| cx.quit()));
        }
        root = root.child(positioned(0., 0., CARD_W, folder::T).on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()));
        root.child(content)
    }
}
fn self_test() -> Result<(), String> {
    let base = PathBuf::from(std::env::var_os("NOTEDD_SETUP_TEST_DIR").ok_or("Missing test directory")?);
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), "scale=120\nunknown=value\n").map_err(|e| e.to_string())?;
    let notes=base.join("Notes"); let app=base.join("Applications");
    let occupied=base.join("Occupied/Notedd.app");
    std::fs::create_dir_all(&occupied).map_err(|e| e.to_string())?;
    std::fs::write(occupied.join("keep"), "unrelated").map_err(|e| e.to_string())?;
    if install_payload(&payload_path()?, &base.join("Occupied"), &notes, true).is_ok() || !occupied.join("keep").is_file() {
        return Err("An unrelated application was not protected".into());
    }
    let destination=install_payload(&payload_path()?,&app,&notes,true)?;
    install_payload(&payload_path()?,&app,&notes,true)?;
    let mut prefs=settings::Prefs::default(); prefs.theme=2; prefs.simple_editor=true; prefs.keys[0]=(3,32); prefs.language=Some(locale::Language::Russian);
    write_options(&prefs,&notes,&destination,true,true)?;
    let data=std::fs::read_to_string(config_path()).map_err(|e| e.to_string())?;
    for value in ["scale=120", "unknown=value", "theme=2", "editor=simple", "language=ru", "key0=3,32"] { if !data.lines().any(|line| line==value) { return Err(format!("Missing {value}")); } }
    if !login_path().is_file() { return Err("Login plist missing".into()); }
    write_options(&prefs,&notes,&destination,false,true)?;
    if login_path().exists() { return Err("Login plist was not removed".into()); }
    println!("Installer self-test passed"); Ok(())
}
fn main() {
    if std::env::args().any(|a| a == "--self-test") { if let Err(e)=self_test() { eprintln!("Installer self-test failed: {e}"); std::process::exit(1); } return; }
    Application::new().run(|cx: &mut App| {
        let _ = cx.text_system().add_fonts(FONTS.iter().map(|f|std::borrow::Cow::Borrowed(*f)).collect());
        let bounds = Bounds::centered(None,size(px(W),px(H)),cx);
        cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), titlebar: None, window_background: WindowBackgroundAppearance::Transparent, window_decorations: Some(WindowDecorations::Client), is_resizable: false, ..Default::default() }, |window,cx| {
            // GPUI's hidden title bar still uses a titled NSWindow. Remove the native frame.
            if let Ok(handle) = HasWindowHandle::window_handle(window) {
                if let RawWindowHandle::AppKit(handle) = handle.as_raw() {
                    unsafe {
                        let native: id = msg_send![handle.ns_view.as_ptr() as id, window];
                        let _: () = msg_send![native, setStyleMask: 0usize];
                        let _: () = msg_send![native, setHasShadow: NO];
                    }
                }
            }
            cx.new(Installer::new)
        }).expect("open setup");
        cx.on_window_closed(|cx| if cx.windows().is_empty() { cx.quit(); }).detach();
    });
}
