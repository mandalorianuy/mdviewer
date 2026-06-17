import Foundation

struct DOCXToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["docx"]

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

        let documentURL = tempDir.appendingPathComponent("word/document.xml")
        guard FileManager.default.isReadableFile(atPath: documentURL.path) else {
            throw ConversionError.conversionFailed(reason: "No se encontró word/document.xml en el DOCX.")
        }

        let document = try parseOOXMLDocument(at: documentURL)
        guard let body = document.firstElement(named: "body") else {
            throw ConversionError.conversionFailed(reason: "No se encontró el cuerpo del documento DOCX.")
        }

        var parts: [String] = []
        for child in body.children {
            switch child.name {
            case "p":
                if let paragraph = renderParagraph(child) {
                    parts.append(paragraph)
                }
            case "tbl":
                if let table = renderTable(child) {
                    parts.append(table)
                }
            default:
                break
            }
        }

        let markdown = parts.joined(separator: "\n\n")

        return MarkdownConversionResult(
            markdown: markdown.isEmpty ? "_Documento DOCX vacío_" : markdown,
            sourceFormat: "DOCX",
            title: nil,
            warnings: []
        )
    }

    // MARK: - Paragraph rendering

    private func renderParagraph(_ paragraph: OOXMLNode) -> String? {
        let text = paragraph.descendants(named: "t").map { $0.allText() }.joined()
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        let prefix = headingPrefix(for: paragraph)
        return prefix + trimmed
    }

    private func headingPrefix(for paragraph: OOXMLNode) -> String {
        guard let pPr = paragraph.firstElement(named: "pPr"),
              let pStyle = pPr.firstElement(named: "pStyle"),
              let styleValue = pStyle.attribute("val")?.lowercased() else {
            return ""
        }

        switch styleValue {
        case "heading1", "título1", "title":
            return "# "
        case "heading2", "título2":
            return "## "
        case "heading3", "título3":
            return "### "
        default:
            return ""
        }
    }

    // MARK: - Table rendering

    private func renderTable(_ table: OOXMLNode) -> String? {
        let rows = table.children.filter { $0.name == "tr" }
        guard !rows.isEmpty else { return nil }

        var renderedRows: [[String]] = []
        for row in rows {
            let cells = row.children.filter { $0.name == "tc" }
            let cellTexts = cells.map { cell in
                cell.descendants(named: "t").map { $0.allText() }.joined()
                    .trimmingCharacters(in: .whitespacesAndNewlines)
            }
            renderedRows.append(cellTexts)
        }

        guard let firstRow = renderedRows.first, !firstRow.isEmpty else { return nil }

        var lines: [String] = []
        lines.append("| " + firstRow.joined(separator: " | ") + " |")
        lines.append("| " + firstRow.map { _ in "---" }.joined(separator: " | ") + " |")

        for row in renderedRows.dropFirst() {
            lines.append("| " + row.joined(separator: " | ") + " |")
        }

        return lines.joined(separator: "\n")
    }
}
