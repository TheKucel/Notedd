import AppKit

final class FolderPopup {
    struct Command {
        let title: String
        let keys: String
        let icon: FolderGlyph
        var enabled = true
        let run: () -> Void
    }

    struct Swatch {
        let name: String
        let color: NSColor
        let choose: () -> Void
    }

    private var panel: NSPanel?
    private var content: PopupContent?
    var isPaletteOpen: Bool { content?.swatches.isEmpty == false }

    func openPalette(owner: NSWindow, anchor: CGPoint, paper: NSColor, colors: PreferenceColors,
                     swatches: [Swatch], selected: Int, delete: Command? = nil) {
        let size = CGSize(width: max(184, CGFloat(swatches.count) * 36 + 8), height: delete == nil ? 44 : 82)
        let view = PopupContent(size: size, paper: paper, colors: colors)
        view.swatches = swatches
        view.selected = selected
        view.commands = delete.map { [$0] } ?? []
        view.setAccessibilityLabel("Note color")
        open(owner: owner, anchor: anchor, centered: true, view: view)
    }

    func openMenu(owner: NSWindow, anchor: CGPoint, paper: NSColor, colors: PreferenceColors, commands: [Command]) {
        let font = Markdown.geologica(size: 13)
        let label = commands.map { ($0.title as NSString).size(withAttributes: [.font: font]).width }.max() ?? 0
        let keys = commands.map { ($0.keys as NSString).size(withAttributes: [.font: font]).width }.max() ?? 0
        let size = CGSize(width: max(244, ceil(label + keys + 82)), height: 22 + CGFloat(commands.count) * 32)
        let view = PopupContent(size: size, paper: paper, colors: colors)
        view.commands = commands
        open(owner: owner, anchor: anchor, centered: false, view: view)
    }

    private func open(owner: NSWindow, anchor: CGPoint, centered: Bool, view: PopupContent) {
        close()
        let work = NSScreen.screens.first { $0.frame.contains(anchor) }?.visibleFrame ?? owner.screen?.visibleFrame ?? owner.frame
        let body = view.bodySize
        let preferredX = centered ? anchor.x - body.width / 2 : anchor.x + 2
        let preferredY = anchor.y - body.height - 4
        let y = preferredY >= work.minY ? preferredY : anchor.y + 4
        let x = min(max(work.minX, preferredX), max(work.minX, work.maxX - body.width))
        let bottom = min(max(work.minY, y), max(work.minY, work.maxY - body.height))
        let origin = CGPoint(x: x - PopupContent.margin, y: bottom - PopupContent.margin)
        let popup = NSPanel(contentRect: CGRect(origin: origin, size: view.frame.size),
                            styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        popup.isOpaque = false
        popup.backgroundColor = .clear
        popup.hasShadow = false
        popup.hidesOnDeactivate = true
        popup.animationBehavior = .none
        popup.acceptsMouseMovedEvents = true
        popup.isReleasedWhenClosed = false
        popup.contentView = view
        view.dismiss = { [weak self] in self?.close() }
        owner.addChildWindow(popup, ordered: .above)
        panel = popup
        content = view
        view.appear()
        popup.orderFront(nil)
    }

    func close() {
        guard let panel else { return }
        panel.parent?.removeChildWindow(panel)
        panel.orderOut(nil)
        self.panel = nil
        content = nil
    }

    func handle(_ event: NSEvent) -> NSEvent? {
        guard let panel, let content else { return event }
        switch event.type {
        case .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel:
            if event.window !== panel { close() }
        case .keyDown:
            if event.modifierFlags.intersection(Shortcut.supported).isEmpty, content.handlePaletteKey(event.keyCode) { return nil }
            switch event.keyCode {
            case 53: close(); return nil
            case 123, 126: content.step(-1); return nil
            case 124, 125: content.step(1); return nil
            case 36, 49, 76: content.activateHot(); return nil
            default: close()
            }
        default: break
        }
        return event
    }
}

private final class PopupContent: NSView, NSViewToolTipOwner {
    static let margin: CGFloat = 19
    let bodySize: CGSize
    let paper: NSColor
    let colors: PreferenceColors
    var swatches: [FolderPopup.Swatch] = []
    var commands: [FolderPopup.Command] = []
    var selected = 0
    var dismiss: (() -> Void)?
    private var tracking: NSTrackingArea?
    private var hot: Int?
    private var pressed: Int?
    private let entry = Motion()
    private let hover = Motion()
    private var entryProgress: CGFloat = 1
    private var swatchScales: [CGFloat] = []

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { false }
    private var body: CGRect {
        CGRect(x: Self.margin, y: Self.margin - 6 * (1 - entryProgress), width: bodySize.width, height: bodySize.height)
    }

