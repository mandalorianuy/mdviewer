# Integración nativa markitdown — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Agregar a MDViewer soporte nativo para abrir `.csv`, `.json`, `.xml`, `.html` y `.zip`, convirtiéndolos a Markdown y renderizándolos con el pipeline existente.

**Architecture:** Se introduce una capa de conversión bajo el protocolo `DocumentConverter`. `MarkdownFileDocument` detecta archivos no-Markdown, escribe sus datos a un archivo temporal y delega la conversión en `DocumentConversionService`. Cada formato tiene su propio convertidor síncrono que devuelve `MarkdownConversionResult` (markdown + metadatos + advertencias). `ContentView` muestra una barra de estado con el formato origen y las advertencias.

**Tech Stack:** Swift 6.0, SwiftUI, Foundation, UniformTypeIdentifiers. Sin dependencias de terceros para el MVP (ZIP se maneja con `/usr/bin/unzip` del sistema).

---

## File structure

### New files

- `Sources/MDViewer/Conversion/MarkdownConversionResult.swift` — modelo del resultado de conversión.
- `Sources/MDViewer/Conversion/ConversionError.swift` — errores de conversión.
- `Sources/MDViewer/Conversion/DocumentConverter.swift` — protocolo de convertidor.
- `Sources/MDViewer/Conversion/FormatDetector.swift` — selección de convertidor por extensión.
- `Sources/MDViewer/Conversion/DocumentConversionService.swift` — orquestador off-main.
- `Sources/MDViewer/Conversion/Converters/CSVToMarkdownConverter.swift`
- `Sources/MDViewer/Conversion/Converters/JSONToMarkdownConverter.swift`
- `Sources/MDViewer/Conversion/Converters/XMLToMarkdownConverter.swift`
- `Sources/MDViewer/Conversion/Converters/HTMLToMarkdownConverter.swift`
- `Sources/MDViewer/Conversion/Converters/ZIPToMarkdownConverter.swift`
- `Tests/MDViewerTests/MDViewerTests.swift` — tests unitarios e integración.
- `Tests/MDViewerTests/Fixtures/` — archivos de ejemplo.

### Modified files

- `Sources/MDViewer/AppState.swift` — `MarkdownFileDocument` soporta conversión.
- `Sources/MDViewer/ContentView.swift` — UI de estado de conversión, advertencias, botón "Abrir archivo".
- `Sources/MDViewer/SettingsView.swift` — asociación de formatos convertibles.
- `Sources/MDViewer/MarkdownAssociationService.swift` — soporte para múltiples UTTypes.
- `macos/Info.plist` — declarar UTTypes y CFBundleDocumentTypes para los nuevos formatos.
- `project.yml` — agregar target de tests y sources de conversión.
- `Package.swift` — agregar target de tests.

---

## Task 1: Configurar target de tests

**Files:**
- Modify: `Package.swift`
- Modify: `project.yml`
- Create: `Tests/MDViewerTests/MDViewerTests.swift`

- [ ] **Step 1: Agregar test target en Package.swift**

```swift
// swift-tools-version: 6.2

import PackageDescription

let package = Package(
    name: "MDViewer",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(name: "MDViewer", targets: ["MDViewer"])
    ],
    dependencies: [
        .package(url: "https://github.com/stackotter/Down-gfm", from: "0.12.0")
    ],
    targets: [
        .executableTarget(
            name: "MDViewer",
            dependencies: [
                .product(name: "Down", package: "Down-gfm")
            ]
        ),
        .testTarget(
            name: "MDViewerTests",
            dependencies: [
                .target(name: "MDViewer"),
                .product(name: "Down", package: "Down-gfm")
            ]
        )
    ]
)
```

- [ ] **Step 2: Agregar test target en project.yml**

Reemplazar:

```yaml
      gatherCoverageData: false
      testTargets: []
```

por:

```yaml
      gatherCoverageData: false
      testTargets:
        - name: MDViewerTests
```

Y agregar al final del archivo la sección de targets:

```yaml
  MDViewerTests:
    type: bundle.unit-test
    platform: macOS
    sources:
      - path: Tests/MDViewerTests
    dependencies:
      - target: MDViewer
```

- [ ] **Step 3: Crear archivo de tests base**

```swift
import XCTest
@testable import MDViewer

final class MDViewerTests: XCTestCase {
    func testConversionServiceExists() {
        let service = DocumentConversionService()
        XCTAssertNotNil(service)
    }
}
```

- [ ] **Step 4: Verificar que `swift test` reconoce el target**

Run:

```bash
cd /Users/facundo/desarrollo/mdviewer
swift test --list-tests
```

Expected: aparece `MDViewerTests.MDViewerTests/testConversionServiceExists`.

- [ ] **Step 5: Commit**

```bash
git add Package.swift project.yml Tests/MDViewerTests/MDViewerTests.swift
git commit -m "chore: add MDViewerTests target"
```

---

## Task 2: Modelo base de conversión

**Files:**
- Create: `Sources/MDViewer/Conversion/MarkdownConversionResult.swift`
- Create: `Sources/MDViewer/Conversion/ConversionError.swift`
- Create: `Sources/MDViewer/Conversion/DocumentConverter.swift`

- [ ] **Step 1: Crear `MarkdownConversionResult.swift`**

```swift
import Foundation

struct MarkdownConversionResult: Sendable {
    let markdown: String
    let sourceFormat: String
    let title: String?
    let warnings: [String]
}
```

- [ ] **Step 2: Crear `ConversionError.swift`**

