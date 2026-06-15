import Foundation

final class XMLToMarkdownConverter: NSObject, DocumentConverter {
    let supportedExtensions: [String] = ["xml"]

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            throw ConversionError.fileNotReadable
        }

        let data: Data
        do {
            data = try Data(contentsOf: url)
        } catch {
            throw ConversionError.conversionFailed(reason: error.localizedDescription)
        }

        let parser = XMLParser(data: data)
        let delegate = XMLParserDelegateHandler()
        parser.delegate = delegate

        guard parser.parse() else {
            let underlying = parser.parserError ?? NSError(domain: "XML", code: 0, userInfo: nil)
            throw ConversionError.conversionFailed(reason: underlying.localizedDescription)
        }

        let markdown = delegate.markdown

        return MarkdownConversionResult(
            markdown: markdown.isEmpty ? "_Documento XML vacio_" : markdown,
            sourceFormat: "XML",
            title: nil,
            warnings: []
        )
    }
}

private final class XMLParserDelegateHandler: NSObject, XMLParserDelegate {
    private var output: [String] = []
    private var currentDepth = 0
    private var currentElementAttributes: [String: String]?
    private var currentText = ""

    var markdown: String {
        output.joined(separator: "\n")
    }

    func parser(_ parser: XMLParser,
                didStartElement elementName: String,
                namespaceURI: String?,
                qualifiedName qName: String?,
                attributes attributeDict: [String: String] = [:]) {
        currentElementAttributes = attributeDict
        currentText = ""
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        currentText += string
    }

    func parser(_ parser: XMLParser,
                didEndElement elementName: String,
                namespaceURI: String?,
                qualifiedName qName: String?) {
        let trimmed = currentText.trimmingCharacters(in: .whitespacesAndNewlines)
        let indent = String(repeating: "  ", count: currentDepth)
        let attrs = currentElementAttributes?.map { "**\($0.key)**: `\($0.value)`" }.joined(separator: ", ")

        if !trimmed.isEmpty {
            if let attrs = attrs, !attrs.isEmpty {
                output.append("\(indent)- **\(elementName)** (\(attrs)): \(trimmed)")
            } else {
                output.append("\(indent)- **\(elementName)**: \(trimmed)")
            }
        } else if let attrs = attrs, !attrs.isEmpty {
            output.append("\(indent)- **\(elementName)** (\(attrs))")
        }

        currentDepth += 1
    }
}
