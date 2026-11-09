import AppKit
import Carbon

enum ShortcutCommand: String, CaseIterable {
    case toggleWindow, globalNew, newNote, allNotes, find, settings, pin, quit, color, trash, highlight
    case preview, restore, bold, italic, strike, code, checkbox, larger, smaller, resetSize

    static let globals: [Self] = [.toggleWindow, .globalNew]

    var title: String {
        switch self {
        case .toggleWindow: return "Show or hide globally"
        case .globalNew: return "New note globally"
        case .newNote: return "New note"
        case .allNotes: return "All notes"
        case .find: return "Find in note"
        case .settings: return "Settings"
        case .pin: return "Always on top"
        case .quit: return "Quit Notedd"
        case .color: return "Note color"
        case .trash: return "Move to Trash"
        case .highlight: return "Highlight"
        case .preview: return "Markdown / Simple"
        case .restore: return "Restore last deleted note"
        case .bold: return "Bold"
        case .italic: return "Italic"
        case .strike: return "Strikethrough"
        case .code: return "Inline code"
        case .checkbox: return "Toggle checkbox"
        case .larger: return "Increase text size"
        case .smaller: return "Decrease text size"
        case .resetSize: return "Reset text size"
        }
    }

    var defaultBinding: Shortcut {
        let command: NSEvent.ModifierFlags = .command
        switch self {
        case .toggleWindow: return Shortcut(kVK_Space, " ", [.control, .option])
        case .globalNew: return Shortcut(kVK_ANSI_N, "n", [.control, .option])
        case .newNote: return Shortcut(kVK_ANSI_N, "n", command)
        case .allNotes: return Shortcut(kVK_ANSI_P, "p", command)
        case .find: return Shortcut(kVK_ANSI_F, "f", command)
        case .settings: return Shortcut(kVK_ANSI_Comma, ",", command)
        case .pin: return Shortcut(kVK_ANSI_T, "t", command)
        case .quit: return Shortcut(kVK_ANSI_Q, "q", command)
        case .color: return Shortcut(kVK_ANSI_C, "c", [.command, .shift])
        case .trash: return Shortcut(kVK_Delete, "\u{7f}", [.command, .shift])
        case .highlight: return Shortcut(kVK_ANSI_H, "h", [.command, .shift])
        case .preview: return Shortcut(kVK_ANSI_M, "m", [.command, .shift])
        case .restore: return Shortcut(kVK_ANSI_Z, "z", [.command, .option])
        case .bold: return Shortcut(kVK_ANSI_B, "b", command)
        case .italic: return Shortcut(kVK_ANSI_I, "i", command)
        case .strike: return Shortcut(kVK_ANSI_S, "s", [.command, .shift])
        case .code: return Shortcut(kVK_ANSI_E, "e", command)
        case .checkbox: return Shortcut(kVK_Return, "\r", command)
        case .larger: return Shortcut(kVK_ANSI_Equal, "=", command)
        case .smaller: return Shortcut(kVK_ANSI_Minus, "-", command)
        case .resetSize: return Shortcut(kVK_ANSI_0, "0", command)
        }
    }
}

struct Shortcut: Codable, Equatable {
    let keyCode: Int
    let key: String
    private let flags: UInt

    static let supported: NSEvent.ModifierFlags = [.control, .option, .shift, .command]

    var modifiers: NSEvent.ModifierFlags { NSEvent.ModifierFlags(rawValue: flags).intersection(Self.supported) }

    init(_ keyCode: Int, _ key: String, _ modifiers: NSEvent.ModifierFlags) {
        self.keyCode = keyCode
        self.key = key
        flags = modifiers.intersection(Self.supported).rawValue
    }

    init?(event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(Self.supported)
        let code = Int(event.keyCode)
        let key: String
        switch code {
        case kVK_Delete: key = "\u{7f}"
        case kVK_ForwardDelete: key = String(UnicodeScalar(NSDeleteFunctionKey)!)
        case kVK_Return: key = "\r"
        case kVK_ANSI_KeypadEnter: key = "\u{3}"
        case kVK_Tab: key = "\t"
        default:
            // Ask AppKit for the base character. The Shift modifier is recorded separately.
            guard let value = event.characters(byApplyingModifiers: []) ?? event.charactersIgnoringModifiers,
                  !value.isEmpty, value.utf16.count == 1 else { return nil }
            key = value.lowercased()
        }
        let functionKey = key.unicodeScalars.first.map { (NSF1FunctionKey...NSF35FunctionKey).contains(Int($0.value)) } ?? false
        guard functionKey || !modifiers.intersection([.control, .option, .command]).isEmpty else { return nil }
        self.init(code, key, modifiers)
    }

    func overlaps(_ other: Self) -> Bool {
        modifiers == other.modifiers && (keyCode == other.keyCode || key == other.key)
    }

    var parts: [String] {
        let prefix: [(NSEvent.ModifierFlags, String)] = [(.control, "Ctrl"), (.option, "Option"), (.shift, "Shift"), (.command, "⌘")]
        return prefix.compactMap { modifiers.contains($0.0) ? $0.1 : nil } + [keyName]
    }

    private var keyName: String {
        if let scalar = key.unicodeScalars.first, (NSF1FunctionKey...NSF35FunctionKey).contains(Int(scalar.value)) {
            return "F\(Int(scalar.value) - NSF1FunctionKey + 1)"
        }
        switch keyCode {
        case kVK_Space: return "Space"
        case kVK_Delete: return "⌫"
        case kVK_ForwardDelete: return "⌦"
        case kVK_Tab: return "Tab"
        case kVK_Return, kVK_ANSI_KeypadEnter: return "↩"
        case kVK_UpArrow: return "↑"
        case kVK_DownArrow: return "↓"
        case kVK_LeftArrow: return "←"
        case kVK_RightArrow: return "→"
        case kVK_Home: return "Home"
        case kVK_End: return "End"
        case kVK_PageUp: return "Page Up"
        case kVK_PageDown: return "Page Down"
        default: return key.uppercased()
        }
    }
}

struct ShortcutPreferences: Codable {
    private var custom: [String: Shortcut] = [:]
    private var cleared: Set<String> = []

    subscript(command: ShortcutCommand) -> Shortcut? {
        get { cleared.contains(command.rawValue) ? nil : custom[command.rawValue] ?? command.defaultBinding }
        set {
            custom[command.rawValue] = newValue
            if newValue == nil { cleared.insert(command.rawValue) }
            else { cleared.remove(command.rawValue) }
        }
    }

    static func load(_ defaults: UserDefaults) -> Self {
        guard let data = defaults.data(forKey: "Shortcuts"),
              let preferences = try? PropertyListDecoder().decode(Self.self, from: data) else { return Self() }
        return preferences
    }

    func save(_ defaults: UserDefaults) {
        if let data = try? PropertyListEncoder().encode(self) { defaults.set(data, forKey: "Shortcuts") }
    }
}
