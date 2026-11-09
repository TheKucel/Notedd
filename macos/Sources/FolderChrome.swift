import AppKit

struct FolderShape {
    static let tabHeight: CGFloat = 34
    static let tabLeft: CGFloat = 38

    var bounds: NSRect
    var corners = FolderCorners()
    var tabStart = Self.tabLeft
    var tabWidth: CGFloat?
    var tabShoulder: CGFloat = 12
    var tabRight: CGFloat { tabStart + (tabWidth ?? min(300, max(180, (bounds.width * 0.34).rounded()))) }

    private func path(_ draw: (CGMutablePath) -> Void) -> NSBezierPath {
        let path = CGMutablePath()
        draw(path)
        path.closeSubpath()
        var transform = CGAffineTransform(translationX: bounds.minX, y: bounds.minY)
        let result = NSBezierPath()
        let shifted = path.copy(using: &transform) ?? path
        shifted.applyWithBlock { item in
            let points = item.pointee.points
            switch item.pointee.type {
            case .moveToPoint: result.move(to: points[0])
            case .addLineToPoint: result.line(to: points[0])
            case .addQuadCurveToPoint:
                let from = result.currentPoint, control = points[0], to = points[1]
                let c1 = CGPoint(x: (from.x + 2 * control.x) / 3, y: (from.y + 2 * control.y) / 3)
                let c2 = CGPoint(x: (to.x + 2 * control.x) / 3, y: (to.y + 2 * control.y) / 3)
                result.curve(to: to, controlPoint1: c1, controlPoint2: c2)
            case .addCurveToPoint: result.curve(to: points[2], controlPoint1: points[0], controlPoint2: points[1])
            case .closeSubpath: result.close()
            @unknown default: break
            }
        }
        return result
    }

    var outline: NSBezierPath {
        path { p in
            let w = bounds.width, h = bounds.height, t = Self.tabHeight, left = tabStart, right = tabRight
            let tl = corners.left, tr = corners.right
            let bl = min(tl, corners.bottom), br = min(tr, corners.bottom)
            p.move(to: CGPoint(x: 0, y: h - bl))
            p.addLine(to: CGPoint(x: 0, y: t + tl))
            p.addQuadCurve(to: CGPoint(x: tl, y: t), control: CGPoint(x: 0, y: t))
            p.addLine(to: CGPoint(x: left - tabShoulder, y: t))
            p.addQuadCurve(to: CGPoint(x: left, y: t - tabShoulder), control: CGPoint(x: left, y: t))
            p.addLine(to: CGPoint(x: left, y: 14))
            p.addQuadCurve(to: CGPoint(x: left + 14, y: 0), control: CGPoint(x: left, y: 0))
            p.addLine(to: CGPoint(x: right - 14, y: 0))
            p.addQuadCurve(to: CGPoint(x: right, y: 14), control: CGPoint(x: right, y: 0))
            p.addLine(to: CGPoint(x: right, y: t - 6))
            p.addQuadCurve(to: CGPoint(x: right + 6, y: t), control: CGPoint(x: right, y: t))
            p.addLine(to: CGPoint(x: w - tr, y: t))
            p.addQuadCurve(to: CGPoint(x: w, y: t + tr), control: CGPoint(x: w, y: t))
            p.addLine(to: CGPoint(x: w, y: h - br))
            p.addQuadCurve(to: CGPoint(x: w - br, y: h), control: CGPoint(x: w, y: h))
            p.addLine(to: CGPoint(x: bl, y: h))
            p.addQuadCurve(to: CGPoint(x: 0, y: h - bl), control: CGPoint(x: 0, y: h))
        }
    }

