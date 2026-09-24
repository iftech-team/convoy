import AppKit
import CoreServices
import SwiftUI

enum GitPanelTab: String, CaseIterable, Identifiable { case changes = "Changes", files = "Files", log = "Log"; var id: String { rawValue } }
enum DiffLayout: String { case unified, sideBySide }
enum ResetMode: String { case soft, mixed }

/// What the diff area shows.
enum DiffTarget: Hashable {
    case unstaged(String), staged(String), untracked(String), conflicted(String), commit(sha: String, path: String)
    var path: String {
        switch self {
        case .unstaged(let p), .staged(let p), .untracked(let p), .conflicted(let p): return p
        case .commit(_, let p): return p
        }
    }
    var isWorkingTree: Bool { if case .commit = self { return false }; return true }
    var isStaged: Bool { if case .staged = self { return true }; return false }
}

struct FileContent: Equatable {
    enum Body: Equatable { case text([String]), markdown(String), image(URL), binary(Int), tooLarge(Int), missing }
    let path: String
    let body: Body
}

struct ActionResult: Equatable {
    var ok: Bool
    var text: String
}

/// State for one repository's Files & Changes panel. Cached per directory in `Store` so drafts survive tab switches.
/// All git runs off the main thread; results hop back and publish only when something changed.
@MainActor
final class GitPanelModel: ObservableObject {
    let requestedDirectory: String
    /// Repository toplevel once resolved; every command runs here so paths are root-relative.
    @Published private(set) var directory: String
    unowned let store: Store

    @Published var tab: GitPanelTab = .changes { didSet { if tab != oldValue { loadTabData() } } }
    @Published private(set) var status: GitStatusSnapshot?
    /// Added/removed line counts for staged and unstaged files, keyed by diff target.
    @Published private(set) var counts: [DiffTarget: LineCounts] = [:]
    @Published private(set) var isRepo = true
    @Published private(set) var resolved = false
    @Published private(set) var refreshing = false
    @Published var target: DiffTarget? { didSet { if target != oldValue { loadDiff() } } }
    @Published private(set) var diff: FileDiff?
    @Published private(set) var diffLoading = false
    @Published var fullDiff = false { didSet { if fullDiff != oldValue { loadDiff() } } }
    @Published var message = ""
    @Published var amend = false { didSet { if amend && !oldValue && message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { prefillAmendMessage() } } }
    @Published private(set) var busy: String?
    @Published private(set) var lastResult: ActionResult?
    @Published private(set) var commits: [GitCommit] = []
    @Published private(set) var logLoading = false
    @Published var selectedCommit: String? { didSet { if selectedCommit != oldValue { loadCommitFiles() } } }
    @Published private(set) var commitFiles: [GitCommitFile] = []
    @Published private(set) var branches: [GitBranch] = []
    @Published private(set) var tree: [FileNode] = []
    @Published private(set) var treeLoading = false
    @Published var selectedFile: String? { didSet { if selectedFile != oldValue { openFile() } } }
    @Published private(set) var fileContent: FileContent?
    @Published var maximizeDiff = false

    private var watcher: DirectoryWatcher?
    private var timer: Timer?
    private var inFlight = false
    private var dirty = false
    private var diffGeneration = 0
    private var fileGeneration = 0
    private var pending: [(String, () async -> Void)] = []

    init(directory: String, store: Store) {
        requestedDirectory = directory; self.directory = directory; self.store = store
    }

    // MARK: Lifecycle

    func activate() {
        guard watcher == nil else { refresh(); return }
        watcher = DirectoryWatcher(path: directory) { [weak self] in Task { @MainActor in self?.refresh() } }
        timer = Timer.scheduledTimer(withTimeInterval: 10, repeats: true) { [weak self] _ in Task { @MainActor in self?.refresh() } }
        refresh(); loadBranches()
    }

    func deactivate() {
        watcher?.stop(); watcher = nil
        timer?.invalidate(); timer = nil
    }

