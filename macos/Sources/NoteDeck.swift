import AppKit

struct EditorSnapshot {
    let image: NSImage
    let viewport: CGRect
}

final class NoteDeck: NSView {
    private static let settleTime: TimeInterval = 0.42
    private static let snapshotTime: TimeInterval = 0.12

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    var notes: [Note] = [] {
        didSet {
            let previous = oldValue.indices.contains(selected) ? oldValue[selected].id : nil
            if queryChanged {
                selected = 0
                first = 0
            } else if let previous, let index = notes.firstIndex(where: { $0.id == previous }) {
                selected = index
            }
            queryChanged = false
            selected = min(selected, max(0, notes.count - 1))
            first = max(0, min(first, selected, max(0, notes.count - capacity)))
            keepSelectionVisible()
            if phase == .open { retarget() }
        }
    }
    var total = 0 { didSet { needsDisplay = true } }
    var query = "" { didSet { queryChanged = oldValue != query; needsDisplay = true } }
    var palette: ((Int) -> NSColor)?
    var markdown: Markdown?
    var line = NSColor.darkGray
    var darkTab = NSColor.darkGray
    var onDark = NSColor.white
    var shadowColor = NSColor.black.withAlphaComponent(0.20)
    var open: ((String) -> Void)?
    var showColors: ((Note, CGPoint) -> Void)?
    var delete: (() -> Void)?
    var close: (() -> Void)?
    var header: ((CGRect, CGFloat) -> Void)?
    private enum Phase { case closed, open, folding }
    private enum Key: Hashable { case search, note(String) }

    private struct Card {
        let key: Key
        var note: Note?
        var shape: FolderShape
        var offset: CGFloat = 0
        var scale: CGFloat = 1
        var angle: CGFloat = 0
        var shift: CGFloat = 0
        var opacity: CGFloat = 1
        var detail: CGFloat = 1
        var snapshot: EditorSnapshot?
        var snapshotOpacity: CGFloat = 0

        var transform: AffineTransform {
            let pivot = CGPoint(x: shape.bounds.midX, y: shape.bounds.minY)
            let radians = angle * .pi / 180
            let cosine = cos(radians) * scale
            let sine = sin(radians) * scale
            return AffineTransform(m11: cosine, m12: sine, m21: -sine, m22: cosine,
                                   tX: pivot.x + shift * scale - pivot.x * cosine + pivot.y * sine,
                                   tY: pivot.y + offset * scale - pivot.x * sine - pivot.y * cosine)
        }

        func interpolated(to target: Card, elapsed: TimeInterval) -> Card {
            let settled = elapsed >= NoteDeck.settleTime
            let geometry: CGFloat = settled ? 1 : 1 - exp(-17 * max(0, elapsed))
            let fade: CGFloat = settled ? 1 : 1 - exp(-30 * max(0, elapsed))
            let text = min(1, max(0, elapsed / NoteDeck.snapshotTime))
            let blend = text * text * (3 - 2 * text)
            var result = target
            result.shape.bounds = shape.bounds.interpolated(to: target.shape.bounds, by: geometry)
            result.shape.corners = shape.corners.interpolated(to: target.shape.corners, by: geometry)
            result.offset = offset + (target.offset - offset) * geometry
            result.scale = scale + (target.scale - scale) * geometry
            result.angle = angle + (target.angle - angle) * geometry
            result.shift = shift + (target.shift - shift) * geometry
            result.opacity = opacity + (target.opacity - opacity) * fade
            result.detail = detail + (target.detail - detail) * geometry
            result.snapshot = target.snapshot ?? snapshot
            result.snapshotOpacity = snapshotOpacity + (target.snapshotOpacity - snapshotOpacity) * blend
            return result
        }
    }

    private let motion = Motion()
    private var phase = Phase.closed
    private var cards: [Card] = []
    private var targets: [Card] = []
    private var queryChanged = false
    private var selected = 0
    private var first = 0
    private var wheel: CGFloat = 0
    private var tracking: NSTrackingArea?
    private var previews: [String: NSAttributedString] = [:]
    private var capacity: Int { max(1, min(10, Int((bounds.height - 245) / 42))) }
    private var count: Int { min(notes.count, capacity) }
    private var step: CGFloat { min(50, max(39, (bounds.height - 274) / CGFloat(max(1, count)))) }
    private let cardHeight: CGFloat = 290
    var selection: Note? { notes.indices.contains(selected) ? notes[selected] : nil }

    func paletteAnchor(for note: Note) -> CGPoint? {
        guard let window, let card = cards.first(where: { $0.note?.id == note.id }) else { return nil }
        let point = CGPoint(x: card.shape.bounds.midX, y: card.shape.bounds.minY + 42)
        return window.convertPoint(toScreen: convert(card.transform.transform(point), to: nil))
    }

