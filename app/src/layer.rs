//! A per-pixel-alpha window drawn with Direct2D: click-through editor overlays and the owned colour popup.

use std::ptr::null_mut;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Layer {
    pub hwnd: HWND,
    dc: HDC,
    bmp: HBITMAP,
    rt: ID2D1DCRenderTarget,
    size: (i32, i32),
    pub shown: bool,
}

impl Layer {
    pub unsafe fn new(factory: &ID2D1Factory, hwnd: HWND, scale: f32) -> Result<Layer> {
        let rt = factory.CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0 * scale, dpiY: 96.0 * scale, usage: D2D1_RENDER_TARGET_USAGE_NONE, minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        })?;
        Ok(Layer { hwnd, dc: CreateCompatibleDC(None), bmp: HBITMAP::default(), rt, size: (0, 0), shown: false })
    }
    /// place the layer (parent-client px for a child, screen px for a popup), draw it and show it
    pub unsafe fn present(&mut self, x: i32, y: i32, w: i32, h: i32, alpha: f32, draw: impl FnOnce(&ID2D1DCRenderTarget)) {
        let (w, h) = (w.max(1), h.max(1));
        if self.size != (w, h) {
            let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
            let mut bits = null_mut();
            let Ok(bmp) = CreateDIBSection(self.dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
            SelectObject(self.dc, bmp);
            if !self.bmp.is_invalid() { let _ = DeleteObject(self.bmp); }
            self.bmp = bmp;
            self.size = (w, h);
        }
        if self.rt.BindDC(self.dc, &RECT { left: 0, top: 0, right: w, bottom: h }).is_err() { return; }
        self.rt.BeginDraw();
        self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
        draw(&self.rt);
        let _ = self.rt.EndDraw(None, None);
        let _ = SetWindowPos(self.hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: (alpha.clamp(0.0, 1.0) * 255.0) as u8, AlphaFormat: 1 };
        let size = SIZE { cx: w, cy: h };
        let src = POINT::default();
        let _ = UpdateLayeredWindow(self.hwnd, None, None, Some(&size), self.dc, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        if !self.shown { let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE); self.shown = true; }
    }
    pub unsafe fn hide(&mut self) {
        if self.shown { let _ = ShowWindow(self.hwnd, SW_HIDE); self.shown = false; }
    }
}
