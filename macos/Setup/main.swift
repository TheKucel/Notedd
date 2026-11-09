import AppKit

let application = NSApplication.shared
let delegate = Setup()
application.delegate = delegate
application.setActivationPolicy(.regular)
application.run()
