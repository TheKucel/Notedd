import AppKit
import CoreText

struct Markdown {
    let fontSize: CGFloat
    let ink: NSColor
    let muted: NSColor
    let accent: NSColor
    let highlight: NSColor
    let checkbox: Checkbox
    let folder: URL

    static func geologica(size: CGFloat, weight: Int = 400, italic: Bool = false) -> NSFont {
        Geologica.font(size: size, weight: weight, italic: italic)
    }

    var bodyFont: NSFont { Self.geologica(size: fontSize) }
    var titleFont: NSFont { Self.geologica(size: fontSize * 21 / 16, weight: 700) }
    private var base: [NSAttributedString.Key: Any] { [.font: bodyFont, .foregroundColor: ink] }

    func render(_ text: String, preview: Bool, width: CGFloat = 0, loadImages: Bool = true,
                simple: Bool = false) -> NSAttributedString {
        let output = NSMutableAttributedString()
        let lines = text.components(separatedBy: "\n")
        var offset = 0
        let scale = fontSize / 16
        let parsed = NoteddCore.markdown(text)
        for (index, pair) in zip(lines, parsed).enumerated() {
            let (value, syntax) = pair
            let line = value as NSString
            defer { offset += line.length + 1 }
            let paragraph = NSMutableParagraphStyle()
            paragraph.minimumLineHeight = (index == 0 ? 27 : 23) * scale
            paragraph.paragraphSpacing = (index == 0 ? 12 : 9) * scale
            var newlineAttributes = base
            newlineAttributes[.paragraphStyle] = paragraph
            if case .fence(let opening) = syntax.block {
                if preview { continue }
                paragraph.minimumLineHeight = 21 * scale
                paragraph.paragraphSpacing = opening ? 0 : 9 * scale
                newlineAttributes[.font] = codeFont
                newlineAttributes[.foregroundColor] = checkbox.outline
                if simple {
                    paragraph.minimumLineHeight = 1
                    paragraph.maximumLineHeight = 1
                    paragraph.paragraphSpacing = opening ? 0 : 5 * scale
                    newlineAttributes[.font] = NSFont.systemFont(ofSize: 1)
                    var hidden = newlineAttributes
                    hidden[.hiddenMarkdown] = true
                    output.append(NSAttributedString(string: value, attributes: hidden))
                } else {
                    output.append(NSAttributedString(string: value, attributes: newlineAttributes))
                }
            } else if case .code = syntax.block {
                paragraph.minimumLineHeight = 21 * scale
                paragraph.paragraphSpacing = 0
                newlineAttributes[.font] = codeFont
                output.append(NSAttributedString(string: value, attributes: newlineAttributes))
            } else if (preview || simple), loadImages, case .image(let path) = syntax.block,
                      let picture = inlineImage(path, width: width, line: value) {
                if simple {
                    var hidden = newlineAttributes
                    hidden[.foregroundColor] = NSColor.clear
                    hidden[.simpleDecoration] = SimpleDecoration.picture(picture.image, picture.size)
                    output.append(NSAttributedString(string: value, attributes: hidden))
                } else {
                    let attachment = NSTextAttachment()
                    attachment.image = picture.image
                    attachment.bounds = CGRect(origin: .zero, size: picture.size)
                    output.append(NSAttributedString(attachment: attachment))
                }
            } else {
                var attributes = newlineAttributes
                attributes[.foregroundColor] = muted
                var prefix: (NSRange, String, [NSAttributedString.Key: Any])?
                var task: (range: NSRange, marker: TaskMarker)?
                if case .heading(let level) = syntax.block {
                    let level = index == 0 ? 1 : level
                    let size: CGFloat = level == 1 ? 21 : level == 2 ? 18.5 : 16
                    paragraph.minimumLineHeight = (level == 1 ? 27 : level == 2 ? 25 : 23) * scale
                    if index > 1 { paragraph.paragraphSpacingBefore = 9 * scale }
                    attributes[.font] = Self.geologica(size: size * scale, weight: 700)
                    attributes[.foregroundColor] = ink
                    prefix = (NSRange(location: 0, length: syntax.blockMarkerLength), "", [.foregroundColor: checkbox.outline])
                } else if case .task(let checked, let state) = syntax.list {
                    let range = syntax.listRange
                    let indent = (line.substring(to: range.location) as NSString).size(withAttributes: [.font: bodyFont]).width
                    paragraph.headIndent = indent + checkbox.advance
                    task = (range, TaskMarker(offset: offset + state, checked: checked, checkbox: checkbox))
                    if checked {
                        attributes[.strikethroughStyle] = NSUnderlineStyle.single.rawValue
                        attributes[.foregroundColor] = checkbox.outline
                    }
                } else if case .bullet = syntax.list {
                    prefix = (syntax.listRange, "•  ", [.foregroundColor: muted])
                } else if case .quote = syntax.block {
                    prefix = (NSRange(location: 0, length: syntax.blockMarkerLength), "│  ", [.foregroundColor: muted])
                } else if index == 0 && !value.isEmpty {
                    attributes[.font] = titleFont
                    attributes[.foregroundColor] = ink
                }
                if index > 0, parsed.indices.contains(index + 1), syntax.isList, parsed[index + 1].isList {
                    paragraph.paragraphSpacing = 3 * scale
                }
                newlineAttributes = attributes
                var start = 0
                if let (range, marker) = task {
                    if range.location > 0 {
                        output.append(NSAttributedString(string: line.substring(to: range.location), attributes: attributes))
                    }
                    var style = attributes
                    style[.strikethroughStyle] = nil
                    if preview {
                        style[.link] = URL(string: "notedd-task:\(marker.offset)")
                        style[.attachment] = checkbox.attachment(checked: marker.checked)
                        output.append(NSAttributedString(string: "\u{FFFC}", attributes: style))
                    } else {
                        style[.foregroundColor] = NSColor.clear
                        style[.taskMarker] = marker
                        let first = line.substring(with: NSRange(location: range.location, length: 1))
                        style[.kern] = checkbox.advance - (first as NSString).size(withAttributes: [.font: bodyFont]).width
                        output.append(NSAttributedString(string: line.substring(with: range), attributes: style))
                    }
                    start = NSMaxRange(range)
                }
                if let (range, replacement, style) = prefix {
                    if range.location > 0 {
                        output.append(NSAttributedString(string: line.substring(to: range.location), attributes: attributes))
                    }
                    var markerStyle = attributes.merging(style) { _, new in new }
                    if simple {
                        if replacement.isEmpty {
                            markerStyle[.hiddenMarkdown] = true
                        } else {
                            let first = line.substring(with: NSRange(location: range.location, length: 1))
                            let font = attributes[.font] as? NSFont ?? bodyFont
                            let decoration = replacement.trimmingCharacters(in: .whitespaces)
                            let glyphWidth = (first as NSString).size(withAttributes: [.font: font]).width
                            let advance = (replacement as NSString).size(withAttributes: [.font: font]).width
                            markerStyle[.foregroundColor] = NSColor.clear
                            markerStyle[.kern] = advance - glyphWidth
                            markerStyle[.simpleDecoration] = SimpleDecoration.marker(decoration, font, muted)
                        }
                    }
                    output.append(NSAttributedString(string: preview ? replacement : line.substring(with: range),
                                                     attributes: markerStyle))
                    start = NSMaxRange(range)
                }
                appendInline(line, syntax: syntax, start: start, to: output, attributes: attributes, preview: preview, simple: simple)
            }
            if index < lines.count - 1 { output.append(NSAttributedString(string: "\n", attributes: newlineAttributes)) }
        }
        return output
    }

