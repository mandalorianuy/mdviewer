#!/usr/bin/env swift

import AppKit
import Foundation

struct IconSpec {
    let filename: String
    let pointSize: CGFloat
}

struct IconAsset {
    let name: String
    let draw: (NSRect) -> Void
}

let specs: [IconSpec] = [
    .init(filename: "icon_16x16.png", pointSize: 16),
    .init(filename: "icon_16x16@2x.png", pointSize: 32),
    .init(filename: "icon_32x32.png", pointSize: 32),
    .init(filename: "icon_32x32@2x.png", pointSize: 64),
    .init(filename: "icon_128x128.png", pointSize: 128),
    .init(filename: "icon_128x128@2x.png", pointSize: 256),
    .init(filename: "icon_256x256.png", pointSize: 256),
    .init(filename: "icon_256x256@2x.png", pointSize: 512),
    .init(filename: "icon_512x512.png", pointSize: 512),
    .init(filename: "icon_512x512@2x.png", pointSize: 1024),
]

let fileManager = FileManager.default
let rootURL = URL(fileURLWithPath: fileManager.currentDirectoryPath)

let assets: [IconAsset] = [
    .init(name: "AppIcon", draw: drawAppIcon),
    .init(name: "MarkdownDocument", draw: drawDocumentIcon),
]

for asset in assets {
    try generateIconAsset(named: asset.name, draw: asset.draw)
}

func generateIconAsset(named name: String, draw: (NSRect) -> Void) throws {
    let iconsetURL = rootURL.appendingPathComponent("macos/\(name).iconset", isDirectory: true)
    let icnsURL = rootURL.appendingPathComponent("macos/\(name).icns")

    try? fileManager.removeItem(at: iconsetURL)
    try fileManager.createDirectory(at: iconsetURL, withIntermediateDirectories: true)

    for spec in specs {
        let image = NSImage(size: NSSize(width: spec.pointSize, height: spec.pointSize))
        image.lockFocus()
        draw(NSRect(origin: .zero, size: image.size))
        image.unlockFocus()

        guard
            let tiffData = image.tiffRepresentation,
            let bitmap = NSBitmapImageRep(data: tiffData),
            let pngData = bitmap.representation(using: .png, properties: [:])
        else {
            throw NSError(
                domain: "IconGeneration",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "No se pudo generar el PNG \(spec.filename) para \(name)."]
            )
        }

        try pngData.write(to: iconsetURL.appendingPathComponent(spec.filename))
    }

    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
    process.arguments = ["-c", "icns", iconsetURL.path, "-o", icnsURL.path]
    try process.run()
    process.waitUntilExit()

    guard process.terminationStatus == 0 else {
        throw NSError(
            domain: "IconGeneration",
            code: Int(process.terminationStatus),
            userInfo: [NSLocalizedDescriptionKey: "iconutil no pudo crear \(name).icns"]
        )
    }
}

func drawAppIcon(in rect: NSRect) {
    let size = min(rect.width, rect.height)
    let corner = size * 0.225

    let background = NSBezierPath(
        roundedRect: rect.insetBy(dx: size * 0.02, dy: size * 0.02),
        xRadius: corner,
        yRadius: corner
    )
    let baseGradient = NSGradient(colors: [
        NSColor(calibratedRed: 0.98, green: 0.93, blue: 0.84, alpha: 1),
        NSColor(calibratedRed: 0.91, green: 0.46, blue: 0.27, alpha: 1)
    ])!
    baseGradient.draw(in: background, angle: -42)

    let topGlow = NSBezierPath(ovalIn: NSRect(
        x: size * 0.07,
        y: size * 0.56,
        width: size * 0.58,
        height: size * 0.38
    ))
    NSColor(calibratedWhite: 1, alpha: 0.28).setFill()
    topGlow.fill()

    let lowerShape = NSBezierPath(ovalIn: NSRect(
        x: size * 0.42,
        y: size * 0.08,
        width: size * 0.48,
        height: size * 0.28
    ))
    NSColor(calibratedRed: 0.37, green: 0.16, blue: 0.14, alpha: 0.14).setFill()
    lowerShape.fill()

    let pageRect = standardPageRect(size: size)
    drawPaperPage(in: pageRect, size: size, includeShadow: true)
    drawPageContent(in: pageRect, size: size, title: "mD", titleYOffset: 0.38)

    let rim = NSBezierPath(
        roundedRect: rect.insetBy(dx: size * 0.02, dy: size * 0.02),
        xRadius: corner,
        yRadius: corner
    )
    NSColor(calibratedWhite: 1, alpha: 0.18).setStroke()
    rim.lineWidth = max(1, size * 0.01)
    rim.stroke()
}

