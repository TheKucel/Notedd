//! Premultiplied checkbox stamps for the RichEdit overlay.

use windows::Win32::Graphics::Gdi::*;

pub const CORNER: f32 = 4.0;
pub const OUTLINE_CORNER: f32 = 3.5;
pub const OUTLINE_WIDTH: f32 = 1.5;
pub const TICK_WIDTH: f32 = 1.7;
pub const TICK: [(f32, f32); 3] = [(0.26, 0.52), (0.43, 0.69), (0.75, 0.33)];

pub struct Checkboxes {
    dc: HDC,
    bitmap: HBITMAP,
    size: i32,
    scale: f32,
    colors: [u32; 3],
}

impl Checkboxes {
    pub fn matches(&self, size: i32, scale: f32, colors: [u32; 3]) -> bool {
        self.size == size && self.scale == scale && self.colors == colors
    }

    pub unsafe fn new(size: i32, scale: f32, colors: [u32; 3]) -> Option<Self> {
        if size <= 0 { return None; }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 2 * size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() { let _ = DeleteObject(bitmap); return None; }
        let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (2 * size * size) as usize);
        let width = size as f32;
        let tick = TICK.map(|(x, y)| (x * width, y * width));
        for y in 0..size {
            for x in 0..size {
                let mut outline = [0; 4];
                let mut checked = [0; 4];
                for sy in 0..4 {
                    for sx in 0..4 {
                        let p = (x as f32 + (sx as f32 + 0.5) * 0.25, y as f32 + (sy as f32 + 0.5) * 0.25);
                        let half = OUTLINE_WIDTH * scale * 0.5;
                        if rounded(p, width, 0.0, OUTLINE_CORNER * scale + half)
                            && !rounded(p, width, 2.0 * half, OUTLINE_CORNER * scale - half) {
                            accumulate(&mut outline, colors[0]);
                        }
                        if rounded(p, width, 0.0, CORNER * scale) {
                            let on_tick = tick.windows(2).any(|s| line_distance_sq(p, s[0], s[1]) <= (TICK_WIDTH * scale * 0.5).powi(2));
                            accumulate(&mut checked, colors[if on_tick { 2 } else { 1 }]);
                        }
                    }
                }
                let index = (y * 2 * size + x) as usize;
                pixels[index] = premultiply(outline);
                pixels[index + size as usize] = premultiply(checked);
            }
        }
        SelectObject(dc, bitmap);
        Some(Self { dc, bitmap, size, scale, colors })
    }

    pub unsafe fn draw(&self, dc: HDC, x: i32, y: i32, checked: bool) {
        let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: 1 };
        let s = self.size;
        let _ = AlphaBlend(dc, x, y, s, s, self.dc, if checked { s } else { 0 }, 0, s, s, blend);
    }
}

impl Drop for Checkboxes {
    fn drop(&mut self) {
        unsafe { let _ = DeleteDC(self.dc); let _ = DeleteObject(self.bitmap); }
    }
}

fn rounded((x, y): (f32, f32), size: f32, inset: f32, radius: f32) -> bool {
    let radius = radius.min((size * 0.5 - inset).max(0.0));
    let c = (inset + radius).min(size * 0.5);
    let dx = x - x.clamp(c, size - c);
    let dy = y - y.clamp(c, size - c);
    dx * dx + dy * dy <= radius * radius
}

fn line_distance_sq((x, y): (f32, f32), (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> f32 {
    let (vx, vy) = (bx - ax, by - ay);
    let t = (((x - ax) * vx + (y - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    (x - ax - t * vx).powi(2) + (y - ay - t * vy).powi(2)
}

fn accumulate(out: &mut [u32; 4], color: u32) {
    out[0] += color & 255;
    out[1] += color >> 8 & 255;
    out[2] += color >> 16 & 255;
    out[3] += 255;
}

fn premultiply(channels: [u32; 4]) -> u32 {
    let [b, g, r, a] = channels.map(|sum| (sum + 8) / 16);
    b | (g << 8) | (r << 16) | (a << 24)
}
