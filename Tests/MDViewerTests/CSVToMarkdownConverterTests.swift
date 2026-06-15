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

    func testEmptyCSV() {
        writeCSV("")
        let result = try! converter.convert(tempURL)
        XCTAssertEqual(result.markdown, "")
        XCTAssertEqual(result.warnings, ["El archivo CSV está vacío."])
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

    func testCSVWithCRLF() {
        writeCSV("Nombre,Edad\r\nJuan,30\r\nMaria,25")
        let result = try! converter.convert(tempURL)
        let expected = """
        | Nombre | Edad |
        | --- | --- |
        | Juan | 30 |
        | Maria | 25 |
        """
        XCTAssertEqual(result.markdown, expected)
    }

    func testCSVWithQuotedCommas() {
        writeCSV("Producto,Descripcion\n\"A, B\",C\nD,\"E, F\"")
        let result = try! converter.convert(tempURL)
        let expected = """
        | Producto | Descripcion |
        | --- | --- |
        | A, B | C |
        | D | E, F |
        """
        XCTAssertEqual(result.markdown, expected)
    }

    func testCSVWithQuotes() {
        writeCSV("Producto,Descripcion\n\"A\",\"B, C\"\n\"D\"\"E\",\"F\"")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B, C |"))
        XCTAssertTrue(result.markdown.contains("| D\"E | F |"))
    }

    func testFieldsWithPipesAreEscaped() {
        writeCSV("A,B\na|b,c|d")
        let result = try! converter.convert(tempURL)
        let expected = """
        | A | B |
        | --- | --- |
        | a\\|b | c\\|d |
        """
        XCTAssertEqual(result.markdown, expected)
    }

    func testQuotedNewlinesPreserved() {
        writeCSV("A,B\n\"multi\nline\",value")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| multi<br>line | value |"))
    }
}
