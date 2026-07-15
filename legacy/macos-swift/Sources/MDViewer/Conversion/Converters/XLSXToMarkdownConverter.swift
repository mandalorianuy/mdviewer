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

        let workbookURL = tempDir.appendingPathComponent("xl/workbook.xml")
        let workbookRelsURL = tempDir.appendingPathComponent("xl/_rels/workbook.xml.rels")
        let sheetInfos = parseWorkbook(at: workbookURL, relationshipsAt: workbookRelsURL)

        let worksheetsDir = tempDir.appendingPathComponent("xl/worksheets")
        let worksheetURLs = try listFiles(at: worksheetsDir)
            .filter { $0.lastPathComponent.hasPrefix("sheet") && $0.pathExtension.lowercased() == "xml" }
            .sorted { sheetNumber($0) < sheetNumber($1) }

        guard !worksheetURLs.isEmpty else {
            throw ConversionError.conversionFailed(reason: "No se encontraron hojas en el XLSX.")
        }

        var parts: [String] = []
        if sheetInfos.isEmpty {
            for sheetURL in worksheetURLs {
                let number = sheetNumber(sheetURL)
                let sheetMarkdown = try renderSheet(at: sheetURL, sharedStrings: sharedStrings)
                if !sheetMarkdown.isEmpty {
                    if worksheetURLs.count > 1 {
                        parts.append("## Hoja \(number)\n\n\(sheetMarkdown)")
                    } else {
                        parts.append(sheetMarkdown)
                    }
                }
            }
        } else {
            for sheetInfo in sheetInfos {
                guard let sheetURL = worksheetURLs.first(where: {
                    $0.lastPathComponent.lowercased() == sheetInfo.target.lastPathComponent.lowercased()
                }) else {
                    continue
                }

                let sheetMarkdown = try renderSheet(at: sheetURL, sharedStrings: sharedStrings)
                if !sheetMarkdown.isEmpty {
                    parts.append("## \(sheetInfo.name)\n\n\(sheetMarkdown)")
                }
            }
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

    // MARK: - Workbook

    private struct SheetInfo {
        let name: String
        let target: URL
    }

    private func parseWorkbook(at url: URL, relationshipsAt relsURL: URL) -> [SheetInfo] {
        let relationships = parseRelationships(at: relsURL)

        guard FileManager.default.isReadableFile(atPath: url.path),
              let root = try? parseOOXMLDocument(at: url) else {
            return []
        }

        guard let sheets = root.firstElement(named: "sheets") else {
            return []
        }

        var infos: [SheetInfo] = []
        for sheet in sheets.children where sheet.name == "sheet" {
            guard let name = sheet.attribute("name"),
                  let relationshipId = sheet.attribute("id"),
                  let target = relationships[relationshipId] else {
                continue
            }
            let fullTarget = target.hasPrefix("/")
                ? String(target.dropFirst())
                : target
            infos.append(SheetInfo(name: name, target: URL(fileURLWithPath: fullTarget)))
        }
        return infos
    }

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

    // MARK: - Sheet rendering

    private func renderSheet(at url: URL, sharedStrings: [String]) throws -> String {
        let root = try parseOOXMLDocument(at: url)
        guard let sheetData = root.firstElement(named: "sheetData") else {
            return ""
        }

        let mergeCells = parseMergeCells(from: root)

        let rows = sheetData.children.filter { $0.name == "row" }
            .sorted { ($0.attribute("r").flatMap(Int.init) ?? 0) < ($1.attribute("r").flatMap(Int.init) ?? 0) }

        var renderedRows: [[String]] = []
        for row in rows {
            let rowNumber = row.attribute("r").flatMap(Int.init) ?? 0
            let cells = row.children.filter { $0.name == "c" }
                .sorted { cellColumnNumber($0) < cellColumnNumber($1) }

            var columnValues: [Int: String] = [:]
            for cell in cells {
                let column = cellColumnNumber(cell)
                columnValues[column] = cellValue(cell, sharedStrings: sharedStrings)
            }

            applyMergeCells(mergeCells, to: &columnValues, rowNumber: rowNumber)

            let maxColumn = max(
                columnValues.keys.max() ?? 0,
                mergeCells.filter { $0.topRow == rowNumber }.map { $0.rightColumn }.max() ?? 0
            )
            let columnCount = max(maxColumn, cells.isEmpty ? 0 : cells.map { cellColumnNumber($0) }.max() ?? 0)

            var rowValues: [String] = []
            for column in 1...columnCount {
                rowValues.append(columnValues[column] ?? "")
            }
            renderedRows.append(rowValues)
        }

        guard let firstRow = renderedRows.first, !firstRow.isEmpty else {
            return ""
        }

        let maxColumns = renderedRows.map { $0.count }.max() ?? 0
        let paddedRows = renderedRows.map { row -> [String] in
            if row.count < maxColumns {
                return row + Array(repeating: "", count: maxColumns - row.count)
            }
            return row
        }

        var lines: [String] = []
        lines.append("| " + paddedRows[0].joined(separator: " | ") + " |")
        lines.append("| " + paddedRows[0].map { _ in "---" }.joined(separator: " | ") + " |")

        for row in paddedRows.dropFirst() {
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

    private func cellColumnNumber(_ cell: OOXMLNode) -> Int {
        columnNumber(cellColumn(cell))
    }

    private func columnNumber(_ letters: String) -> Int {
        var result = 0
        for letter in letters.uppercased() {
            guard let scalar = letter.unicodeScalars.first,
                  scalar.value >= Unicode.Scalar("A").value,
                  scalar.value <= Unicode.Scalar("Z").value else {
                continue
            }
            result = result * 26 + Int(scalar.value - Unicode.Scalar("A").value + 1)
        }
        return result
    }

    private struct MergeCell {
        let topRow: Int
        let leftColumn: Int
        let bottomRow: Int
        let rightColumn: Int
    }

    private func parseMergeCells(from root: OOXMLNode) -> [MergeCell] {
        guard let mergeCells = root.firstElement(named: "mergeCells") else {
            return []
        }

        var merges: [MergeCell] = []
        for mergeCell in mergeCells.children where mergeCell.name == "mergeCell" {
            guard let ref = mergeCell.attribute("ref") else { continue }
            let parts = ref.split(separator: ":").map(String.init)
            guard let topLeft = parts.first else { continue }
            let bottomRight = parts.count > 1 ? parts[1] : topLeft

            guard let topLeftCell = parseCellReference(topLeft),
                  let bottomRightCell = parseCellReference(bottomRight) else {
                continue
            }
            merges.append(MergeCell(
                topRow: topLeftCell.row,
                leftColumn: topLeftCell.column,
                bottomRow: bottomRightCell.row,
                rightColumn: bottomRightCell.column
            ))
        }
        return merges
    }

    private func parseCellReference(_ reference: String) -> (row: Int, column: Int)? {
        let letters = reference.prefix(while: { !$0.isNumber })
        let digits = reference.drop(while: { !$0.isNumber })
        guard let row = Int(digits), !letters.isEmpty else { return nil }
        let column = columnNumber(String(letters))
        guard column > 0 else { return nil }
        return (row: row, column: column)
    }

    private func sheetNumber(_ url: URL) -> Int {
        let name = url.deletingPathExtension().lastPathComponent
        let digits = name.drop(while: { !$0.isNumber })
        return Int(digits) ?? 0
    }

    private func applyMergeCells(
        _ mergeCells: [MergeCell],
        to columnValues: inout [Int: String],
        rowNumber: Int
    ) {
        for merge in mergeCells {
            guard rowNumber >= merge.topRow && rowNumber <= merge.bottomRow else { continue }

            if rowNumber == merge.topRow {
                let value = columnValues[merge.leftColumn] ?? ""
                for column in merge.leftColumn...merge.rightColumn {
                    columnValues[column] = (column == merge.leftColumn) ? value : ""
                }
            } else {
                for column in merge.leftColumn...merge.rightColumn {
                    columnValues[column] = ""
                }
            }
        }
    }
}
