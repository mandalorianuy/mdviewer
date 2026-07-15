import AppKit
import UniformTypeIdentifiers

@MainActor
enum MarkdownAssociationService {
    /// Tipos de archivo que la app puede abrir y convertir a Markdown.
    /// Se mantiene sincronizado con `MarkdownFileDocument.readableContentTypes`.
    static let convertibleUTTypes: [UTType] = MarkdownFileDocument.readableContentTypes

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

    static func setMDViewerAsDefaultForConvertibleTypes() async throws {
        for type in convertibleUTTypes {
            try await NSWorkspace.shared.setDefaultApplication(at: Bundle.main.bundleURL, toOpen: type)
        }
    }
}
