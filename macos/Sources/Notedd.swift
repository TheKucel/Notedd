import AppKit
import Carbon
import ServiceManagement

final class Notedd: NSObject, NSApplicationDelegate, NSWindowDelegate, NSTextViewDelegate,
                   NSTextFieldDelegate, NSMenuItemValidation {
    private let defaults = UserDefaults.standard
    private var store: NoteStore
    private var notes: [Note] = []
    private var current: Note?
    private var deleted: Note?
    private var dirty = false
    private var saveTimer: Timer?
    private let editorUndo = UndoManager()
    private var hotKeys: HotKeys?
    private lazy var shortcuts = ShortcutPreferences.load(defaults)
    private var recordingShortcut: ShortcutCommand?
    private var shortcutError: (command: ShortcutCommand, message: String)?
    private var settingsKeyMonitor: Any?
    private let window = FolderWindow(contentRect: NSRect(x: 0, y: 0, width: 916, height: 595),
                                      styleMask: .borderless, backing: .buffered, defer: false)
    private let root = NSView()
    private let notesSurface = NSView()
    private let preferencesFolder = FolderPage()
    private let preferences = PreferencesPanel()
    private var settingsOpen: Bool { !preferencesFolder.isHidden }
    private let page = FolderPage()
    private let deck = NoteDeck()
    private let search = NSTextField()
    private let find = NSTextField()
    private let searchIcon = NSImageView()
    private let findIcon = NSImageView()
    private let findCounter = NSTextField(labelWithString: "")
    private let title = ChromeButton()
    private let add = ChromeButton()
    private let previousMatch = ChromeButton()
    private let nextMatch = ChromeButton()
    private let closeFind = ChromeButton()
    private let controls = NSView()
    private let controlsMotion = Motion()
    private let close = ChromeButton()
    private let pin = ChromeButton()
    private let action = ChromeButton()
    private let more = ChromeButton()
    private let settings = ChromeButton()
    private let folders = ChromeButton()
    private let trash = ChromeButton()
    private let swatch = ChromeButton()
    private let editor = Editor.scrollableTextView()
    private let folderPopup = FolderPopup()
    private var source: Editor { editor.documentView as! Editor }
    private let footer = NSTextField(labelWithString: "")
    private var isPreview = false
    private var styledWidth: CGFloat = 0
    private var applyingEdit = false
    private var listing = false
    private var finding = false
    private var expanded = false
    private var matches: [NSRange] = []
    private var match = 0
    private var observingAppearance: NSKeyValueObservation?

    private static let paper: [UInt32] = [0xefebe3, 0xdde3d2, 0xd8e0e2, 0xe2dbe8, 0xefdcd4, 0xeee1c5, 0xd9e4ea, 0xe6e6dc, 0xe9dde3, 0xe0e7dc]
    private static let night: [UInt32] = [0x141412, 0x1a1f19, 0x171d1f, 0x1d1a22, 0x221a17, 0x221e15, 0x161c22, 0x1c1c18, 0x201a1d, 0x1a1f1a]
    private static let colorNames = ["Paper", "Sage", "Fog", "Lavender", "Rose", "Sand", "Sky", "Stone", "Mauve", "Mint"]

    override init() {
        let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first!
        let folder = UserDefaults.standard.string(forKey: "NotesFolder").map { URL(fileURLWithPath: $0, isDirectory: true) }
        store = NoteStore(folder: folder ?? documents.appendingPathComponent("Notedd", isDirectory: true))
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        Geologica.register()
        let launchedAtLogin = NSAppleEventManager.shared().currentAppleEvent?
            .paramDescriptor(forKeyword: AEKeyword(keyAELaunchedAsLogInItem)) != nil
        let startInBackground = launchedAtLogin || ProcessInfo.processInfo.arguments.contains("--background")
        NSApp.mainMenu = makeMainMenu()
        configureWindow()
        hotKeys = HotKeys { [weak self] command in
            if command == .toggleWindow { self?.toggleWindow() }
            else { self?.newNote(nil) }
        }
        hotKeys?.register(shortcuts)
        settingsKeyMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel]) { [weak self] event in
            guard let self else { return event }
            if folderPopup.isPaletteOpen && isPaletteToggle(event) {
                folderPopup.close()
                return nil
            }
            guard let event = folderPopup.handle(event) else { return nil }
            return event.type == .keyDown ? settingsKeyEvent(event) : event
        }
        do {
            notes = try store.load()
            openInitialNote()
            setPreview(defaults.bool(forKey: "PreferredPreview"))
        } catch {
            select(Note())
            showWindow()
            present(error, message: "Couldn’t open the notes folder")
            return
        }
        if !startInBackground { showWindow() }
        if let login = defaults.object(forKey: "SetupOpenAtLogin") as? Bool {
            defaults.removeObject(forKey: "SetupOpenAtLogin")
            setLogin(login)
        }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        showWindow()
        return false
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        flushOrReport() ? .terminateNow : .terminateCancel
    }

    func applicationDidBecomeActive(_ notification: Notification) {
        if window.isVisible && !window.hiding && !window.isKeyWindow && window.attachedSheet == nil {
            window.makeKeyAndOrderFront(nil)
        }
    }

    func windowShouldClose(_ sender: NSWindow) -> Bool {
        hideWindow()
        return false
    }

    func windowDidResignKey(_ notification: Notification) {
        folderPopup.close()
        cancelShortcutRecording()
    }

    func windowDidResize(_ notification: Notification) {
        if isPreview && styledWidth != editor.contentSize.width { styleSource() }
        layoutControls()
    }

    private func configureWindow() {
        window.delegate = self
        window.title = "Notedd"
        window.minSize = NSSize(width: 556, height: 335)
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = false
        window.animationBehavior = .none
        window.tabbingMode = .disallowed
        window.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary]
        window.center()
        window.setFrameUsingName("NoteddFolderWindow")
        window.onPlacement = { [weak self] in self?.window.saveFrame(usingName: "NoteddFolderWindow") }
        window.onCorners = { [weak self] corners in
            self?.page.roundCorners(to: corners)
            self?.preferencesFolder.roundCorners(to: corners)
        }
        window.onCancel = { [weak self] in
            guard let self else { return }
            if settingsOpen { closeSettings() }
            else if listing { toggleList(nil) }
            else { cancelEditor() }
        }
        updateMotionPreferences()
        window.level = defaults.bool(forKey: "Pinned") ? .floating : .normal
        window.acceptsMouseMovedEvents = true

        root.wantsLayer = true
        window.installSurface(root)
        place(notesSurface, in: root)
        place(page, in: notesSurface)
        place(deck, in: notesSurface)
        deck.isHidden = true
        configurePreferences()
        configurePage()
        configureHeader()
        configureControls()
        endFind(nil)
        deck.open = { [weak self] in self?.open($0) }
        deck.showColors = { [weak self] in self?.openPalette(for: $0, at: $1, inStack: true) }
        deck.delete = { [weak self] in self?.deleteNote(nil) }
        deck.close = { [weak self] in self?.toggleList(nil) }
        observingAppearance = NSApp.observe(\.effectiveAppearance) { [weak self] _, _ in self?.restyle() }
        restyle()
    }

    private func configureHeader() {
        for (field, placeholder) in [(search, "Search notes"), (find, "Find in note")] {
            field.placeholderString = placeholder
            field.isBordered = false
            field.isBezeled = false
            field.drawsBackground = false
            field.focusRingType = .none
            field.usesSingleLineMode = true
            field.font = Markdown.geologica(size: 15, weight: 500)
            field.delegate = self
            field.setAccessibilityLabel(placeholder)
        }
        deck.addSubview(searchIcon)
        deck.addSubview(search)
        for view in [title, add, find, findIcon, findCounter, previousMatch, nextMatch, closeFind] { page.addSubview(view) }
        title.alignment = .left
        title.lineBreakMode = .byTruncatingTail
        title.setContentHuggingPriority(.defaultLow, for: .horizontal)
        configureButton(title, label: "All notes", action: #selector(toggleList(_:)))
        title.draggable = true
        configureButton(add, label: "New note", action: #selector(newNote(_:)))
        add.onHoverChange = { [weak self] hovering in
            guard let self, !finding else { return }
            page.widenTab(to: hovering ? 42 : 34)
        }
        configureButton(previousMatch, label: "Previous match", action: #selector(moveFind(_:)))
        configureButton(nextMatch, label: "Next match", action: #selector(moveFind(_:)))
        previousMatch.tag = -1
        nextMatch.tag = 1
        configureButton(closeFind, label: "Close find", action: #selector(endFind(_:)))
        findCounter.font = Markdown.geologica(size: 11)
        findCounter.alignment = .center
        page.updateHeader = { [weak self] shape in self?.layoutHeader(shape) }
        deck.header = { [weak self] frame, scale in
            guard let self else { return }
            searchIcon.frame = NSRect(x: frame.minX + 50 * scale, y: frame.minY + 9 * scale,
                                      width: 16 * scale, height: 16 * scale)
            let tabRight = page.shape.tabRight * scale
            search.frame = NSRect(x: frame.minX + 71 * scale, y: frame.minY + 8 * scale,
                                  width: tabRight - 78 * scale, height: 19 * scale)
            if abs((search.font?.pointSize ?? 0) - 15 * scale) > 0.2 {
                search.font = Markdown.geologica(size: 15 * scale, weight: 500)
            }
        }
    }

    private func layoutHeader(_ shape: FolderShape) {
        let x = shape.bounds.minX
        let y = shape.bounds.minY
        let edge = x + shape.tabRight
        title.frame = NSRect(x: x + 43, y: y + 3, width: shape.tabRight - 48, height: 28)
        add.frame = NSRect(x: edge, y: y + 3, width: min(42, page.tabWidth), height: 28)
        findIcon.frame = NSRect(x: x + 50, y: y + 9, width: 16, height: 16)
        find.frame = NSRect(x: x + 71, y: y + 8, width: shape.tabRight - 78, height: 19)
        findCounter.frame = NSRect(x: edge + 3, y: y + 10, width: 44, height: 15)
        for (index, button) in [previousMatch, nextMatch, closeFind].enumerated() {
            button.frame = NSRect(x: edge + 47 + CGFloat(index) * 26, y: y + 3, width: 26, height: 28)
        }
    }

    private func configurePage() {
        editor.drawsBackground = false
        editor.hasVerticalScroller = true
        editor.autohidesScrollers = true
        source.drawsBackground = false
        source.isVerticallyResizable = true
        source.isHorizontallyResizable = false
        source.textContainerInset = NSSize(width: 0, height: 2)
        source.textContainer?.lineFragmentPadding = 0
        source.textContainer?.widthTracksTextView = true
        source.delegate = self
        source.onEscape = { [weak self] in self?.cancelEditor() }
        place(editor, in: page, top: 78, side: 58, bottom: 48)
        source.isRichText = false
        source.allowsUndo = true
        source.isAutomaticQuoteSubstitutionEnabled = false
        source.isAutomaticDashSubstitutionEnabled = false
        source.isAutomaticTextReplacementEnabled = false
        source.isAutomaticLinkDetectionEnabled = false
        source.isAutomaticSpellingCorrectionEnabled = false
        source.registerForDraggedTypes([.fileURL, .png, .tiff])
        source.onImage = { [weak self] data, ext in self?.insertImage(data, ext: ext) }
        source.layoutManager?.delegate = source
        source.onTask = { [weak self] in self?.toggleTask(at: $0) }
        source.onImageResize = { [weak self] range, width in self?.resizeImage(range, width: width) }
        source.onContext = { [weak self] in self?.openEditMenu(at: $0) }
        source.onDelete = { [weak self] in self?.deleteInSimple(backward: $0) ?? false }
        source.onDeleteWord = { [weak self] in self?.deleteWordInSimple(backward: $0) ?? false }
        source.onNewline = { [weak self] in self?.continueListInSimple() ?? false }
        footer.font = Markdown.geologica(size: 11)
        footer.lineBreakMode = .byTruncatingTail
        footer.isHidden = true
        page.addSubview(footer)
        footer.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            footer.leadingAnchor.constraint(equalTo: page.leadingAnchor, constant: 58),
            footer.trailingAnchor.constraint(equalTo: page.trailingAnchor, constant: -58),
            footer.bottomAnchor.constraint(equalTo: page.bottomAnchor, constant: -32)
        ])
    }

    private func configureControls() {
        root.addSubview(controls)
        controls.wantsLayer = true
        controls.layer?.cornerRadius = 14
        controls.layer?.borderWidth = 1
        controls.layer?.shadowOpacity = 0.17
        controls.layer?.shadowRadius = 5
        controls.layer?.shadowOffset = NSSize(width: 0, height: -2)
        for (button, label, selector) in [
            (close, "Hide Notedd", #selector(hideFromButton(_:))), (pin, "Always on top", #selector(togglePin(_:))),
            (action, "Find in note", #selector(primaryAction(_:))), (more, "More controls", #selector(toggleControls(_:))),
            (settings, "Settings", #selector(showSettings(_:))), (folders, "All notes", #selector(toggleList(_:))),
            (trash, "Move note to Trash", #selector(deleteNote(_:))), (swatch, "Note color", #selector(showColors(_:)))
        ] {
            controls.addSubview(button)
            configureButton(button, label: label, action: selector)
        }
    }

    private func configureButton(_ button: ChromeButton, label: String, action: Selector) {
        button.isBordered = false
        button.focusRingType = .none
        button.imagePosition = button === title ? .noImage : .imageOnly
        button.target = self
        button.action = action
        button.toolTip = label
        button.setAccessibilityLabel(label)
    }

    private func layoutControls(animated: Bool = false) {
        let editingExtras = !settingsOpen && !listing && (!finding || root.bounds.width >= 740)
        let buttons = [close, pin, action, more] + (editingExtras ? [swatch] : []) +
            (expanded ? [settings, folders] + (editingExtras ? [trash] : []) : [])
        let oldButtons = [close, pin, action, more, swatch, settings, folders, trash].filter { !$0.isHidden }
        let drawing = buttons.count >= oldButtons.count ? buttons : oldButtons
        let target = CGFloat(buttons.count) * 34
        let from = controls.frame.width
        for button in [close, pin, action, more, settings, folders, trash, swatch] where !drawing.contains(button) {
            button.isHidden = true
        }
        controlsMotion.start(duration: animated && from > 0 && from != target ? 0.19 : 0) { [weak self] time in
            guard let self else { return }
            let width = from + (target - from) * Motion.ease(time)
            controls.frame = NSRect(x: root.bounds.width - 18 - width, y: root.bounds.height - 41, width: width, height: 28)
            for (index, button) in drawing.enumerated() {
                let slot = CGFloat(index + 1) * 34
                let opacity = min(1, max(0, (width - slot + 34) / 34))
                button.isHidden = opacity <= 0.001
                button.alphaValue = opacity
                button.frame = NSRect(x: max(0, width - slot), y: 0, width: 34, height: 28)
            }
        }
    }

    @objc private func hideFromButton(_ sender: Any?) { hideWindow() }

    @objc private func primaryAction(_ sender: Any?) {
        if settingsOpen { closeSettings() }
        else if listing { newNote(nil) }
        else { findInNote(nil) }
    }

    @objc private func toggleControls(_ sender: Any?) {
        expanded.toggle()
        styleControls(animated: true)
    }

    private func place(_ view: NSView, in parent: NSView, top: CGFloat = 0, side: CGFloat = 0, bottom: CGFloat = 0) {
        parent.addSubview(view)
        view.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            view.leadingAnchor.constraint(equalTo: parent.leadingAnchor, constant: side),
            view.trailingAnchor.constraint(equalTo: parent.trailingAnchor, constant: -side),
            view.topAnchor.constraint(equalTo: parent.topAnchor, constant: top),
            view.bottomAnchor.constraint(equalTo: parent.bottomAnchor, constant: -bottom)
        ])
    }

    private var dark: Bool {
        switch defaults.string(forKey: "Theme") ?? "paper" {
        case "paper": return false
        case "night": return true
        default: return NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        }
    }

    private func color(_ rgb: UInt32) -> NSColor {
        NSColor(srgbRed: CGFloat((rgb >> 16) & 255) / 255, green: CGFloat((rgb >> 8) & 255) / 255,
                blue: CGFloat(rgb & 255) / 255, alpha: 1)
    }

    private func paper(_ index: Int) -> NSColor { color((dark ? Self.night : Self.paper)[index]) }
    private var ink: NSColor { color(dark ? 0xece8dc : 0x141412) }
    private var muted: NSColor { color(dark ? 0xb0ab9f : 0x56534b) }
    private var accent: NSColor { color(dark ? 0x8db4ec : 0x2f5fa8) }
    private var markdown: Markdown {
        let size = defaults.object(forKey: "TextSize") as? Double ?? 16
        return Markdown(fontSize: size, ink: ink, muted: muted, accent: accent,
                        highlight: color(dark ? 0x57441d : 0xffe491),
                        checkbox: Checkbox(scale: size / 16, outline: color(dark ? 0x6e6a61 : 0x98938a), fill: tab, tick: onTab),
                        folder: store.folder)
    }

    private var outline: NSColor { color(dark ? 0x5a5952 : 0x4b4a46) }
    private var tab: NSColor { color(dark ? 0xe6e2d5 : 0x2b2b28) }
    private var onTab: NSColor { color(dark ? 0x151513 : 0xf3f0e8) }

    private func restyle(animatedColor: Bool = false) {
        window.appearance = NSAppearance(named: dark ? .darkAqua : .aqua)
        page.changePaper(to: paper(current?.color ?? 0), animated: animatedColor)
        page.line = outline
        page.tab = tab
        let shadow = color(dark ? 0x000000 : 0x28261e).withAlphaComponent(dark ? 0.4 : 0.24)
        page.shadowColor = shadow
        preferencesFolder.paper = paper(0)
        preferencesFolder.line = outline
        preferencesFolder.shadowColor = shadow
        if settingsOpen { refreshPreferences() }
        deck.palette = { [weak self] in self?.paper($0) ?? .white }
        deck.markdown = markdown
        deck.line = outline
        deck.darkTab = tab
        deck.onDark = onTab
        deck.shadowColor = shadow
        deck.refresh()
        for field in [search, find] { field.textColor = ink }
        searchIcon.image = FolderGlyph.find.image(muted)
        findIcon.image = FolderGlyph.find.image(muted)
        findCounter.textColor = onTab
        add.image = FolderGlyph.plus.image(onTab)
        previousMatch.image = FolderGlyph.up.image(onTab)
        nextMatch.image = FolderGlyph.down.image(onTab)
        closeFind.image = FolderGlyph.close.image(onTab)
        for button in [add, previousMatch, nextMatch, closeFind] { button.hoverColor = onTab.withAlphaComponent(0.10) }
        title.hoverColor = ink.withAlphaComponent(0.07)
        source.insertionPointColor = ink
        source.linkTextAttributes = [.foregroundColor: accent, .underlineStyle: NSUnderlineStyle.single.rawValue]
        updateTitle()
        styleControls()
        styleSource()
    }

    private func styleControls(animated: Bool = false) {
        let icons: [(ChromeButton, FolderGlyph)] = [(close, .close), (pin, window.level == .floating ? .pinned : .pin),
            (action, settingsOpen ? .folders : listing ? .plus : .find), (more, expanded ? .less : .more), (settings, .settings),
            (folders, .folders), (trash, .trash), (swatch, .dot)]
        for (button, glyph) in icons {
            button.image = glyph.image(muted, fill: paper(current?.color ?? 0))
            button.hoverColor = ink.withAlphaComponent(0.07)
        }
        let primaryLabel = settingsOpen ? "Back to notes" : listing ? "New note" : "Find in note"
        action.toolTip = primaryLabel
        action.setAccessibilityLabel(primaryLabel)
        controls.layer?.backgroundColor = paper(0).withAlphaComponent(0.96).cgColor
        controls.layer?.borderColor = outline.withAlphaComponent(0.29).cgColor
        layoutControls(animated: animated)
    }

    private func updateTitle() {
        let label = current?.title ?? ""
        let shown = label.isEmpty ? "Untitled" : label
        window.title = label.isEmpty ? "Notedd" : "\(label) — Notedd"
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineBreakMode = .byTruncatingTail
        paragraph.firstLineHeadIndent = 7
        title.attributedTitle = NSAttributedString(string: shown, attributes: [
            .font: Markdown.geologica(size: 15, weight: 500), .foregroundColor: ink, .paragraphStyle: paragraph
        ])
        title.setAccessibilityLabel("\(shown), All notes")
    }

    private func styleSource() {
        guard !source.hasMarkedText(), let storage = source.textStorage else { return }
        styledWidth = editor.contentSize.width
        let styled = markdown.render(source.string, preview: false, width: styledWidth, simple: isPreview)
        let fullRange = NSRange(location: 0, length: styled.length)
        storage.beginEditing()
        styled.enumerateAttributes(in: fullRange) { attributes, range, _ in
            storage.setAttributes(attributes, range: range)
        }
        storage.endEditing()
        // TextKit does not invalidate glyphs for custom attributes.
        source.layoutManager?.invalidateGlyphs(forCharacterRange: fullRange, changeInLength: 0, actualCharacterRange: nil)
        source.window?.invalidateCursorRects(for: source)
        source.needsDisplay = true
        source.typingAttributes = [.font: source.string.isEmpty ? markdown.titleFont : markdown.bodyFont, .foregroundColor: ink]
    }

    private func openInitialNote() {
        let id = NoteddCore.readState(in: store.folder)
        select(notes.first(where: { $0.id == id }) ?? notes.max(by: { $0.recent < $1.recent }) ?? Note())
    }

    private func select(_ note: Note, updateDeck: Bool = true) {
        folderPopup.close()
        saveTimer?.invalidate()
        var note = note
        note.opened = Date()
        current = note
        dirty = false
        source.string = note.text
        editorUndo.removeAllActions()
        source.setSelectedRange(NSRange(location: 0, length: 0))
        editor.contentView.scroll(to: .zero)
        footer.isHidden = true
        if let index = notes.firstIndex(where: { $0.id == note.id }) {
            notes[index] = note
            rememberCurrent(note.id)
        }
        restyle()
        if updateDeck { filterNotes() }
    }

    private func rememberCurrent(_ id: String) {
        NoteddCore.writeState(id, in: store.folder)
    }

    private func recordEdit() {
        dirty = true
        saveTimer?.invalidate()
        saveTimer = Timer.scheduledTimer(withTimeInterval: 0.45, repeats: false) { [weak self] _ in
            guard let self else { return }
            do { try self.flush() } catch {
                self.footer.stringValue = "Couldn’t save · \(error.localizedDescription)"
                self.footer.textColor = .systemRed
                self.footer.isHidden = false
            }
        }
    }

    private func flush() throws {
        saveTimer?.invalidate()
        guard dirty, var note = current else { return }
        note.text = source.string
        try store.save(&note)
        current = note
        dirty = false
        if let index = notes.firstIndex(where: { $0.id == note.id }) { notes[index] = note }
        else if !note.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { notes.append(note) }
        if notes.contains(where: { $0.id == note.id }) { rememberCurrent(note.id) }
        filterNotes()
        footer.isHidden = true
    }

    private func flushOrReport() -> Bool {
        do { try flush(); return true }
        catch { present(error, message: "Couldn’t save the note"); return false }
    }

    func textDidChange(_ notification: Notification) {
        guard notification.object as? Editor === source else { return }
        current?.text = source.string
        updateTitle()
        styleSource()
        if finding { updateFind(select: false) }
        recordEdit()
    }

    func textViewDidChangeSelection(_ notification: Notification) {
        guard notification.object as? Editor === source, isPreview else { return }
        source.needsDisplay = true
    }

    func undoManager(for view: NSTextView) -> UndoManager? { editorUndo }

    func textView(_ textView: NSTextView, clickedOnLink link: Any, at charIndex: Int) -> Bool {
        guard let url = link as? URL else { return false }
        if url.scheme == "notedd-task", let offset = Int(url.absoluteString.dropFirst("notedd-task:".count)) {
            toggleTask(at: offset)
        } else {
            NSWorkspace.shared.open(url)
        }
        return true
    }

    private func showWindow() {
        if window.isMiniaturized { window.deminiaturize(nil) }
        NSApp.activate(ignoringOtherApps: true)
        window.reveal()
        focusCurrentView()
    }

    private func hideWindow() {
        folderPopup.close()
        guard flushOrReport() else { return }
        cancelShortcutRecording()
        if finding { endFind(nil) }
        expanded = false
        styleControls(animated: true)
        window.dismiss()
    }

    @objc private func toggleWindow() {
        if window.isVisible && window.isKeyWindow && !window.hiding { hideWindow() } else { showWindow() }
    }

    @objc private func newNote(_ sender: Any?) {
        createNote("")
    }

    private func createNote(_ title: String) {
        guard flushOrReport() else { return }
        if settingsOpen { closeSettings() }
        let note = Note(text: title.isEmpty ? "" : "\(title)\n")
        let fromStack = listing
        let previous = !fromStack && window.isVisible && !window.hiding && !page.isHidden && Motion.enabled
            ? current.map { (note: $0, snapshot: captureEditor()) } : nil
        endFind(nil)
        select(note, updateDeck: !fromStack)
        if fromStack {
            setListing(false)
        } else if window.isVisible && !window.hiding {
            animateEditor(replacing: previous)
        }
        if !title.isEmpty {
            source.setSelectedRange(NSRange(location: source.string.utf16.count, length: 0))
            recordEdit()
        }
        showWindow()
    }

    @objc private func filterNotes() {
        let query = search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        let visible = notes.filter { query.isEmpty || $0.text.localizedStandardContains(query) }
            .sorted { $0.recent == $1.recent ? $0.id < $1.id : $0.recent > $1.recent }
        deck.query = query
        deck.total = notes.count
        deck.notes = visible
        deck.refresh()
    }

    private func open(_ id: String) {
        guard flushOrReport(), let note = notes.first(where: { $0.id == id }) else { return }
        select(note, updateDeck: !listing)
        setListing(false)
    }

    @objc private func toggleList(_ sender: Any?) {
        guard flushOrReport() else { return }
        if settingsOpen { closeSettings() }
        setListing(!listing)
    }

    private func setListing(_ value: Bool) {
        guard listing != value else { return }
        folderPopup.close()
        listing = value
        if value {
            if finding { endFind(nil) }
            search.stringValue = ""
            filterNotes()
            let snapshot = page.isHidden ? nil : captureEditor()
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            deck.isHidden = false
            deck.showStack(from: page.shape, current: current, snapshot: snapshot)
            page.isHidden = true
            CATransaction.commit()
        } else {
            animateEditor()
        }
        styleControls(animated: true)
        focusCurrentView()
    }

    private func captureEditor() -> EditorSnapshot? {
        root.layoutSubtreeIfNeeded()
        let clip = editor.contentView
        let bounds = clip.bounds
        guard bounds.width > 0, bounds.height > 0,
              let bitmap = clip.bitmapImageRepForCachingDisplay(in: bounds) else { return nil }
        clip.cacheDisplay(in: bounds, to: bitmap)
        let viewport = page.convert(bounds, from: clip)
            .offsetBy(dx: -page.shape.bounds.minX, dy: -page.shape.bounds.minY)
        let image = NSImage(size: viewport.size)
        image.addRepresentation(bitmap)
        return EditorSnapshot(image: image, viewport: viewport)
    }

    private func animateEditor(replacing previous: (note: Note, snapshot: EditorSnapshot?)? = nil) {
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        page.isHidden = false
        let snapshot = Motion.enabled ? captureEditor() : nil
        page.isHidden = true
        deck.isHidden = false
        deck.showEditor(to: page.shape, note: current, snapshot: snapshot, replacing: previous) { [weak self] in
            guard let self else { return }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            page.isHidden = false
            deck.isHidden = true
            focusCurrentView()
            CATransaction.commit()
        }
        CATransaction.commit()
    }

    private func focusCurrentView() {
        if settingsOpen { window.makeFirstResponder(preferences) }
        else if listing { window.makeFirstResponder(search) }
        else if page.isHidden { window.makeFirstResponder(nil) }
        else if finding { window.makeFirstResponder(find) }
        else { window.makeFirstResponder(source) }
    }

    func controlTextDidChange(_ notification: Notification) {
        if notification.object as? NSTextField === find { updateFind() }
        else { filterNotes() }
    }

    func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        if control === find {
            switch commandSelector {
            case #selector(NSResponder.insertNewline(_:)), #selector(NSResponder.moveDown(_:)): moveFind(nextMatch)
            case #selector(NSResponder.moveUp(_:)): moveFind(previousMatch)
            case #selector(NSResponder.cancelOperation(_:)): endFind(nil)
            default: return false
            }
        } else {
            switch commandSelector {
            case #selector(NSResponder.insertNewline(_:)):
                if let note = deck.selection { open(note.id) }
                else { createNote(search.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)) }
            case #selector(NSResponder.moveDown(_:)): deck.moveDown(nil)
            case #selector(NSResponder.moveUp(_:)): deck.moveUp(nil)
            case #selector(NSResponder.cancelOperation(_:)): toggleList(nil)
            default: return false
            }
        }
        return true
    }

    private func setPreview(_ value: Bool) {
        folderPopup.close()
        source.breakUndoCoalescing()
        isPreview = value
        styleSource()
        if value {
            let range = source.selectedRange()
            source.setSelectedRange(normalizedSelection(range, from: range))
        }
        if finding { updateFind() }
        focusCurrentView()
    }

    @objc private func togglePreview(_ sender: Any?) { setPreview(!isPreview) }

    @objc private func findInNote(_ sender: Any?) {
        if settingsOpen { closeSettings() }
        if listing { setListing(false) }
        if !finding {
            finding = true
            page.widenTab(to: 136)
            [find, findIcon, findCounter, previousMatch, nextMatch, closeFind].forEach { $0.isHidden = false }
            title.isHidden = true
            add.isHidden = true
            updateFind()
            styleControls()
        }
        focusCurrentView()
        find.selectText(nil)
    }

    @objc private func endFind(_ sender: Any?) {
        finding = false
        page.widenTab(to: 34)
        find.stringValue = ""
        [find, findIcon, findCounter, previousMatch, nextMatch, closeFind].forEach { $0.isHidden = true }
        title.isHidden = false
        add.isHidden = false
        updateFind()
        styleControls()
        focusCurrentView()
    }

    private func cancelEditor() {
        if finding { endFind(nil) } else { hideWindow() }
    }

    @objc private func moveFind(_ sender: NSButton) {
        guard !matches.isEmpty else { return }
        match = (match + sender.tag + matches.count) % matches.count
        showFindMatches()
    }

    private func updateFind(select: Bool = true) {
        let (content, offsets) = searchableText()
        let query = find.stringValue
        matches = []
        if finding && !query.isEmpty {
            var start = 0
            while start < content.length {
                let result = content.range(of: query, options: .caseInsensitive, range: NSRange(location: start, length: content.length - start))
                guard result.location != NSNotFound else { break }
                if let offsets {
                    let first = offsets[result.location]
                    let end = offsets[NSMaxRange(result) - 1] + 1
                    matches.append(NSRange(location: first, length: end - first))
                } else {
                    matches.append(result)
                }
                start = NSMaxRange(result)
            }
        }
        match = select ? 0 : min(match, max(0, matches.count - 1))
        showFindMatches(scroll: select)
    }

    private func searchableText() -> (NSString, [Int]?) {
        guard isPreview, let storage = source.textStorage else { return (source.string as NSString, nil) }
        let (visible, offsets) = visibleText(in: NSRange(location: 0, length: storage.length), storage: storage)
        return (visible as NSString, offsets)
    }

    private func visibleText(in range: NSRange, storage: NSTextStorage) -> (String, [Int]) {
        let raw = storage.string as NSString
        var visible = ""
        var offsets: [Int] = []
        storage.enumerateAttributes(in: range) { attributes, range, _ in
            guard attributes[.hiddenMarkdown] == nil, attributes[.taskMarker] == nil,
                  attributes[.simpleDecoration] == nil else { return }
            visible += raw.substring(with: range)
            offsets.append(contentsOf: range.location..<NSMaxRange(range))
        }
        return (visible, offsets)
    }

    private func showFindMatches(scroll: Bool = true) {
        let range = NSRange(location: 0, length: source.string.utf16.count)
        source.layoutManager?.removeTemporaryAttribute(.backgroundColor, forCharacterRange: range)
        let fill = NSColor(srgbRed: 1, green: 0.82, blue: 0.4, alpha: 1)
        for (index, range) in matches.enumerated() {
            source.layoutManager?.addTemporaryAttribute(.backgroundColor, value: fill.withAlphaComponent(index == match ? 0.8 : 0.3), forCharacterRange: range)
        }
        findCounter.stringValue = find.stringValue.isEmpty ? "" : matches.isEmpty ? "0" : "\(match + 1)/\(matches.count)"
        if scroll && matches.indices.contains(match) { source.scrollRangeToVisible(matches[match]) }
    }

    private func edit(_ range: NSRange, with text: String, selection: NSRange? = nil) {
        applyingEdit = true
        defer { applyingEdit = false }
        source.breakUndoCoalescing()
        source.insertText(text, replacementRange: range)
        source.breakUndoCoalescing()
        let caret = selection ?? NSRange(location: range.location + text.utf16.count, length: 0)
        source.setSelectedRange(caret)
        source.scrollRangeToVisible(caret)
    }

    private func openEditMenu(at point: CGPoint) {
        let selection = source.selectedRange().length > 0
        let formats: [(ShortcutCommand, FolderGlyph, String)] = [
            (.bold, .bold, "**"), (.italic, .italic, "*"), (.strike, .strike, "~~"),
            (.code, .code, "`"), (.highlight, .highlight, "==")
        ]
        let commands: [FolderPopup.Command] = [
            .init(title: "Undo", keys: "⌘+Z", icon: .undo, enabled: editorUndo.canUndo) { [weak self] in self?.editorUndo.undo() },
            .init(title: "Cut", keys: "⌘+X", icon: .cut, enabled: selection) { [weak self] in self?.source.cut(nil) },
            .init(title: "Copy", keys: "⌘+C", icon: .copy, enabled: selection) { [weak self] in self?.source.copy(nil) },
            .init(title: "Paste", keys: "⌘+V", icon: .paste) { [weak self] in self?.source.paste(nil) },
            .init(title: "Select All", keys: "⌘+A", icon: .selectAll, enabled: !source.string.isEmpty) { [weak self] in self?.source.selectAll(nil) }
        ] + formats.map { command, icon, marker in
            .init(title: command.title, keys: shortcuts[command]?.parts.joined(separator: "+") ?? "",
                  icon: icon, enabled: selection) { [weak self] in self?.format(marker) }
        }
        let colors = PreferenceColors(ink: ink, muted: muted, border: outline, selected: tab, onSelected: onTab)
        folderPopup.openMenu(owner: window, anchor: point, paper: paper(current?.color ?? 0), colors: colors, commands: commands)
    }

    private func resizeImage(_ range: NSRange, width: CGFloat) {
        let text = source.string as NSString
        guard NSMaxRange(range) <= text.length else { return }
        var line = text.substring(with: range)
        let marker = "notedd-width=\(Int(width.rounded()))"
        let pattern = #"(?<=\"|\s)notedd-width=\d+(?=\")"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return }
        let full = NSRange(location: 0, length: (line as NSString).length)
        if let match = regex.firstMatch(in: line, range: full) {
            line = (line as NSString).replacingCharacters(in: match.range, with: marker)
        } else if line.hasSuffix("\")"), line.range(of: " \"", options: .backwards) != nil {
            line.insert(contentsOf: " " + marker, at: line.index(line.endIndex, offsetBy: -2))
        } else if line.hasSuffix(")") {
            line.insert(contentsOf: " \"" + marker + "\"", at: line.index(before: line.endIndex))
        } else { return }
        edit(range, with: line, selection: NSRange(location: range.location, length: 0))
    }

    private func picture(at index: Int) -> NSRange? {
        guard let storage = source.textStorage, (0..<storage.length).contains(index) else { return nil }
        var range = NSRange()
        let decoration = storage.attribute(.simpleDecoration, at: index, effectiveRange: &range) as? SimpleDecoration
        return decoration?.isPicture == true ? range : nil
    }

    private func normalizedSelection(_ range: NSRange, from old: NSRange) -> NSRange {
        guard let storage = source.textStorage else { return range }
        if range.length > 0 {
            let start = picture(at: range.location)?.location ?? range.location
            let end = picture(at: NSMaxRange(range) - 1).map(NSMaxRange) ?? NSMaxRange(range)
            return NSRange(location: start, length: end - start)
        }
        let index = range.location
        guard (0..<storage.length).contains(index) else { return range }
        if let image = picture(at: index), index > image.location {
            let end = NSMaxRange(image)
            return NSRange(location: index < old.location ? image.location : min(end + 1, storage.length), length: 0)
        }
        for key in [NSAttributedString.Key.hiddenMarkdown, .taskMarker, .simpleDecoration] {
            var hidden = NSRange()
            if storage.attribute(key, at: index, longestEffectiveRange: &hidden,
                                 in: NSRange(location: 0, length: storage.length)) != nil, index > hidden.location {
                return NSRange(location: index < old.location ? hidden.location : NSMaxRange(hidden), length: 0)
            }
        }
        return range
    }

    func textView(_ textView: NSTextView, willChangeSelectionFromCharacterRange old: NSRange,
                  toCharacterRange new: NSRange) -> NSRange {
        guard isPreview, !applyingEdit, !source.hasMarkedText() else { return new }
        return normalizedSelection(new, from: old)
    }

    func textView(_ textView: NSTextView, shouldChangeTextIn range: NSRange, replacementString: String?) -> Bool {
        guard let replacementString, !source.hasMarkedText(), !applyingEdit,
              !editorUndo.isUndoing, !editorUndo.isRedoing else { return true }
        return !replaceInSimple(range, with: replacementString)
    }

    private func replaceInSimple(_ range: NSRange, with text: String) -> Bool {
        guard isPreview else { return false }
        if range.length == 0, !text.isEmpty {
            // A standalone Markdown image needs to keep its own paragraph.
            if picture(at: range.location)?.location == range.location {
                let body = text.hasSuffix("\n") ? text : text + "\n"
                edit(range, with: body, selection: NSRange(location: range.location + text.utf16.count, length: 0))
                return true
            }
            if let image = picture(at: range.location - 1), NSMaxRange(image) == range.location {
                edit(range, with: text.hasPrefix("\n") ? text : "\n" + text)
                return true
            }
        }
        guard range.length > 0, let (target, before, after) = NoteddCore.simpleReplacement(source.string, range: range, erase: text.isEmpty)
        else { return false }
        edit(target, with: before + text + after,
             selection: NSRange(location: target.location + before.utf16.count + text.utf16.count, length: 0))
        return true
    }

    private func continueListInSimple() -> Bool {
        let selection = source.selectedRange()
        guard isPreview, selection.length == 0 else { return false }
        let text = source.string as NSString
        guard selection.location <= text.length else { return false }
        let lineRange = text.lineRange(for: selection)
        let lineText = text.substring(with: lineRange).trimmingCharacters(in: .newlines) as NSString
        let lineIndex = (text.substring(to: lineRange.location) as NSString).components(separatedBy: "\n").count - 1
        let lines = NoteddCore.markdown(source.string)
        guard lines.indices.contains(lineIndex) else { return false }
        let syntax = lines[lineIndex]
        let marker = syntax.listRange
        guard marker.length > 0, NSMaxRange(marker) <= lineText.length,
              selection.location >= lineRange.location + NSMaxRange(marker) else { return false }
        let indent = lineText.substring(to: marker.location)
        let prefix = lineText.substring(with: marker)
        let next: String
        switch syntax.list {
        case .none: return false
        case .bullet: next = prefix
        case .task:
            guard let bracket = prefix.range(of: "[") else { return false }
            next = String(prefix[..<bracket.lowerBound]) + "[ ] "
        case .numbered:
            let digits = prefix.prefix(while: { $0.isASCII && $0.isNumber })
            guard let number = Int(digits), number < 999 else { return false }
            next = "\(number + 1)" + prefix.dropFirst(digits.count)
        }
        let body = lineText.substring(from: NSMaxRange(marker))
        if body.trimmingCharacters(in: .whitespaces).isEmpty {
            edit(NSRange(location: lineRange.location + marker.location, length: lineText.length - marker.location), with: "")
        } else {
            edit(selection, with: "\n" + indent + next)
        }
        return true
    }

    private func deleteInSimple(backward: Bool) -> Bool {
        guard isPreview, !source.hasMarkedText(), let storage = source.textStorage else { return false }
        let selection = source.selectedRange()
        let objects = ImageObject.all(in: storage)
        if selection.length > 0 {
            guard let image = objects.first(where: { NSEqualRanges($0.range, selection) }) else { return false }
            edit(image.removal, with: "", selection: NSRange(location: image.removal.location, length: 0))
            return true
        }
        let caret = selection.location
        if let image = objects.first(where: { $0.adjacent(to: caret, backward: backward) }) {
            edit(image.removal, with: "", selection: NSRange(location: image.removal.location, length: 0))
            return true
        }
        let text = source.string as NSString
        var index = backward ? caret - 1 : caret
        var marker = NSRange()
        while (0..<storage.length).contains(index),
              storage.attribute(.hiddenMarkdown, at: index, longestEffectiveRange: &marker,
                                in: NSRange(location: 0, length: storage.length)) != nil {
            index = backward ? marker.location - 1 : NSMaxRange(marker)
        }
        guard (0..<storage.length).contains(index) else { return true }
        let target = text.rangeOfComposedCharacterSequence(at: index)
        if replaceInSimple(target, with: "") { return true }
        if index == (backward ? caret - 1 : caret) { return false }
        edit(target, with: "")
        return true
    }

    private func deleteWordInSimple(backward: Bool) -> Bool {
        guard isPreview, !source.hasMarkedText(), source.selectedRanges.count == 1,
              let storage = source.textStorage else { return false }
        let selection = source.selectedRange()
        let caret = selection.location
        guard selection.length == 0, caret <= storage.length else { return false }

        // Ask AppKit for word boundaries in the visible text, including across paragraphs.
        let (visible, offsets) = visibleText(in: NSRange(location: 0, length: storage.length), storage: storage)
        let position = offsets.prefix { $0 < caret }.count
        let boundary = NSAttributedString(string: visible).nextWord(from: position, forward: !backward)
        let range: NSRange
        if backward {
            guard boundary < position, offsets.indices.contains(boundary) else { return false }
            range = NSRange(location: offsets[boundary], length: caret - offsets[boundary])
        } else {
            guard boundary > position, boundary <= offsets.count else { return false }
            range = NSRange(location: caret, length: offsets[boundary - 1] + 1 - caret)
        }
        // Leave ordinary text and embedded objects to the native handler.
        var hidden = false
        var object = false
        storage.enumerateAttributes(in: range) { attributes, _, _ in
            hidden = hidden || attributes[.hiddenMarkdown] != nil
            object = object || attributes[.taskMarker] != nil || attributes[.simpleDecoration] != nil
        }
        guard hidden, !object else { return false }
        if !replaceInSimple(range, with: "") { edit(range, with: "") }
        return true
    }

    private func formatSimple(_ marker: String) {
        let selection = source.selectedRange()
        guard selection.length > 0,
              let style = ["**": NOTEDD_BOLD, "*": NOTEDD_ITALIC, "~~": NOTEDD_STRIKE,
                           "==": NOTEDD_HIGHLIGHT, "`": NOTEDD_INLINE_CODE][marker] else { return }
        let text = source.string as NSString
        var offset = 0
        var edits: [NoteddCore.FormatEdit] = []
        for (line, syntax) in zip(source.string.components(separatedBy: "\n"), NoteddCore.markdown(source.string)) {
            let start = offset
            let length = line.utf16.count
            offset += length + 1
            if start >= NSMaxRange(selection) { break }
            switch syntax.block {
            case .image, .fence, .code: continue
            default: break
            }
            let lower = max(selection.location - start, syntax.blockMarkerLength, NSMaxRange(syntax.listRange))
            let upper = min(NSMaxRange(selection) - start, length)
            guard lower < upper,
                  let plan = NoteddCore.format(line, selection: NSRange(location: lower, length: upper - lower),
                                               style: UInt8(style), simple: true) else { continue }
            edits.append(.init(range: NSRange(location: start + plan.range.location, length: plan.range.length),
                               marker: plan.marker, wrap: plan.wrap))
        }
        let unwrap = edits.allSatisfy { !$0.wrap }
        edits.removeAll { $0.wrap == unwrap }
        guard let first = edits.first, let last = edits.last else { return }
        let start = first.range.location
        var cursor = start
        var replacement = ""
        for plan in edits {
            replacement += text.substring(with: NSRange(location: cursor, length: plan.range.location - cursor))
            if plan.wrap {
                replacement += plan.marker + text.substring(with: plan.range) + plan.marker
            } else {
                let width = marker.utf16.count
                replacement += text.substring(with: NSRange(location: plan.range.location + width, length: plan.range.length - 2 * width))
            }
            cursor = NSMaxRange(plan.range)
        }
        let leading = first.marker.utf16.count
        let trailing = last.marker.utf16.count
        edit(NSRange(location: start, length: cursor - start), with: replacement,
             selection: NSRange(location: start + leading, length: replacement.utf16.count - leading - trailing))
    }

    private func insertImage(_ data: Data, ext: String) {
        do {
            let path = try store.addImage(data, extension: ext)
            let text = source.string as NSString
            let selection = source.selectedRange()
            let needsNewline = selection.location > 0 && text.character(at: selection.location - 1) != 10
            let image = "\(needsNewline ? "\n" : "")![](\(path))\n"
            if !replaceInSimple(selection, with: image) { edit(selection, with: image) }
        } catch { present(error, message: "Couldn’t save the image") }
    }

    @objc private func formatSelection(_ sender: NSMenuItem) {
        if let marker = sender.representedObject as? String { format(marker) }
    }

    private func format(_ marker: String) {
        if isPreview { formatSimple(marker); return }
        let range = source.selectedRange()
        let string = source.string as NSString
        let width = marker.utf16.count
        if range.length > 0, let style = ["**": NOTEDD_BOLD, "*": NOTEDD_ITALIC, "~~": NOTEDD_STRIKE,
                                          "==": NOTEDD_HIGHLIGHT, "`": NOTEDD_INLINE_CODE][marker] {
            let lineRange = string.lineRange(for: NSRange(location: range.location, length: 0))
            if NSMaxRange(range) <= NSMaxRange(lineRange) {
                let line = string.substring(with: lineRange).trimmingCharacters(in: CharacterSet(charactersIn: "\r\n"))
                let selection = NSRange(location: range.location - lineRange.location, length: range.length)
                guard let plan = NoteddCore.format(line, selection: selection, style: UInt8(style), simple: false) else { return }
                let target = NSRange(location: lineRange.location + plan.range.location, length: plan.range.length)
                let text = string.substring(with: target) as NSString
                if plan.wrap {
                    edit(target, with: plan.marker + (text as String) + plan.marker,
                         selection: NSRange(location: target.location + plan.marker.utf16.count, length: target.length))
                } else {
                    let inner = text.substring(with: NSRange(location: width, length: text.length - width * 2))
                    edit(target, with: inner, selection: NSRange(location: target.location, length: inner.utf16.count))
                }
                return
            }
        }
        let selected = string.substring(with: range)
        if range.length > 0, range.location >= width, NSMaxRange(range) + width <= string.length,
           string.substring(with: NSRange(location: range.location - width, length: width)) == marker,
           string.substring(with: NSRange(location: NSMaxRange(range), length: width)) == marker {
            let expanded = NSRange(location: range.location - width, length: range.length + width * 2)
            edit(expanded, with: selected, selection: NSRange(location: expanded.location, length: range.length))
        } else {
            edit(range, with: marker + selected + marker, selection: NSRange(location: range.location + width, length: range.length))
        }
    }

    @objc private func checkTask(_ sender: Any?) {
        let string = source.string as NSString
        let line = string.lineRange(for: NSRange(location: source.selectedRange().location, length: 0))
        let text = string.substring(with: line).trimmingCharacters(in: CharacterSet(charactersIn: "\r\n"))
        if let syntax = NoteddCore.markdown(text).first, case .task(_, let offset) = syntax.list {
            toggleTask(at: line.location + offset)
        }
    }

    private func toggleTask(at index: Int) {
        let string = source.string as NSString
        guard (0..<string.length).contains(index) else { return }
        let range = NSRange(location: index, length: 1)
        let state = string.substring(with: range)
        guard [" ", "x", "X"].contains(state) else { return }
        edit(range, with: state == " " ? "x" : " ", selection: source.selectedRange())
    }

    @objc private func deleteNote(_ sender: Any?) {
        if let note = listing ? deck.selection : current { deleteNote(id: note.id) }
    }

    private func deleteNote(id: String) {
        guard flushOrReport(), let note = notes.first(where: { $0.id == id }) else { return }
        do {
            try store.trash(note)
            deleted = note
            notes.removeAll { $0.id == note.id }
            if current?.id == note.id { openInitialNote() } else { filterNotes() }
        } catch { present(error, message: "Couldn’t move the note to Trash") }
    }

    @objc private func undoDelete(_ sender: Any?) {
        guard let note = deleted, flushOrReport() else { return }
        do {
            try store.restore(note)
            notes.append(note)
            deleted = nil
            select(note)
            setListing(false)
        } catch { present(error, message: "Couldn’t restore the note") }
    }

    @objc private func togglePin(_ sender: Any?) {
        let pinned = window.level != .floating
        window.level = pinned ? .floating : .normal
        defaults.set(pinned, forKey: "Pinned")
        restyle()
    }

    private func isPaletteToggle(_ event: NSEvent) -> Bool {
        if event.type == .keyDown, let binding = shortcuts[.color] {
            return Int(event.keyCode) == binding.keyCode &&
                event.modifierFlags.intersection(Shortcut.supported) == binding.modifiers
        }
        return event.type == .leftMouseDown && event.window === window && !swatch.isHidden &&
            swatch.bounds.contains(swatch.convert(event.locationInWindow, from: nil))
    }

    @objc private func showColors(_ sender: Any?) {
        guard !settingsOpen, window.isVisible, !window.hiding else { return }
        if folderPopup.isPaletteOpen { folderPopup.close(); return }
        if listing {
            if let note = deck.selection, let anchor = deck.paletteAnchor(for: note) {
                openPalette(for: note, at: anchor, inStack: true)
            }
        } else if let note = current {
            let button = swatch.isHidden ? more : swatch
            let point = CGPoint(x: button.bounds.midX, y: button.bounds.minY)
            openPalette(for: note, at: window.convertPoint(toScreen: button.convert(point, to: nil)), inStack: false)
        }
    }

    private func openPalette(for note: Note, at point: CGPoint, inStack: Bool) {
        let swatches = Self.colorNames.enumerated().map { index, name in
            FolderPopup.Swatch(name: name, color: paper(index)) { [weak self] in
                if inStack { self?.changeStackNoteColor(note.id, to: index) }
                else if self?.current?.id == note.id { self?.changeNoteColor(index) }
            }
        }
        let delete = inStack ? FolderPopup.Command(title: "Move Note to Trash", keys: "", icon: .trash) { [weak self] in
            self?.deleteNote(id: note.id)
        } : nil
        let colors = PreferenceColors(ink: ink, muted: muted, border: outline, selected: tab, onSelected: onTab)
        folderPopup.openPalette(owner: window, anchor: point, paper: paper(0), colors: colors,
                                swatches: swatches, selected: note.color, delete: delete)
    }

    private func changeNoteColor(_ index: Int) {
        guard let current, current.color != index else { return }
        self.current?.color = index
        restyle(animatedColor: true)
        recordEdit()
    }

    private func changeStackNoteColor(_ id: String, to color: Int) {
        guard flushOrReport(), let index = notes.firstIndex(where: { $0.id == id }), notes[index].color != color else { return }
        var note = notes[index]
        note.color = color
        do {
            try store.save(&note)
            notes[index] = note
            if current?.id == id {
                current = note
                restyle(animatedColor: true)
            }
            filterNotes()
        } catch {
            folderPopup.close()
            present(error, message: "Couldn’t change the note color")
        }
    }

    @objc private func changeSize(_ sender: NSMenuItem) {
        setTextSize(sender.tag == 0 ? 16 : Int(markdown.fontSize) + sender.tag)
    }

    private func setTextSize(_ size: Int) {
        defaults.set(Double(min(30, max(11, size))), forKey: "TextSize")
        restyle()
    }

    private func configurePreferences() {
        place(preferencesFolder, in: root)
        preferencesFolder.isHidden = true
        preferencesFolder.showsDarkTab = false
        let heading = PreferencesPanel.label("Settings", color: ink, size: 15)
        preferencesFolder.addSubview(heading)
        preferencesFolder.updateHeader = { [weak self, weak heading] shape in
            heading?.textColor = self?.ink
            heading?.frame = CGRect(x: shape.bounds.minX + 50, y: shape.bounds.minY + 8,
                                    width: shape.tabRight - 56, height: 20)
        }
        place(preferences, in: preferencesFolder, top: 70, side: 58, bottom: 49)
        preferences.onSection = { [weak self] in
            self?.cancelShortcutRecording(refresh: false)
            self?.shortcutError = nil
            self?.refreshPreferences(preserveScroll: false)
        }
        preferences.onClose = { [weak self] in self?.closeSettings() }
    }

    @objc private func showSettings(_ sender: Any?) {
        if settingsOpen { closeSettings(); return }
        folderPopup.close()
        guard flushOrReport() else { return }
        if finding { endFind(nil) }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        preferencesFolder.isHidden = false
        notesSurface.isHidden = true
        refreshPreferences(preserveScroll: false)
        styleControls()
        CATransaction.commit()
        showWindow()
    }

    private func closeSettings() {
        cancelShortcutRecording(refresh: false)
        shortcutError = nil
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        notesSurface.isHidden = false
        preferencesFolder.isHidden = true
        styleControls()
        focusCurrentView()
        CATransaction.commit()
    }

    private func refreshPreferences(preserveScroll: Bool = true) {
        let colors = PreferenceColors(ink: ink, muted: muted, border: outline, selected: tab, onSelected: onTab)
        func choice(_ labels: [String], _ index: Int, _ action: @escaping (Notedd, Int) -> Void) -> NSView {
            PreferencesPanel.choice(labels, selected: index, colors: colors) { [weak self] selected in
                guard let self, selected != index else { return }
                action(self, selected)
            }
        }
        func themeRow() -> NSView {
            let themes = ["paper", "night", "system"]
            let index = themes.firstIndex(of: defaults.string(forKey: "Theme") ?? "paper") ?? 0
            return choice(["Paper", "Night", "System"], index) { app, selected in
                app.defaults.set(themes[selected], forKey: "Theme")
                app.restyle()
            }
        }
        var rows: [(String, NSView)]
        switch preferences.section {
        case .general:
            let directions = WindowAppearance.Direction.allCases
            let speeds = WindowAppearance.Speed.allCases
            let size = Int(markdown.fontSize)
            let sizeRow = PreferencesPanel.buttons([
                ("−", { [weak self] in self?.setTextSize(size - 1) }),
                ("\(size) pt", { [weak self] in self?.setTextSize(16) }),
                ("+", { [weak self] in self?.setTextSize(size + 1) })
            ], colors: colors)
            let folderActions = PreferencesPanel.buttons([
                ("Open", { [weak self] in self?.revealFolder(nil) }),
                ("Choose…", { [weak self] in self?.chooseFolder(nil) })
            ], colors: colors)
            let path = PreferencesPanel.label(store.folder.path, color: muted, size: 11)
            path.lineBreakMode = .byTruncatingMiddle
            path.widthAnchor.constraint(lessThanOrEqualToConstant: 260).isActive = true
            path.toolTip = store.folder.path
            let folder = NSStackView(views: [folderActions, path])
            folder.orientation = .vertical
            folder.alignment = .leading
            folder.spacing = 4
            rows = [
                ("Editor", choice(["Markdown", "Simple"], isPreview ? 1 : 0) { app, index in
                    app.defaults.set(index == 1, forKey: "PreferredPreview")
                    app.setPreview(index == 1)
                    app.refreshPreferences()
                }),
                ("Theme", themeRow()),
                ("Text size", sizeRow),
                ("Show from", choice(directions.map(\.rawValue), directions.firstIndex(of: window.presentation.direction) ?? 0) { app, index in
                    app.setMotionPreference("ShowFrom", directions[index].rawValue)
                }),
                ("Slide distance", choice(["Nearby", "Off-screen"], window.presentation.offScreen ? 1 : 0) { app, index in
                    app.setMotionPreference("SlideOffScreen", index == 1)
                }),
                ("Speed", choice(speeds.map(\.rawValue), speeds.firstIndex(of: window.presentation.speed) ?? 1) { app, index in
                    app.setMotionPreference("SlideSpeed", speeds[index].rawValue)
                }),
                ("Snap to edges", choice(["On", "Off"], window.magnetsEnabled ? 0 : 1) { app, index in
                    app.setMotionPreference("DisableMagnet", index == 1)
                }),
                ("Open at login", choice(["On", "Off"], [.enabled, .requiresApproval].contains(SMAppService.mainApp.status) ? 0 : 1) { app, _ in
                    app.toggleLogin(nil)
                    app.refreshPreferences()
                }),
                ("Notes folder", folder)
            ]
        case .shortcuts:
            rows = ShortcutCommand.allCases.map { command in
                let recording = command == recordingShortcut
                let error = shortcutError?.command == command ? shortcutError?.message :
                    hotKeys?.unavailable.contains(command) == true ? "In use or unavailable" : nil
                let keys = PreferencesPanel.shortcut(shortcuts[command]?.parts, recording: recording, colors: colors) { [weak self] in
                    self?.startShortcutRecording(command)
                }
                if let error {
                    let label = PreferencesPanel.label(error, color: muted, size: 12)
                    label.lineBreakMode = .byTruncatingTail
                    label.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
                    let pair = NSStackView(views: [keys, label])
                    pair.orientation = .horizontal
                    pair.alignment = .centerY
                    pair.spacing = 10
                    return (command.title, pair)
                }
                return (command.title, keys)
            }
            rows.append(("", PreferencesPanel.buttons([
                ("Reset shortcuts", { [weak self] in self?.resetShortcuts() })
            ], colors: colors)))
        case .colors:
            let swatches = Self.colorNames.enumerated().map { ($0.element, paper($0.offset)) }
            rows = [
                ("Theme", themeRow()),
                ("Note color", PreferencesPanel.swatches(swatches, selected: current?.color, colors: colors) { [weak self] index in
                    self?.changeNoteColor(index)
                })
            ]
        }
        preferences.update(rows, colors: colors, preserveScroll: preserveScroll)
    }

    private func startShortcutRecording(_ command: ShortcutCommand) {
        if recordingShortcut == command {
            cancelShortcutRecording()
            return
        }
        recordingShortcut = command
        shortcutError = nil
        hotKeys?.suspend()
        refreshPreferences()
        window.makeFirstResponder(preferences)
    }

    private func cancelShortcutRecording(refresh: Bool = true) {
        guard recordingShortcut != nil else { return }
        recordingShortcut = nil
        hotKeys?.register(shortcuts)
        if refresh && settingsOpen { refreshPreferences() }
    }

    private func settingsKeyEvent(_ event: NSEvent) -> NSEvent? {
        guard settingsOpen, window.isKeyWindow, event.window === window else { return event }
        let modifiers = event.modifierFlags.intersection(Shortcut.supported)
        if let command = recordingShortcut {
            if event.isARepeat { return nil }
            switch event.keyCode {
            case 53:
                cancelShortcutRecording()
            case 51 where modifiers.isEmpty, 117 where modifiers.isEmpty:
                assignShortcut(nil, to: command)
            default:
                if let binding = Shortcut(event: event) { assignShortcut(binding, to: command) }
            }
            return nil
        }
        if event.keyCode == 48, modifiers == .control || modifiers == [.control, .shift] {
            preferences.stepSection(modifiers.contains(.shift) ? -1 : 1)
            return nil
        }
        return event
    }

    private func assignShortcut(_ binding: Shortcut?, to command: ShortcutCommand) {
        if let binding {
            if let existing = ShortcutCommand.allCases.first(where: { other in
                other != command && shortcuts[other]?.overlaps(binding) == true
            }) {
                shortcutError = (command, "Used by \(existing.title)")
                refreshPreferences()
                return
            }
            let nativeKeys = ["a", "c", "v", "x", "z"]
            let reservedEdit = binding.modifiers == .command && nativeKeys.contains(binding.key)
            let reservedRedo = binding.modifiers == [.command, .shift] && binding.key == "z"
            let reservedSettingsTab = binding.key == "\t" &&
                (binding.modifiers == .control || binding.modifiers == [.control, .shift])
            if reservedEdit || reservedRedo || reservedSettingsTab {
                shortcutError = (command, "Reserved shortcut")
                refreshPreferences()
                return
            }
        }
        var proposed = shortcuts
        proposed[command] = binding
        hotKeys?.register(proposed)
        if hotKeys?.unavailable.contains(command) == true {
            hotKeys?.suspend()
            shortcutError = (command, "In use or unavailable")
            refreshPreferences()
            return
        }
        recordingShortcut = nil
        shortcutError = nil
        saveShortcuts(proposed)
    }

    private func resetShortcuts() {
        recordingShortcut = nil
        shortcutError = nil
        let defaults = ShortcutPreferences()
        hotKeys?.register(defaults)
        saveShortcuts(defaults)
    }

    private func saveShortcuts(_ preferences: ShortcutPreferences) {
        shortcuts = preferences
        shortcuts.save(defaults)
        NSApp.mainMenu = makeMainMenu()
        refreshPreferences()
    }

    @objc private func chooseFolder(_ sender: Any?) {
        guard flushOrReport() else { return }
        let panel = NSOpenPanel()
        panel.title = "Choose Notes Folder"
        panel.prompt = "Use Folder"
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.directoryURL = store.folder
        panel.beginSheetModal(for: window) { [weak self] response in
            guard let self, response == .OK, let url = panel.url else { return }
            do {
                let newStore = NoteStore(folder: url)
                let newNotes = try newStore.load()
                self.store = newStore
                self.notes = newNotes
                self.deleted = nil
                self.defaults.set(url.path, forKey: "NotesFolder")
                self.search.stringValue = ""
                self.openInitialNote()
            } catch { self.present(error, message: "Couldn’t open that notes folder") }
        }
    }

    @objc private func revealFolder(_ sender: Any?) { NSWorkspace.shared.open(store.folder) }

    @objc private func toggleLogin(_ sender: Any?) {
        setLogin(![.enabled, .requiresApproval].contains(SMAppService.mainApp.status))
    }

    private func setLogin(_ enabled: Bool) {
        let service = SMAppService.mainApp
        do {
            if enabled && ![.enabled, .requiresApproval].contains(service.status) { try service.register() }
            if !enabled && [.enabled, .requiresApproval].contains(service.status) { try service.unregister() }
            defaults.set(enabled, forKey: "OpenAtLogin")
        } catch { present(error, message: "Couldn’t change Open at Login") }
    }

    private func present(_ error: Error, message: String) {
        let alert = NSAlert(error: error)
        alert.messageText = message
        alert.informativeText = error.localizedDescription
        if window.isVisible && window.attachedSheet == nil { alert.beginSheetModal(for: window) }
        else { alert.runModal() }
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        switch item.action {
        case #selector(undoDelete(_:)): return deleted != nil
        case #selector(deleteNote(_:)): return !notes.isEmpty
        case #selector(formatSelection(_:)), #selector(checkTask(_:)): return !listing && !settingsOpen
        default: return true
        }
    }

    private func item(_ title: String, _ action: Selector, _ key: String = "", modifiers: NSEvent.ModifierFlags = .command) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: key)
        item.keyEquivalentModifierMask = modifiers
        item.target = self
        return item
    }

    private func shortcutItem(_ title: String, _ action: Selector, _ command: ShortcutCommand) -> NSMenuItem {
        let binding = shortcuts[command]
        return item(title, action, binding?.key ?? "", modifiers: binding?.modifiers ?? [])
    }

    private func setMotionPreference(_ key: String, _ value: Any) {
        defaults.set(value, forKey: key)
        updateMotionPreferences()
        refreshPreferences()
    }

    private func updateMotionPreferences() {
        let direction = defaults.string(forKey: "ShowFrom").flatMap(WindowAppearance.Direction.init(rawValue:)) ?? .bottom
        let speed = defaults.string(forKey: "SlideSpeed").flatMap(WindowAppearance.Speed.init(rawValue:)) ?? .normal
        window.presentation = WindowAppearance(direction: direction, speed: speed, offScreen: defaults.bool(forKey: "SlideOffScreen"))
        window.magnetsEnabled = !defaults.bool(forKey: "DisableMagnet")
        window.refreshCorners()
    }

    private func makeMainMenu() -> NSMenu {
        let main = NSMenu()
        func add(_ title: String, items: [NSMenuItem]) {
            let parent = NSMenuItem()
            let menu = NSMenu(title: title)
            for item in items { menu.addItem(item) }
            parent.submenu = menu
            main.addItem(parent)
        }
        add("Notedd", items: [
            NSMenuItem(title: "About Notedd", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: ""),
            .separator(), shortcutItem("Settings…", #selector(showSettings(_:)), .settings), .separator(),
            shortcutItem("Quit Notedd", #selector(quitNotedd(_:)), .quit)
        ])
        add("File", items: [shortcutItem("New Note", #selector(newNote(_:)), .newNote),
                            item("Open Notes Folder", #selector(revealFolder(_:))), .separator(),
                            shortcutItem("Move Note to Trash", #selector(deleteNote(_:)), .trash),
                            shortcutItem("Restore Last Deleted Note", #selector(undoDelete(_:)), .restore)])
        var editItems = [NSMenuItem(title: "Undo", action: Selector(("undo:")), keyEquivalent: "z"),
                         NSMenuItem(title: "Redo", action: Selector(("redo:")), keyEquivalent: "Z"), .separator()]
        for (title, selector, key) in [("Cut", "cut:", "x"), ("Copy", "copy:", "c"), ("Paste", "paste:", "v"), ("Select All", "selectAll:", "a")] {
            editItems.append(NSMenuItem(title: title, action: Selector(selector), keyEquivalent: key))
        }
        editItems += [.separator(), shortcutItem("Find in Note", #selector(findInNote(_:)), .find), .separator()]
        for (command, marker) in [(ShortcutCommand.bold, "**"), (.italic, "*"), (.strike, "~~"), (.highlight, "=="), (.code, "`")] {
            let format = shortcutItem(command.title, #selector(formatSelection(_:)), command)
            format.representedObject = marker
            editItems.append(format)
        }
        editItems.append(shortcutItem("Toggle Checkbox", #selector(checkTask(_:)), .checkbox))
        add("Edit", items: editItems)
        var viewItems = [shortcutItem("All Notes", #selector(toggleList(_:)), .allNotes),
                         shortcutItem("Markdown / Simple", #selector(togglePreview(_:)), .preview),
                         shortcutItem("Always on Top", #selector(togglePin(_:)), .pin),
                         shortcutItem("Note Color", #selector(showColors(_:)), .color), .separator()]
        for (command, amount) in [(ShortcutCommand.larger, 1), (.smaller, -1), (.resetSize, 0)] {
            let size = shortcutItem(command.title, #selector(changeSize(_:)), command)
            size.tag = amount
            viewItems.append(size)
        }
        add("View", items: viewItems)
        return main
    }

    @objc private func quitNotedd(_ sender: Any?) { NSApp.terminate(sender) }
}
