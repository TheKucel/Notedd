import AppKit
import UniformTypeIdentifiers

final class Editor: NSTextView, NSLayoutManagerDelegate {
    var onEscape: (() -> Void)?
    var onImage: ((Data, String) -> Void)?
    var onContext: ((CGPoint) -> Void)?
    var onDelete: ((Bool) -> Bool)?
    var onDeleteWord: ((Bool) -> Bool)?
    var onNewline: (() -> Bool)?
    var onTask: ((Int) -> Void)?
    var onImageResize: ((NSRange, CGFloat) -> Void)?
    private var imageDrag: (range: NSRange, start: CGPoint, size: CGSize, width: CGFloat)?

    override func drawInsertionPoint(in rect: NSRect, color: NSColor, turnedOn flag: Bool) {
        var caret = rect
        if let layoutManager, let font = typingAttributes[.font] as? NSFont,
           let storage = textStorage {
            let index = selectedRange().location
            let picture = [index, index - 1].contains { offset in
                guard (0..<storage.length).contains(offset) else { return false }
                return (storage.attribute(.simpleDecoration, at: offset, effectiveRange: nil) as? SimpleDecoration)?.isPicture == true
            }
            // The image needs the whole line fragment. The caret still belongs to the text font.
            if picture { caret.size.height = min(rect.height, ceil(layoutManager.defaultLineHeight(for: font))) }
        }
        super.drawInsertionPoint(in: caret, color: color, turnedOn: flag)
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        enumerateMarkers(.taskMarker, in: dirtyRect) { value, _, point, height in
            guard let marker = value as? TaskMarker else { return }
            marker.checkbox.draw(in: self.checkboxRect(marker, at: point, height: height), checked: marker.checked)
        }
        enumerateMarkers(.simpleDecoration, in: dirtyRect) { value, range, point, height in
            switch value as? SimpleDecoration {
            case .picture(let image, let size):
                let width = self.imageDrag?.range.location == range.location ? self.imageDrag!.width : size.width
                let rect = CGRect(origin: CGPoint(x: point.x, y: point.y + 3),
                                  size: CGSize(width: width, height: size.height * width / size.width))
                NSGraphicsContext.saveGraphicsState()
                NSBezierPath(roundedRect: rect, xRadius: 8, yRadius: 8).addClip()
                image.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
                NSGraphicsContext.restoreGraphicsState()
                if NSEqualRanges(self.selectedRange(), range) {
                    NSColor.controlAccentColor.setStroke()
                    let border = NSBezierPath(roundedRect: rect.insetBy(dx: 1, dy: 1), xRadius: 7, yRadius: 7)
                    border.lineWidth = 3
                    border.stroke()
                }
                let handle = CGRect(x: rect.maxX - 21, y: rect.maxY - 21, width: 18, height: 18)
                NSColor.black.withAlphaComponent(0.55).setFill()
                NSBezierPath(roundedRect: handle, xRadius: 5, yRadius: 5).fill()
                let arrows = NSBezierPath()
                arrows.move(to: CGPoint(x: handle.minX + 5, y: handle.minY + 5))
                arrows.line(to: CGPoint(x: handle.maxX - 5, y: handle.maxY - 5))
                arrows.move(to: CGPoint(x: handle.minX + 5, y: handle.minY + 9))
                arrows.line(to: CGPoint(x: handle.minX + 5, y: handle.minY + 5))
                arrows.line(to: CGPoint(x: handle.minX + 9, y: handle.minY + 5))
                arrows.move(to: CGPoint(x: handle.maxX - 9, y: handle.maxY - 5))
                arrows.line(to: CGPoint(x: handle.maxX - 5, y: handle.maxY - 5))
                arrows.line(to: CGPoint(x: handle.maxX - 5, y: handle.maxY - 9))
                arrows.lineWidth = 1.6
                arrows.lineCapStyle = .round
                arrows.lineJoinStyle = .round
                NSColor.white.setStroke()
                arrows.stroke()
            case .marker(let label, let font, let color):
                let text = NSAttributedString(string: label, attributes: [.font: font, .foregroundColor: color])
                text.draw(at: CGPoint(x: point.x, y: point.y + (height - text.size().height) / 2))
            case nil: break
            }
        }
    }