    init(size: CGSize, paper: NSColor, colors: PreferenceColors) {
        bodySize = size
        self.paper = paper
        self.colors = colors
        super.init(frame: CGRect(origin: .zero, size: CGSize(width: size.width + 2 * Self.margin, height: size.height + 2 * Self.margin)))
        wantsLayer = true
    }

    required init?(coder: NSCoder) { nil }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    func appear() {
        swatchScales = Array(repeating: 1, count: swatches.count)
        for index in swatches.indices {
            let point = center(index)
            addToolTip(CGRect(x: point.x - 16, y: point.y - 16, width: 32, height: 32),
                       owner: self, userData: nil)
        }
        entry.start(duration: 0.15) { [weak self] time in
            guard let self else { return }
            entryProgress = Motion.ease(time)
            alphaValue = entryProgress
            needsDisplay = true
        }
    }

    func view(_ view: NSView, stringForToolTip tag: NSView.ToolTipTag, point: NSPoint,
              userData: UnsafeMutableRawPointer?) -> String {
        swatches.indices.first { abs(center($0).x - point.x) <= 16 }.map { swatches[$0].name } ?? ""
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.activeAlways, .inVisibleRect, .mouseMoved, .mouseEnteredAndExited], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    private func row(_ index: Int) -> CGRect {
        let y: CGFloat = swatches.isEmpty ? 6 + CGFloat(index) * 32 + (index >= 5 ? 10 : 0) : 48
        return CGRect(x: body.minX + 6, y: body.minY + y, width: body.width - 12, height: 32)
    }

    private func center(_ index: Int) -> CGPoint {
        CGPoint(x: body.midX - CGFloat(swatches.count - 1) * 18 + CGFloat(index) * 36, y: body.minY + 22)
    }

    private func hit(_ event: NSEvent) -> Int? {
        let point = convert(event.locationInWindow, from: nil)
        if let index = swatches.indices.first(where: { hypot(center($0).x - point.x, center($0).y - point.y) <= 16 }) {
            return index
        }
        return commands.indices.first { commands[$0].enabled && row($0).contains(point) }.map { $0 + swatches.count }
    }

    private func setHot(_ value: Int?) {
        guard hot != value else { return }
        hot = value
        if !swatches.isEmpty {
            let from = swatchScales
            hover.start(duration: 0.14) { [weak self] time in
                guard let self else { return }
                swatchScales = from.enumerated().map { index, scale in
                    scale + ((value == index ? 1.12 : 1) - scale) * Motion.ease(time)
                }
                needsDisplay = true
            }
        }
        needsDisplay = true
    }

    override func mouseMoved(with event: NSEvent) { setHot(hit(event)) }
    override func mouseEntered(with event: NSEvent) { setHot(hit(event)) }
    override func mouseExited(with event: NSEvent) { setHot(nil) }
    override func mouseDown(with event: NSEvent) { pressed = hit(event); setHot(pressed); needsDisplay = true }
    override func mouseDragged(with event: NSEvent) { setHot(hit(event)) }
    override func mouseUp(with event: NSEvent) {
        let target = hit(event)
        if pressed == target, let target { activate(target) }
        pressed = nil
        needsDisplay = true
    }

