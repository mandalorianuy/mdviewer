import XCTest
@testable import MDViewer

final class FormatDetectorTests: XCTestCase {
    func testDetectsCSV() {
        let detector = FormatDetector(converters: [CSVToMarkdownConverter()])
        let url = URL(fileURLWithPath: "/tmp/sample.csv")
        XCTAssertNotNil(detector.converter(for: url))
    }

    func testReturnsNilForUnknownExtension() {
        let detector = FormatDetector(converters: [CSVToMarkdownConverter()])
        let url = URL(fileURLWithPath: "/tmp/sample.unknown")
        XCTAssertNil(detector.converter(for: url))
    }

    func testDetectsUppercaseExtension() {
        let detector = FormatDetector(converters: [CSVToMarkdownConverter()])
        let url = URL(fileURLWithPath: "/tmp/sample.CSV")
        XCTAssertNotNil(detector.converter(for: url))
    }

    func testFindsConverterByExtension() {
        let detector = FormatDetector(converters: [CSVToMarkdownConverter()])
        XCTAssertNotNil(detector.converter(forExtension: "csv"))
        XCTAssertNil(detector.converter(forExtension: "unknown"))
    }
}