    // MARK: Status

    func refresh() {
        guard !inFlight else { dirty = true; return }
        inFlight = true; refreshing = true
        let dir = directory, resolved = self.resolved
        Task.detached(priority: .userInitiated) {
            var top = dir, isRepo = true
            if !resolved {
                if let (s, out) = GitInfoService.run(["rev-parse", "--show-toplevel"], in: dir), s == 0, !out.isEmpty { top = out.trimmingCharacters(in: .whitespacesAndNewlines) } else { isRepo = false }
            }
            var snapshot: GitStatusSnapshot?
            var counts: [DiffTarget: LineCounts] = [:]
            if isRepo, let (s, out) = GitInfoService.run(["-c", "core.quotePath=false", "status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all"], in: top, timeout: 20), s == 0 {
                snapshot = GitStatusParser.parse(porcelainV2Z: out)
                if snapshot?.files.isEmpty == false {
                    for (path, c) in GitNumstatParser.parse(GitInfoService.run(["-c", "core.quotePath=false", "diff", "--numstat", "-M"], in: top, timeout: 20)?.1 ?? "") { counts[.unstaged(path)] = c }
                    for (path, c) in GitNumstatParser.parse(GitInfoService.run(["-c", "core.quotePath=false", "diff", "--numstat", "-M", "--cached"], in: top, timeout: 20)?.1 ?? "") { counts[.staged(path)] = c }
                }
            } else if isRepo { isRepo = false }
            await MainActor.run { self.finishRefresh(top: top, isRepo: isRepo, snapshot: snapshot, counts: counts) }
        }
    }

    private func finishRefresh(top: String, isRepo: Bool, snapshot: GitStatusSnapshot?, counts: [DiffTarget: LineCounts]) {
        let firstResolve = !resolved
        if directory != top {
            directory = top
            if watcher != nil { deactivate(); activate() }
        }
        self.isRepo = isRepo; resolved = true
        if snapshot != status { status = snapshot }
        if counts != self.counts { self.counts = counts }
        inFlight = false; refreshing = false
        if let target, target.isWorkingTree {
            if let file = snapshot?.files.first(where: { $0.path == target.path }) {
                let stillAvailable: Bool
                switch target {
                case .staged: stillAvailable = file.hasStaged && !file.conflicted
                case .unstaged: stillAvailable = file.hasUnstaged && !file.conflicted && !file.isUntracked
                case .untracked: stillAvailable = file.isUntracked
                case .conflicted: stillAvailable = file.conflicted
                case .commit: stillAvailable = true
                }
                if stillAvailable { loadDiff() } else { self.target = defaultTarget(for: file) }
            } else { self.target = nil }
        }
        if target == nil, tab == .changes, let first = snapshot?.files.first { target = defaultTarget(for: first) }
        if firstResolve || tab != .changes { loadTabData() }
        if dirty { dirty = false; refresh() }
    }

    func defaultTarget(for file: GitFileStatus) -> DiffTarget {
        if file.conflicted { return .conflicted(file.path) }
        if file.isUntracked { return .untracked(file.path) }
        if file.hasUnstaged { return .unstaged(file.path) }
        return .staged(file.path)
    }

    private func loadTabData() {
        switch tab {
        case .changes: break
        case .log: loadLog()
        case .files: loadTree()
        }
    }

    // MARK: Diff

