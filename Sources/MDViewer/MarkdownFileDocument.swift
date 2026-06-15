import Foundation
import SwiftUI
import UniformTypeIdentifiers

extension UTType {
    static let mdviewerMarkdown = UTType(importedAs: "net.daringfireball.markdown")
}

/// Document model for MDViewer.
/// - Warning: Marked `@unchecked Sendable` because `ReferenceFileDocument` requires a class. All mutations are dispatched to `@MainActor` by `ContentView`.
final class MarkdownFileDocument: ReferenceFileDocument, @unchecked Sendable {
    static let readableContentTypes: [UTType] = [
        .mdviewerMarkdown,
        .commaSeparatedText,
        .json,
        .xml,
        .html,
        .zip
    ]
    static let writableContentTypes: [UTType] = [.mdviewerMarkdown]

    var rawMarkdown: String
    var conversionResult: MarkdownConversionResult?
    var pendingConversionURL: URL?

    init(rawMarkdown: String = "") {
        self.rawMarkdown = rawMarkdown
        self.conversionResult = nil
        self.pendingConversionURL = nil
    }

    convenience init() {
        self.init(rawMarkdown: "")
    }

    init(conversionResult: MarkdownConversionResult) {
        self.rawMarkdown = conversionResult.markdown
        self.conversionResult = conversionResult
        self.pendingConversionURL = nil
    }

    init(configuration: ReadConfiguration) throws {
        guard let fileWrapper = configuration.file.regularFileContents else {
            rawMarkdown = ""
            conversionResult = nil
            pendingConversionURL = nil
            return
        }

        let filename = configuration.file.filename ?? ""
        let ext = (filename as NSString).pathExtension.lowercased()

        if ext == "md" || ext == "markdown" || ext == "mdown" || ext == "mkdn" || ext == "mkd" {
            guard let markdown = String(data: fileWrapper, encoding: .utf8) else {
                throw CocoaError(.fileReadCorruptFile)
            }
            rawMarkdown = markdown
            conversionResult = nil
            pendingConversionURL = nil
            return
        }

        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(ext)

        // Known limitation: the temporary file is left for `ContentView` to convert and is not cleaned up here.
        // Complex cleanup will be addressed later if needed.
        try fileWrapper.write(to: tempURL)

        rawMarkdown = ""
        conversionResult = nil
        pendingConversionURL = tempURL
    }

    func snapshot(contentType: UTType) throws -> String {
        rawMarkdown
    }

    func fileWrapper(snapshot: String, configuration: WriteConfiguration) throws -> FileWrapper {
        let data = snapshot.data(using: .utf8) ?? Data()
        return .init(regularFileWithContents: data)
    }
}
