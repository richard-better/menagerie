// swift-tools-version: 6.2
import PackageDescription

let package = Package(
  name: "Menagerie",
  platforms: [.macOS("27.0")],
  products: [.executable(name: "Menagerie", targets: ["Menagerie"])],
  dependencies: [
    .package(url: "https://github.com/jpsim/Yams.git", from: "6.1.0"),
    .package(
      url: "https://github.com/migueldeicaza/SwiftTerm.git",
      revision: "a480ce66166c9d23798580c1d4d845434b97d24a"),
  ],
  targets: [
    .target(name: "AdminBridge", linkerSettings: [.linkedFramework("Security")]),
    .target(name: "MenagerieCore", dependencies: ["Yams", "AdminBridge"]),
    .executableTarget(name: "Menagerie", dependencies: ["MenagerieCore", "SwiftTerm"]),
    .testTarget(name: "MenagerieCoreTests", dependencies: ["MenagerieCore", "SwiftTerm"]),
  ]
)
