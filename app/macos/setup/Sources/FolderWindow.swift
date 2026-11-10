import AppKit

final class FolderWindow: NSWindow {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }

    private let motion = Motion()
    private var surface: NSView?
    private var home: CGRect?
    private var slide: Slide?
    private(set) var hiding = false
    var presentation = WindowAppearance()
    var magnetsEnabled = true
    var onCorners: ((FolderCorners) -> Void)?
    var onPlacement: (() -> Void)?
    var onCancel: (() -> Void)?
    var onDismiss: (() -> Void)?

    private struct Slide {
        let base: CGRect
        let canvas: CGRect
        var origin: CGPoint
        var opacity: CGFloat
    }

    private var restingFrame: CGRect { slide?.base ?? home ?? frame }

    func installSurface(_ view: NSView) {
        let container = NSView(frame: CGRect(origin: .zero, size: frame.size))
        container.wantsLayer = true
        contentView = container
        view.frame = container.bounds
        view.autoresizingMask = [.width, .height]
        container.addSubview(view)
        surface = view
    }

    override func cancelOperation(_ sender: Any?) {
        if let onCancel { onCancel() } else { super.cancelOperation(sender) }
    }

    private func display(for frame: CGRect) -> NSScreen? {
        NSScreen.screens.max {
            let a = $0.frame.intersection(WindowMagnet.paper(in: frame))
            let b = $1.frame.intersection(WindowMagnet.paper(in: frame))
            return a.width * a.height < b.width * b.height
        } ?? NSScreen.main
    }

    private func magnetFrame(for screen: NSScreen) -> CGRect {
        let visible = screen.visibleFrame
        return CGRect(x: visible.minX, y: screen.frame.minY,
                      width: visible.width, height: visible.maxY - screen.frame.minY)
    }

    func refreshCorners() {
        let corners = display(for: restingFrame).map { WindowMagnet.corners(restingFrame, in: magnetFrame(for: $0)) }
        onCorners?(magnetsEnabled ? corners ?? FolderCorners() : FolderCorners())
    }

    func reveal() {
        if isVisible && !hiding {
            if !isKeyWindow { makeKeyAndOrderFront(nil) }
            return
        }
        let base = restingFrame
        let hidden = presentation.hiddenFrame(from: base, on: display(for: base)?.frame ?? base)
        let fade = !presentation.offScreen || presentation.direction == .inPlace
        let from = slide?.origin ?? (isVisible ? frame.origin : hidden.origin)
        let opacity = slide?.opacity ?? (fade ? 0 : 1)
        hiding = false
        if !Motion.enabled {
            motion.cancel()
            restoreSurface(at: base)
            makeKeyAndOrderFront(nil)
            return
        }
        prepareSlide(base: base, hidden: hidden, from: from, opacity: opacity)
        if !isVisible || !isKeyWindow { makeKeyAndOrderFront(nil) }
        animateSlide(to: base.origin, opacity: 1, duration: presentation.speed.duration) { [weak self] in
            self?.restoreSurface(at: base)
            self?.refreshCorners()
        }
    }

    func dismiss() {
        guard isVisible, !hiding else { return }
        let base = restingFrame
        if !Motion.enabled {
            motion.cancel()
            orderOut(nil)
            restoreSurface(at: base)
            onDismiss?()
            return
        }
        let hidden = presentation.hiddenFrame(from: base, on: display(for: base)?.frame ?? base)
        let fade = !presentation.offScreen || presentation.direction == .inPlace
        let from = slide?.origin ?? frame.origin
        let opacity = slide?.opacity ?? 1
        hiding = true
        prepareSlide(base: base, hidden: hidden, from: from, opacity: opacity)
        animateSlide(to: hidden.origin, opacity: fade ? 0 : 1, duration: presentation.speed.duration * 0.85) { [weak self] in
            guard let self else { return }
            orderOut(nil)
            restoreSurface(at: base)
            hiding = false
            onDismiss?()
        }
    }

    // Keep the native window and its backing buffer still while the folder is moving.
    private func prepareSlide(base: CGRect, hidden: CGRect, from: CGPoint, opacity: CGFloat) {
        let canvas = slide?.canvas.union(base).union(hidden) ??
            base.union(hidden).union(CGRect(origin: from, size: base.size))
        slide = Slide(base: base, canvas: canvas, origin: from, opacity: opacity)
        home = nil
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        surface?.autoresizingMask = []
        if frame != canvas { setFrame(canvas, display: false) }
        applySlide()
        CATransaction.commit()
    }

    private func animateSlide(to origin: CGPoint, opacity: CGFloat, duration: TimeInterval, completion: @escaping () -> Void) {
        guard let start = slide else { return }
        motion.start(duration: duration) { [weak self] time in
            guard let self else { return }
            let progress = Motion.ease(time)
            slide?.origin = CGPoint(x: start.origin.x + (origin.x - start.origin.x) * progress,
                                    y: start.origin.y + (origin.y - start.origin.y) * progress)
            slide?.opacity = start.opacity + (opacity - start.opacity) * Motion.ease(time * 1.3)
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            applySlide()
            CATransaction.commit()
        } completion: { completion() }
    }

    private func applySlide() {
        guard let slide, let surface else { return }
        let anchor = CGPoint(x: slide.base.minX - slide.canvas.minX, y: slide.base.minY - slide.canvas.minY)
        let surfaceFrame = CGRect(origin: anchor, size: slide.base.size)
        if surface.frame != surfaceFrame { surface.frame = surfaceFrame }
        surface.layer?.setAffineTransform(CGAffineTransform(translationX: slide.origin.x - slide.base.minX,
                                                            y: slide.origin.y - slide.base.minY))
        surface.layer?.opacity = Float(slide.opacity)
    }

    private func restoreSurface(at destination: CGRect) {
        let changedSize = frame.size != destination.size
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        slide = nil
        home = nil
        surface?.autoresizingMask = []
        if frame != destination { setFrame(destination, display: false) }
        surface?.layer?.setAffineTransform(.identity)
        surface?.layer?.opacity = 1
        surface?.frame = CGRect(origin: .zero, size: destination.size)
        surface?.autoresizingMask = [.width, .height]
        CATransaction.commit()
        if changedSize { contentView?.layoutSubtreeIfNeeded() }
    }

    private func interruptPlacement() {
        motion.cancel()
        if let slide { restoreSurface(at: CGRect(origin: slide.origin, size: slide.base.size)) }
        home = nil
        hiding = false
    }

    func dragFolder(with event: NSEvent, click: (() -> Void)? = nil) {
        let start = convertPoint(toScreen: event.locationInWindow)
        var original = frame
        var moved = false
        var work: CGRect?
        while let event = nextEvent(matching: [.leftMouseDragged, .leftMouseUp]) {
            let cursor = NSEvent.mouseLocation
            if event.type == .leftMouseUp {
                if !moved { click?() }
                break
            }
            let dx = cursor.x - start.x, dy = cursor.y - start.y
            if !moved {
                guard hypot(dx, dy) >= 3 else { continue }
                let wasSliding = slide != nil
                interruptPlacement()
                if wasSliding { original = frame.offsetBy(dx: -dx, dy: -dy) }
                moved = true
            }
            let raw = original.offsetBy(dx: dx, dy: dy)
            work = (NSScreen.screens.first { $0.frame.contains(cursor) } ?? display(for: raw)).map { magnetFrame(for: $0) }
            let position = magnetsEnabled ? work.map { WindowMagnet.pull(raw, toward: $0) } ?? raw : raw
            setFrameOrigin(position.origin)
            if magnetsEnabled, let work { onCorners?(WindowMagnet.corners(position, in: work)) }
        }
        guard moved else { return }
        if magnetsEnabled, let work {
            let target = WindowMagnet.snap(frame, to: work)
            home = target
            onCorners?(WindowMagnet.corners(target, in: work))
            let start = frame
            motion.start(duration: 0.18) { [weak self] time in
                let next = start.interpolated(to: target, by: Motion.ease(time))
                if self?.frame.origin != next.origin { self?.setFrameOrigin(next.origin) }
            } completion: { [weak self] in
                self?.home = nil
                self?.onPlacement?()
            }
        } else { onPlacement?() }
    }

    func resizeFolder(with event: NSEvent, left: Bool, right: Bool, bottom: Bool) {
        let start = convertPoint(toScreen: event.locationInWindow)
        interruptPlacement()
        let original = frame
        while let event = nextEvent(matching: [.leftMouseDragged, .leftMouseUp]), event.type == .leftMouseDragged {
            let cursor = NSEvent.mouseLocation
            let dx = cursor.x - start.x, dy = cursor.y - start.y
            var next = original
            if left {
                next.size.width = max(minSize.width, original.width - dx)
                next.origin.x = original.maxX - next.width
            }
            if right { next.size.width = max(minSize.width, original.width + dx) }
            if bottom {
                next.size.height = max(minSize.height, original.height - dy)
                next.origin.y = original.maxY - next.height
            }
            setFrame(next, display: true)
            refreshCorners()
        }
        onPlacement?()
    }
}
