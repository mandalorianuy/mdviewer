import Foundation

struct CSVToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["csv"]

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            throw ConversionError.fileNotReadable
        }

        let content: String
        do {
            content = try String(contentsOf: url, encoding: .utf8)
        } catch {
            throw ConversionError.conversionFailed(reason: error.localizedDescription)
        }

        let rows = parseCSV(content)
        guard !rows.isEmpty else {
            return MarkdownConversionResult(
                markdown: "",
                sourceFormat: "CSV",
                title: nil,
                warnings: ["El archivo CSV esta vacio."]
            )
        }

        var lines: [String] = []
        for (index, row) in rows.enumerated() {
            let escaped = row.map { escapeMarkdownTableCell($0) }
            lines.append("| " + escaped.joined(separator: " | ") + " |")
            if index == 0 {
                lines.append("| " + row.map { _ in "---" }.joined(separator: " | ") + " |")
            }
        }

        return MarkdownConversionResult(
            markdown: lines.joined(separator: "\n"),
            sourceFormat: "CSV",
            title: nil,
            warnings: []
        )
    }

    private func parseCSV(_ content: String) -> [[String]] {
        var rows: [[String]] = []
        var currentRow: [String] = []
        var currentField = ""
        var insideQuotes = false

        let characters = Array(content)
        var index = 0

        while index < characters.count {
            let char = characters[index]

            if char == "\"" {
                if insideQuotes && index + 1 < characters.count && characters[index + 1] == "\"" {
                    currentField.append("\"")
                    index += 1
                } else {
                    insideQuotes.toggle()
                }
            } else if char == "," && !insideQuotes {
                currentRow.append(currentField)
                currentField = ""
            } else if char == "\n" && !insideQuotes {
                currentRow.append(currentField)
                if !currentRow.allSatisfy({ $0.isEmpty }) {
                    rows.append(currentRow)
                }
                currentRow = []
                currentField = ""
            } else {
                currentField.append(char)
            }

            index += 1
        }

        currentRow.append(currentField)
        if !currentRow.allSatisfy({ $0.isEmpty }) {
            rows.append(currentRow)
        }

        return rows
    }

    private func escapeMarkdownTableCell(_ value: String) -> String {
        value
            .replacingOccurrences(of: "|", with: "\\|")
            .replacingOccurrences(of: "\n", with: "<br>")
    }
}
