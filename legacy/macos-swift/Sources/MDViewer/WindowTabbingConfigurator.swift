import AppKit
import SwiftUI

struct WindowTabbingConfigurator: NSViewRepresentable {
    private static let tabbingIdentifier = "com.facundo.mdviewer.document"

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

            DispatchQueue.main.async { [weak self, weak window] in
                guard let self, let window else {
                    return
                }

                window.tabbingIdentifier = WindowTabbingConfigurator.tabbingIdentifier
                window.tabbingMode = self.preferTabbedWindows ? .preferred : .disallowed

                if self.preferTabbedWindows {
                    self.attachWindowToExistingTabGroup(window)
                }
            }
        }

        private func attachWindowToExistingTabGroup(_ window: NSWindow) {
            guard window.isVisible else {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.08) { [weak self, weak window] in
                    guard let self, let window else {
                        return
                    }
                    self.attachWindowToExistingTabGroup(window)
                }
                return
            }

            guard window.tabbedWindows?.count ?? 1 <= 1 else {
                return
            }

            guard let targetWindow = NSApp.windows.first(where: {
                $0 !== window &&
                $0.isVisible &&
                !$0.isMiniaturized &&
                $0.tabbingIdentifier == WindowTabbingConfigurator.tabbingIdentifier
            }) else {
                return
            }

            targetWindow.addTabbedWindow(window, ordered: .above)
            targetWindow.makeKeyAndOrderFront(nil)
        }
    }
}
