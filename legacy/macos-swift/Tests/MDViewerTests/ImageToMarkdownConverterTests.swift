import XCTest
@testable import MDViewer
import AppKit
import CoreGraphics
import UniformTypeIdentifiers

final class ImageToMarkdownConverterTests: XCTestCase {
    private let converter = ImageToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".png")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
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
            XCTFail("No se pudo crear el contexto de imagen")
            return
        }

        context.setFillColor(fillColor.cgColor)
        context.fill(CGRect(origin: .zero, size: size))

        guard let cgImage = context.makeImage(),
              let destination = CGImageDestinationCreateWithURL(tempURL as CFURL, UTType.png.identifier as CFString, 1, nil) else {
            XCTFail("No se pudo crear la imagen PNG")
            return
        }

        CGImageDestinationAddImage(destination, cgImage, nil)
        CGImageDestinationFinalize(destination)
    }

    private func writePNGWithText(_ text: String, size: CGSize = CGSize(width: 400, height: 100)) {
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
            XCTFail("No se pudo crear el contexto de imagen")
            return
        }

        context.setFillColor(NSColor.white.cgColor)
        context.fill(CGRect(origin: .zero, size: size))

        let graphicsContext = NSGraphicsContext(cgContext: context, flipped: false)
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = graphicsContext

        let attributed = NSAttributedString(
            string: text,
            attributes: [
                .font: NSFont.systemFont(ofSize: 36),
                .foregroundColor: NSColor.black
            ]
        )
        attributed.draw(in: CGRect(x: 20, y: 30, width: size.width - 40, height: size.height - 40))

        NSGraphicsContext.restoreGraphicsState()

        guard let cgImage = context.makeImage(),
              let destination = CGImageDestinationCreateWithURL(tempURL as CFURL, UTType.png.identifier as CFString, 1, nil) else {
            XCTFail("No se pudo crear la imagen PNG")
            return
        }

        CGImageDestinationAddImage(destination, cgImage, nil)
        CGImageDestinationFinalize(destination)
    }

    func testSupportedExtensions() {
        XCTAssertEqual(
            Set(converter.supportedExtensions),
            Set(["jpg", "jpeg", "png", "heic", "tiff", "tif", "webp"])
        )
    }

    func testConvertSimpleImageRunsAndReturnsFallback() throws {
        writePNG(size: CGSize(width: 100, height: 100))
        let result = try converter.convert(tempURL)

        XCTAssertEqual(result.markdown, "_No se detectó metadata ni texto en la imagen._")
        XCTAssertEqual(result.sourceFormat, "Imagen")
        XCTAssertEqual(result.title, tempURL.lastPathComponent)
        XCTAssertTrue(result.warnings.contains("No se detectó metadata ni texto en la imagen."))
        XCTAssertTrue(result.warnings.contains("El texto se extrajo con OCR y puede contener errores."))
    }

    func testNonReadableFileThrows() throws {
        writePNG(size: CGSize(width: 100, height: 100))
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

    func testUnsupportedExtensionIsRejectedByFormatDetector() {
        let detector = FormatDetector(converters: [ImageToMarkdownConverter()])
        XCTAssertNil(detector.converter(forExtension: "pdf"))
        XCTAssertNotNil(detector.converter(forExtension: "png"))
        XCTAssertNotNil(detector.converter(forExtension: "JPG"))
    }

    func testOCRExtractsTextFromImage() throws {
        writePNGWithText("OCR")
        let result = try converter.convert(tempURL)

        XCTAssertTrue(
            result.markdown.contains("## Texto detectado"),
            "El resultado debería incluir una sección de texto detectado"
        )
        XCTAssertTrue(
            result.markdown.uppercased().contains("OCR"),
            "El OCR debería detectar el texto 'OCR'"
        )
        XCTAssertTrue(result.warnings.contains("El texto se extrajo con OCR y puede contener errores."))
    }
}
