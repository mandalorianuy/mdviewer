import Foundation
import os.log

actor DocumentConversionService {
    static let shared = DocumentConversionService()

    nonisolated private let detector: FormatDetector
    private let logger = Logger(subsystem: "com.facundo.mdviewer.conversion", category: "conversion")

    init(converters: [DocumentConverter] = DocumentConversionService.defaultConverters) {
        self.detector = FormatDetector(converters: converters)
    }

    static var defaultConverters: [DocumentConverter] {
        [
            CSVToMarkdownConverter(),
            JSONToMarkdownConverter(),
            XMLToMarkdownConverter(),
            HTMLToMarkdownConverter(),
            ZIPToMarkdownConverter()
        ]
    }

    static func isConvertibleExtension(_ ext: String) -> Bool {
        let knownExtensions = ["csv", "json", "xml", "html", "htm"]
        return knownExtensions.contains(ext.lowercased())
    }

    func convert(url: URL) async throws -> MarkdownConversionResult {
        logger.info("Convirtiendo archivo: \(url.lastPathComponent)")

        return try await Task.detached(priority: .userInitiated) { [detector] in
            try Self.convertSync(detector: detector, url: url)
        }.value
    }

    nonisolated func convertSync(url: URL) throws -> MarkdownConversionResult {
        try Self.convertSync(detector: detector, url: url)
    }

    private static func convertSync(detector: FormatDetector, url: URL) throws -> MarkdownConversionResult {
        guard let converter = detector.converter(for: url) else {
            throw ConversionError.unsupportedFormat
        }

        do {
            return try converter.convert(url)
        } catch let error as ConversionError {
            throw error
        } catch {
            throw ConversionError.conversionFailed(reason: error.localizedDescription)
        }
    }
}
