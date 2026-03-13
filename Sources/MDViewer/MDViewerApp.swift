import AppKit
import SwiftUI

@main
struct MDViewerApp: App {
    @StateObject private var appState = AppState()
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environmentObject(appState)
                .frame(minWidth: 720, minHeight: 520)
                .onAppear {
                    appDelegate.openHandler = { url in
                        appState.open(url: url)
                    }
                }
        }
        .commands {
            CommandGroup(replacing: .newItem) { }
            CommandMenu("Archivo") {
                Button("Abrir...") {
                    appState.pickFileToOpen()
                }
                .keyboardShortcut("o", modifiers: .command)

                Button("Exportar PDF...") {
                    appState.exportPDF()
                }
                .keyboardShortcut("e", modifiers: .command)
                .disabled(appState.rawMarkdown.isEmpty)
            }
        }
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate {
    var openHandler: ((URL) -> Void)?

    func application(_ sender: NSApplication, openFiles filenames: [String]) {
        guard let first = filenames.first else {
            sender.reply(toOpenOrPrint: .failure)
            return
        }

        openHandler?(URL(fileURLWithPath: first))
        sender.reply(toOpenOrPrint: .success)
    }
}