    func darkTab(width: CGFloat) -> NSBezierPath {
        path { p in
            let end = tabRight + width, t = Self.tabHeight
            p.move(to: CGPoint(x: tabRight - 14, y: 0))
            p.addLine(to: CGPoint(x: end - 14, y: 0))
            p.addQuadCurve(to: CGPoint(x: end, y: 14), control: CGPoint(x: end, y: 0))
            p.addLine(to: CGPoint(x: end, y: t - 6))
            p.addQuadCurve(to: CGPoint(x: end + 6, y: t), control: CGPoint(x: end, y: t))
            p.addLine(to: CGPoint(x: end + 6, y: t + 2))
            p.addLine(to: CGPoint(x: tabRight - 14, y: t + 2))
        }
    }

    func draw(fill: NSColor, line: NSColor, shadow: NSColor, tab: (NSColor, CGFloat)? = nil) {
        let outline = outline
        NSGraphicsContext.saveGraphicsState()
        let drop = NSShadow()
        drop.shadowColor = shadow
        drop.shadowOffset = NSSize(width: 0, height: -7)
        drop.shadowBlurRadius = 18
        drop.set()
        fill.setFill()
        outline.fill()
        NSGraphicsContext.restoreGraphicsState()
        if let (color, width) = tab {
            color.setFill()
            darkTab(width: width).fill()
        }
        fill.setFill()
        outline.fill()
        line.setStroke()
        outline.lineWidth = 1.5
        outline.stroke()
    }
}

final class FolderPage: NSView {
    override var isFlipped: Bool { true }
    var paper = NSColor.white { didSet { needsDisplay = true } }
    var line = NSColor.darkGray { didSet { needsDisplay = true } }
    var tab = NSColor.darkGray { didSet { needsDisplay = true } }
    var shadowColor = NSColor.black.withAlphaComponent(0.26) { didSet { needsDisplay = true } }
    var showsDarkTab = true
    private(set) var tabWidth: CGFloat = 34 {
        didSet {
            needsDisplay = true
            if bounds.width > 36 { updateHeader?(shape) }
        }
    }
    var updateHeader: ((FolderShape) -> Void)?
    private let cornerMotion = Motion()
    private let tabMotion = Motion()
    private let colorMotion = Motion()
    private var corners = FolderCorners() { didSet { needsDisplay = true } }
    private var targetCorners = FolderCorners()
    var shape: FolderShape {
        FolderShape(bounds: NSRect(x: WindowMagnet.side, y: WindowMagnet.top, width: bounds.width - 2 * WindowMagnet.side,
                                   height: bounds.height - WindowMagnet.top - WindowMagnet.bottom), corners: corners)
    }

    func roundCorners(to target: FolderCorners) {
        guard target != targetCorners else { return }
        targetCorners = target
        window?.invalidateCursorRects(for: self)
        let from = corners
        cornerMotion.start(duration: 0.13) { [weak self] time in
            self?.corners = from.interpolated(to: target, by: Motion.ease(time))
        }
    }

    func widenTab(to target: CGFloat) {
        let from = tabWidth
        guard from != target else { tabMotion.cancel(); return }
        tabMotion.start(duration: 0.22) { [weak self] time in
            self?.tabWidth = from + (target - from) * Motion.ease(time)
        }
    }

    func changePaper(to target: NSColor, animated: Bool) {
        let from = paper
        colorMotion.start(duration: animated && from != target ? 0.24 : 0) { [weak self] time in
            self?.paper = from.blended(withFraction: Motion.ease(time), of: target) ?? target
        }
    }

    override func layout() {
        super.layout()
        updateHeader?(shape)
    }

    override func draw(_ dirtyRect: NSRect) {
        shape.draw(fill: paper, line: line, shadow: shadowColor, tab: showsDarkTab ? (tab, tabWidth) : nil)
    }

    override func resetCursorRects() {
        let rect = shape.bounds
        guard rect.width > 24, rect.height > 46 else { return }
        let vertical = rect.height - FolderShape.tabHeight - 12
        if targetCorners.left > 0 {
            addCursorRect(NSRect(x: rect.minX, y: rect.minY + FolderShape.tabHeight, width: 12, height: vertical), cursor: .resizeLeftRight)
        }
        if targetCorners.right > 0 {
            addCursorRect(NSRect(x: rect.maxX - 12, y: rect.minY + FolderShape.tabHeight, width: 12, height: vertical), cursor: .resizeLeftRight)
        }
        if targetCorners.bottom > 0 {
            addCursorRect(NSRect(x: rect.minX, y: rect.maxY - 12, width: rect.width, height: 12), cursor: .resizeUpDown)
        }
    }

