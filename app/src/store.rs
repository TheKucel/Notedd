//! Notes on disk: one Markdown file per note in Documents\Pane.
//! The colour lives in a tiny front matter block so the files stay readable in any editor.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// names of the first card colours in the front matter; the ones after them are "c7", "c8", ...
const NAMES: [&str; 6] = ["paper", "sage", "fog", "lav", "rose", "sand"];
fn color_name(i: usize) -> String { NAMES.get(i).map(|s| s.to_string()).unwrap_or_else(|| format!("c{}", i + 1)) }
fn color_index(name: &str) -> Option<usize> {
    NAMES.iter().position(|c| *c == name)
        .or_else(|| name.strip_prefix('c').and_then(|n| n.parse::<usize>().ok()).filter(|n| (7..=10).contains(n)).map(|n| n - 1))
}

#[derive(Clone)]
struct FrontMatter {
    raw: String,
    color: usize,
    value: Option<Range<usize>>,
    insert_at: usize,
}

#[derive(Clone)]
pub struct Note {
    pub id: String,
    pub color: usize,
    pub text: String,
    /// last change, FILETIME units (100 ns since 1601)
    pub mtime: u64,
    /// last time it was opened in Pane, same units; 0 if never in this session
    pub opened: u64,
    pub lower: String,
    front_matter: Option<FrontMatter>,
}

