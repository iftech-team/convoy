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

struct TasksPanel: View {
    @EnvironmentObject var store: Store
    let project: Project?           // nil = all projects
    @State private var editing: AgentTask?
    @State private var filter: AgentTask.Status?

    private var list: [AgentTask] {
        let base = project.map { store.tasks(for: $0) } ?? store.tasks
        let order: [AgentTask.Status] = [.running, .review, .pr, .queued, .failed, .done]
        return base.filter { filter == nil || $0.status == filter }.sorted { a, b in
            let ia = order.firstIndex(of: a.status) ?? 9, ib = order.firstIndex(of: b.status) ?? 9
            return ia != ib ? ia < ib : a.createdAt > b.createdAt
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Text(project.map { "Tasks · \($0.name)" } ?? "All tasks").font(.system(size: 15, weight: .semibold))
                Picker("", selection: $filter) {
                    Text("All").tag(AgentTask.Status?.none)
                    ForEach(AgentTask.Status.allCases, id: \.self) { s in Text(label(s)).tag(AgentTask.Status?.some(s)) }
                }.pickerStyle(.menu).labelsHidden().frame(width: 120)
                Spacer()
                if let project {
                    Toggle("Auto-run queue", isOn: Binding(get: { project.autoRunTasks == true }, set: { v in var p = project; p.autoRunTasks = v; store.updateProject(p) }))
                        .toggleStyle(.switch).controlSize(.small).help("Start the next queued task when one finishes")
                    Button { editing = AgentTask(projectID: project.id, title: "", mode: project.taskMode ?? "pr", agent: project.defaultAgent ?? (Agent(rawValue: UserDefaults.standard.string(forKey: "defaultAgent") ?? "") ?? .claude)) } label: { Label("New task", systemImage: "plus") }.buttonStyle(.borderedProminent)
                } else if let p = store.project {
                    Button { editing = AgentTask(projectID: p.id, title: "", mode: p.taskMode ?? "pr") } label: { Label("New task in \(p.name)", systemImage: "plus") }.buttonStyle(.borderedProminent)
                }
            }
            Text("Describe the work; the agent takes it in its own worktree, implements, verifies, then opens a PR or pushes. Status follows the agent's hooks; PR links are picked up from the terminal.")
                .font(.caption).foregroundStyle(.secondary)
            if list.isEmpty {
                Text(filter == nil ? "No tasks yet." : "No tasks with this status.").font(.caption).foregroundStyle(.secondary).padding(.top, 8)
            }
            ScrollView {
                LazyVStack(spacing: 0) { ForEach(list) { task in row(task) } }
                    .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(AppTheme.stroke))
            }
        }.padding(20)
            .sheet(item: $editing) { task in TaskSheet(task: task) }
    }

    func label(_ s: AgentTask.Status) -> String {
        switch s { case .queued: "Queued"; case .running: "Running"; case .review: "Needs review"; case .pr: "PR open"; case .done: "Done"; case .failed: "Failed" }
    }
    func tint(_ s: AgentTask.Status) -> Color {
        switch s { case .queued: .secondary; case .running: AppTheme.accent; case .review: .orange; case .pr: .purple; case .done: .green; case .failed: .red }
    }

    private func row(_ task: AgentTask) -> some View {
        let proj = store.workspace.projects.first { $0.id == task.projectID }
        let session = task.sessionID.flatMap { store.session($0) }
        let running = task.sessionID.flatMap { store.terminals.handles[$0]?.running } == true
        return HStack(alignment: .top, spacing: 12) {
            Text(label(task.status)).font(.system(size: 10, weight: .semibold)).foregroundStyle(tint(task.status))
                .padding(.horizontal, 7).padding(.vertical, 3).background(tint(task.status).opacity(0.14), in: Capsule()).frame(width: 96, alignment: .leading)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 8) {
                    Text(task.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                    if project == nil, let proj { Text(proj.name).font(.system(size: 11)).foregroundStyle(proj.tint) }
                    if let b = task.branch { Text("⎇ " + b).font(.system(size: 10)).foregroundStyle(.secondary) }
                    if let spec = task.spec { Text("spec: " + URL(fileURLWithPath: spec).lastPathComponent).font(.system(size: 10)).foregroundStyle(.secondary) }
                }
                if !task.details.isEmpty { Text(task.details).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(2) }
                HStack(spacing: 8) {
                    Text(task.mode == "pr" ? "→ pull request" : (task.mode == "push" ? "→ push to main" : "commit only")).font(.system(size: 10)).foregroundStyle(.tertiary)
                    if let url = task.prURL { Link(url.replacingOccurrences(of: "https://", with: ""), destination: URL(string: url)!).font(.system(size: 10)) }
                    if let s = session, let state = store.agentState(s), running { Text(AgentStateGlyph(state: state, running: true).label).font(.system(size: 10)).foregroundStyle(state.needsYou ? .orange : .secondary) }
                    Text(task.createdAt.formatted(.relative(presentation: .named))).font(.system(size: 10)).foregroundStyle(.tertiary)
                }
            }
            Spacer()
            HStack(spacing: 6) {
                switch task.status {
                case .queued, .failed:
                    Button("Run") { store.runTask(task) }.buttonStyle(.borderedProminent).controlSize(.small)
                    Button("Edit") { editing = task }.controlSize(.small)
                case .running:
                    if let sid = task.sessionID, let proj { Button("Open") { store.openSession(sid, in: proj.id) }.controlSize(.small) }
                    Button("Mark done") { var t = task; t.status = .done; t.finishedAt = Date(); store.saveTask(t) }.controlSize(.small)
                case .review, .pr:
                    if let sid = task.sessionID, let proj { Button("Open") { store.openSession(sid, in: proj.id) }.controlSize(.small) }
                    if let rid = task.reviewSessionID, let proj { Button("Reviewer") { store.openSession(rid, in: proj.id) }.controlSize(.small) }
                    Button("Done") { var t = task; t.status = .done; t.finishedAt = Date(); store.saveTask(t) }.buttonStyle(.borderedProminent).controlSize(.small)
                case .done:
                    if let sid = task.sessionID, let proj { Button("Session") { store.openSession(sid, in: proj.id) }.controlSize(.small) }
                }
                Menu {
                    Button("Edit…") { editing = task }
                    Button("Re-queue") { var t = task; t.status = .queued; store.saveTask(t) }
                    Button("Mark failed") { var t = task; t.status = .failed; store.saveTask(t) }
                    Divider()
                    Button("Delete", role: .destructive) { store.deleteTask(task.id) }
                } label: { Image(systemName: "ellipsis").frame(width: 20, height: 20) }.menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
            }
        }.padding(.horizontal, 14).padding(.vertical, 10).overlay(alignment: .bottom) { Divider().padding(.leading, 14) }
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
