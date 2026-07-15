import Foundation

struct MarkdownConversionResult: Sendable {
    let markdown: String
    let sourceFormat: String
    let title: String?
    let warnings: [String]
    let metadata: [String: String]

    init(
        markdown: String,
        sourceFormat: String,
        title: String? = nil,
        warnings: [String] = [],
        metadata: [String: String] = [:]
    ) {
        self.markdown = markdown
        self.sourceFormat = sourceFormat
        self.title = title
        self.warnings = warnings
        self.metadata = metadata
    }
}
