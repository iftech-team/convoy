import SwiftUI

/// Landing page when no terminal tab is selected: what needs you, what's running, recent work, projects, limits.
struct HomePage: View {
    @EnvironmentObject var store: Store
    @ObservedObject var manager: TerminalManager
    @ObservedObject var usage: UsageLimits

    private var allSessions: [(Project, LinkedSession)] {
        store.workspace.projects.flatMap { p in p.linkedSessions.filter { $0.archived != true }.map { (p, $0) } }
    }
    private var running: [(Project, LinkedSession)] { allSessions.filter { manager.handles[$0.1.id]?.running == true } }
    private var needsYou: [(Project, LinkedSession)] { running.filter { store.agentState($0.1)?.needsYou == true } }
    private var recent: [(Project, LinkedSession)] {
        allSessions.filter { manager.handles[$0.1.id]?.running != true && !$0.1.title.hasPrefix("Login:") }.sorted { $0.1.createdAt > $1.1.createdAt }.prefix(8).map { $0 }
    }
    private var greeting: String {
        let hour = Calendar.current.component(.hour, from: Date())
        return hour < 12 ? "Good morning" : (hour < 18 ? "Good afternoon" : "Good evening")
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                HStack(alignment: .firstTextBaseline) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(greeting).font(.system(size: 26, weight: .bold))
                        Text(Date().formatted(date: .complete, time: .omitted)).font(.system(size: 13)).foregroundStyle(.secondary)
                    }
                    Spacer()
                    HStack(spacing: 8) {
                        Button { store.newSessionInCurrentProject() } label: { Label("New session", systemImage: "plus") }
                            .buttonStyle(.borderedProminent).disabled(store.workspace.projects.isEmpty).help(store.keys.display("session.new"))
                        Button { store.addProject() } label: { Label("Open folder", systemImage: "folder.badge.plus") }.help(store.keys.display("project.open"))
                        Button { store.openPalette(.all) } label: { Label("Search", systemImage: "command") }.help(store.keys.display("go.palette"))
                        Button { store.dashboardRequest = UUID() } label: { Label("Dashboard", systemImage: "rectangle.3.group") }.help(store.keys.display("go.dashboard"))
                    }.controlSize(.large)
                }
                if !needsYou.isEmpty {
                    section("Needs you", icon: "questionmark.circle.fill", tint: .orange) {
                        ForEach(needsYou, id: \.1.id) { p, s in sessionRow(p, s, action: "Open") }
                    }
                }
                HStack(alignment: .top, spacing: 20) {
                    VStack(alignment: .leading, spacing: 28) {
                        section("Running", icon: "terminal", tint: .green, empty: "No agents running. Start one with ⌘N.") {
                            ForEach(running, id: \.1.id) { p, s in sessionRow(p, s, action: "Open") }
                        }
                        section("Recent sessions", icon: "clock.arrow.circlepath", tint: .secondary, empty: "Sessions you stop or sleep show here.") {
                            ForEach(recent, id: \.1.id) { p, s in sessionRow(p, s, action: "Resume") }
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                    VStack(alignment: .leading, spacing: 28) {
                        section("Tasks", icon: "checklist", tint: AppTheme.accent) {
                            let c = store.taskCounts()
                            VStack(alignment: .leading, spacing: 8) {
                                HStack(spacing: 14) {
                                    stat("\(c.running)", "running", AppTheme.accent); stat("\(c.review)", "to review", .orange); stat("\(c.queued)", "queued", .secondary)
                                }
                                ForEach(store.tasks.filter { $0.status == .review || $0.status == .pr || $0.status == .running }.prefix(4)) { task in
                                    Button { if let p = store.workspace.projects.first(where: { $0.id == task.projectID }) { store.selectProject(p.id); store.areaRequest = "Tasks" } } label: {
                                        HStack(spacing: 6) {
                                            Circle().fill(task.status == .running ? AppTheme.accent : (task.status == .done ? Color.green : Color.orange)).frame(width: 6, height: 6)
                                            Text(task.title).font(.system(size: 12)).lineLimit(1)
                                            Spacer()
                                            Text(task.status.rawValue).font(.system(size: 10)).foregroundStyle(.secondary)
                                        }.contentShape(Rectangle())
                                    }.buttonStyle(.plain)
                                }
                                if let p = store.project {
                                    Button { store.selectProject(p.id); store.areaRequest = "Tasks" } label: { Label("Open tasks for \(p.name)", systemImage: "arrow.right") }.buttonStyle(.plain).font(.system(size: 11)).foregroundStyle(AppTheme.accent)
                                }
                            }.padding(14)
                        }
                        section("Limits", icon: "chart.bar.xaxis", tint: AppTheme.accent) {
                            limits()
                        }
                        section("Activity", icon: "bell", tint: .secondary, empty: "Nothing yet.") {
                            ForEach(store.activity.prefix(6)) { e in activityRow(e) }
                        }
                    }.frame(minWidth: 300, idealWidth: 380, maxWidth: 460)
                }
                section("Projects", icon: "folder", tint: .secondary, empty: "Open a folder to get started.") {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 220, maximum: 360), spacing: 10)], spacing: 10) {
                        ForEach(store.workspace.rootProjects) { project in projectCard(project) }
                        ForEach(store.workspace.projects.filter { $0.parentID != nil && !$0.isGroup }) { project in projectCard(project) }
                    }
                }
                HStack(spacing: 14) {
                    hint("⌘K", "palette"); hint("⌘E", "switch terminal"); hint("⌘/", "quick commands"); hint("⌘,", "settings")
                    if !store.diagnostics.problems.isEmpty {
                        let failures = store.diagnostics.failures.count
                        Button { store.showSettings = true; store.settingsSection = "Setup" } label: {
                            Label("Setup: \(store.diagnostics.problems.count) to check", systemImage: failures > 0 ? "xmark.octagon.fill" : "exclamationmark.triangle.fill")
                                .font(.system(size: 11, weight: .medium)).foregroundStyle(failures > 0 ? .red : .orange)
                                .padding(.horizontal, 8).padding(.vertical, 3).background((failures > 0 ? Color.red : Color.orange).opacity(0.1), in: Capsule())
                        }.buttonStyle(.plain).help("Missing CLIs, logins or configuration problems")
                    }
                }.padding(.top, 8)
            }.padding(36).frame(maxWidth: .infinity, alignment: .leading)
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder private func section<Content: View>(_ title: String, icon: String, tint: Color, empty: String? = nil, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: icon).font(.system(size: 11, weight: .semibold)).foregroundStyle(tint)
                Text(title.uppercased()).font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
            }
            VStack(spacing: 0) {
                content()
                if let empty, isEmptySection(title) { Text(empty).font(.caption).foregroundStyle(.secondary).padding(14).frame(maxWidth: .infinity, alignment: .leading) }
            }
            .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 10))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(AppTheme.stroke))
        }
    }

    private func isEmptySection(_ title: String) -> Bool {
        switch title {
        case "Running": return running.isEmpty
        case "Recent sessions": return recent.isEmpty
        case "Activity": return store.activity.isEmpty
        case "Projects": return store.workspace.projects.isEmpty
        default: return false
        }
    }

    private func sessionRow(_ project: Project, _ session: LinkedSession, action: String) -> some View {
        let running = manager.handles[session.id]?.running == true
        return Button {
            if action == "Resume" { store.startSession(session, in: project, resume: true) } else { store.openSession(session.id, in: project.id) }
        } label: {
            HStack(spacing: 10) {
                if store.isHibernated(session.id) { Image(systemName: "moon.zzz.fill").font(.system(size: 9)).foregroundStyle(.secondary).frame(width: 12) }
                else { AgentStateGlyph(state: store.agentState(session), running: running, size: 7).frame(width: 12) }
                VStack(alignment: .leading, spacing: 2) {
                    Text(session.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    Text([project.name, session.agent.rawValue, session.branch.map { "⎇ " + $0 } ?? "", running ? AgentStateGlyph(state: store.agentState(session), running: true).label : session.createdAt.formatted(.relative(presentation: .named))].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                Text(action).font(.system(size: 11, weight: .medium)).foregroundStyle(AppTheme.accent)
            }.padding(.horizontal, 14).padding(.vertical, 9).contentShape(Rectangle())
        }.buttonStyle(.plain).overlay(alignment: .bottom) { Divider().padding(.leading, 14) }
    }

    private func projectCard(_ project: Project) -> some View {
        let git = store.git.info[project.path]
        let count = project.linkedSessions.filter { manager.handles[$0.id]?.running == true }.count
        return Button { store.selectProject(project.id) } label: {
            HStack(spacing: 10) {
                ProjectIconView(project: project, size: 18)
                VStack(alignment: .leading, spacing: 2) {
                    Text(project.name).font(.system(size: 13, weight: .medium)).lineLimit(1).foregroundStyle(project.color.map { Color(hex: $0) } ?? .primary)
                    Text([project.isGroup ? "Group" : (git?.branch ?? "" ).isEmpty ? "" : "⎇ " + (git?.branch ?? ""), git?.summary ?? "", count > 0 ? "\(count) running" : ""].filter { !$0.isEmpty }.joined(separator: " · "))
                        .font(.system(size: 10.5)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 0)
                if count > 0 { Circle().fill(Color.green).frame(width: 6, height: 6) }
            }.padding(12).frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
                .background(Color.primary.opacity(0.03), in: RoundedRectangle(cornerRadius: 8))
        }.buttonStyle(.plain)
    }

    @ViewBuilder private func limits() -> some View {
        VStack(alignment: .leading, spacing: 10) {
            if let snapshot = usage.claudeSnapshot, !snapshot.availableWindows().isEmpty {
                ForEach([("five_hour", "Claude 5h"), ("seven_day", "Claude 7d")], id: \.0) { key, label in
                    if let w = snapshot.availableWindows()[key] { bar(label, w.used_percentage) }
                }
            } else {
                Text("Claude: shows after a response in a session started here.").font(.caption).foregroundStyle(.secondary)
            }
            ForEach(usage.buckets) { bucket in
                if let p = bucket.primary { bar("Codex \(p.label.replacingOccurrences(of: " window", with: ""))", p.percent) }
                if let s = bucket.secondary { bar("Codex \(s.label.replacingOccurrences(of: " window", with: ""))", s.percent) }
            }
            if usage.buckets.isEmpty { Text(usage.loading ? "Codex: refreshing…" : "Codex: open AI Limits to refresh.").font(.caption).foregroundStyle(.secondary) }
        }.padding(14)
    }

    private func bar(_ label: String, _ percent: Double) -> some View {
        let tint: Color = percent >= 90 ? .red : (percent >= 70 ? .orange : AppTheme.accent)
        return VStack(alignment: .leading, spacing: 4) {
            HStack { Text(label).font(.system(size: 11, weight: .medium)); Spacer(); Text("\(Int(percent))%").font(.system(size: 11, weight: .semibold)).monospacedDigit().foregroundStyle(tint) }
            GeometryReader { geo in ZStack(alignment: .leading) { Capsule().fill(Color.primary.opacity(0.08)); Capsule().fill(tint).frame(width: max(3, geo.size.width * min(100, percent) / 100)) } }.frame(height: 4)
        }
    }

    private func activityRow(_ e: ActivityEvent) -> some View {
        Button { if let p = store.project(ofSession: e.sessionID) { store.openSession(e.sessionID, in: p.id) } } label: {
            HStack(spacing: 8) {
                Text(e.sessionTitle).font(.system(size: 12, weight: .medium)).lineLimit(1)
                Text(e.kind == .waiting ? "needs you" : e.kind.rawValue).font(.system(size: 10)).foregroundStyle(e.kind == .waiting ? .orange : (e.kind == .done ? .green : .secondary))
                Spacer()
                Text(e.date.formatted(date: .omitted, time: .shortened)).font(.system(size: 10)).foregroundStyle(.tertiary)
            }.padding(.horizontal, 14).padding(.vertical, 7).contentShape(Rectangle())
        }.buttonStyle(.plain).overlay(alignment: .bottom) { Divider().padding(.leading, 14) }
    }

    private func stat(_ value: String, _ label: String, _ tint: Color) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(value).font(.system(size: 20, weight: .bold)).foregroundStyle(tint).monospacedDigit()
            Text(label).font(.system(size: 10)).foregroundStyle(.secondary)
        }
    }

    private func hint(_ key: String, _ label: String) -> some View {
        HStack(spacing: 4) {
            Text(key).font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary).padding(.horizontal, 4).padding(.vertical, 1).background(Color.primary.opacity(0.07), in: RoundedRectangle(cornerRadius: 3))
            Text(label).font(.system(size: 10)).foregroundStyle(.tertiary)
        }
    }
}
