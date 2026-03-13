import SwiftUI

@main
struct MDViewerApp: App {
    var body: some Scene {
        DocumentGroup(viewing: MarkdownFileDocument.self) { file in
            ContentView(document: file.document)
                .frame(minWidth: 720, minHeight: 520)
        }
    }
}
