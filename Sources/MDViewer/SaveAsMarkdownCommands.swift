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

struct SaveAsMarkdownCommands: Commands {
    @FocusedValue(\.saveAsMarkdownAction) private var action: SaveAsMarkdownAction?

    var body: some Commands {
        CommandGroup(after: .saveItem) {
            Button("Guardar como Markdown") {
                action?.handler()
            }
            .keyboardShortcut("S", modifiers: [.command, .shift])
        }
    }
}
