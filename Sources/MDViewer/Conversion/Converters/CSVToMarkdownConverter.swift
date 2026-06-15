import Foundation

// TODO: Implement real CSV conversion in Task B.
struct CSVToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["csv"]

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        throw ConversionError.unsupportedFormat
    }
}
