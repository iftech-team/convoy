import SwiftUI

// MARK: - Review brief template

enum ReviewTemplate {
    static let `default` = """
    Review the current code changes for: {{title}}
    Workspace root: {{path}}
    {{scope}}

    Original task:
    {{task}}

    Builder notes:
    {{notes}}
    {{spec}}
    Recent terminal context (may include incomplete output; verify claims against code):
    {{output}}

    Inspect the actual diffs. Report actionable findings, severity, file locations, and the revisions reviewed. Do not edit code. Distinguish verified issues from uncertainty and state what you could not verify.
    """

    static func render(project: Project, source: LinkedSession, output: String, spec: String?) -> String {
        let template = (project.reviewTemplate?.isEmpty == false ? project.reviewTemplate : nil)
            ?? (UserDefaults.standard.string(forKey: "reviewTemplate").flatMap { $0.isEmpty ? nil : $0 })
            ?? Self.default
        let values: [String: String] = [
            "title": source.title,
            "path": source.workingDirectory ?? project.path,
            "scope": project.isGroup ? "This is a group of related projects. Inspect relevant repositories beneath this root." : "Review this project.",
            "task": source.initialPrompt ?? source.notes,
            "notes": source.notes,
            "spec": spec.map { "\nSpecification the work must satisfy:\n\($0)\n" } ?? "",
            "output": String(output.suffix(16000)),
            "branch": source.branch ?? "",
        ]
        return values.reduce(template) { $0.replacingOccurrences(of: "{{\($1.key)}}", with: $1.value) }
    }
}

// MARK: - Task prompts

enum TaskPrompt {
    static func build(task: AgentTask, project: Project, docExists: Bool, baseBranch: String) -> String {
        var lines: [String] = []
        lines.append("Task: \(task.title)")
        if !task.details.trimmingCharacters(in: .whitespaces).isEmpty { lines.append("\n\(task.details)") }
        lines.append("")
        if docExists { lines.append("Before changing anything, read .specdesk/PROJECT.md for project context and conventions.") }
        if let spec = task.spec { lines.append("The work must satisfy the specification in \(spec). Read it first and treat its acceptance criteria as the definition of done.") }
        lines.append("Work in this checkout only. Verify your changes (build, tests, or a manual check) before finishing.")
        switch task.mode {
        case "pr":
            lines.append("When done: commit with a clear message, run `git push -u origin HEAD`, then open a pull request against \(baseBranch) with `gh pr create --fill` and print the PR URL on its own line.")
        case "push":
            lines.append("When done: commit with a clear message and run `git push origin HEAD:\(baseBranch)`. Report the commit hash.")
        default:
            lines.append("When done: commit with a clear message. Do not push.")
        }
        lines.append("Finish with a short summary: what changed, how you verified it, and anything left open.")
        return lines.joined(separator: "\n")
    }
}

// MARK: - Store: tasks

extension Store {
    var tasks: [AgentTask] { workspace.tasks ?? [] }
    func tasks(for project: Project) -> [AgentTask] { tasks.filter { $0.projectID == project.id } }

    func saveTask(_ task: AgentTask) {
        var next = workspace
        var list = next.tasks ?? []
        if let i = list.firstIndex(where: { $0.id == task.id }) { list[i] = task } else { list.append(task) }
        next.tasks = list; commit(next)
    }
    func deleteTask(_ id: UUID) { var next = workspace; next.tasks = (next.tasks ?? []).filter { $0.id != id }; commit(next) }

    func specText(_ relative: String?, in project: Project) -> String? {
        guard let relative else { return nil }
        return try? String(contentsOfFile: project.path + "/" + relative, encoding: .utf8)
    }

