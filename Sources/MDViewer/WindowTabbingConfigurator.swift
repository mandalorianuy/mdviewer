import AppKit
import SwiftUI

struct WindowTabbingConfigurator: NSViewRepresentable {
    let preferTabbedWindows: Bool

    func makeNSView(context: Context) -> WindowObserverView {
        let view = WindowObserverView()
        view.preferTabbedWindows = preferTabbedWindows
        return view
    }

    func updateNSView(_ nsView: WindowObserverView, context: Context) {
        nsView.preferTabbedWindows = preferTabbedWindows
        nsView.applyWindowPreferencesIfPossible()
    }

    final class WindowObserverView: NSView {
        var preferTabbedWindows = false

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            applyWindowPreferencesIfPossible()
        }

        func applyWindowPreferencesIfPossible() {
            guard let window else {
                return
            }

            DispatchQueue.main.async {
                window.tabbingIdentifier = "com.facundo.mdviewer.document"
                window.tabbingMode = self.preferTabbedWindows ? .preferred : .disallowed
            }
        }
    }
}
