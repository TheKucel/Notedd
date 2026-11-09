import AppKit

final class Setup: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let window = SetupWindow(contentRect: CGRect(origin: .zero, size: SetupStyle.size),
                                     styleMask: .borderless, backing: .buffered, defer: false)
    private let root = SetupSurface(frame: CGRect(origin: .zero, size: SetupStyle.size))
    private let cards = (0..<5).map { _ in SetupCard() }
    private let motion = Motion()
    private let progressMotion = Motion()
    private let languageMotion = Motion()
    private var poses: [SetupPose] = []
    private var step = 0
    private var transitioning = false
    private var recording = false
    private var shortcutError = false
    private var keyMonitor: Any?
    private var working = false
    private var finished: (URL, String?)?
    private var error: Error?
    private var progress: CGFloat = 0
    private weak var progressBar: NSView?
    private let themeValues = ["paper", "night", "system"]
    private let languages = [("en", "English"), ("ru", "Русский"), ("zh-Hans", "简体中文")]
    private var language = "en"
    private weak var languagePanel: NSView?

    private var theme = 0
    private var simple = false
    private var shortcut = ShortcutCommand.toggleWindow.defaultBinding
    private var otherShortcuts = ShortcutPreferences()
    private var login = false
    private var changeLogin = false
    private var desktopAlias = false
    private var notes = URL(fileURLWithPath: "")
    private var appFolder = URL(fileURLWithPath: "")

    func applicationDidFinishLaunching(_ notification: Notification) {
        Geologica.register()
        loadOptions()
        let preferred = Locale.preferredLanguages.first ?? "en"
        if preferred.hasPrefix("ru") { language = "ru" }
        if preferred.hasPrefix("zh") { language = "zh-Hans" }

        let main = NSMenu()
        let application = NSMenuItem()
        let menu = NSMenu()
        let quit = NSMenuItem(title: t("Quit Notedd Setup"), action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        menu.addItem(quit)
        application.submenu = menu
        main.addItem(application)
        NSApp.mainMenu = main

        window.delegate = self
        window.title = "Notedd Setup"
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = false
        window.isReleasedWhenClosed = false
        window.isMovableByWindowBackground = true
        window.contentView = root
        window.center()
        for card in cards.reversed() { root.addSubview(card) }
        updateTabs()
        renderPage()
        keyMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, event.window === window else { return event }
            if recording {
                capture(event)
                return nil
            }
            if event.keyCode == 53, !working { window.close(); return nil }
            return event
        }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        intro()
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        working ? .terminateCancel : .terminateNow
    }

    private func loadOptions() {
        let saved = UserDefaults.standard.persistentDomain(forName: Installation.appID) ?? [:]
        let files = FileManager.default
        let documents = files.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let applications = files.homeDirectoryForCurrentUser.appendingPathComponent("Applications", isDirectory: true)
        let previous = UserDefaults.standard.string(forKey: "InstalledIn").map { URL(fileURLWithPath: $0, isDirectory: true) }
        let system = URL(fileURLWithPath: "/Applications", isDirectory: true)
        let systemApp = system.appendingPathComponent(Installation.appName)
        let installed = Bundle(url: systemApp)?.bundleIdentifier == Installation.appID
        appFolder = previous ?? (installed ? system : applications)
        notes = (saved["NotesFolder"] as? String).map { URL(fileURLWithPath: $0, isDirectory: true) } ??
            documents.appendingPathComponent("Notedd", isDirectory: true)
        simple = saved["PreferredPreview"] as? Bool ?? false
        theme = themeValues.firstIndex(of: saved["Theme"] as? String ?? "") ?? 0
        otherShortcuts = decodeShortcuts(saved["Shortcuts"])
        shortcut = otherShortcuts[.toggleWindow] ?? ShortcutCommand.toggleWindow.defaultBinding
        changeLogin = saved.isEmpty && !installed
        login = saved["SetupOpenAtLogin"] as? Bool ?? saved["OpenAtLogin"] as? Bool ?? changeLogin
    }

    private func decodeShortcuts(_ data: Any?) -> ShortcutPreferences {
        guard let data = data as? Data, let bindings = try? PropertyListDecoder().decode(ShortcutPreferences.self, from: data)
        else { return ShortcutPreferences() }
        return bindings
    }

    private func t(_ text: String) -> String {
        guard let url = Bundle.main.url(forResource: language, withExtension: "lproj"), let bundle = Bundle(url: url)
        else { return text }
        return bundle.localizedString(forKey: text, value: text, table: nil)
    }

    private func updateTabs() {
        let names = ["Notedd", t("Theme"), t("Shortcut"), t("Folders"), finished == nil ? t("Installation") : t("Done")]
        var x: CGFloat = 38
        for (index, card) in cards.enumerated() {
            card.tab = names[index]
            card.tabX = x
            card.tabWidth = ceil(SetupStyle.text(names[index], size: 15, weight: 500).size().width) + 38
            card.top = index == step
            x += card.tabWidth + 4
        }
    }

    private func intro() {
        let tilt: [CGFloat] = [3, -4, 5, -3, 4]
        let target = cards.indices.map { SetupPose.stack($0, depth: $0) }
        poses = target.enumerated().map { index, pose in
            SetupPose(x: pose.x, y: pose.y + 70, scale: pose.scale, opacity: 0, angle: tilt[index])
        }
        cards[0].page.alphaValue = 0
        animate(to: target, duration: 0.86, delay: { 0.08 + 0.065 * Double(4 - $0) }) { [weak self] progress in
            self?.cards[0].page.alphaValue = Motion.ease((progress - 0.48) / 0.52)
        }
    }

    private func go(to next: Int, completion: (() -> Void)? = nil) {
        guard !transitioning else { return }
        let previous = step
        step = next
        recording = false
        shortcutError = false
        updateTabs()
        renderPage()
        let target: [SetupPose] = cards.indices.map { index in
            index < step ? .aside(index) : .stack(index, depth: index - step)
        }
        animate(to: target, duration: 0.43, completion: { [weak self] in
            self?.cards[previous].page.isHidden = previous != next
            completion?()
        })
    }

    private func animate(to target: [SetupPose], duration: TimeInterval, delay: (Int) -> TimeInterval = { _ in 0 },
                         update: ((CGFloat) -> Void)? = nil, completion: (() -> Void)? = nil) {
        let initial = poses
        let delays = cards.indices.map(delay)
        runTransition(duration: duration, update: { [weak self] time in
            guard let self else { return }
            for index in cards.indices {
                let start = delays[index] / duration
                let progress = Motion.ease((time - start) / (1 - start))
                poses[index] = initial[index].moved(to: target[index], by: progress)
                poses[index].apply(to: cards[index])
            }
            update?(time)
        }, completion: completion)
    }

    private func runTransition(duration: TimeInterval, update: @escaping (CGFloat) -> Void, completion: (() -> Void)? = nil) {
        transitioning = true
        setControlsEnabled(false)
        motion.start(duration: duration) { time in
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            update(time)
            CATransaction.commit()
        } completion: { [weak self] in
            self?.transitioning = false
            self?.setControlsEnabled(true)
            completion?()
        }
    }

    private func setControlsEnabled(_ enabled: Bool) {
        for (index, card) in cards.enumerated() {
            for case let button as SetupButton in card.page.subviews {
                if case .keycap = button.style { continue }
                button.isEnabled = enabled && index == step && !working
            }
        }
    }

    private var page: SetupSurface { cards[step].page }

    private func label(_ text: String, x: CGFloat = 56, y: CGFloat, width: CGFloat = 528, height: CGFloat = 28,
                       size: CGFloat = 15, weight: Int = 400, color: NSColor = SetupStyle.ink,
                       center: Bool = false, truncate: Bool = false) {
        let label = NSTextField(labelWithAttributedString: SetupStyle.text(text, size: size, weight: weight, color: color,
                                    alignment: center ? .center : .left, truncate: truncate))
        label.frame = CGRect(x: x, y: y, width: width, height: height)
        label.maximumNumberOfLines = truncate ? 1 : 0
        label.cell?.wraps = !truncate
        label.cell?.isScrollable = false
        label.lineBreakMode = truncate ? .byTruncatingMiddle : .byWordWrapping
        if truncate { label.toolTip = text }
        page.addSubview(label)
    }

    @discardableResult
    private func button(_ title: String, x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat = 44,
                        style: SetupButton.Style, action: @escaping (Setup) -> Void) -> SetupButton {
        let button = SetupButton(title, frame: CGRect(x: x, y: y, width: width, height: height), style: style) { [weak self] in
            if let self, !transitioning && !working { action(self) }
        }
        page.addSubview(button)
        return button
    }

    private func pillWidth(_ title: String, padding: CGFloat = 44) -> CGFloat {
        ceil(SetupStyle.text(title, size: 15, weight: 500).size().width) + padding
    }

    private func heading(_ title: String, _ subtitle: String) {
        label(title, y: 88, height: 42, size: 30, weight: 700)
        label(subtitle, y: 136, height: 57, size: 15.5, color: SetupStyle.muted)
    }

    private func renderPage() {
        languagePanel?.removeFromSuperview()
        page.subviews.forEach { $0.removeFromSuperview() }
        page.isHidden = false
        window.defaultButtonCell = nil

        if !working {
            button(t("Close"), x: 600, y: 4, width: 26, height: 26, style: .close) { $0.window.close() }
        }
        switch step {
        case 0: welcome()
        case 1: themePage()
        case 2: shortcutPage()
        case 3: foldersPage()
        default: completionPage()
        }
        if !working {
            let titles = [t("Get started"), t("Next"), t("Next"), t("Install"), error == nil ? t("Open Notedd") : t("Try again")]
            let width = pillWidth(titles[step], padding: 56)
            let next = button(titles[step], x: 592 - width, y: 424, width: width, style: .primary) { $0.advance() }
            if !recording { next.keyEquivalent = "\r"; window.defaultButtonCell = next.cell as? NSButtonCell }
            if step > 0 {
                let title = step == 4 && error == nil ? t("Close") : t("Back")
                button(title, x: 36, y: 424, width: pillWidth(title, padding: 40), style: .quiet) { setup in
                    if setup.step == 4 && setup.error == nil { setup.window.close() }
                    else { setup.go(to: setup.step - 1) }
                }
            }
        }
        window.recalculateKeyViewLoop()
        setControlsEnabled(!transitioning)
    }

    private func welcome() {
        label("Notedd", x: 0, y: 143, width: 640, height: 92, size: 68, weight: 700, center: true)
        label(t("Quick notes, always at hand.\nA couple of steps and you can start writing."), x: 60, y: 241,
              width: 520, height: 65, size: 16.5, color: SetupStyle.muted, center: true)
        button(t("Language"), x: 56, y: 424, width: 44, style: .language) { $0.showLanguages() }
    }

    private func showLanguages() {
        if let languagePanel { languagePanel.removeFromSuperview(); return }
        let panel = SetupSurface(frame: CGRect(x: 56, y: 294, width: 166, height: 120))
        panel.wantsLayer = true
        panel.layer?.cornerRadius = 14
        panel.layer?.backgroundColor = NSColor.white.cgColor
        panel.layer?.borderWidth = 1
        panel.layer?.borderColor = SetupStyle.line.withAlphaComponent(0.25).cgColor
        panel.layer?.shadowOpacity = 0.16
        panel.layer?.shadowRadius = 10
        panel.layer?.shadowOffset = CGSize(width: 0, height: -4)
        for (index, entry) in languages.enumerated() {
            let row = SetupButton(entry.1, frame: CGRect(x: 6, y: 6 + 36 * index, width: 154, height: 36),
                                  style: .segment(entry.0 == language)) { [weak self] in
                guard let self else { return }
                language = entry.0
                updateTabs()
                renderPage()
            }
            panel.addSubview(row)
        }
        page.addSubview(panel)
        languagePanel = panel
        languageMotion.start(duration: 0.14) { [weak panel] time in
            panel?.alphaValue = Motion.ease(time)
            panel?.frame.origin.y = 294 + 5 * (1 - Motion.ease(time))
        }
    }

    private func themePage() {
        heading(t("Theme"), t("Choose how Notedd looks. You can change it later in Settings."))
        for (index, title) in [t("Light"), t("Dark"), t("Follow macOS")].enumerated() {
            button(title, x: 51 + 184 * CGFloat(index), y: 195, width: 168, height: 141, style: .tile(index, index == theme)) {
                $0.theme = index
                $0.renderPage()
            }
        }
        label(t("Editor"), y: 358, height: 22, weight: 500)
        label(t(simple ? "Hides Markdown markers" : "Shows Markdown markers"), y: 381, height: 23, size: 13, color: SetupStyle.muted)
        let choices = ["Markdown", t("Simple")]
        let width = choices.map { pillWidth($0, padding: 38) }.max() ?? 120
        let left = 584 - 2 * width
        let capsule = SetupSurface(frame: CGRect(x: left, y: 358, width: 2 * width, height: 44))
        capsule.wantsLayer = true
        capsule.layer?.cornerRadius = 22
        capsule.layer?.backgroundColor = SetupStyle.ink.withAlphaComponent(0.05).cgColor
        capsule.layer?.borderWidth = 1
        capsule.layer?.borderColor = SetupStyle.line.withAlphaComponent(0.35).cgColor
        page.addSubview(capsule)
        for (index, title) in choices.enumerated() {
            button(title, x: left + CGFloat(index) * width, y: 358, width: width, style: .segment((index == 1) == simple)) {
                $0.simple = index == 1
                $0.renderPage()
            }
        }
    }

    private func shortcutPage() {
        heading(t("Shortcut"), t("This shortcut shows and hides Notedd from anywhere."))
        if recording {
            button(t("Press a shortcut…"), x: 56, y: 198, width: 205, style: .outline) {
                $0.recording = false
                $0.renderPage()
            }
            label(t(shortcutError ? "Choose a different shortcut." : "Esc to cancel"), x: 277, y: 211, size: 13, color: SetupStyle.muted)
        } else {
            let right = keycaps(at: 56, y: 198)
            let change = t("Change")
            button(change, x: right + 16, y: 198, width: pillWidth(change), style: .outline) {
                $0.recording = true
                $0.renderPage()
            }
        }
        button(t("Open at login"), x: 56, y: 284, width: 528, height: 50,
               style: .toggle(login, t("Notedd will be ready in the background."))) {
            $0.login.toggle()
            $0.changeLogin = true
            $0.renderPage()
        }
    }

    private func capture(_ event: NSEvent) {
        if event.keyCode == 53 {
            recording = false
        } else if let binding = Shortcut(event: event),
                  !ShortcutCommand.allCases.contains(where: { $0 != .toggleWindow && otherShortcuts[$0]?.overlaps(binding) == true }) {
            shortcut = binding
            recording = false
        } else {
            shortcutError = true
        }
        if !recording { shortcutError = false }
        renderPage()
    }

    @discardableResult
    private func keycaps(at start: CGFloat, y: CGFloat) -> CGFloat {
        var x = start
        for part in shortcut.parts {
            let width = max(46, pillWidth(part, padding: 30))
            let cap = SetupButton(part, frame: CGRect(x: x, y: y, width: width, height: 44), style: .keycap) {}
            cap.refusesFirstResponder = true
            cap.isEnabled = false
            page.addSubview(cap)
            x += width + 8
        }
        return x - 8
    }

    private func foldersPage() {
        heading(t("Folders"), t("Notes are stored as .md files. You can keep them in a synced folder."))
        let change = t("Change")
        let buttonWidth = pillWidth(change)
        for (index, entry) in [(t("Notes"), notes), (t("App"), appFolder)].enumerated() {
            let y = 198 + 80 * CGFloat(index)
            label(entry.0, y: y, size: 13, color: SetupStyle.muted)
            let path = index == 0 ? entry.1.path : entry.1.appendingPathComponent(Installation.appName).path
            label((path as NSString).abbreviatingWithTildeInPath, y: y + 22, width: 510 - buttonWidth,
                  height: 26, weight: 500, truncate: true)
            button(change, x: 584 - buttonWidth, y: y + 4, width: buttonWidth, style: .outline) { $0.chooseFolder(notesFolder: index == 0) }
        }
        button(t("Desktop alias"), x: 56, y: 360, width: 528, style: .toggle(desktopAlias, "")) {
            $0.desktopAlias.toggle()
            $0.renderPage()
        }
    }

    private func chooseFolder(notesFolder: Bool) {
        let panel = NSOpenPanel()
        panel.title = t(notesFolder ? "Choose a folder for notes" : "Choose where to install Notedd")
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = notesFolder ? notes : appFolder
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .OK, let url = panel.url else { return }
            if notesFolder { notes = url } else { appFolder = url }
            renderPage()
        }
    }

    private func completionPage() {
        if let error {
            label(t("Installation failed"), y: 88, height: 42, size: 30, weight: 700)
            label(t(error.localizedDescription), y: 136, height: 148, size: 15.5, color: SetupStyle.muted)
        } else if let (_, warning) = finished {
            label(t("Done"), x: 0, y: 116, width: 640, height: 88, size: 68, weight: 700, center: true)
            label(t("Use this shortcut to open Notedd from anywhere."), x: 70, y: 213, width: 500, height: 55,
                  size: 16.5, color: SetupStyle.muted, center: true)
            let width = shortcut.parts.map { max(46, pillWidth($0, padding: 30)) + 8 }.reduce(-8, +)
            keycaps(at: (640 - width) / 2, y: 280)
            if let warning {
                label(t("Couldn’t create the desktop alias.") + " " + warning, y: 348, height: 60, size: 13, color: SetupStyle.muted)
            }
        } else {
            heading(t("Installing…"), t("Just a moment."))
            let track = SetupSurface(frame: CGRect(x: 56, y: 208, width: 528, height: 8))
            track.wantsLayer = true
            track.layer?.backgroundColor = SetupStyle.ink.withAlphaComponent(0.08).cgColor
            track.layer?.cornerRadius = 4
            let fill = SetupSurface(frame: CGRect(x: 0, y: 0, width: 528 * progress, height: 8))
            fill.wantsLayer = true
            fill.layer?.backgroundColor = SetupStyle.dark.cgColor
            fill.layer?.cornerRadius = 4
            track.addSubview(fill)
            page.addSubview(track)
            progressBar = fill
        }
    }

    private func advance() {
        switch step {
        case 0...2: go(to: step + 1)
        case 3: beginInstallation()
        default:
            if let (destination, _) = finished { open(destination) }
            else { beginInstallation() }
        }
    }

    private func beginInstallation() {
        working = true
        error = nil
        progress = 0.02
        let install = Installation(directory: appFolder, notes: notes, desktopAlias: desktopAlias)
        if step == 4 {
            renderPage()
            run(install)
        } else {
            go(to: 4) { [weak self] in self?.run(install) }
        }
    }

    private func run(_ install: Installation) {
        setProgress(to: 0.58, duration: 0.8)
        guard let payload = Bundle.main.url(forResource: "Notedd-payload", withExtension: "zip") else {
            installationFailed(Installation.Failure.missingPayload)
            return
        }
        let running = NSRunningApplication.runningApplications(withBundleIdentifier: Installation.appID)
        for app in running { app.terminate() }
        waitForExit(running, deadline: ProcessInfo.processInfo.systemUptime + 6) { [weak self] in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = Result { try install.install(payload: payload) }
                DispatchQueue.main.async {
                    guard let self else { return }
                    switch result {
                    case .success(let warning):
                        do {
                            try self.saveOptions(install)
                            self.setProgress(to: 1, duration: 0.25) { [weak self] in self?.installed(install.destination, warning: warning) }
                        } catch { self.installationFailed(error) }
                    case .failure(let error): self.installationFailed(error)
                    }
                }
            }
        }
    }

    private func waitForExit(_ apps: [NSRunningApplication], deadline: TimeInterval, completion: @escaping () -> Void) {
        if apps.allSatisfy(\.isTerminated) { completion(); return }
        guard ProcessInfo.processInfo.systemUptime < deadline else { installationFailed(Installation.Failure.running); return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.12) { [weak self] in
            self?.waitForExit(apps, deadline: deadline, completion: completion)
        }
    }

    private func saveOptions(_ install: Installation) throws {
        let defaults = UserDefaults.standard
        var saved = defaults.persistentDomain(forName: Installation.appID) ?? [:]
        var bindings = decodeShortcuts(saved["Shortcuts"])
        bindings[.toggleWindow] = shortcut
        saved["Shortcuts"] = try PropertyListEncoder().encode(bindings)
        saved["Theme"] = themeValues[theme]
        saved["PreferredPreview"] = simple
        saved["NotesFolder"] = install.notes.path
        if changeLogin { saved["SetupOpenAtLogin"] = login }
        defaults.setPersistentDomain(saved, forName: Installation.appID)
        defaults.set(install.directory.path, forKey: "InstalledIn")
        defaults.synchronize()
    }

    private func setProgress(to target: CGFloat, duration: TimeInterval, completion: (() -> Void)? = nil) {
        let from = progress
        progressMotion.start(duration: duration) { [weak self] time in
            guard let self else { return }
            progress = from + (target - from) * Motion.ease(time)
            progressBar?.frame.size.width = 528 * progress
        } completion: {
            completion?()
        }
    }

    private func installationFailed(_ error: Error) {
        progressMotion.cancel()
        self.error = error
        working = false
        renderPage()
    }

    private func installed(_ destination: URL, warning: String?) {
        working = false
        finished = (destination, warning)
        updateTabs()
        renderPage()
        let front = cards.count - 1
        // During the finale the returning cards pile up from the oldest step to Done.
        for card in cards { root.addSubview(card, positioned: .above, relativeTo: nil) }
        let views = page.subviews
        let copy = views.filter { $0 is NSTextField }
        let keycaps = views.compactMap { view -> SetupButton? in
            guard let button = view as? SetupButton, case .keycap = button.style else { return nil }
            return button
        }
        let actions = views.compactMap { view -> SetupButton? in
            guard let button = view as? SetupButton else { return nil }
            switch button.style {
            case .primary, .quiet: return button
            default: return nil
            }
        }
        let revealCopy = reveal(copy, after: 0.25, lift: 12)
        let revealKeys = reveal(keycaps, after: 0.45, lift: 10)
        let revealActions = reveal(actions, after: 0.70, lift: 0)

        var initial = poses
        initial[front].y += 10
        initial[front].scale = 0.985
        let fan = (0..<front).map(SetupPose.fan) + [SetupPose()]
        let stack = cards.indices.map { SetupPose.stack($0, depth: front - $0) }
        let fanEnd = 0.95
        let duration = 1.6
        func intoFan(_ index: Int, elapsed: TimeInterval) -> SetupPose {
            let delay = index == front ? 0 : 0.06 * Double(front - 1 - index)
            let time = max(0, elapsed - delay)
            return initial[index].moved(to: fan[index], by: 1 - exp(-15 * time), fade: 1 - exp(-22 * time))
        }
        let spread = cards.indices.map { intoFan($0, elapsed: fanEnd) }
        runTransition(duration: duration, update: { [weak self] time in
            guard let self else { return }
            let elapsed = duration * time
            for index in cards.indices {
                if time == 1 {
                    poses[index] = stack[index]
                } else if elapsed < fanEnd {
                    poses[index] = intoFan(index, elapsed: elapsed)
                } else {
                    let delay = index == front ? 0 : 0.04 * Double(front - 1 - index)
                    let foldTime = max(0, elapsed - fanEnd - delay)
                    poses[index] = spread[index].moved(to: stack[index], by: 1 - exp(-15 * foldTime))
                }
                poses[index].apply(to: cards[index])
            }
            revealCopy(elapsed)
            revealKeys(elapsed)
            revealActions(elapsed)
        })
    }

    private func reveal(_ views: [NSView], after delay: TimeInterval, lift: CGFloat) -> (TimeInterval) -> Void {
        let elements = views.map { ($0, $0.frame.minY) }
        return { elapsed in
            let progress = min(1, max(0, (elapsed - delay) / 0.4))
            for (view, top) in elements {
                view.alphaValue = progress
                view.frame.origin.y = top + (1 - progress) * lift
            }
        }
    }

    private func open(_ destination: URL) {
        NSWorkspace.shared.openApplication(at: destination, configuration: NSWorkspace.OpenConfiguration()) { [weak self] _, error in
            DispatchQueue.main.async {
                guard let self else { return }
                if let error {
                    NSAlert(error: error).beginSheetModal(for: self.window)
                } else {
                    self.window.close()
                }
            }
        }
    }
}
