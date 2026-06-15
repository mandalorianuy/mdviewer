import Foundation

protocol DocumentConverter: Sendable {
    var supportedExtensions: [String] { get }
    func canConvert(_ url: URL) -> Bool
    func convert(_ url: URL) throws -> MarkdownConversionResult
}

extension DocumentConverter {
    func canConvert(_ url: URL) -> Bool {
        supportedExtensions.contains(url.pathExtension.lowercased())
    }
}
