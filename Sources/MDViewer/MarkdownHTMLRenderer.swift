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
  padding: 28px;
}
.article {
  max-width: 940px;
  margin: 0 auto;
  background: var(--paper);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 38px 46px;
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
ul, ol {
  margin: 0.6em 0 1.1em 1.4em;
  padding-left: 1.1em;
}
li { margin: 0.35em 0; }
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
}
code {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, \"Liberation Mono\", monospace;
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
  width: 100%;
  margin: 1em 0;
}
th, td {
  border: 1px solid var(--border);
  padding: 8px 10px;
  text-align: left;
}
th {
  background: #f8fafc;
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
</head>
<body>
  <article class=\"article\">\(body)</article>
</body>
</html>
"""
    }

    private static func renderBody(_ markdown: String) -> String {
        let normalized = markdown.replacingOccurrences(of: "\r\n", with: "\n")
        let lines = normalized.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)

        var html: [String] = []
        var paragraphBuffer: [String] = []
        var codeBuffer: [String] = []
        var inCodeBlock = false
        var codeLanguage = ""
        var inUnorderedList = false
        var inOrderedList = false
        var inBlockquote = false

        func flushParagraph() {
            guard !paragraphBuffer.isEmpty else { return }
            let text = paragraphBuffer.map { $0.trimmingCharacters(in: .whitespaces) }.joined(separator: " ")
            html.append("<p>\(renderInline(text))</p>")
            paragraphBuffer.removeAll(keepingCapacity: true)
        }

        func closeLists() {
            if inUnorderedList {
                html.append("</ul>")
                inUnorderedList = false
            }
            if inOrderedList {
                html.append("</ol>")
                inOrderedList = false
            }
        }

        func closeBlockquote() {
            if inBlockquote {
                html.append("</blockquote>")
                inBlockquote = false
            }
        }

        for line in lines {
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            if trimmed.hasPrefix("```") {
                flushParagraph()
                closeLists()
                closeBlockquote()

                if inCodeBlock {
                    let langClass = codeLanguage.isEmpty ? "" : " class=\"language-\(htmlEscaped(codeLanguage))\""
                    let code = htmlEscaped(codeBuffer.joined(separator: "\n"))
                    html.append("<pre><code\(langClass)>\(code)</code></pre>")
                    codeBuffer.removeAll(keepingCapacity: true)
                    inCodeBlock = false
                    codeLanguage = ""
                } else {
                    inCodeBlock = true
                    codeLanguage = String(trimmed.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                }
                continue
            }

            if inCodeBlock {
                codeBuffer.append(line)
                continue
            }

            if trimmed.isEmpty {
                flushParagraph()
                closeLists()
                closeBlockquote()
                continue
            }

            if let heading = headingLevelAndText(trimmed) {
                flushParagraph()
                closeLists()
                closeBlockquote()
                html.append("<h\(heading.level)>\(renderInline(heading.text))</h\(heading.level)>")
                continue
            }

            if isHorizontalRule(trimmed) {
                flushParagraph()
                closeLists()
                closeBlockquote()
                html.append("<hr />")
                continue
            }

            if let quoteText = blockquoteText(trimmed) {
                flushParagraph()
                closeLists()
                if !inBlockquote {
                    html.append("<blockquote>")
                    inBlockquote = true
                }
                html.append("<p>\(renderInline(quoteText))</p>")
                continue
            }

            closeBlockquote()

            if let bullet = unorderedItemText(trimmed) {
                flushParagraph()
                if inOrderedList {
                    html.append("</ol>")
                    inOrderedList = false
                }
                if !inUnorderedList {
                    html.append("<ul>")
                    inUnorderedList = true
                }
                html.append("<li>\(renderInline(bullet))</li>")
                continue
            }

            if let ordered = orderedItemText(trimmed) {
                flushParagraph()
                if inUnorderedList {
                    html.append("</ul>")
                    inUnorderedList = false
                }
                if !inOrderedList {
                    html.append("<ol>")
                    inOrderedList = true
                }
                html.append("<li>\(renderInline(ordered))</li>")
                continue
            }

            closeLists()
            paragraphBuffer.append(line)
        }

        if inCodeBlock {
            let code = htmlEscaped(codeBuffer.joined(separator: "\n"))
            html.append("<pre><code>\(code)</code></pre>")
        }

        flushParagraph()
        closeLists()
        closeBlockquote()

        return html.joined(separator: "\n")
    }

    private static func headingLevelAndText(_ line: String) -> (level: Int, text: String)? {
        let hashes = line.prefix { $0 == "#" }
        guard !hashes.isEmpty, hashes.count <= 6 else { return nil }
        let remainder = line.dropFirst(hashes.count)
        guard remainder.first == " " else { return nil }
        return (hashes.count, String(remainder.dropFirst()))
    }

    private static func isHorizontalRule(_ line: String) -> Bool {
        let compact = line.replacingOccurrences(of: " ", with: "")
        return compact == "---" || compact == "***" || compact == "___"
    }

    private static func blockquoteText(_ line: String) -> String? {
        guard line.hasPrefix(">") else { return nil }
        return String(line.dropFirst()).trimmingCharacters(in: .whitespaces)
    }

    private static func unorderedItemText(_ line: String) -> String? {
        for prefix in ["- ", "* ", "+ "] {
            if line.hasPrefix(prefix) {
                return String(line.dropFirst(prefix.count))
            }
        }
        return nil
    }

    private static func orderedItemText(_ line: String) -> String? {
        let pattern = #"^\d+\.\s+(.+)$"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }
        let range = NSRange(line.startIndex..<line.endIndex, in: line)
        guard let match = regex.firstMatch(in: line, range: range), match.numberOfRanges > 1,
              let textRange = Range(match.range(at: 1), in: line) else {
            return nil
        }
        return String(line[textRange])
    }

    private static func renderInline(_ text: String) -> String {
        let escaped = htmlEscaped(text)
        let segments = escaped.split(separator: "`", omittingEmptySubsequences: false)

        var rendered = ""
        for (index, rawPart) in segments.enumerated() {
            let part = String(rawPart)
            if index.isMultiple(of: 2) {
                rendered += applyInlineRegex(part)
            } else {
                rendered += "<code>\(part)</code>"
            }
        }
        return rendered
    }

    private static func applyInlineRegex(_ text: String) -> String {
        var output = text

        output = replacing(
            pattern: #"\[([^\]]+)\]\(([^\)]+)\)"#,
            in: output,
            template: "<a href=\"$2\">$1</a>"
        )
        output = replacing(pattern: #"\*\*([^*]+)\*\*"#, in: output, template: "<strong>$1</strong>")
        output = replacing(pattern: #"__([^_]+)__"#, in: output, template: "<strong>$1</strong>")
        output = replacing(pattern: #"~~([^~]+)~~"#, in: output, template: "<del>$1</del>")
        output = replacing(pattern: #"\*([^*]+)\*"#, in: output, template: "<em>$1</em>")
        output = replacing(pattern: #"_([^_]+)_"#, in: output, template: "<em>$1</em>")

        return output
    }

    private static func replacing(pattern: String, in text: String, template: String) -> String {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: []) else {
            return text
        }
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        return regex.stringByReplacingMatches(in: text, options: [], range: range, withTemplate: template)
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
}
