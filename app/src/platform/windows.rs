//! Win32: the window is moved and sized with SetWindowPos, shaped with a region, and kept off the taskbar.

use super::{LocalTime, Pt, Rect};
use windows::Win32::Foundation::{FILETIME, HWND, LPARAM, POINT, RECT, SYSTEMTIME, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::*;

static ICO: &[u8] = include_bytes!("../../../assets/notedd.ico");

/// the main window
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Win(isize);

fn rect(r: RECT) -> Rect { Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom } }

impl Win {
    pub fn of(window: &gpui::Window) -> Option<Win> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(Win(h.hwnd.get() as isize)),
            _ => None,
        }
    }

    fn h(self) -> HWND { HWND(self.0 as *mut _) }

    /// the client area on the screen
    pub fn client_rect(self) -> Rect {
        let mut cl = RECT::default();
        let mut org = POINT { x: 0, y: 0 };
        unsafe {
            let _ = GetClientRect(self.h(), &mut cl);
            let _ = ClientToScreen(self.h(), &mut org);
        }
        Rect { left: org.x, top: org.y, right: org.x + cl.right, bottom: org.y + cl.bottom }
    }

    /// (left, top, extra width, extra height) the invisible frame adds around the client area
    fn frame(self) -> (i32, i32, i32, i32) {
        let mut r = RECT::default();
        unsafe { let _ = GetWindowRect(self.h(), &mut r); }
        let c = self.client_rect();
        (c.left - r.left, c.top - r.top, (r.right - r.left) - c.w(), (r.bottom - r.top) - c.h())
    }

    /// moves and sizes the window so its client area is `r`
    pub fn set_client_rect(self, r: Rect) {
        let (ox, oy, ew, eh) = self.frame();
        unsafe {
            let _ = SetWindowPos(self.h(), None, r.left - ox, r.top - oy, r.w() + ew, r.h() + eh, SWP_NOZORDER | SWP_NOACTIVATE);
            // GPUI's resize callback is dropped while this window is being updated: say it again from the message loop
            let mut rc = RECT::default();
            if GetClientRect(self.h(), &mut rc).is_ok() {
                let lp = (((rc.bottom - rc.top) as u16 as isize) << 16) | ((rc.right - rc.left) as u16 as isize);
                let _ = PostMessageW(Some(self.h()), WM_SIZE, WPARAM(0), LPARAM(lp));
            }
        }
    }

    /// moves the window so its client area starts at (x, y), size unchanged
    pub fn move_client_to(self, x: i32, y: i32) {
        let (ox, oy, _, _) = self.frame();
        unsafe { let _ = SetWindowPos(self.h(), None, x - ox, y - oy, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE); }
    }

    fn monitor(self) -> MONITORINFO {
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        unsafe { let _ = GetMonitorInfoW(MonitorFromWindow(self.h(), MONITOR_DEFAULTTONEAREST), &mut mi); }
        mi
    }
    /// the screen the window is on, without the taskbar
    pub fn work_area(self) -> Rect { rect(self.monitor().rcWork) }
    /// the whole screen the window is on
    pub fn monitor_rect(self) -> Rect { rect(self.monitor().rcMonitor) }

    /// only these parts of the client area (client coordinates) show and take clicks; `None`: all of it
    pub fn set_shape(self, parts: Option<&[Rect]>) {
        unsafe {
            let Some(parts) = parts.filter(|p| !p.is_empty()) else { SetWindowRgn(self.h(), None, true); return };
            // region coordinates count from the window's corner, not the client area's
            let (ox, oy, _, _) = self.frame();
            let mk = |r: &Rect| CreateRectRgn(ox + r.left, oy + r.top, ox + r.right, oy + r.bottom);
            let all = mk(&parts[0]);
            for p in &parts[1..] {
                let one = mk(p);
                CombineRgn(Some(all), Some(all), Some(one), RGN_OR);
                let _ = DeleteObject(one.into());
            }
            SetWindowRgn(self.h(), Some(all), true);
        }
    }

    pub fn visible(self) -> bool { unsafe { IsWindowVisible(self.h()).as_bool() && !IsIconic(self.h()).as_bool() } }
    pub fn hide(self) { unsafe { let _ = ShowWindow(self.h(), SW_HIDE); } }
    pub fn show(self) {
        unsafe {
            let _ = ShowWindow(self.h(), if IsIconic(self.h()).as_bool() { SW_RESTORE } else { SW_SHOW });
            // Windows lets only the foreground process take focus; a tap on Alt lifts that rule.
            let _ = SetForegroundWindow(self.h());
        }
    }
    /// the window has the keyboard
    pub fn foreground(self) -> bool { unsafe { GetForegroundWindow() == self.h() } }
    pub fn set_topmost(self, on: bool) {
        unsafe { let _ = SetWindowPos(self.h(), Some(if on { HWND_TOPMOST } else { HWND_NOTOPMOST }), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); }
    }

    /// the pointer on the screen
    pub fn cursor_pos(self) -> Pt {
        let mut p = POINT::default();
        unsafe { let _ = GetCursorPos(&mut p); }
        Pt { x: p.x, y: p.y }
    }

    /// once, after the window opens: the app icon; no frame, no shadow, square corners, no taskbar button
    pub fn prepare(self) {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
        let h = self.h();
        unsafe {
            if let Some(big) = icon(32) { SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(big.0 as isize))); }
            if let Some(small) = icon(16) { SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(small.0 as isize))); }
            let none: u32 = 0xFFFF_FFFE;
            let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(34), &none as *const _ as _, 4);
            let square: u32 = 1;
            let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(33), &square as *const _ as _, 4);
            let off: u32 = 1;
            let _ = DwmSetWindowAttribute(h, DWMWINDOWATTRIBUTE(2), &off as *const _ as _, 4);
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW.0 as isize) & !(WS_EX_APPWINDOW.0 as isize));
        }
    }
}

fn icon(size: i32) -> Option<HICON> {
    if ICO.len() < 22 { return None; }
    let count = u16::from_le_bytes([ICO[4], ICO[5]]) as usize;
    let mut best: Option<(i32, usize, usize)> = None;
    for i in 0..count {
        let e = 6 + i * 16;
        if e + 16 > ICO.len() { break; }
        let w = if ICO[e] == 0 { 256 } else { ICO[e] as i32 };
        let len = u32::from_le_bytes(ICO[e + 8..e + 12].try_into().ok()?) as usize;
        let off = u32::from_le_bytes(ICO[e + 12..e + 16].try_into().ok()?) as usize;
        let score = (w - size).abs();
        if off + len <= ICO.len() && best.is_none_or(|b| score < (b.0 - size).abs()) { best = Some((w, off, len)); }
    }
    let (w, off, len) = best?;
    unsafe { CreateIconFromResourceEx(&ICO[off..off + len], true, 0x30000, w, w, LR_DEFAULTCOLOR).ok() }
}

/// what one logical pixel of the window is in this layer's units: physical pixels
pub fn px_scale(window: &gpui::Window) -> f32 { window.scale_factor() }

/// the left mouse button is held, wherever the pointer is
pub fn left_button_down() -> bool { unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000 != 0 } }

/// a FILETIME (100 ns ticks since 1601, UTC) on the local clock
pub fn local_time(ft: u64) -> LocalTime {
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut utc, mut loc) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    unsafe {
        let _ = FileTimeToSystemTime(&f, &mut utc);
        let _ = SystemTimeToTzSpecificLocalTime(None, &utc, &mut loc);
    }
    LocalTime { year: loc.wYear, month: loc.wMonth, day: loc.wDay, hour: loc.wHour, minute: loc.wMinute }
}
