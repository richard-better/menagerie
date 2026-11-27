// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "Menagerie",
    platforms: [.macOS("27.0")],
    products: [.executable(name: "Menagerie", targets: ["Menagerie"])],
    dependencies: [.package(url: "https://github.com/jpsim/Yams.git", from: "6.1.0")],
    targets: [
        .target(name: "MenagerieCore", dependencies: ["Yams"]),
        .executableTarget(name: "Menagerie", dependencies: ["MenagerieCore"]),
        .testTarget(name: "MenagerieCoreTests", dependencies: ["MenagerieCore"])
    ]
)