    func loadDiff() {
        diffGeneration += 1
        guard let target else { diff = nil; diffLoading = false; return }
        let generation = diffGeneration
        diffLoading = true
        let dir = directory, maxLines = fullDiff ? Int.max : DiffParser.defaultMaxLines
        let original = status?.files.first { $0.path == target.path }?.originalPath
        Task.detached(priority: .userInitiated) {
            var result: FileDiff?
            let base = ["-c", "core.quotePath=false", "diff", "--no-color", "--no-ext-diff", "-M"]
            switch target {
            case .unstaged(let path):
                if let (_, out) = GitInfoService.run(base + ["--"] + [path] + (original.map { [$0] } ?? []), in: dir, timeout: 20) { result = DiffParser.parseSingle(out, maxLines: maxLines) }
            case .staged(let path):
                if let (_, out) = GitInfoService.run(base + ["--cached", "--"] + [path] + (original.map { [$0] } ?? []), in: dir, timeout: 20) { result = DiffParser.parseSingle(out, maxLines: maxLines) }
            case .untracked(let path), .conflicted(let path):
                let url = URL(fileURLWithPath: dir + "/" + path)
                if let data = TextFileInspector.readBounded(url) {
                    if TextFileInspector.isBinary(data) { result = FileDiff(newPath: path, isBinary: true, isNew: target == .untracked(path)) }
                    else {
                        var file = DiffParser.untrackedDiff(path: path, contents: String(decoding: data, as: UTF8.self))
                        if case .conflicted = target { file.isNew = false }
                        if file.lineCount > maxLines { file.hunks[0].lines = Array(file.hunks[0].lines.prefix(maxLines)); file.truncated = true }
                        result = file
                    }
                } else { result = FileDiff(newPath: path, truncated: true) }
            case .commit(let sha, let path):
                if let (_, out) = GitInfoService.run(["-c", "core.quotePath=false", "show", "--no-color", "--no-ext-diff", "--format=", "-M", sha, "--", path], in: dir, timeout: 20) { result = DiffParser.parseSingle(out, maxLines: maxLines) }
            }
            await MainActor.run {
                guard generation == self.diffGeneration else { return }
                self.diff = result ?? FileDiff(newPath: target.path)
                self.diffLoading = false
            }
        }
    }

    // MARK: Serial git operations

    /// Runs one operation at a time so two clicks never fight over index.lock. `label` shows while it runs.
    private func enqueue(_ label: String, _ operation: @escaping () async -> Void) {
        pending.append((label, operation)); pump()
    }

    private func pump() {
        guard busy == nil, !pending.isEmpty else { return }
        let (label, operation) = pending.removeFirst()
        busy = label
        Task { await operation(); busy = nil; pump() }
    }

    /// One git command with the standard outcome handling; returns success.
    private func git(_ args: [String], timeout: TimeInterval = 60, input: Data? = nil, success: String? = nil, failureHint: String? = nil) async -> Bool {
        let result = await store.gitCommand(args, in: directory, timeout: timeout, input: input, environment: ["GIT_EDITOR": "true"])
        if result.ok {
            if let success { lastResult = ActionResult(ok: true, text: success) }
        } else {
            var text = result.output.isEmpty ? "git \(args.first ?? "") failed." : result.output
            if let failureHint { text += "\n\n" + failureHint }
            lastResult = ActionResult(ok: false, text: text)
            store.error = text
        }
        refresh(); store.git.refreshNow([directory, requestedDirectory])
        return result.ok
    }

    // MARK: Changes

    func stage(_ paths: [String]) { guard !paths.isEmpty else { return }; enqueue("Staging…") { [self] in _ = await git(["add", "--"] + paths) } }
    private func hasHead() async -> Bool {
        let dir = directory
        return await Task.detached { GitInfoService.run(["rev-parse", "--verify", "HEAD"], in: dir)?.0 == 0 }.value
    }
    func unstage(_ paths: [String]) {
        guard !paths.isEmpty else { return }
        enqueue("Unstaging…") { [self] in
            let args = await hasHead() ? ["restore", "--staged", "--"] : ["rm", "--cached", "--"]
            _ = await git(args + paths)
        }
    }
    func stageAll() { enqueue("Staging…") { [self] in _ = await git(["add", "-A"]) } }
    func unstageAll() { enqueue("Unstaging…") { [self] in let args = await hasHead() ? ["reset", "-q"] : ["rm", "--cached", "-r", "--", "."]; _ = await git(args) } }
    func markResolved(_ path: String) { stage([path]) }

