import AppKit
import Foundation

enum PDFExporter {
    @MainActor
    static func export(markdown: String, fontFamily: String, fontSize: CGFloat, outputURL: URL) throws {
        let attributed: NSAttributedString
        do {
            let swiftAttributed = try AttributedString(
                markdown: markdown,
                options: AttributedString.MarkdownParsingOptions(
                    interpretedSyntax: .full,
                    failurePolicy: .returnPartiallyParsedIfPossible
                )
            )
            attributed = NSAttributedString(swiftAttributed)
        } catch {
            let fallbackFont = NSFont(name: fontFamily, size: fontSize) ?? NSFont.systemFont(ofSize: fontSize)
            attributed = NSAttributedString(string: markdown, attributes: [.font: fallbackFont])
        }

        let pageWidth: CGFloat = 595.2
        let pageHeight: CGFloat = 841.8
        let margin: CGFloat = 48
        let contentWidth = pageWidth - (margin * 2)

        let textStorage = NSTextStorage(attributedString: attributed)
        let layoutManager = NSLayoutManager()
        textStorage.addLayoutManager(layoutManager)

        let textContainer = NSTextContainer(size: NSSize(width: contentWidth, height: .greatestFiniteMagnitude))
        layoutManager.addTextContainer(textContainer)
        layoutManager.glyphRange(for: textContainer)

        let usedRect = layoutManager.usedRect(for: textContainer)
        let height = max(pageHeight, usedRect.height + margin * 2)

        let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: pageWidth, height: height))
        textView.textContainerInset = NSSize(width: margin, height: margin)
        textView.isEditable = false
        textView.isRichText = true
        textView.textStorage?.setAttributedString(attributed)

        let printInfo = NSPrintInfo()
        printInfo.paperSize = NSSize(width: pageWidth, height: pageHeight)
        printInfo.leftMargin = 0
        printInfo.rightMargin = 0
        printInfo.topMargin = 0
        printInfo.bottomMargin = 0
        printInfo.horizontalPagination = .automatic
        printInfo.verticalPagination = .automatic
        printInfo.isHorizontallyCentered = false
        printInfo.isVerticallyCentered = false

        let pdfData = NSMutableData()
        let operation = NSPrintOperation.pdfOperation(
            with: textView,
            inside: textView.bounds,
            to: pdfData,
            printInfo: printInfo
        )
        operation.showsPrintPanel = false
        operation.showsProgressPanel = false

        guard operation.run() else {
            throw NSError(domain: "MDViewer", code: 2, userInfo: [NSLocalizedDescriptionKey: "La exportación PDF falló."])
        }

        try pdfData.write(to: outputURL)
    }
}
