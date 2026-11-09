import AppKit

struct PreferenceColors {
    let ink: NSColor
    let muted: NSColor
    let border: NSColor
    let selected: NSColor
    let onSelected: NSColor
}

final class PreferencesPanel: NSView {
    enum Section: Int, CaseIterable {
        case general, shortcuts, colors

        var title: String {
            switch self {
            case .general: return "General"
            case .shortcuts: return "Shortcuts"
            case .colors: return "Colors"
            }
        }

        var labelWidth: CGFloat {
            switch self {
            case .general: return 170
            case .shortcuts: return 250
            case .colors: return 130
            }
        }
    }

    override var acceptsFirstResponder: Bool { true }
    private let scroll = NSScrollView()
    private let tabs = NSView()
    private let sectionMotion = Motion()
    private var picker: PreferenceChoice?
    private var choices: [String: PreferenceChoice] = [:]
    private var shownSection: Section?
    private(set) var section = Section.general
    var onSection: (() -> Void)?
    var onClose: (() -> Void)?

    override init(frame: NSRect) {
        super.init(frame: frame)
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder
        scroll.wantsLayer = true
        for view in [tabs, scroll] {
            view.translatesAutoresizingMaskIntoConstraints = false
            addSubview(view)
        }
        NSLayoutConstraint.activate([
            tabs.leadingAnchor.constraint(equalTo: leadingAnchor),
            tabs.topAnchor.constraint(equalTo: topAnchor),
            tabs.trailingAnchor.constraint(equalTo: trailingAnchor),
            tabs.heightAnchor.constraint(equalToConstant: 32),
            scroll.topAnchor.constraint(equalTo: tabs.bottomAnchor, constant: 18),
            scroll.leadingAnchor.constraint(equalTo: leadingAnchor),
            scroll.trailingAnchor.constraint(equalTo: trailingAnchor),
            scroll.bottomAnchor.constraint(equalTo: bottomAnchor)
        ])
    }

    required init?(coder: NSCoder) { nil }

    override func cancelOperation(_ sender: Any?) { onClose?() }

    override func keyDown(with event: NSEvent) {
        if event.keyCode == 53 { onClose?() }
        else { super.keyDown(with: event) }
    }

    func stepSection(_ delta: Int) {
        let sections = Section.allCases
        select(sections[(section.rawValue + delta + sections.count) % sections.count])
    }

    private func select(_ next: Section) {
        guard next != section else { return }
        section = next
        onSection?()
    }