```swift
import Foundation

enum ConversionError: Error {
    case unsupportedFormat
    case fileNotReadable
    case conversionFailed(underlying: Error)
    case timeout
}

extension ConversionError: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .unsupportedFormat:
            return "Formato no soportado todavia."
        case .fileNotReadable:
            return "No se pudo leer el archivo."
        case .conversionFailed(let underlying):
            return "Error de conversion: \(underlying.localizedDescription)"
        case .timeout:
            return "La conversion tardo demasiado."
        }
    }
}
```

- [ ] **Step 3: Crear `DocumentConverter.swift`**

```swift
import Foundation

protocol DocumentConverter: Sendable {
    var supportedExtensions: [String] { get }
    func canConvert(_ url: URL) -> Bool
    func convert(_ url: URL) throws -> MarkdownConversionResult
}

extension DocumentConverter {
    func canConvert(_ url: URL) -> Bool {
        supportedExtensions.contains(url.pathExtension.lowercased())
    }
}
```

- [ ] **Step 4: Verificar compilación**

Run:

```bash
cd /Users/facundo/desarrollo/mdviewer
swift build
```

Expected: compila sin errores.

- [ ] **Step 5: Commit**

```bash
git add Sources/MDViewer/Conversion/
git commit -m "feat(conversion): add base conversion models and protocol"
```

---

## Task 3: FormatDetector

**Files:**
- Create: `Sources/MDViewer/Conversion/FormatDetector.swift`

- [ ] **Step 1: Implementar `FormatDetector.swift`**

```swift
import Foundation

struct FormatDetector: Sendable {
    private let converters: [DocumentConverter]

    init(converters: [DocumentConverter]) {
        self.converters = converters
    }

    func converter(for url: URL) -> DocumentConverter? {
        converters.first { $0.canConvert(url) }
    }

    func converter(forExtension ext: String) -> DocumentConverter? {
        let lowercased = ext.lowercased()
        return converters.first { $0.supportedExtensions.contains(lowercased) }
    }
}
```

- [ ] **Step 2: Agregar test**

En `Tests/MDViewerTests/FormatDetectorTests.swift`:

```swift
import XCTest
@testable import MDViewer

final class FormatDetectorTests: XCTestCase {
    func testDetectsCSV() {
        let detector = FormatDetector(converters: [CSVToMarkdownConverter()])
        let url = URL(fileURLWithPath: "/tmp/sample.csv")
        XCTAssertNotNil(detector.converter(for: url))
    }

    func testReturnsNilForUnknownExtension() {
        let detector = FormatDetector(converters: [CSVToMarkdownConverter()])
        let url = URL(fileURLWithPath: "/tmp/sample.unknown")
        XCTAssertNil(detector.converter(for: url))
    }
}
```

- [ ] **Step 3: Run tests**

```bash
swift test --filter FormatDetectorTests
```

Expected: FAIL porque CSVToMarkdownConverter aún no existe (TDD red).

- [ ] **Step 4: Commit**

```bash
git add Sources/MDViewer/Conversion/FormatDetector.swift Tests/MDViewerTests/FormatDetectorTests.swift
git commit -m "feat(conversion): add FormatDetector with failing tests"
```

---

## Task 4: Convertidor CSV

**Files:**
- Create: `Sources/MDViewer/Conversion/Converters/CSVToMarkdownConverter.swift`
- Create: `Tests/MDViewerTests/CSVToMarkdownConverterTests.swift`

- [ ] **Step 1: Implementar `CSVToMarkdownConverter.swift`**

```swift
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
            throw ConversionError.conversionFailed(underlying: error)
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
```

- [ ] **Step 2: Tests CSV**

```swift
import XCTest
@testable import MDViewer

final class CSVToMarkdownConverterTests: XCTestCase {
    private let converter = CSVToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".csv")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeCSV(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testSimpleCSV() {
        writeCSV("Nombre,Edad\nJuan,30\nMaria,25")
        let result = try! converter.convert(tempURL)
        let expected = """
        | Nombre | Edad |
        | --- | --- |
        | Juan | 30 |
        | Maria | 25 |
        """
        XCTAssertEqual(result.markdown, expected)
    }

    func testCSVWithQuotes() {
        writeCSV("Producto,Descripcion\n\"A\",\"B, C\"\n\"D\"\"E\",\"F\"")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B, C |"))
        XCTAssertTrue(result.markdown.contains("| D\"E | F |"))
    }
}
```

- [ ] **Step 3: Run tests**

```bash
swift test --filter CSVToMarkdownConverterTests
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add Sources/MDViewer/Conversion/Converters/CSVToMarkdownConverter.swift Tests/MDViewerTests/CSVToMarkdownConverterTests.swift
git commit -m "feat(conversion): add CSV to Markdown converter"
```

---

## Task 5: Convertidor JSON

**Files:**
- Create: `Sources/MDViewer/Conversion/Converters/JSONToMarkdownConverter.swift`
- Create: `Tests/MDViewerTests/JSONToMarkdownConverterTests.swift`

- [ ] **Step 1: Implementar `JSONToMarkdownConverter.swift`**

```swift
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
            throw ConversionError.conversionFailed(underlying: error)
        }

        let jsonObject: Any
        do {
            jsonObject = try JSONSerialization.jsonObject(with: data, options: [])
        } catch {
            throw ConversionError.conversionFailed(underlying: error)
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
```

- [ ] **Step 2: Tests JSON**

```swift
import XCTest
@testable import MDViewer

final class JSONToMarkdownConverterTests: XCTestCase {
    private let converter = JSONToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".json")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeJSON(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testObject() {
        writeJSON("{\"nombre\": \"Juan\", \"edad\": 30, \"activo\": true}")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("- **nombre**: Juan"))
        XCTAssertTrue(result.markdown.contains("- **edad**: `30`"))
        XCTAssertTrue(result.markdown.contains("- **activo**: `true`"))
    }

    func testInvalidJSONThrows() {
        writeJSON("{ no es json }")
        XCTAssertThrowsError(try converter.convert(tempURL))
    }
}
```

