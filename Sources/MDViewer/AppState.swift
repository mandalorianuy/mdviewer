import AppKit
import Foundation
import SwiftUI
import UniformTypeIdentifiers

@MainActor
final class AppState: ObservableObject {
    @Published var fileURL: URL?
    @Published var rawMarkdown: String = ""
    @Published var renderedMarkdown: AttributedString = AttributedString("Abrí un archivo .md para comenzar.")
    @Published var selectedFontFamily: String = "SF Pro Text"
    @Published var fontSize: Double = 16
    @Published var errorMessage: String?

    let availableFonts: [String] = NSFontManager.shared.availableFontFamilies.sorted()

    func pickFileToOpen() {
        let panel = NSOpenPanel()
        let markdownType = UTType(filenameExtension: "md") ?? .plainText
        panel.allowedContentTypes = [
            markdownType,
            .plainText
        ]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false

        guard panel.runModal() == .OK, let url = panel.url else {
            return
        }

        open(url: url)
    }

    func open(url: URL) {
        do {
            let data = try Data(contentsOf: url)
            guard let markdown = String(data: data, encoding: .utf8) else {
                throw NSError(domain: "MDViewer", code: 1, userInfo: [NSLocalizedDescriptionKey: "El archivo no está en UTF-8."])
            }

            fileURL = url
            rawMarkdown = markdown
            renderMarkdown()
            errorMessage = nil
        } catch {
            errorMessage = "No se pudo abrir el archivo: \(error.localizedDescription)"
        }
    }

    func renderMarkdown() {
        if rawMarkdown.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            renderedMarkdown = AttributedString("(Archivo vacío)")
            return
        }

        do {
            var attributed = try AttributedString(
                markdown: rawMarkdown,
                options: AttributedString.MarkdownParsingOptions(
                    interpretedSyntax: .full,
                    failurePolicy: .returnPartiallyParsedIfPossible
                )
            )
            let font = Font.custom(selectedFontFamily, size: fontSize)
            attributed.font = font
            renderedMarkdown = attributed
        } catch {
            renderedMarkdown = AttributedString(rawMarkdown)
            renderedMarkdown.font = Font.custom(selectedFontFamily, size: fontSize)
            errorMessage = "Markdown parcialmente inválido. Se muestra texto plano."
        }
    }

    func updateTypography(fontFamily: String? = nil, size: Double? = nil) {
        if let fontFamily {
            selectedFontFamily = fontFamily
        }
        if let size {
            fontSize = max(10, min(size, 40))
        }
        renderMarkdown()
    }

    func exportPDF() {
        guard !rawMarkdown.isEmpty else {
            return
        }

        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        panel.canCreateDirectories = true
        panel.nameFieldStringValue = suggestedPDFName

        guard panel.runModal() == .OK, let url = panel.url else {
            return
        }

        do {
            try PDFExporter.export(
                markdown: rawMarkdown,
                fontFamily: selectedFontFamily,
                fontSize: CGFloat(fontSize),
                outputURL: url
            )
            errorMessage = nil
        } catch {
            errorMessage = "No se pudo exportar el PDF: \(error.localizedDescription)"
        }
    }

    private var suggestedPDFName: String {
        let base = fileURL?.deletingPathExtension().lastPathComponent ?? "documento"
        return "\(base).pdf"
    }
}
