import AppKit
import CoreText

let data = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
let sample = CommandLine.arguments[2]
let inter = CTFontManagerCreateFontDescriptorFromData(data as CFData)!
let weightAxis = NSNumber(value: 0x77676874) // OpenType 'wght'

for (weight, systemWeight) in [(400, NSFont.Weight.regular), (500, .medium), (600, .semibold)] {
    let instance = CTFontDescriptorCreateCopyWithVariation(inter, weightAxis, CGFloat(weight))
    for size: CGFloat in [11, 13] {
        let faces: [(String, CTFont)] = [
            (".SystemUIFont", NSFont.systemFont(ofSize: size, weight: systemWeight)),
            ("Inter", CTFontCreateWithFontDescriptor(instance, size, nil)),
        ]
        for (family, font) in faces {
            let text = NSAttributedString(string: sample, attributes: [.font: font])
            let line = CTLineCreateWithAttributedString(text)
            let width = CTLineGetTypographicBounds(line, nil, nil, nil)
            print("\(family)\t\(weight)\t\(Int(size))\t\(width)")
        }
    }
}
