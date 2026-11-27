import MenagerieCore
import Testing

@testable import Menagerie

@Test func subtitlesDescribeManagementAndUseHumanUptime() throws {
  let fleet = try Fleet.parse(
    """
    version: 1
    machines:
      managed: {sync: {preset: nix-darwin}}
      agent: {agent: 'http://localhost:9184'}
      unmanaged: {}
    """)
  #expect(
    machineSubtitle(fleet.machines["managed"]!, uptime: 500 * 3600) == "nix-darwin sync · up 20d")
  #expect(
    machineSubtitle(fleet.machines["managed"]!, uptime: 47 * 3600) == "nix-darwin sync · up 47h")
  #expect(
    machineSubtitle(fleet.machines["managed"]!, uptime: 48 * 3600) == "nix-darwin sync · up 2d")
  #expect(machineSubtitle(fleet.machines["agent"]!, uptime: nil) == "No sync configured")
  #expect(machineSubtitle(fleet.machines["unmanaged"]!, uptime: nil) == "Not managed by Menagerie")
}

@Test func loadAxesDescribeTheSelectedTimeWindow() {
  #expect(loadAxisLabels(hours: 24) == ["-24h", "-18h", "-12h", "-6h", "now"])
  #expect(loadAxisLabels(hours: 1) == ["-60m", "-45m", "-30m", "-15m", "now"])
}
