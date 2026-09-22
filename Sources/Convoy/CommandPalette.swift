import SwiftUI

struct PaletteItem: Identifiable {
    enum Kind: String { case session = "Session", project = "Project", action = "Action" }
    let id: String
    let kind: Kind
    let title: String
    let subtitle: String
    let icon: String
    var running = false
    var needsYou = false
    var shortcut: String? = nil
    let action: () -> Void
}

/// ⌘K palette: jump to any session or project, or run an app action.
struct CommandPalette: View {
    @EnvironmentObject var store: Store
    @AppStorage("appearance") private var appearance = "system"
    @State private var query = ""
    @State private var highlighted = 0
    @FocusState private var focused: Bool

    private var allItems: [PaletteItem] {
        var items: [PaletteItem] = []
        for project in store.workspace.projects {
            for session in project.linkedSessions where session.archived != true {
                let running = store.terminals.handles[session.id]?.running == true
                let state = store.agentState(session)
                let stateText = running ? (state.map { " · " + AgentStateGlyph(state: $0, running: true).label.lowercased() } ?? "") : ""
                items.append(PaletteItem(id: "s-\(session.id)", kind: .session, title: session.title,
                                         subtitle: "\(project.name) · \(session.agent.rawValue)\(session.branch.map { " · ⎇ " + $0 } ?? "")\(session.reviewOf != nil ? " · review" : "")\(stateText)",
                                         icon: "terminal", running: running, needsYou: running && state?.needsYou == true) { store.openSession(session.id, in: project.id) })
            }
        }
        for project in store.workspace.projects {
            items.append(PaletteItem(id: "p-\(project.id)", kind: .project, title: project.name,
                                     subtitle: project.isGroup ? "Group · \(project.path)" : project.path,
                                     icon: project.isGroup ? "folder.fill" : "folder") { store.selectProject(project.id) })
        }
        let actions: [(String, String, String, String?, () -> Void)] = [
            ("New session in current project", "Creates a Claude Code or Codex session", "plus.rectangle", "⌘N", { store.newSessionInCurrentProject() }),
            ("Open folder…", "Add a repository or a folder of projects", "folder.badge.plus", "⌘O", { store.addProject() }),
            ("Resume current session", "Reconnect to the selected agent", "play", "⇧⌘R", { store.resumeCurrentSession() }),
            ("Stop current session", "Interrupts the running agent", "stop", "⌘.", { store.stopCurrentSession() }),
            ("Close tab", "Closes the selected terminal tab", "xmark.rectangle", "⌘W", { store.closeCurrentTab() }),
            ("Toggle sidebar", "Show or hide the project list", "sidebar.left", "⌘B", { store.toggleSidebar() }),
            ("AI Limits", "Account usage for Codex and Claude", "chart.bar.xaxis", "⇧⌘L", { store.showLimitsRequest = UUID() }),
            ("Refresh Codex limits", "Reads the current usage from your login", "arrow.clockwise", nil, { store.usage.refresh() }),
            ("Theme: System", "Follow the macOS appearance", "circle.lefthalf.filled", nil, { appearance = "system" }),
            ("Theme: Light", "", "sun.max", nil, { appearance = "light" }),
            ("Theme: Dark", "", "moon.fill", nil, { appearance = "dark" }),
            ("Settings…", "Appearance, terminal, agents, git, notifications", "gearshape", "⌘,", { store.showSettings = true }),
            ("Reopen closed tab", "", "arrow.uturn.backward", "⇧⌘T", { store.reopenClosedTab() }),
            ("Edit session name & notes", "", "pencil", "⌘I", { store.editRequest = store.workspace.selectedSessionID }),
            ("Start review of current session", "Hands the work to the other agent", "checkmark.bubble", "⌥⌘R", { store.reviewRequest = store.workspace.selectedSessionID }),
            ("Send feedback to builder", "From a review session", "arrowshape.turn.up.left", "⇧⌘B", { store.feedbackRequest = store.workspace.selectedSessionID }),
            ("Switch terminal", "Open terminals, most recent first", "rectangle.on.rectangle", "⌘E", { store.openPalette(.terminals) }),
            ("Go to Sessions", "", "terminal", "⌥⌘1", { store.areaRequest = "Sessions" }),
            ("Go to Reviews", "", "checkmark.bubble", "⌥⌘2", { store.areaRequest = "Reviews" }),
            ("Go to Specs", "", "doc.text", "⌥⌘3", { store.areaRequest = "Specs" }),
            ("New specification", "", "doc.badge.plus", "⌥⌘N", { store.newSpecRequest = UUID() }),
            ("New task", "Queue work for an agent in the current project", "checklist", "⇧⌘N", { store.requestNewTask() }),
            ("Go to History", "", "clock.arrow.circlepath", "⌥⌘4", { store.areaRequest = "History" }),
            ("Go to Tasks", "", "checklist", "⌥⌘5", { store.areaRequest = "Tasks" }),
            ("Go to Docs & Specs", "", "book", "⌥⌘6", { store.areaRequest = "Docs" }),
            ("Next section", "", "chevron.right", "⇧⌘]", { store.cycleArea(1) }),
            ("Previous section", "", "chevron.left", "⇧⌘[", { store.cycleArea(-1) }),
            ("Show project in Finder", "", "folder", "⌥⌘F", { if let p = store.project { NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: p.path) } }),
            ("Copy project path", "", "doc.on.doc", "⌥⌘C", { if let p = store.project { store.copy(p.path) } }),
            ("Open Claude /usage", "Runs /usage in the current project", "gauge", "⌥⌘U", { store.openClaudeUsage() }),
            ("Toggle keep awake", "", "cup.and.saucer", "⌥⌘K", { store.wake.mode = store.wake.mode == "off" ? "sessions" : "off" }),
            ("Find project in sidebar", "", "magnifyingglass", "⌘F", { store.searchFocusRequest = UUID() }),
        ]
        for (title, subtitle, icon, shortcut, run) in actions {
            items.append(PaletteItem(id: "a-\(title)", kind: .action, title: title, subtitle: subtitle, icon: icon, shortcut: shortcut, action: run))
        }
        return items
    }

    /// Open terminals, most recently used first, with the current one last so Return jumps "back".
    private var terminalItems: [PaletteItem] {
        let tabs = store.tabOrder
        let current = store.workspace.selectedSessionID
        let ordered = store.recentTabs.filter { tabs.contains($0) && $0 != current } + tabs.filter { !store.recentTabs.contains($0) && $0 != current } + (current.map { [$0] } ?? [])
        return ordered.compactMap { id in
            guard let project = store.project(ofSession: id), let session = store.session(id) else { return nil }
            let index = tabs.firstIndex(of: id) ?? 0
            let running = store.terminals.handles[id]?.running == true
            return PaletteItem(id: "t-\(id)", kind: .session, title: session.title,
                               subtitle: "\(project.name) · \(session.agent.rawValue)\(id == current ? " · current" : "")",
                               icon: "terminal", running: running, shortcut: index < 9 ? "⌘\(index + 1)" : nil) { store.openSession(id, in: project.id) }
        }
    }

    private var quickItems: [PaletteItem] {
        guard let id = store.workspace.selectedSessionID else { return [] }
        return store.quickCommands(for: store.project).map { c in
            PaletteItem(id: "q-\(c.id)", kind: .action, title: c.title, subtitle: String(c.text.prefix(90)).replacingOccurrences(of: "\n", with: " "), icon: c.submit ? "return" : "text.cursor", shortcut: c.projectID == nil ? "global" : nil) { store.send(c, to: id) }
        }
    }

    private var results: [PaletteItem] {
        let q = query.trimmingCharacters(in: .whitespaces)
        if store.paletteMode == .quick {
            let items = quickItems
            return q.isEmpty ? items : items.filter { PaletteMatch.score(query: q, title: $0.title, subtitle: $0.subtitle) > 0 }
        }
        if store.paletteMode == .terminals {
            let items = terminalItems
            return q.isEmpty ? items : items.filter { PaletteMatch.score(query: q, title: $0.title, subtitle: $0.subtitle) > 0 }
        }
        if q.isEmpty {
            let items = allItems
            return items.filter { $0.kind == .session && $0.needsYou } + items.filter { $0.kind == .session && $0.running && !$0.needsYou } + items.filter { $0.kind == .session && !$0.running }.prefix(6)
                + items.filter { $0.kind == .action }.prefix(6)
        }
        return allItems.compactMap { item in
            let score = PaletteMatch.score(query: q, title: item.title, subtitle: item.subtitle)
            return score > 0 ? (item, score + (item.needsYou ? 10 : 0) + (item.running ? 5 : 0) + (item.kind == .session ? 2 : 0)) : nil
        }.sorted { $0.1 > $1.1 }.prefix(40).map(\.0)
    }

    var body: some View {
        ZStack(alignment: .top) {
            Color.black.opacity(0.35).ignoresSafeArea().onTapGesture { store.showPalette = false }
            VStack(spacing: 0) {
                HStack(spacing: 10) {
                    Image(systemName: "magnifyingglass").foregroundStyle(.secondary)
                    TextField(store.paletteMode == .terminals ? "Switch to an open terminal…" : (store.paletteMode == .quick ? "Send a quick command to this terminal…" : "Jump to a session or project, or run a command…"), text: $query)
                        .textFieldStyle(.plain).font(.system(size: 15)).focused($focused)
                        .onSubmit { run(highlighted) }
                    Text("esc").font(.system(size: 10, weight: .medium)).foregroundStyle(.tertiary)
                        .padding(.horizontal, 5).padding(.vertical, 2)
                        .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(AppTheme.stroke))
                }.padding(.horizontal, 16).frame(height: 48)
                Divider()
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(spacing: 1) {
                            let list = results
                            ForEach(Array(list.enumerated()), id: \.element.id) { index, item in
                                row(item, highlighted: index == highlighted).id(item.id)
                                    .onTapGesture { run(index) }
                                    .onHover { if $0 { highlighted = index } }
                            }
                            if list.isEmpty {
                                Text(store.paletteMode == .terminals && query.isEmpty ? "No open terminals. Pick a session in the sidebar or press ⌘N." : (store.paletteMode == .quick && query.isEmpty ? "No quick commands yet — add them in Settings → Quick Commands." : "No matches"))
                                    .font(.callout).foregroundStyle(.secondary).padding(20)
                            }
                        }.padding(6)
                    }.frame(maxHeight: 380)
                        .onChange(of: highlighted) { _, index in
                            let list = results
                            if list.indices.contains(index) { proxy.scrollTo(list[index].id) }
                        }
                }
                Divider()
                HStack(spacing: 14) {
                    hint("↑↓", "navigate"); hint("↩", "open"); hint("esc", "close")
                    Spacer()
                    Text("\(results.count) results").font(.system(size: 10)).foregroundStyle(.tertiary)
                }.padding(.horizontal, 14).frame(height: 28)
            }
            .frame(width: 600)
            .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(Color.primary.opacity(0.12)))
            .shadow(color: .black.opacity(0.35), radius: 30, y: 12)
            .padding(.top, 90)
        }
        .onAppear { focused = true }
        .onChange(of: query) { _, _ in highlighted = 0 }
        .onKeyPress(.escape) { store.showPalette = false; return .handled }
        .onKeyPress(.downArrow) { highlighted = min(highlighted + 1, max(0, results.count - 1)); return .handled }
        .onKeyPress(.upArrow) { highlighted = max(highlighted - 1, 0); return .handled }
        .onExitCommand { store.showPalette = false }
    }

    private func run(_ index: Int) {
        let list = results
        guard list.indices.contains(index) else { return }
        store.showPalette = false
        list[index].action()
    }

    private func hint(_ key: String, _ label: String) -> some View {
        HStack(spacing: 4) {
            Text(key).font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
                .padding(.horizontal, 4).padding(.vertical, 1).background(Color.primary.opacity(0.07), in: RoundedRectangle(cornerRadius: 3))
            Text(label).font(.system(size: 10)).foregroundStyle(.tertiary)
        }
    }

    private func row(_ item: PaletteItem, highlighted: Bool) -> some View {
        HStack(spacing: 10) {
            Image(systemName: item.icon).font(.system(size: 13)).foregroundStyle(highlighted ? AppTheme.accent : .secondary).frame(width: 20)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 6) {
                    Text(item.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    if item.needsYou { Text("needs you").font(.system(size: 9, weight: .semibold)).foregroundStyle(.orange) }
                    else if item.running { Circle().fill(Color.green).frame(width: 6, height: 6) }
                }
                if !item.subtitle.isEmpty {
                    Text(item.subtitle).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                }
            }
            Spacer()
            if let shortcut = item.shortcut {
                Text(shortcut).font(.system(size: 11, weight: .medium)).foregroundStyle(.tertiary)
            } else {
                Text(item.kind.rawValue).font(.system(size: 10)).foregroundStyle(.tertiary)
            }
        }.padding(.horizontal, 10).padding(.vertical, 7)
            .background(highlighted ? AppTheme.accent.opacity(0.18) : .clear, in: RoundedRectangle(cornerRadius: 7))
            .contentShape(Rectangle())
    }
}

enum PaletteMatch {
    /// Lightweight ranking: prefix > word start > substring > subsequence, on title then subtitle.
    static func score(query: String, title: String, subtitle: String) -> Int {
        let q = query.lowercased(), t = title.lowercased(), s = subtitle.lowercased()
        if t.hasPrefix(q) { return 100 }
        if t.split(separator: " ").contains(where: { $0.hasPrefix(q) }) { return 80 }
        if t.contains(q) { return 60 }
        if s.contains(q) { return 30 }
        if isSubsequence(q, of: t) { return 15 }
        return 0
    }
    static func isSubsequence(_ q: String, of text: String) -> Bool {
        var it = text.makeIterator()
        for ch in q {
            var found = false
            while let c = it.next() { if c == ch { found = true; break } }
            if !found { return false }
        }
        return true
    }
}