    func toggleStaged(_ file: GitFileStatus) {
        if file.conflicted { markResolved(file.path) }
        else if file.hasStaged && !file.hasUnstaged { unstage([file.path]) }
        else { stage([file.path]) }
    }

    /// Throws away working-tree changes. Untracked files go to the Trash so a mistake is recoverable.
    func discard(_ file: GitFileStatus, includeStaged: Bool) {
        if file.isUntracked { deleteUntracked([file.path]); return }
        enqueue("Discarding…") { [self] in
            if includeStaged {
                if file.index == .added || file.index == .renamed || file.index == .copied {
                    guard await git(["restore", "--staged", "--", file.path] + (file.originalPath.map { [$0] } ?? [])) else { return }
                    if file.index == .added { trash([file.path]) } else { _ = await git(["restore", "--"] + [file.path] + (file.originalPath.map { [$0] } ?? [])) }
                    return
                }
                _ = await git(["restore", "--source=HEAD", "--staged", "--worktree", "--", file.path], success: "Discarded \(file.path)")
            } else {
                _ = await git(["restore", "--", file.path], success: "Discarded changes in \(file.path)")
            }
        }
    }

    func discardHunk(_ hunk: DiffHunk) {
        guard let diff, let target, target.isWorkingTree else { return }
        if case .untracked = target { return }
        let patch = DiffParser.patch(for: diff, hunk: hunk)
        var args = ["apply", "--reverse", "--recount", "--whitespace=nowarn"]
        if target.isStaged { args.append("--cached") }
        args.append("-")
        enqueue("Discarding hunk…") { [self] in _ = await git(args, input: patch.data(using: .utf8), success: "Hunk discarded", failureHint: "The file changed since the diff was read, or the patch has whitespace differences. Discard the whole file instead.") }
    }

    func deleteUntracked(_ paths: [String]) {
        enqueue("Deleting…") { [self] in trash(paths); refresh(); store.git.refreshNow([directory]) }
    }

    private func trash(_ paths: [String]) {
        for path in paths {
            do { try FileManager.default.trashItem(at: URL(fileURLWithPath: directory + "/" + path), resultingItemURL: nil) }
            catch { store.error = "Couldn’t move \(path) to the Trash: \(error.localizedDescription)"; return }
        }
        lastResult = ActionResult(ok: true, text: paths.count == 1 ? "Moved \(paths[0]) to the Trash" : "Moved \(paths.count) files to the Trash")
    }

    // MARK: Commit / push / pull

    var canCommit: Bool {
        busy == nil && !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && (amend || !(status?.staged.isEmpty ?? true))
    }

    func commit(andPush: Bool) {
        let text = message.trimmingCharacters(in: .whitespacesAndNewlines)
        let requestedAmend = amend
        guard !text.isEmpty else { return }
        enqueue(andPush ? "Committing…" : "Committing…") { [self] in
            var args = ["commit", "-m", text]
            if requestedAmend { args.append("--amend") }
            guard await git(args) else { return }
            let short = await store.gitCommand(["rev-parse", "--short", "HEAD"], in: directory).output
            lastResult = ActionResult(ok: true, text: "Committed \(short)")
            if message.trimmingCharacters(in: .whitespacesAndNewlines) == text { message = ""; amend = false }
            if tab == .log { loadLog() }
            if andPush { busy = "Pushing…"; await pushNow() }
        }
    }

    private func prefillAmendMessage() {
        let dir = directory
        Task.detached(priority: .userInitiated) {
            let text = GitInfoService.run(["log", "-1", "--format=%B"], in: dir)?.1.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            await MainActor.run { if self.amend && self.message.isEmpty { self.message = text } }
        }
    }

