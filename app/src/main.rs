#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod editor;
mod folder;
mod layout;
#[path = "../../locale.rs"]
mod locale;
mod pictures;
mod platform;
mod settings;
mod shell;
mod strings;
mod system;
mod theme;

use gpui::*;

#[cfg(not(target_os = "macos"))]
static FONT: &[u8] = include_bytes!("../../assets/fonts/Geologica.ttf");
#[cfg(target_os = "macos")]
static FONTS: &[&[u8]] = &[
    include_bytes!("../../assets/fonts/Geologica-Regular.ttf"),
    include_bytes!("../../assets/fonts/Geologica-Medium.ttf"),
    include_bytes!("../../assets/fonts/Geologica-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Geologica-Bold.ttf"),
];

fn main() {
    // Wayland does not let a window place itself, and the folder moves, docks and slides on its own:
    // with an X server around (XWayland too), GPUI is sent there. NOTEDD_WAYLAND=1 keeps Wayland.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()) && std::env::var_os("NOTEDD_WAYLAND").is_none() {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    // The release build has no console: panics go to a log file.
    let log = std::env::temp_dir().join("notedd.log");
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let text = format!("{info}\n{}\n\n", std::backtrace::Backtrace::force_capture());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) { let _ = f.write_all(text.as_bytes()); }
    }));
    if std::env::var_os("NOTEDD_MULTI").is_none() && !system::single_instance() { return; }
    let background = std::env::args().any(|a| a == "--background");
    let prefs = settings::load();
    let (tx, rx) = async_channel::unbounded();
    Application::new().run(move |cx: &mut App| {
        // the tray and the global shortcuts live on the main thread, in the app's event loop
        system::start([prefs.keys[0], prefs.keys[1]], tx);
        settings::set_system_dark(matches!(cx.window_appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark));
        #[cfg(not(target_os = "macos"))]
        let fonts = vec![std::borrow::Cow::Borrowed(FONT)];
        #[cfg(target_os = "macos")]
        let fonts = FONTS.iter().map(|font| std::borrow::Cow::Borrowed(*font)).collect();
        let _ = cx.text_system().add_fonts(fonts);
        let bounds = Bounds::centered(None, size(px(882.), px(586.)), cx);
        let handle = cx.open_window(
            WindowOptions {
                #[cfg(target_os = "macos")]
                kind: WindowKind::PopUp,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                window_background: WindowBackgroundAppearance::Transparent,
                window_decorations: Some(WindowDecorations::Client),
                window_min_size: Some(size(px(522.), px(330.))),
                app_id: Some("Notedd".into()),
                show: !background,
                ..Default::default()
            },
            |window, cx| {
                let shell = cx.new(|cx| shell::Shell::new(window, cx));
                // The close button hides the window; Quit (tray, shortcut) ends the app.
                window.on_window_should_close(cx, |window, _| {
                    if let Some(w) = platform::Win::of(window) { w.hide(); }
                    false
                });
                shell
            },
        )
        .expect("open window");
        let shell = handle.entity(cx).expect("shell");
        cx.spawn(async move |cx| {
            while let Ok(cmd) = rx.recv().await {
                let shell = shell.clone();
                let _ = cx.update_window(handle.into(), |_, window, cx| shell.update(cx, |s, cx| s.command(cmd, window, cx)));
            }
        }).detach();
        if let Ok(script) = std::env::var("NOTEDD_SCRIPT") {
            // Diagnostics: keystrokes ("ctrl-p", "type:text") sent to the window, 600 ms apart.
            cx.spawn(async move |cx| {
                for step in script.split(';') {
                    cx.background_executor().timer(std::time::Duration::from_millis(600)).await;
                    let step = step.to_string();
                    let _ = cx.update_window(handle.into(), move |_, window, cx| {
                        if let Some(t) = step.strip_prefix("type:") {
                            for ch in t.chars() {
                                let (key, kc) = if ch == ' ' { ("space".to_string(), " ".to_string()) } else { (ch.to_string(), ch.to_string()) };
                                window.dispatch_keystroke(Keystroke { modifiers: Modifiers::none(), key, key_char: Some(kc) }, cx);
                            }
                        } else if let Some((kind, xy)) = step.split_once('@') {
                            // mouse: "move@x,y", "click@x,y", "rclick@x,y" (window coordinates)
                            let v: Vec<f32> = xy.split(',').filter_map(|n| n.parse().ok()).collect();
                            if kind == "drag" && v.len() == 4 {
                                let (p0, p1, m) = (point(px(v[0]), px(v[1])), point(px(v[2]), px(v[3])), Modifiers::none());
                                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p0, pressed_button: None, modifiers: m }), cx);
                                window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: p0, modifiers: m, click_count: 1, first_mouse: false }), cx);
                                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: p1, pressed_button: Some(MouseButton::Left), modifiers: m }), cx);
                                window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: p1, modifiers: m, click_count: 1 }), cx);
                                return;
                            }
                            if v.len() == 2 {
                                let position = point(px(v[0]), px(v[1]));
                                let m = Modifiers::none();
                                if kind == "leave" { window.dispatch_event(PlatformInput::MouseExited(MouseExitEvent { position: point(px(-50.), px(-50.)), pressed_button: None, modifiers: Modifiers::none() }), cx); return; }
                                let mv = PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: None, modifiers: m });
                                window.dispatch_event(mv, cx);
                                let button = if kind == "rclick" { MouseButton::Right } else { MouseButton::Left };
                                if kind != "move" {
                                    window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent { button, position, modifiers: m, click_count: 1, first_mouse: false }), cx);
                                    window.dispatch_event(PlatformInput::MouseUp(MouseUpEvent { button, position, modifiers: m, click_count: 1 }), cx);
                                }
                            }
                        } else if let Ok(k) = Keystroke::parse(&step) { window.dispatch_keystroke(k, cx); }
                    });
                }
            }).detach();
        }
        cx.on_window_closed(|cx| if cx.windows().is_empty() { cx.quit(); }).detach();
        if !background { cx.activate(true); }
    });
}