- [ ] **Step 3: Run tests**

```bash
swift test --filter JSONToMarkdownConverterTests
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add Sources/MDViewer/Conversion/Converters/JSONToMarkdownConverter.swift Tests/MDViewerTests/JSONToMarkdownConverterTests.swift
git commit -m "feat(conversion): add JSON to Markdown converter"
```

---

## Task 6: Convertidor XML

**Files:**
- Create: `Sources/MDViewer/Conversion/Converters/XMLToMarkdownConverter.swift`
- Create: `Tests/MDViewerTests/XMLToMarkdownConverterTests.swift`

- [ ] **Step 1: Implementar `XMLToMarkdownConverter.swift`**

```swift
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
            throw ConversionError.conversionFailed(underlying: error)
        }

        let parser = XMLParser(data: data)
        let delegate = XMLParserDelegateHandler()
        parser.delegate = delegate

        guard parser.parse() else {
            let underlying = parser.parserError ?? NSError(domain: "XML", code: 0, userInfo: nil)
            throw ConversionError.conversionFailed(underlying: underlying)
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
```

- [ ] **Step 2: Tests XML**

```swift
import XCTest
@testable import MDViewer

final class XMLToMarkdownConverterTests: XCTestCase {
    private let converter = XMLToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".xml")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeXML(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testSimpleXML() {
        writeXML("<?xml version=\"1.0\"?><root><user><name>Juan</name></user></root>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("**root**"))
        XCTAssertTrue(result.markdown.contains("**user**"))
        XCTAssertTrue(result.markdown.contains("**name**: Juan"))
    }

    func testInvalidXMLThrows() {
        writeXML("<root><unclosed>")
        XCTAssertThrowsError(try converter.convert(tempURL))
    }
}
```

- [ ] **Step 3: Run tests**

```bash
swift test --filter XMLToMarkdownConverterTests
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add Sources/MDViewer/Conversion/Converters/XMLToMarkdownConverter.swift Tests/MDViewerTests/XMLToMarkdownConverterTests.swift
git commit -m "feat(conversion): add XML to Markdown converter"
```

---

## Task 7: Convertidor HTML

**Files:**
- Create: `Sources/MDViewer/Conversion/Converters/HTMLToMarkdownConverter.swift`
- Create: `Tests/MDViewerTests/HTMLToMarkdownConverterTests.swift`

- [ ] **Step 1: Implementar `HTMLToMarkdownConverter.swift`**

```swift
import Foundation

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
            throw ConversionError.conversionFailed(underlying: error)
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
```

- [ ] **Step 2: Tests HTML**

```swift
import XCTest
@testable import MDViewer

final class HTMLToMarkdownConverterTests: XCTestCase {
    private let converter = HTMLToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".html")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    private func writeHTML(_ content: String) {
        try! content.write(to: tempURL, atomically: true, encoding: .utf8)
    }

    func testHeadingsAndParagraphs() {
        writeHTML("<h1>Titulo</h1><p>Este es un <strong>texto</strong>.</p>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("# Titulo"))
        XCTAssertTrue(result.markdown.contains("Este es un **texto**."))
    }

    func testLink() {
        writeHTML("<a href=\"https://example.com\">Ejemplo</a>")
        let result = try! converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("[Ejemplo](https://example.com)"))
    }
}
```

- [ ] **Step 3: Run tests**

```bash
swift test --filter HTMLToMarkdownConverterTests
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add Sources/MDViewer/Conversion/Converters/HTMLToMarkdownConverter.swift Tests/MDViewerTests/HTMLToMarkdownConverterTests.swift
git commit -m "feat(conversion): add HTML to Markdown converter"
```

---

## Task 8: Convertidor ZIP

**Files:**
- Create: `Sources/MDViewer/Conversion/Converters/ZIPToMarkdownConverter.swift`
- Create: `Tests/MDViewerTests/ZIPToMarkdownConverterTests.swift`

- [ ] **Step 1: Implementar `ZIPToMarkdownConverter.swift`**

```swift
import Foundation

struct ZIPToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["zip"]

    private let service: DocumentConversionService

    init(service: DocumentConversionService = DocumentConversionService()) {
        self.service = service
    }

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            throw ConversionError.fileNotReadable
        }

        let tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)

        try? FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        process.arguments = ["-o", url.path, "-d", tempDir.path]

        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = pipe

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            throw ConversionError.conversionFailed(underlying: error)
        }

        guard process.terminationStatus == 0 else {
            throw ConversionError.conversionFailed(underlying: NSError(domain: "ZIP", code: Int(process.terminationStatus)))
        }

        let files = try listFiles(at: tempDir)
        let convertibleFiles = files.filter { path in
            DocumentConversionService.isConvertibleExtension(URL(fileURLWithPath: path).pathExtension)
        }

        guard let firstFile = convertibleFiles.first else {
            let index = files.map { "- \($0)" }.joined(separator: "\n")
            return MarkdownConversionResult(
                markdown: "_El archivo ZIP no contiene formatos soportados._\n\n## Contenido\n\n\(index.isEmpty ? "_Vacío_" : index)",
                sourceFormat: "ZIP",
                title: nil,
                warnings: ["No se encontro un archivo convertible dentro del ZIP."]
            )
        }

        let firstURL = URL(fileURLWithPath: firstFile)
        let innerResult = try service.convert(url: firstURL)

        var warnings = innerResult.warnings
        if convertibleFiles.count > 1 {
            warnings.append("El ZIP contiene varios archivos soportados; se convirtio el primero: \(firstURL.lastPathComponent).")
        }

        return MarkdownConversionResult(
            markdown: innerResult.markdown,
            sourceFormat: "ZIP (\(innerResult.sourceFormat))",
            title: innerResult.title,
            warnings: warnings
        )
    }

    private func listFiles(at directory: URL) throws -> [String] {
        let enumerator = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: nil)
        var files: [String] = []

        while let fileURL = enumerator?.nextObject() as? URL {
            var isDirectory: ObjCBool = false
            FileManager.default.fileExists(atPath: fileURL.path, isDirectory: &isDirectory)
            if !isDirectory.boolValue {
                files.append(fileURL.path)
            }
        }

        return files.sorted()
    }
}
```

