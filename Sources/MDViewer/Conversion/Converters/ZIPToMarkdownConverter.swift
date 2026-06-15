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

        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        defer {
            try? FileManager.default.removeItem(at: tempDir)
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        process.arguments = ["-j", "-o", url.path, "-d", tempDir.path]

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        let timeoutSeconds: TimeInterval = 30
        let timeoutLock = NSLock()
        var didTimeOut = false
        var extractionFinished = false

        let timer = DispatchSource.makeTimerSource(queue: DispatchQueue.global())
        timer.schedule(deadline: .now() + timeoutSeconds)
        timer.setEventHandler { [weak process] in
            timeoutLock.lock()
            defer { timeoutLock.unlock() }
            guard !extractionFinished else { return }
            didTimeOut = true
            process?.terminate()
        }

        do {
            timer.resume()
            try process.run()
            process.waitUntilExit()
            timeoutLock.lock(); extractionFinished = true; timeoutLock.unlock()
            timer.cancel()
        } catch {
            timeoutLock.lock(); extractionFinished = true; timeoutLock.unlock()
            timer.cancel()
            throw ConversionError.conversionFailed(reason: error.localizedDescription)
        }

        if didTimeOut && process.terminationReason == .uncaughtSignal {
            throw ConversionError.timeout
        }

        guard process.terminationStatus == 0 else {
            let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
            let stderr = String(data: stderrData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
            let reason = stderr?.isEmpty == false ? stderr! : "unzip exited with status \(process.terminationStatus)"
            throw ConversionError.conversionFailed(reason: reason)
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
                warnings: ["No se encontró un archivo convertible dentro del ZIP."]
            )
        }

        let firstURL = URL(fileURLWithPath: firstFile)
        let innerResult = try service.convertSync(url: firstURL)

        var warnings = innerResult.warnings
        if convertibleFiles.count > 1 {
            warnings.append("El ZIP contiene varios archivos soportados; se convirtió el primero: \(firstURL.lastPathComponent).")
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
