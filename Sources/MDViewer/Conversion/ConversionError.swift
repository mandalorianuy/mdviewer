import Foundation

enum ConversionError: Error, Sendable {
    case unsupportedFormat
    case fileNotReadable
    case conversionFailed(reason: String)
    case timeout
}

extension ConversionError: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .unsupportedFormat:
            return "Formato no soportado todavía."
        case .fileNotReadable:
            return "No se pudo leer el archivo."
        case .conversionFailed(let reason):
            return "Error de conversión: \(reason)"
        case .timeout:
            return "La conversión tardó demasiado."
        }
    }
}
