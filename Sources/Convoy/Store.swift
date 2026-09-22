import AppKit
import SwiftUI

@MainActor
final class Store: ObservableObject {
    @Published private(set) var workspace = Workspace()
    @Published var error: String?
    @Published var notice: String?
    @Published private(set) var isImporting = false
    @Published var sessionCreationProject: Project?
    @Published var sessionFocusRequest = UUID()
    @Published var showPalette = false
    enum PaletteMode { case all, terminals, quick }
    @Published var paletteMode: PaletteMode = .all
    /// Session ids by most recent use, for the ⌘E terminal switcher.
    @Published private(set) var recentTabs: [UUID] = []
    func openPalette(_ mode: PaletteMode) { paletteMode = mode; showPalette = true }
    @Published var showLimitsRequest = UUID()
    @Published var closeTabRequest: UUID?
    @Published var stopRequest: UUID?
    @Published var editRequest: UUID?
    @Published var reviewRequest: UUID?
    @Published var feedbackRequest: UUID?
    @Published var areaRequest: String?
    @Published var newSpecRequest = UUID()
    @Published var searchFocusRequest = UUID()
    private var canSave = true
    private let file: WorkspaceFile
    let terminals = TerminalManager()
    let usage = UsageLimits()
    let wake = WakeControl()
    let agentStatus = AgentStatusStore()
    let git = GitInfoService()
    let notifier = Notifier()
    let accounts = Accounts()
    let keys = Keybindings.shared
    @Published var showSettings = false
    @Published var settingsSection = "General"
    /// Pane grid: 1, 2 or 4 slots, each holding a session from any project. Persisted.
    @Published var panes: [UUID?] = Store.loadPanes() { didSet { UserDefaults.standard.set(panes.map { $0?.uuidString ?? "" }, forKey: "paneLayout") } }
    @Published var focusedPane = 0
    private static func loadPanes() -> [UUID?] {
        let raw = UserDefaults.standard.stringArray(forKey: "paneLayout") ?? [""]
        let list = raw.map { UUID(uuidString: $0) }
        return [1, 2, 4].contains(list.count) ? list : [nil]
    }
    /// True when the user picked a project row (project overview with Sessions / Reviews / Specs); false shows Home or the session.
    @Published var showProjectPage = false
    @Published var showActivity = false
    @Published var activity: [ActivityEvent] = ActivityEvent.load()
    @Published var unreadActivity = 0
    @Published private(set) var hibernated: Set<UUID> = Set((UserDefaults.standard.stringArray(forKey: "hibernatedSessions") ?? []).compactMap(UUID.init)) {
        didSet { UserDefaults.standard.set(hibernated.map(\.uuidString), forKey: "hibernatedSessions") }
    }
    @Published var multiSelection: Set<UUID> = []
    @Published var quickRequest = UUID()
    @Published var findRequest = UUID()
    @Published var taskFromSpec: (UUID, String)?
    @Published var projectSettingsID: UUID?
    /// Collapsed sidebar rows, persisted so folders stay the way you left them.
    @Published private(set) var collapsed: Set<UUID> = Set((UserDefaults.standard.stringArray(forKey: "collapsedProjects") ?? []).compactMap(UUID.init)) {
        didSet { UserDefaults.standard.set(collapsed.map(\.uuidString), forKey: "collapsedProjects") }
    }
    func isExpanded(_ id: UUID) -> Bool { !collapsed.contains(id) }
    func toggleExpanded(_ id: UUID) { if collapsed.contains(id) { collapsed.remove(id) } else { collapsed.insert(id) } }
    /// Set by the new-session sheet's Advanced section; consumed by the next startSession.
    var skipSetupOnce = false
    @Published var dashboardRequest = UUID()
    private var hibernationTimer: Timer?

