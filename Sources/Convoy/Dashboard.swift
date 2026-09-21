import SwiftUI
import SwiftTerm

/// Kanban of every session across projects: Needs you · Working · Done · Idle. Lives in its own window.
struct DashboardView: View {
    @EnvironmentObject var store: Store
    @ObservedObject var manager: TerminalManager
    @ObservedObject var status: AgentStatusStore
    @State private var filter = ""

    private var sessions: [(Project, LinkedSession)] {
        let q = filter.lowercased().trimmingCharacters(in: .whitespaces)
        return store.workspace.projects.flatMap { p in p.linkedSessions.filter { $0.archived != true && !$0.title.hasPrefix("Login:") }.map { (p, $0) } }
            .filter { q.isEmpty || $0.1.title.lowercased().contains(q) || $0.0.name.lowercased().contains(q) }
    }
    private func column(_ name: String) -> [(Project, LinkedSession)] {
        sessions.filter { p, s in
            let running = manager.handles[s.id]?.running == true
            let state = store.agentState(s)
            switch name {
            case "Needs you": return running && state?.needsYou == true
            case "Working": return running && (state == .working || (state == nil && s.agent == .codex) || state == nil)
            case "Done": return running && state == .done
            default: return !running
            }
        }.sorted { $0.1.createdAt > $1.1.createdAt }
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Text("Agent Dashboard").font(.system(size: 15, weight: .semibold))
                Spacer()
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass").font(.system(size: 11)).foregroundStyle(.secondary)
                    TextField("Filter", text: $filter).textFieldStyle(.plain).font(.system(size: 12)).frame(width: 200)
                }.padding(.horizontal, 8).frame(height: 26).background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 6))
                Text("\(manager.runningCount) running").font(.caption).foregroundStyle(.secondary)
            }.padding(.horizontal, 16).frame(height: 44).background(AppTheme.raised)
            Divider()
            HStack(alignment: .top, spacing: 12) {
                ForEach([("Needs you", "questionmark.circle.fill", Color.orange), ("Working", "circle.fill", AppTheme.accent), ("Done", "checkmark.circle.fill", Color.green), ("Idle", "moon.zzz", Color.secondary)], id: \.0) { name, icon, tint in
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 6) {
                            Image(systemName: icon).font(.system(size: 11)).foregroundStyle(tint)
                            Text(name.uppercased()).font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
                            Text("\(column(name).count)").font(.system(size: 10)).foregroundStyle(.tertiary)
                        }
                        ScrollView {
                            LazyVStack(spacing: 6) {
                                ForEach(column(name), id: \.1.id) { p, s in card(p, s) }
                            }
                        }
                    }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                }
            }.padding(16)
        }.frame(minWidth: 820, minHeight: 420).background(AppTheme.window)
    }

    private func card(_ project: Project, _ session: LinkedSession) -> some View {
        let running = manager.handles[session.id]?.running == true
        let record = status.records[session.sessionID.lowercased()]
        return Button {
            store.openSession(session.id, in: project.id)
            NSApp.activate(ignoringOtherApps: true)
            NSApp.windows.first { $0.identifier?.rawValue.contains("workspace") == true || $0.title == "Convoy" }?.makeKeyAndOrderFront(nil)
        } label: {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    AgentStateGlyph(state: store.agentState(session), running: running, size: 6).frame(width: 10)
                    Text(session.title).font(.system(size: 12.5, weight: .medium)).lineLimit(2)
                }
                HStack(spacing: 6) {
                    ProjectIconView(project: project, size: 11)
                    Text(project.name).font(.system(size: 10.5)).foregroundStyle(.secondary).lineLimit(1)
                    if let b = session.branch { Text("⎇ " + b).font(.system(size: 10)).foregroundStyle(AppTheme.accent).lineLimit(1) }
                }
                HStack {
                    Text(session.agent.rawValue).font(.system(size: 10)).foregroundStyle(.tertiary)
                    Spacer()
                    if let record, running { Text(record.at.formatted(.relative(presentation: .named))).font(.system(size: 10)).foregroundStyle(.tertiary) }
                    else if !running { Text(store.isHibernated(session.id) ? "sleeping" : "stopped").font(.system(size: 10)).foregroundStyle(.tertiary) }
                }
            }.padding(10).frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
                .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 8))
                .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(AppTheme.stroke))
        }.buttonStyle(.plain)
    }
}

/// Find in the terminal buffer using SwiftTerm's search service.
struct TerminalSearchBar: View {
    let handle: TerminalHandle
    @Binding var shown: Bool
    @State private var term = ""
    @State private var caseSensitive = false
    @State private var summary: (Int, Int) = (0, 0)
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
            TextField("Find in terminal", text: $term).textFieldStyle(.plain).font(.system(size: 12)).focused($focused)
                .onSubmit { if NSEvent.modifierFlags.contains(.shift) { previous() } else { next() } }
                .onChange(of: term) { _, _ in next(fromStart: true) }
            Text(term.isEmpty ? "" : (summary.1 == 0 ? "No matches" : "\(summary.0) of \(summary.1)")).font(.system(size: 11)).foregroundStyle(summary.1 == 0 && !term.isEmpty ? .orange : .secondary).monospacedDigit()
            Toggle(isOn: $caseSensitive) { Text("Aa").font(.system(size: 11, weight: .semibold)) }.toggleStyle(.button).controlSize(.small).help("Match case")
                .onChange(of: caseSensitive) { _, _ in next(fromStart: true) }
            Button { previous() } label: { Image(systemName: "chevron.up") }.buttonStyle(.plain).help("Previous (⇧↩)")
            Button { next() } label: { Image(systemName: "chevron.down") }.buttonStyle(.plain).help("Next (↩)")
            Button { close() } label: { Image(systemName: "xmark") }.buttonStyle(.plain).help("Close (esc)")
        }.font(.system(size: 12)).padding(.horizontal, 12).frame(height: 32).background(AppTheme.raised).overlay(alignment: .bottom) { Divider() }
            .onAppear { focused = true }
            .onExitCommand { close() }
    }

    private var options: SearchOptions { SearchOptions(caseSensitive: caseSensitive, regex: false, wholeWord: false) }
    private func next(fromStart: Bool = false) {
        guard !term.isEmpty else { handle.view.clearSearch(); summary = (0, 0); return }
        _ = handle.view.findNext(term, options: options)
        summary = handle.view.searchMatchSummary(term, options: options)
    }
    private func previous() {
        guard !term.isEmpty else { return }
        _ = handle.view.findPrevious(term, options: options)
        summary = handle.view.searchMatchSummary(term, options: options)
    }
    private func close() {
        handle.view.clearSearch(); shown = false
        DispatchQueue.main.async { handle.view.window?.makeFirstResponder(handle.view) }
    }
}
