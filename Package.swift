// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "Typr",
    platforms: [.macOS(.v14)],
    dependencies: [
        .package(
            url: "https://github.com/moonshine-ai/moonshine-swift.git",
            exact: "0.0.69"
        )
    ],
    targets: [
        .executableTarget(
            name: "Typr",
            dependencies: [
                .product(name: "MoonshineVoice", package: "moonshine-swift")
            ],
            path: "macos",
            exclude: ["Info.plist"],
            sources: ["Typr.swift"],
            linkerSettings: [
                .linkedFramework("AppKit"),
                .linkedFramework("ApplicationServices"),
                .linkedFramework("AVFoundation"),
                .linkedFramework("CoreGraphics"),
                .linkedFramework("ServiceManagement"),
                .linkedFramework("SwiftUI")
            ]
        )
    ],
    swiftLanguageModes: [.v5]
)
