import AppKit

enum SetupStyle {
    static let width: CGFloat = 640
    static let height: CGFloat = 504
    static let margin: CGFloat = 24
    static let cardFrame = CGRect(x: 40, y: 84, width: width + 2 * margin, height: height + 2 * margin)
    static let size = CGSize(width: 768, height: 676)
    static let ink = color(0x1c1b19)
    static let muted = color(0x6f6b63)
    static let line = color(0x3d3c38)
    static let dark = color(0x22211f)
    static let light = color(0xfaf7f1)

    static func color(_ rgb: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((rgb >> 16) & 255) / 255, green: CGFloat((rgb >> 8) & 255) / 255,
                blue: CGFloat(rgb & 255) / 255, alpha: 1)
    }

    static func text(_ string: String, size: CGFloat, weight: Int = 400, color: NSColor = ink,
                     alignment: NSTextAlignment = .left, truncate: Bool = false) -> NSAttributedString {
        let style = NSMutableParagraphStyle()
        style.alignment = alignment
        style.lineBreakMode = truncate ? .byTruncatingMiddle : .byWordWrapping
        return NSAttributedString(string: string, attributes: [
            .font: Geologica.font(size: size, weight: weight), .foregroundColor: color, .paragraphStyle: style
        ])
    }
}

class SetupSurface: NSView {
    override var isFlipped: Bool { true }
}

final class SetupWindow: NSWindow {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }
}

final class SetupCard: SetupSurface {
    let page = SetupSurface(frame: CGRect(x: SetupStyle.margin, y: SetupStyle.margin, width: SetupStyle.width, height: SetupStyle.height))
    var tab = "" { didSet { needsDisplay = true } }
    var tabX: CGFloat = 38 { didSet { needsDisplay = true } }
    var tabWidth: CGFloat = 80 { didSet { needsDisplay = true } }
    var top = false { didSet { needsDisplay = true } }

    init() {
        super.init(frame: SetupStyle.cardFrame)
        wantsLayer = true
        addSubview(page)
    }

    required init?(coder: NSCoder) { nil }

    override func draw(_ dirtyRect: NSRect) {
        let shape = FolderShape(bounds: page.frame, tabStart: tabX, tabWidth: tabWidth, tabShoulder: 6)
        shape.draw(fill: .white, line: SetupStyle.line.withAlphaComponent(0.9),
                   shadow: NSColor.black.withAlphaComponent(top ? 0.20 : 0.06))
        let text = SetupStyle.text(tab, size: 15, weight: 500)
        text.draw(at: CGPoint(x: page.frame.minX + tabX + 12, y: page.frame.minY + (34 - text.size().height) / 2))
    }
}

final class SetupButton: NSButton {
    enum Style {
        case primary, quiet, outline, keycap, close, language, tile(Int, Bool), segment(Bool), toggle(Bool, String)
    }

    let style: Style
    private let invoke: () -> Void
    private var tracking: NSTrackingArea?
    private let motion = Motion()
    private var hover: CGFloat = 0

    init(_ title: String, frame: CGRect, style: Style, invoke: @escaping () -> Void) {
        self.style = style
        self.invoke = invoke
        super.init(frame: frame)
        self.title = title
        isBordered = false
        focusRingType = .none
        target = self
        action = #selector(performAction)
        setAccessibilityLabel(title)
        if case .toggle(let on, _) = style {
            setAccessibilityRole(.checkBox)
            setAccessibilityValue(on ? 1 : 0)
        }
    }

    required init?(coder: NSCoder) { nil }
    override var isFlipped: Bool { true }