impl Note {
    pub fn new(id: String, color: usize, text: String, mtime: u64) -> Note {
        let lower = text.to_lowercase();
        Note { id, color, text, mtime, opened: 0, lower, front_matter: None }
    }
    pub fn set_text(&mut self, text: String) { self.lower = text.to_lowercase(); self.text = text; }
    pub fn title(&self) -> &str {
        let t = self.text.lines().next().unwrap_or("").trim();
        let h = t.trim_start_matches('#');
        if h.len() < t.len() && (h.is_empty() || h.starts_with(' ')) { h.trim() } else { t }
    }
    pub fn body_lines(&self) -> Vec<&str> { self.text.lines().skip(1).collect() }
    /// body on one line; list items are joined with commas, like the prototype
    pub fn body_flat(&self) -> String {
        let mut out = String::new();
        let mut prev = Kind::Plain;
        for l in self.body_lines() {
            if crate::md::image_path(&l.encode_utf16().collect::<Vec<_>>()).is_some() || l.trim_start().starts_with("```") { continue; }
            let (k, t) = marker(l);
            let t = task(k, t).map_or(t, |x| x.1);
            let t = crate::md::strip(t);
            let t = t.trim();
            if t.is_empty() { continue; }
            if !out.is_empty() { out.push_str(if k != Kind::Plain && prev != Kind::Plain { ", " } else { " " }); }
            out.push_str(t);
            prev = k;
        }
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    pub fn words(&self) -> usize { self.text.lines().map(|l| marker(l).1.split_whitespace().count()).sum() }
    pub fn is_empty(&self) -> bool { self.text.trim().is_empty() && self.front_matter.is_none() }
    pub fn seen(&self) -> u64 { self.opened.max(self.mtime) }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind { Plain, Bullet, Number }

/// Markdown list marker at the start of a line: "- x", "* x", "1. x", "1) x"
pub fn marker(line: &str) -> (Kind, &str) {
    for p in ["- ", "* ", "• "] { if let Some(r) = line.strip_prefix(p) { return (Kind::Bullet, r); } }
    let d = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if (1..4).contains(&d) {
        let rest = &line[d..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) { return (Kind::Number, r); }
    }
    (Kind::Plain, line)
}

/// a task item: "- [ ] x" / "- [x] x" (the bullet already taken off by `marker`)
pub fn task(k: Kind, rest: &str) -> Option<(bool, &str)> {
    if k != Kind::Bullet { return None; }
    for (p, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] { if let Some(r) = rest.strip_prefix(p) { return Some((done, r)); } }
    match rest { "[ ]" => Some((false, "")), "[x]" | "[X]" => Some((true, "")), _ => None }
}

pub fn now_ft() -> u64 { to_ft(SystemTime::now()) }
fn to_ft(t: SystemTime) -> u64 {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    d.as_secs() * 10_000_000 + d.subsec_nanos() as u64 / 100 + 116_444_736_000_000_000
}

/// the chosen notes folder, or Documents\Notedd (Documents\Pane if the notes are still there from before the rename)
pub fn notes_dir() -> PathBuf {
    if let Some(d) = crate::settings::notes_dir_pref() { let _ = std::fs::create_dir_all(&d); return d; }
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath, KNOWN_FOLDER_FLAG};
    let docs = unsafe {
        match SHGetKnownFolderPath(&FOLDERID_Documents, KNOWN_FOLDER_FLAG(0), None) {
            Ok(p) => { let s = p.to_string().unwrap_or_default(); CoTaskMemFree(Some(p.0 as _)); PathBuf::from(s) }
            Err(_) => PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("Documents"),
        }
    };
    let (new, old) = (docs.join("Notedd"), docs.join("Pane"));
    let dir = if !new.exists() && old.exists() { old } else { new };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn front_matter(raw: &str) -> Option<FrontMatter> {
    let mut lines = raw.split_inclusive('\n');
    let opening = lines.next()?;
    if opening.trim_start_matches('\u{feff}').trim_end_matches(['\r', '\n']) != "---" { return None; }
    let mut offset = opening.len();
    let mut legacy = None;
    let mut own = None;
    for line in lines {
        let content = line.trim_end_matches(['\r', '\n']);
        if matches!(content, "---" | "...") {
            let (color, value) = own.or(legacy).map_or((0, None), |(color, range)| (color, Some(range)));
            return Some(FrontMatter {
                raw: raw[..offset + line.len()].to_string(),
                color,
                value,
                insert_at: opening.len(),
            });
        }
        if let Some((key, tail)) = content.split_once(':') {
            if matches!(key, "color" | "# notedd-color") {
                let trimmed = tail.trim_start();
                let value = trimmed.split_whitespace().next().unwrap_or_default();
                let remainder = trimmed[value.len()..].trim_start();
                if remainder.is_empty() || remainder.starts_with('#') {
                    if let Some(color) = color_index(value) {
                        let start = offset + key.len() + 1 + tail.len() - trimmed.len();
                        let entry = Some((color, start..start + value.len()));
                        if key == "# notedd-color" { own = entry; } else { legacy = entry; }
                    }
                }
            }
        }
        offset += line.len();
    }
    None
}

pub fn load(dir: &Path) -> Vec<Note> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("md") { continue; }
        let Ok(raw) = std::fs::read_to_string(&p) else { continue };
        let header = front_matter(&raw);
        let body = &raw[header.as_ref().map_or(0, |h| h.raw.len())..];
        let mtime = e.metadata().and_then(|m| m.modified()).map(to_ft).unwrap_or(0);
        let id = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let mut note = Note::new(id, header.as_ref().map_or(0, |h| h.color), body.replace("\r\n", "\n"), mtime);
        note.front_matter = header;
        out.push(note);
    }
    out
}

pub fn save(dir: &Path, n: &Note) -> io::Result<()> {
    let mut data = String::new();
    if let Some(header) = &n.front_matter {
        data.push_str(&header.raw);
        if n.color != header.color {
            if let Some(value) = &header.value {
                data.replace_range(value.clone(), &color_name(n.color));
            } else {
                let newline = if header.raw[..header.insert_at].ends_with("\r\n") { "\r\n" } else { "\n" };
                // Arbitrary YAML can be a sequence or already have its own color key.
                data.insert_str(header.insert_at, &format!("# notedd-color: {}{newline}", color_name(n.color)));
            }
        }
        if !data.ends_with('\n') && !n.text.is_empty() { data.push('\n'); }
    } else if n.color != 0 {
        data.push_str(&format!("---\ncolor: {}\n---\n", color_name(n.color)));
    }
    data.push_str(&n.text);
    let path = dir.join(format!("{}.md", n.id));
    if let Ok(existing) = fs::read_to_string(&path) {
        if existing == data || existing.replace("\r\n", "\n") == data.replace("\r\n", "\n") {
            return Ok(());
        }
    }
    replace_file(&path, data.as_bytes())
}