    /// Starts the task in its own worktree (or the project folder when mode is "none" and worktrees are off).
    func runTask(_ task: AgentTask) {
        guard let project = workspace.projects.first(where: { $0.id == task.projectID }) else { return }
        guard terminals.runningCount < 12 else { error = "Too many running sessions."; return }
        let docExists = FileManager.default.fileExists(atPath: project.path + "/.specdesk/PROJECT.md")
        let base = project.baseRef?.isEmpty == false ? project.baseRef! : GitWorktree.defaultBase(project.path)
        let prompt = TaskPrompt.build(task: task, project: project, docExists: docExists, baseBranch: base)
        let session = LinkedSession(agent: task.agent, sessionID: task.agent == .claude ? UUID().uuidString.lowercased() : "", title: task.title, initialPrompt: prompt)
        let isRepo = FileManager.default.fileExists(atPath: project.path + "/.git")
        let branch = isRepo && task.mode != "none" ? GitWorktree.slug(task.title, prefix: project.branchPrefix ?? UserDefaults.standard.string(forKey: "branchPrefix") ?? "") : nil
        startSession(session, in: project, worktreeBranch: branch, base: base)
        guard error == nil, let started = self.session(session.id) else { return }
        var updated = task
        updated.status = .running; updated.startedAt = Date(); updated.sessionID = started.id; updated.branch = started.branch
        saveTask(updated)
        terminals.handles[started.id]?.onPullRequest = { [weak self] url in
            Task { @MainActor in
                guard let self, var t = self.tasks.first(where: { $0.id == updated.id }) else { return }
                t.prURL = url; if t.status == .running || t.status == .review { t.status = .pr }
                self.saveTask(t)
                self.record(.done, session: started, project: project, detail: "Pull request: \(url)")
            }
        }
    }

    /// Called from agent status transitions: finished agent → task needs review; then optionally auto-review and queue next.
    func taskAgentFinished(sessionID: UUID) {
        guard var task = tasks.first(where: { $0.sessionID == sessionID && $0.status == .running }),
              let project = workspace.projects.first(where: { $0.id == task.projectID }) else { return }
        task.status = task.prURL != nil ? .pr : .review
        task.finishedAt = Date()
        saveTask(task)
        if task.autoReview, task.reviewSessionID == nil, let source = session(sessionID) {
            let brief = ReviewTemplate.render(project: project, source: source, output: terminals.snapshot(for: source), spec: specText(task.spec, in: project))
            var reviewer = LinkedSession(agent: source.agent.other, sessionID: source.agent.other == .claude ? UUID().uuidString.lowercased() : "", title: "Review: \(source.title)", initialPrompt: brief, reviewOf: source.id)
            reviewer.workingDirectory = source.workingDirectory; reviewer.branch = source.branch
            startSession(reviewer, in: project)
            task.reviewSessionID = reviewer.id; saveTask(task)
        }
        if project.autoRunTasks == true, let next = tasks(for: project).first(where: { $0.status == .queued }) { runTask(next) }
    }

    func taskCounts(for project: Project? = nil) -> (queued: Int, running: Int, review: Int) {
        let list = project.map { tasks(for: $0) } ?? tasks
        return (list.filter { $0.status == .queued }.count, list.filter { $0.status == .running }.count, list.filter { $0.status == .review || $0.status == .pr }.count)
    }
}

// MARK: - Views

extension AgentTask.Status {
    var label: String {
        switch self { case .queued: "Queued"; case .running: "Running"; case .review: "Needs review"; case .pr: "PR open"; case .done: "Done"; case .failed: "Failed" }
    }
    var tint: Color {
        switch self { case .queued: .secondary; case .running: AppTheme.accent; case .review: .orange; case .pr: .purple; case .done: .green; case .failed: .red }
    }
    /// Linear-style status glyph.
    var symbol: String {
        switch self { case .queued: "circle.dotted"; case .running: "circle.lefthalf.filled"; case .review: "circle.righthalf.filled"; case .pr: "arrow.triangle.pull"; case .done: "checkmark.circle.fill"; case .failed: "xmark.circle" }
    }
    /// Display order for sections and columns.
    static let display: [AgentTask.Status] = [.queued, .running, .review, .pr, .done, .failed]
}

struct TasksPanel: View {
    @EnvironmentObject var store: Store
    let project: Project?           // nil = all projects
    @AppStorage("tasksLayout") private var layout = "list"
    @State private var editing: AgentTask?
    @State private var collapsed: Set<AgentTask.Status> = []
    @State private var hideDone = false

    private var all: [AgentTask] {
        (project.map { store.tasks(for: $0) } ?? store.tasks).sorted { $0.createdAt > $1.createdAt }
    }
    private func tasks(in status: AgentTask.Status) -> [AgentTask] { all.filter { $0.status == status } }
    private var visibleStatuses: [AgentTask.Status] { AgentTask.Status.display.filter { !(hideDone && $0 == .done) } }

