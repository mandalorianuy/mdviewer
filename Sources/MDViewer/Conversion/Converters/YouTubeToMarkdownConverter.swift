import Foundation

// LIMITATIONS:
// - YouTube page structure may change; transcript extraction relies on ytInitialPlayerResponse.
// - Network fetch uses Data(contentsOf:) synchronously; failures fall back to title/description/URL.
// - Only simple XML caption extraction is supported (text/p element text nodes).
// - HTML entities are decoded only for common cases.
struct YouTubeToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["url", "webloc"]

    private let urlContentProvider: @Sendable (URL) throws -> Data

    init(urlContentProvider: @escaping @Sendable (URL) throws -> Data = { try Data(contentsOf: $0) }) {
        self.urlContentProvider = urlContentProvider
    }

    func canConvert(_ url: URL) -> Bool {
        let ext = url.pathExtension.lowercased()
        if supportedExtensions.contains(ext) {
            return true
        }
        if let host = url.host?.lowercased() {
            return host.contains("youtube.com") || host.contains("youtu.be")
        }
        return false
    }

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        let videoURL = try resolveYouTubeURL(from: url)
        guard isYouTubeURL(videoURL) else {
            throw ConversionError.conversionFailed(reason: "URL de YouTube no válida")
        }

        let html: String
        do {
            html = try fetchString(from: videoURL)
        } catch {
            return fallbackResult(for: videoURL, title: nil)
        }

        let title = extractTitle(from: html)
        let description = extractDescription(from: html)

        if let transcript = extractTranscript(from: html), !transcript.isEmpty {
            let markdown = transcriptMarkdown(title: title, transcript: transcript, url: videoURL)
            return MarkdownConversionResult(
                markdown: markdown,
                sourceFormat: "YouTube",
                title: title,
                warnings: ["La transcripción se obtuvo de YouTube y puede ser inexacta."]
            )
        }

        let markdown = fallbackMarkdown(title: title, description: description, url: videoURL)
        return MarkdownConversionResult(
            markdown: markdown,
            sourceFormat: "YouTube",
            title: title,
            warnings: ["La transcripción se obtuvo de YouTube y puede ser inexacta."]
        )
    }
}

private extension YouTubeToMarkdownConverter {
    func resolveYouTubeURL(from url: URL) throws -> URL {
        let ext = url.pathExtension.lowercased()
        if ext == "url" {
            return try extractURLFromURLFile(url)
        } else if ext == "webloc" {
            return try extractURLFromWebloc(url)
        } else {
            return url
        }
    }

