import Foundation

struct CSVToMarkdownConverter: DocumentConverter {
    var supportedExtensions: [String] { ["csv"] }

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        throw ConversionError.conversionFailed(underlying: NSError(domain: "Stub", code: 0))
    }
}