    private func newTask(in status: AgentTask.Status = .queued) {
        guard let p = project ?? store.project else { return }
        var task = AgentTask(projectID: p.id, title: "", mode: p.taskMode ?? "pr", agent: p.defaultAgent ?? (Agent(rawValue: UserDefaults.standard.string(forKey: "defaultAgent") ?? "") ?? .claude))
        task.status = status == .running ? .queued : status
        editing = task
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Text(project.map { "Tasks · \($0.name)" } ?? "All tasks").font(.system(size: 15, weight: .semibold))
                Text("\(all.count)").font(.system(size: 12, weight: .medium)).foregroundStyle(.tertiary)
                Picker("", selection: $layout) {
                    Label("List", systemImage: "list.bullet").tag("list")
                    Label("Board", systemImage: "rectangle.split.3x1").tag("board")
                }.pickerStyle(.segmented).labelsHidden().frame(width: 150).help("List or kanban board")
                Toggle(isOn: $hideDone) { Text("Hide done").font(.system(size: 11)) }.toggleStyle(.checkbox)
                Spacer()
                if let project {
                    Toggle("Auto-run queue", isOn: Binding(get: { project.autoRunTasks == true }, set: { v in var p = project; p.autoRunTasks = v; store.updateProject(p) }))
                        .toggleStyle(.switch).controlSize(.small).help("Start the next queued task when one finishes")
                }
                if project != nil || store.project != nil {
                    Button { newTask() } label: { Label(project == nil ? "New task in \(store.project?.name ?? "")" : "New task", systemImage: "plus") }.buttonStyle(.borderedProminent)
                }
            }.padding(.horizontal, 20).padding(.vertical, 14)
            Divider()
            if all.isEmpty {
                VStack(spacing: 10) {
                    Image(systemName: "checklist").font(.system(size: 26)).foregroundStyle(.tertiary)
                    Text("No tasks yet").font(.system(size: 13, weight: .semibold)).foregroundStyle(.secondary)
                    Text("Describe the work; the agent takes it in its own worktree, implements, verifies, then opens a PR or pushes.")
                        .font(.caption).foregroundStyle(.tertiary).multilineTextAlignment(.center).frame(maxWidth: 380)
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if layout == "board" {
                board
            } else {
                list
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .sheet(item: $editing) { task in TaskSheet(task: task) }
    }

    // MARK: List

    private var list: some View {
        ScrollView {
            LazyVStack(spacing: 0, pinnedViews: .sectionHeaders) {
                ForEach(visibleStatuses, id: \.self) { status in
                    let items = tasks(in: status)
                    if !items.isEmpty || status == .queued {
                        Section {
                            if !collapsed.contains(status) {
                                ForEach(items) { task in TaskRow(task: task, showProject: project == nil, editing: $editing) }
                                if items.isEmpty { Text("Nothing queued.").font(.caption).foregroundStyle(.tertiary).padding(.leading, 44).padding(.vertical, 8).frame(maxWidth: .infinity, alignment: .leading) }
                            }
                        } header: {
                            sectionHeader(status, count: items.count)
                        }
                        .dropDestination(for: String.self) { ids, _ in move(ids, to: status) }
                    }
                }
            }
        }
    }

    private func sectionHeader(_ status: AgentTask.Status, count: Int) -> some View {
        HStack(spacing: 8) {
            Button { if collapsed.contains(status) { collapsed.remove(status) } else { collapsed.insert(status) } } label: {
                Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).foregroundStyle(.tertiary)
                    .rotationEffect(.degrees(collapsed.contains(status) ? -90 : 0)).frame(width: 14)
            }.buttonStyle(.plain)
            Image(systemName: status.symbol).font(.system(size: 12)).foregroundStyle(status.tint)
            Text(status.label).font(.system(size: 12, weight: .semibold))
            Text("\(count)").font(.system(size: 11)).foregroundStyle(.tertiary)
            Spacer()
            if status == .queued || status == .done {
                Button { newTask(in: status) } label: { Image(systemName: "plus").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).frame(width: 20, height: 20) }
                    .buttonStyle(.plain).help("New task")
            }
        }.padding(.horizontal, 14).frame(height: 32).background(AppTheme.raised)
    }

