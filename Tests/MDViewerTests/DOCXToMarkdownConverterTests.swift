import XCTest
@testable import MDViewer

final class DOCXToMarkdownConverterTests: XCTestCase {
    private let converter = DOCXToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString + ".docx")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testConvertsParagraphsAndHeadings() throws {
        try createDOCX(at: tempURL, entries: [
            (path: "word/document.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                    <w:body>
                        <w:p>
                            <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
                            <w:r><w:t>Título principal</w:t></w:r>
                        </w:p>
                        <w:p>
                            <w:r><w:t>Primer párrafo.</w:t></w:r>
                        </w:p>
                        <w:p>
                            <w:pPr><w:pStyle w:val="Heading2"/></w:pPr>
                            <w:r><w:t>Subtítulo</w:t></w:r>
                        </w:p>
                        <w:p>
                            <w:r><w:t>Segundo párrafo</w:t></w:r>
                            <w:r><w:t> con más texto.</w:t></w:r>
                        </w:p>
                    </w:body>
                </w:document>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.sourceFormat, "DOCX")
        XCTAssertTrue(result.markdown.contains("# Título principal"))
        XCTAssertTrue(result.markdown.contains("## Subtítulo"))
        XCTAssertTrue(result.markdown.contains("Primer párrafo."))
        XCTAssertTrue(result.markdown.contains("Segundo párrafo con más texto."))
    }

    func testConvertsTable() throws {
        try createDOCX(at: tempURL, entries: [
            (path: "word/document.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                    <w:body>
                        <w:tbl>
                            <w:tr>
                                <w:tc><w:p><w:r><w:t>Nombre</w:t></w:r></w:p></w:tc>
                                <w:tc><w:p><w:r><w:t>Edad</w:t></w:r></w:p></w:tc>
                            </w:tr>
                            <w:tr>
                                <w:tc><w:p><w:r><w:t>Ana</w:t></w:r></w:p></w:tc>
                                <w:tc><w:p><w:r><w:t>30</w:t></w:r></w:p></w:tc>
                            </w:tr>
                        </w:tbl>
                    </w:body>
                </w:document>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| Nombre | Edad |"))
        XCTAssertTrue(result.markdown.contains("| --- | --- |"))
        XCTAssertTrue(result.markdown.contains("| Ana | 30 |"))
    }

    func testMissingDocumentXMLThrowsConversionFailed() throws {
        try createDOCX(at: tempURL, entries: [
            (path: "word/styles.xml", content: "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>")
        ])

        XCTAssertThrowsError(try converter.convert(tempURL)) { error in
            guard case ConversionError.conversionFailed = error else {
                XCTFail("Expected conversionFailed, got \(error)")
                return
            }
        }
    }

    // MARK: - Helpers

    private func createDOCX(at docxURL: URL, entries: [(path: String, content: String)]) throws {
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
        process.arguments = [docxURL.path] + entries.map(\.path)

        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "zip command failed")
    }
}