    func extractURLFromURLFile(_ url: URL) throws -> URL {
        let content: String
        do {
            content = try String(contentsOf: url, encoding: .utf8)
        } catch {
            throw ConversionError.conversionFailed(reason: "URL de YouTube no válida")
        }

        let pattern = #"https?://(www\.)?(youtube\.com|youtu\.be)/[^\s\"<>]+"#
        guard let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive),
              let match = regex.firstMatch(in: content, options: [], range: NSRange(content.startIndex..., in: content)),
              let range = Range(match.range, in: content),
              let result = URL(string: String(content[range])) else {
            throw ConversionError.conversionFailed(reason: "URL de YouTube no válida")
        }
        return result
    }

    func extractURLFromWebloc(_ url: URL) throws -> URL {
        let data: Data
        do {
            data = try urlContentProvider(url)
        } catch {
            throw ConversionError.conversionFailed(reason: "URL de YouTube no válida")
        }

        guard let plist = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
              let urlString = plist["URL"] as? String,
              let result = URL(string: urlString) else {
            throw ConversionError.conversionFailed(reason: "URL de YouTube no válida")
        }
        return result
    }

    func isYouTubeURL(_ url: URL) -> Bool {
        guard let host = url.host?.lowercased() else { return false }
        return host.contains("youtube.com") || host.contains("youtu.be")
    }

    func fetchString(from url: URL) throws -> String {
        let data = try urlContentProvider(url)
        return String(data: data, encoding: .utf8) ?? ""
    }

    func extractTitle(from html: String) -> String? {
        let pattern = #"<title[^>]*>(.*?)</title>"#
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive, .dotMatchesLineSeparators]),
              let match = regex.firstMatch(in: html, options: [], range: NSRange(html.startIndex..., in: html)),
              let range = Range(match.range(at: 1), in: html) else {
            return nil
        }
        var title = String(html[range])
        title = title.replacingOccurrences(of: " - YouTube", with: "")
        title = decodeCommonHTMLEntities(title)
        title = title.trimmingCharacters(in: .whitespacesAndNewlines)
        return title.isEmpty ? nil : title
    }

    func extractDescription(from html: String) -> String? {
        let pattern = #"<meta[^>]*name=\"description\"[^>]*content=\"([^\"]*)\"[^>]*>"#
        if let regex = try? NSRegularExpression(pattern: pattern, options: .caseInsensitive),
           let match = regex.firstMatch(in: html, options: [], range: NSRange(html.startIndex..., in: html)),
           let range = Range(match.range(at: 1), in: html) {
            let desc = decodeCommonHTMLEntities(String(html[range]))
            return desc.isEmpty ? nil : desc
        }

        if let json = extractInitialPlayerResponse(from: html),
           let data = json.data(using: .utf8),
           let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let details = obj["videoDetails"] as? [String: Any],
           let desc = details["shortDescription"] as? String {
            return desc.isEmpty ? nil : desc
        }

        return nil
    }

    func extractTranscript(from html: String) -> String? {
        guard let baseUrl = extractCaptionBaseURL(from: html) else { return nil }
        do {
            let xml = try fetchString(from: baseUrl)
            return parseTranscriptXML(xml)
        } catch {
            return nil
        }
    }

    func extractCaptionBaseURL(from html: String) -> URL? {
        guard let json = extractInitialPlayerResponse(from: html) else { return nil }
        guard let data = json.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let captions = obj["captions"] as? [String: Any],
              let tracklist = captions["playerCaptionsTracklistRenderer"] as? [String: Any],
              let tracks = tracklist["captionTracks"] as? [[String: Any]],
              let firstTrack = tracks.first,
              let baseUrlString = firstTrack["baseUrl"] as? String else {
            return nil
        }
        return URL(string: baseUrlString)
    }

    func extractInitialPlayerResponse(from html: String) -> String? {
        let patterns = [
            #"var ytInitialPlayerResponse = (\{.*?\});\s*</script>"#,
            #"window\[\"ytInitialPlayerResponse\"\]\s*=\s*(\{.*?\});\s*</script>"#
        ]
        for pattern in patterns {
            guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive, .dotMatchesLineSeparators]),
                  let match = regex.firstMatch(in: html, options: [], range: NSRange(html.startIndex..., in: html)),
                  match.numberOfRanges > 1,
                  let range = Range(match.range(at: 1), in: html) else {
                continue
            }
            return String(html[range])
        }
        return nil
    }

    func parseTranscriptXML(_ xml: String) -> String? {
        let pattern = #"<(text|p)[^>]*>(.*?)</\1>"#
        guard let regex = try? NSRegularExpression(pattern: pattern, options: [.caseInsensitive, .dotMatchesLineSeparators]) else {
            return nil
        }
        let matches = regex.matches(in: xml, options: [], range: NSRange(xml.startIndex..., in: xml))
        var texts: [String] = []
        for match in matches {
            guard let range = Range(match.range(at: 2), in: xml) else { continue }
            var text = String(xml[range])
            text = text.replacingOccurrences(of: "<[^>]+>", with: "", options: .regularExpression, range: nil)
            text = decodeCommonHTMLEntities(text)
            text = text.trimmingCharacters(in: .whitespacesAndNewlines)
            if !text.isEmpty {
                texts.append(text)
            }
        }
        return texts.isEmpty ? nil : texts.joined(separator: " ")
    }

    func decodeCommonHTMLEntities(_ text: String) -> String {
        var result = text
        let entities = [
            "&amp;": "&",
            "&lt;": "<",
            "&gt;": ">",
            "&quot;": "\"",
            "&#39;": "'",
            "&#x27;": "'",
            "&#x2F;": "/",
            "&#x3D;": "=",
            "&#x22;": "\""
        ]
        for (entity, replacement) in entities {
            result = result.replacingOccurrences(of: entity, with: replacement)
        }
        return result
    }

    func fallbackResult(for url: URL, title: String?) -> MarkdownConversionResult {
        let displayTitle = title ?? "Video de YouTube"
        let markdown = "# \(displayTitle)\n\nURL: \(url.absoluteString)"
        return MarkdownConversionResult(
            markdown: markdown,
            sourceFormat: "YouTube",
            title: displayTitle,
            warnings: ["La transcripción se obtuvo de YouTube y puede ser inexacta."]
        )
    }

    func fallbackMarkdown(title: String?, description: String?, url: URL) -> String {
        var parts: [String] = []
        if let title = title {
            parts.append("# \(title)")
        }
        if let description = description {
            parts.append(description)
        }
        parts.append("URL: \(url.absoluteString)")
        return parts.joined(separator: "\n\n")
    }

    func transcriptMarkdown(title: String?, transcript: String, url: URL) -> String {
        var parts: [String] = []
        if let title = title {
            parts.append("# \(title)")
        }
        parts.append(transcript)
        parts.append("URL: \(url.absoluteString)")
        return parts.joined(separator: "\n\n")
    }
}
