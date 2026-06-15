import Foundation

enum ConversionError: Error {
    case unsupportedFormat
    case fileNotReadable
    case conversionFailed(underlying: Error)
    case timeout
}

extension ConversionError: LocalizedError {
    var errorDescription: String? {
        switch self {
        case .unsupportedFormat:
            return "Formato no soportado todavia."
        case .fileNotReadable:
            return "No se pudo leer el archivo."
        case .conversionFailed(let underlying):
            return "Error de conversion: \(underlying.localizedDescription)"
        case .timeout:
            return "La conversion tardo demasiado."
        }
    }
}
