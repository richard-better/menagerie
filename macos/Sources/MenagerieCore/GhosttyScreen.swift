import Foundation
import GhosttyBridge

public struct TerminalGlyph: Sendable, Equatable {
  public var column, row, width: Int
  public var text: String
  public var foreground, background: UInt32?
  public var flags: UInt16
}
public struct TerminalRun: Sendable {
  public var glyphs: [TerminalGlyph]
  public var foreground, background: UInt32?
  public var flags: UInt16
}
public struct TerminalFrame: Sendable {
  public var runs: [TerminalRun] = []
  public var glyphs: [TerminalGlyph] = []
  public var cursor: (column: Int, row: Int)?
  public var columns = 100
  public var rows = 22
  public var applicationCursor = false
  public var bracketedPaste = false
  public var text = ""
  public init(
    columns: Int = 100, rows: Int = 22, applicationCursor: Bool = false,
    bracketedPaste: Bool = false
  ) {
    self.columns = columns
    self.rows = rows
    self.applicationCursor = applicationCursor
    self.bracketedPaste = bracketedPaste
    cursor = nil
  }
  public func selectedText(from start: Int, through end: Int) -> String {
    let lower = max(0, min(start, end))
    let upper = min(columns * rows - 1, max(start, end))
    guard lower <= upper else { return "" }
    return (lower / columns...upper / columns).map { row in
      glyphs.filter { glyph in
        let index = row * columns + glyph.column
        return glyph.row == row && index <= upper && index + glyph.width - 1 >= lower
      }.map(\.text).joined().replacingOccurrences(
        of: #" +$"#, with: "", options: .regularExpression)
    }.joined(separator: "\n")
  }
}
/// Confined to one worker. Frames copied out of it are immutable and Sendable.
public final class GhosttyScreen {
  private let handle: OpaquePointer
  private var columns, rows: Int
  public init(columns: Int = 100, rows: Int = 22) throws {
    guard (1...512).contains(columns), (1...256).contains(rows),
      let handle = mg_screen_new(UInt16(columns), UInt16(rows))
    else { throw FleetError("Could not create Ghostty terminal") }
    self.handle = handle
    self.columns = columns
    self.rows = rows
  }
  deinit { mg_screen_free(handle) }
  public func write(_ bytes: Data) {
    bytes.withUnsafeBytes {
      mg_screen_write(handle, $0.bindMemory(to: UInt8.self).baseAddress, bytes.count)
    }
  }
  public func write(_ text: String) { write(Data(text.utf8)) }
  public func resize(columns: Int, rows: Int) throws {
    guard (1...512).contains(columns), (1...256).contains(rows),
      mg_screen_resize(handle, UInt16(columns), UInt16(rows))
    else { throw FleetError("Could not resize Ghostty terminal") }
    self.columns = columns
    self.rows = rows
  }
  public func mode(_ code: UInt16) -> Bool { mg_screen_mode(handle, code) }
  public func replies() -> Data {
    var bytes = [UInt8](repeating: 0, count: 65536)
    let count = mg_screen_replies(handle, &bytes, bytes.count)
    return Data(bytes.prefix(count))
  }
  public func snapshot() -> TerminalFrame {
    var frame = TerminalFrame(
      columns: columns, rows: rows, applicationCursor: mode(1), bracketedPaste: mode(2004))
    guard mg_screen_snapshot(handle) else { return frame }
    var cell = MGCell()
    while mg_screen_next(handle, &cell) {
      let text = withUnsafePointer(to: &cell.text) {
        $0.withMemoryRebound(to: CChar.self, capacity: 256) { String(cString: $0) }
      }
      let fg = UInt32(cell.fg.0) << 16 | UInt32(cell.fg.1) << 8 | UInt32(cell.fg.2)
      let bg = UInt32(cell.bg.0) << 16 | UInt32(cell.bg.1) << 8 | UInt32(cell.bg.2)
      frame.glyphs.append(
        TerminalGlyph(
          column: Int(cell.x), row: Int(cell.y), width: Int(cell.width), text: text,
          foreground: cell.has_fg ? fg : nil, background: cell.has_bg ? bg : nil, flags: cell.flags)
      )
    }
    for glyph in frame.glyphs {
      if let last = frame.runs.last, last.glyphs.last?.row == glyph.row,
        last.foreground == glyph.foreground, last.background == glyph.background,
        last.flags == glyph.flags
      {
        frame.runs[frame.runs.count - 1].glyphs.append(glyph)
      } else {
        frame.runs.append(
          TerminalRun(
            glyphs: [glyph], foreground: glyph.foreground, background: glyph.background,
            flags: glyph.flags))
      }
    }
    var x: UInt16 = 0
    var y: UInt16 = 0
    if mg_screen_cursor(handle, &x, &y) { frame.cursor = (Int(x), Int(y)) }
    var lines = [String](repeating: "", count: rows)
    for glyph in frame.glyphs where glyph.row < rows { lines[glyph.row] += glyph.text }
    frame.text = lines.map {
      $0.replacingOccurrences(of: #" +$"#, with: "", options: .regularExpression)
    }.joined(separator: "\n")
    return frame
  }
}
public enum TerminalInput {
  public static let pasteLimit = 256 * 1024
  public static func fit(width: Double, height: Double, cellWidth: Double) -> (
    columns: Int, rows: Int
  ) {
    (
      max(20, min(512, Int(max(0, width - 20) / cellWidth))),
      max(4, min(256, Int(max(0, height - 20) / 16)))
    )
  }
  public static func key(
    _ name: String, text: String? = nil, control: Bool = false, alt: Bool = false,
    command: Bool = false, applicationCursor: Bool = false
  ) -> Data? {
    guard !command else { return nil }
    if let final = ["up": "A", "down": "B", "right": "C", "left": "D"][name] {
      return Data((applicationCursor ? "\u{1b}O" : "\u{1b}[").appending(final).utf8)
    }
    if let bytes = [
      "enter": "\r", "backspace": "\u{7f}", "tab": "\t", "escape": "\u{1b}", "home": "\u{1b}[H",
      "end": "\u{1b}[F", "delete": "\u{1b}[3~", "pageup": "\u{1b}[5~", "pagedown": "\u{1b}[6~",
    ][name] {
      return Data(bytes.utf8)
    }
    if control, name.utf8.count == 1, let char = name.lowercased().utf8.first,
      (97...122).contains(char)
    {
      return Data([char - 96])
    }
    let value = text ?? name
    guard !value.isEmpty else { return nil }
    return Data(((alt ? "\u{1b}" : "") + value).utf8)
  }
  public static func paste(_ text: String, bracketed: Bool) -> Data {
    let safe = bracketed ? text.replacingOccurrences(of: "\u{1b}", with: "") : text
    var bytes = Array(safe.utf8.prefix(pasteLimit))
    while String(bytes: bytes, encoding: .utf8) == nil { bytes.removeLast() }
    return bracketed
      ? Data(Array("\u{1b}[200~".utf8) + bytes + Array("\u{1b}[201~".utf8)) : Data(bytes)
  }
}
