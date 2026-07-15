import Foundation

enum HTMLExporter {
    static func export(html: String, outputURL: URL) throws {
        let standaloneHTML = """
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <title>Documento exportado</title>
            <style>
                body { font-family: -apple-system, BlinkMacSystemFont, sans-serif; line-height: 1.6; max-width: 800px; margin: 40px auto; padding: 0 20px; }
                img { max-width: 100%; }
                pre { background: #f4f4f4; padding: 10px; border-radius: 6px; overflow-x: auto; }
                code { font-family: Menlo, monospace; }
                table { border-collapse: collapse; width: 100%; }
                th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
            </style>
        </head>
        <body>
        \(html)
        </body>
        </html>
        """
        try standaloneHTML.write(to: outputURL, atomically: true, encoding: .utf8)
    }
}
