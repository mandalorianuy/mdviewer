import SwiftUI

struct ContentView: View {
    @EnvironmentObject private var appState: AppState

    var body: some View {
        VStack(spacing: 0) {
            controlsBar
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .background(.ultraThinMaterial)

            Divider()

            MarkdownWebView(html: appState.renderedHTML)
            .background(Color(NSColor.textBackgroundColor))

            if let error = appState.errorMessage {
                Divider()
                Text(error)
                    .foregroundStyle(.red)
                    .font(.system(size: 12, weight: .medium))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .background(Color(NSColor.windowBackgroundColor))
            }
        }
    }

    private var controlsBar: some View {
        HStack(spacing: 14) {
            Button("Abrir .md") {
                appState.pickFileToOpen()
            }

            Divider()
                .frame(height: 20)

            Text("Fuente")
                .font(.system(size: 12, weight: .medium))

            Picker("", selection: Binding(
                get: { appState.selectedFontFamily },
                set: { appState.updateTypography(fontFamily: $0) }
            )) {
                ForEach(appState.availableFonts, id: \.self) { family in
                    Text(family).tag(family)
                }
            }
            .labelsHidden()
            .frame(width: 220)

            Text("Tamaño")
                .font(.system(size: 12, weight: .medium))

            HStack(spacing: 6) {
                Slider(
                    value: Binding(
                        get: { appState.fontSize },
                        set: { appState.updateTypography(size: $0) }
                    ),
                    in: 10...40,
                    step: 1
                )
                .frame(width: 150)

                Text("\(Int(appState.fontSize)) pt")
                    .font(.system(size: 12, weight: .semibold))
                    .frame(width: 48, alignment: .leading)
            }

            Spacer()

            Button("Exportar PDF") {
                appState.exportPDF()
            }
            .disabled(appState.rawMarkdown.isEmpty)
        }
    }
}
