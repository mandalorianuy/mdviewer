import SwiftUI

struct SearchCommandAction {
    let handler: () -> Void

    func callAsFunction() {
        handler()
    }
}

private struct ShowFindActionKey: FocusedValueKey {
    typealias Value = SearchCommandAction
}

private struct FindNextActionKey: FocusedValueKey {
    typealias Value = SearchCommandAction
}

private struct FindPreviousActionKey: FocusedValueKey {
    typealias Value = SearchCommandAction
}

extension FocusedValues {
    var showFindAction: SearchCommandAction? {
        get { self[ShowFindActionKey.self] }
        set { self[ShowFindActionKey.self] = newValue }
    }

    var findNextAction: SearchCommandAction? {
        get { self[FindNextActionKey.self] }
        set { self[FindNextActionKey.self] = newValue }
    }

    var findPreviousAction: SearchCommandAction? {
        get { self[FindPreviousActionKey.self] }
        set { self[FindPreviousActionKey.self] = newValue }
    }
}

struct DocumentSearchCommands: Commands {
    @FocusedValue(\.showFindAction) private var showFindAction
    @FocusedValue(\.findNextAction) private var findNextAction
    @FocusedValue(\.findPreviousAction) private var findPreviousAction

    var body: some Commands {
        CommandMenu("Buscar") {
            Button("Buscar") {
                showFindAction?()
            }
            .keyboardShortcut("f", modifiers: .command)
            .disabled(showFindAction == nil)

            Button("Buscar siguiente") {
                findNextAction?()
            }
            .keyboardShortcut("g", modifiers: .command)
            .disabled(findNextAction == nil)

            Button("Buscar anterior") {
                findPreviousAction?()
            }
            .keyboardShortcut("g", modifiers: [.command, .shift])
            .disabled(findPreviousAction == nil)
        }
    }
}
