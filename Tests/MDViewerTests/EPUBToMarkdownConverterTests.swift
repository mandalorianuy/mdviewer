import XCTest
@testable import MDViewer

final class EPUBToMarkdownConverterTests: XCTestCase {
    private let converter = EPUBToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString + ".epub")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testConvertsEPUBTitleAndChapters() throws {
        try createEPUB(at: tempURL, entries: [
            (path: "mimetype", content: "application/epub+zip"),
            (path: "META-INF/container.xml", content: """
                <?xml version="1.0" encoding="UTF-8"?>
                <container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
                    <rootfiles>
                        <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
                    </rootfiles>
                </container>
                """),
            (path: "OEBPS/content.opf", content: """
                <?xml version="1.0" encoding="UTF-8"?>
                <package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="id">
                    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
                        <dc:title>Mi Libro</dc:title>
                    </metadata>
                    <manifest>
                        <item id="toc" href="toc.xhtml" media-type="application/xhtml+xml"/>
                        <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
                        <item id="ch2" href="chapter2.xhtml" media-type="application/xhtml+xml"/>
                    </manifest>
                    <spine>
                        <itemref idref="ch1"/>
                        <itemref idref="ch2"/>
                    </spine>
                </package>
                """),
            (path: "OEBPS/chapter1.xhtml", content: "<h1>Capítulo 1</h1><p>Primer párrafo.</p>"),
            (path: "OEBPS/chapter2.xhtml", content: "<h1>Capítulo 2</h1><p>Segundo párrafo.</p>")
        ])

        let result = try converter.convert(tempURL)

        XCTAssertEqual(result.title, "Mi Libro")
        XCTAssertEqual(result.sourceFormat, "EPUB")
        XCTAssertTrue(result.markdown.contains("# Capítulo 1"), "Missing chapter 1 heading")
        XCTAssertTrue(result.markdown.contains("Primer párrafo."), "Missing chapter 1 text")
        XCTAssertTrue(result.markdown.contains("# Capítulo 2"), "Missing chapter 2 heading")
        XCTAssertTrue(result.markdown.contains("Segundo párrafo."), "Missing chapter 2 text")
        XCTAssertTrue(result.markdown.contains("\n\n---\n\n"), "Missing separator between chapters")
    }

    func testEPUBWithoutContainerThrowsConversionFailed() throws {
        try createEPUB(at: tempURL, entries: [
            (path: "mimetype", content: "application/epub+zip"),
            (path: "OEBPS/content.opf", content: """
                <?xml version="1.0" encoding="UTF-8"?>
                <package version="3.0" xmlns="http://www.idpf.org/2007/opf">
                    <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
                        <dc:title>Sin container</dc:title>
                    </metadata>
                    <manifest>
                        <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
                    </manifest>
                    <spine>
                        <itemref idref="ch1"/>
                    </spine>
                </package>
                """),
            (path: "OEBPS/chapter1.xhtml", content: "<p>Hola</p>")
        ])

        XCTAssertThrowsError(try converter.convert(tempURL)) { error in
            guard case ConversionError.conversionFailed = error else {
                XCTFail("Expected conversionFailed, got \(error)")
                return
            }
        }
    }

    // MARK: - Helpers

    private func createEPUB(at epubURL: URL, entries: [(path: String, content: String)]) throws {
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
        process.arguments = [epubURL.path] + entries.map(\.path)

        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "zip command failed")
    }
}
