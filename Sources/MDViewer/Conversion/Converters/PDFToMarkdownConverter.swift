import Foundation
import PDFKit

struct PDFToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["pdf"]

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            throw ConversionError.fileNotReadable
        }

        guard let document = PDFDocument(url: url) else {
            throw ConversionError.conversionFailed(reason: "No se pudo cargar el PDF")
        }

        let pageCount = document.pageCount
        var pages: [String] = []
        var hasAnyText = false

        for index in 0..<pageCount {
            guard let page = document.page(at: index) else { continue }
            let text = page.string ?? ""
            if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                hasAnyText = true
            }
            pages.append("## Página \(index + 1)\n\(text)")
        }

        guard hasAnyText else {
            return MarkdownConversionResult(
                markdown: "_El PDF no contiene texto extraíble._",
                sourceFormat: "PDF",
                title: nil,
                warnings: ["El PDF no contiene texto extraíble."],
                metadata: ["pageCount": String(pageCount)]
            )
        }

        let title = document.documentAttributes?[PDFDocumentAttribute.titleAttribute] as? String
        let trimmedTitle = title?.trimmingCharacters(in: .whitespacesAndNewlines)

        return MarkdownConversionResult(
            markdown: pages.joined(separator: "\n\n"),
            sourceFormat: "PDF",
            title: trimmedTitle?.isEmpty == false ? trimmedTitle : nil,
            warnings: [],
            metadata: ["pageCount": String(pageCount)]
        )
    }
}
