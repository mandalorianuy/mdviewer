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

    func testInvalidJSONThrows() {
        writeJSON("{ no es json }")
        XCTAssertThrowsError(try converter.convert(tempURL))
    }
}