- [ ] **Step 2: Agregar helper `isConvertibleExtension` a `DocumentConversionService`**

Ver Task 9 para la implementación completa de `DocumentConversionService`. Incluir allí:

```swift
static func isConvertibleExtension(_ ext: String) -> Bool {
    let knownExtensions = ["csv", "json", "xml", "html", "htm"]
    return knownExtensions.contains(ext.lowercased())
}
```

- [ ] **Step 3: Tests ZIP**

```swift
import XCTest
@testable import MDViewer

final class ZIPToMarkdownConverterTests: XCTestCase {
    private let converter = ZIPToMarkdownConverter()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".zip")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testZIPWithCSV() throws {
        let csvURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".csv")
        try "A,B\n1,2".write(to: csvURL, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(at: csvURL) }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
        process.arguments = [tempURL.path, csvURL.path]
        try process.run()
        process.waitUntilExit()

        let result = try converter.convert(tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B |"))
    }
}
```

- [ ] **Step 4: Run tests**

```bash
swift test --filter ZIPToMarkdownConverterTests
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Sources/MDViewer/Conversion/Converters/ZIPToMarkdownConverter.swift Tests/MDViewerTests/ZIPToMarkdownConverterTests.swift
git commit -m "feat(conversion): add ZIP to Markdown converter"
```

---

## Task 9: DocumentConversionService

**Files:**
- Create: `Sources/MDViewer/Conversion/DocumentConversionService.swift`

- [ ] **Step 1: Implementar `DocumentConversionService.swift`**

```swift
import Foundation
import os.log

actor DocumentConversionService {
    static let shared = DocumentConversionService()

    private let detector: FormatDetector
    private let logger = Logger(subsystem: "com.facundo.mdviewer.conversion", category: "conversion")

    init(converters: [DocumentConverter] = DocumentConversionService.defaultConverters) {
        self.detector = FormatDetector(converters: converters)
    }

    static var defaultConverters: [DocumentConverter] {
        [
            CSVToMarkdownConverter(),
            JSONToMarkdownConverter(),
            XMLToMarkdownConverter(),
            HTMLToMarkdownConverter(),
            ZIPToMarkdownConverter()
        ]
    }

    static func isConvertibleExtension(_ ext: String) -> Bool {
        let knownExtensions = ["csv", "json", "xml", "html", "htm"]
        return knownExtensions.contains(ext.lowercased())
    }

    func convert(url: URL) async throws -> MarkdownConversionResult {
        logger.info("Convirtiendo archivo: \(url.lastPathComponent)")

        guard let converter = detector.converter(for: url) else {
            logger.error("Formato no soportado para: \(url.lastPathComponent)")
            throw ConversionError.unsupportedFormat
        }

        return try await Task.detached(priority: .userInitiated) {
            do {
                return try converter.convert(url)
            } catch let error as ConversionError {
                throw error
            } catch {
                throw ConversionError.conversionFailed(underlying: error)
            }
        }.value
    }
}
```

- [ ] **Step 2: Tests de integración**

Agregar a `Tests/MDViewerTests/DocumentConversionServiceTests.swift`:

```swift
import XCTest
@testable import MDViewer

final class DocumentConversionServiceTests: XCTestCase {
    private let service = DocumentConversionService()
    private var tempURL: URL!

    override func setUp() {
        super.setUp()
        tempURL = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".csv")
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tempURL)
        super.tearDown()
    }

    func testConvertsCSV() async throws {
        try "A,B\n1,2".write(to: tempURL, atomically: true, encoding: .utf8)
        let result = try await service.convert(url: tempURL)
        XCTAssertTrue(result.markdown.contains("| A | B |"))
    }

    func testUnsupportedFormatThrows() async {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString + ".xyz")
        do {
            _ = try await service.convert(url: url)
            XCTFail("Deberia haber fallado")
        } catch {
            XCTAssertTrue(error is ConversionError)
        }
    }
}
```

- [ ] **Step 3: Run tests**

```bash
swift test --filter DocumentConversionServiceTests
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add Sources/MDViewer/Conversion/DocumentConversionService.swift Tests/MDViewerTests/DocumentConversionServiceTests.swift
git commit -m "feat(conversion): add DocumentConversionService"
```

---

## Task 10: Integrar MarkdownFileDocument con conversión

**Files:**
- Modify: `Sources/MDViewer/AppState.swift`

- [ ] **Step 1: Extender `MarkdownFileDocument` para soportar conversión**

