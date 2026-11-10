//! X11 (also XWayland): the app keeps its own connection to the X server and works on GPUI's window by
//! its id. X counts in pixels from the top-left, like the rest of the app. The window is shaped with the
//! SHAPE extension and kept above / off the taskbar through `_NET_WM_STATE`. Without an X server (pure
//! Wayland) the window falls back to `other.rs`: it stays where the compositor put it.

use super::{other, Pt, Rect};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

pub use super::other::local_time;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        _NET_WORKAREA,
        _NET_ACTIVE_WINDOW,
        _NET_WM_STATE,
        _NET_WM_STATE_ABOVE,
        _NET_WM_STATE_SKIP_TASKBAR,
        _NET_WM_STATE_SKIP_PAGER,
    }
}

struct X { c: RustConnection, root: Window, a: Atoms }

fn x() -> Option<&'static X> {
    static X: OnceLock<Option<X>> = OnceLock::new();
    X.get_or_init(|| {
        let (c, n) = x11rb::connect(None).ok()?;
        let root = c.setup().roots.get(n)?.root;
        let a = Atoms::new(&c).ok()?.reply().ok()?;
        Some(X { c, root, a })
    })
    .as_ref()
}

/// the window is meant to stay above the others (kept here: the window manager forgets it when the window hides)
static TOPMOST: AtomicBool = AtomicBool::new(false);

/// the main window
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Win {
    X(Window),
    Other(other::Win),
}

impl Default for Win {
    fn default() -> Self { Win::Other(other::Win) }
}