    private static let imageResizeCursor: NSCursor = {
        let image = NSImage(size: NSSize(width: 24, height: 24), flipped: true) { _ in
            let path = NSBezierPath()
            path.move(to: NSPoint(x: 5, y: 5)); path.line(to: NSPoint(x: 19, y: 19))
            path.move(to: NSPoint(x: 5, y: 10)); path.line(to: NSPoint(x: 5, y: 5)); path.line(to: NSPoint(x: 10, y: 5))
            path.move(to: NSPoint(x: 14, y: 19)); path.line(to: NSPoint(x: 19, y: 19)); path.line(to: NSPoint(x: 19, y: 14))
            path.lineCapStyle = .round; path.lineJoinStyle = .round
            NSColor.white.setStroke(); path.lineWidth = 4; path.stroke()
            NSColor.black.setStroke(); path.lineWidth = 2; path.stroke()
            return true
        }
        return NSCursor(image: image, hotSpot: NSPoint(x: 12, y: 12))
    }()

    private var imageCursorTracking: NSTrackingArea?
    private var overImageResize = false

    override func updateTrackingAreas() {
        if let imageCursorTracking { removeTrackingArea(imageCursorTracking) }
        let tracking = NSTrackingArea(rect: .zero, options: [.cursorUpdate, .mouseMoved, .activeInKeyWindow, .inVisibleRect], owner: self, userInfo: nil)
        addTrackingArea(tracking)
        window?.acceptsMouseMovedEvents = true
        imageCursorTracking = tracking
        super.updateTrackingAreas()
    }

    private func resizeHandle(at point: CGPoint) -> (NSRange, CGSize)? {
        var result: (NSRange, CGSize)?
        enumerateMarkers(.simpleDecoration, in: visibleRect) { value, range, anchor, _ in
            guard case .picture(_, let size) = value as? SimpleDecoration else { return }
            let corner = CGPoint(x: anchor.x + size.width, y: anchor.y + 3 + size.height)
            if CGRect(x: corner.x - 22, y: corner.y - 22, width: 25, height: 25).contains(point) {
                result = (range, size)
            }
        }
        return result
    }

    override func mouseMoved(with event: NSEvent) {
        super.mouseMoved(with: event)
        let over = resizeHandle(at: convert(event.locationInWindow, from: nil)) != nil
        if over { Self.imageResizeCursor.set() }
        else if overImageResize { NSCursor.iBeam.set() }
        overImageResize = over
    }

    override func cursorUpdate(with event: NSEvent) {
        if resizeHandle(at: convert(event.locationInWindow, from: nil)) != nil { Self.imageResizeCursor.set() }
        else { super.cursorUpdate(with: event) }
    }

    private func skipPicture(up: Bool) -> Bool {
        guard selectedRange().length == 0, let storage = textStorage else { return false }
        let text = storage.string as NSString
        let caret = selectedRange().location
        let line = text.lineRange(for: NSRange(location: caret, length: 0))
        var adjacent = up ? line.location - 1 : NSMaxRange(line)
        var crossedImage = false
        while adjacent >= 0 && adjacent < text.length {
            let candidate = text.lineRange(for: NSRange(location: adjacent, length: 0))
            guard candidate.location < storage.length,
                  (storage.attribute(.simpleDecoration, at: candidate.location, effectiveRange: nil) as? SimpleDecoration)?.isPicture == true else { break }
            crossedImage = true
            adjacent = up ? candidate.location - 1 : NSMaxRange(candidate)
        }
        guard crossedImage else { return false }
        if !up && adjacent == text.length && text.length > 0 && text.character(at: text.length - 1) == 10 {
            setSelectedRange(NSRange(location: text.length, length: 0))
            scrollRangeToVisible(selectedRange())
            return true
        }
        guard adjacent >= 0 && adjacent < text.length else { return true }
        let destination = text.lineRange(for: NSRange(location: adjacent, length: 0))
        let end = NSMaxRange(destination) - (text.substring(with: destination).hasSuffix("\n") ? 1 : 0)
        let target = up ? end : min(destination.location + max(0, caret - line.location), end)
        setSelectedRange(NSRange(location: target, length: 0))
        scrollRangeToVisible(selectedRange())
        return true
    }

