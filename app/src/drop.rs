//! Drag and drop into the note. RichEdit registers its own OLE drop target, so WM_DROPFILES never
//! arrives for a real drag: files from Explorer and pictures from a browser reach RichEdit, which
//! inserts them its own way (a file name, a link, an object). Its OLE callback sees every drop
//! first: files and pictures are handed to the app, and anything else comes in as plain text.

use std::cell::RefCell;
use std::path::PathBuf;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::StructuredStorage::IStorage;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MODIFIERKEYS_FLAGS, RECO_DROP, RECO_FLAGS};
use windows::Win32::UI::Controls::RichEdit::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const CF_TEXT_W: u16 = 13;
const CF_FILES: u16 = 15;

pub enum Item {
    /// a file on disk (Explorer, the desktop)
    File(PathBuf),
    /// a file that exists only in the drag (a picture dragged out of a browser): its name and bytes
    Data(String, Vec<u8>),
}

/// what was dropped and where (screen coordinates)
pub struct Dropped { pub items: Vec<Item>, pub at: POINT }

thread_local! { static PENDING: RefCell<Option<Dropped>> = const { RefCell::new(None) }; }

/// the drop the callback posted, once
pub fn take() -> Option<Dropped> { PENDING.with(|p| p.borrow_mut().take()) }

#[implement(IRichEditOleCallback)]
pub struct Callback { hwnd: HWND, msg: u32, files: std::cell::Cell<bool> }

impl Callback {
    /// `msg` is posted to `hwnd` after a drop of files or pictures; `take` then gives them
    pub fn new(hwnd: HWND, msg: u32) -> IRichEditOleCallback { Callback { hwnd, msg, files: std::cell::Cell::new(false) }.into() }
}

fn format(cf: u16, tymed: TYMED, index: i32) -> FORMATETC {
    FORMATETC { cfFormat: cf, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: index, tymed: tymed.0 as u32 }
}
fn has(o: &IDataObject, cf: u16, tymed: TYMED) -> bool { let hr = unsafe { o.QueryGetData(&format(cf, tymed, -1)) }; hr == S_OK }
fn descriptor_format() -> u16 { unsafe { RegisterClipboardFormatW(w!("FileGroupDescriptorW")) as u16 } }
fn contents_format() -> u16 { unsafe { RegisterClipboardFormatW(w!("FileContents")) as u16 } }

/// bytes of an HGLOBAL medium
unsafe fn global_bytes(g: HGLOBAL) -> Vec<u8> {
    let p = GlobalLock(g) as *const u8;
    if p.is_null() { return vec![]; }
    let v = std::slice::from_raw_parts(p, GlobalSize(g)).to_vec();
    let _ = GlobalUnlock(g);
    v
}

unsafe fn files(o: &IDataObject) -> Vec<Item> {
    let Ok(mut m) = o.GetData(&format(CF_FILES, TYMED_HGLOBAL, -1)) else { return vec![] };
    let drop = HDROP(m.u.hGlobal.0);
    let mut out = Vec::new();
    for i in 0..DragQueryFileW(drop, u32::MAX, None) {
        let n = DragQueryFileW(drop, i, None) as usize;
        let mut b = vec![0u16; n + 1];
        DragQueryFileW(drop, i, Some(&mut b));
        out.push(Item::File(PathBuf::from(String::from_utf16_lossy(&b[..n]))));
    }
    ReleaseStgMedium(&mut m);
    out
}

/// virtual files: names from FileGroupDescriptorW, bytes from FileContents (memory or a stream)
unsafe fn virtual_files(o: &IDataObject) -> Vec<Item> {
    let Ok(mut m) = o.GetData(&format(descriptor_format(), TYMED_HGLOBAL, -1)) else { return vec![] };
    let raw = global_bytes(m.u.hGlobal);
    ReleaseStgMedium(&mut m);
    let head = std::mem::size_of::<u32>();
    let one = std::mem::size_of::<FILEDESCRIPTORW>();
    if raw.len() < head { return vec![]; }
    let count = u32::from_ne_bytes(raw[..4].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    for i in 0..count {
        let at = head + i * one;
        if raw.len() < at + one { break; }
        let d: FILEDESCRIPTORW = std::ptr::read_unaligned(raw[at..].as_ptr() as *const FILEDESCRIPTORW);
        let file_name = d.cFileName;
        let n = file_name.iter().position(|&c| c == 0).unwrap_or(file_name.len());
        let name = String::from_utf16_lossy(&file_name[..n]);
        let fmt = format(contents_format(), TYMED(TYMED_HGLOBAL.0 | TYMED_ISTREAM.0), i as i32);
        let Ok(mut m) = o.GetData(&fmt) else { continue };
        let mut bytes = if m.tymed == TYMED_HGLOBAL.0 as u32 {
            global_bytes(m.u.hGlobal)
        } else if m.tymed == TYMED_ISTREAM.0 as u32 {
            let mut v = Vec::new();
            if let Some(s) = (*m.u.pstm).as_ref() {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let mut got = 0u32;
                    let hr = s.Read(buf.as_mut_ptr() as _, buf.len() as u32, Some(&mut got));
                    if hr.is_err() || got == 0 { break; }
                    v.extend_from_slice(&buf[..got as usize]);
                }
            }
            v
        } else { vec![] };
        ReleaseStgMedium(&mut m);
        if d.dwFlags & FD_FILESIZE.0 as u32 != 0 {
            let size = ((d.nFileSizeHigh as u64) << 32 | d.nFileSizeLow as u64) as usize;
            if size <= bytes.len() { bytes.truncate(size); }
        }
        if !bytes.is_empty() { out.push(Item::Data(name, bytes)); }
    }
    out
}

