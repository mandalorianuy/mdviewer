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
        try createZIP(at: tempURL, entries: [
            ("data.csv", "A,B\n1,2")
        ])

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B |"))
        XCTAssertEqual(result.sourceFormat, "ZIP (CSV)")
    }

    func testEmptyZIPReturnsFallback() throws {
        try createZIP(at: tempURL, entries: [])

        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.sourceFormat, "ZIP")
        XCTAssertTrue(result.markdown.contains("_El archivo ZIP no contiene formatos soportados._"))
        XCTAssertTrue(result.warnings.contains("No se encontro un archivo convertible dentro del ZIP."))
    }

    func testZIPWithUnsupportedFilesReturnsFallback() throws {
        try createZIP(at: tempURL, entries: [
            ("notes.txt", "hello world")
        ])

        let result = try converter.convert(tempURL)
        XCTAssertEqual(result.sourceFormat, "ZIP")
        XCTAssertTrue(result.markdown.contains("_El archivo ZIP no contiene formatos soportados._"))
        XCTAssertTrue(result.warnings.contains("No se encontro un archivo convertible dentro del ZIP."))
    }

    func testZIPWithMultipleConvertibleFilesConvertsFirstAndWarns() throws {
        try createZIP(at: tempURL, entries: [
            ("a.csv", "A,B\n1,2"),
            ("b.json", "{\"x\":1}")
        ])

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B |"))
        XCTAssertEqual(result.sourceFormat, "ZIP (CSV)")
        XCTAssertTrue(result.warnings.contains(where: { $0.contains("varios archivos soportados") }))
    }

    private func createZIP(at zipURL: URL, entries: [(name: String, content: String)]) throws {
        let staging = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: staging) }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
        process.currentDirectoryURL = staging

        if entries.isEmpty {
            let emptyDir = staging.appendingPathComponent("empty")
            try FileManager.default.createDirectory(at: emptyDir, withIntermediateDirectories: true)
            process.arguments = [zipURL.path, "empty"]
        } else {
            for entry in entries {
                let fileURL = staging.appendingPathComponent(entry.name)
                try entry.content.write(to: fileURL, atomically: true, encoding: .utf8)
            }
            process.arguments = [zipURL.path] + entries.map(\.name)
        }

        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0, "zip command failed")
    }
}
