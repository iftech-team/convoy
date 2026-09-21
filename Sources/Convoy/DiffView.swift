import SwiftUI

struct DiffFile: Identifiable, Equatable {
    let path: String
    let status: String      // M A D R ? etc.
    let added: Int
    let removed: Int
    var staged: Bool = false
    var id: String { path }
}

struct DiffLine: Identifiable {
    enum Kind { case context, add, remove, hunk, meta }
    let id: Int
    let kind: Kind
    let text: String
    let newLine: Int?
    let oldLine: Int?
}

struct DiffComment: Identifiable, Equatable {
    let id = UUID()
    let file: String
    let line: Int
    var text: String
}

/// Changes for a session's directory against its base (merge-base with `baseRef`, or HEAD), Orca-style:
/// committed + staged + unstaged + untracked in one view, with line comments you can send to the agent.
@MainActor
final class DiffModel: ObservableObject {
    @Published var files: [DiffFile] = []
    @Published var selected: String?
    @Published var lines: [DiffLine] = []
    @Published var comments: [DiffComment] = []
    @Published var loading = false
    @Published var baseLabel = "HEAD"
    let directory: String
    let baseRef: String?
    private var loadingFile = false

    init(directory: String, baseRef: String?) { self.directory = directory; self.baseRef = baseRef }

    func refresh() {
        guard !loading else { return }
        loading = true
        let dir = directory, base = baseRef
        Task.detached(priority: .userInitiated) {
            var from = "HEAD"
            if let base, !base.isEmpty, let (s, out) = GitInfoService.run(["merge-base", "HEAD", base], in: dir), s == 0 {
                let mb = out.trimmingCharacters(in: .whitespacesAndNewlines); if !mb.isEmpty { from = mb }
            }
            var files: [DiffFile] = []
            if let (_, out) = GitInfoService.run(["-c", "core.quotePath=false", "diff", "--numstat", "-M", from], in: dir) {
                for line in out.split(separator: "\n") {
                    let parts = line.split(separator: "\t", maxSplits: 2).map(String.init)
                    guard parts.count == 3 else { continue }
                    files.append(DiffFile(path: parts[2].components(separatedBy: " => ").last ?? parts[2], status: parts[2].contains(" => ") ? "R" : "M", added: Int(parts[0]) ?? 0, removed: Int(parts[1]) ?? 0))
                }
            }
            if let (_, out) = GitInfoService.run(["-c", "core.quotePath=false", "ls-files", "--others", "--exclude-standard"], in: dir) {
                for line in out.split(separator: "\n") where !line.isEmpty {
                    let path = String(line)
                    let count = (try? String(contentsOfFile: dir + "/" + path, encoding: .utf8))?.split(separator: "\n", omittingEmptySubsequences: false).count ?? 0
                    files.append(DiffFile(path: path, status: "?", added: count, removed: 0))
                }
            }
            // Deleted files show in diff --numstat already (0 added). Mark them.
            if let (_, out) = GitInfoService.run(["diff", "--name-status", from], in: dir) {
                let deleted = Set(out.split(separator: "\n").filter { $0.hasPrefix("D") }.compactMap { $0.split(separator: "\t").last.map(String.init) })
                files = files.map { deleted.contains($0.path) ? DiffFile(path: $0.path, status: "D", added: $0.added, removed: $0.removed) : $0 }
            }
            var stagedPaths: Set<String> = []
            if let (_, out) = GitInfoService.run(["diff", "--cached", "--name-only"], in: dir) { stagedPaths = Set(out.split(separator: "\n").map(String.init)) }
            files = files.map { var f = $0; f.staged = stagedPaths.contains($0.path); return f }
            let label = base.map { "\($0) (merge-base)" } ?? "HEAD"
            await MainActor.run {
                self.files = files.sorted { $0.path < $1.path }
                self.baseLabel = label
                self.loading = false
                if let sel = self.selected, !files.contains(where: { $0.path == sel }) { self.selected = nil; self.lines = [] }
                if self.selected == nil, let first = files.first { self.select(first.path) }
                else if let sel = self.selected { self.select(sel) }
            }
        }
    }

    func select(_ path: String) {
        selected = path
        let dir = directory, base = baseRef
        let untracked = files.first { $0.path == path }?.status == "?"
        Task.detached(priority: .userInitiated) {
            var from = "HEAD"
            if let base, !base.isEmpty, let (s, out) = GitInfoService.run(["merge-base", "HEAD", base], in: dir), s == 0 {
                let mb = out.trimmingCharacters(in: .whitespacesAndNewlines); if !mb.isEmpty { from = mb }
            }
            var raw = ""
            if untracked {
                let content = (try? String(contentsOfFile: dir + "/" + path, encoding: .utf8)) ?? ""
                raw = "@@ -0,0 +1,\(content.split(separator: "\n").count) @@\n" + content.split(separator: "\n", omittingEmptySubsequences: false).map { "+" + $0 }.joined(separator: "\n")
            } else if let (_, out) = GitInfoService.run(["-c", "core.quotePath=false", "diff", "-M", from, "--", path], in: dir) { raw = out }
            let parsed = Self.parse(raw)
            await MainActor.run { if self.selected == path { self.lines = parsed } }
        }
    }