    func step(_ amount: Int) {
        let enabled = Array(swatches.indices) + commands.indices.filter { commands[$0].enabled }.map { $0 + swatches.count }
        guard !enabled.isEmpty else { return }
        let start = hot.flatMap { enabled.firstIndex(of: $0) } ?? (amount > 0 ? -1 : 0)
        setHot(enabled[(start + amount + enabled.count) % enabled.count])
    }

    func activateHot() {
        if let hot { activate(hot) }
    }

    func handlePaletteKey(_ code: UInt16) -> Bool {
        guard !swatches.isEmpty else { return false }
        switch code {
        case 123, 126: activate((selected + swatches.count - 1) % swatches.count)
        case 124, 125: activate((selected + 1) % swatches.count)
        case 115: activate(0)
        case 119: activate(swatches.count - 1)
        case 36, 49, 76: dismiss?()
        default: return false
        }
        return true
    }

    private func activate(_ index: Int) {
        if swatches.indices.contains(index) {
            selected = index
            swatches[index].choose()
            needsDisplay = true
        } else if commands.indices.contains(index - swatches.count) {
            let command = commands[index - swatches.count]
            dismiss?()
            if command.enabled { command.run() }
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        let outline = NSBezierPath(roundedRect: body.insetBy(dx: 0.75, dy: 0.75), xRadius: 14, yRadius: 14)
        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow()
        shadow.shadowBlurRadius = 14
        shadow.shadowOffset = CGSize(width: 0, height: -5)
        shadow.shadowColor = NSColor.black.withAlphaComponent(0.27)
        shadow.set()
        paper.setFill()
        outline.fill()
        NSGraphicsContext.restoreGraphicsState()
        colors.border.withAlphaComponent(swatches.isEmpty ? 0.55 : 1).setStroke()
        outline.lineWidth = swatches.isEmpty ? 1 : 1.5
        outline.stroke()

        for (index, swatch) in swatches.enumerated() {
            let point = center(index)
            let radius = 10 * (swatchScales.indices.contains(index) ? swatchScales[index] : 1)
            let chip = NSBezierPath(ovalIn: CGRect(x: point.x - radius, y: point.y - radius, width: radius * 2, height: radius * 2))
            swatch.color.setFill()
            chip.fill()
            colors.border.setStroke()
            chip.lineWidth = 1.5
            chip.stroke()
            if index == selected {
                let ring = NSBezierPath(ovalIn: CGRect(x: point.x - 15.25, y: point.y - 15.25, width: 30.5, height: 30.5))
                ring.lineWidth = 1.5
                ring.stroke()
            }
        }
        if !commands.isEmpty && (!swatches.isEmpty || commands.count > 5) {
            let y = swatches.isEmpty ? row(5).minY - 5 : body.minY + 44
            colors.ink.withAlphaComponent(0.15).setFill()
            CGRect(x: body.minX + 12, y: y, width: body.width - 24, height: 1).fill()
        }
        let font = Markdown.geologica(size: 13)
        for (index, command) in commands.enumerated() {
            let row = row(index)
            let key = index + swatches.count
            if key == hot {
                colors.ink.withAlphaComponent(key == pressed ? 0.15 : 0.09).setFill()
                NSBezierPath(roundedRect: row.insetBy(dx: 0, dy: 1), xRadius: 7, yRadius: 7).fill()
            }
            let ink = colors.ink.withAlphaComponent(command.enabled ? 1 : 0.34)
            command.icon.image(ink).draw(in: CGRect(x: row.minX + 9, y: row.midY - 8, width: 16, height: 16),
                                         from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: nil)
            let title = NSAttributedString(string: command.title, attributes: [.font: font, .foregroundColor: ink])
            title.draw(at: CGPoint(x: row.minX + 36, y: row.midY - title.size().height / 2))
            let keys = NSAttributedString(string: command.keys, attributes: [.font: font,
                .foregroundColor: colors.muted.withAlphaComponent(command.enabled ? 0.85 : 0.34)])
            keys.draw(at: CGPoint(x: row.maxX - 10 - keys.size().width, y: row.midY - keys.size().height / 2))
        }
    }
}
