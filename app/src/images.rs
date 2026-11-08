//! Pictures in notes: decoded with WIC, kept as premultiplied DIBs at the size they are shown,
//! and drawn under the image link line of the editor. Pasted bitmaps are saved as PNG files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

struct Shown { w: i32, h: i32, r: i32, dc: HDC, bmp: HBITMAP }
struct Entry { size: Option<(u32, u32)>, mtime: Option<std::time::SystemTime>, shown: Option<Shown> }

pub struct Images { wic: Option<IWICImagingFactory>, cache: HashMap<PathBuf, Entry> }

pub const EXTS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "bmp", "webp", "tif"];
pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map_or(false, |e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
}
fn wide(p: &Path) -> Vec<u16> { p.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect() }
fn mtime(p: &Path) -> Option<std::time::SystemTime> { std::fs::metadata(p).and_then(|m| m.modified()).ok() }

impl Images {
    pub fn new() -> Images {
        let wic = unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() };
        Images { wic, cache: HashMap::new() }
    }
    fn entry(&mut self, p: &Path) -> &mut Entry {
        let m = mtime(p);
        let stale = self.cache.get(p).map_or(true, |e| e.mtime != m);
        if stale {
            if let Some(old) = self.cache.remove(p) { free(old.shown); }
            let size = self.wic.as_ref().and_then(|w| unsafe { frame(w, p) }.ok()).and_then(|f| {
                let (mut w, mut h) = (0u32, 0u32);
                unsafe { f.GetSize(&mut w, &mut h) }.ok().map(|_| (w, h))
            }).filter(|s| s.0 > 0 && s.1 > 0);
            self.cache.insert(p.to_path_buf(), Entry { size, mtime: m, shown: None });
        }
        self.cache.get_mut(p).unwrap()
    }
    /// natural size in pixels, None when the file is missing or not a picture
    pub fn size(&mut self, p: &Path) -> Option<(u32, u32)> { self.entry(p).size }

    /// draws the picture into the box (device px) with corners rounded by `r`, blended over what is there at `alpha`
    pub unsafe fn draw(&mut self, dc: HDC, p: &Path, x: i32, y: i32, w: i32, h: i32, r: i32, alpha: u8) {
        if w <= 0 || h <= 0 { return; }
        let Some(wic) = self.wic.clone() else { return };
        let e = self.entry(p);
        if e.size.is_none() { return; }
        let r = r.clamp(0, w.min(h) / 4);
        if e.shown.as_ref().map_or(true, |s| s.w != w || s.h != h || s.r != r) {
            free(e.shown.take());
            e.shown = scaled(&wic, p, w, h, r).ok();
        }
        let Some(s) = &e.shown else { return };
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: 1 };
        let _ = AlphaBlend(dc, x, y, w, h, s.dc, 0, 0, w, h, blend);
    }

    /// a GDI bitmap (what the clipboard gives for screenshots) written as a PNG file
    pub unsafe fn save_bitmap(&self, hbm: HBITMAP, to: &Path) -> bool {
        let Some(wic) = &self.wic else { return false };
        let r = (|| -> Result<()> {
            let src = wic.CreateBitmapFromHBITMAP(hbm, HPALETTE::default(), WICBitmapIgnoreAlpha)?;
            let (mut w, mut h) = (0u32, 0u32);
            src.GetSize(&mut w, &mut h)?;
            let stream = wic.CreateStream()?;
            let name = wide(to);
            stream.InitializeFromFilename(PCWSTR(name.as_ptr()), GENERIC_WRITE.0)?;
            let enc = wic.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
            enc.Initialize(&stream, WICBitmapEncoderNoCache)?;
            let mut fr = None;
            let mut props = None;
            enc.CreateNewFrame(&mut fr, &mut props)?;
            let fr = fr.ok_or(Error::from(E_FAIL))?;
            fr.Initialize(props.as_ref())?;
            fr.SetSize(w, h)?;
            let mut fmt = GUID_WICPixelFormat24bppBGR;
            fr.SetPixelFormat(&mut fmt)?;
            fr.WriteSource(&src, std::ptr::null())?;
            fr.Commit()?;
            enc.Commit()?;
            Ok(())
        })();
        if r.is_err() { let _ = std::fs::remove_file(to); }
        r.is_ok()
    }
}