    /// One-time move from the SpecDesk name: copies app state (not worktrees, whose paths git already knows) and preferences.
    static func migrateFromSpecDesk() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let old = support.appendingPathComponent("SpecDesk"), new = support.appendingPathComponent("Convoy")
        let fm = FileManager.default
        if !fm.fileExists(atPath: new.appendingPathComponent("workspace.json").path), fm.fileExists(atPath: old.path) {
            try? fm.createDirectory(at: new, withIntermediateDirectories: true)
            for item in (try? fm.contentsOfDirectory(atPath: old.path)) ?? [] where item != "worktrees" {
                let target = new.appendingPathComponent(item)
                if !fm.fileExists(atPath: target.path) { try? fm.copyItem(at: old.appendingPathComponent(item), to: target) }
            }
        }
        if UserDefaults.standard.object(forKey: "migratedFromSpecDesk") == nil, let legacy = UserDefaults(suiteName: "dev.specdesk.mac") {
            for (key, value) in legacy.dictionaryRepresentation() where !key.hasPrefix("NS") && !key.hasPrefix("Apple") && UserDefaults.standard.object(forKey: key) == nil {
                UserDefaults.standard.set(value, forKey: key)
            }
            UserDefaults.standard.set(true, forKey: "migratedFromSpecDesk")
        }
    }

    init(workspaceURL: URL? = nil) {
        if workspaceURL == nil { Self.migrateFromSpecDesk() }
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        file = WorkspaceFile(url: workspaceURL ?? support.appendingPathComponent("Convoy/workspace.json"))
        do {
            // Preserve data created by the earlier sandboxed prototype.
            let legacy = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Containers/com.iftech.convoy/Data/Library/Application Support/Convoy/workspace.json")
            if workspaceURL == nil, !FileManager.default.fileExists(atPath: file.url.path), FileManager.default.fileExists(atPath: legacy.path) {
                let previous = try WorkspaceFile(url: legacy).load()
                try file.save(previous)
            }
            workspace = try file.load()
        }
        catch { self.error = "Could not open saved workspace. It has not been overwritten. \(error.localizedDescription)"; canSave = false }
        notifier.onOpen = { [weak self] id in
            guard let self, let project = self.project(ofSession: id) else { return }
            self.openSession(id, in: project.id)
        }
        agentStatus.onAttention = { [weak self] providerID, state in self?.handleAttention(providerID: providerID, state: state) }
        terminals.accounts = accounts
        hibernationTimer = Timer.scheduledTimer(withTimeInterval: 60, repeats: true) { [weak self] _ in Task { @MainActor in self?.hibernateIdleSessions() } }
    }

    // MARK: Activity feed

    func record(_ kind: ActivityEvent.Kind, session: LinkedSession, project: Project, detail: String = "") {
        activity.insert(ActivityEvent(kind: kind, sessionID: session.id, sessionTitle: session.title, projectName: project.name, detail: detail), at: 0)
        if activity.count > 300 { activity.removeLast(activity.count - 300) }
        if !showActivity { unreadActivity += 1 }
        ActivityEvent.save(activity)
    }
    func markActivityRead() { unreadActivity = 0 }

    // MARK: Hibernation (Orca-style: stop finished, idle agents; resume when reopened)

    var hibernateAfterMinutes: Int { UserDefaults.standard.object(forKey: "hibernateAfterMinutes") as? Int ?? 0 }

    func hibernateIdleSessions() {
        guard hibernateAfterMinutes > 0 else { return }
        for project in workspace.projects {
            for session in project.linkedSessions where session.agent == .claude && terminals.handles[session.id]?.running == true {
                guard session.id != workspace.selectedSessionID, !panes.contains(session.id),
                      let record = agentStatus.records[session.sessionID.lowercased()], record.state == .done,
                      Date().timeIntervalSince(record.at) > Double(hibernateAfterMinutes) * 60 else { continue }
                sleep(session, in: project, automatic: true)
            }
        }
    }

    func sleep(_ session: LinkedSession, in project: Project, automatic: Bool = false) {
        guard let handle = terminals.handles[session.id], handle.running else { return }
        handle.stop()
        hibernated.insert(session.id)
        record(.slept, session: session, project: project, detail: automatic ? "Idle after finishing — hibernated to free memory" : "Put to sleep")
    }
    func isHibernated(_ id: UUID) -> Bool { hibernated.contains(id) }

    // MARK: Pane grid

    var paneCount: Int { panes.count }
    var visiblePaneSessions: [UUID] { panes.compactMap { $0 }.filter { tabOrder.contains($0) } }

    func setLayout(_ count: Int) {
        guard [1, 2, 4].contains(count), count != panes.count else { return }
        var next = panes
        if count < next.count {
            // Keep the focused pane first, then the rest in order.
            let focused = next.indices.contains(focusedPane) ? next.remove(at: focusedPane) : nil
            next = [focused] + next
            next = Array(next.prefix(count))
            focusedPane = 0
        } else {
            next += Array(repeating: nil, count: count - next.count)
        }
        panes = next
    }

    /// Puts a session into a pane. If it is already visible, that pane is focused instead.
    func place(_ id: UUID, in pane: Int? = nil) {
        if let existing = panes.firstIndex(of: id) { focusedPane = existing; return }
        let target = pane ?? focusedPane
        guard panes.indices.contains(target) else { return }
        panes[target] = id
        focusedPane = target
    }
    func closePane(_ index: Int) {
        guard panes.indices.contains(index) else { return }
        panes[index] = nil
        if panes.compactMap({ $0 }).isEmpty { goHome() }
        else if focusedPane == index, let next = panes.firstIndex(where: { $0 != nil }) { focusPane(next) }
    }
    func focusPane(_ index: Int) {
        guard panes.indices.contains(index) else { return }
        focusedPane = index
        if let id = panes[index], let project = project(ofSession: id) {
            var next = workspace
            next.selectedProjectID = project.id; next.selectedSessionID = id
            commit(next)
            showProjectPage = false; showSettings = false
            recentTabs.removeAll { $0 == id }; recentTabs.insert(id, at: 0)
            if let session = session(id), !session.sessionID.isEmpty { agentStatus.acknowledge(session.sessionID) }
            updateBadge()
            sessionFocusRequest = UUID()
        }
    }
    func focusPane(offset: Int) {
        let filled = panes.indices.filter { panes[$0] != nil }
        guard !filled.isEmpty else { return }
        let current = filled.firstIndex(of: focusedPane) ?? 0
        focusPane(filled[((current + offset) % filled.count + filled.count) % filled.count])
    }

    // MARK: Pinning, reordering, multi-select

    func togglePin(_ session: LinkedSession, in project: Project) {
        var value = session; value.pinned = session.pinned == true ? nil : true
        updateSession(value, project: project.id)
    }
    var pinnedSessions: [(Project, LinkedSession)] {
        workspace.projects.flatMap { p in p.linkedSessions.filter { $0.pinned == true && $0.archived != true }.map { (p, $0) } }
    }
    func moveProject(_ id: UUID, by offset: Int) {
        var roots = workspace.rootProjects
        guard let index = roots.firstIndex(where: { $0.id == id }) else { return }
        let target = max(0, min(roots.count - 1, index + offset))
        guard target != index else { return }
        roots.move(fromOffsets: IndexSet(integer: index), toOffset: target > index ? target + 1 : target)
        var next = workspace
        for (i, root) in roots.enumerated() { if let pi = next.projects.firstIndex(where: { $0.id == root.id }) { next.projects[pi].order = i } }
        commit(next)
    }
    func moveProject(_ id: UUID, before targetID: UUID) {
        var roots = workspace.rootProjects
        guard let from = roots.firstIndex(where: { $0.id == id }), let to = roots.firstIndex(where: { $0.id == targetID }), from != to else { return }
        let item = roots.remove(at: from)
        roots.insert(item, at: to > from ? to - 1 : to)
        var next = workspace
        for (i, root) in roots.enumerated() { if let pi = next.projects.firstIndex(where: { $0.id == root.id }) { next.projects[pi].order = i } }
        commit(next)
    }
    func updateProject(_ project: Project) {
        var next = workspace
        guard let pi = next.projects.firstIndex(where: { $0.id == project.id }) else { return }
        next.projects[pi] = project
        commit(next)
    }
    func archive(sessions ids: Set<UUID>) {
        var next = workspace
        for pi in next.projects.indices {
            for si in (next.projects[pi].sessions ?? []).indices where ids.contains(next.projects[pi].sessions![si].id) && terminals.handles[next.projects[pi].sessions![si].id]?.running != true {
                next.projects[pi].sessions![si].archived = true
                next.projects[pi].sessions![si].pinned = nil
            }
        }
        commit(next); multiSelection = []
    }
    func closeTabs(_ ids: Set<UUID>) {
        let closable = ids.filter { terminals.handles[$0]?.running != true }
        for id in closable { terminals.close(id) }
        panes = panes.map { $0.map { closable.contains($0) ? nil : $0 } ?? nil }
        multiSelection = []
        if let current = workspace.selectedSessionID, !tabOrder.contains(current) {
            if let last = tabOrder.last, let project = project(ofSession: last) { openSession(last, in: project.id) } else { goHome() }
        }
    }

    // MARK: Quick commands

    var quickCommands: [QuickCommand] { workspace.quickCommands ?? [] }
    func quickCommands(for project: Project?) -> [QuickCommand] {
        quickCommands.filter { $0.projectID == nil || $0.projectID == project?.id }
    }
    func saveQuickCommand(_ command: QuickCommand) {
        var next = workspace
        var list = next.quickCommands ?? []
        if let i = list.firstIndex(where: { $0.id == command.id }) { list[i] = command } else { list.append(command) }
        next.quickCommands = list; commit(next)
    }
    func deleteQuickCommand(_ id: UUID) {
        var next = workspace; next.quickCommands = (next.quickCommands ?? []).filter { $0.id != id }; commit(next)
    }
    /// Inserts text into a running terminal using bracketed paste; optionally presses Enter.
    func send(_ command: QuickCommand, to sessionID: UUID) {
        guard let handle = terminals.handles[sessionID], handle.running else { error = "Start or resume the session first."; return }
        let safe = command.text.replacingOccurrences(of: "\u{1b}", with: "")
        handle.view.send(txt: "\u{1b}[200~" + safe + "\u{1b}[201~" + (command.submit ? "\r" : ""))
    }

    // MARK: Source control (runs git in the session's directory; output surfaces in the panel)

    func gitCommand(_ args: [String], in directory: String, timeout: TimeInterval = 60) async -> (ok: Bool, output: String) {
        await Task.detached(priority: .userInitiated) { () -> (Bool, String) in
            guard let (status, out) = GitInfoService.run(args, in: directory, timeout: timeout) else { return (false, "git timed out") }
            return (status == 0, out.trimmingCharacters(in: .whitespacesAndNewlines))
        }.value
    }

    /// Asks Claude for a conventional commit message from the staged (or all) changes. Uses the active managed account.
    func generateCommitMessage(in directory: String) async -> String? {
        let env = accounts.environment(for: .claude)
        return await Task.detached(priority: .userInitiated) { () -> String? in
            var diff = GitInfoService.run(["diff", "--cached", "--stat", "-p", "--no-color"], in: directory)?.1 ?? ""
            if diff.trimmingCharacters(in: .whitespaces).isEmpty { diff = GitInfoService.run(["diff", "--stat", "-p", "--no-color"], in: directory)?.1 ?? "" }
            guard !diff.trimmingCharacters(in: .whitespaces).isEmpty else { return nil }
            let prompt = "Write a git commit message for this diff. First line: imperative summary under 72 characters. Then a blank line and 1-4 short bullet points if useful. Output only the message, no code fences.\n\n" + String(diff.prefix(60_000))
            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/bin/zsh")
            process.arguments = ["-ilc", "claude -p --max-turns 1 --output-format text"]
            var environment = ProcessInfo.processInfo.environment
            for key in environment.keys where key.hasPrefix("CLAUDE") && key != "CLAUDE_CONFIG_DIR" { environment.removeValue(forKey: key) }
            environment.merge(env) { $1 }
            process.environment = environment
            process.currentDirectoryURL = URL(fileURLWithPath: directory)
            let input = Pipe(), output = Pipe()
            process.standardInput = input; process.standardOutput = output; process.standardError = Pipe()
            do { try process.run() } catch { return nil }
            input.fileHandleForWriting.write(prompt.data(using: .utf8)!)
            try? input.fileHandleForWriting.close()
            let data = output.fileHandleForReading.readDataToEndOfFile()
            let started = Date()
            while process.isRunning { if Date().timeIntervalSince(started) > 90 { process.terminate(); return nil }; usleep(50_000) }
            let text = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            return text.isEmpty ? nil : text.replacingOccurrences(of: "```", with: "")
        }.value
    }

    // MARK: Accounts

    func addAccount(_ agent: Agent, label: String) {
        do {
            let account = try accounts.add(agent, label: label)
            guard let project = project ?? workspace.projects.first else { notice = "Account created. Open a project, then log in from Settings → Accounts."; return }
            let session = LinkedSession(agent: agent, sessionID: "", title: "Login: \(agent.rawValue) · \(account.label)")
            linkSession(session, to: project.id)
            terminals.startLogin(for: account, project: project, session: session)
            openSession(session.id, in: project.id)
        } catch { self.error = error.localizedDescription }
    }

    // MARK: Agent status & notifications

    /// Agent state for a Convoy session, if Claude hooks have reported one.
    func agentState(_ session: LinkedSession) -> AgentState? {
        guard session.agent == .claude, !session.sessionID.isEmpty else { return nil }
        return agentStatus.state(for: session.sessionID)
    }
    func agentState(_ id: UUID) -> AgentState? { session(id).flatMap(agentState) }

    /// Sessions currently waiting for input, for the Dock badge and status bar.
    var needsYou: [LinkedSession] {
        workspace.projects.flatMap(\.linkedSessions).filter { $0.archived != true && terminals.handles[$0.id]?.running == true && agentState($0)?.needsYou == true }
    }

    private func handleAttention(providerID: String, state: AgentState) {
        let defaults = UserDefaults.standard
        guard defaults.object(forKey: "notificationsEnabled") as? Bool ?? true else { updateBadge(); return }
        guard let project = workspace.projects.first(where: { $0.linkedSessions.contains { $0.sessionID.lowercased() == providerID } }),
              let session = project.linkedSessions.first(where: { $0.sessionID.lowercased() == providerID }),
              terminals.handles[session.id]?.running == true else { return }
        updateBadge()
        record(state == .waiting ? .waiting : .done, session: session, project: project)
        if state == .done { taskAgentFinished(sessionID: session.id) }
        let wantWaiting = defaults.object(forKey: "notifyWaiting") as? Bool ?? true
        let wantDone = defaults.object(forKey: "notifyDone") as? Bool ?? true
        guard (state == .waiting && wantWaiting) || (state == .done && wantDone) else { return }
        let focused = NSApp.isActive && workspace.selectedSessionID == session.id
        if focused, defaults.object(forKey: "suppressWhenFocused") as? Bool ?? true { return }
        guard agentStatus.shouldNotify(providerID) else { return }
        let sound = defaults.string(forKey: "notificationSound") ?? "default"
        notifier.post(title: state == .waiting ? "\(session.title) needs you" : "\(session.title) finished",
                      body: state == .waiting ? "\(project.name) · waiting for input or permission" : "\(project.name) · ready for review",
                      sessionID: session.id, sound: sound)
    }

    func updateBadge() {
        let count = needsYou.count
        NSApp.dockTile.badgeLabel = count > 0 ? "\(count)" : nil
    }

    // MARK: Git

    /// Directory a session runs in: its worktree when it has one, otherwise the project folder.
    func directory(for session: LinkedSession, in project: Project) -> String { session.workingDirectory ?? project.path }

    /// Keeps branch/dirty info fresh for what is on screen.
    func trackGit() {
        var paths = workspace.projects.filter { !$0.isGroup }.map(\.path)
        paths += workspace.projects.flatMap(\.linkedSessions).compactMap(\.workingDirectory)
        git.track(paths)
    }

    func removeWorktree(_ session: LinkedSession, in project: Project, deleteBranch: Bool) {
        guard let path = session.workingDirectory, terminals.handles[session.id]?.running != true else { error = "Stop the session before removing its worktree."; return }
        do {
            try GitWorktree.remove(repo: project.path, path: path, deleteBranch: deleteBranch ? session.branch : nil)
            var value = session; value.workingDirectory = nil; value.archived = true
            updateSession(value, project: project.id)
            notice = "Worktree removed\(deleteBranch ? " and branch deleted" : ""). Session archived."
        } catch { self.error = error.localizedDescription }
    }

    var project: Project? { workspace.projects.first { $0.id == workspace.selectedProjectID } }
    var spec: Specification? { project?.specs.first { $0.id == workspace.selectedSpecID } }
    var attentionCount: Int {
        workspace.projects.flatMap(\.specs).flatMap(\.tasks).filter { $0.status == .review || $0.status == .changes }.count
    }

    func commit(_ candidate: Workspace) {
        guard canSave else { error = "Workspace recovery is required before saving. Your existing file is preserved at \(file.url.path)."; return }
        var candidate = candidate
        candidate.schemaVersion = 2
        do { try file.save(candidate); workspace = candidate }
        catch { self.error = "Changes could not be saved: \(error.localizedDescription)" }
    }

    func selectProject(_ id: UUID) {
        var next = workspace
        next.selectedProjectID = id
        next.selectedSpecID = next.projects.first { $0.id == id }?.specs.first?.id
        next.selectedSessionID = nil
        commit(next)
        showProjectPage = true; showSettings = false
    }

    /// Home: nothing selected, keep the current project for ⌘N.
    func goHome() {
        var next = workspace; next.selectedSessionID = nil; commit(next)
        showProjectPage = false; showSettings = false; projectSettingsID = nil
        panes = Array(repeating: nil, count: panes.count)
    }

    /// The session to display: only sessions that are open as tabs are shown; anything else lands on Home.
    var displayedSession: LinkedSession? {
        guard let id = workspace.selectedSessionID, tabOrder.contains(id), panes.contains(id) else { return nil }
        return session(id)
    }

    func selectSpec(_ id: UUID) {
        var next = workspace; next.selectedSpecID = id; commit(next)
    }

    func removeProject(_ id: UUID) {
        guard !terminals.hasRunningSessions(in: id) else {
            error = "Stop this workspace's running sessions before removing it."; return
        }
        var next = workspace
        next.removeProject(id)
        commit(next)
    }

    func addProject() {
        guard !isImporting else { return }
        let panel = NSOpenPanel()
        panel.title = "Open a project or project collection"
        panel.message = "Choose a project, or a parent folder to import the projects inside it as groups. Agents can work from a group's parent folder."
        panel.canChooseDirectories = true; panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        importFolder(url)
    }

    func importSubprojects(_ project: Project) {
        guard !isImporting else { return }
        importFolder(URL(fileURLWithPath: project.path), forceGroup: true)
    }

    private func importFolder(_ url: URL, forceGroup: Bool = false) {
        isImporting = true
        Task {
            let accessed = url.startAccessingSecurityScopedResource()
            defer {
                if accessed { url.stopAccessingSecurityScopedResource() }
                isImporting = false
            }
            do {
                var imported = try await Task.detached(priority: .userInitiated) {
                    try ProjectDiscovery().scan(url, forceGroup: forceGroup)
                }.value
                for i in imported.indices {
                    imported[i].bookmark = try URL(fileURLWithPath: imported[i].path).bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
                }
                var next = workspace
                next.importProjects(imported)
                commit(next)
                if error == nil {
                    let count = imported.filter { !$0.isGroup }.count
                    notice = "Imported \(count) \(count == 1 ? "project" : "projects") · Existing specs and sessions preserved"
                }
            } catch { self.error = "Import failed; existing projects were kept. \(error.localizedDescription)" }
        }
    }

    func linkSession(_ session: LinkedSession, to id: UUID) {
        guard let index = workspace.projects.firstIndex(where: { $0.id == id }) else { return }
        var next = workspace
        var sessions = next.projects[index].linkedSessions
        sessions.append(session)
        next.projects[index].sessions = sessions
        next.selectedSessionID = session.id
        commit(next)
    }

    func selectSession(_ id: UUID) {
        var next = workspace; next.selectedSessionID = id; commit(next)
    }

    /// Looks up the project that owns a session.
    func project(ofSession id: UUID) -> Project? {
        workspace.projects.first { $0.linkedSessions.contains { $0.id == id } }
    }
    func session(_ id: UUID) -> LinkedSession? {
        project(ofSession: id)?.linkedSessions.first { $0.id == id }
    }

    // MARK: Keyboard-driven navigation

    func toggleSidebar() {
        let current = UserDefaults.standard.object(forKey: "showSidebar") as? Bool ?? true
        UserDefaults.standard.set(!current, forKey: "showSidebar")
    }
    func newSessionInCurrentProject() {
        guard let project else { return }
        sessionCreationProject = project
    }
    /// Open tab ids in display order (archived or removed sessions are skipped).
    var tabOrder: [UUID] { terminals.openOrder.filter { session($0).map { $0.archived != true } ?? false } }
    func selectTab(index: Int) {
        let tabs = tabOrder
        guard tabs.indices.contains(index), let project = project(ofSession: tabs[index]) else { return }
        openSession(tabs[index], in: project.id)
    }
    func selectTab(offset: Int) {
        let tabs = tabOrder
        guard !tabs.isEmpty else { return }
        let current = workspace.selectedSessionID.flatMap { tabs.firstIndex(of: $0) } ?? -offset
        selectTab(index: ((current + offset) % tabs.count + tabs.count) % tabs.count)
    }
    func closeCurrentTab() {
        guard let id = workspace.selectedSessionID else { return }
        if tabOrder.contains(id) { closeTabRequest = id } else { goHome() }
    }
    func stopCurrentSession() {
        guard let id = workspace.selectedSessionID, terminals.handles[id]?.running == true else { return }
        stopRequest = id
    }
    func resumeCurrentSession() {
        guard let id = workspace.selectedSessionID, terminals.handles[id]?.running != true,
              let project = project(ofSession: id), let session = session(id) else { return }
        startSession(session, in: project, resume: true)
    }

    /// Jumps to a session in any project, switching the selected project if needed (used by terminal tabs).
    func openSession(_ id: UUID, in projectID: UUID) {
        var next = workspace
        if next.selectedProjectID != projectID {
            next.selectedProjectID = projectID
            next.selectedSpecID = next.projects.first { $0.id == projectID }?.specs.first?.id
        }
        next.selectedSessionID = id
        commit(next)
        showProjectPage = false; showSettings = false
        terminals.openTab(id)
        place(id)
        recentTabs.removeAll { $0 == id }; recentTabs.insert(id, at: 0)
        if let session = session(id), !session.sessionID.isEmpty { agentStatus.acknowledge(session.sessionID) }
        updateBadge()
        sessionFocusRequest = UUID()
        if hibernated.contains(id), terminals.handles[id]?.running != true, let session = session(id), let project = workspace.projects.first(where: { $0.id == projectID }) {
            hibernated.remove(id)
            startSession(session, in: project, resume: true)
        }
    }

    func openClaudeUsage() {
        guard let project else { return }
        if let existing = project.linkedSessions.last(where: { $0.agent == .claude && $0.initialPrompt == "/usage" && $0.archived != true }),
           terminals.handles[existing.id]?.running == true {
            selectSession(existing.id)
            sessionFocusRequest = UUID()
            notice = "Claude usage session selected. Run /usage there to refresh."
            return
        }
        let session = LinkedSession(agent: .claude, sessionID: UUID().uuidString.lowercased(), title: "Claude usage", initialPrompt: "/usage")
        startSession(session, in: project)
    }

    /// `worktreeBranch` creates (or reuses) a git worktree for the session before launching.
    func startSession(_ session: LinkedSession, in project: Project, resume: Bool = false, worktreeBranch: String? = nil, base: String? = nil) {
        guard canSave else { error = "Recover your workspace before launching a session."; return }
        if terminals.handles[session.id]?.running == true {
            openSession(session.id, in: project.id)
            return
        }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: project.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            error = "This workspace folder no longer exists. Reconnect its folder before launching a session."; return
        }
        var session = session
        if session.agentHome == nil {
            let key = session.agent == .claude ? "CLAUDE_CONFIG_DIR" : "CODEX_HOME"
            session.agentHome = accounts.environment(for: session.agent)[key]
                ?? ProcessInfo.processInfo.environment[key]
                ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(session.agent == .claude ? ".claude" : ".codex").path
        }
        if let home = session.agentHome, home.hasPrefix(Accounts.root.path + "/"),
           !FileManager.default.fileExists(atPath: home) {
            error = "The account used by this session was removed. Restore that account before resuming."
            return
        }
        var createdWorktree = false
        if let worktreeBranch, !worktreeBranch.isEmpty, session.workingDirectory == nil {
            do {
                session.workingDirectory = try GitWorktree.create(repo: project.path, projectName: project.name, branch: worktreeBranch, base: base)
                session.branch = worktreeBranch
                session.baseRef = (base?.isEmpty == false ? base : nil) ?? GitWorktree.defaultBase(project.path)
                createdWorktree = true
                let shared = (project.sharedPaths ?? []) + ((UserDefaults.standard.string(forKey: "worktreeSharedPaths") ?? "").split(separator: "\n").map(String.init))
                let notes = GitWorktree.materialize(sharedPaths: shared, from: project.path, into: session.workingDirectory!)
                if !notes.isEmpty { notice = "Shared paths: " + notes.joined(separator: ", ") }
            } catch { self.error = "Couldn’t create the worktree: \(error.localizedDescription)"; return }
        }
        if let dir = session.workingDirectory,
           (!FileManager.default.fileExists(atPath: dir, isDirectory: &isDirectory) || !isDirectory.boolValue) {
            error = "This session’s worktree is missing (\(dir)). Remove the worktree from the session menu, or recreate it."; return
        }
        if !project.linkedSessions.contains(where: { $0.id == session.id }) {
            linkSession(session, to: project.id)
        } else {
            updateSession(session, project: project.id)
        }
        guard workspace.projects.first(where: { $0.id == project.id })?.linkedSessions.first(where: { $0.id == session.id }) == session else { return }
        let directory = directory(for: session, in: project)
        // Resume is an explicit provider operation. A guessed transcript path is
        // not evidence that a conversation is absent (accounts and formats vary).
        // Never replay the initial task or silently create a replacement here.
        if session.agent == .claude, !session.sessionID.isEmpty, !resume { agentStatus.forget(session.sessionID) }
        hibernated.remove(session.id)
        var setup: String?
        if createdWorktree && !skipSetupOnce {
            let own = project.setupCommands ?? ""
            let global = UserDefaults.standard.string(forKey: "worktreeSetupCommands") ?? ""
            setup = [global, own].filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }.joined(separator: "\n")
            record(.worktree, session: session, project: project, detail: "Worktree \(session.branch ?? "") created")
        }
        skipSetupOnce = false
        terminals.start(session, project: project, directory: directory, resume: resume, setup: setup) { [weak self] sessionID in
            self?.updateSessionID(sessionID, session: session.id, project: project.id)
        }
        record(resume ? .resumed : .started, session: session, project: project)
        openSession(session.id, in: project.id)
        trackGit()
    }

    func reopenClosedTab() {
        guard let id = terminals.popClosedTab(), let project = project(ofSession: id) else { return }
        openSession(id, in: project.id)
    }

    func updateSessionID(_ providerID: String, session id: UUID, project projectID: UUID) {
        var next = workspace
        guard let pi = next.projects.firstIndex(where: { $0.id == projectID }),
              let si = next.projects[pi].sessions?.firstIndex(where: { $0.id == id }) else { return }
        next.projects[pi].sessions?[si].sessionID = providerID
        commit(next)
    }

    func updateSession(_ session: LinkedSession, project projectID: UUID) {
        var next = workspace
        guard let pi = next.projects.firstIndex(where: { $0.id == projectID }),
              let si = next.projects[pi].sessions?.firstIndex(where: { $0.id == session.id }) else { return }
        next.projects[pi].sessions?[si] = session
        commit(next)
    }

    func reviewBrief(for task: WorkTask, spec: Specification) -> String {
        guard let project else { return spec.handoff(for: task) }
        let children = workspace.children(of: project.id).map { "- \($0.path)" }.joined(separator: "\n")
        return """
        ## Workspace context
        Working directory: \(project.path)
        Scope: \(project.isGroup ? "Entire project group" : "Individual project")
        \(children.isEmpty ? "" : "Related projects:\n" + children)

        \(spec.handoff(for: task))
        """
    }

    func addSpec(title: String) {
        guard let index = workspace.projects.firstIndex(where: { $0.id == workspace.selectedProjectID }) else { return }
        let spec = Specification(title: title.trimmingCharacters(in: .whitespacesAndNewlines))
        guard !spec.title.isEmpty else { return }
        var next = workspace; next.projects[index].specs.append(spec); next.selectedSpecID = spec.id
        commit(next)
    }

    func updateSpec(_ value: Specification, requirementsChanged: Bool = false) {
        guard let pi = workspace.projects.firstIndex(where: { $0.id == workspace.selectedProjectID }),
              let si = workspace.projects[pi].specs.firstIndex(where: { $0.id == value.id }) else { return }
        var value = value
        if requirementsChanged {
            value.revise()
        }
        value.updatedAt = Date()
        var next = workspace; next.projects[pi].specs[si] = value; commit(next)
    }

    func updateTask(_ value: WorkTask) {
        guard var spec, let index = spec.tasks.firstIndex(where: { $0.id == value.id }) else { return }
        spec.tasks[index] = value; updateSpec(spec)
    }

    func copy(_ value: String) {
        NSPasteboard.general.clearContents(); NSPasteboard.general.setString(value, forType: .string)
        notice = "Copied to clipboard"
    }

    func exportSpec() {
        guard let project, let spec else { return }
        do {
            let root = URL(fileURLWithPath: project.path)
            let panel = NSSavePanel()
            panel.title = "Export specification as Markdown"
            panel.directoryURL = root
            panel.nameFieldStringValue = "spec-\(spec.id.uuidString.prefix(8).lowercased()).md"
            guard panel.runModal() == .OK, let destination = panel.url else { return }
            try spec.markdown.write(to: destination, atomically: true, encoding: .utf8)
            notice = "Exported \(destination.lastPathComponent)"
        } catch { self.error = "Export failed: \(error.localizedDescription)" }
    }

    func reconnectProject() {
        guard let project else { return }
        guard !workspace.projects.contains(where: {
            ($0.id == project.id || $0.path.hasPrefix(project.path + "/")) && terminals.hasRunningSessions(in: $0.id)
        }) else { error = "Stop sessions in this workspace before reconnecting its folder."; return }
        let panel = NSOpenPanel(); panel.canChooseDirectories = true; panel.canChooseFiles = false
        panel.message = "Choose the folder for \(project.name). This also supports a moved project."
        guard panel.runModal() == .OK, let url = panel.url else { return }
        do {
            var next = workspace
            guard let index = next.projects.firstIndex(where: { $0.id == project.id }) else { return }
            guard !next.projects.contains(where: { $0.id != project.id && $0.path == url.path }) else {
                error = "That folder is already connected to another project."; return
            }
            next.projects[index].bookmark = try url.bookmarkData(options: [], includingResourceValuesForKeys: nil, relativeTo: nil)
            for i in next.projects.indices where next.projects[i].path.hasPrefix(project.path + "/") {
                let suffix = next.projects[i].path.dropFirst(project.path.count)
                next.projects[i].path = url.path + suffix
                next.projects[i].bookmark = nil
            }
            next.projects[index].path = url.path
            commit(next)
        } catch { self.error = error.localizedDescription }
    }
}

struct ActivityEvent: Codable, Identifiable {
    enum Kind: String, Codable { case started, resumed, waiting, done, slept, worktree }
    var id = UUID()
    var date = Date()
    var kind: Kind
    var sessionID: UUID
    var sessionTitle: String
    var projectName: String
    var detail: String = ""

    static var url: URL { FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/activity.json") }
    static func load() -> [ActivityEvent] { (try? Data(contentsOf: url)).flatMap { try? JSONDecoder().decode([ActivityEvent].self, from: $0) } ?? [] }
    static func save(_ events: [ActivityEvent]) { try? JSONEncoder().encode(events).write(to: url, options: [.atomic]) }
}
