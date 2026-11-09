//! Stand-in for the systems that have no window layer yet (the Mac, Linux). It keeps the app building
//! and running in a plain way: the window stays where the system put it, the numbers the motion code
//! asks for are remembered rather than applied. Replaced by `macos.rs` / `linux.rs`.

use super::{LocalTime, Pt, Rect};
use std::cell::Cell;

thread_local! {
    static RECT: Cell<Rect> = const { Cell::new(Rect { left: 0, top: 0, right: 882, bottom: 586 }) };
    static VISIBLE: Cell<bool> = const { Cell::new(true) };
}

/// the main window
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Win;

impl Win {
    pub fn of(window: &gpui::Window) -> Option<Win> {
        let b = window.bounds();
        let s = window.scale_factor();
        let p = |v: gpui::Pixels| (f32::from(v) * s).round() as i32;
        RECT.with(|r| r.set(Rect { left: p(b.origin.x), top: p(b.origin.y), right: p(b.origin.x + b.size.width), bottom: p(b.origin.y + b.size.height) }));
        Some(Win)
    }
    pub fn client_rect(self) -> Rect { RECT.with(|r| r.get()) }
    pub fn set_client_rect(self, r: Rect) { RECT.with(|c| c.set(r)); }
    pub fn move_client_to(self, x: i32, y: i32) {
        RECT.with(|c| { let r = c.get(); c.set(Rect { left: x, top: y, right: x + r.w(), bottom: y + r.h() }); });
    }
    pub fn work_area(self) -> Rect { Rect { left: 0, top: 0, right: 1920, bottom: 1080 } }
    pub fn monitor_rect(self) -> Rect { self.work_area() }
    pub fn set_shape(self, _parts: Option<&[Rect]>) {}
    pub fn visible(self) -> bool { VISIBLE.with(|v| v.get()) }
    pub fn hide(self) { VISIBLE.with(|v| v.set(false)); }
    pub fn show(self) { VISIBLE.with(|v| v.set(true)); }
    pub fn foreground(self) -> bool { self.visible() }
    pub fn set_topmost(self, _on: bool) {}
    pub fn prepare(self) {}
}

pub fn cursor_pos() -> Pt { Pt::default() }
pub fn left_button_down() -> bool { false }

/// a FILETIME (100 ns ticks since 1601, UTC) on the local clock
pub fn local_time(ft: u64) -> LocalTime {
    use chrono::{Datelike, TimeZone, Timelike};
    let unix = (ft / 10_000_000) as i64 - 11_644_473_600;
    match chrono::Local.timestamp_opt(unix, 0).single() {
        Some(t) => LocalTime { year: t.year() as u16, month: t.month() as u16, day: t.day() as u16, hour: t.hour() as u16, minute: t.minute() as u16 },
        None => LocalTime::default(),
    }
}
