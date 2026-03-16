import AppKit
import SwiftUI

struct ContentView: View {
    let document: MarkdownFileDocument

    @AppStorage(AppPreferenceKey.selectedFontFamily) private var selectedFontFamily = AppPreferenceDefault.fontFamily
    @AppStorage(AppPreferenceKey.fontSize) private var fontSize = AppPreferenceDefault.fontSize
    @AppStorage(AppPreferenceKey.preferTabbedWindows) private var preferTabbedWindows = AppPreferenceDefault.preferTabbedWindows
    @State private var errorMessage: String?

    private let availableFonts = NSFontManager.shared.availableFontFamilies.sorted()
    private let appVersion = AppVersion.current

    var body: some View {
        VStack(spacing: 0) {
            controlsBar
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(.ultraThinMaterial)

            Divider()

            MarkdownWebView(html: renderedHTML)
                .background(Color(NSColor.textBackgroundColor))

            if let error = errorMessage {
                Divider()
                Text(error)
                    .foregroundStyle(.red)
                    .font(.system(size: 12, weight: .medium))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(Color(NSColor.windowBackgroundColor))
            }

            footerBar
        }
        .background(WindowTabbingConfigurator(preferTabbedWindows: preferTabbedWindows))
        .onAppear {
            if !availableFonts.contains(selectedFontFamily) {
                selectedFontFamily = effectiveDefaultFont
            }
        }
    }

    private var footerBar: some View {
        VStack(spacing: 0) {
            Divider()
            HStack {
                Spacer()
                Text(appVersion.displayString)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
            }
            .background(Color(NSColor.windowBackgroundColor))
        }
    }

    private var controlsBar: some View {
        HStack(spacing: 14) {
            Button("Abrir .md") {
                pickFilesToOpen()
            }

            Divider()
                .frame(height: 20)

            Text("Fuente")
                .font(.system(size: 12, weight: .medium))

            Picker("", selection: $selectedFontFamily) {
                ForEach(availableFonts, id: \.self) { family in
                    Text(family).tag(family)
                }
            }
            .labelsHidden()
            .frame(width: 220)

            Text("Tamaño")
                .font(.system(size: 12, weight: .medium))

            HStack(spacing: 6) {
                Slider(
                    value: $fontSize,
                    in: 10...40,
                    step: 1
                )
                .frame(width: 150)

                Text("\(Int(fontSize)) pt")
                    .font(.system(size: 12, weight: .semibold))
                    .frame(width: 48, alignment: .leading)
            }

            Spacer()

            Button {
                openSettings()
            } label: {
                Image(systemName: "gearshape")
            }
            .buttonStyle(.borderless)
            .help("Configuracion")

            Button("Exportar PDF") {
                exportPDF()
            }
            .disabled(document.rawMarkdown.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
    }

    private var renderedHTML: String {
        MarkdownHTMLRenderer.renderDocument(
            markdown: document.rawMarkdown,
            fontFamily: effectiveFontFamily,
            baseFontSize: fontSize
        )
    }

    private var effectiveFontFamily: String {
        availableFonts.contains(selectedFontFamily) ? selectedFontFamily : effectiveDefaultFont
    }

    private var effectiveDefaultFont: String {
        if availableFonts.contains(AppPreferenceDefault.fontFamily) {
            return AppPreferenceDefault.fontFamily
        }

        return availableFonts.first ?? "Helvetica"
    }

    @MainActor
    private func pickFilesToOpen() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.mdviewerMarkdown]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.begin { response in
            guard response == .OK else { return }

            for url in panel.urls {
                NSDocumentController.shared.openDocument(withContentsOf: url, display: true) { _, _, openError in
                    if let openError {
                        errorMessage = openError.localizedDescription
                    }
                }
            }
        }
    }

    @MainActor
    private func exportPDF() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [.pdf]
        panel.canCreateDirectories = true
        panel.nameFieldStringValue = suggestedPDFName

        guard panel.runModal() == .OK, let outputURL = panel.url else {
            return
        }

        do {
            try PDFExporter.export(html: renderedHTML, outputURL: outputURL)
            errorMessage = nil
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private var suggestedPDFName: String {
        if let firstHeading = document.rawMarkdown
            .split(separator: "\n")
            .map(String.init)
            .first(where: { $0.trimmingCharacters(in: .whitespaces).hasPrefix("#") }) {
            let cleanedTitle = firstHeading
                .replacingOccurrences(of: "#", with: "")
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .replacingOccurrences(of: "/", with: "-")
            if !cleanedTitle.isEmpty {
                return cleanedTitle + ".pdf"
            }
        }

        return "Markdown.pdf"
    }

    private func openSettings() {
        NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
    }
}
