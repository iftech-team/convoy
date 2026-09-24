import SwiftUI

struct SessionWorkspace: View {
    @EnvironmentObject var store: Store
    let project: Project
    @Binding var area: String
    var reviewsOnly: Bool { area == "Reviews" }
    @Binding var newSession: Bool

    var body: some View {
        SessionPanel(project: project, area: $area, newSession: $newSession, manager: store.terminals)
    }
}

struct SessionPanel: View {
    @EnvironmentObject var store: Store
    let project: Project
    @Binding var area: String
    var reviewsOnly: Bool { area == "Reviews" }
    @Binding var newSession: Bool
    @ObservedObject var manager: TerminalManager
    @State private var search = ""
    @State private var showArchived = false
    @State private var reviewSource: LinkedSession?
    @State private var editing: LinkedSession?
    @State private var feedback: LinkedSession?
    @State private var stopID: UUID?

    var sessions: [LinkedSession] {
        project.linkedSessions.filter {
            (!reviewsOnly || $0.reviewOf != nil) && ($0.archived != true || showArchived) &&
            (search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) || $0.notes.localizedCaseInsensitiveContains(search))
        }.sorted { $0.createdAt > $1.createdAt }
    }
    var selected: LinkedSession? {
        guard let id = store.workspace.selectedSessionID, store.tabOrder.contains(id) else { return nil }
        return project.linkedSessions.first { $0.id == id && (!reviewsOnly || $0.reviewOf != nil) }
    }

    var body: some View {
        Group {
            if let session = selected {
                VStack(alignment: .leading, spacing: 0) {
                    HStack(spacing: 12) {
                        Menu {
                            Picker("Workspace section", selection: $area) {
                                Text("Sessions").tag("Sessions")
                                Text("Reviews").tag("Reviews")
                                Text("Specs").tag("Specs")
                                Text("Tasks").tag("Tasks")
                                Text("Docs").tag("Docs")
                            }
                            Divider()
                            Text(project.path)
                        } label: {
                            Text(project.name).lineLimit(1)
                        }.menuStyle(.borderlessButton).fixedSize().help(project.path)
                        Text("/").foregroundStyle(.tertiary)
                        Menu {
                            ForEach(project.linkedSessions.filter { $0.archived != true }) { item in
                                Button {
                                    area = "Sessions"
                                    search = ""
                                    store.selectSession(item.id)
                                } label: {
                                    Label(item.title, systemImage: item.id == session.id ? "checkmark" : "terminal")
                                }
                            }
                            Divider()
                            Button("New session…") { newSession = true }
                        } label: {
                            Text(session.title).fontWeight(.semibold).lineLimit(1)
                        }.menuStyle(.borderlessButton).frame(maxWidth: 260)
                        Spacer(minLength: 8)
                        if let git = store.git.info[store.directory(for: session, in: project)] {
                            Button { store.toggleGitPanel() } label: {
                                Label(git.branch + (git.summary.isEmpty ? "" : " · " + git.summary), systemImage: "arrow.triangle.branch")
                                    .font(.caption).foregroundStyle(store.showGitPanel || session.branch != nil ? AppTheme.accent : .secondary).lineLimit(1)
                            }.buttonStyle(.plain).help("Files & Changes (\(store.keys.display("git.panel"))) · \(session.workingDirectory ?? project.path)")
                        }
                        Text(session.agent.rawValue).font(.caption).foregroundStyle(.secondary)
                        let running = manager.handles[session.id]?.running == true
                        HStack(spacing: 5) {
                            AgentStateGlyph(state: store.agentState(session), running: running, size: 6).frame(width: 10)
                            Text(AgentStateGlyph(state: store.agentState(session), running: running).label).font(.caption).foregroundStyle(.secondary)
                        }
                        if manager.handles[session.id]?.running != true {
                            if !SessionCommand.missingConversation(in: manager.snapshot(for: session)) {
                                Button("Resume") { store.startSession(session, in: project, resume: true) }
                            }
                        }
                        Menu {
                            let commands = store.quickCommands(for: project)
                            if commands.isEmpty { Text("No quick commands yet") }
                            ForEach(commands) { command in
                                Button { store.send(command, to: session.id) } label: { Label(command.title, systemImage: command.submit ? "return" : "text.cursor") }
                            }
                            Divider()
                            Button("Manage quick commands…") { store.showSettings = true; store.settingsSection = "Quick Commands" }
                        } label: { Image(systemName: "bolt").frame(width: 22, height: 22) }
                            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                            .help("Quick commands (\(store.keys.display("session.quick")))").accessibilityLabel("Quick commands")
                        Button { newSession = true } label: { Image(systemName: "plus") }
                            .buttonStyle(.plain).help("New session").accessibilityLabel("New session")
                        Menu {
                            sessionActions(session)
                            if session.reviewOf != nil {
                                Button("Send feedback to builder…") { feedback = session }
                            }
                            if manager.handles[session.id]?.running == true {
                                Divider()
                                Button("Stop session…", role: .destructive) { stopID = session.id }
                            }
                        } label: { Image(systemName: "ellipsis").frame(width: 24, height: 24) }
                            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                            .help("Session actions").accessibilityLabel("Session actions")
                    }.padding(.horizontal, 14).frame(height: 40)
                    Divider()
                    if manager.handles[session.id]?.running != true && SessionCommand.missingConversation(in: manager.snapshot(for: session)) {
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 3) {
                                Text("Claude couldn’t find this conversation").font(.callout.bold())
                                Text("It may have closed before the first message. Start fresh here; this session record will be kept.")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button("Start fresh") {
                                let fresh = session.freshConversation()
                                store.startSession(fresh, in: project)
                            }.buttonStyle(.borderedProminent)
                        }.padding(12).background(.bar)
                    } else if manager.handles[session.id] == nil {
                        HStack {
                            Label("Saved output · read-only", systemImage: "clock.arrow.circlepath")
                            Text("Resume to reconnect to the agent.").foregroundStyle(.secondary)
                            Spacer()
                        }.font(.caption).padding(10).background(.bar)
                    }
                    if let handle = manager.handles[session.id] {
                        TerminalPane(handle: handle).id(ObjectIdentifier(handle))
                    } else {
                        let snapshot = manager.snapshot(for: session)
                        if snapshot.isEmpty {
                            ContentUnavailableView("Resume your work", systemImage: "terminal", description: Text(session.sessionID.isEmpty ? "Open the agent's resume picker to choose this conversation. You can save its exact ID using Edit." : "This conversation is saved. Resume to reopen the agent in this workspace."))
                        } else {
                            ScrollView {
                                Text(snapshot).font(.system(size: 12, design: .monospaced)).textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading).padding(16)
                            }
                        }
                    }
                }.frame(minWidth: 480, maxWidth: .infinity, maxHeight: .infinity)
            } else if !reviewsOnly {
                HistoryPanel(project: project, onNewSession: { newSession = true })
            } else {
                VStack(spacing: 18) {
                    Image(systemName: reviewsOnly ? "checkmark.bubble" : "terminal")
                        .font(.system(size: 26, weight: .medium)).foregroundStyle(AppTheme.accent)
                        .frame(width: 64, height: 64)
                        .background(AppTheme.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 17))
                    VStack(spacing: 8) {
                        Text(reviewsOnly ? "Review with another agent" : (project.linkedSessions.isEmpty ? "Start your first session" : "Pick a session in the sidebar"))
                            .font(.system(size: 23, weight: .semibold)).foregroundStyle(.primary)
                        Text(reviewsOnly ? "Open a coding session and choose Start review. The other agent will receive an editable brief." : "Run Claude Code or Codex inside Convoy. \(project.isGroup ? "Work across the related projects in this group." : "Keep your conversations together in this workspace.")")
                            .font(.system(size: 13)).foregroundStyle(.secondary)
                            .multilineTextAlignment(.center).lineSpacing(3).frame(maxWidth: 380)
                    }
                    if !reviewsOnly {
                        Button { newSession = true } label: { Label("New session", systemImage: "plus") }
                            .buttonStyle(.borderedProminent).controlSize(.large)
                        Label("\(project.name) · \(project.isGroup ? "Entire group" : "Project folder")", systemImage: "folder")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }.padding(32).frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Color(nsColor: .textBackgroundColor).opacity(0.4))
            }
        }
        .onChange(of: store.editRequest) { _, id in
            guard let id, let session = project.linkedSessions.first(where: { $0.id == id }) else { return }
            store.editRequest = nil; editing = session
        }
        .onChange(of: store.reviewRequest) { _, id in
            guard let id, let session = project.linkedSessions.first(where: { $0.id == id }), session.reviewOf == nil else { return }
            store.reviewRequest = nil; reviewSource = session
        }
        .onChange(of: store.feedbackRequest) { _, id in
            guard let id, let session = project.linkedSessions.first(where: { $0.id == id }), session.reviewOf != nil else { return }
            store.feedbackRequest = nil; feedback = session
        }
        .onChange(of: store.stopRequest) { _, id in
            guard let id, project.linkedSessions.contains(where: { $0.id == id }) else { return }
            store.stopRequest = nil; stopID = id
        }
        .sheet(item: $reviewSource) { source in
            NewSessionSheet(project: project, source: source, output: manager.snapshot(for: source))
        }
        .sheet(item: $editing) { session in SessionDetails(session: session, project: project) }
        .sheet(item: $feedback) { session in FeedbackSheet(session: session, project: project, manager: manager) }
        .alert("Stop this agent?", isPresented: Binding(get: { stopID != nil }, set: { if !$0 { stopID = nil } })) {
            Button("Cancel", role: .cancel) { stopID = nil }
            Button("Stop", role: .destructive) {
                if let stopID { manager.handles[stopID]?.stop() }
                stopID = nil
            }
        } message: { Text("This interrupts current work. Saved conversation history remains with the agent. Unsaved work in progress may be incomplete.") }
    }

    @ViewBuilder private func sessionActions(_ session: LinkedSession) -> some View {
        Button { editing = session } label: { Label("Edit name & notes…", systemImage: "pencil") }
        Button { store.togglePin(session, in: project) } label: { Label(session.pinned == true ? "Unpin" : "Pin to top", systemImage: session.pinned == true ? "pin.slash" : "pin") }
        if manager.handles[session.id]?.running == true {
            Button { store.sleep(session, in: project) } label: { Label("Sleep (stop, resume on open)", systemImage: "moon.zzz") }
        }
        if session.reviewOf == nil {
            Button { reviewSource = session } label: { Label("Start review…", systemImage: "checkmark.bubble") }
        }
        Divider()
        Button {
            var value = session; value.archived = session.archived != true
            store.updateSession(value, project: project.id)
        } label: { Label(session.archived == true ? "Unarchive" : "Archive", systemImage: "archivebox") }
            .disabled(manager.handles[session.id]?.running == true)
    }
}

