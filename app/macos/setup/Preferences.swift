import AppKit
import Carbon

/// Reads and writes the settings.txt shared with the current Rust application.
enum Preferences {
    static var file: URL {
        if let root = ProcessInfo.processInfo.environment["NOTEDD_SETUP_TEST_DIR"] { return URL(fileURLWithPath: root).appendingPathComponent("settings.txt") }
        return FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Notedd/settings.txt")
    }
    static var loginFile: URL {
        if let root = ProcessInfo.processInfo.environment["NOTEDD_SETUP_TEST_DIR"] { return URL(fileURLWithPath: root).appendingPathComponent("login.plist") }
        return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/LaunchAgents/app.notedd.plist")
    }

    static func read() -> [String: String] {
        let contents = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
        return contents.split(separator: "\n").reduce(into: [String: String]()) { result, line in
            let pair = line.split(separator: "=", maxSplits: 1, omittingEmptySubsequences: false)
            guard pair.count == 2 else { return }
            result[String(pair[0]).trimmingCharacters(in: .whitespaces)] = String(pair[1]).trimmingCharacters(in: .whitespaces)
        }
    }

    // Virtual-key codes are the format used by Notedd's cross-platform preferences.
    static let keys: [(Int, UInt32, String)] = [
        (kVK_ANSI_A, 0x41, "a"), (kVK_ANSI_B, 0x42, "b"), (kVK_ANSI_C, 0x43, "c"), (kVK_ANSI_D, 0x44, "d"),
        (kVK_ANSI_E, 0x45, "e"), (kVK_ANSI_F, 0x46, "f"), (kVK_ANSI_G, 0x47, "g"), (kVK_ANSI_H, 0x48, "h"),
        (kVK_ANSI_I, 0x49, "i"), (kVK_ANSI_J, 0x4a, "j"), (kVK_ANSI_K, 0x4b, "k"), (kVK_ANSI_L, 0x4c, "l"),
        (kVK_ANSI_M, 0x4d, "m"), (kVK_ANSI_N, 0x4e, "n"), (kVK_ANSI_O, 0x4f, "o"), (kVK_ANSI_P, 0x50, "p"),
        (kVK_ANSI_Q, 0x51, "q"), (kVK_ANSI_R, 0x52, "r"), (kVK_ANSI_S, 0x53, "s"), (kVK_ANSI_T, 0x54, "t"),
        (kVK_ANSI_U, 0x55, "u"), (kVK_ANSI_V, 0x56, "v"), (kVK_ANSI_W, 0x57, "w"), (kVK_ANSI_X, 0x58, "x"),
        (kVK_ANSI_Y, 0x59, "y"), (kVK_ANSI_Z, 0x5a, "z"), (kVK_Space, 0x20, " "),
        (kVK_ANSI_0, 0x30, "0"), (kVK_ANSI_1, 0x31, "1"), (kVK_ANSI_2, 0x32, "2"), (kVK_ANSI_3, 0x33, "3"),
        (kVK_ANSI_4, 0x34, "4"), (kVK_ANSI_5, 0x35, "5"), (kVK_ANSI_6, 0x36, "6"), (kVK_ANSI_7, 0x37, "7"),
        (kVK_ANSI_8, 0x38, "8"), (kVK_ANSI_9, 0x39, "9"), (kVK_ANSI_Quote, 0xde, "'"),
        (kVK_ANSI_Comma, 0xbc, ","), (kVK_ANSI_Period, 0xbe, "."), (kVK_ANSI_Minus, 0xbd, "-"),
        (kVK_ANSI_Equal, 0xbb, "="), (kVK_ANSI_Slash, 0xbf, "/"), (kVK_ANSI_Semicolon, 0xba, ";"),
        (kVK_ANSI_LeftBracket, 0xdb, "["), (kVK_ANSI_RightBracket, 0xdd, "]"), (kVK_ANSI_Backslash, 0xdc, "\\"),
        (kVK_ANSI_Grave, 0xc0, "`"), (kVK_Return, 0x0d, "\r"), (kVK_Tab, 0x09, "\t"),
        (kVK_Delete, 0x08, "\u{7f}"), (kVK_ForwardDelete, 0x2e, String(UnicodeScalar(NSDeleteFunctionKey)!))
    ]

    static func encode(_ shortcut: Shortcut) -> String? {
        guard let key = keys.first(where: { $0.0 == shortcut.keyCode }) else { return nil }
        let m = shortcut.modifiers
        let flags = (m.contains(.option) ? 1 : 0) | (m.contains(.control) ? 2 : 0) | (m.contains(.shift) ? 4 : 0) | (m.contains(.command) ? 8 : 0)
        return "\(flags),\(key.1)"
    }

    static func decode(_ value: String?) -> Shortcut? {
        guard let value else { return nil }
        let parts = value.split(separator: ",").compactMap { UInt32($0) }
        guard parts.count == 2, let key = keys.first(where: { $0.1 == parts[1] }) else { return nil }
        var flags: NSEvent.ModifierFlags = []
        if parts[0] & 1 != 0 { flags.insert(.option) }
        if parts[0] & 2 != 0 { flags.insert(.control) }
        if parts[0] & 4 != 0 { flags.insert(.shift) }
        if parts[0] & 8 != 0 { flags.insert(.command) }
        return Shortcut(key.0, key.2, flags)
    }

    static func save(theme: Int, simple: Bool, language: String, shortcut: Shortcut, notes: URL) throws {
        guard let key = encode(shortcut), !notes.path.contains("\n") else { throw CocoaError(.fileWriteInvalidFileName) }
        let changes = ["theme": String(theme), "editor": simple ? "simple" : "markdown", "language": language == "zh-Hans" ? "zh-CN" : language,
                       "key0": key, "notes_dir": notes.path]
        let old = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
        var lines = old.split(separator: "\n").map(String.init).filter { line in
            guard let at = line.firstIndex(of: "=") else { return true }
            return changes[String(line[..<at]).trimmingCharacters(in: .whitespaces)] == nil
        }
        lines += changes.keys.sorted().map { "\($0)=\(changes[$0]!)" }
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try (lines.joined(separator: "\n") + "\n").write(to: file, atomically: true, encoding: .utf8)
    }

    static func setLogin(_ on: Bool, destination: URL) throws {
        let files = FileManager.default
        if !on { if files.fileExists(atPath: loginFile.path) { try files.removeItem(at: loginFile) }; return }
        try files.createDirectory(at: loginFile.deletingLastPathComponent(), withIntermediateDirectories: true)
        let plist: [String: Any] = ["Label": "app.notedd", "ProgramArguments": [destination.appendingPathComponent("Contents/MacOS/Notedd").path, "--background"], "RunAtLoad": true]
        try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0).write(to: loginFile, options: .atomic)
    }
}