    func update(_ entries: [(String, NSView)], colors: PreferenceColors, preserveScroll: Bool = true) {
        let previous = shownSection
        let changedSection = previous != nil && previous != section
        let oldScroll = preserveScroll ? scroll.contentView.bounds.origin.y : 0
        if let picker {
            picker.colors = colors
            picker.select(section.rawValue, animated: changedSection)
        } else {
            let bar = PreferenceChoice(Section.allCases.map(\.title), selected: section.rawValue, colors: colors) { [weak self] index in
                if let next = Section(rawValue: index) { self?.select(next) }
            }
            bar.frame = CGRect(origin: .zero, size: bar.intrinsicContentSize)
            tabs.addSubview(bar)
            picker = bar
        }
        shownSection = section

        let oldChoices = changedSection ? [:] : choices.mapValues(\.indicator)
        choices.removeAll()
        let document = PreferencesDocument()
        scroll.documentView = document
        document.translatesAutoresizingMaskIntoConstraints = false
        let available = max(420, bounds.width) - 10
        let labelWidth = section.labelWidth * (available < 525 ? 0.8 : 1)
        let stack = NSStackView(views: entries.map { label, value in
            let caption = Self.label(label, color: colors.muted, size: 15)
            caption.lineBreakMode = .byTruncatingTail
            caption.widthAnchor.constraint(equalToConstant: labelWidth).isActive = true
            let vertical = !label.isEmpty && value.fittingSize.width + labelWidth > available
            let row = NSStackView(views: label.isEmpty ? [value] : [caption, value])
            row.orientation = vertical ? .vertical : .horizontal
            row.alignment = vertical ? .leading : .centerY
            row.spacing = vertical ? 6 : 0
            row.heightAnchor.constraint(greaterThanOrEqualToConstant: section == .shortcuts ? 32 : 43).isActive = true
            if let choice = value as? PreferenceChoice { choices[label] = choice }
            return row
        })
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = section == .shortcuts ? 5 : 7
        stack.translatesAutoresizingMaskIntoConstraints = false
        document.addSubview(stack)
        for row in stack.arrangedSubviews { row.widthAnchor.constraint(lessThanOrEqualTo: stack.widthAnchor).isActive = true }
        NSLayoutConstraint.activate([
            document.widthAnchor.constraint(equalTo: scroll.contentView.widthAnchor),
            stack.leadingAnchor.constraint(equalTo: document.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: document.trailingAnchor, constant: -10),
            stack.topAnchor.constraint(equalTo: document.topAnchor),
            stack.bottomAnchor.constraint(equalTo: document.bottomAnchor, constant: -8)
        ])
        layoutSubtreeIfNeeded()
        for (label, choice) in choices {
            if let origin = oldChoices[label] { choice.animate(from: origin) }
        }
        scroll.contentView.scroll(to: CGPoint(x: 0, y: min(oldScroll, max(0, document.bounds.height - scroll.contentSize.height))))
        scroll.reflectScrolledClipView(scroll.contentView)
        if changedSection {
            let direction: CGFloat = section.rawValue > (previous?.rawValue ?? 0) ? 1 : -1
            sectionMotion.start(duration: 0.17) { [weak self] time in
                let p = Motion.ease(time)
                CATransaction.begin()
                CATransaction.setDisableActions(true)
                self?.scroll.layer?.setAffineTransform(CGAffineTransform(translationX: 8 * direction * (1 - p), y: 0))
                self?.scroll.layer?.opacity = Float(0.76 + 0.24 * p)
                CATransaction.commit()
            }
        }
    }

    static func label(_ title: String, color: NSColor, size: CGFloat = 13) -> NSTextField {
        let field = NSTextField(labelWithString: title)
        field.font = Markdown.geologica(size: size, weight: 500)
        field.textColor = color
        return field
    }

    static func choice(_ labels: [String], selected: Int?, colors: PreferenceColors, action: @escaping (Int) -> Void) -> NSView {
        PreferenceChoice(labels, selected: selected, colors: colors, action: action)
    }

