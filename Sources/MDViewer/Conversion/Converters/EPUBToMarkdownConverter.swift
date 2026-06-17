import Foundation

struct EPUBToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["epub"]

    private static let maxHTMLFiles = 50

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

        try unzip(url, into: tempDir)

        let containerURL = tempDir.appendingPathComponent("META-INF/container.xml")
        guard FileManager.default.isReadableFile(atPath: containerURL.path) else {
            throw ConversionError.conversionFailed(reason: "No se encontró META-INF/container.xml en el EPUB.")
        }

        let containerContent: String
        do {
            containerContent = try String(contentsOf: containerURL, encoding: .utf8)
        } catch {
            throw ConversionError.conversionFailed(reason: "No se pudo leer container.xml: \(error.localizedDescription)")
        }

        guard let opfRelativePath = extractFirstRootfileFullPath(from: containerContent) else {
            throw ConversionError.conversionFailed(reason: "No se pudo determinar la ruta del OPF desde container.xml.")
        }

        let opfURL = resolveURL(base: tempDir, relativePath: opfRelativePath)
        guard FileManager.default.isReadableFile(atPath: opfURL.path) else {
            throw ConversionError.conversionFailed(reason: "No se encontró el archivo OPF \(opfRelativePath).")
        }

        let opfContent: String
        do {
            opfContent = try String(contentsOf: opfURL, encoding: .utf8)
        } catch {
            throw ConversionError.conversionFailed(reason: "No se pudo leer el OPF: \(error.localizedDescription)")
        }

        let title = extractTitle(from: opfContent)
        let manifest = extractXHTMLManifest(from: opfContent)
        let spineIdrefs = extractSpineIdrefs(from: opfContent)
        let orderedHrefs = spineIdrefs.compactMap { manifest[$0] }

        var warnings: [String] = [
            "La estructura del EPUB se convirtió a Markdown plano; el formato y los estilos originales pueden no conservarse."
        ]

        let hrefsToConvert = Array(orderedHrefs.prefix(Self.maxHTMLFiles))
        let truncated = orderedHrefs.count > Self.maxHTMLFiles

        var convertedParts: [String] = []
        for href in hrefsToConvert {
            let htmlURL = resolveURL(base: opfURL.deletingLastPathComponent(), relativePath: href)
            guard FileManager.default.isReadableFile(atPath: htmlURL.path) else { continue }

            do {
                let result = try HTMLToMarkdownConverter().convert(htmlURL)
                if !result.markdown.isEmpty {
                    convertedParts.append(result.markdown)
                }
                warnings.append(contentsOf: result.warnings)
            } catch {
                continue
            }
        }

        if truncated {
            warnings.append("El EPUB contiene más de \(Self.maxHTMLFiles) archivos HTML; solo se convirtieron los primeros \(Self.maxHTMLFiles).")
        }

        let chapterCount = orderedHrefs.count

        guard !convertedParts.isEmpty else {
            return MarkdownConversionResult(
                markdown: "_El EPUB no contiene contenido convertible._",
                sourceFormat: "EPUB",
                title: title,
                warnings: warnings + ["No se encontró contenido HTML convertible en el EPUB."],
                metadata: ["chapterCount": String(chapterCount)]
            )
        }

        return MarkdownConversionResult(
            markdown: convertedParts.joined(separator: "\n\n---\n\n"),
            sourceFormat: "EPUB",
            title: title,
            warnings: warnings,
            metadata: ["chapterCount": String(chapterCount)]
        )
    }

    // MARK: - Unzip

    private func unzip(_ url: URL, into destination: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        process.arguments = ["-o", "-q", url.path, "-d", destination.path]

        let stderrPipe = Pipe()
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
    }

    private func resolveURL(base: URL, relativePath: String) -> URL {
        let trimmed = relativePath.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        return URL(fileURLWithPath: base.path + "/" + trimmed)
    }

    // MARK: - OPF / container parsing

    private func extractFirstRootfileFullPath(from xml: String) -> String? {
        let pattern = "<rootfile\\b[^>]*full-path=\"([^\"]*)\""
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive]),
              let match = regex.firstMatch(in: xml, options: [], range: NSRange(xml.startIndex..., in: xml)),
              let range = Range(match.range(at: 1), in: xml) else {
            return nil
        }
        return String(xml[range]).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func extractTitle(from opf: String) -> String? {
        let pattern = "<dc:title\\b[^>]*>(.*?)</dc:title>"
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive, .dotMatchesLineSeparators]),
              let match = regex.firstMatch(in: opf, options: [], range: NSRange(opf.startIndex..., in: opf)),
              let range = Range(match.range(at: 1), in: opf) else {
            return nil
        }
        return String(opf[range]).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func extractXHTMLManifest(from opf: String) -> [String: String] {
        let itemPattern = "<item\\b([^>]*)>"
        guard let regex = try? NSRegularExpression(pattern: itemPattern, options: [.caseInsensitive]) else {
            return [:]
        }

        var manifest: [String: String] = [:]
        let matches = regex.matches(in: opf, options: [], range: NSRange(opf.startIndex..., in: opf))
        for match in matches {
            guard let range = Range(match.range(at: 1), in: opf) else { continue }
            let attrs = String(opf[range])
            let dict = attributes(from: attrs)
            guard let id = dict["id"], let href = dict["href"] else { continue }
            guard dict["media-type"]?.lowercased() == "application/xhtml+xml" else { continue }
            manifest[id] = href
        }
        return manifest
    }

    private func extractSpineIdrefs(from opf: String) -> [String] {
        let itemrefPattern = "<itemref\\b([^>]*)>"
        guard let regex = try? NSRegularExpression(pattern: itemrefPattern, options: [.caseInsensitive]) else {
            return []
        }

        var idrefs: [String] = []
        let matches = regex.matches(in: opf, options: [], range: NSRange(opf.startIndex..., in: opf))
        for match in matches {
            guard let range = Range(match.range(at: 1), in: opf) else { continue }
            let attrs = String(opf[range])
            let dict = attributes(from: attrs)
            if let idref = dict["idref"] {
                idrefs.append(idref)
            }
        }
        return idrefs
    }

    private func attributes(from elementContent: String) -> [String: String] {
        let pattern = "([a-zA-Z0-9_-]+)=[\"']([^\"']*)[\"']"
        guard let regex = try? NSRegularExpression(pattern: pattern, options: []) else {
            return [:]
        }

        var result: [String: String] = [:]
        let matches = regex.matches(in: elementContent, options: [], range: NSRange(elementContent.startIndex..., in: elementContent))
        for match in matches {
            guard let keyRange = Range(match.range(at: 1), in: elementContent),
                  let valueRange = Range(match.range(at: 2), in: elementContent) else { continue }
            let key = String(elementContent[keyRange]).lowercased()
            let value = String(elementContent[valueRange])
            result[key] = value
        }
        return result
    }
}
