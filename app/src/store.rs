//! Windows note-folder preferences and Recycle Bin integration.

pub use notedd_core::store::*;

use std::path::{Path, PathBuf};

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
