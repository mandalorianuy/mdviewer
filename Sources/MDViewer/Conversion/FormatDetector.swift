import Foundation

struct FormatDetector: Sendable {
    private let converters: [DocumentConverter]

    init(converters: [DocumentConverter]) {
        self.converters = converters
    }

    func converter(for url: URL) -> DocumentConverter? {
        converters.first { $0.canConvert(url) }
    }

    func converter(forExtension ext: String) -> DocumentConverter? {
        let lowercased = ext.lowercased()
        return converters.first { $0.supportedExtensions.contains(lowercased) }
    }
}
