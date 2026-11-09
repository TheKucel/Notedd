import AppKit
import CoreText

enum Geologica {
    static let weightAxis = NSNumber(value: 0x77676874) // wght
    static let slantAxis = NSNumber(value: 0x736C6E74) // slnt
    private static let face = NSFont(name: "Geologica-Thin", size: 16)

    static func font(size: CGFloat, weight: Int = 400, italic: Bool = false) -> NSFont {
        guard let face else {
            let font = NSFont.systemFont(ofSize: size, weight: weight >= 700 ? .bold : weight >= 500 ? .medium : .regular)
            return italic ? NSFontManager.shared.convert(font, toHaveTrait: .italicFontMask) : font
        }
        let axes = [weightAxis: NSNumber(value: weight), slantAxis: NSNumber(value: italic ? -12 : 0)]
        let descriptor = CTFontDescriptorCreateWithAttributes([kCTFontVariationAttribute as String: axes] as CFDictionary)
        return CTFontCreateCopyWithAttributes(face, size, nil, descriptor)
    }

    static func register() {
        if let url = Bundle.main.url(forResource: "Geologica", withExtension: "ttf") {
            CTFontManagerRegisterFontsForURL(url as CFURL, .process, nil)
        }
    }
}
