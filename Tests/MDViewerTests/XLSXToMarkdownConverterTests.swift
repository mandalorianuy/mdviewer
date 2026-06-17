import XCTest
@testable import MDViewer

final class XLSXToMarkdownConverterTests: XCTestCase {
    private let converter = XLSXToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString + ".xlsx")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testConvertsSheetWithSharedStringsAndInlineValues() throws {
        try createXLSX(at: tempURL, entries: [
            (path: "xl/sharedStrings.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
                    <si><t>Nombre</t></si>
                    <si><t>Ana</t></si>
                </sst>
                """),
            (path: "xl/worksheets/sheet1.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                    <sheetData>
                        <row r="1">
                            <c r="A1" t="s"><v>0</v></c>
                            <c r="B1"><v>Edad</v></c>
                        </row>
                        <row r="2">
                            <c r="A2" t="s"><v>1</v></c>
                            <c r="B2"><v>30</v></c>
                        </row>
                    </sheetData>
                </worksheet>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.sourceFormat, "XLSX")
        XCTAssertTrue(result.markdown.contains("| Nombre | Edad |"))
        XCTAssertTrue(result.markdown.contains("| --- | --- |"))
        XCTAssertTrue(result.markdown.contains("| Ana | 30 |"))
    }

    func testUsesSheetNameFromWorkbook() throws {
        try createXLSX(at: tempURL, entries: [
            (path: "xl/_rels/workbook.xml.rels", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
                    <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
                </Relationships>
                """),
            (path: "xl/workbook.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
                    <sheets>
                        <sheet name="Ventas" sheetId="1" r:id="rId1"/>
                    </sheets>
                </workbook>
                """),
            (path: "xl/worksheets/sheet1.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                    <sheetData>
                        <row r="1">
                            <c r="A1"><v>Producto</v></c>
                            <c r="B1"><v>Precio</v></c>
                        </row>
                    </sheetData>
                </worksheet>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("## Ventas"))
        XCTAssertTrue(result.markdown.contains("| Producto | Precio |"))
    }

    func testHandlesMergedCells() throws {
        try createXLSX(at: tempURL, entries: [
            (path: "xl/worksheets/sheet1.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
                    <sheetData>
                        <row r="1">
                            <c r="A1"><v>Total</v></c>
                            <c r="B1"><v>100</v></c>
                        </row>
                        <row r="2">
                            <c r="A2"><v>Detalle</v></c>
                        </row>
                    </sheetData>
                    <mergeCells>
                        <mergeCell ref="A1:B1"/>
                    </mergeCells>
                </worksheet>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| Total |  |"))
        XCTAssertTrue(result.markdown.contains("| Detalle |"))
    }

    func testMissingWorksheetsThrowsConversionFailed() throws {
        try createXLSX(at: tempURL, entries: [
            (path: "xl/sharedStrings.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="0" uniqueCount="0"/>
                """)
        ])

        XCTAssertThrowsError(try converter.convert(tempURL)) { error in
            guard case ConversionError.conversionFailed = error else {
                XCTFail("Expected conversionFailed, got \(error)")
                return
            }
        }
    }

    // MARK: - Helpers

    private func createXLSX(at xlsxURL: URL, entries: [(path: String, content: String)]) throws {
        let staging = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: staging) }

        for entry in entries {
            var fileURL = staging
            for component in entry.path.split(separator: "/") {
                fileURL = fileURL.appendingPathComponent(String(component))
            }
            try FileManager.default.createDirectory(
                at: fileURL.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try entry.content.write(to: fileURL, atomically: true, encoding: .utf8)
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
        process.currentDirectoryURL = staging
        process.arguments = [xlsxURL.path] + entries.map(\.path)

        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "zip command failed")
    }
}
