import AppKit
import SwiftUI

@MainActor
final class SettingsWindowController {
    static let shared = SettingsWindowController()

    private var window: NSWindow?

    func show() {
        let window = existingWindow()
        window.makeKeyAndOrderFront(nil)
        window.center()
        NSApp.activate(ignoringOtherApps: true)
    }

    private func existingWindow() -> NSWindow {
        if let window {
            return window
        }

        let hostingController = NSHostingController(rootView: SettingsView())
        let window = NSWindow(contentViewController: hostingController)
        window.title = "Configuracion"
        window.styleMask = [.titled, .closable, .miniaturizable]
        window.setContentSize(NSSize(width: 620, height: 520))
        window.isReleasedWhenClosed = false
        window.tabbingMode = .disallowed
        self.window = window
        return window
    }
}
