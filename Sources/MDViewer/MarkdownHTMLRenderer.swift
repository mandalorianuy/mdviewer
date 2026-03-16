import Down
import Foundation

enum MarkdownHTMLRenderer {
    static func renderDocument(markdown: String, fontFamily: String, baseFontSize: Double) -> String {
        let body = renderBody(markdown)
        let safeFontFamily = cssEscaped(fontFamily)
        let bodySize = max(12, min(baseFontSize, 28))
        let h1Size = bodySize * 2.0
        let h2Size = bodySize * 1.6
        let h3Size = bodySize * 1.3

        return """
<!doctype html>
<html lang=\"es\">
<head>
<meta charset=\"utf-8\" />
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
<style>
:root {
  --bg: #eceff3;
  --paper: #ffffff;
  --text: #1f2937;
  --muted: #6b7280;
  --border: #e5e7eb;
  --accent: #2563eb;
  --code-bg: #0f172a;
  --code-text: #e2e8f0;
  --inline-code-bg: #eef2ff;
  --inline-code-text: #1e3a8a;
  --font-size: \(bodySize)px;
  --h1: \(h1Size)px;
  --h2: \(h2Size)px;
  --h3: \(h3Size)px;
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; }
body {
  background: radial-gradient(circle at 15% 15%, #f8fafc, var(--bg));
  color: var(--text);
  font-family: \"\(safeFontFamily)\", -apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif;
  font-size: var(--font-size);
  line-height: 1.7;
  padding: 16px;
}
.article {
  width: 100%;
  max-width: none;
  margin: 0;
  background: var(--paper);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 30px 32px;
  box-shadow: 0 18px 40px rgba(15, 23, 42, 0.08);
}
h1, h2, h3, h4, h5, h6 {
  line-height: 1.25;
  margin-top: 1.5em;
  margin-bottom: 0.6em;
  letter-spacing: -0.01em;
}
h1 { font-size: var(--h1); margin-top: 0.2em; }
h2 { font-size: var(--h2); }
h3 { font-size: var(--h3); }
p {
  margin: 0.85em 0;
}
strong {
  font-weight: 700;
}
em {
  font-style: italic;
}
del {
  color: #64748b;
  text-decoration-thickness: 2px;
}
ul, ol {
  margin: 0.6em 0 1.1em 1.4em;
  padding-left: 1.1em;
}
li { margin: 0.35em 0; }
li > p:first-child {
  margin-top: 0;
}
li > p:last-child {
  margin-bottom: 0;
}
ul.contains-task-list,
ul.task-list {
  list-style: none;
  margin-left: 0;
  padding-left: 0.2em;
}
li.task-list-item {
  display: flex;
  align-items: flex-start;
  gap: 0.55em;
}
li.task-list-item input[type="checkbox"] {
  margin-top: 0.35em;
  flex: 0 0 auto;
}
blockquote {
  margin: 1.1em 0;
  padding: 0.7em 1em;
  border-left: 4px solid var(--accent);
  color: #334155;
  background: #f8fafc;
  border-radius: 8px;
}
blockquote p { margin: 0.4em 0; }
pre {
  margin: 1.1em 0;
  background: var(--code-bg);
  color: var(--code-text);
  border-radius: 10px;
  padding: 14px 16px;
  overflow-x: auto;
  font-size: 0.92em;
  line-height: 1.55;
  border: 1px solid rgba(148, 163, 184, 0.18);
}
code {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \"Liberation Mono\", monospace;
}
pre code {
  background: transparent;
  color: inherit;
  border: none;
  padding: 0;
  font-size: 1em;
  white-space: pre;
}
p code, li code, blockquote code {
  background: var(--inline-code-bg);
  color: var(--inline-code-text);
  border: 1px solid #dbeafe;
  border-radius: 6px;
  padding: 0.08em 0.4em;
  font-size: 0.92em;
}
a {
  color: var(--accent);
  text-decoration: none;
  border-bottom: 1px dashed color-mix(in oklab, var(--accent) 45%, transparent);
}
a:hover {
  border-bottom-style: solid;
}
hr {
  border: none;
  border-top: 1px solid var(--border);
  margin: 1.6em 0;
}
table {
  border-collapse: collapse;
  width: max-content;
  min-width: 100%;
  margin: 0;
  table-layout: auto;
  background: var(--paper);
}
th, td {
  border: 1px solid var(--border);
  padding: 10px 12px;
  text-align: left;
  vertical-align: top;
  word-break: normal;
  overflow-wrap: anywhere;
}
th {
  background: #f8fafc;
}
thead th {
  font-weight: 700;
}
tbody tr:nth-child(even) {
  background: #fbfdff;
}
td code, th code {
  background: var(--inline-code-bg);
  color: var(--inline-code-text);
  border: 1px solid #dbeafe;
  border-radius: 6px;
  padding: 0.08em 0.4em;
  font-size: 0.92em;
  white-space: normal;
  word-break: break-word;
  overflow-wrap: anywhere;
}
.table-wrap {
  width: 100%;
  overflow-x: auto;
  margin: 1.2em 0;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: linear-gradient(180deg, #ffffff, #fcfdff);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.75);
}
.table-wrap table.resizable-table th,
.table-wrap table.resizable-table td {
  position: relative;
}
.table-wrap table.resizable-table th {
  user-select: none;
}
.column-resize-handle {
  position: absolute;
  top: 0;
  right: -5px;
  width: 10px;
  height: 100%;
  cursor: col-resize;
  z-index: 5;
}
.column-resize-handle::after {
  content: "";
  position: absolute;
  top: 22%;
  bottom: 22%;
  left: 50%;
  width: 2px;
  transform: translateX(-50%);
  border-radius: 999px;
  background: rgba(37, 99, 235, 0.18);
  transition: background 140ms ease;
}
.column-resize-handle:hover::after,
.column-resize-handle.is-active::after {
  background: rgba(37, 99, 235, 0.75);
}
input[type="checkbox"] {
  accent-color: var(--accent);
}
details {
  margin: 1em 0;
  padding: 0.85em 1em;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: #fafcff;
}
summary {
  cursor: pointer;
  font-weight: 600;
}
section.footnotes {
  margin-top: 2.4em;
  padding-top: 1.2em;
  border-top: 1px solid var(--border);
  color: #334155;
}
section.footnotes ol {
  margin-bottom: 0;
}
mark {
  background: #fef3c7;
  color: inherit;
  border-radius: 4px;
  padding: 0.05em 0.2em;
}
kbd {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.9em;
  background: #f8fafc;
  border: 1px solid #cbd5e1;
  border-bottom-width: 2px;
  border-radius: 6px;
  padding: 0.08em 0.38em;
}
img {
  max-width: 100%;
  border-radius: 8px;
}
@media (max-width: 860px) {
  body { padding: 12px; }
  .article {
    padding: 20px 16px;
    border-radius: 10px;
  }
}
</style>
<script>
function setupResizableTables() {
  const tables = Array.from(document.querySelectorAll('.table-wrap table'));

  tables.forEach((table, tableIndex) => {
    const wrapper = table.closest('.table-wrap');
    const headerRow = table.tHead && table.tHead.rows.length > 0 ? table.tHead.rows[0] : table.rows[0];
    if (!wrapper || !headerRow || headerRow.cells.length === 0) {
      return;
    }

    table.classList.add('resizable-table');

    if (!table.querySelector('colgroup')) {
      const colgroup = document.createElement('colgroup');
      const headerCells = Array.from(headerRow.cells);
      headerCells.forEach((cell, cellIndex) => {
        const col = document.createElement('col');
        col.dataset.columnIndex = String(cellIndex);
        const width = Math.max(120, Math.ceil(cell.getBoundingClientRect().width));
        col.style.width = `${width}px`;
        colgroup.appendChild(col);
      });
      table.insertBefore(colgroup, table.firstChild);
    }

    const columns = Array.from(table.querySelectorAll('colgroup col'));
    if (columns.length === 0) {
      return;
    }

    fitTableToWrapper(table, wrapper, columns);

    Array.from(headerRow.cells).forEach((cell, columnIndex) => {
      if (cell.querySelector('.column-resize-handle')) {
        return;
      }

      const handle = document.createElement('div');
      handle.className = 'column-resize-handle';
      handle.title = 'Arrastrar para ajustar columna';

      handle.addEventListener('mousedown', (event) => {
        event.preventDefault();

        const startX = event.clientX;
        const column = columns[columnIndex];
        const startWidth = column.getBoundingClientRect().width;
        const minWidth = 90;

        handle.classList.add('is-active');

        const onMouseMove = (moveEvent) => {
          const nextWidth = Math.max(minWidth, startWidth + (moveEvent.clientX - startX));
          column.dataset.userSized = 'true';
          column.style.width = `${nextWidth}px`;
          table.style.width = `${Math.max(wrapper.clientWidth, totalColumnWidth(columns))}px`;
        };

        const onMouseUp = () => {
          handle.classList.remove('is-active');
          window.removeEventListener('mousemove', onMouseMove);
          window.removeEventListener('mouseup', onMouseUp);
        };

        window.addEventListener('mousemove', onMouseMove);
        window.addEventListener('mouseup', onMouseUp);
      });

      cell.appendChild(handle);
    });

    table.dataset.tableIndex = String(tableIndex);
    window.addEventListener('resize', () => fitTableToWrapper(table, wrapper, columns));
  });
}

function totalColumnWidth(columns) {
  return columns.reduce((sum, column) => sum + Math.max(90, Math.ceil(column.getBoundingClientRect().width)), 0);
}

function fitTableToWrapper(table, wrapper, columns) {
  const wrapperWidth = Math.max(0, Math.floor(wrapper.clientWidth));
  let totalWidth = totalColumnWidth(columns);

  if (wrapperWidth > totalWidth) {
    const flexibleColumns = columns.filter((column) => column.dataset.userSized !== 'true');
    const targets = flexibleColumns.length > 0 ? flexibleColumns : columns;
    const extraWidth = wrapperWidth - totalWidth;
    const perColumn = Math.floor(extraWidth / targets.length);
    let remainder = extraWidth - (perColumn * targets.length);

    targets.forEach((column) => {
      const currentWidth = Math.max(90, Math.ceil(column.getBoundingClientRect().width));
      const growth = perColumn + (remainder > 0 ? 1 : 0);
      remainder = Math.max(0, remainder - 1);
      column.style.width = `${currentWidth + growth}px`;
    });

    totalWidth = totalColumnWidth(columns);
  }

  table.style.width = `${Math.max(wrapperWidth, totalWidth)}px`;
}

window.addEventListener('load', setupResizableTables);
</script>
</head>
<body>
  <article class=\"article\">\(body)</article>
</body>
</html>
"""
    }