impl Drop for Images {
    fn drop(&mut self) { for (_, e) in self.cache.drain() { free(e.shown); } }
}

fn free(s: Option<Shown>) {
    if let Some(s) = s { unsafe { let _ = DeleteDC(s.dc); let _ = DeleteObject(s.bmp); } }
}

unsafe fn frame(wic: &IWICImagingFactory, p: &Path) -> Result<IWICBitmapFrameDecode> {
    let name = wide(p);
    let dec = wic.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
    dec.GetFrame(0)
}

unsafe fn scaled(wic: &IWICImagingFactory, p: &Path, w: i32, h: i32, r: i32) -> Result<Shown> {
    let f = frame(wic, p)?;
    let src: IWICBitmapSource = match reduced(wic, &f, w, h) { Some(r) => r, None => f.cast()? };
    let sc = wic.CreateBitmapScaler()?;
    sc.Initialize(&src, w as u32, h as u32, WICBitmapInterpolationModeHighQualityCubic)?;
    let conv = wic.CreateFormatConverter()?;
    conv.Initialize(&sc, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
    let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
    let dc = CreateCompatibleDC(None);
    let mut bits = std::ptr::null_mut();
    let bmp = match CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(b) if !bits.is_null() => b,
        _ => { let _ = DeleteDC(dc); return Err(Error::from(E_FAIL)); }
    };
    let stride = w as u32 * 4;
    let buf = std::slice::from_raw_parts_mut(bits as *mut u8, (stride * h as u32) as usize);
    if let Err(e) = conv.CopyPixels(std::ptr::null(), stride, buf) { let _ = DeleteDC(dc); let _ = DeleteObject(bmp); return Err(e); }
    round_corners(buf, w, h, r);
    SelectObject(dc, bmp);
    Ok(Shown { w, h, r, dc, bmp })
}

/// A JPEG decodes straight to 1/2, 1/4 or 1/8 of its size for a fraction of the work. The smallest
/// such size still no smaller than the box, or None when the decoder cannot or need not.
unsafe fn reduced(wic: &IWICImagingFactory, f: &IWICBitmapFrameDecode, w: i32, h: i32) -> Option<IWICBitmapSource> {
    let t: IWICBitmapSourceTransform = f.cast().ok()?;
    let (mut fw, mut fh) = (0u32, 0u32);
    f.GetSize(&mut fw, &mut fh).ok()?;
    let (w, h) = (w as u32, h as u32);
    let k = [8u32, 4, 2].into_iter().find(|k| fw / k >= w && fh / k >= h)?;
    let (mut rw, mut rh) = (fw / k, fh / k);
    t.GetClosestSize(&mut rw, &mut rh).ok()?;
    if rw < w || rh < h || rw >= fw { return None; }
    let mut fmt = GUID_WICPixelFormat24bppBGR;
    t.GetClosestPixelFormat(&mut fmt).ok()?;
    let bpp = wic.CreateComponentInfo(&fmt).ok()?.cast::<IWICPixelFormatInfo>().ok()?.GetBitsPerPixel().ok()?;
    let stride = (rw * bpp + 31) / 32 * 4;
    let mut buf = vec![0u8; (stride * rh) as usize];
    t.CopyPixels(std::ptr::null(), rw, rh, &fmt, WICBitmapTransformRotate0, stride, &mut buf).ok()?;
    wic.CreateBitmapFromMemory(rw, rh, &fmt, stride, &buf).ok()?.cast().ok()
}

/// fades the corners of premultiplied BGRA pixels out along a circle of radius `r`, antialiased
fn round_corners(buf: &mut [u8], w: i32, h: i32, r: i32) {
    if r <= 0 { return; }
    let rf = r as f32;
    for cy in 0..r {
        for cx in 0..r {
            // distance from the corner circle's centre to this pixel's centre
            let (dx, dy) = (rf - cx as f32 - 0.5, rf - cy as f32 - 0.5);
            let cover = (rf - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            if cover >= 1.0 { continue; }
            for (x, y) in [(cx, cy), (w - 1 - cx, cy), (cx, h - 1 - cy), (w - 1 - cx, h - 1 - cy)] {
                let i = ((y * w + x) * 4) as usize;
                for c in &mut buf[i..i + 4] { *c = (*c as f32 * cover).round() as u8; }
            }
        }
    }
}
