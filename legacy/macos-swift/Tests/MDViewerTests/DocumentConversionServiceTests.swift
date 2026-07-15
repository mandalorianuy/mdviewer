import XCTest
@testable import MDViewer

final class DocumentConversionServiceTests: XCTestCase {
    private let service = DocumentConversionService()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".csv")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testConvertsCSV() async throws {
        try "A,B\n1,2".write(to: tempURL, atomically: true, encoding: .utf8)
        let result = try await service.convert(url: tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B |"))
    }

    func testConvertSyncConvertsCSV() throws {
        try "A,B\n1,2".write(to: tempURL, atomically: true, encoding: .utf8)
        let result = try service.convertSync(url: tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B |"))
    }

    func testUnsupportedFormatThrows() async {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".xyz")
        FileManager.default.createFile(atPath: url.path, contents: Data(), attributes: nil)
        defer { try? FileManager.default.removeItem(at: url) }

        do {
            _ = try await service.convert(url: url)
            XCTFail("Expected conversion to throw for unsupported format")
        } catch {
            XCTAssertTrue(error is ConversionError)
        }
    }
}

final class FixtureConversionTests: XCTestCase {
    private let service = DocumentConversionService()

    private func fixtureURL(named name: String, withExtension ext: String) -> URL {
        guard let url = Bundle.module.url(forResource: name, withExtension: ext) else {
            XCTFail("Missing fixture: \(name).\(ext)")
            return URL(fileURLWithPath: "/dev/null")
        }
        return url
    }

    func testCSVFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample", withExtension: "csv"))
        XCTAssertTrue(result.markdown.contains("| Name | Age |"))
        XCTAssertTrue(result.markdown.contains("| Alice | 30 |"))
    }

    func testJSONFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample", withExtension: "json"))
        XCTAssertTrue(result.markdown.contains("- **name**: Alice"))
    }

    func testXMLFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample", withExtension: "xml"))
        XCTAssertTrue(result.markdown.contains("**name**: Alice"))
    }

    func testHTMLFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample", withExtension: "html"))
        XCTAssertTrue(result.markdown.contains("# Hello"))
        XCTAssertTrue(result.markdown.contains("World"))
    }
}

import AppKit
import CoreGraphics
import PDFKit
import UniformTypeIdentifiers

