import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct SettingsView: View {
    @AppStorage(AppPreferenceKey.selectedFontFamily) private var selectedFontFamily = AppPreferenceDefault.fontFamily
    @AppStorage(AppPreferenceKey.fontSize) private var fontSize = AppPreferenceDefault.fontSize
    @AppStorage(AppPreferenceKey.preferTabbedWindows) private var preferTabbedWindows = AppPreferenceDefault.preferTabbedWindows
    @AppStorage(AppPreferenceKey.appearanceMode) private var appearanceModeRawValue = AppPreferenceDefault.appearanceMode

    @State private var isUpdatingAssociation = false
    @State private var associationStatus = "Consultando asociación actual..."
    @State private var associationIsCurrent = false
    @State private var markdownIsAssociated = false

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
                        Text("Tamaño base")
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

            Section("Asociación de archivos") {
                HStack(alignment: .center, spacing: 12) {
                    Circle()
                        .fill(associationIsCurrent ? Color.green : Color.orange)
                        .frame(width: 10, height: 10)

                    Text(associationStatus)
                        .foregroundStyle(.secondary)
                }

                Button(markdownIsAssociated ? "MDViewer ya está asociado a .md" : "Asociar archivos .md con MDViewer") {
                    Task {
                        await associateMarkdownFiles()
                    }
                }
                .disabled(isUpdatingAssociation || markdownIsAssociated)

                Button("Asociar formatos convertibles con MDViewer") {
                    Task {
                        await associateConvertibleFiles()
                    }
                }
                .disabled(isUpdatingAssociation)

                Text("macOS puede pedirte confirmación para cambiar la app por defecto de archivos Markdown.")
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
            associationStatus = "No se pudo asociar .md automáticamente: \(error.localizedDescription)"
        }

        isUpdatingAssociation = false
    }

    @MainActor
    private func associateConvertibleFiles() async {
        isUpdatingAssociation = true
        associationStatus = "Solicitando asociación de formatos convertibles..."

        do {
            try await MarkdownAssociationService.setMDViewerAsDefaultForConvertibleTypes()
            await refreshAssociationStatus()
        } catch {
            associationIsCurrent = false
            associationStatus = "No se pudo asociar todos los formatos: \(error.localizedDescription)"
        }

        isUpdatingAssociation = false
    }

    @MainActor
    private func refreshAssociationStatus() async {
        let currentURL = MarkdownAssociationService.currentDefaultApplicationURL()
        let allTypes = MarkdownAssociationService.convertibleUTTypes

        let associatedTypes = allTypes.filter { type in
            isDefaultApplicationMDViewer(for: type)
        }

        let isMarkdownAssociated = associatedTypes.contains(.mdviewerMarkdown)
        let allAssociated = associatedTypes.count == allTypes.count

        markdownIsAssociated = isMarkdownAssociated

        if allAssociated {
            associationIsCurrent = true
            associationStatus = "MDViewer ya es la app por defecto para Markdown y formatos convertibles."
            return
        }

        associationIsCurrent = false

        if isMarkdownAssociated {
            associationStatus = "MDViewer está asociado a .md, pero no a todos los formatos convertibles."
        } else if let currentURL {
            let appName = FileManager.default.displayName(atPath: currentURL.path)
            associationStatus = "La app actual para Markdown es \(appName)."
        } else {
            associationStatus = "No hay una app por defecto definida para Markdown."
        }
    }

    @MainActor
    private func isDefaultApplicationMDViewer(for type: UTType) -> Bool {
        guard
            let defaultAppURL = NSWorkspace.shared.urlForApplication(toOpen: type),
            let defaultBundleID = Bundle(url: defaultAppURL)?.bundleIdentifier,
            let currentBundleID = Bundle.main.bundleIdentifier
        else {
            return false
        }

        return defaultBundleID == currentBundleID
    }
}