    func generateMessage() {
        enqueue("Asking Claude for a message…") { [self] in
            if let text = await store.generateCommitMessage(in: directory) { message = text; lastResult = nil }
            else { lastResult = ActionResult(ok: false, text: "Couldn’t generate a message. Is there a diff, and is claude installed and logged in?") }
        }
    }

    func push() { enqueue("Pushing…") { [self] in await pushNow() } }

    private func pushNow() async {
        let hint = "Push from the terminal if git needs to authenticate."
        let result = await store.gitCommand(["push"], in: directory, timeout: 180)
        if result.ok { finishPush(result.output); return }
        if result.output.contains("no upstream") {
            let retry = await store.gitCommand(["push", "-u", "origin", "HEAD"], in: directory, timeout: 180)
            if retry.ok { finishPush(retry.output); return }
            fail(retry.output, hint: hint); return
        }
        fail(result.output, hint: hint)
    }

    private func finishPush(_ output: String) {
        lastResult = ActionResult(ok: true, text: "Pushed"); store.notice = "Pushed \(status?.branch ?? "")".trimmingCharacters(in: .whitespaces)
        refresh(); store.git.refreshNow([directory, requestedDirectory])
    }

    private func fail(_ output: String, hint: String? = nil) {
        var text = output.isEmpty ? "git failed." : output
        if let hint { text += "\n\n" + hint }
        lastResult = ActionResult(ok: false, text: text); store.error = text
        refresh(); store.git.refreshNow([directory, requestedDirectory])
    }

    func fetch() {
        enqueue("Fetching…") { [self] in
            if await git(["fetch", "--prune"], timeout: 120, success: "Fetched") { store.notice = "Fetched from remote"; loadBranches(); if tab == .log { loadLog() } }
        }
    }

    func pull() {
        enqueue("Pulling…") { [self] in
            if await git(["pull", "--ff-only"], timeout: 180, success: "Pulled", failureHint: "Only fast-forward pulls run here. Rebase or merge from the terminal.") { store.notice = "Pulled \(status?.branch ?? "")"; if tab == .log { loadLog() } }
        }
    }

    // MARK: Branches

    func loadBranches() {
        let dir = directory
        Task.detached(priority: .utility) {
            let out = GitInfoService.run(["for-each-ref", "--sort=-committerdate", "--format=%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(committerdate:unix)", "refs/heads", "refs/remotes"], in: dir)?.1 ?? ""
            let list = GitBranchParser.parse(out)
            await MainActor.run { if list != self.branches { self.branches = list } }
        }
    }

    func checkout(_ branch: GitBranch) {
        enqueue("Switching…") { [self] in
            let args: [String]
            if branch.isRemote {
                if branches.contains(where: { !$0.isRemote && $0.name == branch.shortName }) { args = ["switch", branch.shortName] }
                else { args = ["switch", "-c", branch.shortName, "--track", branch.name] }
            } else { args = ["switch", branch.name] }
            if await git(args, success: "Switched to \(branch.shortName)") { store.notice = "Switched to \(branch.shortName)"; loadBranches(); if tab == .log { loadLog() } }
        }
    }

    func createBranch(_ name: String, from start: String?) {
        let clean = name.trimmingCharacters(in: .whitespaces)
        guard !clean.isEmpty else { return }
        enqueue("Creating branch…") { [self] in
            var args = ["switch", "-c", clean]
            if let start, !start.isEmpty { args.append(start) }
            if await git(args, success: "Created \(clean)") { store.notice = "Created and switched to \(clean)"; loadBranches(); if tab == .log { loadLog() } }
        }
    }

    // MARK: Log

    func loadLog() {
        guard isRepo else { return }
        logLoading = true
        let dir = directory
        Task.detached(priority: .userInitiated) {
            let out = GitInfoService.run(["log", "--no-color", "-n", "200", "--format=%H%x00%h%x00%an%x00%ae%x00%at%x00%s%x00%P%x00%D%x1e"], in: dir, timeout: 20)?.1 ?? ""
            let list = GitLogParser.parse(out)
            await MainActor.run {
                self.logLoading = false
                if list != self.commits { self.commits = list }
                if let selected = self.selectedCommit, !list.contains(where: { $0.sha == selected }) { self.selectedCommit = nil }
            }
        }
    }

