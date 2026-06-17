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
