import SwiftUI

@main
struct MDViewerApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate

    var body: some Scene {
        DocumentGroup(newDocument: { MarkdownFileDocument() }) { file in
            ContentView(document: file.document)
                .frame(minWidth: 720, minHeight: 520)
        }
        .commands {
            DocumentSearchCommands()
            MarkdownFileCommands()
        }

        Settings {
            SettingsView()
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    private var defaultsObserver: NSObjectProtocol?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSWindow.allowsAutomaticWindowTabbing = true
        AppAppearanceController.applyCurrentPreference()
        defaultsObserver = NotificationCenter.default.addObserver(
            forName: UserDefaults.didChangeNotification,
            object: UserDefaults.standard,
            queue: .main
        ) { _ in
            Task { @MainActor in
                AppAppearanceController.applyCurrentPreference()
            }
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        if let defaultsObserver {
            NotificationCenter.default.removeObserver(defaultsObserver)
        }
    }
}
