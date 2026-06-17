import XCTest
import SwiftUI
import UniformTypeIdentifiers
@testable import MDViewer

extension FileDocumentWriteConfiguration {
    static func forTesting(contentType: UTType, existingFile: FileWrapper? = nil) -> FileDocumentWriteConfiguration {
        struct Standin {
            let contentType: UTType
            let existingFile: FileWrapper?
        }
        return unsafeBitCast(
            Standin(contentType: contentType, existingFile: existingFile),
            to: FileDocumentWriteConfiguration.self
        )
    }
}

final class MarkdownFileDocumentMetadataTests: XCTestCase {
    func testConversionResultIncludesFrontmatter() throws {
        let result = MarkdownConversionResult(
            markdown: "# Hola",
            sourceFormat: "PDF",
            title: "Doc.pdf",
            warnings: [],
            metadata: ["pageCount": "3"]
        )
        let doc = MarkdownFileDocument(conversionResult: result)
        let wrapper = try doc.fileWrapper(
            snapshot: doc.rawMarkdown,
            configuration: .forTesting(contentType: .mdviewerMarkdown)
        )

        let data = try XCTUnwrap(wrapper.regularFileContents)
        let saved = try XCTUnwrap(String(data: data, encoding: .utf8))

        XCTAssertTrue(saved.hasPrefix("---"), "Frontmatter should start with ---")
        XCTAssertTrue(saved.contains("source_format: PDF"), "Missing source_format")
        XCTAssertTrue(saved.contains("title: \"Doc.pdf\""), "Missing title")
        XCTAssertTrue(saved.contains("pageCount: \"3\""), "Missing metadata entry")
    }

    func testConversionResultIncludesConversionDate() throws {
        let result = MarkdownConversionResult(
            markdown: "# Test",
            sourceFormat: "HTML",
            title: nil,
            warnings: [],
            metadata: [:]
        )
        let doc = MarkdownFileDocument(conversionResult: result)
        let wrapper = try doc.fileWrapper(
            snapshot: doc.rawMarkdown,
            configuration: .forTesting(contentType: .mdviewerMarkdown)
        )

        let data = try XCTUnwrap(wrapper.regularFileContents)
        let saved = try XCTUnwrap(String(data: data, encoding: .utf8))

        XCTAssertTrue(saved.contains("conversion_date:"), "Missing conversion_date")
    }

    func testPlainMarkdownDocumentDoesNotIncludeFrontmatter() throws {
        let doc = MarkdownFileDocument(rawMarkdown: "# Plain")
        let wrapper = try doc.fileWrapper(
            snapshot: doc.rawMarkdown,
            configuration: .forTesting(contentType: .mdviewerMarkdown)
        )

        let data = try XCTUnwrap(wrapper.regularFileContents)
        let saved = try XCTUnwrap(String(data: data, encoding: .utf8))

        XCTAssertFalse(saved.hasPrefix("---"), "Plain Markdown should not include frontmatter")
        XCTAssertFalse(saved.contains("source_format:"), "Plain Markdown should not include source_format")
    }
}