final class NewConverterIntegrationTests: XCTestCase {
    private let service = DocumentConversionService()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    }

    override func tearDown() {
        if let tempURL = tempURL {
            try? FileManager.default.removeItem(at: tempURL)
        }
        super.tearDown()
    }

    // MARK: - PDF

    func testConvertsPDF() async throws {
        tempURL = tempURL.appendingPathExtension("pdf")
        writePDF(pages: ["Hola PDF"])

        let result = try await service.convert(url: tempURL)
        XCTAssertEqual(result.sourceFormat, "PDF")
        XCTAssertTrue(result.markdown.contains("Hola PDF"))
    }

    // MARK: - Image

    func testConvertsImage() async throws {
        tempURL = tempURL.appendingPathExtension("png")
        writePNG(size: CGSize(width: 100, height: 100))

        let result = try await service.convert(url: tempURL)
        XCTAssertEqual(result.sourceFormat, "Imagen")
        XCTAssertFalse(result.markdown.isEmpty)
    }

    // MARK: - EPUB

    func testConvertsEPUB() async throws {
        tempURL = tempURL.appendingPathExtension("epub")
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
                        <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
                    </manifest>
                    <spine>
                        <itemref idref="ch1"/>
                    </spine>
                </package>
                """),
            (path: "OEBPS/chapter1.xhtml", content: "<h1>Capítulo 1</h1><p>Primer párrafo.</p>")
        ])

        let result = try await service.convert(url: tempURL)
        XCTAssertEqual(result.sourceFormat, "EPUB")
        XCTAssertTrue(result.markdown.contains("Capítulo 1"))
        XCTAssertTrue(result.markdown.contains("Primer párrafo."))
    }

    // MARK: - DOCX

    func testConvertsDOCX() async throws {
        tempURL = tempURL.appendingPathExtension("docx")
        try createZIPArchive(at: tempURL, entries: [
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
                    </w:body>
                </w:document>
                """)
        ])

        let result = try await service.convert(url: tempURL)
        XCTAssertEqual(result.sourceFormat, "DOCX")
        XCTAssertTrue(result.markdown.contains("# Título principal"))
        XCTAssertTrue(result.markdown.contains("Primer párrafo."))
    }

    // MARK: - PPTX

    func testConvertsPPTX() async throws {
        tempURL = tempURL.appendingPathExtension("pptx")
        try createZIPArchive(at: tempURL, entries: [
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

        let result = try await service.convert(url: tempURL)
        XCTAssertEqual(result.sourceFormat, "PPTX")
        XCTAssertTrue(result.markdown.contains("Primera diapositiva."))
    }

    // MARK: - XLSX

    func testConvertsXLSX() async throws {
        tempURL = tempURL.appendingPathExtension("xlsx")
        try createZIPArchive(at: tempURL, entries: [
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

        let result = try await service.convert(url: tempURL)
        XCTAssertEqual(result.sourceFormat, "XLSX")
        XCTAssertTrue(result.markdown.contains("| Nombre | Edad |"))
        XCTAssertTrue(result.markdown.contains("| Ana | 30 |"))
    }

    // MARK: - YouTube

    func testConvertsYouTubeURLFile() async throws {
        let mockHTML = try loadFixture(named: "youtube", extension: "html")
        let mockCaptions = try loadFixture(named: "youtube_captions", extension: "xml")
        let youtubeConverter = YouTubeToMarkdownConverter { requestURL in
            if requestURL.absoluteString.contains("timedtext") {
                return mockCaptions
            }
            return mockHTML
        }
        let service = DocumentConversionService(converters: [
            CSVToMarkdownConverter(),
            JSONToMarkdownConverter(),
            XMLToMarkdownConverter(),
            HTMLToMarkdownConverter(),
            ZIPToMarkdownConverter(),
            PDFToMarkdownConverter(),
            ImageToMarkdownConverter(),
            EPUBToMarkdownConverter(),
            DOCXToMarkdownConverter(),
            PPTXToMarkdownConverter(),
            XLSXToMarkdownConverter(),
            youtubeConverter
        ])

        tempURL = tempURL.appendingPathExtension("url")
        try "[InternetShortcut]\nURL=https://www.youtube.com/watch?v=dQw4w9WgXcQ\n".write(
            to: tempURL,
            atomically: true,
            encoding: .utf8
        )

        let result = try await service.convert(url: tempURL)
        XCTAssertEqual(result.sourceFormat, "YouTube")
        XCTAssertTrue(result.markdown.contains("First caption line"))
        XCTAssertTrue(result.markdown.contains("URL: https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
    }

    // MARK: - Helpers

    private func loadFixture(named name: String, extension ext: String) throws -> Data {
        guard let url = Bundle.module.url(forResource: name, withExtension: ext) else {
            XCTFail("Missing fixture: \(name).\(ext)")
            return Data()
        }
        return try Data(contentsOf: url)
    }

    private func writePDF(pages: [String]) {
        var mediaBox = CGRect(x: 0, y: 0, width: 200, height: 100)
        guard let context = CGContext(tempURL as CFURL, mediaBox: &mediaBox, nil) else {
            XCTFail("Could not create PDF context")
            return
        }

        for text in pages {
            context.beginPDFPage(nil as CFDictionary?)
            let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = graphicsContext

            let attributed = NSAttributedString(
                string: text,
                attributes: [.font: NSFont.systemFont(ofSize: 12)]
            )
            attributed.draw(in: CGRect(x: 10, y: 50, width: 180, height: 40))

            NSGraphicsContext.restoreGraphicsState()
            context.endPDFPage()
        }

        context.closePDF()
    }

    private func writePNG(size: CGSize, fillColor: NSColor = .red) {
        let width = Int(size.width)
        let height = Int(size.height)

        guard let context = CGContext(
            data: nil,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: 0,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ) else {
            XCTFail("Could not create image context")
            return
        }

        context.setFillColor(fillColor.cgColor)
        context.fill(CGRect(origin: .zero, size: size))

        guard let cgImage = context.makeImage(),
              let destination = CGImageDestinationCreateWithURL(tempURL as CFURL, UTType.png.identifier as CFString, 1, nil) else {
            XCTFail("Could not create PNG image")
            return
        }

        CGImageDestinationAddImage(destination, cgImage, nil)
        CGImageDestinationFinalize(destination)
    }

    private func createEPUB(at epubURL: URL, entries: [(path: String, content: String)]) throws {
        try createZIPArchive(at: epubURL, entries: entries)
    }

    private func createZIPArchive(at archiveURL: URL, entries: [(path: String, content: String)]) throws {
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
        process.arguments = [archiveURL.path] + entries.map(\.path)

        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "zip command failed")
    }
}