struct TerminalPane: View {
    @EnvironmentObject var store: Store
    @ObservedObject var handle: TerminalHandle
    var primary = true
    @State private var showSearch = false
    var body: some View {
        VStack(spacing: 0) {
            if showSearch { TerminalSearchBar(handle: handle, shown: $showSearch) }
            if !handle.running {
                Text("Process exited\(handle.exitCode.map { " (\($0))" } ?? ""). Output is kept below.")
                    .font(.caption).padding(8).frame(maxWidth: .infinity, alignment: .leading)
            }
            if let error = handle.snapshotError {
                Text("Could not save terminal snapshot: \(error)").font(.caption).foregroundStyle(.orange)
            }
            EmbeddedTerminal(handle: handle)
        }
        .onChange(of: store.findRequest) { _, _ in if primary { showSearch = true } }
    }
}

struct NewSessionSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State private var project: Project
    let source: LinkedSession?
    /// Header / ⌘N: the sheet opens on the current project but any project can be chosen.
    let pickProject: Bool
    @State private var agent: Agent
    @State private var title: String
    @State private var prompt: String

    init(project: Project, source: LinkedSession? = nil, output: String = "", pickProject: Bool = false) {
        _project = State(initialValue: project); self.source = source; self.pickProject = pickProject
        let preferred = project.defaultAgent ?? (Agent(rawValue: UserDefaults.standard.string(forKey: "defaultAgent") ?? "") ?? .claude)
        _agent = State(initialValue: source?.agent.other ?? preferred)
        _title = State(initialValue: source.map { "Review: \($0.title)" } ?? "")
        _prompt = State(initialValue: source.map { ReviewTemplate.render(project: project, source: $0, output: output, spec: nil) } ?? "")
    }

    @AppStorage("worktreeByDefault") private var worktreeByDefault = false
    @AppStorage("branchPrefix") private var branchPrefix = ""
    @State private var useWorktree = false
    @State private var branch = ""
    @State private var branchEdited = false
    @State private var base = ""
    @State private var refs: [String] = []

    private var isGitRepo: Bool { FileManager.default.fileExists(atPath: project.path + "/.git") }

    private func switchProject(to next: Project) {
        guard next.id != project.id else { return }
        project = next
        refs = []; base = next.baseRef ?? ""; branchEdited = false
        branch = GitWorktree.slug(title, prefix: next.branchPrefix ?? branchPrefix)
        if agent != .codex || next.defaultAgent != nil { agent = next.defaultAgent ?? agent }
        useWorktree = worktreeByDefault
        loadRefs()
    }

    private var projectChip: some View {
        HStack(spacing: 6) {
            ProjectIconView(project: project, size: 14)
            Text(project.name).font(.system(size: 12, weight: .semibold)).foregroundStyle(project.tint)
            if pickProject { Image(systemName: "chevron.up.chevron.down").font(.system(size: 9, weight: .semibold)).foregroundStyle(.secondary) }
        }.padding(.horizontal, 8).padding(.vertical, 4).background(project.tint.opacity(0.12), in: Capsule())
    }

    @ViewBuilder private var projectPicker: some View {
        if pickProject {
            Menu {
                ForEach(store.workspace.rootProjects) { root in
                    Button { switchProject(to: root) } label: { Label(root.name, systemImage: root.id == project.id ? "checkmark" : (root.isGroup ? "folder" : "shippingbox")) }
                    ForEach(store.workspace.children(of: root.id)) { child in
                        Button { switchProject(to: child) } label: { Label("    " + child.name, systemImage: child.id == project.id ? "checkmark" : "shippingbox") }
                    }
                }
            } label: { projectChip }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().help("Choose the project for this session")
        } else {
            projectChip
        }
    }

    private func loadRefs() {
        let path = project.path
        Task.detached(priority: .utility) {
            var list: [String] = []
            if let (s, out) = GitInfoService.run(["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/heads"], in: path), s == 0 { list += out.split(separator: "\n").map(String.init) }
            if let (s, out) = GitInfoService.run(["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)", "refs/remotes/origin"], in: path), s == 0 { list += out.split(separator: "\n").map(String.init).filter { $0 != "origin/HEAD" } }
            if let (s, out) = GitInfoService.run(["for-each-ref", "--sort=-creatordate", "--format=tag:%(refname:short)", "refs/tags"], in: path), s == 0 { list += out.split(separator: "\n").map(String.init) }
            await MainActor.run { refs = list }
        }
    }

    @EnvironmentObject private var envStore: Store
    @AppStorage("yoloClaude") private var yoloClaude = false
    @AppStorage("yoloCodex") private var yoloCodex = false
    @AppStorage("defaultAgent") private var defaultAgent = "Claude Code"

    private func agentCard(_ candidate: Agent) -> some View {
        let selected = agent == candidate
        let account = store.accounts.active(for: candidate)?.label
        let yolo = candidate == .claude ? yoloClaude : yoloCodex
        return Button { agent = candidate } label: {
            HStack(spacing: 12) {
                AgentIcon(agent: candidate, size: 22).frame(width: 40, height: 40)
                    .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 9))
                    .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(AppTheme.stroke))
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 6) {
                        Text(candidate.rawValue).font(.system(size: 13, weight: .semibold))
                        if defaultAgent == candidate.rawValue { Text("default").font(.system(size: 9, weight: .medium)).foregroundStyle(.secondary).padding(.horizontal, 5).padding(.vertical, 1).background(Color.primary.opacity(0.06), in: Capsule()) }
                    }
                    Text([candidate == .claude ? "claude" : "codex", account.map { "account: " + $0 } ?? "system login", yolo ? "yolo" : nil].compactMap { $0 }.joined(separator: " · "))
                        .font(.system(size: 10.5, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer()
                if selected { Image(systemName: "checkmark.circle.fill").foregroundStyle(AppTheme.accent) }
            }.padding(12).frame(maxWidth: .infinity).contentShape(Rectangle())
                .background(selected ? AppTheme.accent.opacity(0.14) : Color.primary.opacity(0.03), in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(selected ? AppTheme.accent.opacity(0.7) : AppTheme.stroke, lineWidth: selected ? 1.5 : 1))
        }.buttonStyle(.plain)
    }

    @State private var advanced = false
    @State private var note = ""
    @State private var setupPolicy = "run"
    @State private var keepOpen = false

    private func label(_ text: String) -> some View {
        Text(text.uppercased()).font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
    }
    private var effectiveBase: String { base.isEmpty ? (project.baseRef ?? refs.first { !$0.hasPrefix("origin/") && !$0.hasPrefix("tag:") } ?? "current branch") : base }
    private var setupPreview: String {
        [UserDefaults.standard.string(forKey: "worktreeSetupCommands") ?? "", project.setupCommands ?? ""].filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }.joined(separator: "\n")
    }

    /// Editable ref with a picker inside, styled like the Branch field.
    private var baseRefPicker: some View {
        let locals = refs.filter { !$0.hasPrefix("origin/") && !$0.hasPrefix("tag:") }
        let remotes = refs.filter { $0.hasPrefix("origin/") }
        let tags = refs.filter { $0.hasPrefix("tag:") }.map { String($0.dropFirst(4)) }
        return HStack(spacing: 6) {
            Image(systemName: "arrow.triangle.branch").foregroundStyle(.tertiary).font(.system(size: 10))
            TextField(project.baseRef ?? "current branch", text: $base).textFieldStyle(.plain).font(.system(size: 12, design: .monospaced))
            Menu {
                Button("Current branch" + (project.baseRef.map { " (\($0))" } ?? "")) { base = "" }
                if !locals.isEmpty { Section("Branches") { ForEach(locals, id: \.self) { r in Button(r) { base = r } } } }
                if !remotes.isEmpty { Section("Remote") { ForEach(remotes.prefix(60), id: \.self) { r in Button(r) { base = r } } } }
                if !tags.isEmpty { Section("Tags") { ForEach(tags.prefix(40), id: \.self) { r in Button(r) { base = r } } } }
                if refs.isEmpty { Text("Loading refs…") }
            } label: {
                Image(systemName: "chevron.up.chevron.down").font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary).frame(width: 18, height: 18)
            }.menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().help("Pick a branch, remote branch or tag")
        }.padding(.horizontal, 10).frame(height: 30)
            .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
            .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(AppTheme.stroke))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 10) {
                Text(source == nil ? "New session" : "Start a review").font(.system(size: 18, weight: .bold))
                Spacer()
                projectPicker
            }
            Text(project.path).font(.system(size: 10.5, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled).lineLimit(1).truncationMode(.middle)

            VStack(alignment: .leading, spacing: 6) {
                label("Name (optional)")
                TextField("Leave empty to name it after the first message", text: $title).textFieldStyle(.roundedBorder).font(.system(size: 13))
                    .onChange(of: title) { _, value in if !branchEdited { branch = GitWorktree.slug(value, prefix: project.branchPrefix ?? branchPrefix) } }
            }

            if source == nil && isGitRepo && !project.isGroup {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .center, spacing: 12) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Isolated git worktree").font(.system(size: 13, weight: .medium))
                            Text("Own checkout and branch, so parallel sessions never touch each other's files.").font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer(minLength: 12)
                        Toggle("", isOn: $useWorktree).labelsHidden().toggleStyle(.switch)
                    }
                    if useWorktree {
                        HStack(alignment: .top, spacing: 12) {
                            VStack(alignment: .leading, spacing: 6) { label("Create from"); baseRefPicker }
                            VStack(alignment: .leading, spacing: 6) {
                                label("Branch")
                                HStack(spacing: 6) {
                                    Image(systemName: "arrow.turn.down.right").foregroundStyle(.tertiary).font(.system(size: 10))
                                    TextField("branch-name", text: $branch).textFieldStyle(.plain).font(.system(size: 12, design: .monospaced))
                                        .onChange(of: branch) { _, _ in branchEdited = true }
                                    if branchEdited { Button { branchEdited = false; branch = GitWorktree.slug(title, prefix: project.branchPrefix ?? branchPrefix) } label: { Image(systemName: "arrow.uturn.backward").font(.system(size: 10)) }.buttonStyle(.plain).foregroundStyle(.secondary).help("Back to the generated name") }
                                }.padding(.horizontal, 10).frame(height: 30)
                                    .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
                                    .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(AppTheme.stroke))
                            }
                        }
                        Text("Worktree: Application Support/Convoy/worktrees/\(project.name)/\(branch.isEmpty ? "…" : branch.replacingOccurrences(of: "/", with: "--"))").font(.caption2).foregroundStyle(.tertiary).lineLimit(1).truncationMode(.middle)
                    }
                }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8))
                    .onAppear { useWorktree = worktreeByDefault; base = project.baseRef ?? ""; loadRefs() }
            }

            VStack(alignment: .leading, spacing: 8) {
                label("Agent")
                HStack(spacing: 10) { agentCard(.claude); agentCard(.codex) }
                AgentAvailabilityNote(agent: agent)
            }

            VStack(alignment: .leading, spacing: 6) {
                label(source == nil ? "First message (optional)" : "Review brief — edit before sending")
                TextEditor(text: $prompt).font(.system(size: 12.5)).frame(height: source == nil ? (advanced ? 80 : 110) : 200)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke))
            }

            DisclosureGroup(isExpanded: $advanced) {
                VStack(alignment: .leading, spacing: 12) {
                    VStack(alignment: .leading, spacing: 6) {
                        label("Note")
                        TextField("Where you left off, links, context for later (saved in the session)", text: $note).textFieldStyle(.roundedBorder).font(.system(size: 12))
                    }
                    if useWorktree && source == nil {
                        VStack(alignment: .leading, spacing: 6) {
                            HStack {
                                label("Worktree setup commands")
                                Spacer()
                                Picker("", selection: $setupPolicy) { Text("Run").tag("run"); Text("Skip").tag("skip") }.pickerStyle(.segmented).labelsHidden().controlSize(.small).frame(width: 110)
                            }
                            if setupPreview.isEmpty {
                                Text("None configured. Add them in Project settings → Git & worktrees or Settings → Git.").font(.caption).foregroundStyle(.tertiary)
                            } else {
                                Text(setupPreview).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).lineLimit(4)
                                    .padding(8).frame(maxWidth: .infinity, alignment: .leading).background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6))
                            }
                        }
                    }
                    Toggle("Keep this dialog open to create more", isOn: $keepOpen).font(.system(size: 12))
                }.padding(.top, 8)
            } label: {
                Text("Advanced").font(.system(size: 12, weight: .medium)).foregroundStyle(.secondary)
            }

            if source != nil {
                Text("Let the builder finish its changes before reviewing. The reviewer is asked to inspect without editing; its own permission settings still apply.").font(.caption).foregroundStyle(.secondary)
            }
            HStack {
                Text((agent == .claude ? yoloClaude : yoloCodex) ? "Starts without permission prompts (Yolo is on in Settings → Agents)." : "Uses your installed agent, login and approval settings.").font(.caption).foregroundStyle(.tertiary)
                Spacer(); Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button {
                    let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
                    let finalTitle = trimmedTitle.isEmpty ? SessionNaming.autoTitle(agent: agent, prompt: prompt) : trimmedTitle
                    var session = LinkedSession(agent: agent, sessionID: agent == .claude ? UUID().uuidString.lowercased() : "", title: finalTitle, notes: note, initialPrompt: prompt, reviewOf: source?.id)
                    if let source, let builder = project.linkedSessions.first(where: { $0.id == source.id }) { session.workingDirectory = builder.workingDirectory; session.branch = builder.branch; session.baseRef = builder.baseRef }
                    var branchName = branch.trimmingCharacters(in: .whitespaces)
                    if useWorktree && source == nil && branchName.isEmpty { branchName = GitWorktree.slug(finalTitle, prefix: project.branchPrefix ?? branchPrefix) }
                    let wantWorktree = source == nil && useWorktree && !branchName.isEmpty
                    store.skipSetupOnce = setupPolicy == "skip"
                    if pickProject, store.workspace.selectedProjectID != project.id { store.selectProject(project.id) }
                    store.startSession(session, in: project, worktreeBranch: wantWorktree ? branchName : nil, base: base.trimmingCharacters(in: .whitespaces))
                    if store.error == nil {
                        if keepOpen { title = ""; prompt = ""; note = ""; branch = ""; branchEdited = false } else { dismiss() }
                    }
                } label: {
                    Text(source == nil ? "Start \(agent.rawValue)" : "Launch reviewer")
                }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)

            }
        }.padding(24).frame(width: 640)
    }
}