pub(crate) fn replace_file(path: &Path, data: &[u8]) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (temp, mut file) = loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let temp = path.with_file_name(format!(".notedd-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let written = file.write_all(data).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&temp, path));
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result
}

/// Copy the complete collection before switching the preference or removing any originals.
pub fn move_notes(from: &Path, to: &Path, commit: impl FnOnce() -> io::Result<()>) -> io::Result<bool> {
    fs::create_dir_all(to)?;
    let from = fs::canonicalize(from)?;
    let to = fs::canonicalize(to)?;
    if from == to { return Ok(false); }
    if from.starts_with(&to) || to.starts_with(&from) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "The notes folders overlap"));
    }

    let mut copied = Vec::new();
    let mut directories = Vec::new();
    let result = (|| {
        for entry in fs::read_dir(&from)? {
            let entry = entry?;
            let name = entry.file_name();
            let kind = entry.file_type()?;
            let note = !kind.is_dir() && Path::new(&name).extension().is_some_and(|e| e == "md");
            let assets = name == "assets" && !kind.is_file();
            if note || assets {
                copy_for_move(&from, &to, Path::new(&name), &mut copied, &mut directories)?;
            }
        }
        commit()
    })();
    if let Err(error) = result {
        for path in copied.iter().rev() { let _ = fs::remove_file(to.join(path)); }
        for (path, created) in directories.iter().rev() {
            if *created { let _ = fs::remove_dir(to.join(path)); }
        }
        return Err(error);
    }

    // A cleanup failure leaves a duplicate in the old folder.
    for path in copied { let _ = fs::remove_file(from.join(path)); }
    for (path, _) in directories.into_iter().rev() { let _ = fs::remove_dir(from.join(path)); }
    Ok(true)
}

fn copy_for_move(
    from: &Path,
    to: &Path,
    relative: &Path,
    copied: &mut Vec<PathBuf>,
    directories: &mut Vec<(PathBuf, bool)>,
) -> io::Result<()> {
    let source = from.join(relative);
    let target = to.join(relative);
    let kind = fs::symlink_metadata(&source)?.file_type();
    if kind.is_dir() && !kind.is_symlink() && fs::canonicalize(&source)? == source {
        let created = match fs::create_dir(&target) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && fs::canonicalize(&target)? == target && target.is_dir() => false,
            Err(e) => return Err(e),
        };
        directories.push((relative.to_owned(), created));
        for entry in fs::read_dir(source)? {
            copy_for_move(from, to, &relative.join(entry?.file_name()), copied, directories)?;
        }
    } else if kind.is_file() {
        let mut input = File::open(source)?;
        let mut output = OpenOptions::new().write(true).create_new(true).open(target)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", relative.display())))?;
        copied.push(relative.to_owned());
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
    } else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, relative.display().to_string()));
    }
    Ok(())
}

pub fn delete(dir: &Path, n: &Note) { let _ = std::fs::remove_file(dir.join(format!("{}.md", n.id))); }

/// a note the user deletes goes to the Recycle Bin, so it can still be brought back from there;
/// where there is no Recycle Bin (some network folders) the file is removed
pub fn trash(dir: &Path, n: &Note) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{SHFileOperationW, SHFILEOPSTRUCTW, FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT};
    let path = dir.join(format!("{}.md", n.id));
    if !path.exists() { return; }
    // pFrom is a list: each name ends with a zero, the list with one more
    let from: Vec<u16> = path.to_string_lossy().encode_utf16().chain([0, 0]).collect();
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0 | FOF_SILENT.0) as u16,
        ..Default::default()
    };
    let ok = unsafe { SHFileOperationW(&mut op) } == 0 && !op.fAnyOperationsAborted.as_bool();
    if !ok || path.exists() { let _ = std::fs::remove_file(&path); }
}

pub fn new_id() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("note-{}", d.as_millis())
}

pub fn read_state(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(".pane-state")).ok()?.lines()
        .find_map(|l| l.strip_prefix("current=").map(|s| s.trim().to_string()))
}
pub fn write_state(dir: &Path, current: &str) { let _ = std::fs::write(dir.join(".pane-state"), format!("current={current}\n")); }