    @objc private func performAction() { invoke() }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.activeInKeyWindow, .inVisibleRect, .mouseEnteredAndExited], owner: self)
        tracking = area
        addTrackingArea(area)
    }

    override func mouseEntered(with event: NSEvent) { setHover(1) }
    override func mouseExited(with event: NSEvent) { setHover(0) }

    private func setHover(_ to: CGFloat) {
        let from = hover
        motion.start(duration: 0.14) { [weak self] time in
            self?.hover = from + (to - from) * Motion.ease(time)
            self?.needsDisplay = true
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        let ink = SetupStyle.ink, muted = SetupStyle.muted, dark = SetupStyle.dark
        let down = cell?.isHighlighted == true
        let pressed = bounds.insetBy(dx: down ? 1.5 : 0, dy: down ? 1.5 : 0)
        func label(_ color: NSColor) {
            let text = SetupStyle.text(title, size: 15, weight: 500, color: color, alignment: .center)
            text.draw(in: CGRect(x: 0, y: (bounds.height - text.size().height) / 2, width: bounds.width, height: text.size().height + 1))
        }
        func rounded(_ rect: CGRect, _ radius: CGFloat, fill: NSColor, border: NSColor? = nil, line: CGFloat = 1) {
            let path = NSBezierPath(roundedRect: rect.insetBy(dx: line / 2, dy: line / 2), xRadius: radius, yRadius: radius)
            fill.setFill()
            path.fill()
            if let border { border.setStroke(); path.lineWidth = line; path.stroke() }
        }
        switch style {
        case .primary:
            rounded(pressed, 22, fill: dark.blended(withFraction: hover, of: SetupStyle.color(0x3a3935)) ?? dark)
            label(SetupStyle.light)
        case .quiet, .segment(false):
            rounded(pressed, 22, fill: ink.withAlphaComponent(0.08 * hover))
            label(muted)
        case .segment(true):
            rounded(pressed.insetBy(dx: 3, dy: 3), 19, fill: dark)
            label(SetupStyle.light)
        case .outline, .keycap:
            let radius: CGFloat = bounds.height == bounds.width ? bounds.height / 2 : 12
            rounded(pressed, radius, fill: ink.withAlphaComponent(0.05 * hover),
                    border: SetupStyle.line.withAlphaComponent(0.22 + 0.25 * hover))
            label(ink)
        case .close:
            rounded(pressed, 13, fill: SetupStyle.light, border: SetupStyle.line, line: 1.4)
            if hover > 0 { rounded(pressed.insetBy(dx: 1, dy: 1), 12, fill: ink.withAlphaComponent(0.09 * hover)) }
            FolderGlyph.close.image(muted).draw(in: CGRect(x: 5, y: 5, width: 16, height: 16),
                                                from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
        case .language:
            rounded(pressed, 22, fill: ink.withAlphaComponent(0.05 * hover), border: SetupStyle.line.withAlphaComponent(0.22))
            let globe = NSImage(systemSymbolName: "globe", accessibilityDescription: nil)?
                .withSymbolConfiguration(.init(paletteColors: [ink]))
            globe?.draw(in: CGRect(x: 12, y: 12, width: 20, height: 20), from: .zero, operation: .sourceOver,
                        fraction: 1, respectFlipped: true, hints: nil)
        case .toggle(let on, let hint):
            let hasHint = !hint.isEmpty
            SetupStyle.text(title, size: 15, weight: 500).draw(at: CGPoint(x: 0, y: hasHint ? 3 : 12))
            if hasHint { SetupStyle.text(hint, size: 13, color: muted).draw(at: CGPoint(x: 0, y: 27)) }
            let track = CGRect(x: bounds.maxX - 48, y: bounds.midY - 14, width: 48, height: 28)
            rounded(track, 14, fill: on ? dark : ink.withAlphaComponent(0.12 + 0.05 * hover))
            rounded(CGRect(x: track.minX + (on ? 23 : 3), y: track.minY + 3, width: 22, height: 22), 11, fill: .white)
        case .tile(let index, let selected):
            let box = CGRect(x: 5, y: 5, width: bounds.width - 10, height: 100)
            let tile = NSBezierPath(roundedRect: box, xRadius: 14, yRadius: 14)
            let isDark = index == 1
            NSGraphicsContext.saveGraphicsState()
            tile.addClip()
            (isDark ? SetupStyle.color(0x1e1e1c) : SetupStyle.color(0xf4f1ea)).setFill()
            box.fill()
            if index == 2 {
                SetupStyle.color(0x1e1e1c).setFill()
                CGRect(x: box.midX, y: box.minY, width: box.width / 2, height: box.height).fill()
            }
            let aa = SetupStyle.text("Aa", size: 34, weight: 700, color: isDark ? SetupStyle.light : ink)
            let point = CGPoint(x: box.midX - aa.size().width / 2, y: box.midY - aa.size().height / 2)
            aa.draw(at: point)
            if index == 2 {
                NSBezierPath(rect: CGRect(x: box.midX, y: box.minY, width: box.width / 2, height: box.height)).addClip()
                SetupStyle.text("Aa", size: 34, weight: 700, color: SetupStyle.light).draw(at: point)
            }
            NSGraphicsContext.restoreGraphicsState()
            let ring = NSBezierPath(roundedRect: box.insetBy(dx: -4, dy: -4), xRadius: 18, yRadius: 18)
            dark.withAlphaComponent(selected ? 1 : hover * 0.25).setStroke()
            ring.lineWidth = 2
            ring.stroke()
            SetupStyle.text(title, size: 15, weight: 500, color: selected ? ink : muted, alignment: .center)
                .draw(in: CGRect(x: 0, y: 115, width: bounds.width, height: 24))
        }
        if window?.firstResponder === self, keyEquivalent.isEmpty {
            let ring = NSBezierPath(roundedRect: bounds.insetBy(dx: 1, dy: 1), xRadius: 10, yRadius: 10)
            SetupStyle.line.withAlphaComponent(0.45).setStroke()
            ring.lineWidth = 1
            ring.stroke()
        }
    }
}

struct SetupPose {
    var x: CGFloat = 0
    var y: CGFloat = 0
    var scale: CGFloat = 1
    var opacity: CGFloat = 1
    var angle: CGFloat = 0

    static func stack(_ index: Int, depth: Int) -> Self {
        guard depth > 0 else { return Self() }
        let angle: CGFloat = [-1.6, 1.4, -1.1, 1.8, -1.3][index]
        let shift: CGFloat = [-3, 3, -2, 3, -2][index]
        return Self(x: shift - SetupStyle.height * 0.9 * sin(angle * .pi / 180), y: -13 * CGFloat(depth),
                    scale: 1 - 0.012 * CGFloat(depth), angle: angle)
    }

    static func aside(_ index: Int) -> Self {
        let side: CGFloat = index.isMultiple(of: 2) ? -1 : 1
        return Self(x: side * SetupStyle.width * 0.62, y: 46, scale: 0.97, opacity: 0, angle: side * 9)
    }

    static func fan(_ index: Int) -> Self {
        Self(y: [-6, -16, -16, -6][index], scale: 0.985, angle: [-5, -2.5, 2.5, 5][index])
    }

    func moved(to other: Self, by progress: CGFloat, fade: CGFloat? = nil) -> Self {
        func mix(_ from: CGFloat, _ to: CGFloat) -> CGFloat { from + (to - from) * progress }
        return Self(x: mix(x, other.x), y: mix(y, other.y), scale: mix(scale, other.scale),
                    opacity: opacity + (other.opacity - opacity) * (fade ?? progress), angle: mix(angle, other.angle))
    }

    func apply(to card: SetupCard) {
        let pivot = SetupStyle.cardFrame.height / 2 - SetupStyle.margin - SetupStyle.height * 0.9
        // Move the cached layer instead of changing the view frame on every tick.
        let transform = CGAffineTransform(translationX: x, y: y)
            .translatedBy(x: 0, y: pivot)
            .rotated(by: -angle * .pi / 180).scaledBy(x: scale, y: scale).translatedBy(x: 0, y: -pivot)
        card.layer?.setAffineTransform(transform)
        card.alphaValue = opacity
        card.isHidden = opacity <= 0.001
    }
}
