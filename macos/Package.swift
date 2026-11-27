// swift-tools-version: 6.2
import PackageDescription
import Foundation
let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().path

let package = Package(
  name: "Menagerie",
  platforms: [.macOS("27.0")],
  products: [.executable(name: "Menagerie", targets: ["Menagerie"])],
  dependencies: [
    .package(url: "https://github.com/jpsim/Yams.git", from: "6.1.0"),
    .package(url: "https://github.com/migueldeicaza/SwiftTerm.git", revision: "a480ce66166c9d23798580c1d4d845434b97d24a"),

  ],
  targets: [
    .systemLibrary(name: "CGhostty"),
    .target(name: "GhosttyBridge", dependencies: ["CGhostty"], cSettings: [.unsafeFlags(["-I" + root + "/target/ghostty/include"])], linkerSettings: [.unsafeFlags(["-L" + root + "/target/ghostty/lib"])]),
    .target(name: "AdminBridge", linkerSettings: [.linkedFramework("Security")]),
    .target(name: "MenagerieCore", dependencies: ["Yams", "AdminBridge", "GhosttyBridge"]),
    .executableTarget(name: "Menagerie", dependencies: ["MenagerieCore", "SwiftTerm"]),
    .testTarget(name: "MenagerieCoreTests", dependencies: ["MenagerieCore", "SwiftTerm"]),
  ]
)
