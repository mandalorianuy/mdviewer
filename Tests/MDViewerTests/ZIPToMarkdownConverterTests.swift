import XCTest
@testable import MDViewer

final class ZIPToMarkdownConverterTests: XCTestCase {
    private let converter = ZIPToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".zip")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testZIPWithCSV() throws {
        let csvURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".csv")
        try "A,B\n1,2".write(to: csvURL, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: csvURL) }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
        process.arguments = [tempURL.path, csvURL.path]
        try process.run()
        process.waitUntilExit()

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B |"))
    }
}
