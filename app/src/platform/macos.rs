//! AppKit: the window is an NSWindow, moved and sized with `setFrame:`. AppKit counts in points from the
//! bottom-left corner of the main screen; this layer hands out points from the top-left, so every rectangle
//! is flipped against the main screen's height. Points, not pixels: screens of different density share one
//! space of points but none of pixels, so a window dragged from a Retina screen to a plain one would jump
//! (`px_scale` is 1 here: the app's "pixels" are points on the Mac).
//! A window region does not exist here: `set_shape` makes the window ignore the mouse while the pointer
//! is outside the given parts, checked on the main queue every 30 ms while the window is shown with a shape.

#![allow(deprecated)] // cocoa 0.26 marks itself deprecated in favour of objc2; GPUI 0.2 still uses it

use super::{Pt, Rect};
use cocoa::base::{id, nil, BOOL, NO, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize};
use objc::{class, msg_send, sel, sel_impl};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread::Thread;

pub use super::other::local_time;

/// The AppKit window and the GPUI view that receives native keyboard events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Win(usize, usize);

/// the height of the screen with the menu bar, which AppKit's coordinates start from
fn main_height() -> f64 {
    unsafe {
        let screens: id = msg_send![class!(NSScreen), screens];
        if screens == nil { return 0.; }
        let n: usize = msg_send![screens, count];
        if n == 0 { return 0.; }
        let first: id = msg_send![screens, objectAtIndex: 0usize];
        let f: NSRect = msg_send![first, frame];
        f.size.height
    }
}

/// what one logical pixel of the window is in this layer's units: points, so 1
pub fn px_scale(_window: &gpui::Window) -> f32 { 1. }

