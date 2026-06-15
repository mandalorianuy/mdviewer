import XCTest
@testable import MDViewer

final class JSONToMarkdownConverterTests: XCTestCase {
    private let converter = JSONToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeJSON(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testObject() {
        writeJSON("{\"nombre\": \"Juan\", \"edad\": 30, \"activo\": true}")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("- **nombre**: Juan"))
        XCTAssertTrue(result.markdown.contains("- **edad**: `30`"))
        XCTAssertTrue(result.markdown.contains("- **activo**: `true`"))
    }

    func testTopLevelArrayWithObjects() {
        writeJSON("[{\"nombre\": \"Juan\"}, {\"nombre\": \"Maria\"}]")
        let result = try! converter.convert(tempURL)
        let expected = """
        -
          - **nombre**: Juan
        -
          - **nombre**: Maria
        """
        XCTAssertEqual(result.markdown, expected)
    }

    func testNestedObjects() {
        writeJSON("{\"usuario\": {\"nombre\": \"Juan\", \"direccion\": {\"ciudad\": \"BUE\"}}}")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("- **usuario**:"))
        XCTAssertTrue(result.markdown.contains("- **nombre**: Juan"))
        XCTAssertTrue(result.markdown.contains("- **direccion**:"))
        XCTAssertTrue(result.markdown.contains("- **ciudad**: BUE"))
    }

    func testScalars() {
        writeJSON("{\"nulo\": null, \"falso\": false, \"numero\": 42}")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("- **nulo**: `null`"))
        XCTAssertTrue(result.markdown.contains("- **falso**: `false`"))
        XCTAssertTrue(result.markdown.contains("- **numero**: `42`"))
    }

    func testStringEscaping() {
        writeJSON("{\"texto\": \"hola *mundo* _test_ `code`\"}")
        let result = try! converter.convert(tempURL)
        let expected = "- **texto**: hola \\*mundo\\* \\_test\\_ \\`code\\`"
        XCTAssertEqual(result.markdown, expected)
    }

    func testInvalidJSONThrows() {
        writeJSON("{ no es json }")
        XCTAssertThrowsError(try converter.convert(tempURL))
    }
}