func drawDocumentIcon(in rect: NSRect) {
    let size = min(rect.width, rect.height)
    let pageRect = NSRect(
        x: size * 0.15,
        y: size * 0.1,
        width: size * 0.7,
        height: size * 0.8
    )

    drawPaperPage(in: pageRect, size: size, includeShadow: true)
    drawPageContent(in: pageRect, size: size, title: "md", titleYOffset: 0.43)

    let badgeRect = NSRect(
        x: pageRect.maxX - size * 0.26,
        y: pageRect.minY + size * 0.02,
        width: size * 0.22,
        height: size * 0.22
    )
    let badge = NSBezierPath(roundedRect: badgeRect, xRadius: size * 0.07, yRadius: size * 0.07)
    let badgeGradient = NSGradient(colors: [
        NSColor(calibratedRed: 0.91, green: 0.46, blue: 0.27, alpha: 1),
        NSColor(calibratedRed: 0.76, green: 0.27, blue: 0.16, alpha: 1)
    ])!
    badgeGradient.draw(in: badge, angle: -90)

    let badgeParagraph = NSMutableParagraphStyle()
    badgeParagraph.alignment = .center
    let badgeAttributes: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: size * 0.095, weight: .bold),
        .foregroundColor: NSColor.white,
        .paragraphStyle: badgeParagraph
    ]
    let badgeTextRect = badgeRect.offsetBy(dx: 0, dy: size * 0.014)
    NSString(string: ".md").draw(in: badgeTextRect, withAttributes: badgeAttributes)
}

func standardPageRect(size: CGFloat) -> NSRect {
    NSRect(
        x: size * 0.18,
        y: size * 0.14,
        width: size * 0.64,
        height: size * 0.72
    )
}

func drawPaperPage(in pageRect: NSRect, size: CGFloat, includeShadow: Bool) {
    if includeShadow {
        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow()
        shadow.shadowBlurRadius = size * 0.05
        shadow.shadowOffset = NSSize(width: 0, height: -size * 0.018)
        shadow.shadowColor = NSColor(calibratedWhite: 0, alpha: 0.18)
        shadow.set()
    }

    let pageRadius = size * 0.09
    let pagePath = NSBezierPath(roundedRect: pageRect, xRadius: pageRadius, yRadius: pageRadius)
    NSColor(calibratedRed: 0.995, green: 0.992, blue: 0.985, alpha: 1).setFill()
    pagePath.fill()

    if includeShadow {
        NSGraphicsContext.restoreGraphicsState()
    }

    let accentBar = NSBezierPath(roundedRect: NSRect(
        x: pageRect.minX + size * 0.04,
        y: pageRect.minY + size * 0.1,
        width: size * 0.035,
        height: pageRect.height - size * 0.18
    ), xRadius: size * 0.018, yRadius: size * 0.018)
    NSColor(calibratedRed: 0.95, green: 0.49, blue: 0.24, alpha: 1).setFill()
    accentBar.fill()

    let fold = NSBezierPath()
    let foldSize = size * 0.17
    fold.move(to: NSPoint(x: pageRect.maxX - foldSize, y: pageRect.maxY))
    fold.line(to: NSPoint(x: pageRect.maxX, y: pageRect.maxY))
    fold.line(to: NSPoint(x: pageRect.maxX, y: pageRect.maxY - foldSize))
    fold.close()
    NSColor(calibratedRed: 0.95, green: 0.89, blue: 0.79, alpha: 1).setFill()
    fold.fill()

    let foldLine = NSBezierPath()
    foldLine.move(to: NSPoint(x: pageRect.maxX - foldSize, y: pageRect.maxY))
    foldLine.line(to: NSPoint(x: pageRect.maxX - foldSize, y: pageRect.maxY - foldSize))
    foldLine.line(to: NSPoint(x: pageRect.maxX, y: pageRect.maxY - foldSize))
    NSColor(calibratedRed: 0.86, green: 0.77, blue: 0.66, alpha: 1).setStroke()
    foldLine.lineWidth = max(1.2, size * 0.008)
    foldLine.stroke()
}

func drawPageContent(in pageRect: NSRect, size: CGFloat, title: String, titleYOffset: CGFloat) {
    let titleRect = NSRect(
        x: pageRect.minX + size * 0.12,
        y: pageRect.minY + size * titleYOffset,
        width: pageRect.width - size * 0.2,
        height: size * 0.2
    )
    let titleParagraph = NSMutableParagraphStyle()
    titleParagraph.alignment = .center
    let titleAttributes: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: size * 0.18, weight: .bold),
        .foregroundColor: NSColor(calibratedRed: 0.18, green: 0.21, blue: 0.26, alpha: 1),
        .paragraphStyle: titleParagraph
    ]
    NSString(string: title).draw(in: titleRect, withAttributes: titleAttributes)

    let sublineColor = NSColor(calibratedRed: 0.65, green: 0.69, blue: 0.74, alpha: 1)
    let headingLine = NSBezierPath(roundedRect: NSRect(
        x: pageRect.minX + size * 0.14,
        y: pageRect.minY + size * 0.29,
        width: pageRect.width - size * 0.28,
        height: size * 0.032
    ), xRadius: size * 0.012, yRadius: size * 0.012)
    sublineColor.withAlphaComponent(0.55).setFill()
    headingLine.fill()

    let bulletYPositions: [CGFloat] = [0.22, 0.165, 0.11]
    for multiplier in bulletYPositions {
        let y = pageRect.minY + size * multiplier
        let bullet = NSBezierPath(ovalIn: NSRect(
            x: pageRect.minX + size * 0.14,
            y: y,
            width: size * 0.024,
            height: size * 0.024
        ))
        NSColor(calibratedRed: 0.95, green: 0.49, blue: 0.24, alpha: 1).setFill()
        bullet.fill()

        let line = NSBezierPath(roundedRect: NSRect(
            x: pageRect.minX + size * 0.18,
            y: y + size * 0.002,
            width: pageRect.width - size * 0.28,
            height: size * 0.02
        ), xRadius: size * 0.01, yRadius: size * 0.01)
        sublineColor.withAlphaComponent(0.45).setFill()
        line.fill()
    }
}