impl Win {
    pub fn of(window: &gpui::Window) -> Option<Win> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let RawWindowHandle::AppKit(h) = HasWindowHandle::window_handle(window).ok()?.as_raw() else { return None };
        let view = h.ns_view.as_ptr() as id;
        let w: id = unsafe { msg_send![view, window] };
        (w != nil).then_some(Win(w as usize, view as usize))
    }

    fn w(self) -> id { self.0 as id }

    fn to_px(self, r: NSRect) -> Rect {
        let top = main_height() - (r.origin.y + r.size.height);
        let p = |v: f64| v.round() as i32;
        Rect { left: p(r.origin.x), top: p(top), right: p(r.origin.x + r.size.width), bottom: p(top + r.size.height) }
    }

    fn from_px(self, r: Rect) -> NSRect {
        let (x, y, w, h) = (r.left as f64, r.top as f64, r.w() as f64, r.h() as f64);
        NSRect::new(NSPoint::new(x, main_height() - y - h), NSSize::new(w, h))
    }

    /// the client area on the screen
    pub fn client_rect(self) -> Rect {
        if self.0 == 0 { return Rect::default(); }
        unsafe {
            let f: NSRect = msg_send![self.w(), frame];
            let c: NSRect = msg_send![self.w(), contentRectForFrameRect: f];
            self.to_px(c)
        }
    }

    /// moves and sizes the window so its client area is `r`
    pub fn set_client_rect(self, r: Rect) {
        if self.0 == 0 { return; }
        unsafe {
            let c = self.from_px(r);
            let f: NSRect = msg_send![self.w(), frameRectForContentRect: c];
            let _: () = msg_send![self.w(), setFrame: f display: NO];
        }
    }

    /// moves the window so its client area starts at (x, y), size unchanged
    pub fn move_client_to(self, x: i32, y: i32) {
        let r = self.client_rect();
        self.set_client_rect(Rect { left: x, top: y, right: x + r.w(), bottom: y + r.h() });
    }

    fn screen(self) -> id {
        unsafe {
            let s: id = if self.0 == 0 { nil } else { msg_send![self.w(), screen] };
            if s != nil { s } else { msg_send![class!(NSScreen), mainScreen] }
        }
    }
    /// the screen the window is on, without the menu bar and the Dock
    pub fn work_area(self) -> Rect {
        let s = self.screen();
        if s == nil { return Rect::default(); }
        let f: NSRect = unsafe { msg_send![s, visibleFrame] };
        self.to_px(f)
    }
    /// the whole screen the window is on
    pub fn monitor_rect(self) -> Rect {
        let s = self.screen();
        if s == nil { return Rect::default(); }
        let f: NSRect = unsafe { msg_send![s, frame] };
        self.to_px(f)
    }

    /// Full bounds of the display under the pointer, including the menu bar and Dock.
    pub fn pointer_monitor_rect(self) -> Rect {
        unsafe {
            let p: NSPoint = msg_send![class!(NSEvent), mouseLocation];
            let screens: id = msg_send![class!(NSScreen), screens];
            if screens == nil { return self.monitor_rect(); }
            let n: usize = msg_send![screens, count];
            for i in 0..n {
                let screen: id = msg_send![screens, objectAtIndex: i];
                let f: NSRect = msg_send![screen, frame];
                if p.x >= f.origin.x && p.x < f.origin.x + f.size.width && p.y >= f.origin.y && p.y < f.origin.y + f.size.height {
                    return self.to_px(f);
                }
            }
        }
        self.monitor_rect()
    }

    /// only these parts of the client area (client coordinates) take clicks; `None`: all of it.
    /// What lies outside them is transparent anyway; AppKit just would not let clicks through it.
    pub fn set_shape(self, parts: Option<&[Rect]>) {
        if self.0 == 0 { return; }
        *SHAPE.lock().unwrap_or_else(|e| e.into_inner()) = (self, parts.filter(|p| !p.is_empty()).map(|p| p.to_vec()));
        refresh_shape();
    }

    pub fn visible(self) -> bool {
        if self.0 == 0 { return false; }
        unsafe {
            let v: BOOL = msg_send![self.w(), isVisible];
            let m: BOOL = msg_send![self.w(), isMiniaturized];
            v == YES && m == NO
        }
    }
    pub fn hide(self) {
        if self.0 == 0 { return; }
        unsafe { let _: () = msg_send![self.w(), orderOut: nil]; }
        refresh_shape();
    }
    pub fn set_alpha(self, alpha: f32) {
        if self.0 == 0 { return; }
        unsafe { let _: () = msg_send![self.w(), setAlphaValue: alpha.clamp(0., 1.) as f64]; }
    }
    pub fn show(self) {
        if self.0 == 0 { return; }
        unsafe {
            let m: BOOL = msg_send![self.w(), isMiniaturized];
            if m == YES { let _: () = msg_send![self.w(), deminiaturize: nil]; }
            // Full-screen windows sit above NSFloatingWindowLevel. Use the status-window
            // level while Notedd is active, as menu-bar overlays do.
            let _: () = msg_send![self.w(), setLevel: 25isize];
            // A nonactivating panel can take keyboard focus without switching applications or Spaces.
            let view = self.1 as id;
            let responder: id = msg_send![self.w(), firstResponder];
            if responder != view {
                let _: BOOL = msg_send![self.w(), makeFirstResponder: view];
            }
            let _: () = msg_send![self.w(), makeKeyAndOrderFront: nil];
        }
        refresh_shape();
    }
    /// the window has the keyboard
    pub fn foreground(self) -> bool {
        if self.0 == 0 { return false; }
        unsafe {
            let key: BOOL = msg_send![self.w(), isKeyWindow];
            key == YES
        }
    }
    pub fn set_topmost(self, on: bool) {
        if self.0 == 0 { return; }
        // NSStatusWindowLevel / NSNormalWindowLevel
        let level: isize = if on { 25 } else { 0 };
        unsafe { let _: () = msg_send![self.w(), setLevel: level]; }
    }

    /// once, after the window opens: no shadow, no Dock icon, and it comes to whichever Space is current
    pub fn prepare(self) {
        if self.0 == 0 { return; }
        unsafe {
            let _: () = msg_send![self.w(), setHasShadow: NO];
            // GPUI creates a titled full-size window even without a title bar. A borderless
            // window avoids the native outline around our transparent, custom-shaped content.
            // Borderless NSPanel, retaining NSWindowStyleMaskNonactivatingPanel.
            let _: () = msg_send![self.w(), setStyleMask: (1usize << 7)];
            let _: () = msg_send![self.w(), setHidesOnDeactivate: NO];
            let _: () = msg_send![self.w(), setBecomesKeyOnlyIfNeeded: NO];
            let _: () = msg_send![self.w(), setInitialFirstResponder: self.1 as id];
            // Move to the current Space when activated, including other apps' full-screen Spaces.
            let behavior: usize = (1 << 0) | (1 << 8) | (1 << 18);
            let _: () = msg_send![self.w(), setCollectionBehavior: behavior];
            // NSApplicationActivationPolicyAccessory: the tray icon stands for the app, like on Windows
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let _: BOOL = msg_send![app, setActivationPolicy: 1isize];
        }
    }

    /// When invoked on another display, bring the window to the display containing the pointer.
    pub fn move_to_pointer_screen(self) {
        if self.0 == 0 { return; }
        unsafe {
            let p: NSPoint = msg_send![class!(NSEvent), mouseLocation];
            let screens: id = msg_send![class!(NSScreen), screens];
            let n: usize = msg_send![screens, count];
            let current: id = msg_send![self.w(), screen];
            for i in 0..n {
                let screen: id = msg_send![screens, objectAtIndex: i];
                let frame: NSRect = msg_send![screen, frame];
                if p.x < frame.origin.x || p.x >= frame.origin.x + frame.size.width || p.y < frame.origin.y || p.y >= frame.origin.y + frame.size.height { continue; }
                if screen == current { break; }
                let visible: NSRect = msg_send![screen, visibleFrame];
                let mut window_frame: NSRect = msg_send![self.w(), frame];
                window_frame.origin.x = visible.origin.x + (visible.size.width - window_frame.size.width).max(0.) / 2.;
                window_frame.origin.y = visible.origin.y + (visible.size.height - window_frame.size.height).max(0.) / 2.;
                let _: () = msg_send![self.w(), setFrame: window_frame display: NO];
                break;
            }
        }
    }

    /// the pointer on the screen
    pub fn cursor_pos(self) -> Pt {
        let p: NSPoint = unsafe { msg_send![class!(NSEvent), mouseLocation] };
        Pt { x: p.x.round() as i32, y: (main_height() - p.y).round() as i32 }
    }
}

