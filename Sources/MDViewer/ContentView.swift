import AppKit
import SwiftUI

struct ContentView: View {
    let document: MarkdownFileDocument

    @Environment(\.colorScheme) private var colorScheme
    @AppStorage(AppPreferenceKey.selectedFontFamily) private var selectedFontFamily = AppPreferenceDefault.fontFamily
    @AppStorage(AppPreferenceKey.fontSize) private var fontSize = AppPreferenceDefault.fontSize
    @AppStorage(AppPreferenceKey.preferTabbedWindows) private var preferTabbedWindows = AppPreferenceDefault.preferTabbedWindows
    @AppStorage(AppPreferenceKey.appearanceMode) private var appearanceModeRawValue = AppPreferenceDefault.appearanceMode
    @State private var errorMessage: String?
    @State private var themeIconScale: CGFloat = 1.0
    @State private var themeIconRotation: Double = 0
    @State private var themeIconGlowOpacity = 0.0

    private let availableFonts = NSFontManager.shared.availableFontFamilies.sorted()
    private let appVersion = AppVersion.current

    var body: some View {
        VStack(spacing: 0) {
            controlsBar
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(chromeBackground)

            Rectangle()
                .fill(dividerColor)
                .frame(height: 1)

            MarkdownWebView(html: renderedHTML)
                .background(windowBackground)

            if let error = errorMessage {
                Rectangle()
                    .fill(dividerColor)
                    .frame(height: 1)
                Text(error)
                    .foregroundStyle(.red)
                    .font(.system(size: 12, weight: .medium))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(chromeAccentBackground)
            }

            footerBar
        }
        .preferredColorScheme(selectedAppearanceMode.preferredColorScheme)
        .tint(controlAccent)
        .background(WindowTabbingConfigurator(preferTabbedWindows: preferTabbedWindows))
        .background(windowBackground)
        .onAppear {
            if !availableFonts.contains(selectedFontFamily) {
                selectedFontFamily = effectiveDefaultFont
            }
        }
    }

    private var footerBar: some View {
        VStack(spacing: 0) {
            Rectangle()
                .fill(dividerColor)
                .frame(height: 1)
            HStack {
                Spacer()
                Text(appVersion.displayString)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(mutedText)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
            }
            .background(chromeBackground)
        }
    }

    private var controlsBar: some View {
        HStack(spacing: 14) {
            Button {
                pickFilesToOpen()
            } label: {
                Text("Abrir .md")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(primaryText)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(secondaryActionBackground)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .stroke(secondaryActionBorder, lineWidth: 1)
                    )
            }
            .buttonStyle(.plain)

            Divider()
                .frame(height: 20)

            Text("Fuente")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(secondaryText)

            Picker("", selection: $selectedFontFamily) {
                ForEach(availableFonts, id: \.self) { family in
                    Text(family).tag(family)
                }
            }
            .labelsHidden()
            .frame(width: 220)

            Text("Tamaño")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(secondaryText)

            HStack(spacing: 6) {
                Slider(
                    value: $fontSize,
                    in: 10...40,
                    step: 1
                )
                .frame(width: 150)

                Text("\(Int(fontSize)) pt")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(primaryText)
                    .frame(width: 48, alignment: .leading)
            }

            Spacer()

            Button {
                cycleAppearanceMode()
            } label: {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(secondaryActionBackground)
                        .frame(width: 32, height: 32)

                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .stroke(secondaryActionBorder, lineWidth: 1)
                        .frame(width: 32, height: 32)

                    Circle()
                        .fill(themeIconTint.opacity(themeIconGlowOpacity))
                        .frame(width: 28, height: 28)
                        .blur(radius: 10)

                    Image(systemName: selectedAppearanceMode.symbolName)
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(themeIconTint)
                        .scaleEffect(themeIconScale)
                        .rotationEffect(.degrees(themeIconRotation))
                }
                .frame(width: 32, height: 32)
                .contentShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            }
            .buttonStyle(.borderless)
            .help("Tema actual: \(selectedAppearanceMode.title). Click para cambiar a \(selectedAppearanceMode.next.title).")

