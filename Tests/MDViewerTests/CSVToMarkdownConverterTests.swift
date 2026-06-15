import XCTest
@testable import MDViewer

final class CSVToMarkdownConverterTests: XCTestCase {
    private let converter = CSVToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".csv")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeCSV(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testSimpleCSV() {
        writeCSV("Nombre,Edad\nJuan,30\nMaria,25")
        let result = try! converter.convert(tempURL)
        let expected = """
        | Nombre | Edad |
        | --- | --- |
        | Juan | 30 |
        | Maria | 25 |
        """
        XCTAssertEqual(result.markdown, expected)
    }

    func testCSVWithQuotes() {
        writeCSV("Producto,Descripcion\n\"A\",\"B, C\"\n\"D\"\"E\",\"F\"")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B, C |"))
        XCTAssertTrue(result.markdown.contains("| D\"E | F |"))
    }
}
