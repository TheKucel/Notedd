import AppKit

@main struct ImageEditingTests {
    static func main() {
        let image = NSImage(size: NSSize(width: 30, height: 30))
        func check(_ value: Bool, _ message: String) {
            if !value { fatalError(message) }
        }
        func objects(_ text: String) -> (NSTextStorage, [ImageObject]) {
            let storage = NSTextStorage(string: text)
            let ns = text as NSString
            var search = NSRange(location: 0, length: ns.length)
            while search.length > 0 {
                let range = ns.range(of: "![](x)", options: [], range: search)
                if range.location == NSNotFound { break }
                storage.addAttribute(.simpleDecoration, value: SimpleDecoration.picture(image, NSSize(width: 30, height: 30)), range: range)
                search = NSRange(location: NSMaxRange(range), length: ns.length - NSMaxRange(range))
            }
            return (storage, ImageObject.all(in: storage))
        }
        let (storage, images) = objects("title\n1. one\n2. two \n![](x)\n")
        check(images.count == 1, "image must be found")
        let photo = images[0]
        check(photo.adjacent(to: photo.after, backward: true), "Backspace after image")
        check(photo.adjacent(to: photo.before, backward: false), "Delete before image")
        check(!photo.adjacent(to: photo.before, backward: true), "Backspace before image must delete text")
        check((storage.string as NSString).replacingCharacters(in: photo.removal, with: "") == "title\n1. one\n2. two \n", "remove image without damaging list")
        let (last, lastImages) = objects("text\n![](x)")
        check((last.string as NSString).replacingCharacters(in: lastImages[0].removal, with: "") == "text", "remove final image without newline")
        let (_, consecutive) = objects("text\n![](x)\n![](x)\n")
        check(consecutive.count == 2, "consecutive images remain distinct")
        check(consecutive[0].after == consecutive[1].range.location, "shared insertion point")
        print("Image editing boundary checks passed")
    }
}
