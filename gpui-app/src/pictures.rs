//! Pictures of a note: decoded on first use, kept as GPU-ready frames, and
//! imported into the note's `assets` folder like the Windows app does.

use gpui::RenderImage;
use image::Frame;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
/// Pictures wider than this are shrunk once at load (memory, texture size).
const MAX_SIDE: u32 = 2400;

pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

pub struct Loaded {
    pub image: Arc<RenderImage>,
    pub w: f32,
    pub h: f32,
}

pub struct Pictures {
    dir: PathBuf,
    cache: HashMap<String, Option<(std::time::SystemTime, Arc<Loaded>)>>,
}

impl Pictures {
    pub fn new(dir: PathBuf) -> Pictures { Pictures { dir, cache: HashMap::new() } }

    /// A link target as a file in the note's folder.
    pub fn resolve(&self, link: &str) -> Option<PathBuf> {
        if link.contains("://") && !link.starts_with("file://") { return None; }
        let link = link.strip_prefix("file:///").unwrap_or(link).replace("%20", " ");
        let p = PathBuf::from(link);
        let p = if p.is_absolute() { p } else { self.dir.join(p) };
        (is_image(&p) && p.is_file()).then_some(p)
    }

    /// The decoded picture, reloaded when the file changed.
    pub fn get(&mut self, link: &str) -> Option<Arc<Loaded>> {
        let path = self.resolve(link)?;
        let stamp = std::fs::metadata(&path).and_then(|m| m.modified()).ok()?;
        if let Some(Some((t, l))) = self.cache.get(link) {
            if *t == stamp { return Some(l.clone()); }
        }
        let loaded = decode(&path).map(|l| (stamp, Arc::new(l)));
        self.cache.insert(link.to_string(), loaded.clone());
        loaded.map(|(_, l)| l)
    }

    fn assets(&self) -> PathBuf { self.dir.join("assets") }

    /// A free file name in the assets folder: name.ext, name-2.ext, ...
    fn free_asset(&self, stem: &str, ext: &str) -> PathBuf {
        let d = self.assets();
        let _ = std::fs::create_dir_all(&d);
        let clean: String = stem.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect();
        let clean = if clean.is_empty() { "image".to_string() } else { clean };
        (1..).map(|k| d.join(if k == 1 { format!("{clean}.{ext}") } else { format!("{clean}-{k}.{ext}") })).find(|p| !p.exists()).unwrap()
    }

    fn link(p: &Path) -> String { format!("assets/{}", p.file_name().unwrap_or_default().to_string_lossy()) }

    /// An image file from outside: copied into assets (unless it is there already).
    pub fn import_file(&self, src: &Path) -> Option<String> {
        if !is_image(src) || !src.is_file() { return None; }
        if src.parent() == Some(self.assets().as_path()) { return Some(Self::link(src)); }
        let stem = src.file_stem()?.to_string_lossy().to_string();
        let ext = src.extension()?.to_string_lossy().to_ascii_lowercase();
        let to = self.free_asset(&stem, &ext);
        std::fs::copy(src, &to).ok()?;
        Some(Self::link(&to))
    }

    /// A picture that exists only as bytes (clipboard screenshot).
    pub fn import_bytes(&self, stem: &str, ext: &str, bytes: &[u8]) -> Option<String> {
        image::load_from_memory(bytes).ok()?;
        let to = self.free_asset(stem, ext);
        std::fs::write(&to, bytes).ok()?;
        Some(Self::link(&to))
    }
}

fn decode(path: &Path) -> Option<Loaded> {
    let mut img = image::open(path).ok()?;
    if img.width().max(img.height()) > MAX_SIDE {
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let (w, h) = (img.width() as f32, img.height() as f32);
    let mut data = img.into_rgba8();
    // GPUI wants BGRA.
    for px in data.chunks_exact_mut(4) { px.swap(0, 2); }
    Some(Loaded { image: Arc::new(RenderImage::new(smallvec::SmallVec::from_elem(Frame::new(data), 1))), w, h })
}
