import Foundation

struct MarkdownRenderRequest: Hashable {
    let markdown: String
    let fontFamily: String
    let baseFontSize: Double
    let appearanceMode: AppAppearanceMode
}

actor MarkdownRenderPipeline {
    static let shared = MarkdownRenderPipeline()

    private var cache: [MarkdownRenderRequest: String] = [:]

    func render(_ request: MarkdownRenderRequest) async -> String {
        if let cachedHTML = cache[request] {
            return cachedHTML
        }

        let html = await Task.detached(priority: .userInitiated) {
            MarkdownHTMLRenderer.renderDocument(
                markdown: request.markdown,
                fontFamily: request.fontFamily,
                baseFontSize: request.baseFontSize,
                appearanceMode: request.appearanceMode
            )
        }.value

        cache[request] = html
        return html
    }
}