    // MARK: Board

    private var board: some View {
        ScrollView(.horizontal) {
            HStack(alignment: .top, spacing: 12) {
                ForEach(visibleStatuses, id: \.self) { status in column(status) }
            }.padding(16)
        }
    }

    private func column(_ status: AgentTask.Status) -> some View {
        let items = tasks(in: status)
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: status.symbol).font(.system(size: 12)).foregroundStyle(status.tint)
                Text(status.label).font(.system(size: 12, weight: .semibold))
                Text("\(items.count)").font(.system(size: 11)).foregroundStyle(.tertiary)
                Spacer()
                if status == .queued {
                    Button { newTask() } label: { Image(systemName: "plus").font(.system(size: 11, weight: .semibold)).foregroundStyle(.secondary).frame(width: 20, height: 20) }.buttonStyle(.plain)
                }
            }.padding(.horizontal, 4)
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(items) { task in TaskCardView(task: task, showProject: project == nil, editing: $editing) }
                    if items.isEmpty {
                        Text(status == .running ? "Drop a task here to run it." : "No tasks").font(.caption).foregroundStyle(.tertiary)
                            .frame(maxWidth: .infinity).padding(.vertical, 24)
                            .background(Color.primary.opacity(0.02), in: RoundedRectangle(cornerRadius: 8))
                            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(AppTheme.stroke, style: StrokeStyle(lineWidth: 1, dash: [4])))
                    }
                }.padding(2)
            }
        }
        .frame(width: 272).frame(maxHeight: .infinity, alignment: .top)
        .dropDestination(for: String.self) { ids, _ in move(ids, to: status) }
    }

    // MARK: Moving between statuses

    private func move(_ ids: [String], to status: AgentTask.Status) -> Bool {
        var moved = false
        for raw in ids {
            guard let id = UUID(uuidString: raw), var task = store.tasks.first(where: { $0.id == id }), task.status != status else { continue }
            moved = true
            switch status {
            case .running:
                if task.status == .queued || task.status == .failed { store.runTask(task) } else { task.status = .running; store.saveTask(task) }
            case .done:
                task.status = .done; task.finishedAt = Date(); store.saveTask(task)
            case .queued:
                task.status = .queued; task.sessionID = nil; task.startedAt = nil; task.finishedAt = nil; store.saveTask(task)
            default:
                task.status = status; store.saveTask(task)
            }
        }
        return moved
    }
}

/// Shared pieces for a task in the list or on the board.
@MainActor private struct TaskMeta {
    let task: AgentTask
    let store: Store
    var project: Project? { store.workspace.projects.first { $0.id == task.projectID } }
    var session: LinkedSession? { task.sessionID.flatMap { store.session($0) } }
    var running: Bool { task.sessionID.flatMap { store.terminals.handles[$0]?.running } == true }
    var agentLabel: String? {
        guard let session, running, let state = store.agentState(session) else { return nil }
        return AgentStateGlyph(state: state, running: true).label
    }
    var needsYou: Bool { session.flatMap { store.agentState($0)?.needsYou } == true && running }
    var modeLabel: String { task.mode == "pr" ? "pull request" : (task.mode == "push" ? "push to main" : "commit only") }
    func open() { if let sid = task.sessionID, let project { store.openSession(sid, in: project.id) } }
}