    static func buttons(_ buttons: [(String, () -> Void)], colors: PreferenceColors) -> NSView {
        let views = buttons.map { title, action in
            let button = PreferenceButton(title, ink: colors.ink, padding: 30, action: action)
            button.paint = { bounds, hover in
                let shape = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 16, yRadius: 16)
                colors.ink.withAlphaComponent(0.08 * hover).setFill()
                shape.fill()
                colors.border.withAlphaComponent(0.4).setStroke()
                shape.stroke()
            }
            return button
        }
        return group(views, spacing: 6)
    }

    static func swatches(_ swatches: [(String, NSColor)], selected: Int?, colors: PreferenceColors,
                         action: @escaping (Int) -> Void) -> NSView {
        let buttons = swatches.enumerated().map { index, swatch in
            let button = PreferenceButton("", ink: colors.ink, padding: 30) { action(index) }
            button.setAccessibilityLabel(swatch.0)
            button.toolTip = swatch.0
            button.setAccessibilityValue(selected == index ? 1 : 0)
            button.paint = { bounds, hover in
                let ring = NSBezierPath(ovalIn: bounds.insetBy(dx: 0.8, dy: 1.8))
                colors.ink.withAlphaComponent(selected == index ? 1 : 0.3 * hover).setStroke()
                ring.lineWidth = selected == index ? 1.6 : 1.2
                ring.stroke()
                let disk = NSBezierPath(ovalIn: CGRect(x: bounds.midX - 11, y: bounds.midY - 11, width: 22, height: 22))
                swatch.1.setFill()
                disk.fill()
                colors.border.withAlphaComponent(0.6).setStroke()
                disk.stroke()
            }
            return button
        }
        return group(buttons, spacing: 8)
    }

    static func shortcut(_ keys: [String]?, recording: Bool, colors: PreferenceColors, action: @escaping () -> Void) -> NSView {
        let font = Markdown.geologica(size: 13, weight: 500)
        let caption = recording ? "Press shortcut…" : "Not set"
        let widths = keys?.map { max(10, ceil(($0 as NSString).size(withAttributes: [.font: font]).width)) + 18 } ?? []
        let width = keys == nil || recording ? 144 : widths.reduce(0, +) + CGFloat(max(0, widths.count - 1)) * 5
        let button = PreferenceButton("", ink: colors.ink, padding: width, action: action)
        button.setAccessibilityLabel(recording || keys == nil ? caption : keys?.joined(separator: " + "))
        button.paint = { bounds, hover in
            func text(_ label: String, at rect: CGRect, color: NSColor) {
                let string = NSAttributedString(string: label, attributes: [.font: font, .foregroundColor: color])
                let size = string.size()
                string.draw(at: CGPoint(x: rect.midX - size.width / 2, y: rect.midY - size.height / 2))
            }
            if let keys, !recording {
                var x: CGFloat = 0
                for (key, width) in zip(keys, widths) {
                    let face = CGRect(x: x + 0.5, y: 4, width: width - 1, height: 25)
                    colors.border.withAlphaComponent(0.45).setFill()
                    NSBezierPath(roundedRect: face.offsetBy(dx: 0, dy: -2), xRadius: 7, yRadius: 7).fill()
                    let cap = NSBezierPath(roundedRect: face, xRadius: 7, yRadius: 7)
                    // The underlying paper shows through the face tint.
                    colors.onSelected.blended(withFraction: 0.08 + 0.04 * hover, of: colors.ink)?.setFill()
                    cap.fill()
                    colors.border.withAlphaComponent(0.35).setStroke()
                    cap.stroke()
                    text(key, at: face, color: colors.ink)
                    x += width + 5
                }
            } else {
                let box = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.6, dy: 3), xRadius: 8, yRadius: 8)
                colors.ink.withAlphaComponent(recording ? 0.08 : 0.04 * hover).setFill()
                box.fill()
                colors.border.withAlphaComponent(recording ? 1 : 0.6).setStroke()
                box.lineWidth = 1.2
                box.setLineDash([3, 3], count: 2, phase: 0)
                box.stroke()
                text(caption, at: bounds, color: recording ? colors.ink : colors.muted)
            }
        }
        return button
    }

    private static func group(_ buttons: [PreferenceButton], spacing: CGFloat) -> NSStackView {
        let group = NSStackView(views: buttons)
        group.orientation = .horizontal
        group.spacing = spacing
        return group
    }
}

private final class PreferencesDocument: NSView {
    override var isFlipped: Bool { true }
}

private final class PreferenceChoice: NSView {
    var colors: PreferenceColors { didSet { refresh() } }
    private var selected: Int?
    private let buttons: [PreferenceButton]
    private let motion = Motion()
    private(set) var indicator = CGRect.zero
    private var target: CGRect {
        guard let selected, buttons.indices.contains(selected) else { return .zero }
        return buttons[selected].frame.insetBy(dx: 3, dy: 3)
    }

