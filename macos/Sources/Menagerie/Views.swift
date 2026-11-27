import MenagerieCore
import SwiftUI

extension Color {
  init(hex: UInt32) {
    self.init(
      .sRGB, red: Double(hex >> 16 & 255) / 255, green: Double(hex >> 8 & 255) / 255,
      blue: Double(hex & 255) / 255, opacity: 1)
  }
}
enum Palette {
  static let background = Color(hex: 0x151c19), panel = Color(hex: 0x1b231f),
    raised = Color(hex: 0x232d28),
    ink = Color(hex: 0xf0eee8), muted = Color(hex: 0x939e97), accent = Color(hex: 0xc5dcc0),
    amber = Color(hex: 0xd9a657), green = Color(hex: 0x7ccb93)
}
func symbol(_ machine: Machine) -> String {
  switch machine.device {
  case .laptop: "laptopcomputer"
  case .desktop: "desktopcomputer"
  case .server: "server.rack"
  case .box: "externaldrive"
  case .phone: "iphone"
  case .other: "display"
  }
}
struct StatusDot: View {
  var status: MachineStatus
  var body: some View {
    Circle().fill(status.needsSync ? Palette.amber : status.online ? Palette.green : Palette.muted)
      .opacity(status == .offline ? 0.35 : 1).frame(width: 6, height: 6).accessibilityLabel(
        status.label)
  }
}
struct FleetPopover: View {
  @Bindable var model: Model
  var open: () -> Void
  var body: some View {
    VStack(spacing: 0) {
      ForEach(model.fleet?.order ?? [], id: \.self) { name in
        if let machine = model.fleet?.machines[name] {
          HStack(spacing: 12) {
            Button {
              model.selected = name
              model.page = "details"
              open()
            } label: {
              HStack(spacing: 10) {
                Image(systemName: symbol(machine)).font(.title2).frame(width: 32)
                VStack(alignment: .leading, spacing: 4) {
                  HStack {
                    StatusDot(status: model.status(name))
                    Text(name).fontWeight(.medium)
                  }
                  Text(machine.role ?? "").font(.caption).foregroundStyle(Palette.muted)
                  if let job = model.jobs[name], !job.running, let result = job.result {
                    Text(result).font(.caption).foregroundStyle(
                      result == "Succeeded" ? Palette.green : Palette.amber
                    ).lineLimit(2).help(result)
                  }
                }
              }.frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
            }.buttonStyle(.plain)
            if let job = model.jobs[name], job.running {
              VStack(spacing: 4) {
                Text("Syncing…").font(.caption)
                Button("Cancel") { job.cancel.cancel() }.buttonStyle(.glass)
              }
            } else if model.status(name).needsSync {
              Button(model.status(name).label, systemImage: "arrow.trianglehead.2.clockwise") {
                model.deploy(name, action: .sync)
              }.buttonStyle(.glass).disabled(model.jobs[name]?.running == true)
            } else {
              Text(model.status(name).label).font(.caption).foregroundStyle(Palette.muted)
            }
            HStack(spacing: 4) {
              ForEach(Array(machine.services.enumerated()), id: \.offset) { _, service in
                if let url = URL(string: service.url) {
                  Link(destination: url) { ServiceIcon(service: service) }.help(service.name)
                }
              }
            }
          }.padding(14)
        }
      }
      if let error = model.error { Text(error).font(.caption).textSelection(.enabled).padding() }
      Divider()
      HStack {
        Text(model.summary).font(.caption).foregroundStyle(Palette.muted)
        Spacer()
        Button("Open Menagerie", systemImage: "macwindow", action: open).labelStyle(.iconOnly)
        Button("Edit fleet", systemImage: "gearshape", action: model.editFleet).labelStyle(
          .iconOnly)
      }.buttonStyle(.plain).padding(10)
    }.frame(width: 450).foregroundStyle(Palette.ink).background(Palette.background.opacity(0.92))
      .preferredColorScheme(.dark)
  }
}
struct NotchedPanel: Shape {
  var fraction: CGFloat
  func path(in rect: CGRect) -> Path {
    let x = min(rect.maxX - 18, max(18, rect.width * fraction))
    var p = Path()
    p.move(to: CGPoint(x: 0, y: 10))
    p.addLine(to: CGPoint(x: x - 10, y: 10))
    p.addLine(to: CGPoint(x: x, y: 0))
    p.addLine(to: CGPoint(x: x + 10, y: 10))
    p.addLine(to: CGPoint(x: rect.maxX, y: 10))
    p.addLine(to: CGPoint(x: rect.maxX, y: rect.maxY))
    p.addLine(to: CGPoint(x: 0, y: rect.maxY))
    p.closeSubpath()
    return p
  }
}
struct FleetView: View {
  @Bindable var model: Model
  @State private var hours = 24
  var body: some View {
    VStack(spacing: 0) {
      HStack(spacing: 0) {
        ForEach(model.fleet?.order ?? [], id: \.self) { name in
          if let machine = model.fleet?.machines[name] {
            Button {
              model.selected = name
              model.page = "details"
            } label: {
              VStack(spacing: 9) {
                Image(systemName: symbol(machine)).font(.system(size: 32)).foregroundStyle(
                  model.selected == name ? Palette.ink : Palette.muted)
                Text(name).font(.caption).lineLimit(1)
                HStack(spacing: 5) {
                  StatusDot(status: model.status(name))
                  Text(model.status(name).label).font(.caption2).foregroundStyle(Palette.muted)
                }
              }.frame(maxWidth: .infinity).padding(.vertical, 22).contentShape(Rectangle())
            }.buttonStyle(.plain).accessibilityAddTraits(model.selected == name ? .isSelected : [])
          }
        }
      }
      VStack(alignment: .leading, spacing: 20) {
        if model.page != "details" {
          HStack {
            Button("Back", systemImage: "chevron.left") { model.page = "details" }
            Text(model.page.capitalized).foregroundStyle(Palette.muted)
            Spacer()
            if model.page == "terminal", let session = model.terminals[model.selected] {
              Button("End session") {
                session.transport.stop()
                model.terminals[model.selected] = nil
                model.page = "details"
              }
            }
          }.buttonStyle(PanelButtonStyle())
        }
        switch model.page {
        case "commit": CommitPage(model: model)
        case "changes":
          ScrollView {
            Text(
              model.observations[model.selected]?.report?.probes.revision.value.flatMap {
                model.changes[$0.rev]
              } ?? "No known missing commits"
            ).frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
          }
        case "log": log
        case "terminal":
          if let session = model.terminals[model.selected] {
            TerminalPage(session: session).frame(maxWidth: .infinity, maxHeight: .infinity)
          }
        default:
          ScrollView {
            VStack(alignment: .leading, spacing: 20) { details }
              .frame(maxWidth: .infinity, alignment: .leading)
          }.defaultScrollAnchor(.top).id(model.selected)
        }
      }.padding(26).padding(.top, 10).frame(
        maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading
      )
      .background(NotchedPanel(fraction: selectedFraction).fill(Palette.panel))
      HStack {
        Text(model.error ?? model.summary).lineLimit(1)
        Spacer()
        if let updated = model.updated {
          Text("Updated \(updated.formatted(.dateTime.hour().minute().second()))")
        }
      }.font(.caption).foregroundStyle(Palette.muted).padding(12)
    }.foregroundStyle(Palette.ink).background(Palette.background).preferredColorScheme(.dark)
      .frame(minWidth: 680, minHeight: 560)
      .toolbar {
        ToolbarItem {
          Button("Refresh", systemImage: "arrow.clockwise") {
            Task {
              await model.reload()
              await model.poll()
            }
          }
        }
        ToolbarItem { Button("Edit fleet", systemImage: "gearshape", action: model.editFleet) }
      }
  }
  var selectedFraction: CGFloat {
    let order = model.fleet?.order ?? []
    return (CGFloat(order.firstIndex(of: model.selected) ?? 0) + 0.5) / CGFloat(max(1, order.count))
  }
  @ViewBuilder var details: some View {
    if let machine = model.fleet?.machines[model.selected] {
      HStack(alignment: .top) {
        VStack(alignment: .leading, spacing: 8) {
          Text(model.selected).font(.title2).fontWeight(.semibold).textSelection(.enabled)
          Text(machine.role ?? "").foregroundStyle(Palette.muted)
          Text(
            machineSubtitle(
              machine,
              uptime: model.observations[model.selected]?.report?.probes.system.value?.uptimeS)
          )
          .font(.caption).foregroundStyle(Palette.muted)
        }
        Spacer()
        VStack(alignment: .trailing, spacing: 8) {
          Text(model.status(model.selected).label).font(.title).monospacedDigit().foregroundStyle(
            model.status(model.selected).needsSync ? Palette.amber : Palette.ink)
          if let report = model.observations[model.selected]?.report,
            report.probes.revision.isFresh(now: report.agent.now),
            let revision = report.probes.revision.value
          {
            Text((revision.generation.map { "generation \($0) · " } ?? "") + revision.rev.prefix(7))
              .font(.caption.monospaced()).foregroundStyle(Palette.muted).textSelection(.enabled)
          }
        }
      }.fixedSize(horizontal: false, vertical: true)
      GlassEffectContainer {
        HStack {
          Button("Changes") { model.page = "changes" }
          Button("Terminal", action: model.openTerminal)
          Button("Log") { model.page = "log" }
          Spacer()
          Button("Sync", systemImage: "arrow.trianglehead.2.clockwise") {
            model.deploy(model.selected, action: .sync)
          }.buttonStyle(PanelButtonStyle(primary: true)).disabled(
            machine.sync == nil || model.jobs[model.selected]?.running == true)
          Menu {
            Button("Roll back") { model.deploy(model.selected, action: .rollback) }
          } label: {
            Image(systemName: "ellipsis")
          }.disabled(machine.sync == nil)
        }.buttonStyle(PanelButtonStyle())
      }
      Divider()
      if let note = model.comparisonNote(model.selected) {
        Text(note).font(.callout).foregroundStyle(Palette.amber).textSelection(.enabled)
          .fixedSize(horizontal: false, vertical: true)
      }
      if let report = model.observations[model.selected]?.report {
        VStack(alignment: .leading) {
          HStack {
            Text("Load").fontWeight(.medium)
            Text(
              report.probes.load.value.map { String(format: "%.2f / %d cpus", $0.one, $0.cpus) }
                ?? "Unavailable"
            ).monospacedDigit().foregroundStyle(Palette.muted)
            Spacer()
            Picker("History", selection: $hours) {
              Text("1h").tag(1)
              Text("24h").tag(24)
            }.pickerStyle(.segmented).labelsHidden().frame(width: 120)
          }
          LoadChart(samples: report.loadHistory, now: report.agent.now, hours: hours)
        }
        Grid(horizontalSpacing: 32, verticalSpacing: 20) {
          GridRow {
            UsageMeter(name: "Memory", usage: report.probes.memory.value)
            UsageMeter(name: "Disk", usage: report.probes.disk.value)
          }
          GridRow {
            VStack(alignment: .leading) {
              Text(
                report.probes.battery.value.map {
                  "Battery · \(Int($0.percent))%" + ($0.charging ? " · charging" : "")
                } ?? "Battery · none")
              Dots(
                fraction: (report.probes.battery.value?.percent ?? 0) / 100,
                color: report.probes.battery.value?.charging == true ? Palette.green : Palette.ink)
            }
            VStack(alignment: .leading) {
              Text(
                report.probes.temperature.value.map { "Temperature · \(Int($0.celsius))°C" }
                  ?? "Temperature · unavailable")
              Dots(
                fraction: (report.probes.temperature.value?.celsius ?? 0) / 100,
                color: (report.probes.temperature.value?.celsius ?? 0) >= 80
                  ? Palette.amber : Palette.ink)
            }
          }
        }.font(.callout)
      } else {
        ContentUnavailableView(
          "No agent report", systemImage: "network",
          description: Text(
            model.observations[model.selected]?.error ?? "This machine does not run an agent."))
      }
      if !machine.services.isEmpty {
        Divider()
        Text("Services").fontWeight(.medium)
        ForEach(Array(machine.services.enumerated()), id: \.offset) { index, service in
          if let url = URL(string: service.url) {
            HStack {
              Link(destination: url) {
                HStack {
                  ServiceIcon(service: service)
                  Text(service.name)
                  Image(systemName: "arrow.up.right")
                }
              }
              Spacer()
              Text(model.health["\(model.selected)/\(index)"] ?? "No health check").font(.caption)
                .foregroundStyle(Palette.muted)
            }
          }
        }
      }
      Spacer(minLength: 0)
      if let checkout = model.checkout {
        Button(
          checkout.dirty
            ? "Checkout: uncommitted changes"
            : "Checkout: \(checkout.counts.ahead) unpushed · \(checkout.counts.behind) unpulled",
          action: model.loadChanges
        ).buttonStyle(.plain).foregroundStyle(Palette.accent)
      }
    } else {
      ContentUnavailableView(
        "Add your machines", systemImage: "desktopcomputer",
        description: Text("Edit the fleet file to get started."))
      Button("Edit fleet", action: model.editFleet)
    }
  }
  @ViewBuilder var log: some View {
    if let job = model.jobs[model.selected] {
      HStack {
        Text(job.result ?? "Running…")
        Spacer()
        if job.running { Button("Cancel") { job.cancel.cancel() } }
      }
      ScrollView {
        Text(job.text).font(.system(.caption, design: .monospaced)).textSelection(.enabled).frame(
          maxWidth: .infinity, alignment: .leading)
      }.frame(maxHeight: .infinity)
    } else {
      Text("No sync has run for this machine.")
      Spacer()
    }
  }
}
struct CommitPage: View {
  @Bindable var model: Model
  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Commit checkout changes").font(.title2)
      List(model.files) { change in
        Toggle(
          isOn: Binding(
            get: { model.included.contains(change.path) },
            set: {
              if $0 {
                model.included.insert(change.path)
              } else {
                model.included.remove(change.path)
              }
            })
        ) {
          HStack {
            Text(change.path).textSelection(.enabled)
            Spacer()
            Text(change.kind).foregroundStyle(Palette.muted)
            if let lines = change.lines {
              Text("+\(lines.added) -\(lines.removed)").font(.caption.monospacedDigit())
                .foregroundStyle(Palette.muted)
            }
          }
        }.toggleStyle(.checkbox)
      }.scrollContentBackground(.hidden).disabled(model.committing)
      TextField(
        "Commit message",
        text: Binding(get: { model.message }, set: { model.message = Commit.oneLine($0) })
      ).textFieldStyle(
        .roundedBorder
      ).disabled(model.committing)
      Text(model.commitNote).font(.caption).textSelection(.enabled)
      HStack {
        Button(model.drafting ? "Drafting…" : "Draft message", action: model.draft).disabled(
          model.drafting || model.committing || model.loadingChanges
            || model.commitContext?.draftCommand == nil)
        Spacer()
        Button(model.committing ? "Committing…" : "Commit and push", action: model.commit)
          .buttonStyle(.glassProminent).disabled(
            model.committing || model.drafting || model.included.isEmpty || model.message.isEmpty)
      }.buttonStyle(.glass)
    }
  }
}
struct Dots: View {
  var fraction: Double
  var color: Color
  var body: some View {
    DotDrawing(content: .meter(fraction, color)).frame(height: 14).accessibilityLabel(
      "\(Int(fraction * 100)) percent")
  }
}
struct UsageMeter: View {
  var name: String
  var usage: Usage?
  var body: some View {
    VStack(alignment: .leading) {
      HStack {
        Text(name)
        Spacer()
        Text(
          usage.map {
            String(
              format: "%.1f / %.1f GB", Double($0.usedBytes) / 1e9, Double($0.totalBytes) / 1e9)
          } ?? "Unavailable"
        ).font(.caption).monospacedDigit()
      }
      Dots(
        fraction: usage?.fraction ?? 0,
        color: (usage?.fraction ?? 0) > 0.95
          ? .red : (usage?.fraction ?? 0) > 0.85 ? Palette.amber : Palette.ink)
    }.frame(maxWidth: .infinity)
  }
}
struct LoadChart: View {
  var samples: [AgentStatus.Sample]
  var now: Int64
  var hours: Int
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text(String(format: "peak %.2f", peak)).font(.caption).foregroundStyle(Palette.muted)
      DotDrawing(content: .history(samples.map { .init(at: $0.at, value: $0.value) }, now, hours))
        .frame(maxWidth: .infinity).frame(height: 72)
        .accessibilityLabel("Load history, \(samples.count) samples")
      HStack {
        ForEach(Array(loadAxisLabels(hours: hours).enumerated()), id: \.offset) { index, label in
          if index > 0 { Spacer() }
          Text(label)
        }
      }.font(.caption).monospacedDigit().foregroundStyle(Palette.muted)
    }
  }
  var peak: Double {
    samples.filter {
      let age = now.subtractingReportingOverflow($0.at)
      return !age.overflow && (0...Int64(hours) * 3_600_000).contains(age.partialValue)
    }.map(\.value).max() ?? 0
  }
}

