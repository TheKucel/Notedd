import Foundation

private extension NoteddSlice {
    var buffer: UnsafeBufferPointer<UInt8> { UnsafeBufferPointer(start: data, count: length) }
    var string: String { String(decoding: buffer, as: UTF8.self) }
}

enum NoteddCore {
    struct Line {
        enum Block { case plain, heading(Int), quote, fence(opening: Bool), code, image(String) }
        enum List { case none, bullet, numbered, task(checked: Bool, offset: Int) }

        let block: Block
        let blockMarkerLength: Int
        let list: List
        let listRange: NSRange
        let spans: [NoteddSpan]
        let links: [(range: NSRange, target: String)]

        fileprivate init(_ line: NoteddLine, spans: [NoteddSpan], links: [(NSRange, String)]) {
            switch Int(line.block) {
            case NOTEDD_HEADING: block = .heading(Int(line.heading_level))
            case NOTEDD_QUOTE: block = .quote
            case NOTEDD_FENCE: block = .fence(opening: line.opening_fence)
            case NOTEDD_CODE_BLOCK: block = .code
            case NOTEDD_IMAGE: block = .image(line.image_path.string)
            default: block = .plain
            }
            switch Int(line.list) {
            case NOTEDD_BULLET: list = .bullet
            case NOTEDD_NUMBERED: list = .numbered
            case NOTEDD_TASK: list = .task(checked: line.checked, offset: line.task_offset)
            default: list = .none
            }
            blockMarkerLength = line.block_marker_length
            listRange = NSRange(location: line.list_start, length: line.list_end - line.list_start)
            self.spans = spans
            self.links = links
        }

        var isList: Bool {
            switch list {
            case .bullet, .numbered: return true
            default: return false
            }
        }
    }

    struct FormatEdit {
        let range: NSRange
        let marker: String
        let wrap: Bool
    }

    static func decode(_ raw: Data) -> (text: String, color: Int)? {
        var result: (String, Int)?
        withUnsafeMutablePointer(to: &result) { output in
            _ = withBytes(raw) { raw in
                notedd_decode_note(raw, output) { context, text, color in
                    context!.assumingMemoryBound(to: Optional<(String, Int)>.self).pointee = (text.string, color)
                }
            }
        }
        return result
    }

    static func title(_ text: String) -> String {
        var result = ""
        withUnsafeMutablePointer(to: &result) { output in
            withText(text) { input in
                notedd_note_title(input, output) { context, title in
                    context!.assumingMemoryBound(to: String.self).pointee = title.string
                }
            }
        }
        return result
    }

    static func save(to url: URL, original: Data?, text: String, color: Int) throws -> Data? {
        var written: Data?
        let status = withUnsafeMutablePointer(to: &written) { output in
            url.withUnsafeFileSystemRepresentation { path in
                withBytes(original ?? Data()) { baseline in
                    withText(text) { body in
                        notedd_save_note(path, baseline, original != nil, body, color, output) { context, bytes in
                            context!.assumingMemoryBound(to: Optional<Data>.self).pointee = Data(bytes.buffer)
                        }
                    }
                }
            }
        }
        try check(status, url: url)
        return written
    }

    static func writeNew(_ data: Data, to url: URL) throws {
        let status = url.withUnsafeFileSystemRepresentation { path in
            withBytes(data) { notedd_write_new(path, $0) }
        }
        try check(status, url: url)
    }

    static func readState(in folder: URL) -> String? {
        var result: String?
        withUnsafeMutablePointer(to: &result) { output in
            folder.withUnsafeFileSystemRepresentation { path in
                notedd_read_state(path, output) { context, id in
                    context!.assumingMemoryBound(to: Optional<String>.self).pointee = id.string
                }
            }
        }
        return result
    }

    static func writeState(_ id: String, in folder: URL) {
        folder.withUnsafeFileSystemRepresentation { path in
            withText(id) { notedd_write_state(path, $0) }
        }
    }

    static func markdown(_ text: String) -> [Line] {
        var result: [Line] = []
        withUnsafeMutablePointer(to: &result) { output in
            withText(text) { input in
                notedd_markdown(input, output) { context, line, spans, spanCount, links, linkCount in
                    let spans = Array(UnsafeBufferPointer(start: spans, count: spanCount))
                    let links = UnsafeBufferPointer(start: links, count: linkCount).map {
                        (NSRange(location: $0.start, length: $0.end - $0.start), $0.target.string)
                    }
                    context!.assumingMemoryBound(to: [Line].self).pointee.append(Line(line, spans: spans, links: links))
                }
            }
        }
        return result
    }

    static func format(_ line: String, selection: NSRange, style: UInt8, simple: Bool) -> FormatEdit? {
        var result: FormatEdit?
        withUnsafeMutablePointer(to: &result) { output in
            withText(line) { input in
                notedd_format_edit(input, selection.location, NSMaxRange(selection), style, simple, output) { context, start, end, marker, wrap in
                    context!.assumingMemoryBound(to: Optional<FormatEdit>.self).pointee =
                        FormatEdit(range: NSRange(location: start, length: end - start), marker: marker.string, wrap: wrap)
                }
            }
        }
        return result
    }

    static func simpleReplacement(_ text: String, range: NSRange, erase: Bool) -> (NSRange, String, String)? {
        typealias Replacement = (NSRange, String, String)
        var result: Replacement?
        withUnsafeMutablePointer(to: &result) { output in
            withText(text) { input in
                notedd_simple_replacement(input, range.location, NSMaxRange(range), erase, output) { context, start, end, before, after in
                    context!.assumingMemoryBound(to: Optional<Replacement>.self).pointee =
                        (NSRange(location: start, length: end - start), before.string, after.string)
                }
            }
        }
        return result
    }

    private static func withBytes<T>(_ data: Data, _ body: (NoteddSlice) -> T) -> T {
        data.withUnsafeBytes { (buffer: UnsafeRawBufferPointer) in
            body(NoteddSlice(data: buffer.bindMemory(to: UInt8.self).baseAddress, length: buffer.count))
        }
    }

    private static func withText<T>(_ text: String, _ body: (NoteddSlice) -> T) -> T {
        withBytes(Data(text.utf8), body)
    }

    private static func check(_ status: Int32, url: URL) throws {
        let code: CocoaError.Code
        switch Int(status) {
        case NOTEDD_OK: return
        case NOTEDD_INVALID: code = .fileWriteInvalidFileName
        case NOTEDD_NOT_FOUND: code = .fileNoSuchFile
        case NOTEDD_PERMISSION: code = .fileWriteNoPermission
        case NOTEDD_CONFLICT: code = .fileWriteFileExists
        default: code = .fileWriteUnknown
        }
        throw CocoaError(code, userInfo: [NSURLErrorKey: url])
    }
}
