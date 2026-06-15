// swift-tools-version: 6.2

import PackageDescription

let package = Package(
    name: "MDViewer",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(name: "MDViewer", targets: ["MDViewer"])
    ],
    dependencies: [
        .package(url: "https://github.com/stackotter/Down-gfm", from: "0.12.0")
    ],
    targets: [
        .executableTarget(
            name: "MDViewer",
            dependencies: [
                .product(name: "Down", package: "Down-gfm")
            ]
        ),
        .testTarget(
            name: "MDViewerTests",
            dependencies: [
                .target(name: "MDViewer"),
                .product(name: "Down", package: "Down-gfm")
            ]
        )
    ]
)