```swift
import Foundation
import SwiftUI
import UniformTypeIdentifiers

extension UTType {
    static let mdviewerMarkdown = UTType(importedAs: "net.daringfireball.markdown")
}

struct MarkdownFileDocument: FileDocument {
    static let readableContentTypes: [UTType] = [
        .mdviewerMarkdown,
        .commaSeparatedText,
        .json,
        .xml,
        .html,
        .zip
    ]
    static let writableContentTypes: [UTType] = [.mdviewerMarkdown]

    var rawMarkdown: String
    var conversionResult: MarkdownConversionResult?

    init(rawMarkdown: String = "") {
        self.rawMarkdown = rawMarkdown
        self.conversionResult = nil
    }

    init(conversionResult: MarkdownConversionResult) {
        self.rawMarkdown = conversionResult.markdown
        self.conversionResult = conversionResult
    }

    init(configuration: ReadConfiguration) throws {
        guard let fileWrapper = configuration.file.regularFileContents else {
            rawMarkdown = ""
            conversionResult = nil
            return
        }

        let filename = configuration.file.filename ?? ""
        let ext = (filename as NSString).pathExtension.lowercased()

        if ext == "md" || ext == "markdown" || ext == "mdown" || ext == "mkdn" || ext == "mkd" {
            guard let markdown = String(data: fileWrapper, encoding: .utf8) else {
                throw CocoaError(.fileReadCorruptFile)
            }
            rawMarkdown = markdown
            conversionResult = nil
            return
        }

        guard DocumentConversionService.isConvertibleExtension(ext) || ext == "zip" || ext == "html" || ext == "htm" else {
            guard let markdown = String(data: fileWrapper, encoding: .utf8) else {
                throw CocoaError(.fileReadCorruptFile)
            }
            rawMarkdown = markdown
            conversionResult = nil
            return
        }

        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(ext)

        try fileWrapper.write(to: tempURL)

        let service = DocumentConversionService()
        let result = try await service.convert(url: tempURL)

        rawMarkdown = result.markdown
        conversionResult = result
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        let data = rawMarkdown.data(using: .utf8) ?? Data()
        return .init(regularFileWithContents: data)
    }
}
```

Nota: `FileDocument.init(configuration:)` no es `async`. Para llamar al servicio async, se requiere usar `await` dentro de un contexto async. SwiftUI `FileDocument` no soporta init async. Se debe usar `Task` y un estado de carga, o refactorizar para que la conversión no ocurra en el init.

**Corrección del diseño:** El init síncrono no puede llamar async. La solución es:

1. En `init(configuration:)`, si es un archivo convertible, guardar los datos sin convertir y marcar `needsConversion = true`.
2. La conversión se realiza en `ContentView` al aparecer, mostrando un estado de carga.
3. O: usar `DocumentGroup(editing:)` en lugar de `viewing:` y controlar manualmente.

Para mantener la simplicidad, elegimos la opción 1: `MarkdownFileDocument` guarda los datos originales y `ContentView` dispara la conversión si `needsConversion` es true.

Reescribir `AppState.swift`:

```swift
import Foundation
import SwiftUI
import UniformTypeIdentifiers

extension UTType {
    static let mdviewerMarkdown = UTType(importedAs: "net.daringfireball.markdown")
}

struct MarkdownFileDocument: FileDocument {
    static let readableContentTypes: [UTType] = [
        .mdviewerMarkdown,
        .commaSeparatedText,
        .json,
        .xml,
        .html,
        .zip
    ]
    static let writableContentTypes: [UTType] = [.mdviewerMarkdown]

    var rawMarkdown: String
    var conversionResult: MarkdownConversionResult?
    var pendingConversionURL: URL?

    init(rawMarkdown: String = "") {
        self.rawMarkdown = rawMarkdown
        self.conversionResult = nil
        self.pendingConversionURL = nil
    }

    init(conversionResult: MarkdownConversionResult) {
        self.rawMarkdown = conversionResult.markdown
        self.conversionResult = conversionResult
        self.pendingConversionURL = nil
    }

    init(configuration: ReadConfiguration) throws {
        guard let fileWrapper = configuration.file.regularFileContents else {
            rawMarkdown = ""
            conversionResult = nil
            pendingConversionURL = nil
            return
        }

        let filename = configuration.file.filename ?? ""
        let ext = (filename as NSString).pathExtension.lowercased()

        if ext == "md" || ext == "markdown" || ext == "mdown" || ext == "mkdn" || ext == "mkd" {
            guard let markdown = String(data: fileWrapper, encoding: .utf8) else {
                throw CocoaError(.fileReadCorruptFile)
            }
            rawMarkdown = markdown
            conversionResult = nil
            pendingConversionURL = nil
            return
        }

        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(ext)

        try fileWrapper.write(to: tempURL)

        rawMarkdown = ""
        conversionResult = nil
        pendingConversionURL = tempURL
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        let data = rawMarkdown.data(using: .utf8) ?? Data()
        return .init(regularFileWithContents: data)
    }
}
```

- [ ] **Step 2: Compilar**

```bash
swift build
```

Expected: compila (aunque ContentView aún no use pendingConversionURL).

- [ ] **Step 3: Commit**

```bash
git add Sources/MDViewer/AppState.swift
git commit -m "feat(document): support convertible file types and pending conversion state"
```

---

## Task 11: Actualizar ContentView

**Files:**
- Modify: `Sources/MDViewer/ContentView.swift`

- [ ] **Step 1: Agregar estados de conversión**

Agregar al inicio de `ContentView`:

```swift
    @State private var conversionError: String?
    @State private var isConverting = false
```

Y modificar el cuerpo para mostrar la barra de conversión.

- [ ] **Step 2: Implementar `conversionBar`**

