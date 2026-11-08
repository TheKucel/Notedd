//! Pictures in notes: decoded with WIC, kept as premultiplied DIBs at the size they are shown,
//! and drawn under the image link line of the editor. Pasted bitmaps are saved as PNG files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

struct Shown { w: i32, h: i32, dc: HDC, bmp: HBITMAP }
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

    /// draws the picture into the box (device px), blending it with what is already there
    pub unsafe fn draw(&mut self, dc: HDC, p: &Path, x: i32, y: i32, w: i32, h: i32) {
        if w <= 0 || h <= 0 { return; }
        let Some(wic) = self.wic.clone() else { return };
        let e = self.entry(p);
        if e.size.is_none() { return; }
        if e.shown.as_ref().map_or(true, |s| s.w != w || s.h != h) {
            free(e.shown.take());
            e.shown = scaled(&wic, p, w, h).ok();
        }
        let Some(s) = &e.shown else { return };
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: 1 };
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

unsafe fn scaled(wic: &IWICImagingFactory, p: &Path, w: i32, h: i32) -> Result<Shown> {
    let f = frame(wic, p)?;
    let conv = wic.CreateFormatConverter()?;
    conv.Initialize(&f, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
    let sc = wic.CreateBitmapScaler()?;
    sc.Initialize(&conv, w as u32, h as u32, WICBitmapInterpolationModeHighQualityCubic)?;
    let bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
    let dc = CreateCompatibleDC(None);
    let mut bits = std::ptr::null_mut();
    let bmp = match CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(b) if !bits.is_null() => b,
        _ => { let _ = DeleteDC(dc); return Err(Error::from(E_FAIL)); }
    };
    let stride = w as u32 * 4;
    let buf = std::slice::from_raw_parts_mut(bits as *mut u8, (stride * h as u32) as usize);
    if let Err(e) = sc.CopyPixels(std::ptr::null(), stride, buf) { let _ = DeleteDC(dc); let _ = DeleteObject(bmp); return Err(e); }
    SelectObject(dc, bmp);
    Ok(Shown { w, h, dc, bmp })
}
