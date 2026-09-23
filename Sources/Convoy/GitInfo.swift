import Foundation

struct GitInfo: Equatable {
    var branch: String
    var changed: Int
    var ahead: Int
    var behind: Int
    var isWorktree: Bool
    var summary: String {
        var parts: [String] = []
        if changed > 0 { parts.append("\(changed) changed") }
        if ahead > 0 { parts.append("↑\(ahead)") }
        if behind > 0 { parts.append("↓\(behind)") }
        return parts.joined(separator: " · ")
    }
}

/// Reads branch and dirty state for the directories on screen. Runs git off the main thread with
/// optional locks disabled so polling never races an agent's own git commands on index.lock.
@MainActor
final class GitInfoService: ObservableObject {
    @Published private(set) var info: [String: GitInfo] = [:]
    private var inFlight = false
    private var pending: Set<String> = []
    private var timer: Timer?
    var interval: TimeInterval = 10 { didSet { schedule() } }
    var enabled = true

    init() { schedule() }

    private func schedule() {
        timer?.invalidate()
        timer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            Task { @MainActor in self?.flush() }
        }
    }

    func track(_ paths: [String]) {
        for path in paths { pending.insert(path) }
        if info.isEmpty { flush() }
    }

    func refreshNow(_ paths: [String]) { track(paths); flush() }

    private func flush() {
        guard enabled, !inFlight, !pending.isEmpty else { return }
        inFlight = true
        let paths = Array(pending)
        Task.detached(priority: .utility) {
            var result: [String: GitInfo] = [:]
            for path in paths { if let info = Self.read(path) { result[path] = info } }
            await MainActor.run {
                var next = self.info
                for path in paths { next[path] = result[path] }
                if next != self.info { self.info = next }
                self.inFlight = false
            }
        }
    }

    /// `input` is written to git's stdin (for `apply -`); `environment` is merged last (for `GIT_EDITOR=true`).
    /// Output is decoded leniently so a diff with non-UTF-8 bytes still comes back.
    ///
    /// Reads the pipe on the calling thread. Waiting on a DispatchGroup here deadlocks when called from a
    /// detached Task: the cooperative pool is blocked and the global-queue reader never runs, so every call
    /// times out. The timeout is enforced by a small dedicated thread instead of GCD.
    nonisolated static func run(_ arguments: [String], in directory: String, timeout: TimeInterval = 8, input: Data? = nil, environment: [String: String] = [:]) -> (Int32, String)? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        process.arguments = ["git", "-C", directory, "--no-optional-locks"] + arguments
        process.environment = ProcessInfo.processInfo.environment.merging(["GIT_OPTIONAL_LOCKS": "0", "GIT_TERMINAL_PROMPT": "0", "LC_ALL": "C"]) { $1 }.merging(environment) { $1 }
        let pipe = Pipe(); process.standardOutput = pipe; process.standardError = pipe
        let stdin = Pipe(); process.standardInput = stdin
        do { try process.run() } catch { return nil }
        if let input { stdin.fileHandleForWriting.write(input) }
        try? stdin.fileHandleForWriting.close()

        final class Watchdog: @unchecked Sendable { var done = false; var timedOut = false; let condition = NSCondition() }
        let watchdog = Watchdog()
        let deadline = Date(timeIntervalSinceNow: timeout)
        Thread.detachNewThread {
            watchdog.condition.lock()
            while !watchdog.done && watchdog.condition.wait(until: deadline) {}
            if !watchdog.done { watchdog.timedOut = true; if process.isRunning { process.terminate() } }
            watchdog.condition.unlock()
        }
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        watchdog.condition.lock()
        watchdog.done = true; watchdog.condition.signal()
        let timedOut = watchdog.timedOut
        watchdog.condition.unlock()
        if timedOut { return nil }
        return (process.terminationStatus, String(decoding: data, as: UTF8.self))
    }

    nonisolated static func read(_ path: String) -> GitInfo? {
        guard FileManager.default.fileExists(atPath: path + "/.git") || run(["rev-parse", "--is-inside-work-tree"], in: path)?.0 == 0,
              let (status, output) = run(["-c", "core.quotePath=false", "status", "--porcelain=v2", "--branch", "--untracked-files=all"], in: path), status == 0 else { return nil }
        var branch = "", ahead = 0, behind = 0, changed = 0
        for line in output.split(separator: "\n") {
            if line.hasPrefix("# branch.head ") { branch = String(line.dropFirst("# branch.head ".count)) }
            else if line.hasPrefix("# branch.ab ") {
                let parts = line.dropFirst("# branch.ab ".count).split(separator: " ")
                ahead = Int(parts.first?.dropFirst() ?? "") ?? 0; behind = Int(parts.last?.dropFirst() ?? "") ?? 0
            } else if !line.hasPrefix("#") { changed += 1 }
        }
        var isWorktree = false
        var isDir: ObjCBool = false
        if FileManager.default.fileExists(atPath: path + "/.git", isDirectory: &isDir), !isDir.boolValue { isWorktree = true }
        return GitInfo(branch: branch == "(detached)" ? "detached" : branch, changed: changed, ahead: ahead, behind: behind, isWorktree: isWorktree)
    }
}