    override func moveUp(_ sender: Any?) { if !skipPicture(up: true) { super.moveUp(sender) } }
    override func moveDown(_ sender: Any?) { if !skipPicture(up: false) { super.moveDown(sender) } }

    override func mouseDown(with event: NSEvent) {
        if event.modifierFlags.contains(.control), onContext != nil {
            rightMouseDown(with: event)
            return
        }
        let point = convert(event.locationInWindow, from: nil)
        let hit = CGRect(origin: point, size: CGSize(width: 1, height: 1))
        if let (range, size) = resizeHandle(at: point) {
            imageDrag = (range, point, size, size.width)
            window?.makeFirstResponder(self)
            return
        }
        var task: Int?
        enumerateMarkers(.taskMarker, in: hit) { value, _, anchor, height in
            if let marker = value as? TaskMarker, self.checkboxRect(marker, at: anchor, height: height).contains(point) {
                task = marker.offset
            }
        }
        if let task, let onTask { onTask(task) }
        else if let range = pictureRange(at: point) {
            window?.makeFirstResponder(self)
            setSelectedRange(range)
            needsDisplay = true
        } else { super.mouseDown(with: event) }
    }

    override func mouseDragged(with event: NSEvent) {
        guard var drag = imageDrag else { super.mouseDragged(with: event); return }
        let point = convert(event.locationInWindow, from: nil)
        let maxWidth = max(120, (textContainer?.containerSize.width ?? bounds.width))
        drag.width = min(maxWidth, max(60, drag.size.width + point.x - drag.start.x),
                         drag.size.width * 450 / drag.size.height)
        imageDrag = drag
        needsDisplay = true
    }

    override func mouseUp(with event: NSEvent) {
        guard let drag = imageDrag else { super.mouseUp(with: event); return }
        imageDrag = nil
        if abs(drag.width - drag.size.width) >= 1 { onImageResize?(drag.range, drag.width) }
        needsDisplay = true
    }

    override func rightMouseDown(with event: NSEvent) {
        guard onContext != nil else { super.rightMouseDown(with: event); return }
        showContextMenu(for: event)
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard onContext != nil else { return super.menu(for: event) }
        showContextMenu(for: event)
        return nil
    }

    private func showContextMenu(for event: NSEvent) {
        guard let onContext, let window else { return }
        window.makeFirstResponder(self)
        if selectedRange().length == 0 {
            let point = convert(event.locationInWindow, from: nil)
            let index = pictureInsertion(at: point) ?? characterIndexForInsertion(at: point)
            if index <= (textStorage?.length ?? 0) {
                setSelectedRange(NSRange(location: index, length: 0))
            }
        }
        onContext(window.convertPoint(toScreen: event.locationInWindow))
    }

    private func pictureRange(at point: CGPoint) -> NSRange? {
        var result: NSRange?
        enumerateMarkers(.simpleDecoration, in: visibleRect) { value, range, anchor, _ in
            guard case .picture(_, let size) = value as? SimpleDecoration else { return }
            if CGRect(origin: CGPoint(x: anchor.x, y: anchor.y + 3), size: size).contains(point) { result = range }
        }
        return result
    }

    private func pictureInsertion(at point: CGPoint) -> Int? {
        var result: Int?
        enumerateMarkers(.simpleDecoration, in: visibleRect) { value, range, anchor, height in
            guard case .picture(_, let size) = value as? SimpleDecoration,
                  CGRect(x: anchor.x, y: anchor.y, width: size.width, height: height).contains(point) else { return }
            result = point.y < anchor.y + height / 2 ? range.location : NSMaxRange(range) + (NSMaxRange(range) < (self.textStorage?.length ?? 0) ? 1 : 0)
        }
        return result
    }

    private func moveAcrossPicture(backward: Bool) -> Bool {
        guard selectedRange().length == 0, let storage = textStorage,
              let image = ImageObject.all(in: storage).first(where: { $0.adjacent(to: selectedRange().location, backward: backward) }) else { return false }
        setSelectedRange(NSRange(location: backward ? image.before : image.after, length: 0))
        scrollRangeToVisible(selectedRange())
        return true
    }

