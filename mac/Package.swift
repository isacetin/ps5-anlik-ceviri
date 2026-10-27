// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PS5CeviriMac",
    platforms: [.macOS(.v15)],
    products: [
        // Rust masaüstü uygulamasının (desktop/mac) bağladığı statik kütüphane.
        .library(name: "CeviriBridge", type: .static, targets: ["CeviriBridge"]),
    ],
    targets: [
        // Saf mantık: macOS framework'lerine bağımlı değil, unit test edilir.
        .target(name: "PS5CeviriCore"),
        // Ekran yakalama + OCR (ScreenCaptureKit, Vision).
        .target(name: "PS5CeviriMac", dependencies: ["PS5CeviriCore"]),
        // PS5CeviriMac'i Rust'a C ABI ile açar.
        .target(name: "CeviriBridge", dependencies: ["PS5CeviriMac"]),
        .executableTarget(name: "ps5ceviri", dependencies: ["PS5CeviriCore", "PS5CeviriMac"]),
        .testTarget(name: "PS5CeviriCoreTests", dependencies: ["PS5CeviriCore"]),
    ]
)
