import SwiftUI
import WebKit

struct DocumentSearchRequest: Equatable {
    enum Action: Equatable {
        case idle
        case update
        case next
        case previous
        case clear
    }

    let query: String
    let action: Action
    let token: Int

    static let idle = DocumentSearchRequest(query: "", action: .idle, token: 0)
}

struct DocumentSearchResult: Equatable {
    let currentIndex: Int
    let totalMatches: Int

    static let empty = DocumentSearchResult(currentIndex: 0, totalMatches: 0)
}

struct MarkdownWebView: NSViewRepresentable {
    let html: String
    let searchRequest: DocumentSearchRequest
    let onSearchResult: (DocumentSearchResult) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator(onSearchResult: onSearchResult)
    }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.defaultWebpagePreferences.allowsContentJavaScript = true

        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.setValue(false, forKey: "drawsBackground")
        webView.allowsMagnification = true
        webView.customUserAgent = "MDViewer/0.1"
        webView.navigationDelegate = context.coordinator
        context.coordinator.webView = webView
        return webView
    }

    func updateNSView(_ nsView: WKWebView, context: Context) {
        context.coordinator.onSearchResult = onSearchResult

        if context.coordinator.lastHTML != html {
            context.coordinator.lastHTML = html
            context.coordinator.lastAppliedSearchToken = nil
            context.coordinator.pendingSearchRequest = searchRequest
            context.coordinator.isPageLoaded = false
            nsView.loadHTMLString(html, baseURL: nil)
            return
        }

        context.coordinator.handle(searchRequest: searchRequest, in: nsView)
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        weak var webView: WKWebView?
        var lastHTML: String?
        var isPageLoaded = false
        var pendingSearchRequest: DocumentSearchRequest?
        var lastAppliedSearchToken: Int?
        var onSearchResult: (DocumentSearchResult) -> Void

        init(onSearchResult: @escaping (DocumentSearchResult) -> Void) {
            self.onSearchResult = onSearchResult
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            isPageLoaded = true

            if let pendingSearchRequest {
                self.pendingSearchRequest = nil
                handle(searchRequest: pendingSearchRequest, in: webView, force: true)
            }
        }

        func handle(searchRequest: DocumentSearchRequest, in webView: WKWebView, force: Bool = false) {
            guard isPageLoaded else {
                pendingSearchRequest = searchRequest
                return
            }

            guard force || lastAppliedSearchToken != searchRequest.token else {
                return
            }

            lastAppliedSearchToken = searchRequest.token

            let actionName: String
            switch searchRequest.action {
            case .idle:
                return
            case .update:
                actionName = "update"
            case .next:
                actionName = "next"
            case .previous:
                actionName = "previous"
            case .clear:
                actionName = "clear"
            }

            let escapedQuery = searchRequest.query
                .replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "\"", with: "\\\"")
                .replacingOccurrences(of: "\n", with: "\\n")

            let script = """
            window.__mdviewerSearchController.search("\(escapedQuery)", "\(actionName)");
            """

            webView.evaluateJavaScript(script) { [weak self] result, _ in
                guard let self else { return }

                guard
                    let dictionary = result as? [String: Any],
                    let currentIndex = dictionary["currentIndex"] as? Int,
                    let totalMatches = dictionary["totalMatches"] as? Int
                else {
                    self.onSearchResult(.empty)
                    return
                }

                self.onSearchResult(
                    DocumentSearchResult(
                        currentIndex: currentIndex,
                        totalMatches: totalMatches
                    )
                )
            }
        }
    }
}