    private static func renderBody(_ markdown: String) -> String {
        let preparedMarkdown = prepareMarkdown(markdown)

        do {
            let down = Down(markdownString: preparedMarkdown)
            let html = try down.toHTML(.unsafe)
            return html.replacingOccurrences(of: "<table>", with: "<div class=\"table-wrap\"><table>")
                .replacingOccurrences(of: "</table>", with: "</table></div>")
        } catch {
            return "<pre><code>\(htmlEscaped(preparedMarkdown))</code></pre>"
        }
    }

    private static func prepareMarkdown(_ markdown: String) -> String {
        let normalizedLineEndings = markdown.replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
        let repairedTables = repairCollapsedTables(in: normalizedLineEndings)
        return convertMarkdownTablesToHTML(in: repairedTables)
    }

    private static func repairCollapsedTables(in markdown: String) -> String {
        let lines = markdown.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var repairedLines: [String] = []

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            let looksLikeCollapsedTable = trimmed.hasPrefix("|") && trimmed.filter(\.isPipeCharacter).count >= 8 && trimmed.contains("||")

            guard looksLikeCollapsedTable else {
                repairedLines.append(line)
                continue
            }

            let segments = line
                .split(separator: "||", omittingEmptySubsequences: true)
                .map { normalizeCollapsedTableSegment(String($0)) }
                .filter { !$0.isEmpty }

            if segments.count >= 3, isMarkdownTableSeparator(segments[1]) {
                repairedLines.append(contentsOf: segments)
            } else {
                repairedLines.append(line)
            }
        }

