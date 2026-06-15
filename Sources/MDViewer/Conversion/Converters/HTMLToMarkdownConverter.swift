import Foundation

// LIMITATIONS:
// - HTML entities are not decoded.
// - <script>, <style>, <head>, and <title> content is included without special handling.
// - Ordered lists are rendered as bullets.
// - Nested lists, images, tables, blockquotes, <pre>, and <code> blocks are not specially handled.
// - Malformed HTML may produce poor output.
struct HTMLToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["html", "htm"]

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

        let parser = SimpleHTMLParser()
        let markdown = parser.parse(content)

        return MarkdownConversionResult(
            markdown: markdown,
            sourceFormat: "HTML",
            title: nil,
            warnings: ["La estructura HTML se convirtio a Markdown plano; estilos y layouts no se conservaron."]
        )
    }
}

private struct SimpleHTMLParser {
    func parse(_ html: String) -> String {
        var text = html

        text = text.replacingOccurrences(of: "<br\\s*/?>", with: "\n", options: .regularExpression, range: nil)

        text = replaceMatches(pattern: "<a\\s+[^>]*href=\"([^\"]*)\"[^>]*>(.*?)</a>", in: text) { match, groups in
            guard groups.count >= 2 else { return match }
            let url = groups[0]
            let linkText = stripTags(groups[1])
            return "[\(linkText)](\(url))"
        }

        text = replaceMatches(pattern: "<(strong|b)[^>]*>(.*?)</(strong|b)>", in: text) { match, groups in
            guard groups.count >= 2 else { return match }
            return "**\(stripTags(groups[1]))**"
        }

        text = replaceMatches(pattern: "<(em|i)[^>]*>(.*?)</(em|i)>", in: text) { match, groups in
            guard groups.count >= 2 else { return match }
            return "_\(stripTags(groups[1]))_"
        }

        let headingReplacements: [(pattern: String, prefix: String)] = [
            ("<h1[^>]*>(.*?)</h1>", "# "),
            ("<h2[^>]*>(.*?)</h2>", "## "),
            ("<h3[^>]*>(.*?)</h3>", "### "),
            ("<h4[^>]*>(.*?)</h4>", "#### "),
            ("<h5[^>]*>(.*?)</h5>", "##### "),
            ("<h6[^>]*>(.*?)</h6>", "###### ")
        ]

        for (pattern, prefix) in headingReplacements {
            text = replaceMatches(pattern: pattern, in: text) { content in
                "\n\(prefix)\(stripTags(content))\n"
            }
        }

        text = replaceMatches(pattern: "<p[^>]*>(.*?)</p>", in: text) { content in
            "\n\(stripTags(content))\n"
        }

        text = replaceMatches(pattern: "<li[^>]*>(.*?)</li>", in: text) { content in
            "- \(stripTags(content))"
        }

        text = text.replacingOccurrences(of: "<ul[^>]*>", with: "", options: .regularExpression, range: nil)
        text = text.replacingOccurrences(of: "</ul>", with: "", options: .regularExpression, range: nil)
        text = text.replacingOccurrences(of: "<ol[^>]*>", with: "", options: .regularExpression, range: nil)
        text = text.replacingOccurrences(of: "</ol>", with: "", options: .regularExpression, range: nil)

        text = stripTags(text)
        text = collapseWhitespace(text)

        return text.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func replaceMatches(pattern: String, in text: String, transform: (String) -> String) -> String {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive, .dotMatchesLineSeparators]) else {
            return text
        }

        let range = NSRange(text.startIndex..., in: text)
        let matches = regex.matches(in: text, options: [], range: range)

        var result = text
        for match in matches.reversed() {
            guard let contentRange = Range(match.range(at: 1), in: result) else { continue }
            let content = String(result[contentRange])
            let replacement = transform(content)
            if let fullRange = Range(match.range, in: result) {
                result.replaceSubrange(fullRange, with: replacement)
            }
        }

        return result
    }

    private func replaceMatches(pattern: String, in text: String, transform: (String, [String]) -> String) -> String {
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive, .dotMatchesLineSeparators]) else {
            return text
        }

        let range = NSRange(text.startIndex..., in: text)
        let matches = regex.matches(in: text, options: [], range: range)

        var result = text
        for match in matches.reversed() {
            var groups: [String] = []
            for index in 1 ..< match.numberOfRanges {
                guard let groupRange = Range(match.range(at: index), in: result) else {
                    groups.append("")
                    continue
                }
                groups.append(String(result[groupRange]))
            }

            let fullMatch = String(result[Range(match.range, in: result)!])
            let replacement = transform(fullMatch, groups)
            if let fullRange = Range(match.range, in: result) {
                result.replaceSubrange(fullRange, with: replacement)
            }
        }

        return result
    }

    private func stripTags(_ text: String) -> String {
        text.replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression, range: nil)
    }

    private func collapseWhitespace(_ text: String) -> String {
        var result = text
        let patterns = [
            "\\n\\s*\\n\\s*\\n": "\n\n",
            "[ \t]+": " "
        ]
        for (pattern, replacement) in patterns {
            result = result.replacingOccurrences(of: pattern, with: replacement, options: .regularExpression, range: nil)
        }
        return result
    }
}