impl Win {
    pub fn of(window: &gpui::Window) -> Option<Win> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let id = match HasWindowHandle::window_handle(window).ok().map(|h| h.as_raw()) {
            Some(RawWindowHandle::Xcb(h)) => Some(h.window.get()),
            Some(RawWindowHandle::Xlib(h)) => Some(h.window as u32),
            _ => None,
        };
        match (id, x()) {
            (Some(id), Some(_)) => Some(Win::X(id)),
            _ => other::Win::of(window).map(Win::Other),
        }
    }

    /// the client area on the screen
    pub fn client_rect(self) -> Rect {
        let Win::X(w) = self else { return other::Win.client_rect() };
        let Some(x) = x() else { return Rect::default() };
        let get = || -> Option<Rect> {
            let g = x.c.get_geometry(w).ok()?.reply().ok()?;
            let t = x.c.translate_coordinates(w, x.root, 0, 0).ok()?.reply().ok()?;
            let (l, t) = (t.dst_x as i32, t.dst_y as i32);
            Some(Rect { left: l, top: t, right: l + g.width as i32, bottom: t + g.height as i32 })
        };
        get().unwrap_or_default()
    }

    /// moves and sizes the window so its client area is `r`
    pub fn set_client_rect(self, r: Rect) {
        let Win::X(w) = self else { return other::Win.set_client_rect(r) };
        let Some(x) = x() else { return };
        let aux = ConfigureWindowAux::new().x(r.left).y(r.top).width(r.w().max(1) as u32).height(r.h().max(1) as u32);
        let _ = x.c.configure_window(w, &aux);
        let _ = x.c.flush();
    }

    /// moves the window so its client area starts at (x, y), size unchanged
    pub fn move_client_to(self, px: i32, py: i32) {
        let Win::X(w) = self else { return other::Win.move_client_to(px, py) };
        let Some(x) = x() else { return };
        let _ = x.c.configure_window(w, &ConfigureWindowAux::new().x(px).y(py));
        let _ = x.c.flush();
    }

    /// the whole screen the window is on (the monitor holding its middle)
    pub fn monitor_rect(self) -> Rect {
        let Win::X(_) = self else { return other::Win.monitor_rect() };
        let Some(x) = x() else { return Rect::default() };
        let all = || -> Option<Vec<Rect>> {
            let m = x.c.randr_get_monitors(x.root, true).ok()?.reply().ok()?;
            Some(m.monitors.iter().map(|m| Rect { left: m.x as i32, top: m.y as i32, right: m.x as i32 + m.width as i32, bottom: m.y as i32 + m.height as i32 }).collect())
        };
        let screen = || {
            let s = &x.c.setup().roots[0];
            Rect { left: 0, top: 0, right: s.width_in_pixels as i32, bottom: s.height_in_pixels as i32 }
        };
        let r = self.client_rect();
        let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
        let mons = all().unwrap_or_default();
        mons.iter().find(|m| cx >= m.left && cx < m.right && cy >= m.top && cy < m.bottom).or(mons.first()).copied().unwrap_or_else(screen)
    }

    /// the screen the window is on, without panels (the desktop's work area cut to that monitor)
    pub fn work_area(self) -> Rect {
        let Win::X(_) = self else { return other::Win.work_area() };
        let m = self.monitor_rect();
        let Some(x) = x() else { return m };
        let wa = x.c.get_property(false, x.root, x.a._NET_WORKAREA, AtomEnum::CARDINAL, 0, 4).ok().and_then(|c| c.reply().ok());
        let Some(v) = wa.and_then(|p| p.value32().map(|v| v.collect::<Vec<u32>>())).filter(|v| v.len() == 4) else { return m };
        let (l, t) = (v[0] as i32, v[1] as i32);
        let w = Rect { left: l, top: t, right: l + v[2] as i32, bottom: t + v[3] as i32 };
        let cut = Rect { left: m.left.max(w.left), top: m.top.max(w.top), right: m.right.min(w.right), bottom: m.bottom.min(w.bottom) };
        if cut.w() > 0 && cut.h() > 0 { cut } else { m }
    }

    /// only these parts of the client area (client coordinates) show and take clicks; `None`: all of it
    pub fn set_shape(self, parts: Option<&[Rect]>) {
        let Win::X(w) = self else { return };
        let Some(x) = x() else { return };
        for kind in [shape::SK::BOUNDING, shape::SK::INPUT] {
            let _ = match parts.filter(|p| !p.is_empty()) {
                None => x.c.shape_mask(shape::SO::SET, kind, w, 0, 0, x11rb::NONE),
                Some(parts) => {
                    let rects: Vec<Rectangle> = parts
                        .iter()
                        .map(|p| Rectangle { x: p.left as i16, y: p.top as i16, width: p.w().max(0) as u16, height: p.h().max(0) as u16 })
                        .collect();
                    x.c.shape_rectangles(shape::SO::SET, kind, ClipOrdering::UNSORTED, w, 0, 0, &rects)
                }
            };
        }
        let _ = x.c.flush();
    }

    pub fn visible(self) -> bool {
        let Win::X(w) = self else { return other::Win.visible() };
        let Some(x) = x() else { return false };
        x.c.get_window_attributes(w).ok().and_then(|c| c.reply().ok()).is_some_and(|a| a.map_state == MapState::VIEWABLE)
    }

    pub fn hide(self) {
        let Win::X(w) = self else { return other::Win.hide() };
        let Some(x) = x() else { return };
        let _ = x.c.unmap_window(w);
        // ICCCM: a synthetic UnmapNotify tells the window manager the window is withdrawn
        let ev = UnmapNotifyEvent { response_type: UNMAP_NOTIFY_EVENT, sequence: 0, event: x.root, window: w, from_configure: false };
        let _ = x.c.send_event(false, x.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev);
        let _ = x.c.flush();
    }

    pub fn show(self) {
        let Win::X(w) = self else { return other::Win.show() };
        let Some(x) = x() else { return };
        if !self.visible() {
            // a hidden window has no state on the window manager's side: set it before mapping
            let mut states = vec![x.a._NET_WM_STATE_SKIP_TASKBAR, x.a._NET_WM_STATE_SKIP_PAGER];
            if TOPMOST.load(Ordering::Relaxed) { states.push(x.a._NET_WM_STATE_ABOVE); }
            let _ = x.c.change_property32(PropMode::REPLACE, w, x.a._NET_WM_STATE, AtomEnum::ATOM, &states);
            let _ = x.c.map_window(w);
        }
        let _ = x.c.configure_window(w, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        // source 2 (a pager) rather than 1 (an app): window managers let it past focus-stealing prevention
        self.message(x, x.a._NET_ACTIVE_WINDOW, [2, x11rb::CURRENT_TIME, 0, 0, 0]);
        let _ = x.c.flush();
    }

    /// the window has the keyboard
    pub fn foreground(self) -> bool {
        let Win::X(w) = self else { return other::Win.foreground() };
        let Some(x) = x() else { return false };
        let p = x.c.get_property(false, x.root, x.a._NET_ACTIVE_WINDOW, AtomEnum::WINDOW, 0, 1).ok().and_then(|c| c.reply().ok());
        p.and_then(|p| p.value32().and_then(|mut v| v.next())) == Some(w)
    }

    pub fn set_topmost(self, on: bool) {
        TOPMOST.store(on, Ordering::Relaxed);
        let Win::X(_) = self else { return };
        if !self.visible() { return; }
        if let Some(x) = x() { self.state(x, on, x.a._NET_WM_STATE_ABOVE, 0); }
    }

    /// once, after the window opens: no taskbar button, no pager entry
    pub fn prepare(self) {
        let Win::X(_) = self else { return };
        if let Some(x) = x() { self.state(x, true, x.a._NET_WM_STATE_SKIP_TASKBAR, x.a._NET_WM_STATE_SKIP_PAGER); }
    }

    /// the pointer on the screen
    pub fn cursor_pos(self) -> Pt {
        let Win::X(_) = self else { return other::Win.cursor_pos() };
        pointer().map_or(Pt::default(), |p| Pt { x: p.root_x as i32, y: p.root_y as i32 })
    }

    /// `_NET_WM_STATE` add/remove (up to two states at once) on a mapped window
    fn state(self, x: &X, on: bool, a: Atom, b: Atom) {
        self.message(x, x.a._NET_WM_STATE, [on as u32, a, b, 1, 0]);
        let _ = x.c.flush();
    }

    fn message(self, x: &X, kind: Atom, data: [u32; 5]) {
        let Win::X(w) = self else { return };
        let ev = ClientMessageEvent::new(32, w, kind, data);
        let _ = x.c.send_event(false, x.root, EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY, ev);
    }
}

fn pointer() -> Option<QueryPointerReply> {
    let x = x()?;
    x.c.query_pointer(x.root).ok()?.reply().ok()
}

/// the left mouse button is held, wherever the pointer is
pub fn left_button_down() -> bool { pointer().is_some_and(|p| p.mask.contains(KeyButMask::BUTTON1)) }
