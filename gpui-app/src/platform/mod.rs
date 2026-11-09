//! The main window and the screen, behind one interface: Win32 on Windows. The Mac and Linux get a
//! stand-in for now, so the app builds there; each will get its own file next to `windows.rs`.
//! Everything is in physical screen pixels; the window's rectangle is its client area (what the
//! folders are drawn in), never the invisible resize frame around it.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }

impl Rect {
    pub fn w(&self) -> i32 { self.right - self.left }
    pub fn h(&self) -> i32 { self.bottom - self.top }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pt { pub x: i32, pub y: i32 }

/// a moment as the local clock shows it
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalTime { pub year: u16, pub month: u16, pub day: u16, pub hour: u16, pub minute: u16 }

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
mod other;
#[cfg(not(windows))]
pub use self::other::*;
