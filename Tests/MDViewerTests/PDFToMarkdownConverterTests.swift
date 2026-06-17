import XCTest
@testable import MDViewer
import AppKit
import PDFKit

final class PDFToMarkdownConverterTests: XCTestCase {
    private let converter = PDFToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".pdf")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writePDF(pages: [String]) {
        var mediaBox = CGRect(x: 0, y: 0, width: 200, height: 100)
        guard let context = CGContext(tempURL as CFURL, mediaBox: &mediaBox, nil) else {
            XCTFail("No se pudo crear el contexto PDF")
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

    func testSinglePagePDF() throws {
        writePDF(pages: ["Hola PDF"])
        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.markdown, "## Página 1\nHola PDF")
        XCTAssertTrue(result.warnings.isEmpty)
    }

    func testMultiPagePDFIncludesPageHeaders() throws {
        writePDF(pages: ["Primera página", "Segunda página"])
        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("## Página 1"))
        XCTAssertTrue(result.markdown.contains("Primera página"))
        XCTAssertTrue(result.markdown.contains("## Página 2"))
        XCTAssertTrue(result.markdown.contains("Segunda página"))
    }

    func testEmptyPDFReturnsWarning() throws {
        writePDF(pages: [""])
        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.markdown, "_El PDF no contiene texto extraíble._")
        XCTAssertEqual(result.warnings, ["El PDF no contiene texto extraíble."])
    }

    func testTitleMetadataIsExtracted() throws {
        writePDF(pages: ["Contenido"])

        guard let document = PDFDocument(url: tempURL) else {
            XCTFail("No se pudo cargar el PDF")
            return
        }
        document.documentAttributes = [PDFDocumentAttribute.titleAttribute: "Título de prueba"]
        XCTAssertTrue(document.write(toFile: tempURL.path))

        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.title, "Título de prueba")
    }

    func testNonReadableFileThrows() throws {
        writePDF(pages: ["Confidencial"])
        try FileManager.default.setAttributes([.posixPermissions: 0o000], ofItemAtPath: tempURL.path)

        XCTAssertThrowsError(try converter.convert(tempURL)) { error in
            guard let conversionError = error as? ConversionError else {
                XCTFail("Se esperaba ConversionError")
                return
            }
            if case .fileNotReadable = conversionError {
                // expected
            } else {
                XCTFail("Se esperaba ConversionError.fileNotReadable")
            }
        }
    }
}
