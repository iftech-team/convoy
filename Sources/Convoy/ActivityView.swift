import SwiftUI

/// Chronological feed of what agents did while you were elsewhere.
struct ActivityFeed: View {
    @EnvironmentObject var store: Store
    @State private var filter = ""

    private var events: [ActivityEvent] {
        let q = filter.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty ? store.activity : store.activity.filter { $0.sessionTitle.lowercased().contains(q) || $0.projectName.lowercased().contains(q) || $0.detail.lowercased().contains(q) }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text("Activity").font(.system(size: 13, weight: .semibold))
                Spacer()
                Button("Clear") { store.activity = []; ActivityEvent.save([]) }.buttonStyle(.plain).font(.caption).foregroundStyle(.secondary)
            }.padding(.horizontal, 14).frame(height: 36)
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass").font(.system(size: 11)).foregroundStyle(.secondary)
                TextField("Filter by session or project", text: $filter).textFieldStyle(.plain).font(.system(size: 12))
            }.padding(.horizontal, 8).frame(height: 26).background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 6)).padding(.horizontal, 10).padding(.bottom, 6)
            Divider()
            if events.isEmpty {
                Text("Nothing yet. Completions, questions and worktree events show here.").font(.caption).foregroundStyle(.secondary).padding(20).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(groupedDays, id: \.0) { day, items in
                            Text(day).font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 14).padding(.top, 10).padding(.bottom, 4)
                            ForEach(items) { event in row(event) }
                        }
                    }.padding(.bottom, 8)
                }
            }
        }.frame(width: 360, height: 440)
            .onAppear { store.markActivityRead() }
    }

    private var groupedDays: [(String, [ActivityEvent])] {
        let f = DateFormatter(); f.dateStyle = .medium; f.doesRelativeDateFormatting = true
        var order: [String] = []; var groups: [String: [ActivityEvent]] = [:]
        for e in events { let k = f.string(from: e.date); if groups[k] == nil { order.append(k) }; groups[k, default: []].append(e) }
        return order.map { ($0, groups[$0]!) }
    }

    private func row(_ event: ActivityEvent) -> some View {
        let (icon, color, label): (String, Color, String) = switch event.kind {
        case .waiting: ("questionmark.circle.fill", .orange, "needs you")
        case .done: ("checkmark.circle.fill", .green, "finished")
        case .started: ("play.circle", .secondary, "started")
        case .resumed: ("arrow.clockwise.circle", .secondary, "resumed")
        case .slept: ("moon.zzz", .secondary, "slept")
        case .worktree: ("arrow.triangle.branch", AppTheme.accent, "worktree")
        }
        return Button {
            if let project = store.project(ofSession: event.sessionID) { store.openSession(event.sessionID, in: project.id); store.showActivity = false }
        } label: {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: icon).foregroundStyle(color).font(.system(size: 13)).frame(width: 18).padding(.top, 1)
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(event.sessionTitle).font(.system(size: 12, weight: .medium)).lineLimit(1)
                        Text(label).font(.system(size: 10)).foregroundStyle(color)
                    }
                    Text([event.projectName, event.detail].filter { !$0.isEmpty }.joined(separator: " · ")).font(.system(size: 10.5)).foregroundStyle(.secondary).lineLimit(2)
                }
                Spacer()
                Text(event.date.formatted(date: .omitted, time: .shortened)).font(.system(size: 10)).foregroundStyle(.tertiary)
            }.padding(.horizontal, 14).padding(.vertical, 6).contentShape(Rectangle())
        }.buttonStyle(.plain)
    }
}