    func refresh() {
        previews.removeAll(keepingCapacity: true)
        needsDisplay = true
    }

    func showStack(from editor: FolderShape, current: Note?, snapshot: EditorSnapshot?) {
        phase = .open
        selected = 0
        first = 0
        wheel = 0
        let destination = stack()
        if cards.isEmpty, let current {
            cards = [Card(key: .note(current.id), note: current, shape: editor, detail: 0,
                          snapshot: snapshot, snapshotOpacity: snapshot == nil ? 0 : 1)]
        }
        transition(to: destination, stagger: true, editor: editor)
    }

    func showEditor(to editor: FolderShape, note: Note?, snapshot: EditorSnapshot?,
                    replacing previous: (note: Note, snapshot: EditorSnapshot?)?, completion: @escaping () -> Void) {
        phase = .folding
        let front = note.map { Key.note($0.id) }
        let current = cards.first { $0.key == front }
        var underlay: Key?
        if let previous, cards.isEmpty {
            underlay = .note(previous.note.id)
            cards.append(Card(key: .note(previous.note.id), note: previous.note, shape: editor, detail: 0,
                              snapshot: previous.snapshot, snapshotOpacity: previous.snapshot == nil ? 0 : 1))
        } else if current == nil {
            underlay = cards.last { $0.opacity >= 0.999 }?.key
        }
        if let note, let front, current == nil {
            cards.append(Card(key: front, note: note, shape: editor, offset: 46, scale: 0.955,
                              opacity: 0, detail: 0, snapshot: snapshot, snapshotOpacity: snapshot == nil ? 0 : 1))
        }
        var destination = cards.filter { $0.key != front }.map { card in
            if card.key == underlay { return card }
            return Card(key: card.key, note: card.note, shape: editor, offset: 18, scale: 0.955,
                        opacity: 0, detail: card.detail)
        }
        if let note, let front {
            destination.append(Card(key: front, note: note, shape: editor, detail: 0,
                                    snapshot: snapshot, snapshotOpacity: snapshot == nil ? 0 : 1))
        }
        transition(to: destination) { [weak self] in
            guard let self else { return }
            completion()
            phase = .closed
            cards = []
            targets = []
        }
    }

    private func keepSelectionVisible() {
        first = max(selected - capacity + 1, min(first, selected))
    }

    private func moveSelection(_ step: Int) {
        let next = max(0, min(notes.count - 1, selected + step))
        guard next != selected else { return }
        selected = next
        keepSelectionVisible()
        retarget()
    }

    override func layout() {
        super.layout()
        keepSelectionVisible()
        if phase == .open { retarget() }
    }

    private func retarget(direction: CGFloat = 0) {
        guard phase == .open, bounds.width > 36, bounds.height > 60 else { return }
        let destination = stack()
        if targets.map(\.key) == destination.map(\.key),
           zip(targets, destination).allSatisfy({ old, new in
               old.shape.bounds == new.shape.bounds && old.offset == new.offset && old.angle == new.angle &&
                   old.scale == new.scale && old.note?.text == new.note?.text && old.note?.color == new.note?.color &&
                   old.note?.modified == new.note?.modified
           }) { return }
        transition(to: destination, direction: direction)
    }

    private func transition(to destination: [Card], stagger: Bool = false, editor: FolderShape? = nil,
                            direction: CGFloat = 0, completion: (() -> Void)? = nil) {
        let previous = Dictionary(uniqueKeysWithValues: cards.map { ($0.key, $0) })
        let newKeys = Set(destination.map(\.key))
        let leaving = cards.filter { !newKeys.contains($0.key) && $0.opacity > 0.005 }.map { card in
            var end = card
            end.offset = editor == nil ? card.offset + direction * step : 18
            end.opacity = 0
            end.snapshotOpacity = 0
            return end
        }
        let final = leaving + destination
        let initial = final.map { target in
            if let existing = previous[target.key] { return existing }
            var start = target
            if let editor {
                start.shape = editor
                start.offset = 18
                start.scale = 0.955
                start.angle = 0
                start.shift = 0
            } else { start.offset -= direction * step }
            start.opacity = 0
            return start
        }
        targets = destination
        let staggerStep: TimeInterval = stagger ? 0.018 : 0
        let leavingDelay: TimeInterval = stagger && !leaving.isEmpty ? Self.snapshotTime : 0
        let duration = Self.settleTime + max(leavingDelay, staggerStep * Double(max(0, final.count - 1)))
        motion.start(duration: duration) { [weak self] time in
            guard let self else { return }
            cards = zip(initial, final).enumerated().map { index, pair in
                let delay = index < leaving.count && stagger ? leavingDelay :
                    pair.0.snapshotOpacity > 0 ? 0 : Double(index) * staggerStep
                return pair.0.interpolated(to: pair.1, elapsed: time * duration - delay)
            }
            displayCards()
        } completion: { [weak self] in
            self?.cards = destination
            self?.displayCards()
            completion?()
        }
    }

