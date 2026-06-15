import AppKit
import SwiftUI

struct ContentView: View {
    let document: MarkdownFileDocument

    @Environment(\.colorScheme) private var colorScheme
    @AppStorage(AppPreferenceKey.selectedFontFamily) private var selectedFontFamily = AppPreferenceDefault.fontFamily
    @AppStorage(AppPreferenceKey.fontSize) private var fontSize = AppPreferenceDefault.fontSize
    @AppStorage(AppPreferenceKey.preferTabbedWindows) private var preferTabbedWindows = AppPreferenceDefault.preferTabbedWindows
    @AppStorage(AppPreferenceKey.appearanceMode) private var appearanceModeRawValue = AppPreferenceDefault.appearanceMode
    @FocusState private var isSearchFieldFocused: Bool
    @State private var errorMessage: String?
    @State private var isConverting = false
    @State private var isWarningsExpanded = false
    @State private var renderedHTML = ""
    @State private var isRenderingDocument = false
    @State private var isSearchPresented = false
    @State private var searchQuery = ""
    @State private var searchRequest = DocumentSearchRequest.idle
    @State private var searchResult = DocumentSearchResult.empty
    @State private var searchToken = 0
    @State private var themeIconScale: CGFloat = 1.0
    @State private var themeIconRotation: Double = 0
    @State private var themeIconGlowOpacity = 0.0
    @State private var searchDebounceTask: Task<Void, Never>?

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

            conversionBar

            if isSearchPresented {
                searchBar
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(chromeAccentBackground)

                Rectangle()
                    .fill(dividerColor)
                    .frame(height: 1)
            }

            Group {
                if renderedHTML.isEmpty && isRenderingDocument {
                    loadingState
                } else {
                    MarkdownWebView(
                        html: renderedHTML,
                        searchRequest: searchRequest,
                        onSearchResult: { result in
                            searchResult = result
                        }
                    )
                }
            }
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
        .task(id: currentRenderRequest) {
            await renderDocument(for: currentRenderRequest)
        }
        .task {
            await runPendingConversionIfNeeded()
        }
        .onAppear {
            if !availableFonts.contains(selectedFontFamily) {
                selectedFontFamily = effectiveDefaultFont
            }
        }
        .onChange(of: searchQuery) { _ in
            scheduleSearchUpdate()
        }
        .onDisappear {
            searchDebounceTask?.cancel()
        }
        .onExitCommand {
            if isSearchPresented {
                dismissSearch()
            }
        }
        .focusedSceneValue(\.showFindAction, SearchCommandAction(handler: presentSearch))
        .focusedSceneValue(\.findNextAction, SearchCommandAction(handler: findNextMatch))
        .focusedSceneValue(\.findPreviousAction, SearchCommandAction(handler: findPreviousMatch))
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

