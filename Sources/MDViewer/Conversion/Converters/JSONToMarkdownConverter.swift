import Foundation

struct JSONToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["json"]

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

        let jsonObject: Any
        do {
            jsonObject = try JSONSerialization.jsonObject(with: data, options: [])
        } catch {
            throw ConversionError.conversionFailed(reason: error.localizedDescription)
        }

        let markdown = render(jsonObject, level: 0)

        return MarkdownConversionResult(
            markdown: markdown,
            sourceFormat: "JSON",
            title: nil,
            warnings: []
        )
    }

    private func render(_ object: Any, level: Int) -> String {
        let indent = String(repeating: "  ", count: level)

        if let dict = object as? [String: Any] {
            return dict.map { key, value in
                let valueString: String
                if let nestedDict = value as? [String: Any], !nestedDict.isEmpty {
                    valueString = "\n" + render(value, level: level + 1)
                } else if let nestedArray = value as? [Any], !nestedArray.isEmpty {
                    valueString = "\n" + render(value, level: level + 1)
                } else {
                    valueString = " \(renderScalar(value))"
                }
                return "\(indent)- **\(escapeKey(key))**:\(valueString)"
            }.joined(separator: "\n")
        }

        if let array = object as? [Any] {
            return array.enumerated().map { _, value in
                "\(indent)1. \(renderScalar(value))"
            }.joined(separator: "\n")
        }

        return renderScalar(object)
    }

    private func renderScalar(_ value: Any) -> String {
        if value is NSNull {
            return "`null`"
        }
        if let bool = value as? Bool {
            return "`\(bool)`"
        }
        if let number = value as? NSNumber {
            return "`\(number)`"
        }
        if let string = value as? String {
            return escapeValue(string)
        }
        return "`\(value)`"
    }

    private func escapeKey(_ key: String) -> String {
        key.replacingOccurrences(of: "*", with: "\\*")
    }

    private func escapeValue(_ value: String) -> String {
        value
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "*", with: "\\*")
            .replacingOccurrences(of: "_", with: "\\_")
            .replacingOccurrences(of: "`", with: "\\`")
    }
}
