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

    var symbolName: String {
        switch self {
        case .system:
            return "circle.lefthalf.filled"
        case .light:
            return "sun.max"
        case .dark:
            return "moon.stars"
        }
    }

    var next: AppAppearanceMode {
        switch self {
        case .system:
            return .light
        case .light:
            return .dark
        case .dark:
            return .system
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
    static let cyberRaisedSurface = Color(hex: 0x242736)
    static let ironGray = Color(hex: 0x30302D)
    static let lightInk = Color(hex: 0x1A1A1A)
    static let cyberYellow = Color(hex: 0xFFEF34)
    static let deepTeal = Color(hex: 0x00B8A3)
    static let violet = Color(hex: 0x8B5CF6)
    static let lightModeYellow = Color(hex: 0xD4C800)
    static let lightModeTeal = Color(hex: 0x009688)
    static let lightModeViolet = Color(hex: 0x7C3AED)
    static let textPrimaryDark = Color(hex: 0xE8EAF0)
    static let textSecondaryDark = Color(hex: 0xC1C5D2)
    static let textMutedDark = Color(hex: 0x8B90A0)
    static let textPrimaryLight = Color(hex: 0x1A1A1A)
    static let textSecondaryLight = Color(hex: 0x4A4A4A)
    static let textMutedLight = Color(hex: 0x8B8B85)
    static let lightCanvas = Color(hex: 0xF5F5F0)
    static let lightSurface = Color.white
    static let lightSurfaceAlt = Color(hex: 0xEAEAE5)

    static func chromeBackground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberDarkSurface : lightSurface
    }

    static func chromeAccentBackground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberRaisedSurface : lightSurfaceAlt
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

    static func interactiveAccent(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? deepTeal : lightModeTeal
    }

    static func primaryActionBackground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberYellow : lightModeTeal
    }

    static func primaryActionForeground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberDark : Color.white
    }

    static func secondaryActionBorder(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? Color.white.opacity(0.08) : Color.black.opacity(0.10)
    }

    static func secondaryActionBackground(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberRaisedSurface.opacity(0.72) : lightSurface
    }

    static func selectionHighlight(for colorScheme: ColorScheme) -> Color {
        colorScheme == .dark ? cyberYellow : lightModeYellow
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
