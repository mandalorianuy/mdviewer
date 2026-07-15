import Foundation

// MARK: - OOXML unzip

func unzipOOXML(_ url: URL, into destination: URL) throws {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
    process.arguments = ["-o", "-q", url.path, "-d", destination.path]

    let stderrPipe = Pipe()
    process.standardError = stderrPipe

    let timeoutSeconds: TimeInterval = 30
    let timeoutLock = NSLock()
    var didTimeOut = false
    var extractionFinished = false

    let timer = DispatchSource.makeTimerSource(queue: DispatchQueue.global())
    timer.schedule(deadline: .now() + timeoutSeconds)
    timer.setEventHandler { [weak process] in
        timeoutLock.lock()
        defer { timeoutLock.unlock() }
        guard !extractionFinished else { return }
        didTimeOut = true
        process?.terminate()
    }

    do {
        timer.resume()
        try process.run()
        process.waitUntilExit()
        timeoutLock.lock(); extractionFinished = true; timeoutLock.unlock()
        timer.cancel()
    } catch {
        timeoutLock.lock(); extractionFinished = true; timeoutLock.unlock()
        timer.cancel()
        throw ConversionError.conversionFailed(reason: error.localizedDescription)
    }

    if didTimeOut && process.terminationReason == .uncaughtSignal {
        throw ConversionError.timeout
    }

    guard process.terminationStatus == 0 else {
        let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
        let stderr = String(data: stderrData, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let reason = stderr?.isEmpty == false ? stderr! : "unzip exited with status \(process.terminationStatus)"
        throw ConversionError.conversionFailed(reason: reason)
    }
}

// MARK: - File listing

func listFiles(at directory: URL) throws -> [URL] {
    let enumerator = FileManager.default.enumerator(
        at: directory,
        includingPropertiesForKeys: [.isDirectoryKey],
        options: [.skipsHiddenFiles]
    )

    var files: [URL] = []
    while let fileURL = enumerator?.nextObject() as? URL {
        guard let resourceValues = try? fileURL.resourceValues(forKeys: [.isDirectoryKey]),
              resourceValues.isDirectory == false else {
            continue
        }
        files.append(fileURL)
    }
    return files.sorted { $0.path < $1.path }
}

// MARK: - OOXML XML parsing

final class OOXMLNode {
    let name: String
    let attributes: [String: String]
    var text: String = ""
    var children: [OOXMLNode] = []
    weak var parent: OOXMLNode?

    init(name: String, attributes: [String: String]) {
        self.name = name
        self.attributes = attributes
    }

    func firstElement(named name: String) -> OOXMLNode? {
        if self.name == name { return self }
        for child in children {
            if let found = child.firstElement(named: name) { return found }
        }
        return nil
    }

    func descendants(named name: String) -> [OOXMLNode] {
        var result: [OOXMLNode] = []
        for child in children {
            if child.name == name { result.append(child) }
            result.append(contentsOf: child.descendants(named: name))
        }
        return result
    }

    func elements(named name: String) -> [OOXMLNode] {
        children.filter { $0.name == name }
    }

    func siblings() -> [OOXMLNode] {
        guard let parent = parent else { return [] }
        return parent.children.filter { $0 !== self }
    }

    func allText() -> String {
        var result = text
        for child in children {
            result += child.allText()
        }
        return result
    }

    func attribute(_ key: String) -> String? {
        attributes[key.lowercased()]
    }
}

func parseOOXMLDocument(at url: URL) throws -> OOXMLNode {
    let data: Data
    do {
        data = try Data(contentsOf: url)
    } catch {
        throw ConversionError.conversionFailed(reason: "No se pudo leer el XML: \(error.localizedDescription)")
    }

    let parser = XMLParser(data: data)
    parser.shouldProcessNamespaces = true
    let delegate = OOXMLParserDelegate()
    parser.delegate = delegate

    guard parser.parse() else {
        let underlying = parser.parserError ?? NSError(domain: "OOXML", code: 0, userInfo: nil)
        throw ConversionError.conversionFailed(reason: underlying.localizedDescription)
    }

    guard let root = delegate.root else {
        throw ConversionError.conversionFailed(reason: "El XML OOXML está vacío.")
    }
    return root
}

private final class OOXMLParserDelegate: NSObject, XMLParserDelegate {
    private(set) var root: OOXMLNode?
    private var stack: [OOXMLNode] = []

    func parser(_ parser: XMLParser,
                didStartElement elementName: String,
                namespaceURI: String?,
                qualifiedName qName: String?,
                attributes attributeDict: [String: String] = [:]) {
        let normalizedAttributes = attributeDict.reduce(into: [String: String]()) { result, pair in
            let rawKey = pair.key
            let localKey = rawKey.contains(":")
                ? String(rawKey.split(separator: ":", maxSplits: 1).last ?? Substring(rawKey))
                : rawKey
            result[localKey.lowercased()] = pair.value
        }

        let node = OOXMLNode(name: elementName, attributes: normalizedAttributes)
        if let parent = stack.last {
            node.parent = parent
            parent.children.append(node)
        } else {
            root = node
        }
        stack.append(node)
    }

    func parser(_ parser: XMLParser, foundCharacters string: String) {
        stack.last?.text += string
    }

    func parser(_ parser: XMLParser,
                didEndElement elementName: String,
                namespaceURI: String?,
                qualifiedName qName: String?) {
        _ = stack.popLast()
    }
}
