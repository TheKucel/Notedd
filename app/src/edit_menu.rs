//! Formatting and clipboard popup for the simple editor. It shares the palette's layer.
use super::*;

const ROW: f32 = 32.0;
const PAD: f32 = 6.0;
const FORMAT_START: usize = 5;
const GROUP_GAP: f32 = 10.0;
const SHADOW: f32 = 20.0;
const ENTRY: f32 = 0.14;

const GLYPHS: [&str; 10] = [
    "M7 3L3 7l4 4M3 7h6l3 1.5 1 2.5-1 2H8", // Undo
    "M3 3h2l2 2-2 2H3L2 5zM3 9h2l2 2-2 2H3l-1-2zM6 6l7 7M6 10l7-7", // Cut
    "M6 6h7v7H6zM3 10V3h7", // Copy
    "M5 4H3v10h10V4h-2M6 2h4l1 2-1 2H6L5 4z", // Paste
    "M6 3H3v3M10 3h3v3M13 10v3h-3M6 13H3v-3M6 7h4M6 9h4", // Select all
    "M5 13V3h4l2 1v2L9 8H5M9 8l3 1v2l-2 2H5", // Bold
    "M7 3h6M3 13h6M10 3L6 13", // Italic
    "M11 4L9 3H6L4 5v1l2 2 4 1 2 1v1l-2 2H6l-2-1M2 8h12", // Strike
    "M5 5L2 8l3 3M11 5l3 3-3 3M9 4l-2 8", // Code
    "M4 9l6-6 3 3-6 6H4zM4 12l-1 2M8 14h5", // Highlight
];

#[derive(Clone, Copy)]
enum Action {
    Edit(u32),
    Format(md::Format),
}

struct Item {
    action: Action,
    label: Vec<u16>,
    keys: Vec<u16>,
    enabled: bool,
    glyph: ID2D1PathGeometry,
}

pub(super) struct EditMenu {
    items: Vec<Item>,
    origin: POINT,
    width: f32,
    opened: Instant,
    hot: Option<usize>,
    pressed: Option<usize>,
}

impl EditMenu {
    fn row_y(i: usize) -> f32 { SHADOW + PAD + i as f32 * ROW + if i >= FORMAT_START { GROUP_GAP } else { 0.0 } }
    fn height(&self) -> f32 { 2.0 * PAD + self.items.len() as f32 * ROW + GROUP_GAP }
    fn progress(&self) -> f32 {
        let t = (self.opened.elapsed().as_secs_f32() / ENTRY).min(1.0);
        1.0 - (1.0 - t).powi(3)
    }
    fn offset(&self) -> f32 { (self.progress() - 1.0) * 5.0 }
    fn hit(&self, x: f32, y: f32) -> (bool, Option<usize>) {
        let y = y - self.offset();
        let inside = (SHADOW..SHADOW + self.width).contains(&x) && (SHADOW..SHADOW + self.height()).contains(&y);
        let row = inside.then(|| self.items.iter().enumerate().position(|(i, item)| {
            item.enabled && (Self::row_y(i)..Self::row_y(i) + ROW).contains(&y)
        })).flatten();
        (inside, row)
    }
}

