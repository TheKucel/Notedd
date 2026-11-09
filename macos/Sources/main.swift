import AppKit

let application = NSApplication.shared
let delegate = Notedd()
application.delegate = delegate
application.setActivationPolicy(.accessory)
application.run()