fn pressed_buttons() -> usize { unsafe { msg_send![class!(NSEvent), pressedMouseButtons] } }

/// the left mouse button is held, wherever the pointer is
pub fn left_button_down() -> bool { pressed_buttons() & 1 != 0 }

// ---- click-through outside the shape

static SHAPE: Mutex<(Win, Option<Vec<Rect>>)> = Mutex::new((Win(0, 0), None));
static IGNORING: AtomicBool = AtomicBool::new(false);
/// the window is shown with a shape: the poller runs (otherwise it sleeps until woken)
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POLLER: OnceLock<Thread> = OnceLock::new();

#[repr(C)]
struct DispatchQueue { _p: [u8; 0] }
extern "C" {
    static _dispatch_main_q: DispatchQueue;
    fn dispatch_async_f(queue: *const DispatchQueue, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

fn shape() -> (Win, Option<Vec<Rect>>) { SHAPE.lock().unwrap_or_else(|e| e.into_inner()).clone() }

/// on the main thread, after the shape or the visibility changes: polling runs only while it matters
fn refresh_shape() {
    let (w, parts) = shape();
    let on = w.0 != 0 && parts.is_some() && w.visible();
    ACTIVE.store(on, Ordering::Relaxed);
    if on {
        POLLER
            .get_or_init(|| {
                std::thread::spawn(|| loop {
                    if !ACTIVE.load(Ordering::Relaxed) { std::thread::park(); continue; }
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    unsafe { dispatch_async_f(&_dispatch_main_q, std::ptr::null_mut(), check_shape) };
                })
                .thread()
                .clone()
            })
            .unpark();
    }
    check_shape(std::ptr::null_mut());
}

/// on the main queue: the window takes the mouse only while the pointer is over one of the parts
extern "C" fn check_shape(_: *mut c_void) {
    let (w, parts) = shape();
    if w.0 == 0 { return; }
    let ignore = match &parts {
        Some(parts) if w.visible() => {
            let (c, r) = (w.cursor_pos(), w.client_rect());
            let (x, y) = (c.x - r.left, c.y - r.top);
            !parts.iter().any(|p| x >= p.left && x < p.right && y >= p.top && y < p.bottom)
        }
        _ => false,
    };
    // a button held over the window (a drag or a sizing going on): AppKit would stop sending it the
    // moment the window starts ignoring the mouse, so letting clicks through waits for the release
    if ignore && !IGNORING.load(Ordering::Relaxed) && pressed_buttons() != 0 { return; }
    if IGNORING.swap(ignore, Ordering::Relaxed) == ignore { return; }
    let v: BOOL = if ignore { YES } else { NO };
    unsafe { let _: () = msg_send![w.w(), setIgnoresMouseEvents: v]; }
}
