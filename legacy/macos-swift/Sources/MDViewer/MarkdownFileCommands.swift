import SwiftUI

struct SaveAsMarkdownAction {
    let handler: () -> Void
}

private struct SaveAsMarkdownActionKey: FocusedValueKey {
    typealias Value = SaveAsMarkdownAction
}

extension FocusedValues {
    var saveAsMarkdownAction: SaveAsMarkdownAction? {
        get { self[SaveAsMarkdownActionKey.self] }
        set { self[SaveAsMarkdownActionKey.self] = newValue }
    }
}

struct MarkdownFileCommands: Commands {
    @FocusedValue(\.saveAsMarkdownAction) private var saveAsMarkdownAction: SaveAsMarkdownAction?
    @FocusedValue(\.convertToMarkdownAction) private var convertToMarkdownAction: ConvertToMarkdownAction?

    var body: some Commands {
        CommandGroup(after: .saveItem) {
            Button("Guardar como Markdown") {
                saveAsMarkdownAction?.handler()
            }
            .keyboardShortcut("S", modifiers: [.command, .shift])
        }

        CommandGroup(after: .printItem) {
            Button("Convertir a Markdown…") {
                convertToMarkdownAction?.handler()
            }
            .disabled(convertToMarkdownAction == nil)
        }
    }
}
