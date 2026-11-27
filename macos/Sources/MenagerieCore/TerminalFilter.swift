/// Refuses application program commands without accumulating their payloads.
public struct TerminalFilter: Sendable {
  private enum State: Sendable { case ground, escape, apc, apcEscape }
  private var continuation = 0
  private var state: State = .ground
  public init() {}
  public mutating func filter(_ bytes: ArraySlice<UInt8>) -> [UInt8] {
    var output: [UInt8] = []
    for byte in bytes {
      switch state {
      case .ground:
        if continuation > 0 && (0x80...0xbf).contains(byte) {
          continuation -= 1
          output.append(byte)
          continue
        }
        continuation =
          byte >= 0xc2 && byte <= 0xdf
          ? 1 : byte >= 0xe0 && byte <= 0xef ? 2 : byte >= 0xf0 && byte <= 0xf4 ? 3 : 0
        if byte == 0x1b {
          state = .escape
        } else if byte == 0x9f {
          state = .apc
        } else {
          output.append(byte)
        }
      case .escape:
        if byte == 0x5f {
          state = .apc
        } else {
          output.append(0x1b)
          if byte != 0x1b {
            output.append(byte)
            state = .ground
          }
        }
      case .apc:
        if byte == 0x1b {
          state = .apcEscape
        } else if [0x9c, 0x18, 0x1a, 0x07].contains(byte) {
          state = .ground
        }
      case .apcEscape:
        if byte == 0x5c || byte == 0x18 || byte == 0x1a {
          state = .ground
        } else if byte != 0x1b {
          state = .apc
        }
      }
    }
    return output
  }
}