    override func mouseDown(with event: NSEvent) {
        guard let window = window as? FolderWindow else { return }
        let point = convert(event.locationInWindow, from: nil)
        let rect = shape.bounds
        let right = point.x > rect.maxX - 12 && targetCorners.right > 0
        let left = point.x < rect.minX + 12 && targetCorners.left > 0
        let bottom = point.y > rect.maxY - 12 && targetCorners.bottom > 0
        if right || left || bottom {
            window.resizeFolder(with: event, left: left, right: right, bottom: bottom)
        } else {
            window.dragFolder(with: event)
        }
    }
}

final class ChromeButton: NSButton {
    var hoverColor = NSColor.black.withAlphaComponent(0.08)
    private var tracking: NSTrackingArea?
    private var hovering = false
    private var hover: CGFloat = 0
    private let hoverMotion = Motion()
    var draggable = false
    var onHoverChange: ((Bool) -> Void)?

    override func mouseDown(with event: NSEvent) {
        guard draggable, isEnabled, let window = window as? FolderWindow else {
            super.mouseDown(with: event)
            return
        }
        window.dragFolder(with: event) { [weak self] in self?.performClick(nil) }
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.activeAlways, .inVisibleRect, .mouseEnteredAndExited], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) { setHover(true) }
    override func mouseExited(with event: NSEvent) { setHover(false) }

    private func setHover(_ active: Bool) {
        guard hovering != active else { return }
        hovering = active
        onHoverChange?(active)
        let from = hover
        let to: CGFloat = active ? 1 : 0
        hoverMotion.start(duration: 0.14) { [weak self] time in
            self?.hover = from + (to - from) * Motion.ease(time)
            self?.needsDisplay = true
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        if hover > 0 {
            hoverColor.withAlphaComponent(hoverColor.alphaComponent * hover).setFill()
            NSBezierPath(roundedRect: bounds.insetBy(dx: 2, dy: 3), xRadius: 8, yRadius: 8).fill()
        }
        super.draw(dirtyRect)
    }
}

enum FolderGlyph {
    case close, pin, pinned, folders, find, plus, more, less, settings, trash, dot, up, down
    case undo, cut, copy, paste, selectAll, bold, italic, strike, code, highlight

