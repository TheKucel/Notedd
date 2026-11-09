import AppKit

// An image occupies one Markdown paragraph, but behaves as one object in Simple mode.
struct ImageObject {
    let range: NSRange
    let before: Int
    let after: Int
    let removal: NSRange

    static func all(in storage: NSTextStorage) -> [ImageObject] {
        let text = storage.string as NSString
        var objects: [ImageObject] = []
        guard storage.length > 0 else { return objects }
        storage.enumerateAttribute(.simpleDecoration, in: NSRange(location: 0, length: storage.length)) { value, range, _ in
            guard (value as? SimpleDecoration)?.isPicture == true else { return }
            let end = NSMaxRange(range)
            let hasPreviousNewline = range.location > 0 && text.character(at: range.location - 1) == 10
            let hasFollowingNewline = end < text.length && text.character(at: end) == 10
            let before = hasPreviousNewline ? range.location - 1 : range.location
            let after = hasFollowingNewline ? end + 1 : end
            let removal = hasFollowingNewline ? NSRange(location: range.location, length: range.length + 1)
                : hasPreviousNewline ? NSRange(location: before, length: range.length + 1) : range
            objects.append(ImageObject(range: range, before: before, after: after, removal: removal))
        }
        return objects
    }

    func adjacent(to caret: Int, backward: Bool) -> Bool {
        backward ? (caret == after || caret == NSMaxRange(range)) : (caret == before || caret == range.location)
    }
}
