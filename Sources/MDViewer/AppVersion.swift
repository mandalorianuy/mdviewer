import Foundation

struct AppVersion {
    let marketingVersion: String
    let buildNumber: String

    var displayString: String {
        "MDViewer v\(marketingVersion) (\(buildNumber))"
    }

    static var current: AppVersion {
        let info = Bundle.main.infoDictionary ?? [:]
        let marketingVersion = info["CFBundleShortVersionString"] as? String ?? "0.0.0"
        let buildNumber = info["CFBundleVersion"] as? String ?? "0"
        return AppVersion(marketingVersion: marketingVersion, buildNumber: buildNumber)
    }
}