        return repairedLines.joined(separator: "\n")
    }

    private static func normalizeCollapsedTableSegment(_ segment: String) -> String {
        var normalized = segment.trimmingCharacters(in: .whitespacesAndNewlines)

        if !normalized.hasPrefix("|") {
            normalized = "|" + normalized
        }

        if !normalized.hasSuffix("|") {
            normalized += "|"
        }

        return normalized
    }

    private static func isMarkdownTableSeparator(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("|"), trimmed.hasSuffix("|") else {
            return false
        }

        let cells = trimmed
            .dropFirst()
            .dropLast()
            .split(separator: "|", omittingEmptySubsequences: false)
            .map { $0.trimmingCharacters(in: .whitespaces) }

        guard !cells.isEmpty else {
            return false
        }

        return cells.allSatisfy { cell in
            !cell.isEmpty && cell.allSatisfy { $0 == "-" || $0 == ":" }
        }
    }

    private static func htmlEscaped(_ input: String) -> String {
        input
            .replacingOccurrences(of: "&", with: "&amp;")
            .replacingOccurrences(of: "<", with: "&lt;")
            .replacingOccurrences(of: ">", with: "&gt;")
            .replacingOccurrences(of: "\"", with: "&quot;")
            .replacingOccurrences(of: "'", with: "&#39;")
    }

    private static func cssEscaped(_ input: String) -> String {
        input
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
    }

    private static func convertMarkdownTablesToHTML(in markdown: String) -> String {
        let lines = markdown.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        var output: [String] = []
        var index = 0
        var activeFence: String?

        while index < lines.count {
            let line = lines[index]
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            if let fence = markdownFenceMarker(in: trimmed) {
                if activeFence == fence {
                    activeFence = nil
                } else if activeFence == nil {
                    activeFence = fence
                }
                output.append(line)
                index += 1
                continue
            }

            guard activeFence == nil,
                  index + 1 < lines.count,
                  isPotentialTableRow(line),
                  isMarkdownTableSeparator(lines[index + 1]) else {
                output.append(line)
                index += 1
                continue
            }

            var tableLines = [line, lines[index + 1]]
            index += 2

            while index < lines.count, isPotentialTableRow(lines[index]) {
                tableLines.append(lines[index])
                index += 1
            }

            output.append(renderTableHTML(from: tableLines))
        }

        return output.joined(separator: "\n")
    }

    private static func renderTableHTML(from lines: [String]) -> String {
        guard lines.count >= 2 else {
            return lines.joined(separator: "\n")
        }

        let headerCells = splitTableRow(lines[0])
        let alignments = splitTableRow(lines[1]).map(tableAlignment(for:))
        let bodyRows = lines.dropFirst(2).map(splitTableRow)

        guard headerCells.count >= 2, alignments.count == headerCells.count else {
            return lines.joined(separator: "\n")
        }

        func inlineHTML(for cell: String) -> String {
            let trimmed = cell.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else {
                return ""
            }

            do {
                let rendered = try Down(markdownString: trimmed).toHTML(.unsafe)
                return stripOuterParagraph(from: rendered)
            } catch {
                return htmlEscaped(trimmed)
            }
        }

        func styleAttribute(for alignment: TableAlignment) -> String {
            switch alignment {
            case .left:
                return " style=\"text-align: left;\""
            case .center:
                return " style=\"text-align: center;\""
            case .right:
                return " style=\"text-align: right;\""
            case .none:
                return ""
            }
        }

        let headerHTML = zip(headerCells, alignments).map { cell, alignment in
            "<th\(styleAttribute(for: alignment))>\(inlineHTML(for: cell))</th>"
        }.joined()

        let rowsHTML = bodyRows.map { row in
            let paddedRow = row.count < alignments.count ? row + Array(repeating: "", count: alignments.count - row.count) : Array(row.prefix(alignments.count))
            let cellsHTML = zip(paddedRow, alignments).map { cell, alignment in
                "<td\(styleAttribute(for: alignment))>\(inlineHTML(for: cell))</td>"
            }.joined()
            return "<tr>\(cellsHTML)</tr>"
        }.joined(separator: "\n")

        var sections = ["<table>", "<thead><tr>\(headerHTML)</tr></thead>"]
        if !rowsHTML.isEmpty {
            sections.append("<tbody>\n\(rowsHTML)\n</tbody>")
        }
        sections.append("</table>")

        return sections.joined(separator: "\n")
    }

    private static func splitTableRow(_ line: String) -> [String] {
        var trimmed = line.trimmingCharacters(in: .whitespaces)
        if trimmed.hasPrefix("|") {
            trimmed.removeFirst()
        }
        if trimmed.hasSuffix("|") {
            trimmed.removeLast()
        }

        var cells: [String] = []
        var current = ""
        var isEscaped = false

        for character in trimmed {
            if isEscaped {
                current.append(character)
                isEscaped = false
                continue
            }

            if character == "\\" {
                current.append(character)
                isEscaped = true
                continue
            }

            if character == "|" {
                cells.append(current.trimmingCharacters(in: .whitespaces))
                current = ""
            } else {
                current.append(character)
            }
        }

        cells.append(current.trimmingCharacters(in: .whitespaces))
        return cells
    }

    private static func isPotentialTableRow(_ line: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.contains("|"), !trimmed.isEmpty else {
            return false
        }

        let cells = splitTableRow(trimmed)
        return cells.count >= 2 && cells.contains { !$0.isEmpty }
    }

    private static func markdownFenceMarker(in line: String) -> String? {
        if line.hasPrefix("```") {
            return "```"
        }

        if line.hasPrefix("~~~") {
            return "~~~"
        }

        return nil
    }

    private static func stripOuterParagraph(from html: String) -> String {
        let trimmed = html.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("<p>"), trimmed.hasSuffix("</p>") else {
            return trimmed
        }

        return String(trimmed.dropFirst(3).dropLast(4))
    }

    private static func tableAlignment(for cell: String) -> TableAlignment {
        let trimmed = cell.trimmingCharacters(in: .whitespacesAndNewlines)
        let isValidSeparator = !trimmed.isEmpty && trimmed.allSatisfy { $0 == "-" || $0 == ":" }

        guard isValidSeparator else {
            return .none
        }

        let hasLeadingColon = trimmed.hasPrefix(":")
        let hasTrailingColon = trimmed.hasSuffix(":")

        switch (hasLeadingColon, hasTrailingColon) {
        case (true, true):
            return .center
        case (false, true):
            return .right
        case (true, false):
            return .left
        case (false, false):
            return .none
        }
    }
}

private extension Character {
    var isPipeCharacter: Bool {
        self == "|"
    }
}

private enum TableAlignment {
    case none
    case left
    case center
    case right
}
