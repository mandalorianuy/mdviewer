import Foundation

struct XLSXToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["xlsx"]

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

        let sharedStringsURL = tempDir.appendingPathComponent("xl/sharedStrings.xml")
        let sharedStrings = FileManager.default.isReadableFile(atPath: sharedStringsURL.path)
            ? try parseSharedStrings(at: sharedStringsURL)
            : []

        let worksheetsDir = tempDir.appendingPathComponent("xl/worksheets")
        let sheetURLs = try listFiles(at: worksheetsDir)
            .filter { $0.lastPathComponent.hasPrefix("sheet") && $0.pathExtension.lowercased() == "xml" }
            .sorted { sheetNumber($0) < sheetNumber($1) }

        guard !sheetURLs.isEmpty else {
            throw ConversionError.conversionFailed(reason: "No se encontraron hojas en el XLSX.")
        }

        var parts: [String] = []
        for (index, sheetURL) in sheetURLs.enumerated() {
            let number = sheetNumber(sheetURL)
            let sheetMarkdown = try renderSheet(at: sheetURL, sharedStrings: sharedStrings)
            if !sheetMarkdown.isEmpty {
                if sheetURLs.count > 1 {
                    parts.append("## Hoja \(number)\n\n\(sheetMarkdown)")
                } else {
                    parts.append(sheetMarkdown)
                }
            }
            _ = index
        }

        let markdown = parts.joined(separator: "\n\n")

        return MarkdownConversionResult(
            markdown: markdown.isEmpty ? "_Libro XLSX vacío_" : markdown,
            sourceFormat: "XLSX",
            title: nil,
            warnings: []
        )
    }

    // MARK: - Shared strings

    private func parseSharedStrings(at url: URL) throws -> [String] {
        let root = try parseOOXMLDocument(at: url)
        return root.children
            .filter { $0.name == "si" }
            .map { $0.allText().trimmingCharacters(in: .whitespacesAndNewlines) }
    }

    // MARK: - Sheet rendering

    private func renderSheet(at url: URL, sharedStrings: [String]) throws -> String {
        let root = try parseOOXMLDocument(at: url)
        guard let sheetData = root.firstElement(named: "sheetData") else {
            return ""
        }

        let rows = sheetData.children.filter { $0.name == "row" }
            .sorted { ($0.attribute("r").flatMap(Int.init) ?? 0) < ($1.attribute("r").flatMap(Int.init) ?? 0) }

        var renderedRows: [[String]] = []
        for row in rows {
            let cells = row.children.filter { $0.name == "c" }
                .sorted { cellColumn($0) < cellColumn($1) }

            let cellTexts = cells.map { cell in
                cellValue(cell, sharedStrings: sharedStrings)
                    .trimmingCharacters(in: .whitespacesAndNewlines)
            }
            renderedRows.append(cellTexts)
        }

        guard let firstRow = renderedRows.first, !firstRow.isEmpty else {
            return ""
        }

        var lines: [String] = []
        lines.append("| " + firstRow.joined(separator: " | ") + " |")
        lines.append("| " + firstRow.map { _ in "---" }.joined(separator: " | ") + " |")

        for row in renderedRows.dropFirst() {
            lines.append("| " + row.joined(separator: " | ") + " |")
        }

        return lines.joined(separator: "\n")
    }

    private func cellValue(_ cell: OOXMLNode, sharedStrings: [String]) -> String {
        let type = cell.attribute("t")?.lowercased()
        let value = cell.firstElement(named: "v")?.allText() ?? ""

        if type == "s" {
            guard let index = Int(value), index >= 0, index < sharedStrings.count else {
                return ""
            }
            return sharedStrings[index]
        }

        return value
    }

    private func cellColumn(_ cell: OOXMLNode) -> String {
        let reference = cell.attribute("r") ?? ""
        return String(reference.prefix(while: { !$0.isNumber }))
    }

    private func sheetNumber(_ url: URL) -> Int {
        let name = url.deletingPathExtension().lastPathComponent
        let digits = name.drop(while: { !$0.isNumber })
        return Int(digits) ?? 0
    }
}
