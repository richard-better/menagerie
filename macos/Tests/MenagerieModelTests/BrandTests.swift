import AppKit
import Testing

@MainActor @Test func menuBarMarkDecodesAsANativeImage() throws {
  let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
  let image = try #require(
    NSImage(contentsOf: root.appendingPathComponent("assets/brand/menubar.png")))
  #expect(image.size.width > 0 && image.size.height > 0)
}
