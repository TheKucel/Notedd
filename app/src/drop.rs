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

thread_local! {
    static PENDING: RefCell<Option<Dropped>> = const { RefCell::new(None) };
    /// when and what the last drop was: RichEdit can ask for the same drop more than once
    static LAST: RefCell<Option<(u64, Vec<String>)>> = const { RefCell::new(None) };
}

fn signature(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| match i { Item::File(p) => p.display().to_string(), Item::Data(n, b) => format!("{n}:{}", b.len()) }).collect()
}

/// true if this drop repeats the previous one within a moment
fn repeated(items: &[Item]) -> bool {
    let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount64() };
    let sig = signature(items);
    LAST.with(|l| {
        let mut l = l.borrow_mut();
        let again = l.as_ref().is_some_and(|(t, s)| now.saturating_sub(*t) < 1500 && *s == sig);
        *l = Some((now, sig));
        again
    })
}

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
                if !items.is_empty() && !repeated(&items) {
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

/// Drops a fake Explorer drag (CF_HDROP) on a real RichEdit through the drop target RichEdit registered
/// itself, then counts what reaches the app: one drop must be one message and one set of files.
#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::LibraryLoader::LoadLibraryW;

    const MSG: u32 = WM_APP + 5;

    #[implement(IDataObject)]
    struct Files { paths: Vec<PathBuf> }

    impl IDataObject_Impl for Files_Impl {
        fn GetData(&self, f: *const FORMATETC) -> Result<STGMEDIUM> {
            let f = unsafe { &*f };
            if f.cfFormat == CF_TEXT_W {
                let mut w: Vec<u16> = self.paths[0].as_os_str().to_string_lossy().encode_utf16().collect();
                w.push(0);
                unsafe {
                    let g = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2)?;
                    std::ptr::copy_nonoverlapping(w.as_ptr(), GlobalLock(g) as *mut u16, w.len());
                    let _ = GlobalUnlock(g);
                    return Ok(STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: g }, pUnkForRelease: std::mem::ManuallyDrop::new(None) });
                }
            }
            if f.cfFormat != CF_FILES { return Err(DV_E_FORMATETC.into()); }
            let mut wide: Vec<u16> = Vec::new();
            for p in &self.paths { wide.extend(p.as_os_str().to_string_lossy().encode_utf16()); wide.push(0); }
            wide.push(0);
            let head = std::mem::size_of::<DROPFILES>();
            unsafe {
                let g = GlobalAlloc(GMEM_MOVEABLE, head + wide.len() * 2)?;
                let p = GlobalLock(g) as *mut u8;
                std::ptr::write_unaligned(p as *mut DROPFILES, DROPFILES { pFiles: head as u32, pt: POINT::default(), fNC: BOOL(0), fWide: BOOL(1) });
                std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, p.add(head), wide.len() * 2);
                let _ = GlobalUnlock(g);
                Ok(STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: g }, pUnkForRelease: std::mem::ManuallyDrop::new(None) })
            }
        }
        fn GetDataHere(&self, _f: *const FORMATETC, _m: *mut STGMEDIUM) -> Result<()> { Err(E_NOTIMPL.into()) }
        fn QueryGetData(&self, f: *const FORMATETC) -> HRESULT {
            let f = unsafe { &*f };
            if (f.cfFormat == CF_FILES || f.cfFormat == CF_TEXT_W) && f.tymed & TYMED_HGLOBAL.0 as u32 != 0 { S_OK } else { DV_E_FORMATETC }
        }
        fn GetCanonicalFormatEtc(&self, _i: *const FORMATETC, _o: *mut FORMATETC) -> HRESULT { E_NOTIMPL }
        fn SetData(&self, _f: *const FORMATETC, _m: *const STGMEDIUM, _r: BOOL) -> Result<()> { Err(E_NOTIMPL.into()) }
        fn EnumFormatEtc(&self, d: u32) -> Result<IEnumFORMATETC> {
            if d != DATADIR_GET.0 as u32 { return Err(E_NOTIMPL.into()); }
            unsafe { SHCreateStdEnumFmtEtc(&[format(CF_FILES, TYMED_HGLOBAL, -1), format(CF_TEXT_W, TYMED_HGLOBAL, -1)]) }
        }
        fn DAdvise(&self, _f: *const FORMATETC, _a: u32, _s: Option<&IAdviseSink>) -> Result<u32> { Err(OLE_E_ADVISENOTSUPPORTED.into()) }
        fn DUnadvise(&self, _c: u32) -> Result<()> { Err(OLE_E_ADVISENOTSUPPORTED.into()) }
        fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> { Err(OLE_E_ADVISENOTSUPPORTED.into()) }
    }

    /// parent + RichEdit with our callback, and the OLE drop target RichEdit registered on itself
    unsafe fn rig() -> (HWND, IDropTarget) {
        OleInitialize(None).unwrap();
        LoadLibraryW(w!("Msftedit.dll")).unwrap();
        let parent = CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), PCWSTR::null(), WS_POPUP | WS_VISIBLE, 0, 0, 400, 300, None, None, None, None).unwrap();
        let edit = CreateWindowExW(WINDOW_EX_STYLE(0), w!("RICHEDIT50W"), PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL) as u32), 0, 0, 400, 300, parent, None, None, None).unwrap();
        EDIT.with(|e| e.set(edit));
        let cb = Callback::new(parent, MSG);
        SendMessageW(edit, EM_SETOLECALLBACK, WPARAM(0), LPARAM(cb.as_raw() as isize));
        std::mem::forget(cb);
        let raw = GetPropW(edit, w!("OleDropTargetInterface"));
        assert!(!raw.is_invalid(), "RichEdit registered no OLE drop target");
        let target = IDropTarget::from_raw_borrowed(&raw.0).unwrap().clone();
        (parent, target)
    }

    /// messages posted to `parent` so far
    unsafe fn posted(parent: HWND) -> usize {
        let mut m = WinMsg::default();
        let mut n = 0;
        while PeekMessageW(&mut m, parent, MSG, MSG, PM_REMOVE).as_bool() { n += 1; }
        n
    }
    type WinMsg = windows::Win32::UI::WindowsAndMessaging::MSG;

    fn picture() -> PathBuf {
        let p = std::env::temp_dir().join(format!("notedd-drop-test-{}.png", std::process::id()));
        std::fs::write(&p, b"\x89PNG\r\n\x1a\n").unwrap();
        p
    }

    thread_local! { static EDIT: std::cell::Cell<HWND> = std::cell::Cell::new(HWND::default()); }
    fn find_edit() -> HWND { EDIT.with(|e| e.get()) }

    unsafe fn drop_once(target: &IDropTarget, data: &IDataObject) {
        let mut effect = DROPEFFECT(DROPEFFECT_COPY.0 | DROPEFFECT_MOVE.0);
        let mut r = RECT::default();
        let _ = GetWindowRect(find_edit(), &mut r);
        let pt = POINTL { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
        target.DragEnter(data, MODIFIERKEYS_FLAGS(1), pt, &mut effect).unwrap();
        assert_ne!(effect.0, 0, "RichEdit must accept the drag");
        target.DragOver(MODIFIERKEYS_FLAGS(1), pt, &mut effect).unwrap();
        let _ = target.Drop(data, MODIFIERKEYS_FLAGS(1), pt, &mut effect);
    }

    #[test]
    fn one_drop_is_one_message_and_one_set_of_files() {
        unsafe {
            let (parent, target) = rig();
            let path = picture();
            let data: IDataObject = Files { paths: vec![path.clone()] }.into();
            super::PENDING.with(|p| *p.borrow_mut() = None);
            drop_once(&target, &data);
            assert_eq!(posted(parent), 1, "one drop must post exactly one message");
            let got = take().expect("the drop reaches the app");
            assert_eq!(got.items.len(), 1, "one file dropped, one item handed over");
            assert!(take().is_none(), "nothing is left over after the first take");
            let _ = std::fs::remove_file(path);
        }
    }

    #[test]
    fn two_files_stay_two_items() {
        unsafe {
            let (parent, target) = rig();
            let (a, b) = (picture(), picture().with_extension("2.png"));
            std::fs::write(&b, b"\x89PNG\r\n\x1a\n").unwrap();
            let data: IDataObject = Files { paths: vec![a.clone(), b.clone()] }.into();
            super::LAST.with(|l| *l.borrow_mut() = None);
            drop_once(&target, &data);
            assert_eq!(posted(parent), 1);
            assert_eq!(take().unwrap().items.len(), 2);
            let _ = std::fs::remove_file(a);
            let _ = std::fs::remove_file(b);
        }
    }

    #[test]
    fn a_repeated_accept_of_the_same_drop_is_ignored() {
        unsafe {
            let (parent, _target) = rig();
            let path = picture();
            let data: IDataObject = Files { paths: vec![path.clone()] }.into();
            super::LAST.with(|l| *l.borrow_mut() = None);
            let cb: IRichEditOleCallback = Callback::new(parent, MSG);
            let mut cf = 0u16;
            for _ in 0..3 {
                let _ = cb.QueryAcceptData(&data, &mut cf, RECO_DROP, BOOL(1), HGLOBAL::default());
            }
            assert_eq!(posted(parent), 1, "three accepts of one drop post one message");
            let _ = std::fs::remove_file(path);
        }
    }
}