    func image(_ color: NSColor, fill: NSColor = .clear) -> NSImage {
        NSImage(size: NSSize(width: 16, height: 16), flipped: true) { _ in
            let path = NSBezierPath()
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            path.lineWidth = self == .plus ? 1.75 : 1.5
            func draw(_ points: [CGPoint], closed: Bool = false) {
                guard let first = points.first else { return }
                path.move(to: first)
                for point in points.dropFirst() { path.line(to: point) }
                if closed { path.close() }
            }
            func line(_ values: [(CGFloat, CGFloat)], closed: Bool = false) {
                draw(values.map { CGPoint(x: $0.0, y: $0.1) }, closed: closed)
            }
            switch self {
            case .close:
                line([(4.5, 4.5), (11.5, 11.5)]); line([(11.5, 4.5), (4.5, 11.5)])
            case .pin, .pinned:
                line([(9.8, 2.5), (13.5, 6.2), (11.2, 7), (8.8, 9.4), (9.1, 12.2), (8.1, 13.2), (6, 11.1), (3, 14), (6, 11), (3.9, 8.9), (4.9, 7.9), (7.7, 8.2), (10.1, 5.8)], closed: true)
                if self == .pinned { color.setFill(); path.fill() }
            case .folders:
                line([(2.5, 5.5), (6.5, 5.5), (7.7, 3.9), (13.5, 3.9), (13.5, 12.5), (2.5, 12.5)], closed: true)
                line([(2.5, 8), (13.5, 8)])
            case .find:
                path.appendOval(in: NSRect(x: 2.5, y: 2.5, width: 9, height: 9))
                line([(10.5, 10.5), (14, 14)])
            case .plus:
                line([(8, 3), (8, 13)]); line([(3, 8), (13, 8)])
            case .more: line([(9.5, 4.5), (6, 8), (9.5, 11.5)])
            case .less: line([(6, 4.5), (9.5, 8), (6, 11.5)])
            case .up: line([(4, 10), (8, 6), (12, 10)])
            case .down: line([(4, 6), (8, 10), (12, 6)])
            case .settings:
                line([(2.5, 5), (3.5, 5)]); line([(7.5, 5), (13.5, 5)])
                line([(2.5, 11), (8.5, 11)]); line([(12.5, 11), (13.5, 11)])
                path.appendOval(in: NSRect(x: 3.6, y: 3.1, width: 3.8, height: 3.8))
                path.appendOval(in: NSRect(x: 8.6, y: 9.1, width: 3.8, height: 3.8))
            case .trash:
                line([(3, 4.5), (13, 4.5)]); line([(6.5, 4.5), (6.5, 3), (9.5, 3), (9.5, 4.5)])
                line([(4.5, 4.5), (5.2, 13), (10.8, 13), (11.5, 4.5)])
            case .undo:
                line([(7, 3), (3, 7), (7, 11)])
                line([(3, 7), (9, 7), (12, 8.5), (13, 11), (12, 13), (8, 13)])
            case .cut:
                line([(3, 3), (5, 3), (7, 5), (5, 7), (3, 7), (2, 5)], closed: true)
                line([(3, 9), (5, 9), (7, 11), (5, 13), (3, 13), (2, 11)], closed: true)
                line([(6, 6), (13, 13)])
                line([(6, 10), (13, 3)])
            case .copy:
                line([(6, 6), (13, 6), (13, 13), (6, 13)], closed: true)
                line([(3, 10), (3, 3), (10, 3)])
            case .paste:
                line([(5, 4), (3, 4), (3, 14), (13, 14), (13, 4), (11, 4)])
                line([(6, 2), (10, 2), (11, 4), (10, 6), (6, 6), (5, 4)], closed: true)
            case .selectAll:
                line([(6, 3), (3, 3), (3, 6)])
                line([(10, 3), (13, 3), (13, 6)])
                line([(13, 10), (13, 13), (10, 13)])
                line([(6, 13), (3, 13), (3, 10)])
                line([(6, 7), (10, 7)])
                line([(6, 9), (10, 9)])
            case .bold:
                line([(5, 13), (5, 3), (9, 3), (11, 4), (11, 6), (9, 8), (5, 8)])
                line([(9, 8), (12, 9), (12, 11), (10, 13), (5, 13)])
            case .italic:
                line([(7, 3), (13, 3)])
                line([(3, 13), (9, 13)])
                line([(10, 3), (6, 13)])
            case .strike:
                line([(11, 4), (9, 3), (6, 3), (4, 5), (4, 6), (6, 8), (10, 9),
                      (12, 10), (12, 11), (10, 13), (6, 13), (4, 12)])
                line([(2, 8), (14, 8)])
            case .code:
                line([(5, 5), (2, 8), (5, 11)])
                line([(11, 5), (14, 8), (11, 11)])
                line([(9, 4), (7, 12)])
            case .highlight:
                line([(4, 9), (10, 3), (13, 6), (7, 12), (4, 12)], closed: true)
                line([(4, 12), (3, 14)])
                line([(8, 14), (13, 14)])
            case .dot:
                path.appendOval(in: NSRect(x: 1.8, y: 1.8, width: 12.4, height: 12.4))
                fill.setFill(); path.fill()
            }
            color.setStroke()
            path.stroke()
            return true
        }
    }
}
