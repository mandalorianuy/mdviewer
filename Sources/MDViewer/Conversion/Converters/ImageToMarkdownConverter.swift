import Foundation
import ImageIO
import Vision

struct ImageToMarkdownConverter: DocumentConverter {
    let supportedExtensions: [String] = ["jpg", "jpeg", "png", "heic", "tiff", "tif", "webp"]

    func convert(_ url: URL) throws -> MarkdownConversionResult {
        guard FileManager.default.isReadableFile(atPath: url.path) else {
            throw ConversionError.fileNotReadable
        }

        guard let source = CGImageSourceCreateWithURL(url as CFURL, nil) else {
            throw ConversionError.conversionFailed(reason: "No se pudo leer la imagen")
        }

        let metadata = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any]
        let exif = metadata?[kCGImagePropertyExifDictionary as String] as? [String: Any]
        let tiff = metadata?[kCGImagePropertyTIFFDictionary as String] as? [String: Any]

        var resultMetadata: [String: String] = [:]
        if let attributes = try? FileManager.default.attributesOfItem(atPath: url.path),
           let fileSize = attributes[.size] as? NSNumber {
            resultMetadata["fileSize"] = fileSize.stringValue
        }
        if let pixelWidth = metadata?[kCGImagePropertyPixelWidth as String],
           let pixelHeight = metadata?[kCGImagePropertyPixelHeight as String] {
            resultMetadata["dimensions"] = "\(formatValue(pixelWidth)) x \(formatValue(pixelHeight))"
        }

        let exifTable = buildEXIFTable(metadata: metadata, exif: exif, tiff: tiff)
        var warnings: [String] = []

        var ocrText: String?
        if let cgImage = CGImageSourceCreateImageAtIndex(source, 0, nil) {
            ocrText = extractText(from: cgImage)
            warnings.append("El texto se extrajo con OCR y puede contener errores.")
        }

        var markdown = ""
        if !exifTable.isEmpty {
            markdown += exifTable
        }

        if let ocrText = ocrText, !ocrText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            if !markdown.isEmpty {
                markdown += "\n\n"
            }
            markdown += "## Texto detectado\n\n```\n\(ocrText)\n```"
        }

        if markdown.isEmpty {
            let fallbackMessage = "_No se detectó metadata ni texto en la imagen._"
            warnings.append("No se detectó metadata ni texto en la imagen.")
            return MarkdownConversionResult(
                markdown: fallbackMessage,
                sourceFormat: "Imagen",
                title: url.lastPathComponent,
                warnings: warnings,
                metadata: resultMetadata
            )
        }

        return MarkdownConversionResult(
            markdown: markdown,
            sourceFormat: "Imagen",
            title: url.lastPathComponent,
            warnings: warnings,
            metadata: resultMetadata
        )
    }

    // MARK: - EXIF table

    private func buildEXIFTable(
        metadata: [String: Any]?,
        exif: [String: Any]?,
        tiff: [String: Any]?
    ) -> String {
        var rows: [(String, String)] = []

        if let dateTime = exif?[kCGImagePropertyExifDateTimeOriginal as String] as? String {
            rows.append(("Fecha", dateTime))
        }

        let make = tiff?[kCGImagePropertyTIFFMake as String] as? String
        let model = tiff?[kCGImagePropertyTIFFModel as String] as? String
        if make != nil || model != nil {
            let camera = [make, model].compactMap { $0 }.joined(separator: " ")
            rows.append(("Cámara", camera))
        }

        if let iso = exif?[kCGImagePropertyExifISOSpeedRatings as String] {
            rows.append(("ISO", formatValue(iso)))
        }

        if let fNumber = exif?[kCGImagePropertyExifFNumber as String] {
            rows.append(("Apertura", "f/\(formatValue(fNumber))"))
        }

        if let exposureTime = exif?[kCGImagePropertyExifExposureTime as String] {
            rows.append(("Velocidad", formatExposureTime(exposureTime)))
        }

        if let flash = exif?[kCGImagePropertyExifFlash as String] {
            rows.append(("Flash", flashDescription(flash)))
        }

        guard !rows.isEmpty else { return "" }

        if let pixelWidth = metadata?[kCGImagePropertyPixelWidth as String],
           let pixelHeight = metadata?[kCGImagePropertyPixelHeight as String] {
            rows.append(("Dimensiones", "\(formatValue(pixelWidth)) x \(formatValue(pixelHeight))"))
        }

        var table = "| Campo | Valor |\n"
        table += "|---|---|\n"
        for (field, value) in rows {
            table += "| \(field) | \(value) |\n"
        }
        return table.trimmingCharacters(in: .newlines)
    }

    private func formatValue(_ value: Any) -> String {
        if let number = value as? NSNumber {
            if number == number.intValue as NSNumber {
                return number.stringValue
            }
            return String(format: "%g", number.doubleValue)
        }
        return String(describing: value)
    }

    private func formatExposureTime(_ value: Any) -> String {
        if let number = value as? NSNumber, number.doubleValue > 0 {
            let seconds = number.doubleValue
            let denominator = round(1.0 / seconds)
            if denominator > 1 {
                return "1/\(Int(denominator)) s"
            }
            return "\(formatValue(value)) s"
        }
        return "\(formatValue(value)) s"
    }

    private func flashDescription(_ value: Any) -> String {
        guard let number = value as? NSNumber else {
            return formatValue(value)
        }
        switch number.intValue {
        case 0: return "No disparado"
        case 1: return "Disparado"
        case 5: return "Disparado, sin retorno de luz detectado"
        case 7: return "Disparado, con retorno de luz"
        case 9: return "Disparado, modo manual"
        case 16: return "Modo de supresión de ojos rojos"
        case 24: return "Disparado, modo de supresión de ojos rojos"
        default: return formatValue(value)
        }
    }

    // MARK: - OCR

    private func extractText(from cgImage: CGImage) -> String? {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate

        let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])

        do {
            try handler.perform([request])
        } catch {
            return nil
        }

        guard let results = request.results else {
            return nil
        }

        let text = results.compactMap { observation in
            observation.topCandidates(1).first?.string
        }.joined(separator: "\n")

        return text.isEmpty ? nil : text
    }
}