private struct TaskActionsMenu: View {
    @EnvironmentObject var store: Store
    let task: AgentTask
    @Binding var editing: AgentTask?
    var body: some View {
        let meta = TaskMeta(task: task, store: store)
        Group {
            switch task.status {
            case .queued, .failed: Button { store.runTask(task) } label: { Label("Run", systemImage: "play") }
            case .running, .review, .pr, .done:
                if task.sessionID != nil { Button { meta.open() } label: { Label("Open session", systemImage: "terminal") } }
            }
            if let rid = task.reviewSessionID, let project = meta.project { Button { store.openSession(rid, in: project.id) } label: { Label("Open reviewer", systemImage: "checkmark.bubble") } }
            if let url = task.prURL, let link = URL(string: url) { Button { NSWorkspace.shared.open(link) } label: { Label("Open pull request", systemImage: "arrow.triangle.pull") } }
            Divider()
            Button { editing = task } label: { Label("Edit…", systemImage: "pencil") }
            Menu("Set status") {
                ForEach(AgentTask.Status.display, id: \.self) { s in
                    Button { var t = task; t.status = s; if s == .done { t.finishedAt = Date() }; store.saveTask(t) } label: { Label(s.label, systemImage: s.symbol) }.disabled(s == task.status)
                }
            }
            if task.status != .done { Button { var t = task; t.status = .done; t.finishedAt = Date(); store.saveTask(t) } label: { Label("Mark done", systemImage: "checkmark.circle") } }
            if task.status != .queued { Button { var t = task; t.status = .queued; store.saveTask(t) } label: { Label("Re-queue", systemImage: "arrow.counterclockwise") } }
            Divider()
            Button(role: .destructive) { store.deleteTask(task.id) } label: { Label("Delete", systemImage: "trash") }
        }
    }
}

private struct TaskRow: View {
    @EnvironmentObject var store: Store
    let task: AgentTask
    let showProject: Bool
    @Binding var editing: AgentTask?
    @State private var hovered = false

    var body: some View {
        let meta = TaskMeta(task: task, store: store)
        HStack(spacing: 10) {
            Image(systemName: task.status.symbol).font(.system(size: 13)).foregroundStyle(task.status.tint).frame(width: 18)
            AgentIcon(agent: task.agent, size: 12).opacity(0.8)
            Text(task.title).font(.system(size: 13)).lineLimit(1)
            if let label = meta.agentLabel {
                Text(label).font(.system(size: 10.5)).foregroundStyle(meta.needsYou ? .orange : .secondary)
                    .padding(.horizontal, 6).padding(.vertical, 2).background((meta.needsYou ? Color.orange : Color.secondary).opacity(0.12), in: Capsule())
            }
            Spacer(minLength: 12)
            if let url = task.prURL, let link = URL(string: url) {
                Link(destination: link) { Image(systemName: "arrow.triangle.pull").font(.system(size: 11)) }.help(url)
            }
            if let b = task.branch { Text(b).font(.system(size: 10.5, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle).frame(maxWidth: 200) }
            if let spec = task.spec { Image(systemName: "doc.text").font(.system(size: 11)).foregroundStyle(.tertiary).help(spec) }
            if showProject, let project = meta.project {
                HStack(spacing: 5) {
                    Circle().fill(project.tint).frame(width: 6, height: 6)
                    Text(project.name).font(.system(size: 11)).foregroundStyle(.secondary)
                }.padding(.horizontal, 7).padding(.vertical, 2).background(Color.primary.opacity(0.05), in: Capsule())
            }
            Text(task.createdAt.formatted(.relative(presentation: .named))).font(.system(size: 10.5)).foregroundStyle(.tertiary).frame(width: 72, alignment: .trailing)
            Menu { TaskActionsMenu(task: task, editing: $editing) } label: { Image(systemName: "ellipsis").frame(width: 20, height: 20) }
                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().opacity(hovered ? 1 : 0.35)
        }
        .padding(.horizontal, 14).frame(height: 36)
        .background(hovered ? Color.primary.opacity(0.04) : .clear)
        .overlay(alignment: .bottom) { Divider().padding(.leading, 42).opacity(0.6) }
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture(count: 2) { if task.sessionID != nil { meta.open() } else { editing = task } }
        .contextMenu { TaskActionsMenu(task: task, editing: $editing) }
        .draggable(task.id.uuidString)
    }
}

private struct TaskCardView: View {
    @EnvironmentObject var store: Store
    let task: AgentTask
    let showProject: Bool
    @Binding var editing: AgentTask?
    @State private var hovered = false

