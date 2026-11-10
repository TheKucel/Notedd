import AppKit

final class Motion {
    private var timer: Timer?

    static var enabled: Bool { !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    static func ease(_ progress: CGFloat) -> CGFloat {
        let remaining = 1 - min(1, max(0, progress))
        return 1 - remaining * remaining * remaining
    }

    func cancel() {
        timer?.invalidate()
        timer = nil
    }

    func start(duration: TimeInterval, update: @escaping (CGFloat) -> Void, completion: (() -> Void)? = nil) {
        cancel()
        guard Self.enabled, duration > 0 else {
            update(1)
            completion?()
            return
        }
        let start = ProcessInfo.processInfo.systemUptime
        update(0)
        let tick = Timer(timeInterval: 1 / 60, repeats: true) { [weak self] timer in
            let elapsed = ProcessInfo.processInfo.systemUptime - start
            let progress = min(1, elapsed / duration)
            if progress == 1 { self?.cancel() }
            update(progress)
            if progress == 1 { completion?() }
        }
        timer = tick
        RunLoop.main.add(tick, forMode: .common)
    }

    deinit { cancel() }
}

extension CGRect {
    func interpolated(to target: CGRect, by progress: CGFloat) -> CGRect {
        CGRect(x: minX + (target.minX - minX) * progress,
               y: minY + (target.minY - minY) * progress,
               width: width + (target.width - width) * progress,
               height: height + (target.height - height) * progress)
    }
}

struct WindowAppearance {
    enum Direction: String, CaseIterable {
        case bottom = "Bottom", right = "Right", left = "Left", top = "Top", inPlace = "In Place"
    }

    enum Speed: String, CaseIterable {
        case fast = "Fast", normal = "Normal", slow = "Slow"

        var duration: TimeInterval {
            switch self {
            case .fast: return 0.20
            case .normal: return 0.32
            case .slow: return 0.50
            }
        }
    }

    var direction: Direction = .bottom
    var speed: Speed = .normal
    var offScreen = false

    func hiddenFrame(from frame: CGRect, on screen: CGRect) -> CGRect {
        let distance: CGFloat = 90
        switch direction {
        case .bottom: return frame.offsetBy(dx: 0, dy: offScreen ? min(0, screen.minY - frame.maxY - 8) : -distance)
        case .right: return frame.offsetBy(dx: offScreen ? max(0, screen.maxX - frame.minX + 8) : distance, dy: 0)
        case .left: return frame.offsetBy(dx: offScreen ? min(0, screen.minX - frame.maxX - 8) : -distance, dy: 0)
        case .top: return frame.offsetBy(dx: 0, dy: offScreen ? max(0, screen.maxY - frame.minY + 8) : distance)
        case .inPlace: return frame
        }
    }
}

struct FolderCorners: Equatable {
    var left: CGFloat = 20
    var right: CGFloat = 20
    var bottom: CGFloat = 20

    func interpolated(to target: Self, by progress: CGFloat) -> Self {
        Self(left: left + (target.left - left) * progress,
             right: right + (target.right - right) * progress,
             bottom: bottom + (target.bottom - bottom) * progress)
    }
}

enum WindowMagnet {
    static let side: CGFloat = 18
    static let top: CGFloat = 10
    static let bottom: CGFloat = 25

    static func paper(in frame: CGRect) -> CGRect {
        CGRect(x: frame.minX + side, y: frame.minY + bottom,
               width: frame.width - 2 * side, height: frame.height - top - bottom)
    }

    private static func origin(_ start: CGFloat, _ length: CGFloat, inside range: ClosedRange<CGFloat>,
                               apply: (CGFloat) -> CGFloat) -> CGFloat {
        let near = start - range.lowerBound
        let far = range.upperBound - start - length
        if near < far { return range.lowerBound + apply(near) }
        return range.upperBound - length - apply(far)
    }

    static func pull(_ frame: CGRect, toward work: CGRect) -> CGRect {
        adjusted(frame, within: work) { gap in
            if gap <= 0 { return 0 }
            return gap < 64 ? gap * gap / 64 : gap
        }
    }

    static func snap(_ frame: CGRect, to work: CGRect) -> CGRect {
        adjusted(frame, within: work) { $0 < 16 ? 0 : $0 }
    }

    private static func adjusted(_ frame: CGRect, within work: CGRect, apply: (CGFloat) -> CGFloat) -> CGRect {
        let paper = paper(in: frame)
        let x = paper.width <= work.width ? origin(paper.minX, paper.width, inside: work.minX...work.maxX, apply: apply) - side : frame.minX
        let y = paper.height <= work.height ? origin(paper.minY, paper.height, inside: work.minY...work.maxY, apply: apply) - bottom : frame.minY
        return CGRect(origin: CGPoint(x: x, y: y), size: frame.size)
    }

    static func corners(_ frame: CGRect, in work: CGRect) -> FolderCorners {
        let paper = paper(in: frame)
        func radius(_ gap: CGFloat) -> CGFloat { min(20, max(0, gap) * 20 / 12) }
        return FolderCorners(left: radius(paper.minX - work.minX), right: radius(work.maxX - paper.maxX),
                             bottom: radius(paper.minY - work.minY))
    }
}
