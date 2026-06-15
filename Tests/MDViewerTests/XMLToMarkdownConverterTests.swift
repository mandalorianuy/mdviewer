import XCTest
@testable import MDViewer

final class XMLToMarkdownConverterTests: XCTestCase {
    private let converter = XMLToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".xml")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeXML(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testSimpleXML() {
        writeXML("<?xml version=\"1.0\"?><root><user><name>Juan</name></user></root>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("**root**"))
        XCTAssertTrue(result.markdown.contains("**user**"))
        XCTAssertTrue(result.markdown.contains("**name**: Juan"))
    }

    func testNestedXMLExactOutput() {
        writeXML("<root><child>text</child></root>")
        let result = try! converter.convert(tempURL)
        XCTAssertEqual(result.markdown, "- **root**\n  - **child**: text")
    }

    func testAttributes() {
        writeXML("<root><user id=\"42\" active=\"true\">Juan</user></root>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("**id**: `42`"))
        XCTAssertTrue(result.markdown.contains("**active**: `true`"))
        XCTAssertTrue(result.markdown.contains("**user**"))
    }

    func testCDATA() {
        writeXML("<root><![CDATA[<unescaped> & more]]></root>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("<unescaped> & more"))
    }

    func testInvalidXMLThrows() {
        writeXML("<root><unclosed>")
        XCTAssertThrowsError(try converter.convert(tempURL))
    }
}