```swift
    @ViewBuilder
    private var conversionBar: some View {
        if let result = document.conversionResult {
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Image(systemName: "arrow.right.arrow.left")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(controlAccent)

                    Text("Convertido desde \(result.sourceFormat)")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(secondaryText)

                    Spacer()

                    if !result.warnings.isEmpty {
                        Button {
                            // Toggle warnings panel (simplificado: cicla el primer mensaje)
                        } label: {
                            HStack(spacing: 4) {
                                Image(systemName: "exclamationmark.triangle")
                                    .font(.system(size: 10))
                                Text("\(result.warnings.count) advertencia\(result.warnings.count == 1 ? "" : "s")")
                                    .font(.system(size: 11))
                            }
                            .foregroundStyle(.orange)
                        }
                        .buttonStyle(.plain)
                    }
                }

                if !result.warnings.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(result.warnings, id: \.self) { warning in
                            Text("• \(warning)")
                                .font(.system(size: 11))
                                .foregroundStyle(.orange)
                        }
                    }
                }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(chromeAccentBackground)

            Rectangle()
                .fill(dividerColor)
                .frame(height: 1)
        }
    }
```

- [ ] **Step 3: Insertar `conversionBar` en el layout**

Después del `Rectangle()` divisor bajo `controlsBar`, agregar:

```swift
            conversionBar
```

- [ ] **Step 4: Manejar `pendingConversionURL` al aparecer**

Agregar `.task` adicional en `body`:

```swift
        .task {
            await runPendingConversionIfNeeded()
        }
```

Y el método:

```swift
    private func runPendingConversionIfNeeded() async {
        guard let url = document.pendingConversionURL, document.rawMarkdown.isEmpty else { return }

        isConverting = true
        conversionError = nil

        do {
            let result = try await DocumentConversionService.shared.convert(url: url)
            document.rawMarkdown = result.markdown
            document.conversionResult = result
            document.pendingConversionURL = nil
        } catch {
            conversionError = error.localizedDescription
        }

        isConverting = false
    }
```

Nota: `document` es `let` en ContentView. Para mutarlo, `MarkdownFileDocument` debe ser una clase o usar `@State` en ContentView. Como `FileDocument` es un struct pasado por `DocumentGroup`, es mutable dentro de la vista mediante el binding implícito. Pero con `let document: MarkdownFileDocument` no podemos mutar. En `DocumentGroup(viewing:)` el closure recibe `file` que es `FileDocumentConfiguration<MarkdownFileDocument>`. Normalmente `ContentView(document: file.document)` pasa el documento por valor. Para mutarlo, deberíamos pasar un binding o hacer que `MarkdownFileDocument` sea una clase.

**Corrección:** `MarkdownFileDocument` debe ser una `class` que conforme `ReferenceFileDocument` en lugar de `FileDocument`, o mantener `FileDocument` y hacer que `ContentView` reciba un `@Binding` o `@State`.

La solución más simple: cambiar `MarkdownFileDocument` a `ReferenceFileDocument` (una clase `@MainActor`). Esto permite mutar el documento desde `ContentView`.

Reescribir `AppState.swift` como clase:

```swift
import Foundation
import SwiftUI
import UniformTypeIdentifiers

extension UTType {
    static let mdviewerMarkdown = UTType(importedAs: "net.daringfireball.markdown")
}

@MainActor
final class MarkdownFileDocument: ReferenceFileDocument {
    static let readableContentTypes: [UTType] = [
        .mdviewerMarkdown,
        .commaSeparatedText,
        .json,
        .xml,
        .html,
        .zip
    ]
    static let writableContentTypes: [UTType] = [.mdviewerMarkdown]

    var rawMarkdown: String
    var conversionResult: MarkdownConversionResult?
    var pendingConversionURL: URL?

    init(rawMarkdown: String = "") {
        self.rawMarkdown = rawMarkdown
        self.conversionResult = nil
        self.pendingConversionURL = nil
    }

    init(conversionResult: MarkdownConversionResult) {
        self.rawMarkdown = conversionResult.markdown
        self.conversionResult = conversionResult
        self.pendingConversionURL = nil
    }

    init(configuration: ReadConfiguration) throws {
        guard let fileWrapper = configuration.file.regularFileContents else {
            rawMarkdown = ""
            conversionResult = nil
            pendingConversionURL = nil
            return
        }

        let filename = configuration.file.filename ?? ""
        let ext = (filename as NSString).pathExtension.lowercased()

        if ext == "md" || ext == "markdown" || ext == "mdown" || ext == "mkdn" || ext == "mkd" {
            guard let markdown = String(data: fileWrapper, encoding: .utf8) else {
                throw CocoaError(.fileReadCorruptFile)
            }
            rawMarkdown = markdown
            conversionResult = nil
            pendingConversionURL = nil
            return
        }

        let tempURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString)
            .appendingPathExtension(ext)

        try fileWrapper.write(to: tempURL)

        rawMarkdown = ""
        conversionResult = nil
        pendingConversionURL = tempURL
    }

    func snapshot(contentType: UTType) throws -> String {
        rawMarkdown
    }

    func fileWrapper(snapshot: String, configuration: WriteConfiguration) throws -> FileWrapper {
        let data = snapshot.data(using: .utf8) ?? Data()
        return .init(regularFileWithContents: data)
    }
}
```

Y en `MDViewerApp.swift` cambiar `DocumentGroup(viewing:)` por `DocumentGroup(editing:)`:

```swift
        DocumentGroup(editing: MarkdownFileDocument.self) { file in
            ContentView(document: file.document)
                .frame(minWidth: 720, minHeight: 520)
        }
```

`ContentView` sigue recibiendo `let document: MarkdownFileDocument` pero ahora es una clase referenciada por `DocumentGroup`, por lo que las mutaciones se reflejan.

- [ ] **Step 5: Renombrar botón "Abrir .md" a "Abrir archivo"**

En `controlsBar`, cambiar:

```swift
Text("Abrir .md")
```

por:

```swift
Text("Abrir archivo")
```

- [ ] **Step 6: Actualizar `pickFilesToOpen` para aceptar formatos convertibles**

Cambiar:

```swift
panel.allowedContentTypes = [.mdviewerMarkdown]
```