            Button {
                openSettings()
            } label: {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(secondaryActionBackground)
                        .frame(width: 32, height: 32)

                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .stroke(secondaryActionBorder, lineWidth: 1)
                        .frame(width: 32, height: 32)

                    Image(systemName: "gearshape")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(controlAccent)
                }
            }
            .buttonStyle(.borderless)
            .help("Configuracion")

            Button {
                exportPDF()
            } label: {
                Text("Exportar PDF")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(primaryActionForeground)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(primaryActionBackground)
                    )
            }
            .buttonStyle(.plain)
            .disabled(document.rawMarkdown.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            .opacity(document.rawMarkdown.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? 0.55 : 1)
        }
    }

    private var renderedHTML: String {
        MarkdownHTMLRenderer.renderDocument(
            markdown: document.rawMarkdown,
            fontFamily: effectiveFontFamily,
            baseFontSize: fontSize,
            appearanceMode: selectedAppearanceMode
        )
    }

    private var selectedAppearanceMode: AppAppearanceMode {
        AppAppearanceMode(rawValue: appearanceModeRawValue) ?? .system
    }

    @MainActor
    private func cycleAppearanceMode() {
        let nextMode = selectedAppearanceMode.next

        withAnimation(.easeIn(duration: 0.12)) {
            themeIconScale = 0.84
            themeIconRotation -= 12
            themeIconGlowOpacity = 0.22
        }

        appearanceModeRawValue = nextMode.rawValue
        AppAppearanceController.apply(nextMode)

        withAnimation(.spring(response: 0.32, dampingFraction: 0.58)) {
            themeIconScale = 1.14
            themeIconRotation += 132
        }

        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 180_000_000)
            withAnimation(.spring(response: 0.28, dampingFraction: 0.72)) {
                themeIconScale = 1.0
                themeIconGlowOpacity = 0.0
            }
        }
    }

    private var resolvedColorScheme: ColorScheme {
        switch selectedAppearanceMode {
        case .system:
            return colorScheme
        case .light:
            return .light
        case .dark:
            return .dark
        }
    }

    private var windowBackground: Color {
        BrandChrome.windowBackground(for: resolvedColorScheme)
    }

    private var chromeBackground: Color {
        BrandChrome.chromeBackground(for: resolvedColorScheme)
    }

    private var chromeAccentBackground: Color {
        BrandChrome.chromeAccentBackground(for: resolvedColorScheme)
    }

    private var dividerColor: Color {
        BrandChrome.divider(for: resolvedColorScheme)
    }

    private var primaryText: Color {
        BrandChrome.primaryText(for: resolvedColorScheme)
    }

    private var secondaryText: Color {
        BrandChrome.secondaryText(for: resolvedColorScheme)
    }

    private var mutedText: Color {
        BrandChrome.mutedText(for: resolvedColorScheme)
    }

    private var controlAccent: Color {
        BrandChrome.interactiveAccent(for: resolvedColorScheme)
    }

    private var themeIconTint: Color {
        switch selectedAppearanceMode {
        case .system:
            return controlAccent
        case .light:
            return BrandChrome.lightModeTeal
        case .dark:
            return BrandChrome.violet
        }
    }

    private var primaryActionBackground: Color {
        BrandChrome.primaryActionBackground(for: resolvedColorScheme)
    }

    private var primaryActionForeground: Color {
        BrandChrome.primaryActionForeground(for: resolvedColorScheme)
    }

    private var secondaryActionBackground: Color {
        BrandChrome.secondaryActionBackground(for: resolvedColorScheme)
    }

    private var secondaryActionBorder: Color {
        BrandChrome.secondaryActionBorder(for: resolvedColorScheme)
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

    @MainActor
    private func openSettings() {
        SettingsWindowController.shared.show()
    }
}
