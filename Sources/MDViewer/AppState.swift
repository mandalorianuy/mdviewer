import Foundation
import SwiftUI
import UniformTypeIdentifiers

extension UTType {
    static let mdviewerMarkdown = UTType(importedAs: "net.daringfireball.markdown")
}

struct MarkdownFileDocument: FileDocument {
    static let readableContentTypes: [UTType] = [.mdviewerMarkdown]
    static let writableContentTypes: [UTType] = [.mdviewerMarkdown]

    var rawMarkdown: String

    init(rawMarkdown: String = "") {
        self.rawMarkdown = rawMarkdown
    }

    init(configuration: ReadConfiguration) throws {
        guard let data = configuration.file.regularFileContents else {
            rawMarkdown = ""
            return
        }

        guard let markdown = String(data: data, encoding: .utf8) else {
            throw CocoaError(.fileReadCorruptFile)
        }

        rawMarkdown = markdown
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        let data = rawMarkdown.data(using: .utf8) ?? Data()
        return .init(regularFileWithContents: data)
    }
}
