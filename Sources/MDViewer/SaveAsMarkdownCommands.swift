import SwiftUI

private struct SaveAsMarkdownActionKey: FocusedValueKey {
    typealias Value = SearchCommandAction
}

extension FocusedValues {
    var saveAsMarkdownAction: SearchCommandAction? {
        get { self[SaveAsMarkdownActionKey.self] }
        set { self[SaveAsMarkdownActionKey.self] = newValue }
    }
}

struct SaveAsMarkdownCommands: Commands {
    @FocusedValue(\.saveAsMarkdownAction) private var saveAsMarkdownAction

    var body: some Commands {
        CommandMenu("Archivo") {
            Button("Guardar como Markdown") {
                saveAsMarkdownAction?()
            }
            .keyboardShortcut("S", modifiers: [.command, .shift])
            .disabled(saveAsMarkdownAction == nil)
        }
    }
}