    private func displayCards() {
        if let search = cards.first(where: { $0.key == .search }) {
            let rect = search.shape.bounds
            let origin = search.transform.transform(rect.origin)
            header?(CGRect(origin: origin, size: CGSize(width: rect.width * search.scale, height: rect.height * search.scale)), search.scale)
            for view in subviews {
                view.isHidden = phase != .open
                view.alphaValue = search.opacity
            }
        }
        needsDisplay = true
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.activeInKeyWindow, .inVisibleRect, .mouseMoved], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseMoved(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard phase == .open, let note = hitCard(at: point),
              let index = notes.firstIndex(where: { $0.id == note.id }), selected != index else { return }
        selected = index
        retarget()
    }

    override func mouseDown(with event: NSEvent) {
        if event.modifierFlags.contains(.control) { rightMouseDown(with: event); return }
        let point = convert(event.locationInWindow, from: nil)
        guard phase == .open else { return }
        if let note = hitCard(at: point) { open?(note.id) }
        else { (window as? FolderWindow)?.dragFolder(with: event) }
    }

    override func rightMouseDown(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard phase == .open, let window, let note = hitCard(at: point) else { return }
        showColors?(note, window.convertPoint(toScreen: event.locationInWindow))
    }

    override func scrollWheel(with event: NSEvent) {
        guard phase == .open else { return }
        if event.phase == .began { wheel = 0 }
        wheel += event.scrollingDeltaY
        let threshold: CGFloat = event.hasPreciseScrollingDeltas ? 25 : 1
        if abs(wheel) >= threshold {
            let delta = wheel > 0 ? -1 : 1
            let previous = first
            first = max(0, min(notes.count - capacity, first + delta))
            if count > 0 { selected = min(max(first, selected), first + count - 1) }
            wheel = 0
            if previous != first { retarget(direction: CGFloat(delta)) }
        }
    }

    override func keyDown(with event: NSEvent) {
        switch event.keyCode {
        case 125: moveDown(nil)
        case 126: moveUp(nil)
        case 36, 76: if let selection { open?(selection.id) }
        case 51, 117: delete?()
        case 53: close?()
        default: super.keyDown(with: event)
        }
    }

    override func moveUp(_ sender: Any?) { moveSelection(1) }
    override func moveDown(_ sender: Any?) { moveSelection(-1) }

    private func stack() -> [Card] {
        func pose(_ index: Int, note: Note? = nil) -> Card {
            let scale = 0.955 * 1700 / (1700 + CGFloat(index) * 26)
            let picked = note != nil && first + index == selected
            let offset = max(0, CGFloat(count - index) * step - (picked ? 14 : 0))
            let height = min(cardHeight, (bounds.height - WindowMagnet.top - WindowMagnet.bottom) / scale - offset)
            let shape = FolderShape(bounds: CGRect(x: WindowMagnet.side, y: WindowMagnet.top,
                                                   width: bounds.width - 2 * WindowMagnet.side, height: height))
            let hash = note?.id.utf8.reduce(UInt32(2166136261)) { ($0 ^ UInt32($1)) &* 16777619 } ?? 0
            let sign: CGFloat = hash & 1 == 0 ? -1 : 1
            let angle = picked ? sign * (0.7 + CGFloat((hash >> 1) % 100) * 0.011) : 0
            let shift = picked ? sign * (3 + CGFloat((hash >> 8) % 100) * 0.07) : 0
            return Card(key: note.map { .note($0.id) } ?? .search, note: note, shape: shape,
                        offset: offset, scale: scale, angle: angle, shift: shift)
        }
        return [pose(count)] + notes.dropFirst(first).prefix(count).enumerated().reversed().map { pose($0.offset, note: $0.element) }
    }

    private func hitCard(at point: NSPoint) -> Note? {
        cards.reversed().first { card in
            guard card.note != nil, card.opacity > 0.35 else { return false }
            return card.shape.outline.contains(card.transform.inverted()?.transform(point) ?? point)
        }?.note
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let palette, let markdown, let context = NSGraphicsContext.current?.cgContext else { return }
        for card in cards where card.opacity > 0.005 {
            context.saveGState()
            (card.transform as NSAffineTransform).concat()
            if card.opacity < 0.999 {
                context.setAlpha(card.opacity)
                context.beginTransparencyLayer(auxiliaryInfo: nil)
            }
            if let note = card.note {
                drawNote(note, card: card, palette: palette, markdown: markdown)
            } else {
                drawSearch(card.shape, palette: palette, markdown: markdown)
            }
            if card.opacity < 0.999 {
                context.endTransparencyLayer()
            }
            context.restoreGState()
        }
        let visibleIDs = Set(cards.compactMap { $0.note?.id })
        previews = previews.filter { visibleIDs.contains($0.key) }
    }