    override func moveLeft(_ sender: Any?) { if !moveAcrossPicture(backward: true) { super.moveLeft(sender) } }
    override func moveRight(_ sender: Any?) { if !moveAcrossPicture(backward: false) { super.moveRight(sender) } }

    private func checkboxRect(_ marker: TaskMarker, at point: CGPoint, height: CGFloat) -> CGRect {
        let y = point.y + (height - marker.checkbox.size) / 2
        return CGRect(x: point.x, y: y, width: marker.checkbox.size, height: marker.checkbox.size)
    }

    private func enumerateMarkers(_ key: NSAttributedString.Key, in rect: CGRect,
                                  body: (Any, NSRange, CGPoint, CGFloat) -> Void) {
        guard let layoutManager, let container = textContainer, let storage = textStorage, storage.length > 0 else { return }
        let origin = textContainerOrigin
        let glyphs = layoutManager.glyphRange(forBoundingRect: rect.offsetBy(dx: -origin.x, dy: -origin.y), in: container)
        guard glyphs.length > 0 else { return }
        let characters = layoutManager.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        storage.enumerateAttribute(key, in: characters) { value, range, _ in
            guard let value else { return }
            var fullRange = NSRange()
            _ = storage.attribute(key, at: range.location, effectiveRange: &fullRange)
            let glyph = layoutManager.glyphIndexForCharacter(at: fullRange.location)
            let line = layoutManager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
            let location = layoutManager.location(forGlyphAt: glyph)
            var point = CGPoint(x: origin.x + line.minX + location.x, y: origin.y + line.minY)
            var height = line.height
            if key == .taskMarker || (key == .simpleDecoration && (value as? SimpleDecoration)?.isPicture == false) {
                // Paragraph spacing changes the fragment height as adjacent list items appear.
                // Anchor list markers to the text baseline instead of that changing height.
                if let font = storage.attribute(.font, at: fullRange.location, effectiveRange: nil) as? NSFont {
                    if key == .taskMarker {
                        height = font.capHeight
                        point.y += location.y - font.capHeight
                    } else {
                        height = font.ascender - font.descender
                        point.y += location.y - font.ascender
                    }
                }
            }
            body(value, fullRange, point, height)
        }
    }

    private func hidesGlyph(at character: Int, in storage: NSTextStorage) -> Bool {
        if storage.attribute(.hiddenMarkdown, at: character, effectiveRange: nil) != nil { return true }
        for key in [NSAttributedString.Key.taskMarker, .simpleDecoration] {
            var range = NSRange()
            if storage.attribute(key, at: character, effectiveRange: &range) != nil { return character != range.location }
        }
        return false
    }

    func layoutManager(_ layoutManager: NSLayoutManager, shouldGenerateGlyphs glyphs: UnsafePointer<CGGlyph>,
                       properties: UnsafePointer<NSLayoutManager.GlyphProperty>, characterIndexes: UnsafePointer<Int>,
                       font: NSFont, forGlyphRange glyphRange: NSRange) -> Int {
        guard let storage = textStorage else { return 0 }
        var traits: [NSLayoutManager.GlyphProperty]?
        for index in 0..<glyphRange.length {
            let character = characterIndexes[index]
            guard character < storage.length, hidesGlyph(at: character, in: storage) else { continue }
            if traits == nil { traits = Array(UnsafeBufferPointer(start: properties, count: glyphRange.length)) }
            traits?[index].insert(.null)
        }
        guard let traits else { return 0 }
        layoutManager.setGlyphs(glyphs, properties: traits, characterIndexes: characterIndexes, font: font, forGlyphRange: glyphRange)
        return glyphRange.length
    }

    func layoutManager(_ layoutManager: NSLayoutManager, shouldSetLineFragmentRect rect: UnsafeMutablePointer<NSRect>,
                       lineFragmentUsedRect used: UnsafeMutablePointer<NSRect>, baselineOffset: UnsafeMutablePointer<CGFloat>,
                       in textContainer: NSTextContainer, forGlyphRange range: NSRange) -> Bool {
        guard range.length > 0, let storage = textStorage else { return false }
        let index = layoutManager.characterIndexForGlyph(at: range.location)
        guard index < storage.length,
              case .picture(_, let size) = storage.attribute(.simpleDecoration, at: index, effectiveRange: nil) as? SimpleDecoration
        else { return false }
        let height = max(rect.pointee.height, size.height + 6)
        rect.pointee.size.height = height
        used.pointee.size.height = height
        return true
    }

