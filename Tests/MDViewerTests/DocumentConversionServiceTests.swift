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
            XCTFail("Debería haber fallado")
        } catch {
            XCTAssertTrue(error is ConversionError)
        }
    }
}