/// the editor size the user left it at, DIPs
pub fn read_size(dir: &Path) -> Option<(f32, f32)> {
    let s = std::fs::read_to_string(dir.join(".pane-size")).ok()?;
    let mut it = s.split_whitespace().filter_map(|v| v.parse::<f32>().ok());
    Some((it.next()?, it.next()?))
}
pub fn write_size(dir: &Path, w: f32, h: f32) { let _ = std::fs::write(dir.join(".pane-size"), format!("{} {}\n", w.round(), h.round())); }

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join(format!(
                "notedd-store-{}-{}-{}", std::process::id(), now_ft(), NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(dir.parent().unwrap()).unwrap();
            std::fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn saving_markdown_preserves_front_matter() {
        let dir = TestDir::new();
        let path = dir.0.join("note.md");
        let header = "---\r\ntags: [work, ideas]\r\ncolor: sage # card\r\nsummary: |\r\n  Keep these lines.\r\n---\r\n";
        let original = format!("{header}Title\r\nBody\r\n");
        std::fs::write(&path, &original).unwrap();
        let mut note = load(&dir.0).pop().unwrap();
        assert_eq!(note.title(), "Title");
        save(&dir.0, &note).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        note.set_text("Title\nEdited body\n".into());
        save(&dir.0, &note).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("{header}Title\nEdited body\n"));
        note.color = 2;
        save(&dir.0, &note).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().starts_with(&header.replace("sage", "fog")));
    }

    #[test]
    fn changing_card_color_preserves_a_custom_yaml_color() {
        let dir = TestDir::new();
        let path = dir.0.join("note.md");
        let original = "---\ncolor: '#c0ffee'\nowner: 'Notedd user'\n---\nTitle";
        fs::write(&path, original).unwrap();
        let mut note = load(&dir.0).pop().unwrap();
        note.color = 2;
        save(&dir.0, &note).unwrap();
        let saved = fs::read_to_string(path).unwrap();
        assert_eq!(saved.replace("# notedd-color: fog\n", ""), original);
        assert_eq!(load(&dir.0)[0].color, 2);
    }

    #[test]
    fn move_conflict_keeps_both_collections_intact() {
        let dir = TestDir::new();
        let from = dir.0.join("old");
        let to = dir.0.join("new");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        fs::write(from.join("a.md"), "another note").unwrap();
        fs::write(from.join("b.md"), "original").unwrap();
        fs::write(to.join("b.md"), "independent note").unwrap();
        let err = move_notes(&from, &to, || panic!("a conflict must prevent switching folders")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(from.join("b.md")).unwrap(), "original");
        assert_eq!(fs::read_to_string(to.join("b.md")).unwrap(), "independent note");
        assert!(from.join("a.md").exists());
        assert!(!to.join("a.md").exists());
    }

    #[test]
    fn choosing_the_same_folder_by_an_alias_does_not_move_anything() {
        let dir = TestDir::new();
        let alias = dir.0.join(".");
        let note = dir.0.join("note.md");
        fs::write(&note, "Original").unwrap();
        assert!(!move_notes(&dir.0, &alias, || panic!("same folder should be a no-op")).unwrap());
        assert_eq!(fs::read_to_string(note).unwrap(), "Original");
    }

    #[test]
    fn move_copies_images_before_committing_and_removing_originals() {
        let dir = TestDir::new();
        let from = dir.0.join("old");
        let to = dir.0.join("new");
        let note = Path::new("note.md");
        let image = Path::new("assets").join("sub").join("img.png");
        fs::create_dir_all(from.join(image.parent().unwrap())).unwrap();
        fs::write(from.join(note), "Title\n![](assets/sub/img.png)").unwrap();
        fs::write(from.join(&image), [1, 2, 3]).unwrap();
        let simulated_error = || Err(io::Error::other("settings write failed"));
        assert!(move_notes(&from, &to, simulated_error).is_err());
        assert!(from.join(note).exists() && from.join(&image).exists());
        assert!(!to.join(note).exists() && !to.join(&image).exists());

        move_notes(&from, &to, || {
            assert!(from.join(note).exists() && from.join(&image).exists());
            assert_eq!(fs::read(to.join(&image)).unwrap(), [1, 2, 3]);
            Ok(())
        }).unwrap();
        assert!(!from.join(note).exists() && !from.join(&image).exists());
        assert!(to.join(note).exists());
    }
}
