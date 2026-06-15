import XCTest
@testable import MDViewer

final class HTMLToMarkdownConverterTests: XCTestCase {
    private let converter = HTMLToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".html")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeHTML(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testHeadingsAndParagraphs() {
        writeHTML("<h1>Titulo</h1><p>Este es un <strong>texto</strong>.</p>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("# Titulo"))
        XCTAssertTrue(result.markdown.contains("Este es un **texto**."))
    }

    func testLink() {
        writeHTML("<a href=\"https://example.com\">Ejemplo</a>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("[Ejemplo](https://example.com)"))
    }
}