    nonisolated static func parse(_ raw: String) -> [DiffLine] {
        var result: [DiffLine] = []
        var old = 0, new = 0, id = 0
        for line in raw.split(separator: "\n", omittingEmptySubsequences: false).map(String.init) {
            id += 1
            if line.hasPrefix("@@") {
                if let range = line.range(of: #"-(\d+)(?:,\d+)? \+(\d+)"#, options: .regularExpression) {
                    let nums = line[range].split(whereSeparator: { !$0.isNumber }).compactMap { Int($0) }
                    if nums.count >= 2 { old = nums[0]; new = nums[1] }
                }
                result.append(DiffLine(id: id, kind: .hunk, text: line, newLine: nil, oldLine: nil))
            } else if line.hasPrefix("+++") || line.hasPrefix("---") || line.hasPrefix("diff ") || line.hasPrefix("index ") || line.hasPrefix("new file") || line.hasPrefix("deleted file") || line.hasPrefix("similarity") || line.hasPrefix("rename") {
                continue
            } else if line.hasPrefix("+") {
                result.append(DiffLine(id: id, kind: .add, text: String(line.dropFirst()), newLine: new, oldLine: nil)); new += 1
            } else if line.hasPrefix("-") {
                result.append(DiffLine(id: id, kind: .remove, text: String(line.dropFirst()), newLine: nil, oldLine: old)); old += 1
            } else if line.hasPrefix("\\") {
                result.append(DiffLine(id: id, kind: .meta, text: line, newLine: nil, oldLine: nil))
            } else if !line.isEmpty || result.last?.kind != nil {
                result.append(DiffLine(id: id, kind: .context, text: line.hasPrefix(" ") ? String(line.dropFirst()) : line, newLine: new, oldLine: old)); new += 1; old += 1
            }
        }
        return result
    }

    /// Message handed to the agent: grouped per file, Orca's "send to agent" shape.
    func reviewMessage() -> String {
        var out = "Please address these review notes on the current changes:\n"
        for file in Dictionary(grouping: comments, by: \.file).keys.sorted() {
            out += "\n\(file)\n"
            for c in comments.filter({ $0.file == file }).sorted(by: { $0.line < $1.line }) { out += "  line \(c.line): \(c.text)\n" }
        }
        out += "\nVerify each fix and report what changed."
        return out
    }
}

struct DiffPanel: View {
    @EnvironmentObject var store: Store
    @StateObject private var model: DiffModel
    let sessionID: UUID
    let onClose: () -> Void
    @State private var editingLine: Int?
    @State private var draft = ""
    @State private var wrap = false
    @State private var commitMessage = ""
    @State private var busy = false
    @State private var output: String?
    @State private var showCommit = true

