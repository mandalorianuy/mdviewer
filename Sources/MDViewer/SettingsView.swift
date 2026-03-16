import AppKit
import SwiftUI

struct SettingsView: View {
    @AppStorage(AppPreferenceKey.selectedFontFamily) private var selectedFontFamily = AppPreferenceDefault.fontFamily
    @AppStorage(AppPreferenceKey.fontSize) private var fontSize = AppPreferenceDefault.fontSize
    @AppStorage(AppPreferenceKey.preferTabbedWindows) private var preferTabbedWindows = AppPreferenceDefault.preferTabbedWindows
    @AppStorage(AppPreferenceKey.appearanceMode) private var appearanceModeRawValue = AppPreferenceDefault.appearanceMode

    @State private var isUpdatingAssociation = false
    @State private var associationStatus = "Consultando asociacion actual..."
    @State private var associationIsCurrent = false

    private let availableFonts = NSFontManager.shared.availableFontFamilies.sorted()
    private var selectedAppearanceMode: Binding<AppAppearanceMode> {
        Binding(
            get: { AppAppearanceMode(rawValue: appearanceModeRawValue) ?? .system },
            set: { newValue in
                appearanceModeRawValue = newValue.rawValue
                Task { @MainActor in
                    AppAppearanceController.apply(newValue)
                }
            }
        )
    }

    var body: some View {
        Form {
            Section("Apariencia") {
                Picker("Tema", selection: selectedAppearanceMode) {
                    ForEach(AppAppearanceMode.allCases) { appearanceMode in
                        Text(appearanceMode.title).tag(appearanceMode)
                    }
                }
                .pickerStyle(.segmented)
                .frame(maxWidth: 300)

                Text("System sigue la apariencia de macOS. Light y Dark fuerzan el estilo tanto en la app como en el documento renderizado.")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }

            Section("Lectura") {
                Picker("Fuente por defecto", selection: $selectedFontFamily) {
                    ForEach(availableFonts, id: \.self) { family in
                        Text(family).tag(family)
                    }
                }
                .frame(maxWidth: 360)

                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Text("Tamano base")
                        Slider(value: $fontSize, in: 10...40, step: 1)
                            .frame(width: 220)
                        Text("\(Int(fontSize)) pt")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(.secondary)
                    }
                }
            }

            Section("Ventanas") {
                Toggle("Abrir documentos nuevos en tabs", isOn: $preferTabbedWindows)
                Text("Cuando esta opcion esta activa, los proximos archivos Markdown se agrupan en pestanas en lugar de ventanas separadas.")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }

            Section("Asociacion de archivos") {
                HStack(alignment: .center, spacing: 12) {
                    Circle()
                        .fill(associationIsCurrent ? Color.green : Color.orange)
                        .frame(width: 10, height: 10)

                    Text(associationStatus)
                        .foregroundStyle(.secondary)
                }

                Button(associationIsCurrent ? "MDViewer ya esta asociado a .md" : "Asociar archivos .md con MDViewer") {
                    Task {
                        await associateMarkdownFiles()
                    }
                }
                .disabled(isUpdatingAssociation || associationIsCurrent)

                Text("macOS puede pedirte confirmacion para cambiar la app por defecto de archivos Markdown.")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .padding(20)
        .frame(width: 560)
        .task {
            if !availableFonts.contains(selectedFontFamily) {
                selectedFontFamily = availableFonts.first ?? "Helvetica"
            }
            await refreshAssociationStatus()
        }
    }

    @MainActor
    private func associateMarkdownFiles() async {
        isUpdatingAssociation = true
        associationStatus = "Solicitando a macOS que asocie .md con MDViewer..."

        do {
            try await MarkdownAssociationService.setMDViewerAsDefault()
            await refreshAssociationStatus()
        } catch {
            associationIsCurrent = false
            associationStatus = "No se pudo asociar .md automaticamente: \(error.localizedDescription)"
        }

        isUpdatingAssociation = false
    }

    @MainActor
    private func refreshAssociationStatus() async {
        let currentURL = MarkdownAssociationService.currentDefaultApplicationURL()

        if MarkdownAssociationService.isMDViewerDefaultHandler() {
            associationIsCurrent = true
            associationStatus = "MDViewer ya es la app por defecto para archivos Markdown."
            return
        }

        associationIsCurrent = false

        if let currentURL {
            let appName = FileManager.default.displayName(atPath: currentURL.path)
            associationStatus = "La app actual para Markdown es \(appName)."
        } else {
            associationStatus = "No hay una app por defecto definida para Markdown."
        }
    }
}
