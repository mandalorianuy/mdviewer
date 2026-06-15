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

        let scalars = Array(content.unicodeScalars)
        var index = 0

        while index < scalars.count {
            let scalar = scalars[index]

            if scalar == "\"" {
                if insideQuotes {
                    if index + 1 < scalars.count && scalars[index + 1] == "\"" {
                        currentField.append("\"")
                        index += 2
                        continue
                    } else {
                        insideQuotes = false
                        index += 1
                        continue
                    }
                } else if currentField.isEmpty {
                    insideQuotes = true
                    index += 1
                    continue
                } else {
                    currentField.append(String(scalar))
                    index += 1
                    continue
                }
            }

            if scalar == "\r" || scalar == "\n" {
                currentRow.append(currentField)
                if !currentRow.allSatisfy({ $0.isEmpty }) {
                    rows.append(currentRow)
                }
                currentRow = []
                currentField = ""
                if scalar == "\r" && index + 1 < scalars.count && scalars[index + 1] == "\n" {
                    index += 2
                } else {
                    index += 1
                }
                continue
            }

            if scalar == "," && !insideQuotes {
                currentRow.append(currentField)
                currentField = ""
                index += 1
                continue
            }

            currentField.append(String(scalar))
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