    private func loadCommitFiles() {
        guard let sha = selectedCommit else { commitFiles = []; if case .commit = target { target = nil }; return }
        let dir = directory
        Task.detached(priority: .userInitiated) {
            let out = GitInfoService.run(["-c", "core.quotePath=false", "diff-tree", "--no-commit-id", "-r", "-M", "--root", "--name-status", "-z", sha], in: dir, timeout: 20)?.1 ?? ""
            let files = GitNameStatusParser.parse(z: out)
            await MainActor.run {
                guard self.selectedCommit == sha else { return }
                self.commitFiles = files
                if let first = files.first { self.target = .commit(sha: sha, path: first.path) } else if case .commit = self.target { self.target = nil }
            }
        }
    }

    func revert(_ commit: GitCommit) {
        enqueue("Reverting…") { [self] in
            var args = ["revert", "--no-edit"]
            if commit.isMerge { args += ["-m", "1"] }
            args.append(commit.sha)
            if await git(args, success: "Reverted \(commit.shortSHA)", failureHint: "Resolve the conflicts in the terminal, or run `git revert --abort`.") { loadLog() }
        }
    }

    func reset(to commit: GitCommit, mode: ResetMode) {
        enqueue("Resetting…") { [self] in
            if await git(["reset", "--\(mode.rawValue)", commit.sha], success: "Reset (\(mode.rawValue)) to \(commit.shortSHA)") { loadLog() }
        }
    }

    // MARK: Files

    func loadTree() {
        treeLoading = true
        let dir = directory, isRepo = self.isRepo
        Task.detached(priority: .userInitiated) {
            var paths: [String] = []
            if isRepo, let (s, out) = GitInfoService.run(["-c", "core.quotePath=false", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], in: dir, timeout: 30), s == 0 {
                var seen: Set<String> = []
                for p in out.split(separator: "\0").map(String.init) where seen.insert(p).inserted { paths.append(p) }
            } else {
                paths = Self.enumerate(dir)
            }
            let nodes = FileTree.build(paths: paths)
            await MainActor.run { self.treeLoading = false; if nodes != self.tree { self.tree = nodes } }
        }
    }

    /// Fallback for folders that are not repositories: skips hidden, dependency and build folders.
    nonisolated static func enumerate(_ root: String, cap: Int = 20_000) -> [String] {
        let skip: Set<String> = ["node_modules", ".build", "build", "dist", "target", "vendor", "Pods", "DerivedData", "__pycache__", ".venv", "venv"]
        guard let enumerator = FileManager.default.enumerator(atPath: root) else { return [] }
        var paths: [String] = []
        while let relative = enumerator.nextObject() as? String {
            let name = (relative as NSString).lastPathComponent
            let type = enumerator.fileAttributes?[.type] as? FileAttributeType
            if name.hasPrefix(".") || type == .typeSymbolicLink { enumerator.skipDescendants(); continue }
            if type == .typeDirectory { if skip.contains(name) { enumerator.skipDescendants() }; continue }
            paths.append(relative)
            if paths.count >= cap { break }
        }
        return paths
    }

