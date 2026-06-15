import Foundation

struct ZIPToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["zip"]

    private let service: DocumentConversionService

    private static let innerService = DocumentConversionService(converters: [
        CSVToMarkdownConverter(),
        JSONToMarkdownConverter(),
        XMLToMarkdownConverter(),
        HTMLToMarkdownConverter()
    ])

    init(service: DocumentConversionService = innerService) {
        self.service = service
    }

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            throw ConversionError.fileNotReadable
        }

        let tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)

        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        process.arguments = ["-o", url.path, "-d", tempDir.path]

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            throw ConversionError.conversionFailed(reason: error.localizedDescription)
        }

        guard process.terminationStatus == 0 else {
            throw ConversionError.conversionFailed(reason: "unzip exited with status \(process.terminationStatus)")
        }

        let files = try listFiles(at: tempDir)
        let convertibleFiles = files.filter { path in
            DocumentConversionService.isConvertibleExtension(URL(fileURLWithPath: path).pathExtension)
        }

        guard let firstFile = convertibleFiles.first else {
            let index = files.map { "- \($0)" }.joined(separator: "\n")
            return MarkdownConversionResult(
                markdown: "_El archivo ZIP no contiene formatos soportados._\n\n## Contenido\n\n\(index.isEmpty ? "_Vacío_" : index)",
                sourceFormat: "ZIP",
                title: nil,
                warnings: ["No se encontro un archivo convertible dentro del ZIP."]
            )
        }

        let firstURL = URL(fileURLWithPath: firstFile)
        let innerResult = try service.convertSync(url: firstURL)

        var warnings = innerResult.warnings
        if convertibleFiles.count > 1 {
            warnings.append("El ZIP contiene varios archivos soportados; se convirtio el primero: \(firstURL.lastPathComponent).")
        }

        return MarkdownConversionResult(
            markdown: innerResult.markdown,
            sourceFormat: "ZIP (\(innerResult.sourceFormat))",
            title: innerResult.title,
            warnings: warnings
        )
    }

    private func listFiles(at directory: URL) throws -> [String] {
        let enumerator = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: nil)
        var files: [String] = []

        while let fileURL = enumerator?.nextObject() as? URL {
            var isDirectory: ObjCBool = false
            FileManager.default.fileExists(atPath: fileURL.path, isDirectory: &isDirectory)
            if !isDirectory.boolValue {
                files.append(fileURL.path)
            }
        }

        return files.sorted()
    }
}
