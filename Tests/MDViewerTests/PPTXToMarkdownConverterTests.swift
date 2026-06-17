import XCTest
@testable import MDViewer

final class PPTXToMarkdownConverterTests: XCTestCase {
    private let converter = PPTXToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString + ".pptx")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testConvertsSlidesInOrder() throws {
        try createPPTX(at: tempURL, entries: [
            (path: "ppt/slides/slide2.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                    <p:cSld>
                        <p:spTree>
                            <p:sp>
                                <p:txBody>
                                    <a:bodyPr/>
                                    <a:p><a:r><a:t>Texto de la diapositiva dos.</a:t></a:r></a:p>
                                </p:txBody>
                            </p:sp>
                        </p:spTree>
                    </p:cSld>
                </p:sld>
                """),
            (path: "ppt/slides/slide1.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                    <p:cSld>
                        <p:spTree>
                            <p:sp>
                                <p:txBody>
                                    <a:bodyPr/>
                                    <a:p><a:r><a:t>Primera diapositiva.</a:t></a:r></a:p>
                                </p:txBody>
                            </p:sp>
                        </p:spTree>
                    </p:cSld>
                </p:sld>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.sourceFormat, "PPTX")
        XCTAssertTrue(result.markdown.contains("## Diapositiva 1"))
        XCTAssertTrue(result.markdown.contains("Primera diapositiva."))
        XCTAssertTrue(result.markdown.contains("## Diapositiva 2"))
        XCTAssertTrue(result.markdown.contains("Texto de la diapositiva dos."))

        let slide1Range = result.markdown.range(of: "## Diapositiva 1")!
        let slide2Range = result.markdown.range(of: "## Diapositiva 2")!
        XCTAssertLessThan(slide1Range.lowerBound, slide2Range.lowerBound, "Slides should be ordered numerically")
    }

    func testExtractsSlideTitle() throws {
        try createPPTX(at: tempURL, entries: [
            (path: "ppt/slides/slide1.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                    <p:cSld>
                        <p:spTree>
                            <p:sp>
                                <p:nvSpPr>
                                    <p:cNvPr id="2" name="Title 1"/>
                                    <p:cNvSpPr/>
                                    <p:nvPr><p:ph type="title"/></p:nvPr>
                                </p:nvSpPr>
                                <p:txBody>
                                    <a:bodyPr/>
                                    <a:p><a:r><a:t>Título de diapositiva</a:t></a:r></a:p>
                                </p:txBody>
                            </p:sp>
                            <p:sp>
                                <p:txBody>
                                    <a:bodyPr/>
                                    <a:p><a:r><a:t>Texto del cuerpo.</a:t></a:r></a:p>
                                </p:txBody>
                            </p:sp>
                        </p:spTree>
                    </p:cSld>
                </p:sld>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("# Título de diapositiva"))
        XCTAssertTrue(result.markdown.contains("Texto del cuerpo."))
        XCTAssertFalse(result.markdown.contains("# Título de diapositiva\n\n# Título de diapositiva"))
    }

    func testConvertsSlideTable() throws {
        try createPPTX(at: tempURL, entries: [
            (path: "ppt/slides/slide1.xml", content: """
                <?xml version="1.0" encoding="UTF-8" standalone="yes"?>
                <p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
                    <p:cSld>
                        <p:spTree>
                            <p:graphicFrame>
                                <a:graphic>
                                    <a:graphicData>
                                        <a:tbl>
                                            <a:tr>
                                                <a:tc><a:txBody><a:p><a:r><a:t>Nombre</a:t></a:r></a:p></a:txBody></a:tc>
                                                <a:tc><a:txBody><a:p><a:r><a:t>Edad</a:t></a:r></a:p></a:txBody></a:tc>
                                            </a:tr>
                                            <a:tr>
                                                <a:tc><a:txBody><a:p><a:r><a:t>Ana</a:t></a:r></a:p></a:txBody></a:tc>
                                                <a:tc><a:txBody><a:p><a:r><a:t>30</a:t></a:r></a:p></a:txBody></a:tc>
                                            </a:tr>
                                        </a:tbl>
                                    </a:graphicData>
                                </a:graphic>
                            </p:graphicFrame>
                        </p:spTree>
                    </p:cSld>
                </p:sld>
                """)
        ])

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| Nombre | Edad |"))
        XCTAssertTrue(result.markdown.contains("| --- | --- |"))
        XCTAssertTrue(result.markdown.contains("| Ana | 30 |"))
    }

    func testMissingSlidesThrowsConversionFailed() throws {
        try createPPTX(at: tempURL, entries: [
            (path: "ppt/presentation.xml", content: "<p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"/>")
        ])

        XCTAssertThrowsError(try converter.convert(tempURL)) { error in
            guard case ConversionError.conversionFailed = error else {
                XCTFail("Expected conversionFailed, got \(error)")
                return
            }
        }
    }

    // MARK: - Helpers

    private func createPPTX(at pptxURL: URL, entries: [(path: String, content: String)]) throws {
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
        process.arguments = [pptxURL.path] + entries.map(\.path)

        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "zip command failed")
    }
}
