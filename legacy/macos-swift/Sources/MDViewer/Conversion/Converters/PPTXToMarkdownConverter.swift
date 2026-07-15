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
            let number = slideNumber(slideURL)
            if let slideMarkdown = renderSlide(slide, number: number) {
                parts.append(slideMarkdown)
            }
        }

        let markdown = parts.joined(separator: "\n\n")

        return MarkdownConversionResult(
            markdown: markdown.isEmpty ? "_Presentación PPTX vacía_" : markdown,
            sourceFormat: "PPTX",
            title: nil,
            warnings: []
        )
    }

    private func renderSlide(_ slide: OOXMLNode, number: Int) -> String? {
        guard let spTree = slide.firstElement(named: "spTree") else {
            return nil
        }

        var title: String?
        var bodyParts: [String] = []

        for shape in spTree.children {
            switch shape.name {
            case "sp":
                if let shapeText = renderShape(shape, isTitle: &title) {
                    bodyParts.append(shapeText)
                }
            case "graphicFrame":
                if let tableText = renderGraphicFrame(shape) {
                    bodyParts.append(tableText)
                }
            default:
                break
            }
        }

        let header = "## Diapositiva \(number)"
        let titleHeader = title.map { "# \($0)" }
        let body = bodyParts.joined(separator: "\n\n")

        var slideParts: [String] = [header]
        if let titleHeader = titleHeader {
            slideParts.append(titleHeader)
        }
        if !body.isEmpty {
            slideParts.append(body)
        }

        return slideParts.joined(separator: "\n\n")
    }

    private func renderShape(_ shape: OOXMLNode, isTitle: inout String?) -> String? {
        let isShapeTitle = shapeIsTitle(shape)
        guard let txBody = shape.firstElement(named: "txBody") else { return nil }

        let paragraphs = txBody.children
            .filter { $0.name == "p" }
            .map { renderParagraph($0) }
            .filter { !$0.isEmpty }

        guard !paragraphs.isEmpty else { return nil }

        let text = paragraphs.joined(separator: "\n")

        if isShapeTitle {
            isTitle = text
            return nil
        }

        return text
    }

    private func shapeIsTitle(_ shape: OOXMLNode) -> Bool {
        guard let nvSpPr = shape.firstElement(named: "nvSpPr") else { return false }
        guard let nvPr = nvSpPr.firstElement(named: "nvPr") else { return false }
        guard let placeholder = nvPr.firstElement(named: "ph") else { return false }
        let type = placeholder.attribute("type")?.lowercased() ?? ""
        return type == "title" || type == "ctrTitle"
    }

    private func renderParagraph(_ paragraph: OOXMLNode) -> String {
        var text = ""
        for child in paragraph.children {
            switch child.name {
            case "r":
                text += renderRun(child)
            case "br":
                text += "\n"
            default:
                break
            }
        }
        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func renderRun(_ run: OOXMLNode) -> String {
        let rPr = run.firstElement(named: "rPr")
        let isBold = rPr?.firstElement(named: "b") != nil
        let isItalic = rPr?.firstElement(named: "i") != nil

        var text = ""
        for child in run.children {
            if child.name == "t" {
                text += child.allText()
            } else if child.name == "br" {
                text += "\n"
            }
        }

        guard !text.isEmpty else { return text }

        if isBold && isItalic {
            return "***\(text)***"
        } else if isBold {
            return "**\(text)**"
        } else if isItalic {
            return "*\(text)*"
        }
        return text
    }

    private func renderGraphicFrame(_ frame: OOXMLNode) -> String? {
        guard let tbl = frame.firstElement(named: "tbl") else { return nil }

        let rows = tbl.children.filter { $0.name == "tr" }
        guard !rows.isEmpty else { return nil }

        var renderedRows: [[String]] = []
        for row in rows {
            let cells = row.children.filter { $0.name == "tc" }
            let cellTexts = cells.map { renderTableCell($0) }
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

    private func renderTableCell(_ cell: OOXMLNode) -> String {
        guard let txBody = cell.firstElement(named: "txBody") else { return "" }
        let paragraphs = txBody.children
            .filter { $0.name == "p" }
            .map { renderParagraph($0) }
            .filter { !$0.isEmpty }
        return paragraphs.joined(separator: " ")
    }

    private func slideNumber(_ url: URL) -> Int {
        let name = url.deletingPathExtension().lastPathComponent
        let digits = name.drop(while: { !$0.isNumber })
        return Int(digits) ?? 0
    }
}
