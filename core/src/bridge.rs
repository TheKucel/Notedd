//! The Swift adapter borrows UTF-8 buffers and copies the results synchronously.

use crate::{md, store};
use std::ffi::{c_char, c_void, CStr, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::{ptr, slice, str};

#[repr(C)]
#[derive(Clone, Copy)]
struct Bytes {
    data: *const u8,
    length: usize,
}

impl Bytes {
    fn of(value: &str) -> Self {
        Self { data: value.as_ptr(), length: value.len() }
    }

    fn none() -> Self {
        Self { data: ptr::null(), length: 0 }
    }

    // These borrows are confined to a single bridge call.
    unsafe fn bytes<'a>(self) -> Option<&'a [u8]> {
        if self.length == 0 { return Some(&[]); }
        if self.data.is_null() || self.length > isize::MAX as usize { return None; }
        Some(slice::from_raw_parts(self.data, self.length))
    }

    unsafe fn text<'a>(self) -> Option<&'a str> {
        str::from_utf8(self.bytes()?).ok()
    }
}

type TextCallback = extern "C" fn(*mut c_void, Bytes);
type NoteCallback = extern "C" fn(*mut c_void, Bytes, usize);

unsafe fn path<'a>(value: *const c_char) -> Option<&'a Path> {
    if value.is_null() { return None; }
    Some(Path::new(OsStr::from_bytes(CStr::from_ptr(value).to_bytes())))
}

fn io_status(error: io::Error) -> i32 {
    match error.kind() {
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => 1,
        io::ErrorKind::NotFound => 2,
        io::ErrorKind::PermissionDenied => 3,
        io::ErrorKind::AlreadyExists => 4,
        _ => 5,
    }
}

#[no_mangle]
unsafe extern "C" fn notedd_decode_note(raw: Bytes, context: *mut c_void, callback: NoteCallback) -> bool {
    let Some(raw) = raw.text() else { return false };
    let note = store::Note::from_markdown(String::new(), raw, 0);
    callback(context, Bytes::of(&note.text), note.color);
    true
}

#[no_mangle]
unsafe extern "C" fn notedd_note_title(text: Bytes, context: *mut c_void, callback: TextCallback) {
    if let Some(text) = text.text() {
        callback(context, Bytes::of(store::note_title(text)));
    }
}

#[no_mangle]
unsafe extern "C" fn notedd_save_note(
    filename: *const c_char,
    original: Bytes,
    has_original: bool,
    text: Bytes,
    color: usize,
    context: *mut c_void,
    callback: TextCallback,
) -> i32 {
    let (Some(path), Some(text)) = (path(filename), text.text()) else { return 1 };
    if color >= 10 { return 1; }
    let original = if has_original {
        let Some(raw) = original.text() else { return 1 };
        Some(raw)
    } else {
        None
    };
    match store::save_if_current(path, original, text, color) {
        Ok(Some(data)) => { callback(context, Bytes::of(&data)); 0 }
        Ok(None) => 0,
        Err(error) => io_status(error),
    }
}

#[no_mangle]
unsafe extern "C" fn notedd_write_new(filename: *const c_char, data: Bytes) -> i32 {
    let (Some(path), Some(data)) = (path(filename), data.bytes()) else { return 1 };
    store::write_new(path, data).map_or_else(io_status, |()| 0)
}

#[no_mangle]
unsafe extern "C" fn notedd_read_state(folder: *const c_char, context: *mut c_void, callback: TextCallback) {
    if let Some(id) = path(folder).and_then(store::read_state) {
        callback(context, Bytes::of(&id));
    }
}

#[no_mangle]
unsafe extern "C" fn notedd_write_state(folder: *const c_char, id: Bytes) {
    if let (Some(folder), Some(id)) = (path(folder), id.text()) {
        store::write_state(folder, id);
    }
}

#[repr(C)]
struct Line {
    block: u8,
    heading_level: u8,
    opening_fence: bool,
    block_marker_length: usize,
    image_path: Bytes,
    list: u8,
    list_start: usize,
    list_end: usize,
    checked: bool,
    task_offset: usize,
}

#[repr(C)]
struct Span {
    start: usize,
    end: usize,
    style: u8,
    hidden: bool,
}

#[repr(C)]
struct Link {
    start: usize,
    end: usize,
    target: Bytes,
}

type LineCallback = extern "C" fn(*mut c_void, Line, *const Span, usize, *const Link, usize);

#[no_mangle]
unsafe extern "C" fn notedd_markdown(text: Bytes, context: *mut c_void, callback: LineCallback) {
    let Some(text) = text.text() else { return };
    let lines: Vec<_> = text.split('\n').collect();
    let utf16: Vec<Vec<_>> = lines.iter().map(|line| line.encode_utf16().collect()).collect();
    let blocks = md::blocks(&utf16.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let mut fenced = false;
    for ((line, wide), block) in lines.into_iter().zip(utf16).zip(blocks) {
        let (kind, heading_level, image_path) = match &block {
            md::Block::Plain => (0, 0, Bytes::none()),
            md::Block::Heading(level) => (1, *level, Bytes::none()),
            md::Block::Quote => (2, 0, Bytes::none()),
            md::Block::Fence => { fenced = !fenced; (3, 0, Bytes::none()) }
            md::Block::Code => (4, 0, Bytes::none()),
            md::Block::Image(path) => (5, 0, Bytes::of(path)),
        };
        let block_marker_length = md::mark_len(&wide, &block);
        let mut info = Line {
            block: kind, heading_level, opening_fence: kind == 3 && fenced,
            block_marker_length, image_path, list: 0, list_start: 0, list_end: 0,
            checked: false, task_offset: 0,
        };
        if matches!(block, md::Block::Plain) {
            let trimmed = line.trim_start_matches([' ', '\t']);
            let (list, rest) = store::marker(trimmed);
            if list != store::Kind::Plain {
                info.list = if list == store::Kind::Bullet { 1 } else { 2 };
                info.list_start = line[..line.len() - trimmed.len()].encode_utf16().count();
                info.list_end = line[..line.len() - rest.len()].encode_utf16().count();
                if let Some((checked, body)) = store::task(list, rest) {
                    info.list = 3;
                    info.checked = checked;
                    info.task_offset = info.list_end + 1;
                    info.list_end = line[..line.len() - body.len()].encode_utf16().count();
                }
            }
        }
        let start = info.list_end.max(block_marker_length);
        let (spans, links) = if kind == 3 || kind == 4 { (Vec::new(), Vec::new()) } else { md::inline(&wide[start..]) };
        let spans: Vec<_> = spans.iter().map(|span| Span {
            start: start + span.a, end: start + span.b, style: span.style as u8,
            hidden: md::hidden_mark(&wide[start..], span),
        }).collect();
        let links: Vec<_> = links.iter().map(|link| Link {
            start: start + link.a, end: start + link.b, target: Bytes::of(&link.url),
        }).collect();
        callback(context, info, spans.as_ptr(), spans.len(), links.as_ptr(), links.len());
    }
}

type EditCallback = extern "C" fn(*mut c_void, usize, usize, Bytes, bool);

#[no_mangle]
unsafe extern "C" fn notedd_format_edit(
    line: Bytes, start: usize, end: usize, style: u8, simple: bool, context: *mut c_void, callback: EditCallback,
) {
    let Some(line) = line.text() else { return };
    let line: Vec<_> = line.encode_utf16().collect();
    if start > end || end > line.len() { return; }
    let format = match style {
        1 => md::Format::Bold,
        2 => md::Format::Italic,
        3 => md::Format::Strike,
        4 => md::Format::Highlight,
        5 => md::Format::Code,
        _ => return,
    };
    if let Some((range, marker)) = md::format_edit(&line, start..end, format, simple) {
        let marker = marker.map_or_else(String::new, String::from_utf16_lossy);
        callback(context, range.start, range.end, Bytes::of(&marker), !marker.is_empty());
    }
}

type ReplacementCallback = extern "C" fn(*mut c_void, usize, usize, Bytes, Bytes);

#[no_mangle]
unsafe extern "C" fn notedd_simple_replacement(
    text: Bytes, start: usize, end: usize, erase: bool, context: *mut c_void, callback: ReplacementCallback,
) {
    let Some(text) = text.text() else { return };
    let wide: Vec<_> = text.encode_utf16().collect();
    if start >= end || end > wide.len() || (start == 0 && end == wide.len()) { return; }
    let lines: Vec<_> = wide.split(|&ch| ch == 10).collect();
    let mut offset = 0;
    let mut pairs = Vec::new();
    for (line, block) in lines.iter().zip(md::blocks(&lines)) {
        let line_start = offset;
        offset += line.len() + 1;
        if line_start > end { break; }
        if offset <= start || matches!(block, md::Block::Fence | md::Block::Code | md::Block::Image(_)) { continue; }
        let prefix = md::mark_len(line, &block);
        let base = line_start + prefix;
        pairs.extend(md::delimiters(&line[prefix..]).into_iter()
            .map(|(l, r)| (base + l.start..base + l.end, base + r.start..base + r.end)));
    }
    if let Some((range, before, after)) = md::preserve_delimiters(&wide, start..end, pairs, erase) {
        let before = String::from_utf16_lossy(&before);
        let after = String::from_utf16_lossy(&after);
        callback(context, range.start, range.end, Bytes::of(&before), Bytes::of(&after));
    }
}
