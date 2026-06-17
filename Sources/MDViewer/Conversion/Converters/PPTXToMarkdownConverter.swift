import Foundation

struct PPTXToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["pptx"]

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            throw ConversionError.fileNotReadable
        }

        let tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)

        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        defer {
            try? FileManager.default.removeItem(at: tempDir)
        }

        try unzipOOXML(url, into: tempDir)

        let slidesDir = tempDir.appendingPathComponent("ppt/slides")
        let slideURLs = try listFiles(at: slidesDir)
            .filter { $0.lastPathComponent.hasPrefix("slide") && $0.pathExtension.lowercased() == "xml" }
            .sorted { slideNumber($0) < slideNumber($1) }

        guard !slideURLs.isEmpty else {
            throw ConversionError.conversionFailed(reason: "No se encontraron diapositivas en el PPTX.")
        }

        var parts: [String] = []
        for slideURL in slideURLs {
            let slide = try parseOOXMLDocument(at: slideURL)
            let texts = slide.descendants(named: "t").map { $0.allText() }
            let slideText = texts.joined(separator: " ")
                .trimmingCharacters(in: .whitespacesAndNewlines)

            let number = slideNumber(slideURL)
            parts.append("## Diapositiva \(number)\n\n\(slideText)")
        }

        let markdown = parts.joined(separator: "\n\n")

        return MarkdownConversionResult(
            markdown: markdown.isEmpty ? "_Presentación PPTX vacía_" : markdown,
            sourceFormat: "PPTX",
            title: nil,
            warnings: []
        )
    }

    private func slideNumber(_ url: URL) -> Int {
        let name = url.deletingPathExtension().lastPathComponent
        let digits = name.drop(while: { !$0.isNumber })
        return Int(digits) ?? 0
    }
}