struct SessionDetails: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State var session: LinkedSession
    let project: Project
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Session details").font(.title2.bold())
            TextField("Name", text: $session.title).textFieldStyle(.roundedBorder)
            Text("Provider session ID").font(.headline)
            TextField("Optional — used for exact resume", text: $session.sessionID).textFieldStyle(.roundedBorder)
            Text("Claude IDs are assigned at launch. Codex IDs are captured when it prints its resume command on exit; otherwise its resume picker is available.").font(.caption).foregroundStyle(.secondary)
            Text("Notes / where you left off").font(.headline)
            TextEditor(text: $session.notes).frame(height: 140).border(.quaternary)
            HStack {
                Spacer(); Button("Cancel") { dismiss() }
                Button("Save") { store.updateSession(session, project: project.id); if store.error == nil { dismiss() } }
                    .buttonStyle(.borderedProminent).disabled(session.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }.padding(24).frame(width: 520)
    }
}

struct FeedbackSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    let session: LinkedSession
    let project: Project
    @ObservedObject var manager: TerminalManager
    @State private var feedback = ""
    var builder: TerminalHandle? { session.reviewOf.flatMap { manager.handles[$0] } }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Send feedback to builder").font(.title2.bold())
            Text("Review and trim the captured output. It will be inserted into the builder's terminal; press Enter there when you're ready to send.").foregroundStyle(.secondary)
            TextEditor(text: $feedback).frame(height: 300).border(.quaternary)
            if builder?.running != true { Text("Resume the original builder session first.").foregroundStyle(.orange) }
            HStack {
                Spacer(); Button("Cancel") { dismiss() }
                Button("Insert in builder") {
                    let safe = feedback.replacingOccurrences(of: "\u{1b}", with: "")
                    guard let builder, builder.running else { return }
                    builder.view.send(txt: "\u{1b}[200~" + safe + "\u{1b}[201~")
                    dismiss()
                    store.openSession(builder.sessionID, in: builder.projectID)
                }.buttonStyle(.borderedProminent).disabled(builder?.running != true || feedback.isEmpty)
            }
        }.padding(24).frame(width: 650)
            .onAppear { feedback = "Please address the review findings below, verify the changes, and report what you fixed.\n\n" + String(manager.snapshot(for: session).suffix(18000)) }
    }
}


