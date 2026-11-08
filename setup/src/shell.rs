//! The Windows folder picker.

use std::path::{Path, PathBuf};
use windows::core::*;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::*;

/// asks for a folder; None when cancelled
pub unsafe fn pick_folder(owner: HWND, title: &str, start: Option<&Path>) -> Option<PathBuf> {
    let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
    let o = d.GetOptions().ok()?;
    d.SetOptions(o | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).ok()?;
    let t: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let _ = d.SetTitle(PCWSTR(t.as_ptr()));
    if let Some(p) = start.filter(|p| p.exists()) {
        let w: Vec<u16> = p.to_string_lossy().encode_utf16().chain(Some(0)).collect();
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(w.as_ptr()), None) { let _ = d.SetFolder(&item); }
    }
    d.Show(owner).ok()?;
    let r = d.GetResult().ok()?;
    let s = r.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
    let out = s.to_string().ok();
    CoTaskMemFree(Some(s.0 as _));
    out.map(PathBuf::from)
}
