import Foundation

struct MarkdownConversionResult: Sendable {
    let markdown: String
    let sourceFormat: String
    let title: String?
    let warnings: [String]
}
