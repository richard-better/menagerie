import AppKit
import SwiftUI

struct DotDrawing: NSViewRepresentable {
  struct Sample: Equatable {
    var at: Int64
    var value: Double
  }
  enum Content: Equatable {
    case meter(Double, Color)
    case history([Sample], Int64, Int)
  }
  let content: Content
  func makeNSView(context: Context) -> DrawingView { DrawingView() }
  func updateNSView(_ view: DrawingView, context: Context) {
    guard view.content != content else { return }
    view.content = content
    view.needsDisplay = true
  }
  final class DrawingView: NSView {
    var content: Content?
    override var isFlipped: Bool { true }
    override init(frame: NSRect) {
      super.init(frame: frame)
      setContentHuggingPriority(.defaultLow, for: .horizontal)
      setContentHuggingPriority(.defaultLow, for: .vertical)
    }
    required init?(coder: NSCoder) { fatalError("init(coder:) has not been implemented") }
    override func draw(_ dirtyRect: NSRect) {
      guard let context = NSGraphicsContext.current?.cgContext, let content else { return }
      context.clear(dirtyRect)
      switch content {
      case .meter(let fraction, let color):
        let count = max(1, Int(bounds.width / 6))
        let active = NSColor(color).cgColor
        let inactive = NSColor(Color(hex: 0x2f3b35)).cgColor
        for column in 0..<count {
          context.setFillColor(Double(column) / Double(count) < fraction ? active : inactive)
          for row in 0..<2 {
            context.addPath(
              CGPath(
                roundedRect: CGRect(x: column * 6, y: row * 6 + 2, width: 3, height: 3),
                cornerWidth: 0.6, cornerHeight: 0.6, transform: nil))
            context.fillPath()
          }
        }
      case .history(let samples, let now, let hours):
        let interval = Int64(hours) * 3_600_000
        let columns = max(1, Int(bounds.width / 6))
        var values: [Int: Double] = [:]
        for sample in samples {
          let age = now.subtractingReportingOverflow(sample.at)
          guard !age.overflow, (0...interval).contains(age.partialValue) else { continue }
          let column = Int((1 - Double(age.partialValue) / Double(interval)) * Double(columns - 1))
          values[column] = max(values[column] ?? 0, sample.value)
        }
        let scale = max(1, (values.values.max() ?? 0) * 1.15)
        for column in 0..<columns {
          let sample = values[column]
          context.setFillColor(NSColor(Palette.ink.opacity(sample == nil ? 0.15 : 0.65)).cgColor)
          let rows = sample.map { max(1, min(12, Int(($0 / scale * 12).rounded()))) } ?? 1
          for row in 0..<rows {
            context.fillEllipse(
              in: CGRect(x: column * 6, y: Int(bounds.height) - row * 6 - 4, width: 3, height: 3))
          }
        }
      }
    }
  }
}