    init(directory: String, baseRef: String?, sessionID: UUID, onClose: @escaping () -> Void) {
        _model = StateObject(wrappedValue: DiffModel(directory: directory, baseRef: baseRef))
        self.sessionID = sessionID; self.onClose = onClose
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text("Changes").font(.system(size: 12, weight: .semibold))
                Text("vs \(model.baseLabel)").font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
                if model.loading { ProgressView().controlSize(.mini) }
                Toggle(isOn: $wrap) { Image(systemName: "text.word.spacing") }.toggleStyle(.button).buttonStyle(.plain).help("Wrap lines")
                Button { model.refresh() } label: { Image(systemName: "arrow.clockwise") }.buttonStyle(.plain).help("Refresh")
                Button(action: onClose) { Image(systemName: "xmark") }.buttonStyle(.plain).help("Close changes")
            }.font(.system(size: 11)).foregroundStyle(.secondary).padding(.horizontal, 10).frame(height: 32).background(AppTheme.raised)
            Divider()
            HSplitView {
                VStack(spacing: 0) {
                List(selection: Binding(get: { model.selected }, set: { if let v = $0 { model.select(v) } })) {
                    ForEach(model.files) { file in
                        HStack(spacing: 6) {
                            Button { toggleStage(file) } label: {
                                Image(systemName: file.staged ? "checkmark.square.fill" : "square").font(.system(size: 11)).foregroundStyle(file.staged ? AppTheme.accent : .secondary)
                            }.buttonStyle(.plain).help(file.staged ? "Unstage" : "Stage")
                            Text(file.status).font(.system(size: 9, weight: .bold, design: .monospaced))
                                .foregroundStyle(file.status == "D" ? Color.red : (file.status == "?" || file.status == "A" ? Color.green : AppTheme.accent)).frame(width: 12)
                            Text(file.path).font(.system(size: 11)).lineLimit(1).truncationMode(.head)
                            Spacer()
                            Text("+\(file.added)").font(.system(size: 9, design: .monospaced)).foregroundStyle(.green)
                            Text("−\(file.removed)").font(.system(size: 9, design: .monospaced)).foregroundStyle(.red)
                            if model.comments.contains(where: { $0.file == file.path }) { Image(systemName: "text.bubble.fill").font(.system(size: 9)).foregroundStyle(.orange) }
                        }.tag(file.path)
                    }
                    if model.files.isEmpty && !model.loading { Text("No changes").font(.caption).foregroundStyle(.secondary) }
                }.listStyle(.sidebar)
                Divider()
                sourceControl
                }.frame(minWidth: 220, idealWidth: 260, maxWidth: 360)
                VStack(spacing: 0) {
                    ScrollView(wrap ? [.vertical] : [.vertical, .horizontal]) {
                        LazyVStack(alignment: .leading, spacing: 0) {
                            ForEach(model.lines) { line in
                                row(line)
                                if let key = line.newLine ?? line.oldLine, let path = model.selected {
                                    ForEach(model.comments.filter { $0.file == path && $0.line == key && (line.newLine != nil || line.kind == .remove) }) { c in commentRow(c) }
                                    if editingLine == line.id { editor(path: path, line: key) }
                                }
                            }
                        }.frame(maxWidth: wrap ? .infinity : nil, alignment: .leading)
                    }
                    if !model.comments.isEmpty {
                        Divider()
                        HStack {
                            Text("\(model.comments.count) note\(model.comments.count == 1 ? "" : "s")").font(.caption).foregroundStyle(.secondary)
                            Spacer()
                            Button("Clear") { model.comments = [] }.controlSize(.small)
                            Button("Send to agent") {
                                store.send(QuickCommand(title: "Review notes", text: model.reviewMessage(), submit: false), to: sessionID)
                                model.comments = []
                            }.buttonStyle(.borderedProminent).controlSize(.small).disabled(store.terminals.handles[sessionID]?.running != true)
                        }.padding(8).background(AppTheme.raised)
                    }
                }.frame(minWidth: 300)
            }
        }
        .onAppear { model.refresh() }
        .onReceive(store.git.$info) { _ in model.refresh() }
    }

    private var stagedCount: Int { model.files.filter(\.staged).count }

    private var sourceControl: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text("SOURCE CONTROL").font(.system(size: 9, weight: .semibold)).tracking(0.5).foregroundStyle(.secondary)
                Spacer()
                Button(stagedCount == model.files.count && !model.files.isEmpty ? "Unstage all" : "Stage all") { stageAll(stagedCount != model.files.count) }.controlSize(.mini).disabled(model.files.isEmpty || busy)
            }
            TextField("Commit message", text: $commitMessage, axis: .vertical).textFieldStyle(.roundedBorder).font(.system(size: 11)).lineLimit(2...5).disabled(busy)
            HStack(spacing: 6) {
                Button { Task { await aiMessage() } } label: { Label("AI message", systemImage: "sparkles") }.controlSize(.small).disabled(busy || model.files.isEmpty)
                Spacer()
                Button("Commit") { Task { await commit() } }.buttonStyle(.borderedProminent).controlSize(.small)
                    .disabled(busy || stagedCount == 0 || commitMessage.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            HStack(spacing: 6) {
                Button { Task { await push() } } label: { Label("Push", systemImage: "arrow.up.circle") }.controlSize(.small).disabled(busy)
                Button { Task { await openPR() } } label: { Label("Pull request", systemImage: "arrow.triangle.pull") }.controlSize(.small).disabled(busy)
                    .help("Opens GitHub's PR page for this branch (needs the gh CLI)")
                Spacer()
                if busy { ProgressView().controlSize(.mini) }
            }
            if let output { Text(output).font(.system(size: 10, design: .monospaced)).foregroundStyle(output.hasPrefix("✓") ? .green : .orange).lineLimit(4).textSelection(.enabled) }
        }.padding(10).background(AppTheme.raised)
    }

    private func toggleStage(_ file: DiffFile) {
        Task {
            let r = await store.gitCommand(file.staged ? ["restore", "--staged", "--", file.path] : ["add", "--", file.path], in: model.directory)
            if !r.ok { output = r.output }
            model.refresh()
        }
    }
    private func stageAll(_ stage: Bool) {
        Task { let r = await store.gitCommand(stage ? ["add", "-A"] : ["reset", "-q"], in: model.directory); if !r.ok { output = r.output }; model.refresh() }
    }
    private func aiMessage() async {
        busy = true; output = "Asking Claude for a message…"
        if stagedCount == 0 { _ = await store.gitCommand(["add", "-A"], in: model.directory); model.refresh() }
        if let message = await store.generateCommitMessage(in: model.directory) { commitMessage = message; output = nil } else { output = "Couldn’t generate a message. Is claude installed and logged in?" }
        busy = false
    }
    private func commit() async {
        busy = true
        let r = await store.gitCommand(["commit", "-m", commitMessage], in: model.directory)
        output = r.ok ? "✓ " + (r.output.split(separator: "\n").first.map(String.init) ?? "Committed") : r.output
        if r.ok { commitMessage = "" }
        busy = false; model.refresh(); store.git.refreshNow([model.directory])
    }
    private func push() async {
        busy = true; output = "Pushing…"
        var r = await store.gitCommand(["push"], in: model.directory, timeout: 180)
        if !r.ok, r.output.contains("no upstream") || r.output.contains("has no upstream") { r = await store.gitCommand(["push", "-u", "origin", "HEAD"], in: model.directory, timeout: 180) }
        output = r.ok ? "✓ Pushed" : r.output
        busy = false; store.git.refreshNow([model.directory])
    }
    private func openPR() async {
        busy = true
        let directory = model.directory
        let r = await Task.detached { () -> (Bool, String) in
            let p = Process(); p.executableURL = URL(fileURLWithPath: "/bin/zsh"); p.arguments = ["-ilc", "gh pr create --web --fill"]
            p.currentDirectoryURL = URL(fileURLWithPath: directory)
            let pipe = Pipe(); p.standardOutput = pipe; p.standardError = pipe
            do { try p.run() } catch { return (false, "Couldn’t run gh.") }
            p.waitUntilExit()
            return (p.terminationStatus == 0, String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? "")
        }.value
        output = r.0 ? "✓ Opened PR page in browser" : (r.1.isEmpty ? "gh CLI not found. Install GitHub CLI (brew install gh) and run gh auth login." : r.1.trimmingCharacters(in: .whitespacesAndNewlines))
        busy = false
    }

    private func row(_ line: DiffLine) -> some View {
        let bg: Color = switch line.kind { case .add: .green.opacity(0.13); case .remove: .red.opacity(0.13); case .hunk: AppTheme.accent.opacity(0.08); default: .clear }
        let sign = switch line.kind { case .add: "+"; case .remove: "−"; default: " " }
        return HStack(spacing: 0) {
            Text(line.oldLine.map(String.init) ?? "").frame(width: 40, alignment: .trailing)
            Text(line.newLine.map(String.init) ?? "").frame(width: 40, alignment: .trailing)
            Button { if line.kind != .hunk && line.kind != .meta { editingLine = editingLine == line.id ? nil : line.id; draft = "" } } label: {
                Image(systemName: "plus.bubble").font(.system(size: 9)).frame(width: 18).opacity(line.kind == .hunk || line.kind == .meta ? 0 : 0.5)
            }.buttonStyle(.plain).help("Add a note on this line")
            Text(sign).frame(width: 12)
            Text(line.text).lineLimit(wrap ? nil : 1).fixedSize(horizontal: !wrap, vertical: false)
            Spacer(minLength: 0)
        }.font(.system(size: 11, design: .monospaced)).foregroundStyle(line.kind == .hunk || line.kind == .meta ? .secondary : .primary)
            .padding(.vertical, 1).background(bg)
    }

    private func commentRow(_ c: DiffComment) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "text.bubble.fill").foregroundStyle(.orange).font(.system(size: 10)).padding(.top, 2)
            Text(c.text).font(.system(size: 11)).textSelection(.enabled)
            Spacer()
            Button { model.comments.removeAll { $0.id == c.id } } label: { Image(systemName: "xmark").font(.system(size: 9)) }.buttonStyle(.plain).foregroundStyle(.secondary)
        }.padding(8).padding(.leading, 90).background(Color.orange.opacity(0.08))
    }

    private func editor(path: String, line: Int) -> some View {
        HStack(spacing: 8) {
            TextField("Note for the agent about line \(line)…", text: $draft, axis: .vertical).textFieldStyle(.roundedBorder).font(.system(size: 11)).lineLimit(1...4)
                .onSubmit { commit(path: path, line: line) }
            Button("Add") { commit(path: path, line: line) }.controlSize(.small).disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty)
            Button("Cancel") { editingLine = nil }.controlSize(.small)
        }.padding(8).padding(.leading, 90).background(AppTheme.card)
    }

    private func commit(path: String, line: Int) {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        model.comments.append(DiffComment(file: path, line: line, text: text))
        draft = ""; editingLine = nil
    }
}