enum GitWorktree {
    static var root: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/worktrees")
    }

    /// Branch slug in the style of Orca: lowercase, non-alphanumerics collapsed to "-", at most four words.
    static func slug(_ title: String, prefix: String = "") -> String {
        let words = title.lowercased().replacingOccurrences(of: "[^a-z0-9]+", with: "-", options: .regularExpression)
            .split(separator: "-").filter { !$0.isEmpty }.prefix(4)
        let leaf = words.isEmpty ? "session" : words.joined(separator: "-")
        let cleanPrefix = prefix.trimmingCharacters(in: CharacterSet(charactersIn: "/ "))
        return cleanPrefix.isEmpty ? leaf : "\(cleanPrefix)/\(leaf)"
    }

    static func branchExists(_ branch: String, in repo: String) -> Bool {
        GitInfoService.run(["rev-parse", "--verify", "--quiet", "refs/heads/\(branch)"], in: repo)?.0 == 0
    }

    /// Creates `<App Support>/SpecDesk/worktrees/<project>/<branch>` from `base` (default: current HEAD).
    /// `--no-track` avoids inheriting the base's upstream, so status won't claim "behind" before the branch is pushed.
    static func create(repo: String, projectName: String, branch: String, base: String?) throws -> String {
        let safeName = projectName.replacingOccurrences(of: "[^A-Za-z0-9._-]+", with: "-", options: .regularExpression)
        let path = root.appendingPathComponent(safeName).appendingPathComponent(branch.replacingOccurrences(of: "/", with: "--")).path
        if FileManager.default.fileExists(atPath: path) { return path }
        try FileManager.default.createDirectory(at: URL(fileURLWithPath: path).deletingLastPathComponent(), withIntermediateDirectories: true)
        var args = ["worktree", "add"]
        if branchExists(branch, in: repo) { args += [path, branch] }
        else { args += ["--no-track", "-b", branch, path]; if let base, !base.isEmpty { args.append(base) } }
        guard let (status, output) = GitInfoService.run(args, in: repo, timeout: 120) else { throw GitError("git did not finish creating the worktree.") }
        guard status == 0 else { throw GitError(output.trimmingCharacters(in: .whitespacesAndNewlines)) }
        _ = GitInfoService.run(["config", "push.autoSetupRemote", "true"], in: path)
        return path
    }

    /// Copies gitignored paths (env files, node_modules…) from the primary checkout into a new worktree.
    /// APFS clone (`cp -c`) is instant and space-free; falls back to a symlink.
    static func materialize(sharedPaths: [String], from repo: String, into worktree: String) -> [String] {
        var notes: [String] = []
        for raw in sharedPaths.map({ $0.trimmingCharacters(in: .whitespaces) }) where !raw.isEmpty && !raw.hasPrefix("/") && !raw.contains("..") {
            let source = repo + "/" + raw, target = worktree + "/" + raw
            guard FileManager.default.fileExists(atPath: source), !FileManager.default.fileExists(atPath: target) else { continue }
            try? FileManager.default.createDirectory(atPath: (target as NSString).deletingLastPathComponent, withIntermediateDirectories: true)
            let cp = Process(); cp.executableURL = URL(fileURLWithPath: "/bin/cp"); cp.arguments = ["-c", "-R", source, target]
            cp.standardError = Pipe(); cp.standardOutput = Pipe()
            if (try? cp.run()) != nil { cp.waitUntilExit() }
            if cp.terminationStatus != 0 || !FileManager.default.fileExists(atPath: target) {
                try? FileManager.default.removeItem(atPath: target)
                if (try? FileManager.default.createSymbolicLink(atPath: target, withDestinationPath: source)) != nil { notes.append("\(raw) → symlink") }
                else { notes.append("\(raw) → failed") }
            } else { notes.append("\(raw) → cloned") }
        }
        return notes
    }

    static func remove(repo: String, path: String, deleteBranch: String?) throws {
        guard path.hasPrefix(root.path) else { throw GitError("Only worktrees created by SpecDesk are removed automatically.") }
        guard let (status, output) = GitInfoService.run(["worktree", "remove", "--force", path], in: repo, timeout: 60) else { throw GitError("git did not finish removing the worktree.") }
        guard status == 0 else { throw GitError(output.trimmingCharacters(in: .whitespacesAndNewlines)) }
        _ = GitInfoService.run(["worktree", "prune"], in: repo)
        if let deleteBranch { _ = GitInfoService.run(["branch", "-D", deleteBranch], in: repo) }
    }

    /// Current branch of `repo`; "main" when the folder is not a repository (a project group) or git fails.
    static func defaultBase(_ repo: String) -> String {
        guard let (status, out) = GitInfoService.run(["rev-parse", "--abbrev-ref", "HEAD"], in: repo), status == 0 else { return "main" }
        let name = out.trimmingCharacters(in: .whitespacesAndNewlines)
        return name.isEmpty || name.hasPrefix("fatal") ? "main" : name
    }
    static func isRepository(_ path: String) -> Bool {
        guard let (status, out) = GitInfoService.run(["rev-parse", "--is-inside-work-tree"], in: path), status == 0 else { return false }
        return out.trimmingCharacters(in: .whitespacesAndNewlines) == "true"
    }
}

struct GitError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message.isEmpty ? "git failed." : message }
}