    var body: some View {
        let meta = TaskMeta(task: task, store: store)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                if let project = meta.project {
                    Circle().fill(project.tint).frame(width: 6, height: 6)
                    if showProject { Text(project.name).font(.system(size: 10.5, weight: .medium)).foregroundStyle(project.tint).lineLimit(1) }
                }
                AgentIcon(agent: task.agent, size: 11).opacity(0.8)
                Text(task.createdAt.formatted(.relative(presentation: .named))).font(.system(size: 10.5)).foregroundStyle(.tertiary)
                Spacer()
                Menu { TaskActionsMenu(task: task, editing: $editing) } label: { Image(systemName: "ellipsis").frame(width: 18, height: 18) }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().opacity(hovered ? 1 : 0.3)
            }
            Text(task.title).font(.system(size: 13, weight: .medium)).lineLimit(3).fixedSize(horizontal: false, vertical: true)
            if !task.details.isEmpty { Text(task.details).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(2) }
            HStack(spacing: 6) {
                if let label = meta.agentLabel {
                    Text(label).font(.system(size: 10)).foregroundStyle(meta.needsYou ? .orange : .secondary)
                        .padding(.horizontal, 6).padding(.vertical, 2).background((meta.needsYou ? Color.orange : Color.secondary).opacity(0.12), in: Capsule())
                }
                if let b = task.branch { Label(b, systemImage: "arrow.triangle.branch").font(.system(size: 10, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle) }
                if let spec = task.spec { Image(systemName: "doc.text").font(.system(size: 10)).foregroundStyle(.tertiary).help(spec) }
                Spacer()
                if let url = task.prURL, let link = URL(string: url) { Link(destination: link) { Image(systemName: "arrow.triangle.pull").font(.system(size: 11)) }.help(url) }
                switch task.status {
                case .queued, .failed:
                    Button { store.runTask(task) } label: { Image(systemName: "play.fill").font(.system(size: 10)) }.buttonStyle(.borderedProminent).controlSize(.mini).help("Run")
                case .running, .review, .pr:
                    Button { meta.open() } label: { Image(systemName: "terminal").font(.system(size: 10)) }.controlSize(.mini).help("Open session")
                case .done: EmptyView()
                }
            }
        }
        .padding(12).frame(maxWidth: .infinity, alignment: .leading)
        .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 9))
        .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(hovered ? AppTheme.accent.opacity(0.4) : AppTheme.stroke))
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture(count: 2) { if task.sessionID != nil { meta.open() } else { editing = task } }
        .contextMenu { TaskActionsMenu(task: task, editing: $editing) }
        .draggable(task.id.uuidString)
    }
}

