import AppKit

if CommandLine.arguments.contains("--self-test") {
    guard let root = ProcessInfo.processInfo.environment["NOTEDD_SETUP_TEST_DIR"],
          let payload = Bundle.main.url(forResource: "Notedd-payload", withExtension: "zip") else { fatalError("Missing test directory or payload") }
    do {
        let base = URL(fileURLWithPath: root)
        let install = Installation(directory: base.appendingPathComponent("Applications"), notes: base.appendingPathComponent("Notes"), desktopAlias: false)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        try "scale=120\nunknown=value\n".write(to: Preferences.file, atomically: true, encoding: .utf8)
        let shortcut = ShortcutCommand.toggleWindow.defaultBinding
        _ = try install.install(payload: payload)
        _ = try install.install(payload: payload) // update an existing Notedd bundle
        try Preferences.save(theme: 2, simple: true, language: "ru", shortcut: shortcut, notes: install.notes)
        let saved = Preferences.read()
        guard saved["scale"] == "120", saved["unknown"] == "value", saved["theme"] == "2",
              saved["editor"] == "simple", saved["language"] == "ru", saved["key0"] == "3,32",
              saved["notes_dir"] == install.notes.path else { fatalError("Settings did not round-trip") }
        try Preferences.setLogin(true, destination: install.destination)
        guard FileManager.default.fileExists(atPath: Preferences.loginFile.path) else { fatalError("Login item missing") }
        try Preferences.setLogin(false, destination: install.destination)
        print("Installer self-test passed")
        exit(0)
    } catch { fputs("Installer self-test failed: \(error)\n", stderr); exit(1) }
}

let application = NSApplication.shared
let delegate = Setup()
application.delegate = delegate
application.setActivationPolicy(.regular)
application.run()
