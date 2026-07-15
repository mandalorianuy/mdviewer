import XCTest
@testable import MDViewer

final class HTMLExporterTests: XCTestCase {
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension("html")
    }

    override func tearDown() {
        if let tempURL = tempURL {
            try? FileManager.default.removeItem(at: tempURL)
        }
        super.tearDown()
    }

    func testExportWritesFile() throws {
        let html = "<h1>Hello</h1>"
        try HTMLExporter.export(html: html, outputURL: tempURL)

        XCTAssertTrue(FileManager.default.fileExists(atPath: tempURL.path))
    }

    func testExportContainsCompleteDocumentStructure() throws {
        let html = "<h1>Hello</h1>"
        try HTMLExporter.export(html: html, outputURL: tempURL)

        let content = try String(contentsOf: tempURL, encoding: .utf8)
        XCTAssertTrue(content.contains("<!DOCTYPE html>"), "Missing DOCTYPE")
        XCTAssertTrue(content.contains("<html>"), "Missing <html>")
        XCTAssertTrue(content.contains("</html>"), "Missing </html>")
        XCTAssertTrue(content.contains("<head>"), "Missing <head>")
        XCTAssertTrue(content.contains("</head>"), "Missing </head>")
        XCTAssertTrue(content.contains("<body>"), "Missing <body>")
        XCTAssertTrue(content.contains("</body>"), "Missing </body>")
    }

    func testExportWrapsInputHTML() throws {
        let html = "<h1>Hello</h1><p>World</p>"
        try HTMLExporter.export(html: html, outputURL: tempURL)

        let content = try String(contentsOf: tempURL, encoding: .utf8)
        XCTAssertTrue(content.contains(html), "Exported file should contain the input HTML")
    }

    func testExportIncludesCSSStyles() throws {
        let html = "<p>Styled</p>"
        try HTMLExporter.export(html: html, outputURL: tempURL)

        let content = try String(contentsOf: tempURL, encoding: .utf8)
        XCTAssertTrue(content.contains("<style>"), "Missing <style> tag")
        XCTAssertTrue(content.contains("</style>"), "Missing </style> tag")
        XCTAssertTrue(content.contains("body {"), "Missing body CSS rule")
        XCTAssertTrue(content.contains("img {"), "Missing img CSS rule")
        XCTAssertTrue(content.contains("pre {"), "Missing pre CSS rule")
    }
}
