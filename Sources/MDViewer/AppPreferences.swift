import Foundation

enum AppPreferenceKey {
    static let selectedFontFamily = "selectedFontFamily"
    static let fontSize = "fontSize"
    static let preferTabbedWindows = "preferTabbedWindows"
    static let appearanceMode = "appearanceMode"
}

enum AppPreferenceDefault {
    static let fontFamily = "Space Grotesk"
    static let fontSize = 16.0
    static let preferTabbedWindows = false
    static let appearanceMode = AppAppearanceMode.system.rawValue
}