    private func drawSearch(_ search: FolderShape, palette: (Int) -> NSColor, markdown: Markdown) {
        search.draw(fill: palette(0), line: line, shadow: shadowColor, tab: (darkTab, 104))
        let countText = notes.count > count ? "\(first + 1)–\(first + count)/\(notes.count)" : query.isEmpty ? "\(total)" : "\(notes.count)/\(total)"
        let counter = NSAttributedString(string: countText, attributes: [.font: Markdown.geologica(size: 12), .foregroundColor: onDark])
        counter.draw(at: NSPoint(x: search.bounds.minX + search.tabRight + (104 - counter.size().width) / 2,
                                 y: search.bounds.minY + (34 - counter.size().height) / 2))
        if count == 0 {
            let message = query.isEmpty ? "Your notes will appear here." : "No matches for “\(query)”. Press Return to create a note."
            NSAttributedString(string: message, attributes: [.font: Markdown.geologica(size: 13), .foregroundColor: markdown.muted])
                .draw(in: search.bounds.insetBy(dx: 40, dy: 82))
        }
    }

    private func drawNote(_ note: Note, card: Card, palette: (Int) -> NSColor, markdown: Markdown) {
        let folder = card.shape
        let rect = folder.bounds
        NSGraphicsContext.saveGraphicsState()
        let tabWidth = (1 - card.detail) * 34
        folder.draw(fill: palette(note.color), line: line, shadow: shadowColor,
                    tab: tabWidth > 2 ? (darkTab, tabWidth) : nil)
        if tabWidth > 18 {
            FolderGlyph.plus.image(onDark).draw(in: NSRect(x: rect.minX + folder.tabRight + (tabWidth - 16) / 2,
                                                         y: rect.minY + 9, width: 16, height: 16),
                                               from: .zero, operation: .sourceOver, fraction: (tabWidth - 18) / 16,
                                               respectFlipped: true, hints: nil)
        }
        let label = note.title.isEmpty ? "Untitled" : note.title
        let style = NSMutableParagraphStyle()
        style.lineBreakMode = .byTruncatingTail
        let title = NSAttributedString(string: label, attributes: [.font: Markdown.geologica(size: 15, weight: 500), .foregroundColor: markdown.ink, .paragraphStyle: style])
        title.draw(in: NSRect(x: rect.minX + 50, y: rect.minY + 8, width: folder.tabRight - 56, height: 20))
        let words = note.text.split(whereSeparator: \.isWhitespace).count
        let calendar = Calendar.current
        let date: String
        if calendar.isDateInToday(note.modified) || calendar.isDateInYesterday(note.modified) {
            let day = calendar.isDateInToday(note.modified) ? "Today" : "Yesterday"
            date = "\(day), \(note.modified.formatted(date: .omitted, time: .shortened))"
        } else { date = note.modified.formatted(.dateTime.month(.abbreviated).day().locale(Locale(identifier: "en_US"))) }
        let info = NSAttributedString(string: "\(date) · \(words) \(words == 1 ? "word" : "words")",
                                      attributes: [.font: Markdown.geologica(size: 11), .foregroundColor: markdown.muted.withAlphaComponent(0.76 * card.detail)])
        info.draw(at: NSPoint(x: rect.maxX - 40 - info.size().width, y: rect.minY + 44))
        folder.outline.addClip()
        if card.snapshotOpacity < 0.999 {
            if previews[note.id] == nil {
                let excerpt = note.text.components(separatedBy: "\n").prefix(20).filter {
                    !$0.trimmingCharacters(in: .whitespaces).hasPrefix("![")
                }.joined(separator: "\n")
                previews[note.id] = markdown.render(String(excerpt.prefix(1800)), preview: true, loadImages: false)
            }
            let previewRect = NSRect(x: rect.minX + 40, y: rect.minY + 70, width: rect.width - 80, height: max(0, rect.height - 86))
            previews[note.id]?.draw(with: previewRect, options: [.usesLineFragmentOrigin])
        }
        if let snapshot = card.snapshot, card.snapshotOpacity > 0.001,
           let context = NSGraphicsContext.current?.cgContext {
            let viewport = snapshot.viewport.offsetBy(dx: rect.minX, dy: rect.minY)
            context.saveGState()
            context.setAlpha(card.snapshotOpacity)
            context.beginTransparencyLayer(auxiliaryInfo: nil)
            palette(note.color).setFill()
            viewport.fill()
            snapshot.image.draw(in: viewport, from: .zero, operation: .sourceOver,
                                fraction: 1, respectFlipped: true, hints: nil)
            context.endTransparencyLayer()
            context.restoreGState()
        }
        NSGraphicsContext.restoreGraphicsState()
    }
}
