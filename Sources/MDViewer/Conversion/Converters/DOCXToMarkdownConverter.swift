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

        let relationships = parseRelationships(at: tempDir.appendingPathComponent("word/_rels/document.xml.rels"))
        let numbering = parseNumbering(at: tempDir.appendingPathComponent("word/numbering.xml"))
        let title = parseCoreTitle(at: tempDir.appendingPathComponent("docProps/core.xml"))

        var parts: [String] = []
        for child in body.children {
            switch child.name {
            case "p":
                if let paragraph = renderParagraph(child, relationships: relationships, numbering: numbering) {
                    parts.append(paragraph)
                }
            case "tbl":
                if let table = renderTable(child, relationships: relationships) {
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
            title: title,
            warnings: []
        )
    }

    // MARK: - Relationships

    private func parseRelationships(at url: URL) -> [String: String] {
        guard FileManager.default.isReadableFile(atPath: url.path),
              let root = try? parseOOXMLDocument(at: url) else {
            return [:]
        }

        var relationships: [String: String] = [:]
        for relationship in root.children where relationship.name == "Relationship" {
            guard let id = relationship.attribute("id"),
                  let target = relationship.attribute("target") else {
                continue
            }
            relationships[id] = target
        }
        return relationships
    }

    // MARK: - Numbering

    private func parseNumbering(at url: URL) -> [String: Bool] {
        guard FileManager.default.isReadableFile(atPath: url.path),
              let root = try? parseOOXMLDocument(at: url) else {
            return [:]
        }

        let abstractFormats: [String: String] = {
            var formats: [String: String] = [:]
            for abstractNum in root.children where abstractNum.name == "abstractNum" {
                guard let abstractNumId = abstractNum.attribute("abstractnumid") else { continue }
                for lvl in abstractNum.children where lvl.name == "lvl" {
                    guard lvl.attribute("ilvl") == "0",
                          let numFmt = lvl.firstElement(named: "numFmt")?.attribute("val") else {
                        continue
                    }
                    formats[abstractNumId] = numFmt.lowercased()
                    break
                }
            }
            return formats
        }()

        var numbering: [String: Bool] = [:]
        for num in root.children where num.name == "num" {
            guard let numId = num.attribute("numid"),
                  let abstractNumId = num.firstElement(named: "abstractNumId")?.attribute("val"),
                  let numFmt = abstractFormats[abstractNumId] else {
                continue
            }
            numbering[numId] = (numFmt == "decimal")
        }
        return numbering
    }

    // MARK: - Core properties

    private func parseCoreTitle(at url: URL) -> String? {
        guard FileManager.default.isReadableFile(atPath: url.path),
              let root = try? parseOOXMLDocument(at: url) else {
            return nil
        }

        let title = root.firstElement(named: "title")?.allText()
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return title?.isEmpty == false ? title : nil
    }

    // MARK: - Paragraph rendering

    private func renderParagraph(
        _ paragraph: OOXMLNode,
        relationships: [String: String],
        numbering: [String: Bool]
    ) -> String? {
        let inlineText = renderInlineNodes(paragraph.children, relationships: relationships)
        let trimmed = inlineText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        if let listPrefix = listPrefix(for: paragraph, numbering: numbering) {
            return listPrefix + trimmed
        }

        let headingPrefix = headingPrefix(for: paragraph)
        return headingPrefix + trimmed
    }

    private func renderInlineNodes(_ nodes: [OOXMLNode], relationships: [String: String]) -> String {
        var result = ""
        for node in nodes {
            switch node.name {
            case "r":
                result += renderRun(node)
            case "hyperlink":
                result += renderHyperlink(node, relationships: relationships)
            case "tab":
                result += "\t"
            case "br":
                result += "\n"
            default:
                break
            }
        }
        return result
    }

    private func renderRun(_ run: OOXMLNode) -> String {
        let rPr = run.firstElement(named: "rPr")
        let isBold = rPr?.firstElement(named: "b") != nil
        let isItalic = rPr?.firstElement(named: "i") != nil
        let isUnderline = rPr?.firstElement(named: "u") != nil
        let isStrike = rPr?.firstElement(named: "strike") != nil
            || rPr?.firstElement(named: "dstrike") != nil

        var text = ""
        for child in run.children {
            switch child.name {
            case "t":
                text += child.allText()
            case "tab":
                text += "\t"
            case "br":
                text += "\n"
            default:
                break
            }
        }

        return applyInlineFormatting(
            text,
            bold: isBold,
            italic: isItalic,
            underline: isUnderline,
            strikethrough: isStrike
        )
    }

    private func renderHyperlink(_ hyperlink: OOXMLNode, relationships: [String: String]) -> String {
        let text = hyperlink.children
            .filter { $0.name == "r" }
            .map { renderRun($0) }
            .joined()

        guard let relationshipId = hyperlink.attribute("id"),
              let url = relationships[relationshipId],
              !url.isEmpty else {
            return text
        }

        return "[\(text)](\(url))"
    }

    private func applyInlineFormatting(
        _ text: String,
        bold: Bool,
        italic: Bool,
        underline: Bool,
        strikethrough: Bool
    ) -> String {
        guard !text.isEmpty else { return text }

        var result = text
        if strikethrough {
            result = "~~\(result)~~"
        }
        if bold && italic {
            result = "***\(result)***"
        } else if bold {
            result = "**\(result)**"
        } else if italic {
            result = "*\(result)*"
        }
        if underline {
            result = "<u>\(result)</u>"
        }
        return result
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

    private func listPrefix(for paragraph: OOXMLNode, numbering: [String: Bool]) -> String? {
        guard let numPr = paragraph.firstElement(named: "numPr"),
              let numId = numPr.firstElement(named: "numId")?.attribute("val"),
              let ilvl = numPr.firstElement(named: "ilvl")?.attribute("val"),
              let level = Int(ilvl) else {
            return nil
        }

        let indent = String(repeating: "    ", count: level)
        let isNumbered = numbering[numId] ?? false
        return isNumbered ? "\(indent)1. " : "\(indent)- "
    }

    // MARK: - Table rendering

    private func renderTable(_ table: OOXMLNode, relationships: [String: String]) -> String? {
        let rows = table.children.filter { $0.name == "tr" }
        guard !rows.isEmpty else { return nil }

        var renderedRows: [[String]] = []
        for row in rows {
            let cells = row.children.filter { $0.name == "tc" }
            let cellTexts = cells.map { cell in
                renderCell(cell, relationships: relationships)
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

    private func renderCell(_ cell: OOXMLNode, relationships: [String: String]) -> String {
        let paragraphs = cell.children
            .filter { $0.name == "p" }
            .compactMap { renderParagraph($0, relationships: relationships, numbering: [:]) }
        return paragraphs.joined(separator: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
