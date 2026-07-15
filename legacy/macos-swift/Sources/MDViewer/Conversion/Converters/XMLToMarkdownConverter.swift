import Foundation

// LIMITATIONS:
// - Mixed content (text + child elements) is simplified: text directly before
//   a child element may be dropped. This is acceptable for the MVP.
struct XMLToMarkdownConverter: DocumentConverter {
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
    private var rootNodes: [XMLNode] = []
    private var nodeStack: [XMLNode] = []

    var markdown: String {
        render(nodes: rootNodes, depth: 0).joined(separator: "\n")
    }

    func parser(_ parser: XMLParser,
                didStartElement elementName: String,
                namespaceURI: String?,
                qualifiedName qName: String?,
                attributes attributeDict: [String: String] = [:]) {
        let node = XMLNode(name: elementName, attributes: attributeDict)
        if let parent = nodeStack.last {
            parent.children.append(node)
        } else {
            rootNodes.append(node)
        }
        nodeStack.append(node)
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        nodeStack.last?.text += string
    }

    func parser(_ parser: XMLParser, foundCDATA CDATABlock: Data) {
        if let string = String(data: CDATABlock, encoding: .utf8) {
            nodeStack.last?.text += string
        }
    }

    func parser(_ parser: XMLParser,
                didEndElement elementName: String,
                namespaceURI: String?,
                qualifiedName qName: String?) {
        nodeStack.removeLast()
    }

    private func render(nodes: [XMLNode], depth: Int) -> [String] {
        var output: [String] = []
        for node in nodes {
            let indent = String(repeating: "  ", count: depth)
            let attrs = node.attributes.map { "**\($0.key)**: `\($0.value)`" }.joined(separator: ", ")
            let trimmed = node.text.trimmingCharacters(in: .whitespacesAndNewlines)

            var line = "\(indent)- **\(node.name)**"
            if !attrs.isEmpty {
                line += " (\(attrs))"
            }
            if !trimmed.isEmpty {
                line += ": \(trimmed)"
            }
            output.append(line)
            output += render(nodes: node.children, depth: depth + 1)
        }
        return output
    }
}

private final class XMLNode {
    let name: String
    let attributes: [String: String]
    var text: String = ""
    var children: [XMLNode] = []

    init(name: String, attributes: [String: String]) {
        self.name = name
        self.attributes = attributes
    }
}
