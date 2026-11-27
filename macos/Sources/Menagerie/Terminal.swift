import AppKit
import CoreText
import MenagerieCore
import SwiftUI

@MainActor final class TerminalSession {
  private weak var cachedView: TerminalGridView?
  var view: TerminalGridView {
    if let cachedView { return cachedView }
    let view = TerminalGridView()
    view.transport = transport
    cachedView = view
    return view
  }
  let transport = PTYSession()
  static func command(machine: Machine, name: String) -> Command? {
    if Deployment.isLocal(name, machine) {
      return Command(
        ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh", ["-l"],
        directory: machine.sync?.repo.map(expandHome) ?? NSHomeDirectory())
    }
    return machine.ssh.map { Command("/usr/bin/ssh", ["-tt", "--", $0]) }
  }
  init(machine: Machine, name: String) {
    if let command = Self.command(machine: machine, name: name) { transport.start(command) }
  }
  deinit { transport.stop() }
}
struct TerminalPage: NSViewRepresentable {
  let session: TerminalSession
  func makeNSView(context: Context) -> TerminalGridView { session.view }
  func updateNSView(_ nsView: TerminalGridView, context: Context) {}
}
@MainActor final class TerminalGridView: NSView {
  var transport: PTYSession?
  private(set) var pasteNotice: String?
  private var frameState = TerminalFrame()
  private var revision: UInt64 = 0
  private var timer: Timer?
  private var selection: (start: Int, end: Int)?
  private let font: NSFont
  private let cellWidth: CGFloat
  private static let fontNames = [
    "JetBrainsMono Nerd Font Mono", "JetBrainsMono Nerd Font", "Symbols Nerd Font Mono", "Menlo",
  ]
  override var isFlipped: Bool { true }
  override var acceptsFirstResponder: Bool { true }
  override init(frame: NSRect) {
    let fonts = Self.fontNames.compactMap { NSFont(name: $0, size: 12) }
    let first = fonts.first ?? NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
    let descriptor = first.fontDescriptor.addingAttributes([
      .cascadeList: fonts.dropFirst().map(\.fontDescriptor)
    ])
    font = NSFont(descriptor: descriptor, size: 12) ?? first
    cellWidth = ceil(("M" as NSString).size(withAttributes: [.font: font]).width * 100) / 100
    super.init(frame: frame)
    clipsToBounds = true
    setContentHuggingPriority(.defaultLow, for: .vertical)
    setContentHuggingPriority(.defaultLow, for: .horizontal)
    setAccessibilityElement(true)
    setAccessibilityRole(.textArea)
    setAccessibilityLabel("Terminal")
    setAccessibilityHelp(
      "Select text with the mouse and press Command-C to copy. Paste with Command-V.")
  }
  convenience init() { self.init(frame: NSRect(x: 0, y: 0, width: 700, height: 450)) }
  required init?(coder: NSCoder) { nil }
  override func viewDidMoveToWindow() {
    super.viewDidMoveToWindow()
    timer?.invalidate()
    timer = nil
    if window != nil {
      let timer = Timer(timeInterval: 1.0 / 30, repeats: true) { [weak self] _ in
        MainActor.assumeIsolated { self?.receiveFrame() }
      }
      self.timer = timer
      RunLoop.main.add(timer, forMode: .common)
      window?.makeFirstResponder(self)
      resizeTerminal()
    }
  }
  private func receiveFrame() {
    guard let (revision, frame) = transport?.output.latest(after: revision) else { return }
    self.revision = revision
    frameState = frame
    needsDisplay = true
    setAccessibilityValue(frame.text)
  }
  override func setFrameSize(_ newSize: NSSize) {
    super.setFrameSize(newSize)
    resizeTerminal()
  }
  private func resizeTerminal() {
    let size = TerminalInput.fit(width: bounds.width, height: bounds.height, cellWidth: cellWidth)
    transport?.resize(columns: size.columns, rows: size.rows)
  }
  private func color(_ rgb: UInt32) -> NSColor {
    NSColor(
      srgbRed: CGFloat(rgb >> 16 & 255) / 255, green: CGFloat(rgb >> 8 & 255) / 255,
      blue: CGFloat(rgb & 255) / 255, alpha: 1)
  }
  override func draw(_ dirtyRect: NSRect) {
    color(0x0e1311).setFill()
    dirtyRect.intersection(bounds).fill()
    for run in frameState.runs {
      let inverse = run.flags & 16 != 0
      let fg = inverse ? run.background ?? 0x0e1311 : run.foreground ?? 0xf0eee8
      let bg = inverse ? run.foreground ?? 0xf0eee8 : run.background
      var styledFont = font
      if run.flags & 1 != 0 {
        styledFont = NSFontManager.shared.convert(styledFont, toHaveTrait: .boldFontMask)
      }
      if run.flags & 2 != 0 {
        styledFont = NSFontManager.shared.convert(styledFont, toHaveTrait: .italicFontMask)
      }
      let attributes: [NSAttributedString.Key: Any] = [
        .font: styledFont,
        .foregroundColor: color(fg).withAlphaComponent(run.flags & 4 != 0 ? 0.5 : 1),
      ]
      for glyph in run.glyphs {
        let rect = NSRect(
          x: 10 + CGFloat(glyph.column) * cellWidth, y: 10 + CGFloat(glyph.row) * 16,
          width: CGFloat(glyph.width) * cellWidth, height: 16)
        guard dirtyRect.intersects(rect) else { continue }
        if let bg {
          color(bg).setFill()
          rect.fill()
        }
        if let selection {
          let index = glyph.row * frameState.columns + glyph.column
          if index + glyph.width - 1 >= min(selection.start, selection.end)
            && index <= max(selection.start, selection.end)
          {
            color(0x33413a).setFill()
            rect.fill()
          }
        }
        guard run.flags & 32 == 0 else { continue }
        if glyph.text != " ", let context = NSGraphicsContext.current?.cgContext {
          let line = CTLineCreateWithAttributedString(
            NSAttributedString(string: glyph.text, attributes: attributes))
          context.saveGState()
          context.clip(to: rect)
          context.textMatrix = CGAffineTransform(scaleX: 1, y: -1)
          context.textPosition = CGPoint(x: rect.minX, y: rect.minY + font.ascender)
          CTLineDraw(line, context)
          context.restoreGState()
        }
        color(fg).setFill()
        if run.flags & 0x700 != 0 {
          NSRect(x: rect.minX, y: rect.maxY - 2, width: rect.width, height: 1).fill()
        }
        if run.flags & 64 != 0 {
          NSRect(x: rect.minX, y: rect.midY, width: rect.width, height: 1).fill()
        }
      }
    }
    if let cursor = frameState.cursor {
      color(0xc5dcc0).setStroke()
      NSBezierPath(
        rect: NSRect(
          x: 10 + CGFloat(cursor.column) * cellWidth, y: 10 + CGFloat(cursor.row) * 16,
          width: cellWidth, height: 16
        ).insetBy(dx: 0.5, dy: 0.5)
      ).stroke()
    }
    if let pasteNotice {
      let rect = NSRect(
        x: 8, y: max(0, bounds.height - 28), width: max(0, bounds.width - 16), height: 24)
      color(0x232d28).setFill()
      rect.fill()
      (pasteNotice as NSString).draw(
        in: rect.insetBy(dx: 6, dy: 4),
        withAttributes: [.font: NSFont.systemFont(ofSize: 12), .foregroundColor: color(0xd9a657)])
    }
  }
  private func cell(at point: NSPoint) -> Int {
    let column = max(0, min(frameState.columns - 1, Int((point.x - 10) / cellWidth)))
    let row = max(0, min(frameState.rows - 1, Int((point.y - 10) / 16)))
    return row * frameState.columns + column
  }
  override func mouseDown(with event: NSEvent) {
    window?.makeFirstResponder(self)
    let index = cell(at: convert(event.locationInWindow, from: nil))
    selection = (index, index)
    needsDisplay = true
  }
  override func mouseDragged(with event: NSEvent) {
    selection?.end = cell(at: convert(event.locationInWindow, from: nil))
    needsDisplay = true
    if let selection {
      setAccessibilitySelectedText(
        frameState.selectedText(from: selection.start, through: selection.end))
    }
  }
  @objc func copy(_ sender: Any?) {
    guard let selection else { return }
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(
      frameState.selectedText(from: selection.start, through: selection.end), forType: .string)
  }
  @objc func paste(_ sender: Any?) {
    guard let text = NSPasteboard.general.string(forType: .string) else { return }
    pasteText(text)
  }
  func pasteText(_ text: String) {
    pasteNotice = TerminalInput.pasteRejection(text)
    needsDisplay = true
    setAccessibilityHelp(
      pasteNotice ?? "Select text with the mouse and press Command-C to copy. Paste with Command-V."
    )
    if pasteNotice != nil {
      NSAccessibility.post(element: self, notification: .valueChanged)
      return
    }
    transport?.input.enqueue(TerminalInput.paste(text, bracketed: frameState.bracketedPaste))
  }
  override func performKeyEquivalent(with event: NSEvent) -> Bool {
    guard event.modifierFlags.contains(.command) else {
      return super.performKeyEquivalent(with: event)
    }
    switch event.charactersIgnoringModifiers?.lowercased() {
    case "c":
      copy(nil)
      return true
    case "v":
      paste(nil)
      return true
    case "a":
      selection = (0, frameState.columns * frameState.rows - 1)
      needsDisplay = true
      return true
    default: return super.performKeyEquivalent(with: event)
    }
  }
  override func keyDown(with event: NSEvent) {
    let names: [UInt16: String] = [
      123: "left", 124: "right", 125: "down", 126: "up", 36: "enter", 76: "enter", 51: "backspace",
      48: "tab", 53: "escape", 115: "home", 119: "end", 117: "delete", 116: "pageup",
      121: "pagedown",
    ]
    let name = names[event.keyCode] ?? event.charactersIgnoringModifiers ?? ""
    if let bytes = TerminalInput.key(
      name,
      text: event.modifierFlags.contains(.option)
        ? event.charactersIgnoringModifiers : event.characters,
      control: event.modifierFlags.contains(.control),
      alt: event.modifierFlags.contains(.option), command: event.modifierFlags.contains(.command),
      applicationCursor: frameState.applicationCursor)
    {
      transport?.input.enqueue(bytes)
    }
  }
  override func accessibilityValue() -> Any? { frameState.text }
}
