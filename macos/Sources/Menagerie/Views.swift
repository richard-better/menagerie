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
                }
              }.frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
            }.buttonStyle(.plain)
            if model.status(name).needsSync {
              Button(model.status(name).label, systemImage: "arrow.trianglehead.2.clockwise") {
                model.deploy(name, action: .sync)
              }.buttonStyle(.glass).disabled(model.jobs[name]?.running == true)
            } else {
              Text(model.status(name).label).font(.caption).foregroundStyle(Palette.muted)
            }
            ForEach(Array(machine.services.enumerated()), id: \.offset) { _, service in
              if let url = URL(string: service.url) {
                Link(destination: url) { ServiceIcon(service: service) }.help(service.name)
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
            Spacer()
            Text(model.page.capitalized).foregroundStyle(Palette.muted)
          }.buttonStyle(.glass)
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
            Button("End session") {
              session.transport.stop()
              model.terminals[model.selected] = nil
              model.page = "details"
            }.buttonStyle(.glass)
            TerminalPage(session: session).frame(maxWidth: .infinity, maxHeight: .infinity)
          }
        default: details
        }
      }.padding(26).padding(.top, 10).frame(
        maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading
      )
      .background(NotchedPanel(fraction: selectedFraction).fill(Palette.panel))
      HStack {
        Text(model.error ?? model.summary).lineLimit(1)
        Spacer()
        if let updated = model.updated { Text(updated, style: .relative) }
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
          if let system = model.observations[model.selected]?.report?.probes.system.value {
            Text("\(system.os) · up \(system.uptimeS / 3600)h").font(.caption).foregroundStyle(
              Palette.muted)
          }
        }
        Spacer()
        Text(model.status(model.selected).label).font(.title).monospacedDigit().foregroundStyle(
          model.status(model.selected).needsSync ? Palette.amber : Palette.ink)
      }
      GlassEffectContainer {
        HStack {
          Button("Changes") { model.page = "changes" }
          Button("Terminal", action: model.openTerminal)
          Button("Log") { model.page = "log" }
          Spacer()
          Button("Sync", systemImage: "arrow.trianglehead.2.clockwise") {
            model.deploy(model.selected, action: .sync)
          }.buttonStyle(.glassProminent).tint(Palette.accent).disabled(
            machine.sync == nil || model.jobs[model.selected]?.running == true)
          Menu {
            Button("Roll back") { model.deploy(model.selected, action: .rollback) }
          } label: {
            Image(systemName: "ellipsis")
          }.disabled(machine.sync == nil)
        }.buttonStyle(.glass)
      }
      Divider()
      if let report = model.observations[model.selected]?.report {
        VStack(alignment: .leading) {
          HStack {
            Text("Load").fontWeight(.medium)
            Spacer()
            Text(
              report.probes.load.value.map { String(format: "%.2f / %d cores", $0.one, $0.cpus) }
                ?? "Unavailable"
            ).monospacedDigit()
          }
          Picker("History", selection: $hours) {
            Text("1h").tag(1)
            Text("24h").tag(24)
          }.pickerStyle(.segmented).frame(width: 120)
          LoadChart(
            samples: report.loadHistory.filter {
              $0.at >= report.agent.now - Int64(hours) * 3_600_000
            }.map(\.value)
          ).frame(height: 72)
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
          }
        }.toggleStyle(.checkbox)
      }.scrollContentBackground(.hidden).disabled(model.committing)
      TextField("Commit message", text: $model.message, axis: .vertical).textFieldStyle(
        .roundedBorder
      ).disabled(model.committing)
      Text(model.commitNote).font(.caption).textSelection(.enabled)
      HStack {
        Button(model.drafting ? "Drafting…" : "Draft message", action: model.draft).disabled(
          model.drafting || model.committing || model.fleet?.commitMessage == nil)
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
    Canvas { context, size in
      let count = max(1, Int(size.width / 6))
      for i in 0..<count {
        for row in 0..<2 {
          let rect = CGRect(x: CGFloat(i) * 6, y: CGFloat(row) * 6 + 2, width: 3, height: 3)
          context.fill(
            Path(roundedRect: rect, cornerRadius: 0.6),
            with: .color(Double(i) / Double(count) < fraction ? color : Color(hex: 0x2f3b35)))
        }
      }
    }.frame(height: 14).accessibilityLabel("\(Int(fraction * 100)) percent")
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
  var samples: [Double]
  var body: some View {
    Canvas { context, size in
      let maxValue = max(1, samples.max() ?? 1)
      let columns = max(1, Int(size.width / 6))
      for column in 0..<columns {
        guard !samples.isEmpty else { continue }
        let sample = samples[min(samples.count - 1, column * samples.count / columns)]
        let rows = max(0, min(12, Int(sample / maxValue * 12)))
        for row in 0..<rows {
          context.fill(
            Path(
              ellipseIn: CGRect(
                x: column * 6, y: Int(size.height) - row * 6 - 4, width: 3, height: 3)),
            with: .color(Palette.ink.opacity(0.65)))
        }
      }
    }.accessibilityLabel("Load history, \(samples.count) samples")
  }
}

struct ServiceIcon: View {
  let service: Service
  var color: Color {
    switch service.icon {
    case "jellyfin": Color(hex: 0x7f5bc8)
    case "jellyseerr": Color(hex: 0x5b4bdb)
    case "home-assistant": Color(hex: 0x18bcf2)
    case "plex": Color(hex: 0xd9a20b)
    case "grafana": Color(hex: 0xe8622c)
    default: Palette.muted
    }
  }
  var body: some View {
    Text(service.icon == "jellyseerr" ? "S" : String(service.name.prefix(1)).uppercased())
      .font(.system(size: 12, weight: .bold)).foregroundStyle(.white)
      .frame(width: 24, height: 24).background(color, in: RoundedRectangle(cornerRadius: 6))
      .accessibilityLabel(service.name)
  }
}