    init(_ labels: [String], selected: Int?, colors: PreferenceColors, action: @escaping (Int) -> Void) {
        self.colors = colors
        self.selected = selected
        buttons = labels.enumerated().map { index, title in
            PreferenceButton(title, ink: colors.muted, padding: 30) { action(index) }
        }
        super.init(frame: .zero)
        var x: CGFloat = 0
        for button in buttons {
            button.frame = CGRect(x: x, y: 0, width: button.intrinsicContentSize.width, height: 32)
            addSubview(button)
            x += button.frame.width
        }
        indicator = target
        refresh()
    }

    required init?(coder: NSCoder) { nil }

    override var intrinsicContentSize: NSSize { NSSize(width: buttons.last?.frame.maxX ?? 0, height: 32) }

    override func draw(_ dirtyRect: NSRect) {
        let shell = NSBezierPath(roundedRect: bounds.insetBy(dx: 0.5, dy: 0.5), xRadius: 16, yRadius: 16)
        colors.ink.withAlphaComponent(0.05).setFill()
        shell.fill()
        colors.border.withAlphaComponent(0.35).setStroke()
        shell.stroke()
        if !indicator.isEmpty {
            colors.selected.setFill()
            NSBezierPath(roundedRect: indicator, xRadius: 13, yRadius: 13).fill()
        }
    }

    func select(_ index: Int, animated: Bool) {
        guard index != selected else { return }
        selected = index
        if animated { animate(from: indicator) }
        else { indicator = target; refresh() }
    }

    func animate(from origin: CGRect) {
        let end = target
        guard !origin.isEmpty, origin != end else { return }
        motion.start(duration: 0.24) { [weak self] time in
            let amount = time == 1 ? 1 : 1 - exp(-4.32 * time)
            self?.indicator = origin.interpolated(to: end, by: amount)
            self?.refresh()
        }
    }

    private func refresh() {
        needsDisplay = true
        for (index, button) in buttons.enumerated() {
            let cover = indicator.intersection(button.frame.insetBy(dx: 3, dy: 3))
            let fraction = cover.isNull ? 0 : cover.width / max(1, button.frame.width - 6)
            button.setInk(colors.muted.blended(withFraction: fraction, of: colors.onSelected) ?? colors.muted)
            button.setAccessibilityValue(index == selected ? 1 : 0)
        }
    }
}

private final class PreferenceButton: NSButton {
    private let handler: () -> Void
    private let text: String
    private let width: CGFloat
    private static let font = Markdown.geologica(size: 13, weight: 500)
    private var tracking: NSTrackingArea?
    private var hover: CGFloat = 0
    private let hoverMotion = Motion()
    var paint: ((CGRect, CGFloat) -> Void)?

    init(_ title: String, ink: NSColor, padding: CGFloat, action: @escaping () -> Void) {
        handler = action
        text = title
        width = ceil((title as NSString).size(withAttributes: [.font: Self.font]).width) + padding
        super.init(frame: .zero)
        target = self
        self.action = #selector(activate)
        isBordered = false
        setInk(ink)
    }

    required init?(coder: NSCoder) { nil }

    override var intrinsicContentSize: NSSize { NSSize(width: width, height: 32) }

    func setInk(_ ink: NSColor) {
        attributedTitle = NSAttributedString(string: text, attributes: [.font: Self.font, .foregroundColor: ink])
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(rect: .zero, options: [.activeInKeyWindow, .inVisibleRect, .mouseEnteredAndExited], owner: self)
        addTrackingArea(area)
        tracking = area
    }

    override func mouseEntered(with event: NSEvent) { animateHover(to: 1) }
    override func mouseExited(with event: NSEvent) { animateHover(to: 0) }

    private func animateHover(to target: CGFloat) {
        let from = hover
        hoverMotion.start(duration: 0.12) { [weak self] time in
            self?.hover = from + (target - from) * Motion.ease(time)
            self?.needsDisplay = true
        }
    }

    override func draw(_ dirtyRect: NSRect) {
        paint?(bounds, hover)
        if !text.isEmpty { super.draw(dirtyRect) }
    }

    @objc private func activate() { handler() }
}