impl IRichEditOleCallback_Impl for Callback_Impl {
    fn QueryAcceptData(&self, data: Option<&IDataObject>, cf: *mut u16, reco: RECO_FLAGS, really: BOOL, _pict: HGLOBAL) -> Result<()> {
        let Some(o) = data else { return Ok(()) };
        self.files.set(false);
        if reco == RECO_DROP {
            let real_files = has(o, CF_FILES, TYMED_HGLOBAL);
            if real_files || has(o, descriptor_format(), TYMED_HGLOBAL) {
                self.files.set(true);
                // while dragging: say yes, so the cursor shows a drop is possible
                if !really.as_bool() { return Ok(()); }
                let items = unsafe { if real_files { files(o) } else { virtual_files(o) } };
                if !items.is_empty() {
                    let mut at = POINT::default();
                    unsafe { let _ = GetCursorPos(&mut at); }
                    PENDING.with(|p| *p.borrow_mut() = Some(Dropped { items, at }));
                    // the note changes after the drag is over, outside RichEdit's own drop
                    unsafe { let _ = PostMessageW(self.hwnd, self.msg, WPARAM(0), LPARAM(0)); }
                }
                // RichEdit inserts nothing itself
                return Err(E_ABORT.into());
            }
        }
        // notes are Markdown: text comes in plain, never as rich text or an object
        if !cf.is_null() && unsafe { *cf } == 0 && has(o, CF_TEXT_W, TYMED_HGLOBAL) { unsafe { *cf = CF_TEXT_W; } }
        Ok(())
    }
    fn GetNewStorage(&self) -> Result<IStorage> { Err(E_NOTIMPL.into()) }
    fn GetInPlaceContext(&self, _f: *mut Option<IOleInPlaceFrame>, _d: *mut Option<IOleInPlaceUIWindow>, _i: *mut OLEINPLACEFRAMEINFO) -> Result<()> { Err(E_NOTIMPL.into()) }
    fn ShowContainerUI(&self, _show: BOOL) -> Result<()> { Ok(()) }
    /// no embedded objects in a Markdown note
    fn QueryInsertObject(&self, _clsid: *mut GUID, _stg: Option<&IStorage>, _cp: i32) -> Result<()> { Err(E_FAIL.into()) }
    fn DeleteObject(&self, _obj: Option<&IOleObject>) -> Result<()> { Ok(()) }
    fn ContextSensitiveHelp(&self, _enter: BOOL) -> Result<()> { Ok(()) }
    fn GetClipboardData(&self, _r: *mut CHARRANGE, _reco: u32, _o: *mut Option<IDataObject>) -> Result<()> { Err(E_NOTIMPL.into()) }
    /// files are always copied in: a move would let Explorer delete the original
    fn GetDragDropEffect(&self, drag: BOOL, _keys: MODIFIERKEYS_FLAGS, effect: *mut DROPEFFECT) -> Result<()> {
        if !drag.as_bool() && self.files.get() && !effect.is_null() {
            unsafe { *effect = if (*effect).0 == 0 { DROPEFFECT_NONE } else { DROPEFFECT_COPY }; }
        }
        Ok(())
    }
    fn GetContextMenu(&self, _t: RICH_EDIT_GET_CONTEXT_MENU_SEL_TYPE, _o: Option<&IOleObject>, _r: *mut CHARRANGE, menu: *mut HMENU) -> Result<()> {
        if !menu.is_null() { unsafe { *menu = HMENU::default(); } }
        Err(E_NOTIMPL.into())
    }
}