por:

```swift
panel.allowedContentTypes = [
    .mdviewerMarkdown,
    .commaSeparatedText,
    .json,
    .xml,
    .html,
    .zip
]
```

- [ ] **Step 7: Compilar**

```bash
swift build
```

Expected: compila sin errores.

- [ ] **Step 8: Commit**

```bash
git add Sources/MDViewer/AppState.swift Sources/MDViewer/ContentView.swift Sources/MDViewer/MDViewerApp.swift
git commit -m "feat(ui): integrate conversion into document and ContentView"
```

---

## Task 12: Asociación de formatos convertibles

**Files:**
- Modify: `Sources/MDViewer/MarkdownAssociationService.swift`
- Modify: `Sources/MDViewer/SettingsView.swift`

- [ ] **Step 1: Extender `MarkdownAssociationService.swift`**

```swift
import AppKit
import UniformTypeIdentifiers

@MainActor
enum MarkdownAssociationService {
    static let convertibleUTTypes: [UTType] = [
        .mdviewerMarkdown,
        .commaSeparatedText,
        .json,
        .xml,
        .html,
        .zip
    ]

    static func currentDefaultApplicationURL() -> URL? {
        NSWorkspace.shared.urlForApplication(toOpen: .mdviewerMarkdown)
    }

    static func isMDViewerDefaultHandler() -> Bool {
        guard
            let defaultAppURL = currentDefaultApplicationURL(),
            let defaultBundleID = Bundle(url: defaultAppURL)?.bundleIdentifier,
            let currentBundleID = Bundle.main.bundleIdentifier
        else {
            return false
        }

        return defaultBundleID == currentBundleID
    }

    static func setMDViewerAsDefault() async throws {
        try await NSWorkspace.shared.setDefaultApplication(at: Bundle.main.bundleURL, toOpen: .mdviewerMarkdown)
    }

    static func setMDViewerAsDefaultForConvertibleTypes() async throws {
        for type in convertibleUTTypes {
            try await NSWorkspace.shared.setDefaultApplication(at: Bundle.main.bundleURL, toOpen: type)
        }
    }
}
```

- [ ] **Step 2: Actualizar `SettingsView.swift`**

Agregar un nuevo botón en la sección "Asociacion de archivos":

```swift
                Button("Asociar formatos convertibles con MDViewer") {
                    Task {
                        await associateConvertibleFiles()
                    }
                }
                .disabled(isUpdatingAssociation)
```

Y el método:

```swift
    @MainActor
    private func associateConvertibleFiles() async {
        isUpdatingAssociation = true
        associationStatus = "Solicitando asociacion de formatos convertibles..."

        do {
            try await MarkdownAssociationService.setMDViewerAsDefaultForConvertibleTypes()
            await refreshAssociationStatus()
        } catch {
            associationIsCurrent = false
            associationStatus = "No se pudo asociar todos los formatos: \(error.localizedDescription)"
        }

        isUpdatingAssociation = false
    }
```

- [ ] **Step 3: Compilar**

```bash
swift build
```

- [ ] **Step 4: Commit**

```bash
git add Sources/MDViewer/MarkdownAssociationService.swift Sources/MDViewer/SettingsView.swift
git commit -m "feat(settings): add association support for convertible formats"
```

---

## Task 13: Registrar UTTypes en Info.plist

**Files:**
- Modify: `macos/Info.plist`

- [ ] **Step 1: Agregar UTImportedTypeDeclarations para formatos nativos del sistema**

Los UTTypes `.commaSeparatedText`, `.json`, `.xml`, `.html` y `.zip` son tipos del sistema, no es necesario declararlos como imported. Solo necesitamos declarar `CFBundleDocumentTypes` para que la app declare que puede abrirlos.

- [ ] **Step 2: Extender CFBundleDocumentTypes**

Reemplazar la sección `CFBundleDocumentTypes` completa por:

```xml
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeIconFile</key>
            <string>MarkdownDocument</string>
            <key>CFBundleTypeName</key>
            <string>Markdown Document</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Owner</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>net.daringfireball.markdown</string>
            </array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>md</string>
                <string>markdown</string>
                <string>mdown</string>
                <string>mkdn</string>
                <string>mkd</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>
            <string>CSV Document</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.comma-separated-values-text</string>
            </array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>csv</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>
            <string>JSON Document</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.json</string>
            </array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>json</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>
            <string>XML Document</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.xml</string>
            </array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>xml</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>
            <string>HTML Document</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.html</string>
            </array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>html</string>
                <string>htm</string>
            </array>
        </dict>
        <dict>
            <key>CFBundleTypeName</key>
            <string>ZIP Archive</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
            <key>LSHandlerRank</key>
            <string>Alternate</string>
            <key>LSItemContentTypes</key>
            <array>
                <string>public.zip-archive</string>
            </array>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>zip</string>
            </array>
        </dict>
    </array>
```

- [ ] **Step 3: Regenerar proyecto Xcode**

```bash
xcodegen generate
```

- [ ] **Step 4: Commit**

```bash
git add macos/Info.plist MDViewer.xcodeproj
git commit -m "feat(plist): register CSV/JSON/XML/HTML/ZIP document types"
```

---

## Task 14: Actualizar project.yml para sources de conversión

**Files:**
- Modify: `project.yml`

- [ ] **Step 1: Asegurar que Sources/MDViewer incluye subdirectorios**

`project.yml` usa `path: Sources/MDViewer`, que debería incluir recursivamente todos los `.swift` por defecto en XcodeGen. Verificar que compila; si no, agregar `includes: ["**/*.swift"]`.

- [ ] **Step 2: Compilar**

```bash
swift build
```

- [ ] **Step 3: Commit**