    @ViewBuilder
    private var conversionBar: some View {
        if isConverting && document.conversionResult == nil {
            HStack(spacing: 8) {
                ProgressView()
                    .controlSize(.small)
                Text("Convirtiendo...")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(secondaryText)
                Spacer()
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(chromeAccentBackground)

            Rectangle()
                .fill(dividerColor)
                .frame(height: 1)
        } else if let result = document.conversionResult {
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
                            isWarningsExpanded.toggle()
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

                if isWarningsExpanded && !result.warnings.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(result.warnings.enumerated()), id: \.offset) { index, warning in
                            Text("\(index + 1). \(warning)")
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

    private var controlsBar: some View {
        HStack(spacing: 14) {
            Button {
                pickFilesToOpen()
            } label: {
                Text("Abrir archivo")
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
            .help("Configuración")

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

    private var searchBar: some View {
        HStack(spacing: 10) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(controlAccent)

            TextField("Buscar en el documento", text: $searchQuery)
                .textFieldStyle(.plain)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(primaryText)
                .focused($isSearchFieldFocused)
                .onSubmit {
                    findNextMatch()
                }

            if !searchQuery.isEmpty {
                Text(searchStatusText)
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(mutedText)
                    .monospacedDigit()
            }

            Button {
                findPreviousMatch()
            } label: {
                Image(systemName: "chevron.up")
            }
            .buttonStyle(.borderless)
            .disabled(searchQuery.isEmpty || searchResult.totalMatches == 0)

            Button {
                findNextMatch()
            } label: {
                Image(systemName: "chevron.down")
            }
            .buttonStyle(.borderless)
            .disabled(searchQuery.isEmpty || searchResult.totalMatches == 0)

            Button {
                dismissSearch()
            } label: {
                Image(systemName: "xmark")
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 9)
        .background(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .fill(secondaryActionBackground)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(secondaryActionBorder, lineWidth: 1)
        )
    }

    private var loadingState: some View {
        VStack(spacing: 12) {
            ProgressView()
                .controlSize(.regular)
            Text("Renderizando documento…")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(mutedText)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var currentRenderRequest: MarkdownRenderRequest {
        MarkdownRenderRequest(
            markdown: document.rawMarkdown,
            fontFamily: effectiveFontFamily,
            baseFontSize: fontSize,
            appearanceMode: selectedAppearanceMode
        )
    }

    private var searchStatusText: String {
        guard !searchQuery.isEmpty else {
            return ""
        }

        guard searchResult.totalMatches > 0 else {
            return "0 resultados"
        }

        return "\(searchResult.currentIndex) / \(searchResult.totalMatches)"
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

    @MainActor
    private func presentSearch() {
        isSearchPresented = true
        searchResult = DocumentSearchResult.empty

        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 120_000_000)
            isSearchFieldFocused = true
        }
    }

    @MainActor
    private func dismissSearch() {
        searchDebounceTask?.cancel()
        isSearchPresented = false
        isSearchFieldFocused = false
        searchQuery = ""
        issueSearch(action: .clear)
    }

    @MainActor
    private func findNextMatch() {
        guard !searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            presentSearch()
            return
        }

        if !isSearchPresented {
            presentSearch()
        }

        issueSearch(action: .next)
    }

    @MainActor
    private func findPreviousMatch() {
        guard !searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            presentSearch()
            return
        }

        if !isSearchPresented {
            presentSearch()
        }

        issueSearch(action: .previous)
    }

    @MainActor
    private func scheduleSearchUpdate() {
        guard isSearchPresented else { return }

        searchDebounceTask?.cancel()
        let currentQuery = searchQuery

        searchDebounceTask = Task { @MainActor in
            try? await Task.sleep(nanoseconds: 140_000_000)
            guard !Task.isCancelled else { return }

            if currentQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                issueSearch(action: .clear)
            } else {
                issueSearch(action: .update)
            }
        }
    }

    @MainActor
    private func issueSearch(action: DocumentSearchRequest.Action) {
        searchToken += 1
        searchRequest = DocumentSearchRequest(
            query: searchQuery,
            action: action,
            token: searchToken
        )
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
        panel.allowedContentTypes = [
            .mdviewerMarkdown,
            .commaSeparatedText,
            .json,
            .xml,
            .html,
            .zip
        ]
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

        let exportRequest = currentRenderRequest

        Task { @MainActor in
            let html = await MarkdownRenderPipeline.shared.render(exportRequest)

            do {
                try PDFExporter.export(html: html, outputURL: outputURL)
                errorMessage = nil
            } catch {
                errorMessage = error.localizedDescription
            }
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

    @MainActor
    private func renderDocument(for request: MarkdownRenderRequest) async {
        isRenderingDocument = true
        let html = await MarkdownRenderPipeline.shared.render(request)
        guard !Task.isCancelled else {
            isRenderingDocument = false
            return
        }

        renderedHTML = html
        isRenderingDocument = false

        if isSearchPresented {
            let action: DocumentSearchRequest.Action = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .clear : .update
            issueSearch(action: action)
        }
    }

    private func runPendingConversionIfNeeded() async {
        guard let url = document.pendingConversionURL, document.rawMarkdown.isEmpty else { return }

        await MainActor.run {
            isConverting = true
            errorMessage = nil
        }

        do {
            let result = try await DocumentConversionService.shared.convert(url: url)
            await MainActor.run {
                document.rawMarkdown = result.markdown
                document.conversionResult = result
                document.pendingConversionURL = nil
            }
        } catch {
            await MainActor.run {
                errorMessage = error.localizedDescription
                document.pendingConversionURL = nil
            }
        }

        await MainActor.run {
            isConverting = false
        }
    }
}