    private var codeFont: NSFont { .monospacedSystemFont(ofSize: fontSize * 14.5 / 16, weight: .regular) }

    private func appendInline(_ line: NSString, syntax: NoteddCore.Line, start: Int, to output: NSMutableAttributedString,
                              attributes: [NSAttributedString.Key: Any], preview: Bool, simple: Bool) {
        let body = NSMutableAttributedString(string: line.substring(from: start), attributes: attributes)
        var hidden = IndexSet()
        for span in syntax.spans where span.start >= start {
            let range = NSRange(location: span.start - start, length: span.end - span.start)
            if (preview || simple) && span.hidden { hidden.insert(integersIn: range.location..<NSMaxRange(range)) }
            switch Int(span.style) {
            case NOTEDD_MARK:
                body.addAttribute(.foregroundColor, value: checkbox.outline, range: range)
            case NOTEDD_BOLD, NOTEDD_ITALIC:
                var fonts: [(NSFont, NSRange)] = []
                body.enumerateAttribute(.font, in: range) { value, range, _ in
                    let font = value as? NSFont ?? bodyFont
                    let axes = CTFontCopyVariation(font) as? [NSNumber: NSNumber]
                    let weight = span.style == UInt8(NOTEDD_BOLD) ? 700 : axes?[Geologica.weightAxis]?.intValue ?? 400
                    let italic = span.style == UInt8(NOTEDD_ITALIC) || (axes?[Geologica.slantAxis]?.intValue ?? 0) < 0
                    fonts.append((Self.geologica(size: font.pointSize, weight: weight, italic: italic), range))
                }
                for (font, range) in fonts { body.addAttribute(.font, value: font, range: range) }
            case NOTEDD_STRIKE:
                body.addAttribute(.strikethroughStyle, value: NSUnderlineStyle.single.rawValue, range: range)
            case NOTEDD_HIGHLIGHT:
                body.addAttribute(.backgroundColor, value: highlight, range: range)
            case NOTEDD_INLINE_CODE:
                body.addAttribute(.font, value: codeFont, range: range)
            default: break
            }
        }
        for link in syntax.links where link.range.location >= start {
            guard let url = linkURL(link.target) else { continue }
            let range = NSRange(location: link.range.location - start, length: link.range.length)
            body.addAttributes([.link: url, .foregroundColor: accent, .underlineStyle: NSUnderlineStyle.single.rawValue], range: range)
        }
        for range in hidden.rangeView.reversed() {
            let characters = NSRange(location: range.lowerBound, length: range.count)
            if simple {
                body.removeAttribute(.link, range: characters)
                body.addAttribute(.hiddenMarkdown, value: true, range: characters)
            } else {
                body.deleteCharacters(in: characters)
            }
        }
        output.append(body)
    }