```bash
git add project.yml
git commit -m "chore(project): include conversion sources in target"
```

---

## Task 15: Tests de integración

**Files:**
- Modify: `Tests/MDViewerTests/DocumentConversionServiceTests.swift` (ya creado en Task 9)
- Create: `Tests/MDViewerTests/Fixtures/sample.csv`
- Create: `Tests/MDViewerTests/Fixtures/sample.json`
- Create: `Tests/MDViewerTests/Fixtures/sample.xml`
- Create: `Tests/MDViewerTests/Fixtures/sample.html`

- [ ] **Step 1: Crear fixtures**

`Tests/MDViewerTests/Fixtures/sample.csv`:

```csv
Name,Age
Alice,30
Bob,25
```

`Tests/MDViewerTests/Fixtures/sample.json`:

```json
{"name": "Alice", "age": 30}
```

`Tests/MDViewerTests/Fixtures/sample.xml`:

```xml
<?xml version="1.0"?>
<root>
    <user>
        <name>Alice</name>
    </user>
</root>
```

`Tests/MDViewerTests/Fixtures/sample.html`:

```html
<html><body><h1>Hello</h1><p>World</p></body></html>
```

- [ ] **Step 2: Agregar tests que usen fixtures**

```swift
import XCTest
@testable import MDViewer

final class FixtureConversionTests: XCTestCase {
    private let service = DocumentConversionService()

    private func fixtureURL(named name: String) -> URL {
        let thisFile = URL(fileURLWithPath: #file)
        return thisFile
            .deletingLastPathComponent()
            .appendingPathComponent("Fixtures")
            .appendingPathComponent(name)
    }

    func testCSVFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample.csv"))
        XCTAssertTrue(result.markdown.contains("| Name | Age |"))
        XCTAssertTrue(result.markdown.contains("| Alice | 30 |"))
    }

    func testJSONFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample.json"))
        XCTAssertTrue(result.markdown.contains("- **name**: Alice"))
    }

    func testXMLFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample.xml"))
        XCTAssertTrue(result.markdown.contains("**name**: Alice"))
    }

    func testHTMLFixture() async throws {
        let result = try await service.convert(url: fixtureURL(named: "sample.html"))
        XCTAssertTrue(result.markdown.contains("# Hello"))
        XCTAssertTrue(result.markdown.contains("World"))
    }
}
```

- [ ] **Step 3: Run all tests**

```bash
swift test
```

Expected: todos los tests PASS.

- [ ] **Step 4: Commit**

```bash
git add Tests/MDViewerTests
git commit -m "test: add fixture-based integration tests"
```

---

## Task 16: Validación manual

**Files:** ninguno (solo comandos).

- [ ] **Step 1: Ejecutar la app en desarrollo**

```bash
swift run
```

- [ ] **Step 2: Probar abrir archivos**

Crear archivos de prueba:

```bash
mkdir -p /tmp/mdviewer-test
echo -e "A,B\n1,2" > /tmp/mdviewer-test/sample.csv
echo '{"name":"test"}' > /tmp/mdviewer-test/sample.json
echo '<root><item>value</item></root>' > /tmp/mdviewer-test/sample.xml
echo '<h1>Hello</h1><p>World</p>' > /tmp/mdviewer-test/sample.html
```

Desde MDViewer, usar "Abrir archivo" y seleccionar cada uno. Verificar que:

- Se renderiza el Markdown resultante.
- Aparece la barra "Convertido desde ...".
- No hay crash.

- [ ] **Step 3: Probar asociación**

Ir a Settings y presionar "Asociar formatos convertibles con MDViewer". Verificar que no falla.

- [ ] **Step 4: Probar guardar como Markdown**

Abrir un CSV, luego File > Guardar como. Verificar que genera un `.md` con el contenido convertido.

- [ ] **Step 5: Commit final (si todo OK)**

```bash
git add -A
git commit -m "feat: native markitdown-style conversion for CSV/JSON/XML/HTML/ZIP"
```

---

## Self-review del plan

### Spec coverage

- ✅ Arquitectura extensible con `DocumentConverter` (Tasks 2, 9).
- ✅ Convertidores CSV, JSON, XML, HTML, ZIP (Tasks 4-8).
- ✅ Integración con `MarkdownFileDocument` (Task 10).
- ✅ UI de conversión y advertencias (Task 11).
- ✅ Asociación de formatos (Task 12).
- ✅ Registro de UTTypes (Task 13).
- ✅ Tests unitarios e integración (Tasks 3-5, 9, 15).
- ✅ Manejo de errores (Task 2, 9, 11).
- ✅ Modelo editable/guardable (Task 10 con `ReferenceFileDocument`).

### Placeholder scan

- ✅ Sin TBD/TODO.
- ✅ Cada paso incluye código o comandos concretos.
- ✅ Tests con código completo.

### Type consistency

- ✅ `DocumentConverter.convert(_ url: URL) throws -> MarkdownConversionResult` en todos los convertidores.
- ✅ `MarkdownConversionResult` usa los mismos nombres de propiedades en todo el plan.
- ✅ `DocumentConversionService` es `actor` con `convert(url:) async throws`.

### Aclaraciones importantes para el implementador

1. `FileDocument` no permite `async` en `init(configuration:)`. El plan resuelve esto usando `ReferenceFileDocument` (clase) y una conversión pendiente que `ContentView` ejecuta.
2. `DocumentGroup(viewing:)` debe cambiarse a `DocumentGroup(editing:)` en `MDViewerApp.swift`.
3. El convertidor ZIP usa `/usr/bin/unzip` del sistema; asegurar que funcione en macOS 13+.
4. El convertidor HTML es un parser regex simple; puede necesitar mejora para HTML real complejo.
