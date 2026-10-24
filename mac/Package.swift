// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PS5CeviriMac",
    platforms: [.macOS(.v15)],
    targets: [
        // Saf mantık: macOS framework'lerine bağımlı değil, unit test edilir.
        .target(name: "PS5CeviriCore"),
        .executableTarget(name: "ps5ceviri", dependencies: ["PS5CeviriCore"]),
        .testTarget(name: "PS5CeviriCoreTests", dependencies: ["PS5CeviriCore"]),
    ]
)