struct TaskSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State var task: AgentTask
    @State private var runNow = true
    @FocusState private var titleFocused: Bool
    private var project: Project? { store.workspace.projects.first { $0.id == task.projectID } }
    private var specs: [String] { project.map { Docs.specFiles(in: $0.path) } ?? [] }
    private var isNew: Bool { task.title.isEmpty }
    private var canSave: Bool { !task.title.trimmingCharacters(in: .whitespaces).isEmpty }

    private func label(_ text: String) -> some View {
        Text(text.uppercased()).font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
    }

    private var modeSummary: String {
        switch task.mode {
        case "push": return "Works in a fresh worktree branch, verifies, then pushes straight to the main branch."
        case "none": return "Works in a fresh worktree branch, verifies, then commits. Nothing is pushed."
        default: return "Works in a fresh worktree branch, verifies, then pushes and opens a pull request with gh."
        }
    }

    private func agentCard(_ candidate: Agent) -> some View {
        let selected = task.agent == candidate
        return Button { task.agent = candidate } label: {
            HStack(spacing: 10) {
                AgentIcon(agent: candidate, size: 18)
                Text(candidate.rawValue).font(.system(size: 13, weight: .semibold))
                Spacer()
                if selected { Image(systemName: "checkmark.circle.fill").foregroundStyle(AppTheme.accent) }
            }.padding(.horizontal, 12).padding(.vertical, 10).frame(maxWidth: .infinity).contentShape(Rectangle())
                .background(selected ? AppTheme.accent.opacity(0.14) : Color.primary.opacity(0.03), in: RoundedRectangle(cornerRadius: 9))
                .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(selected ? AppTheme.accent.opacity(0.7) : AppTheme.stroke, lineWidth: selected ? 1.5 : 1))
        }.buttonStyle(.plain)
    }

    private var specField: some View {
        let current = task.spec.map { $0.replacingOccurrences(of: ".specdesk/specs/", with: "") }
        return Menu {
            Button { task.spec = nil } label: { if task.spec == nil { Label("None", systemImage: "checkmark") } else { Text("None") } }
            if !specs.isEmpty {
                Divider()
                ForEach(specs, id: \.self) { spec in
                    let name = spec.replacingOccurrences(of: ".specdesk/specs/", with: "")
                    Button { task.spec = spec } label: { if task.spec == spec { Label(name, systemImage: "checkmark") } else { Text(name) } }
                }
            }
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "doc.text").foregroundStyle(.tertiary).font(.system(size: 10))
                Text(current ?? "No spec").font(.system(size: 12, design: current == nil ? .default : .monospaced))
                    .foregroundStyle(current == nil ? .secondary : .primary).lineLimit(1).truncationMode(.middle)
                Spacer()
                Image(systemName: "chevron.up.chevron.down").font(.system(size: 10, weight: .semibold)).foregroundStyle(.secondary)
            }.padding(.horizontal, 10).frame(height: 30).contentShape(Rectangle())
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(AppTheme.stroke))
        }.menuStyle(.borderlessButton).menuIndicator(.hidden)
            .help(specs.isEmpty ? "No specs in .specdesk/specs yet" : "Attach a spec from .specdesk/specs")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(spacing: 10) {
                Text(isNew ? "New task" : "Edit task").font(.system(size: 18, weight: .bold))
                Spacer()
                if let project {
                    HStack(spacing: 6) {
                        ProjectIconView(project: project, size: 14)
                        Text(project.name).font(.system(size: 12, weight: .semibold)).foregroundStyle(project.tint)
                    }.padding(.horizontal, 8).padding(.vertical, 4).background(project.tint.opacity(0.12), in: Capsule())
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                label("Task")
                TextField("What should be done? e.g. Add filter by source to referrals", text: $task.title)
                    .textFieldStyle(.roundedBorder).font(.system(size: 13)).focused($titleFocused)
            }

            VStack(alignment: .leading, spacing: 6) {
                label("Details (optional)")
                TextEditor(text: $task.details).font(.system(size: 12.5)).scrollContentBackground(.hidden).padding(6).frame(height: 120)
                    .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 7))
                    .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(AppTheme.stroke))
                    .overlay(alignment: .topLeading) {
                        if task.details.isEmpty {
                            Text("Constraints, acceptance criteria, links, branch prefix…").font(.system(size: 12.5)).foregroundStyle(.tertiary)
                                .padding(.horizontal, 11).padding(.vertical, 6).allowsHitTesting(false)
                        }
                    }
            }

            HStack(alignment: .top, spacing: 16) {
                VStack(alignment: .leading, spacing: 8) {
                    label("Agent")
                    HStack(spacing: 8) { agentCard(.claude); agentCard(.codex) }
                }.frame(maxWidth: .infinity)
                VStack(alignment: .leading, spacing: 8) {
                    label("Spec")
                    specField
                }.frame(width: 220)
            }

            VStack(alignment: .leading, spacing: 8) {
                label("When done")
                Picker("", selection: $task.mode) {
                    Text("Open pull request").tag("pr")
                    Text("Push to main").tag("push")
                    Text("Commit only").tag("none")
                }.pickerStyle(.segmented).labelsHidden()
                Text(modeSummary).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }

            HStack(alignment: .center, spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Auto-review when finished").font(.system(size: 13, weight: .medium))
                    Text("The other agent inspects the result and reports back without editing.").font(.caption).foregroundStyle(.secondary)
                }
                Spacer(minLength: 12)
                Toggle("", isOn: $task.autoReview).labelsHidden().toggleStyle(.switch)
            }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 8))

            HStack(spacing: 12) {
                if task.status == .queued {
                    Toggle(isOn: $runNow) { Text("Run now").font(.system(size: 12)) }.toggleStyle(.checkbox)
                    Text("Unchecked tasks wait in the queue.").font(.caption).foregroundStyle(.tertiary)
                }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button {
                    store.saveTask(task)
                    if task.status == .queued && runNow { store.runTask(task) }
                    dismiss()
                } label: {
                    Label(task.status == .queued && runNow ? "Save & run" : "Save", systemImage: task.status == .queued && runNow ? "play.fill" : "checkmark")
                }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(!canSave)
            }
        }.padding(24).frame(width: 640)
            .onAppear { if isNew { titleFocused = true } }
    }
}
