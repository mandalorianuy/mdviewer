import AppKit
import Foundation

enum MarkdownRenderer {
    static func render(markdown: String, fontFamily: String, baseFontSize: CGFloat) throws -> NSAttributedString {
        let parsed: NSMutableAttributedString
        do {
            let swiftAttributed = try AttributedString(
                markdown: markdown,
                options: AttributedString.MarkdownParsingOptions(
                    interpretedSyntax: .full,
                    failurePolicy: .returnPartiallyParsedIfPossible
                )
            )
            parsed = NSMutableAttributedString(attributedString: NSAttributedString(swiftAttributed))
        } catch {
            let fallbackFont = NSFont(name: fontFamily, size: baseFontSize) ?? NSFont.systemFont(ofSize: baseFontSize)
            return NSAttributedString(string: markdown, attributes: [.font: fallbackFont])
        }

        let fullRange = NSRange(location: 0, length: parsed.length)
        let bodyBaseline: CGFloat = 14
        let scale = max(0.6, min(baseFontSize / bodyBaseline, 3.0))

        parsed.enumerateAttribute(.font, in: fullRange) { value, range, _ in
            let existing = (value as? NSFont) ?? NSFont.systemFont(ofSize: bodyBaseline)
            let newSize = max(10, min(existing.pointSize * scale, 72))
            let isMonospaced = existing.fontDescriptor.symbolicTraits.contains(.monoSpace) ||
                existing.fontName.localizedCaseInsensitiveContains("mono") ||
                existing.fontName.localizedCaseInsensitiveContains("menlo") ||
                existing.fontName.localizedCaseInsensitiveContains("courier")

            let finalFont: NSFont
            if isMonospaced {
                finalFont = NSFont.monospacedSystemFont(ofSize: newSize, weight: .regular)
                parsed.addAttribute(.backgroundColor, value: NSColor.textBackgroundColor.blended(withFraction: 0.35, of: .controlAccentColor) ?? NSColor.controlAccentColor.withAlphaComponent(0.15), range: range)
            } else {
                let traits = NSFontManager.shared.traits(of: existing)
                let weight = NSFontManager.shared.weight(of: existing)
                let replaced = NSFontManager.shared.font(
                    withFamily: fontFamily,
                    traits: traits,
                    weight: weight,
                    size: newSize
                )
                finalFont = replaced ?? NSFont(name: fontFamily, size: newSize) ?? NSFont.systemFont(ofSize: newSize)
            }

            parsed.addAttribute(.font, value: finalFont, range: range)
        }

        let paragraphStyle = NSMutableParagraphStyle()
        paragraphStyle.lineSpacing = 3
        paragraphStyle.paragraphSpacing = 8
        parsed.addAttribute(.paragraphStyle, value: paragraphStyle, range: fullRange)

        return parsed
    }
}
