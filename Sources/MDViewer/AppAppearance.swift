import AppKit
import SwiftUI

enum AppAppearanceMode: String, CaseIterable, Identifiable {
    case system
    case light
    case dark

    var id: String { rawValue }

    var title: String {
        switch self {
        case .system:
            return "Sistema"
        case .light:
            return "Light"
        case .dark:
            return "Dark"
        }
    }

    var preferredColorScheme: ColorScheme? {
        switch self {
        case .system:
            return nil
        case .light:
            return .light
        case .dark:
            return .dark
        }
    }

    var nsAppearance: NSAppearance? {
        switch self {
        case .system:
            return nil
        case .light:
            return NSAppearance(named: .aqua)
        case .dark:
            return NSAppearance(named: .darkAqua)
        }
    }
}

enum AppAppearanceController {
    static func currentPreference() -> AppAppearanceMode {
        let rawValue = UserDefaults.standard.string(forKey: AppPreferenceKey.appearanceMode) ?? AppPreferenceDefault.appearanceMode
        return AppAppearanceMode(rawValue: rawValue) ?? .system
    }

    @MainActor
    static func applyCurrentPreference() {
        apply(currentPreference())
    }

    @MainActor
    static func apply(_ appearanceMode: AppAppearanceMode) {
        NSApp.appearance = appearanceMode.nsAppearance
    }
}

enum BrandChrome {
    static let cyberDark = Color(hex: 0x0F1117)
    static let cyberDarkSurface = Color(hex: 0x1A1D27)
    static let cyberMidSurface = Color(hex: 0x222633)
    static let ironGray = Color(hex: 0x30302D)
    static let cyberYellow = Color(hex: 0xFFEF34)
    static let deepTeal = Color(hex: 0x00B8A3)
    static let violet = Color(hex: 0x8B5CF6)
    static let textPrimaryDark = Color(hex: 0xE8EAF0)
    static let textSecondaryDark = Color(hex: 0xC1C5D2)
    static let textMutedDark = Color(hex: 0x8B90A0)
    static let textPrimaryLight = Color(hex: 0x0F1117)
    static let textSecondaryLight = Color(hex: 0x30302D)
    static let textMutedLight = Color(hex: 0x5C6475)
    static let lightCanvas = Color(hex: 0xF5F5F5)
    static let lightSurface = Color.white
    static let lightSurfaceAlt = Color(hex: 0xECEFF4)

    static func chromeBackground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberDarkSurface : lightSurface
    }

    static func chromeAccentBackground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberMidSurface : lightSurfaceAlt
    }

    static func windowBackground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberDark : lightCanvas
    }

    static func divider(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? Color.white.opacity(0.08) : Color.black.opacity(0.08)
    }

    static func primaryText(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? textPrimaryDark : textPrimaryLight
    }

    static func secondaryText(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? textSecondaryDark : textSecondaryLight
    }

    static func mutedText(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? textMutedDark : textMutedLight
    }
}

private extension Color {
    init(hex: Int, opacity: Double = 1.0) {
        let red = Double((hex >> 16) & 0xFF) / 255.0
        let green = Double((hex >> 8) & 0xFF) / 255.0
        let blue = Double(hex & 0xFF) / 255.0
        self.init(.sRGB, red: red, green: green, blue: blue, opacity: opacity)
    }
}
