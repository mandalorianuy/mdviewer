import XCTest
@testable import MDViewer

final class YouTubeToMarkdownConverterTests: XCTestCase {
    private let converter = YouTubeToMarkdownConverter()

    // MARK: - canConvert

    func testCanConvertYouTubeURL() {
        XCTAssertTrue(converter.canConvert(URL(string: "https://www.youtube.com/watch?v=abc123")!))
        XCTAssertTrue(converter.canConvert(URL(string: "https://youtube.com/watch?v=abc123")!))
        XCTAssertTrue(converter.canConvert(URL(string: "https://youtu.be/abc123")!))
    }

    func testCanConvertURLFile() {
        let url = URL(fileURLWithPath: "/tmp/test.url")
        XCTAssertTrue(converter.canConvert(url))
    }

    func testCanConvertWeblocFile() {
        let url = URL(fileURLWithPath: "/tmp/test.webloc")
        XCTAssertTrue(converter.canConvert(url))
    }

    func testCanConvertRejectsNonYouTubeURL() {
        XCTAssertFalse(converter.canConvert(URL(string: "https://example.com")!))
        XCTAssertFalse(converter.canConvert(URL(string: "https://vimeo.com/12345")!))
        XCTAssertFalse(converter.canConvert(URL(fileURLWithPath: "/tmp/test.txt")))
    }

    // MARK: - File parsing

    func testURLFileParsing() throws {
        let tempDir = FileManager.default.temporaryDirectory
        let urlFile = tempDir.appendingPathComponent(UUID().uuidString + ".url")
        try "[InternetShortcut]\nURL=https://www.youtube.com/watch?v=dQw4w9WgXcQ\n".write(
            to: urlFile,
            atomically: true,
            encoding: .utf8
        )
        defer { try? FileManager.default.removeItem(at: urlFile) }

        let mockHTML = try loadFixture(named: "youtube", extension: "html")
        let mockCaptions = try loadFixture(named: "youtube_captions", extension: "xml")
        let converter = YouTubeToMarkdownConverter { requestURL in
            if requestURL.absoluteString.contains("timedtext") {
                return mockCaptions
            }
            return mockHTML
        }
        let result = try converter.convert(urlFile)

        XCTAssertEqual(result.title, "Test Video Title")
        XCTAssertTrue(result.markdown.contains("First caption line"))
        XCTAssertTrue(result.markdown.contains("URL: https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
    }

    func testWeblocFileParsing() throws {
        let tempDir = FileManager.default.temporaryDirectory
        let webloc = tempDir.appendingPathComponent(UUID().uuidString + ".webloc")
        let plist: [String: Any] = ["URL": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"]
        let data = try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
        try data.write(to: webloc)
        defer { try? FileManager.default.removeItem(at: webloc) }

        let mockHTML = try loadFixture(named: "youtube", extension: "html")
        let mockCaptions = try loadFixture(named: "youtube_captions", extension: "xml")
        let converter = YouTubeToMarkdownConverter { requestURL in
            if requestURL.pathExtension.lowercased() == "webloc" {
                return data
            }
            if requestURL.absoluteString.contains("timedtext") {
                return mockCaptions
            }
            return mockHTML
        }
        let result = try converter.convert(webloc)

        XCTAssertEqual(result.title, "Test Video Title")
        XCTAssertTrue(result.markdown.contains("First caption line"))
        XCTAssertTrue(result.markdown.contains("URL: https://www.youtube.com/watch?v=dQw4w9WgXcQ"))
    }

    // MARK: - Transcript extraction from fixture

    func testDirectYouTubeURLWithFixture() throws {
        let mockHTML = try loadFixture(named: "youtube", extension: "html")
        let mockCaptions = try loadFixture(named: "youtube_captions", extension: "xml")
        let converter = YouTubeToMarkdownConverter { url in
            if url.absoluteString.contains("timedtext") {
                return mockCaptions
            }
            return mockHTML
        }

        let result = try converter.convert(URL(string: "https://www.youtube.com/watch?v=test")!)

        XCTAssertEqual(result.title, "Test Video Title")
        XCTAssertTrue(result.markdown.contains("First caption line"))
        XCTAssertTrue(result.markdown.contains("Second caption line"))
        XCTAssertTrue(result.markdown.contains("URL: https://www.youtube.com/watch?v=test"))
        XCTAssertFalse(result.warnings.isEmpty)
    }

    func testFallbackWhenTranscriptMissing() throws {
        let html = """
        <html><head>
        <title>Fallback Title - YouTube</title>
        <meta name="description" content="Fallback description">
        </head><body></body></html>
        """
        let converter = YouTubeToMarkdownConverter { _ in Data(html.utf8) }
        let result = try converter.convert(URL(string: "https://www.youtube.com/watch?v=fallback")!)

        XCTAssertEqual(result.title, "Fallback Title")
        XCTAssertTrue(result.markdown.contains("Fallback description"))
        XCTAssertTrue(result.markdown.contains("URL: https://www.youtube.com/watch?v=fallback"))
    }

    func testNetworkFailureFallback() throws {
        let converter = YouTubeToMarkdownConverter { _ in
            throw URLError(.notConnectedToInternet)
        }
        let result = try converter.convert(URL(string: "https://www.youtube.com/watch?v=offline")!)

        XCTAssertEqual(result.title, "Video de YouTube")
        XCTAssertTrue(result.markdown.contains("URL: https://www.youtube.com/watch?v=offline"))
    }

    // MARK: - Invalid URL

    func testInvalidURLThrows() {
        XCTAssertThrowsError(try converter.convert(URL(string: "https://example.com/not-youtube")!)) { error in
            guard let conversionError = error as? ConversionError else {
                XCTFail("Se esperaba ConversionError")
                return
            }
            if case .conversionFailed(let reason) = conversionError {
                XCTAssertEqual(reason, "URL de YouTube no válida")
            } else {
                XCTFail("Se esperaba ConversionError.conversionFailed")
            }
        }
    }

    // MARK: - Helpers

    private func loadFixture(named name: String, extension ext: String) throws -> Data {
        guard let url = Bundle.module.url(forResource: name, withExtension: ext) else {
            XCTFail("No se encontró el fixture \(name).\(ext)")
            return Data()
        }
        return try Data(contentsOf: url)
    }
}