    private func openFile() {
        fileGeneration += 1
        guard let path = selectedFile else { fileContent = nil; return }
        let generation = fileGeneration
        let url = URL(fileURLWithPath: directory + "/" + path)
        Task.detached(priority: .userInitiated) {
            let body: FileContent.Body
            let size = (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int) ?? 0
            var isDir: ObjCBool = false
            if !FileManager.default.fileExists(atPath: url.path, isDirectory: &isDir) { body = .missing }
            else if isDir.boolValue { await MainActor.run { if generation == self.fileGeneration { self.fileContent = nil } }; return }
            else if size > TextFileInspector.maxBytes { body = .tooLarge(size) }
            else if TextFileInspector.isImage(path) { body = .image(url) }
            else if let data = TextFileInspector.readBounded(url) {
                if TextFileInspector.isBinary(data) { body = .binary(size) }
                else {
                    let text = String(decoding: data, as: UTF8.self)
                    body = TextFileInspector.isMarkdown(path) ? .markdown(text) : .text(text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init))
                }
            } else { body = .missing }
            await MainActor.run { if generation == self.fileGeneration { self.fileContent = FileContent(path: path, body: body) } }
        }
    }

    func revealInFinder(_ path: String) { NSWorkspace.shared.selectFile(directory + "/" + path, inFileViewerRootedAtPath: directory) }
    func openExternally(_ path: String) { NSWorkspace.shared.open(URL(fileURLWithPath: directory + "/" + path)) }
    func absolutePath(_ path: String) -> String { directory + "/" + path }

    /// Colour of a path in the file tree, from the current status.
    func statusCode(for path: String) -> GitFileCode? { status?.files.first { $0.path == path }?.displayCode }
    var changedDirectories: Set<String> { FileTree.changedDirectories(status?.files ?? []) }

    // MARK: Pull request

    func openPullRequest() {
        enqueue("Opening pull request…") { [self] in
            let dir = directory
            let result = await Task.detached { () -> (Bool, String) in
                let p = Process(); p.executableURL = URL(fileURLWithPath: "/bin/zsh"); p.arguments = ["-ilc", "gh pr create --web --fill"]
                p.currentDirectoryURL = URL(fileURLWithPath: dir)
                let pipe = Pipe(); p.standardOutput = pipe; p.standardError = pipe
                do { try p.run() } catch { return (false, "Couldn’t run gh.") }
                p.waitUntilExit()
                return (p.terminationStatus == 0, String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
            }.value
            if result.0 { lastResult = ActionResult(ok: true, text: "Opened the pull request page") }
            else { let text = result.1.isEmpty ? "GitHub CLI not found. Install it (brew install gh) and run gh auth login." : result.1; lastResult = ActionResult(ok: false, text: text); store.error = text }
        }
    }
}

/// FSEvents on a folder, coalesced so a burst of agent edits becomes one callback.
final class DirectoryWatcher: @unchecked Sendable {
    private var stream: FSEventStreamRef?
    private let queue = DispatchQueue(label: "convoy.directory-watcher", qos: .utility)
    private let onChange: @Sendable () -> Void
    private var scheduled: DispatchWorkItem?

    init(path: String, onChange: @escaping @Sendable () -> Void) {
        self.onChange = onChange
        var context = FSEventStreamContext(version: 0, info: Unmanaged.passUnretained(self).toOpaque(), retain: nil, release: nil, copyDescription: nil)
        let callback: FSEventStreamCallback = { _, info, _, _, _, _ in
            guard let info else { return }
            Unmanaged<DirectoryWatcher>.fromOpaque(info).takeUnretainedValue().fire()
        }
        guard let stream = FSEventStreamCreate(nil, callback, &context, [path] as CFArray, FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 0.5,
                                               FSEventStreamCreateFlags(kFSEventStreamCreateFlagNoDefer)) else { return }
        FSEventStreamSetDispatchQueue(stream, queue)
        FSEventStreamStart(stream)
        self.stream = stream
    }

    private func fire() {
        scheduled?.cancel()
        let work = DispatchWorkItem { [onChange] in onChange() }
        scheduled = work
        queue.asyncAfter(deadline: .now() + 0.4, execute: work)
    }

    func stop() {
        guard let stream else { return }
        FSEventStreamStop(stream); FSEventStreamInvalidate(stream); FSEventStreamRelease(stream)
        self.stream = nil
    }

    deinit { stop() }
}
