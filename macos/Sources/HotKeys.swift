import AppKit
import Carbon

final class HotKeys {
    private var handler: EventHandlerRef?
    private var registrations: [EventHotKeyRef] = []
    private let callback: (ShortcutCommand) -> Void
    private(set) var unavailable: Set<ShortcutCommand> = []

    init(callback: @escaping (ShortcutCommand) -> Void) {
        self.callback = callback
        var event = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        let target = GetApplicationEventTarget()
        InstallEventHandler(target, { _, event, context in
            guard let event, let context else { return OSStatus(eventNotHandledErr) }
            var key = EventHotKeyID()
            let result = GetEventParameter(event, EventParamName(kEventParamDirectObject), EventParamType(typeEventHotKeyID),
                                           nil, MemoryLayout<EventHotKeyID>.size, nil, &key)
            let index = Int(key.id) - 1
            guard result == noErr, key.signature == 0x4E544444, ShortcutCommand.globals.indices.contains(index) else {
                return OSStatus(eventNotHandledErr)
            }
            Unmanaged<HotKeys>.fromOpaque(context).takeUnretainedValue().callback(ShortcutCommand.globals[index])
            return noErr
        }, 1, &event, Unmanaged.passUnretained(self).toOpaque(), &handler)
    }

    func suspend() {
        for key in registrations { UnregisterEventHotKey(key) }
        registrations.removeAll()
    }

    func register(_ bindings: ShortcutPreferences) {
        suspend()
        unavailable.removeAll()
        let target = GetApplicationEventTarget()
        for (index, command) in ShortcutCommand.globals.enumerated() {
            guard let binding = bindings[command] else { continue }
            let flags: [(NSEvent.ModifierFlags, Int)] = [(.control, controlKey), (.option, optionKey),
                                                        (.shift, shiftKey), (.command, cmdKey)]
            let modifiers = flags.reduce(0) { $0 | (binding.modifiers.contains($1.0) ? $1.1 : 0) }
            var reference: EventHotKeyRef?
            let id = EventHotKeyID(signature: 0x4E544444, id: UInt32(index + 1))
            if handler != nil,
               RegisterEventHotKey(UInt32(binding.keyCode), UInt32(modifiers), id, target, 0, &reference) == noErr,
               let reference {
                registrations.append(reference)
            } else {
                unavailable.insert(command)
            }
        }
    }

    deinit {
        suspend()
        if let handler { RemoveEventHandler(handler) }
    }
}
