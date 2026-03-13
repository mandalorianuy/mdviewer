import AppKit
import UniformTypeIdentifiers

@MainActor
enum MarkdownAssociationService {
    static func currentDefaultApplicationURL() -> URL? {
        NSWorkspace.shared.urlForApplication(toOpen: .mdviewerMarkdown)
    }

    static func isMDViewerDefaultHandler() -> Bool {
        guard
            let defaultAppURL = currentDefaultApplicationURL(),
            let defaultBundleID = Bundle(url: defaultAppURL)?.bundleIdentifier,
            let currentBundleID = Bundle.main.bundleIdentifier
        else {
            return false
        }

        return defaultBundleID == currentBundleID
    }

    static func setMDViewerAsDefault() async throws {
        try await NSWorkspace.shared.setDefaultApplication(at: Bundle.main.bundleURL, toOpen: .mdviewerMarkdown)
    }
}