impl App {
    pub(super) unsafe fn edit_menu(&mut self, lp: LPARAM) {
        self.close_edit_menu();
        self.close_palette();
        let (a, b) = self.get_sel();
        let selection = a < b;
        let undo = SendMessageW(self.edit, EM_CANUNDO, WPARAM(0), LPARAM(0)).0 != 0;
        let specs = [
            (WM_UNDO, ui::UNDO, "Ctrl+Z", undo),
            (WM_CUT, ui::CUT, "Ctrl+X", selection),
            (WM_COPY, ui::COPY, "Ctrl+C", selection),
            (WM_PASTE, ui::PASTE, "Ctrl+V", true),
            (EM_EXSETSEL, ui::SELECT_ALL, "Ctrl+A", !self.styled_text.is_empty()),
        ].into_iter().map(|(msg, label, keys, enabled)| (Action::Edit(msg), label.get(), keys.to_owned(), enabled));
        let formats = SIMPLE_FORMATS.into_iter().map(|(f, label, keys)| (f, label.get(), keys))
            .chain([(md::Format::Highlight, ui::KEY_NAMES[10].get(), self.prefs.keys[10])])
            .map(|(f, label, (mods, vk))| (Action::Format(f), label, if vk == 0 { String::new() } else { key_parts(mods, vk).join("+") }, selection));
        let items: Result<Vec<_>> = specs.chain(formats).zip(GLYPHS).map(|((action, label, keys, enabled), path)| {
            Ok(Item { action, label: u(label), keys: u(&keys), enabled, glyph: icons::path(&self.factory, path)? })
        }).collect();
        let Ok(items) = items else { return };
        let label_w = items.iter().map(|i| self.text_w(&i.label, &self.fonts.small)).fold(0.0, f32::max);
        let key_w = items.iter().map(|i| self.text_w(&i.keys, &self.fonts.small_r)).fold(0.0, f32::max);
        let width = (42.0 + label_w + 24.0 + key_w + 16.0).max(244.0).ceil();
        let mut pt = POINT { x: lp.0 as i16 as i32, y: (lp.0 >> 16) as i16 as i32 };
        let keyboard = pt.x == -1 && pt.y == -1;
        if keyboard {
            pt = POINT::default();
            let _ = GetCaretPos(&mut pt);
            let _ = ClientToScreen(self.edit, &mut pt);
        }
        let hot = if keyboard { items.iter().position(|item| item.enabled) } else { None };
        let mut menu = EditMenu { items, origin: pt, width, opened: Instant::now(), hot, pressed: None };
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mi);
        let work = mi.rcWork;
        let px = |v: f32| (v * self.scale).ceil() as i32;
        let (w, h) = (px(width), px(menu.height()));
        let (mut x, mut y) = (pt.x + px(2.0), pt.y + px(if keyboard { 22.0 } else { 4.0 }));
        if x + w > work.right { x = pt.x - w - px(2.0); }
        if y + h > work.bottom { y = pt.y - h - px(4.0); }
        menu.origin = POINT {
            x: x.min(work.right - w).max(work.left) - px(SHADOW),
            y: y.min(work.bottom - h).max(work.top) - px(SHADOW),
        };
        self.edit_menu = Some(menu);
        self.draw_edit_menu();
        SetTimer(self.hwnd, TIMER_EDIT_MENU, 12, None);
    }

    pub(super) unsafe fn draw_edit_menu(&mut self) {
        let Some(menu) = &self.edit_menu else { return };
        let t = menu.progress();
        let y = SHADOW + menu.offset();
        let (w, h) = (menu.width, menu.height());
        let px = |v: f32| (v * self.scale).ceil() as i32;
        let tint = self.target_color(&self.cur);
        let (fonts, round) = (&self.fonts, &self.round);
        self.pop.present(menu.origin.x, menu.origin.y, px(w + 2.0 * SHADOW), px(h + 2.0 * SHADOW), t.max(0.06), |rt| {
            let base = shift(0.0, menu.offset());
            rt.SetTransform(&shift(0.0, 0.0));
            let bounds = rect(SHADOW, y, SHADOW + w, y + h);
            let _ = soft_shadow(rt, bounds, 13.0, 5.0, 15.0, 0.18);
            let Ok(brush) = rt.CreateSolidColorBrush(&rgb(tint, 1.0), None) else { return };
            rt.FillRoundedRectangle(&rrect(bounds.left, bounds.top, bounds.right, bounds.bottom, 13.0), &brush);
            brush.SetColor(&rgb(line_c(), 0.55));
            rt.DrawRoundedRectangle(&rrect(bounds.left + 0.5, bounds.top + 0.5, bounds.right - 0.5, bounds.bottom - 0.5, 12.5), &brush, 1.0, None);
            rt.SetTransform(&base);
            let sep = EditMenu::row_y(FORMAT_START) - GROUP_GAP / 2.0;
            brush.SetColor(&rgb(ink(), 0.15));
            rt.DrawLine(pt(SHADOW + 12.0, sep), pt(SHADOW + w - 12.0, sep), &brush, 1.0, None);
            for (i, item) in menu.items.iter().enumerate() {
                let y = EditMenu::row_y(i);
                if menu.hot == Some(i) {
                    brush.SetColor(&rgb(ink(), if menu.pressed == Some(i) { 0.15 } else { 0.09 }));
                    rt.FillRoundedRectangle(&rrect(SHADOW + PAD, y + 1.0, SHADOW + w - PAD, y + ROW - 1.0, 7.0), &brush);
                }
                brush.SetColor(&rgb(ink(), if item.enabled { 1.0 } else { 0.34 }));
                rt.SetTransform(&mul(&shift(SHADOW + 15.0, y + (ROW - 16.0) / 2.0), &base));
                rt.DrawGeometry(&item.glyph, &brush, 1.25, round);
                rt.SetTransform(&base);
                rt.DrawText(&item.label, &fonts.small, &rect(SHADOW + 42.0, y, SHADOW + w - 16.0, y + ROW), &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
                brush.SetColor(&rgb(ink2(), if item.enabled { 0.85 } else { 0.34 }));
                rt.DrawText(&item.keys, &fonts.small_r, &rect(SHADOW + 42.0, y, SHADOW + w - 16.0, y + ROW), &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
            }
            rt.SetTransform(&shift(0.0, 0.0));
        });
        if t >= 1.0 { let _ = KillTimer(self.hwnd, TIMER_EDIT_MENU); }
    }

    pub(super) unsafe fn close_edit_menu(&mut self) {
        if self.edit_menu.take().is_some() {
            let _ = KillTimer(self.hwnd, TIMER_EDIT_MENU);
            self.pop.hide();
        }
    }

    unsafe fn run_edit_menu(&mut self, row: usize) {
        let action = self.edit_menu.as_ref().and_then(|m| m.items.get(row)).filter(|i| i.enabled).map(|i| i.action);
        self.close_edit_menu();
        match action {
            Some(Action::Edit(EM_EXSETSEL)) => self.select(0, i32::MAX),
            Some(Action::Edit(msg)) => { SendMessageW(self.edit, msg, WPARAM(0), LPARAM(0)); }
            Some(Action::Format(format)) => self.toggle_format(format),
            None => {}
        }
    }

    pub(super) unsafe fn edit_menu_key(&mut self, vk: u16) -> bool {
        let Some(menu) = &mut self.edit_menu else { return false };
        match VIRTUAL_KEY(vk) {
            VK_ESCAPE => self.close_edit_menu(),
            VK_RETURN | VK_SPACE => if let Some(i) = menu.hot { self.run_edit_menu(i); },
            VK_UP | VK_DOWN | VK_HOME | VK_END => {
                let n = menu.items.len();
                let up = matches!(VIRTUAL_KEY(vk), VK_UP | VK_END);
                let start = match VIRTUAL_KEY(vk) { VK_HOME => n - 1, VK_END => 0, _ => menu.hot.unwrap_or(if up { 0 } else { n - 1 }) };
                menu.hot = (1..=n).map(|k| (start + if up { n - k } else { k }) % n).find(|&i| menu.items[i].enabled);
                self.draw_edit_menu();
            }
            _ => { self.close_edit_menu(); return false; }
        }
        true
    }

    pub(super) unsafe fn edit_menu_mouse(&mut self, hwnd: HWND, msg: u32, lp: LPARAM) -> Option<LRESULT> {
        if !matches!(msg, WM_NCHITTEST | WM_MOUSEACTIVATE | WM_SETCURSOR | WM_MOUSEMOVE | 0x02A3
            | WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_MOUSEWHEEL) { return None; }
        let menu = self.edit_menu.as_mut()?;
        if msg == WM_SETCURSOR {
            let cursor = if menu.hot.is_some() { IDC_HAND } else { IDC_ARROW };
            if let Ok(c) = LoadCursorW(None, cursor) { SetCursor(c); }
            return Some(LRESULT(1));
        }
        let mut p = POINT { x: lp.0 as i16 as i32, y: (lp.0 >> 16) as i16 as i32 };
        if msg == WM_NCHITTEST { let _ = ScreenToClient(hwnd, &mut p); }
        let (inside, row) = menu.hit(p.x as f32 / self.scale, p.y as f32 / self.scale);
        match msg {
            WM_NCHITTEST => return Some(LRESULT(if inside { HTCLIENT as isize } else { HTTRANSPARENT as isize })),
            WM_MOUSEACTIVATE => return Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_MOUSEMOVE => {
                let mut event = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut event);
                if menu.hot != row { menu.hot = row; self.draw_edit_menu(); }
            }
            0x02A3 => { menu.hot = None; menu.pressed = None; self.draw_edit_menu(); }
            WM_LBUTTONDOWN => { menu.pressed = row; self.draw_edit_menu(); }
            WM_LBUTTONUP => {
                if let Some(i) = row.filter(|i| menu.pressed == Some(*i)) { self.run_edit_menu(i); }
                else { menu.pressed = None; self.draw_edit_menu(); }
            }
            WM_RBUTTONDOWN | WM_MOUSEWHEEL => self.close_edit_menu(),
            _ => return None,
        }
        Some(LRESULT(0))
    }
}