struct ServiceIcon: View {
  let service: Service
  private static let resources = Dictionary(
    uniqueKeysWithValues: (Bundle.main.urls(
      forResourcesWithExtension: "svg", subdirectory: "services") ?? []).map {
        ($0.deletingPathExtension().lastPathComponent, $0)
      })
  private static var glyphs: [String: NSImage] = [:]
  static func releaseImages() { glyphs.removeAll() }
  private static func glyph(_ name: String) -> NSImage? {
    if let cached = glyphs[name] { return cached }
    guard let url = resources[name], let image = NSImage(contentsOf: url) else { return nil }
    image.isTemplate = true
    glyphs[name] = image
    return image
  }
  var body: some View {
    Group {
      if let name = service.icon, let glyph = Self.glyph(name) {
        Image(nsImage: glyph).renderingMode(.template).resizable().scaledToFit()
          .frame(width: 13.2, height: 13.2)
      } else {
        Text(String(service.name.prefix(1)).uppercased()).font(.system(size: 14, weight: .bold))
      }
    }.foregroundStyle(Palette.ink).frame(width: 24, height: 24)
      .background(Palette.raised, in: RoundedRectangle(cornerRadius: 6))
      .accessibilityLabel(service.name)
  }
}

func machineSubtitle(_ machine: Machine, uptime: UInt64?) -> String {
  let preset =
    machine.sync.map { "\($0.preset.rawValue) sync" }
    ?? (machine.agent == nil ? "Not managed by Menagerie" : "No sync configured")
  guard let uptime else { return preset }
  let duration = uptime >= 48 * 3600 ? "\(uptime / 86400)d" : "\(uptime / 3600)h"
  return "\(preset) · up \(duration)"
}
func loadAxisLabels(hours: Int) -> [String] {
  hours == 1 ? ["-60m", "-45m", "-30m", "-15m", "now"] : ["-24h", "-18h", "-12h", "-6h", "now"]
}
struct PanelButtonStyle: ButtonStyle {
  var primary = false
  @Environment(\.isEnabled) private var enabled
  func makeBody(configuration: Configuration) -> some View {
    let fill = primary && enabled ? Palette.accent : Palette.raised
    configuration.label
      .foregroundStyle(enabled ? (primary ? Palette.background : Palette.ink) : Palette.muted)
      .padding(.horizontal, 12).padding(.vertical, 6)
      .background(fill, in: .rect(cornerRadius: 8))
      .glassEffect(.regular.tint(fill).interactive(), in: .rect(cornerRadius: 8))
      .opacity(configuration.isPressed ? 0.8 : 1)
  }
}
