import Foundation

struct FormatDetector: Sendable {
    private let converters: [any DocumentConverter]

    init(converters: [any DocumentConverter]) {
        self.converters = converters
    }

    func converter(for url: URL) -> (any DocumentConverter)? {
        converters.first { $0.canConvert(url) }
    }

    func converter(forExtension ext: String) -> (any DocumentConverter)? {
        let lowercased = ext.lowercased()
        return converters.first { $0.supportedExtensions.contains(lowercased) }
    }
}