    private func linkURL(_ target: String) -> URL? {
        if let url = URL(string: target), let scheme = url.scheme?.lowercased() {
            return ["https", "http", "mailto"].contains(scheme) ? url : nil
        }
        return localURL(target)
    }

    private func localURL(_ target: String) -> URL? {
        let path = target.removingPercentEncoding ?? target
        guard !path.hasPrefix("/"), !path.contains(":"), !path.isEmpty else { return nil }
        let root = folder.resolvingSymlinksInPath().standardizedFileURL
        let resolved = root.appendingPathComponent(path).resolvingSymlinksInPath().standardizedFileURL
        guard resolved.pathComponents.dropLast().starts(with: root.pathComponents) else { return nil }
        return resolved
    }

    private func inlineImage(_ path: String, width: CGFloat, line: String) -> (image: NSImage, size: CGSize)? {
        guard let url = localURL(path), let image = NSImage(contentsOf: url), image.size.width > 0, image.size.height > 0 else { return nil }
        let pattern = #"(?<=\"|\s)notedd-width=(\d+)(?=\")"#
        let range = NSRange(location: 0, length: (line as NSString).length)
        let match = try? NSRegularExpression(pattern: pattern).firstMatch(in: line, range: range)
        let requested = match.flatMap { Double((line as NSString).substring(with: $0.range(at: 1))) }.map { CGFloat($0) }
        let limit = min(max(120, width), requested ?? .greatestFiniteMagnitude)
        let scale = requested == nil ? min(1, limit / image.size.width, 450 / image.size.height) : min(limit / image.size.width, 450 / image.size.height)
        return (image, CGSize(width: image.size.width * scale, height: image.size.height * scale))
    }
}

extension NSAttributedString.Key {
    static let hiddenMarkdown = Self("NoteddHiddenMarkdown")
    static let simpleDecoration = Self("NoteddSimpleDecoration")
}

enum SimpleDecoration {
    case marker(String, NSFont, NSColor)
    case picture(NSImage, CGSize)

    var isPicture: Bool {
        if case .picture = self { return true }
        return false
    }
}
