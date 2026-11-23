import AppKit

let window = CGSize(width: 640, height: 400)
let canvas = CGSize(width: 1200, height: 900)
let iconY: CGFloat = 180
let miraX: CGFloat = 180
let applicationsX: CGFloat = 460
let space = CGColorSpace(name: CGColorSpace.sRGB)!

func color(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(
        srgbRed: CGFloat(hex >> 16 & 255) / 255,
        green: CGFloat(hex >> 8 & 255) / 255,
        blue: CGFloat(hex & 255) / 255,
        alpha: alpha
    )
}

func glow(_ context: CGContext, at center: CGPoint, radius: CGFloat, _ color: CGColor) {
    let gradient = CGGradient(
        colorsSpace: space, colors: [color, color.copy(alpha: 0)!] as CFArray, locations: [0, 1])!
    context.drawRadialGradient(
        gradient, startCenter: center, startRadius: 0, endCenter: center, endRadius: radius, options: [])
}

func sparkle(_ context: CGContext, at center: CGPoint, size: CGFloat, _ color: CGColor) {
    let waist = size * 0.22
    context.move(to: CGPoint(x: center.x, y: center.y - size))
    context.addQuadCurve(to: CGPoint(x: center.x + size, y: center.y), control: CGPoint(x: center.x + waist, y: center.y - waist))
    context.addQuadCurve(to: CGPoint(x: center.x, y: center.y + size), control: CGPoint(x: center.x + waist, y: center.y + waist))
    context.addQuadCurve(to: CGPoint(x: center.x - size, y: center.y), control: CGPoint(x: center.x - waist, y: center.y + waist))
    context.addQuadCurve(to: CGPoint(x: center.x, y: center.y - size), control: CGPoint(x: center.x - waist, y: center.y - waist))
    context.setFillColor(color)
    context.fillPath()
}

func render(scale: CGFloat) -> Data {
    let context = CGContext(
        data: nil, width: Int(canvas.width * scale), height: Int(canvas.height * scale),
        bitsPerComponent: 8, bytesPerRow: 0, space: space,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.translateBy(x: 0, y: canvas.height * scale)
    context.scaleBy(x: scale, y: -scale)

    let dawn = color(0xfdf6f0)
    let dusk = color(0xeeeafc)
    context.setFillColor(dusk)
    context.fill(CGRect(origin: .zero, size: canvas))
    let sky = CGGradient(colorsSpace: space, colors: [dawn, dusk] as CFArray, locations: [0, 1])!
    context.saveGState()
    context.clip(to: CGRect(x: 0, y: 0, width: canvas.width, height: window.height))
    context.drawLinearGradient(sky, start: .zero, end: CGPoint(x: 0, y: window.height), options: [])
    context.restoreGState()

    glow(context, at: CGPoint(x: 120, y: 60), radius: 320, color(0xffd2b8, 0.55))
    glow(context, at: CGPoint(x: 560, y: 360), radius: 340, color(0xcfc6ff, 0.55))
    glow(context, at: CGPoint(x: miraX, y: iconY), radius: 140, color(0xffffff, 0.7))

    let ink: UInt32 = 0x1a1a22
    let start = CGPoint(x: miraX + 78, y: iconY - 8)
    let end = CGPoint(x: applicationsX - 82, y: iconY - 8)
    let apex = CGPoint(x: (miraX + applicationsX) / 2, y: iconY - 92)
    let dots = 11
    for index in 0...dots {
        let t = CGFloat(index) / CGFloat(dots)
        let u = 1 - t
        let point = CGPoint(
            x: u * u * start.x + 2 * u * t * apex.x + t * t * end.x,
            y: u * u * start.y + 2 * u * t * apex.y + t * t * end.y)
        let radius = 1.2 + t * 1.3
        context.setFillColor(color(ink, 0.16 + t * 0.42))
        context.fillEllipse(in: CGRect(x: point.x - radius, y: point.y - radius, width: radius * 2, height: radius * 2))
    }
    sparkle(context, at: CGPoint(x: apex.x + 30, y: apex.y + 2), size: 5, color(0x8f7cf0, 0.75))
    sparkle(context, at: CGPoint(x: apex.x - 44, y: apex.y + 14), size: 3, color(0xf0a07c, 0.7))

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: true)
    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = .center
    NSAttributedString(
        string: "Drag Mira into Applications to begin",
        attributes: [
            .font: NSFont.systemFont(ofSize: 13, weight: .medium),
            .foregroundColor: NSColor(cgColor: color(ink, 0.5))!,
            .paragraphStyle: paragraph,
        ]
    ).draw(in: CGRect(x: 0, y: window.height - 68, width: window.width, height: 20))
    NSGraphicsContext.restoreGraphicsState()

    let image = NSBitmapImageRep(cgImage: context.makeImage()!)
    image.size = canvas
    return image.representation(using: .png, properties: [:])!
}

let directory = URL(fileURLWithPath: CommandLine.arguments[1])
try render(scale: 1).write(to: directory.appendingPathComponent("background.png"))
try render(scale: 2).write(to: directory.appendingPathComponent("background@2x.png"))