    override func deleteBackward(_ sender: Any?) {
        if onDelete?(true) != true { super.deleteBackward(sender) }
    }

    override func insertNewline(_ sender: Any?) {
        if !hasMarkedText(), NSApp.currentEvent?.modifierFlags.contains(.shift) != true,
           onNewline?() == true { return }
        super.insertNewline(sender)
    }

    override func deleteWordBackward(_ sender: Any?) {
        if !hasMarkedText(), selectedRanges.count == 1, selectedRange().length == 0,
           let storage = textStorage {
            let caret = selectedRange().location
            let text = storage.string as NSString
            if caret > 0, caret <= text.length {
                let start = text.lineRange(for: NSRange(location: caret, length: 0)).location
                if start == caret {
                    deleteBackward(sender)
                    return
                }
                let prefix = NSRange(location: start, length: caret - start)
                if text.substring(with: prefix).trimmingCharacters(in: .whitespaces).isEmpty {
                    insertText("", replacementRange: prefix)
                    return
                }
            }
        }
        if onDeleteWord?(true) != true { super.deleteWordBackward(sender) }
    }

    override func deleteWordForward(_ sender: Any?) {
        if onDeleteWord?(false) != true { super.deleteWordForward(sender) }
    }

    override func deleteToBeginningOfLine(_ sender: Any?) {
        guard !hasMarkedText(), selectedRanges.count == 1, selectedRange().length == 0,
              let storage = textStorage else { super.deleteToBeginningOfLine(sender); return }
        let caret = selectedRange().location
        let text = storage.string as NSString
        guard caret > 0, caret <= text.length,
              text.lineRange(for: NSRange(location: caret, length: 0)).location == caret else {
            super.deleteToBeginningOfLine(sender)
            return
        }
        let previous = text.lineRange(for: NSRange(location: caret - 1, length: 0))
        insertText("", replacementRange: NSRange(location: previous.location, length: caret - previous.location))
    }

    override func deleteForward(_ sender: Any?) {
        if onDelete?(false) != true { super.deleteForward(sender) }
    }

    override func cancelOperation(_ sender: Any?) { onEscape?() }

    override func paste(_ sender: Any?) {
        if let image = Self.images(on: .general).first, let onImage {
            onImage(image.0, image.1)
        } else {
            pasteAsPlainText(sender)
        }
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        if Self.hasImage(on: sender.draggingPasteboard) { return .copy }
        return super.draggingEntered(sender)
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        if Self.hasImage(on: sender.draggingPasteboard) { return .copy }
        return super.draggingUpdated(sender)
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        let images = Self.images(on: sender.draggingPasteboard)
        guard !images.isEmpty, let onImage else { return super.performDragOperation(sender) }
        let point = convert(sender.draggingLocation, from: nil)
        let index = min(pictureInsertion(at: point) ?? characterIndexForInsertion(at: point), textStorage?.length ?? 0)
        setSelectedRange(NSRange(location: index, length: 0))
        for (data, ext) in images { onImage(data, ext) }
        return true
    }

    private static func imageURLs(on board: NSPasteboard) -> [URL] {
        let urls = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
        return urls.filter { UTType(filenameExtension: $0.pathExtension)?.conforms(to: .image) == true }
    }

    private static func hasImage(on board: NSPasteboard) -> Bool {
        board.availableType(from: [.png, .tiff]) != nil || !imageURLs(on: board).isEmpty
    }

    private static func images(on board: NSPasteboard) -> [(Data, String)] {
        let files: [(Data, String)] = imageURLs(on: board).compactMap {
            guard let data = try? Data(contentsOf: $0), NSImage(data: data) != nil else { return nil }
            return (data, $0.pathExtension.lowercased())
        }
        if !files.isEmpty { return files }
        if let png = board.data(forType: .png) { return [(png, "png")] }
        if let tiff = board.data(forType: .tiff), let png = NSBitmapImageRep(data: tiff)?.representation(using: .png, properties: [:]) {
            return [(png, "png")]
        }
        return []
    }
}