/// Emoji or GitHub avatar plus badge colour for a project row.
struct ProjectIconSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State var project: Project
    @State private var emoji = ""
    @State private var fetching = false
    @State private var fetchError: String?
    static let colors = ["#5E6AD2", "#4CAF83", "#E5A54B", "#D65C5C", "#3FA7D6", "#B067C9", "#8A8F98", "#E07C4C"]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Icon & color").font(.title2.bold())
            Text(project.name).foregroundStyle(.secondary)
            HStack(spacing: 12) {
                TextField("Emoji", text: $emoji).textFieldStyle(.roundedBorder).frame(width: 70).font(.system(size: 20))
                    .onChange(of: emoji) { _, v in if let last = v.last { emoji = String(last); project.icon = emoji } else { project.icon = nil } }
                ForEach(["🚀", "🧪", "📦", "🛒", "💳", "📱", "🌐", "🤖", "🧭", "🛠️"], id: \.self) { e in
                    Button(e) { emoji = e }.buttonStyle(.plain).font(.system(size: 18))
                }
            }
            HStack(spacing: 10) {
                Button(fetching ? "Fetching…" : "Use GitHub avatar") { fetchAvatar() }.disabled(fetching)
                Button("Folder icon") { emoji = ""; project.icon = nil }
                if let fetchError { Text(fetchError).font(.caption).foregroundStyle(.orange) }
            }
            Text("Badge color").font(.headline)
            HStack(spacing: 10) {
                ForEach(Self.colors, id: \.self) { hex in
                    Circle().fill(Color(hex: hex)).frame(width: 22, height: 22)
                        .overlay(Circle().strokeBorder(Color.primary, lineWidth: project.color == hex ? 2 : 0))
                        .onTapGesture { project.color = hex }
                }
                Button("None") { project.color = nil }.controlSize(.small)
            }
            HStack { Spacer(); Button("Cancel") { dismiss() }; Button("Save") { store.updateProject(project); dismiss() }.buttonStyle(.borderedProminent) }
        }.padding(24).frame(width: 460)
            .onAppear { if let icon = project.icon, !icon.hasPrefix("gh:") { emoji = icon } }
    }

    private func fetchAvatar() {
        guard let (s, out) = GitInfoService.run(["remote", "get-url", "origin"], in: project.path), s == 0 else { fetchError = "No origin remote."; return }
        let remote = out.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let range = remote.range(of: #"github\.com[:/]([^/]+)/"#, options: .regularExpression) else { fetchError = "Origin is not on GitHub."; return }
        let owner = remote[range].split(whereSeparator: { $0 == ":" || $0 == "/" })[1]
        fetching = true; fetchError = nil
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/icons")
        let target = dir.appendingPathComponent("\(owner).png")
        Task {
            do {
                try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                let (data, _) = try await URLSession.shared.data(from: URL(string: "https://github.com/\(owner).png?size=64")!)
                try data.write(to: target)
                await MainActor.run { project.icon = "gh:" + target.path; emoji = ""; fetching = false }
            } catch { await MainActor.run { fetchError = error.localizedDescription; fetching = false } }
        }
    }
}

extension Color {
    init(hex: String) {
        var h = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        if h.count == 3 { h = h.map { "\($0)\($0)" }.joined() }
        let v = UInt64(h, radix: 16) ?? 0x5E6AD2
        self.init(red: Double((v >> 16) & 0xFF) / 255, green: Double((v >> 8) & 0xFF) / 255, blue: Double(v & 0xFF) / 255)
    }
}

extension Project {
    /// Stable colour per project: the chosen badge colour, or one derived from the name so every project differs.
    var tint: Color {
        if let color { return Color(hex: color) }
        let palette = ProjectIconSheet.colors
        var hash: UInt64 = 5381
        for byte in name.utf8 { hash = (hash &* 33) &+ UInt64(byte) }
        return Color(hex: palette[Int(hash % UInt64(palette.count))])
    }
}

struct ProjectIconView: View {
    let project: Project
    var size: CGFloat = 14
    var body: some View {
        let tint = project.color.map { Color(hex: $0) }
        Group {
            if let icon = project.icon, icon.hasPrefix("gh:") || icon.hasPrefix("img:"), let image = NSImage(contentsOfFile: String(icon.dropFirst(icon.hasPrefix("gh:") ? 3 : 4))) {
                Image(nsImage: image).resizable().scaledToFill().frame(width: size, height: size).clipShape(RoundedRectangle(cornerRadius: 3))
            } else if let icon = project.icon, icon.hasPrefix("sf:") {
                Image(systemName: String(icon.dropFirst(3))).font(.system(size: size - 2)).foregroundStyle(project.tint)
            } else if let icon = project.icon, !icon.isEmpty {
                Text(icon).font(.system(size: size - 1))
            } else {
                Image(systemName: project.isGroup ? "folder.fill" : "folder").font(.system(size: size - 2))
                    .foregroundStyle(project.isGroup ? (tint ?? AppTheme.accent) : project.tint)
            }
        }.frame(width: size + 2)
            .overlay(alignment: .bottomTrailing) { if let tint, project.icon != nil { Circle().fill(tint).frame(width: 5, height: 5).offset(x: 1, y: 1) } }
    }
}
