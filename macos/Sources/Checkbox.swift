import AppKit

struct Checkbox {
    let scale: CGFloat
    let outline: NSColor
    let fill: NSColor
    let tick: NSColor

    var size: CGFloat { 15 * scale }
    var advance: CGFloat { 23 * scale }

    func draw(in rect: CGRect, checked: Bool) {
        if checked {
            fill.setFill()
            NSBezierPath(roundedRect: rect, xRadius: 4 * scale, yRadius: 4 * scale).fill()
            let mark = NSBezierPath()
            mark.move(to: CGPoint(x: rect.minX + size * 0.26, y: rect.minY + size * 0.52))
            mark.line(to: CGPoint(x: rect.minX + size * 0.43, y: rect.minY + size * 0.69))
            mark.line(to: CGPoint(x: rect.minX + size * 0.75, y: rect.minY + size * 0.33))
            mark.lineCapStyle = .round
            mark.lineJoinStyle = .round
            mark.lineWidth = 1.7 * scale
            tick.setStroke()
            mark.stroke()
        } else {
            let stroke = 1.5 * scale
            let box = NSBezierPath(roundedRect: rect.insetBy(dx: stroke / 2, dy: stroke / 2),
                                   xRadius: 3.5 * scale, yRadius: 3.5 * scale)
            box.lineWidth = stroke
            outline.setStroke()
            box.stroke()
        }
    }

    func attachment(checked: Bool) -> NSTextAttachment {
        let attachment = NSTextAttachment()
        attachment.image = NSImage(size: CGSize(width: advance, height: size), flipped: true) { _ in
            draw(in: CGRect(x: 0, y: 0, width: size, height: size), checked: checked)
            return true
        }
        attachment.bounds = CGRect(x: 0, y: -6 * scale, width: advance, height: size)
        return attachment
    }
}

extension NSAttributedString.Key {
    static let taskMarker = Self("NoteddTaskMarker")
}

struct TaskMarker {
    let offset: Int
    let checked: Bool
    let checkbox: Checkbox
}
