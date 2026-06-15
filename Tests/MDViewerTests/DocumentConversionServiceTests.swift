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
