import AppKit
import SwiftUI
@preconcurrency import SwiftTerm

enum SessionCommand {
    static func missingConversation(in text: String) -> Bool {
        text.replacingOccurrences(of: "\0", with: " ").localizedCaseInsensitiveContains("No conversation found with session ID")
    }
    static func quote(_ value: String) -> String {
        "'" + value.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
    }

    /// Claude Code hook events Convoy listens to for working / waiting / done. `PreCompact` is deliberately absent.
    static let hookEvents = ["SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PermissionRequest", "Notification", "Stop", "SessionEnd"]

    static func claudeSettings(statusCommand: String?, hookHelper: String?) -> [String: Any] {
        var settings: [String: Any] = [:]
        if let statusCommand { settings["statusLine"] = ["type": "command", "command": statusCommand] }
        if let hookHelper {
            var hooks: [String: Any] = [:]
            for event in hookEvents {
                var entry: [String: Any] = ["hooks": [["type": "command", "command": "\(quote(hookHelper)) hook \(event)", "timeout": 5]]]
                if ["PreToolUse", "PostToolUse", "PermissionRequest"].contains(event) { entry["matcher"] = "" }
                hooks[event] = [entry]
            }
            settings["hooks"] = hooks
        }
        return settings
    }

    static func script(session: LinkedSession, directory: String, resume: Bool, claudeStatusCommand: String? = nil, hookHelper: String? = nil, setup: String? = nil) -> String {
        var args: [String]
        let yolo = UserDefaults.standard.bool(forKey: session.agent == .claude ? "yoloClaude" : "yoloCodex")
        if session.agent == .claude {
            args = ["claude"]
            if yolo { args.append("--dangerously-skip-permissions") }
            let settings = claudeSettings(statusCommand: claudeStatusCommand, hookHelper: hookHelper)
            if !settings.isEmpty, let data = try? JSONSerialization.data(withJSONObject: settings), let json = String(data: data, encoding: .utf8) {
                args += ["--settings", json]
            }
            if resume {
                args += session.sessionID.isEmpty ? ["--resume"] : ["--resume", session.sessionID.lowercased()]
            } else {
                args += ["--session-id", session.sessionID.lowercased(), "--name", session.title]
            }
        } else {
            args = ["codex"]
            if yolo { args.append("--dangerously-bypass-approvals-and-sandbox") }
            if resume {
                args += ["resume"]
                if !session.sessionID.isEmpty { args.append(session.sessionID) }
            }
            args.append("--no-alt-screen")
        }
        if !resume, let prompt = session.initialPrompt, !prompt.isEmpty { args += ["--", prompt] }
        var prefix = "cd -- \(quote(directory))"
        if let setup = setup?.trimmingCharacters(in: .whitespacesAndNewlines), !setup.isEmpty {
            // Worktree setup hooks run once, visibly, before the agent starts; a failure stops the launch so it is noticed.
            prefix += " && printf '\\n\\033[2m── SpecDesk worktree setup ──\\033[0m\\n' && { " + setup + "\n} && printf '\\033[2m── setup done ──\\033[0m\\n'"
        }
        return prefix + " && exec " + args.map(quote).joined(separator: " ")
    }

    /// Claude writes `~/.claude/projects/<encoded cwd>/<session id>.jsonl` after the first message.
    /// Nothing is written for a session that was opened and closed without a message.
    static func claudeTranscriptExists(sessionID: String, directory: String) -> Bool {
        let id = sessionID.lowercased()
        guard !id.isEmpty else { return false }
        let root = (ProcessInfo.processInfo.environment["CLAUDE_CONFIG_DIR"].map { URL(fileURLWithPath: $0) }
                    ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude")).appendingPathComponent("projects")
        let encoded = directory.replacingOccurrences(of: "[^A-Za-z0-9]", with: "-", options: .regularExpression)
        if FileManager.default.fileExists(atPath: root.appendingPathComponent(encoded).appendingPathComponent("\(id).jsonl").path) { return true }
        // Encoding rules may differ between versions; fall back to a shallow search.
        let dirs = (try? FileManager.default.contentsOfDirectory(atPath: root.path)) ?? []
        return dirs.contains { FileManager.default.fileExists(atPath: root.appendingPathComponent($0).appendingPathComponent("\(id).jsonl").path) }
    }

    static func codexID(in text: String) -> String? {
        let pattern = #"codex resume ([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})"#
        guard let range = text.range(of: pattern, options: .regularExpression) else { return nil }
        return String(text[range].dropFirst("codex resume ".count))
    }
}

@MainActor
final class CapturingTerminal: LocalProcessTerminalView {
    var onOutput: ((String) -> Void)?
    override func dataReceived(slice: ArraySlice<UInt8>) {
        super.dataReceived(slice: slice)
        onOutput?(String(decoding: slice, as: UTF8.self))
    }
}

@MainActor
final class TerminalHandle: NSObject, ObservableObject, @preconcurrency LocalProcessTerminalViewDelegate {
    let view: CapturingTerminal
    let projectID: UUID
    let sessionID: UUID
    @Published var running = false
    @Published var exitCode: Int32?
    private var tail = ""
    private var lastSnapshot = Date.distantPast
    var onProviderID: ((String) -> Void)?
    var onPullRequest: ((String) -> Void)?
    private var seenPR: String?
    private let snapshotURL: URL
    var onChange: (() -> Void)?

    init(session: LinkedSession, project: Project, snapshotURL: URL) {
        self.sessionID = session.id
        self.projectID = project.id
        self.snapshotURL = snapshotURL
        let defaults = UserDefaults.standard
        let scrollback = max(1_000, min(50_000, defaults.object(forKey: "terminalScrollback") as? Int ?? 10_000))
        let fontSize = max(10, min(20, defaults.object(forKey: "terminalFontSize") as? Double ?? 13))
        view = CapturingTerminal(frame: NSRect(x: 0, y: 0, width: 1000, height: 650), options: TerminalOptions(scrollback: scrollback))
        super.init()
        view.font = NSFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
        view.nativeBackgroundColor = AppTheme.terminalBackground
        view.nativeForegroundColor = AppTheme.terminalForeground
        view.processDelegate = self
        view.onOutput = { [weak self] text in
            guard let self else { return }
            self.tail = String((self.tail + text).suffix(6000))
            if session.agent == .codex, let id = SessionCommand.codexID(in: self.tail) {
                self.onProviderID?(id); self.onProviderID = nil
            }
            if let range = self.tail.range(of: #"https://(github\.com|gitlab\.com)/[^\s'"`)]+/(pull|merge_requests)/\d+"#, options: .regularExpression) {
                let url = String(self.tail[range])
                if url != self.seenPR { self.seenPR = url; self.onPullRequest?(url) }
            }
            if Date().timeIntervalSince(self.lastSnapshot) > 2 { self.saveSnapshot() }
        }
    }

    func start(script: String, directory: String, extraEnvironment: [String: String] = [:]) {
        running = true; exitCode = nil
        var environment = ProcessInfo.processInfo.environment.merging(extraEnvironment) { $1 }
        environment["TERM"] = "xterm-256color"
        environment["COLORTERM"] = "truecolor"
        // This is an independent interactive terminal, not a tool subprocess. If SpecDesk itself
        // was launched from inside an agent, inherited markers would make Claude treat these
        // sessions as children and skip saving transcripts, which breaks Resume.
        for key in environment.keys where key.hasPrefix("CLAUDE") && key != "CLAUDE_CONFIG_DIR" { environment.removeValue(forKey: key) }
        environment.removeValue(forKey: "CODEX_THREAD_ID")
        environment["CLAUDE_CODE_FORCE_SESSION_PERSISTENCE"] = "1"
        view.startProcess(executable: "/bin/zsh", args: ["-ilc", script], environment: environment.map { "\($0.key)=\($0.value)" }, currentDirectory: directory)
        onChange?()
    }

    func stop() {
        saveSnapshot()
        view.terminate()
        running = false
        onChange?()
    }

    var text: String { terminalSnapshot(view.getTerminal()) }

    func saveSnapshot() {
        lastSnapshot = Date()
        do {
            try FileManager.default.createDirectory(at: snapshotURL.deletingLastPathComponent(), withIntermediateDirectories: true)
            try String(text.suffix(250_000)).write(to: snapshotURL, atomically: true, encoding: .utf8)
        } catch {
            // The UI exposes live terminal output independently of snapshot persistence.
            snapshotError = error.localizedDescription
        }
    }
    @Published var snapshotError: String?

    func sizeChanged(source: LocalProcessTerminalView, newCols: Int, newRows: Int) {}
    func setTerminalTitle(source: LocalProcessTerminalView, title: String) {}
    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
    func processTerminated(source: TerminalView, exitCode: Int32?) {
        self.exitCode = exitCode
        running = false
        saveSnapshot()
        onChange?()
    }
}

@MainActor
final class TerminalManager: ObservableObject {
    @Published private(set) var handles: [UUID: TerminalHandle] = [:]
    /// Open tabs in order (first opened first). Persisted across launches; a tab whose
    /// process is not running shows the saved output with Resume.
    @Published private(set) var openOrder: [UUID] = (UserDefaults.standard.stringArray(forKey: "openTerminalTabs") ?? []).compactMap(UUID.init) {
        didSet { UserDefaults.standard.set(openOrder.map(\.uuidString), forKey: "openTerminalTabs") }
    }
    func openTab(_ sessionID: UUID) { if !openOrder.contains(sessionID) { openOrder.append(sessionID) } }
    /// Recently closed tabs, newest last (⇧⌘T reopens).
    private(set) var closedTabs: [UUID] = []
    func popClosedTab() -> UUID? { closedTabs.popLast() }
    private var snapshotRoot: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/TerminalHistory")
    }

    var accounts: Accounts?

    func start(_ session: LinkedSession, project: Project, directory: String? = nil, resume: Bool, setup: String? = nil, onProviderID: @escaping (String) -> Void) {
        guard handles[session.id]?.running != true else { return }
        let directory = directory ?? session.workingDirectory ?? project.path
        let handle = TerminalHandle(session: session, project: project, snapshotURL: snapshotRoot.appendingPathComponent("\(session.id).txt"))
        handle.onProviderID = onProviderID
        handle.onChange = { [weak self] in self?.objectWillChange.send() }
        handles[session.id] = handle
        openTab(session.id)
        closedTabs.removeAll { $0 == session.id }
        var statusCommand: String?
        var hookHelper: String?
        if session.agent == .claude,
           let helper = Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("ConvoyStatus"),
           FileManager.default.isExecutableFile(atPath: helper.path) {
            if UserDefaults.standard.bool(forKey: "claudeLimitsIntegration") {
                statusCommand = SessionCommand.quote(helper.path) + " " + SessionCommand.quote(session.title)
            }
            if UserDefaults.standard.object(forKey: "agentStatusHooks") as? Bool ?? true { hookHelper = helper.path }
        }
        var extra = accounts?.environment(for: session.agent) ?? [:]
        if session.agent == .claude, let dir = extra["CLAUDE_CONFIG_DIR"] { extra["CLAUDE_CONFIG_DIR"] = dir }
        handle.start(script: SessionCommand.script(session: session, directory: directory, resume: resume, claudeStatusCommand: statusCommand, hookHelper: hookHelper, setup: setup), directory: directory, extraEnvironment: extra)
    }

    /// Opens a plain terminal running the provider's login flow inside a managed account home.
    func startLogin(for account: ManagedAccount, project: Project, session: LinkedSession) {
        let handle = TerminalHandle(session: session, project: project, snapshotURL: snapshotRoot.appendingPathComponent("\(session.id).txt"))
        handle.onChange = { [weak self] in self?.objectWillChange.send() }
        handles[session.id] = handle
        openTab(session.id)
        let env = account.agent == .claude ? ["CLAUDE_CONFIG_DIR": account.home.path] : ["CODEX_HOME": account.home.path]
        let command = account.agent == .claude ? "claude /login" : "codex login"
        handle.start(script: "cd -- \(SessionCommand.quote(project.path)) && echo 'Logging in to \(account.agent.rawValue) account \"\(account.label)\"…' && \(command)", directory: project.path, extraEnvironment: env)
    }

    func hasRunningSessions(in projectID: UUID) -> Bool {
        handles.values.contains { $0.projectID == projectID && $0.running }
    }

    var runningCount: Int { handles.values.filter(\.running).count }

    func snapshot(for session: LinkedSession) -> String {
        if let handle = handles[session.id] { return handle.text }
        return ((try? String(contentsOf: snapshotRoot.appendingPathComponent("\(session.id).txt"), encoding: .utf8)) ?? "")
            .replacingOccurrences(of: "\0", with: " ")
    }

    func stopAll() { for handle in handles.values where handle.running { handle.stop() } }

    /// Closes a terminal tab. A running agent is stopped first; its snapshot is kept on disk.
    func close(_ sessionID: UUID) {
        if let handle = handles[sessionID] {
            if handle.running { handle.stop() } else { handle.saveSnapshot() }
        }
        handles[sessionID] = nil
        if openOrder.contains(sessionID) { closedTabs.removeAll { $0 == sessionID }; closedTabs.append(sessionID) }
        openOrder.removeAll { $0 == sessionID }
    }
}

func terminalSnapshot(_ terminal: Terminal) -> String {
    var lines: [String] = []
    var row = terminal.buffer.totalLinesTrimmed
    while let line = terminal.getScrollInvariantLine(row: row) {
        lines.append(line.translateToString(trimRight: true, skipNullCellsFollowingWide: true) { cell in
            let character = terminal.getCharacter(for: cell)
            return character == "\0" ? " " : character
        })
        row += 1
    }
    return lines.joined(separator: "\n")
}

struct EmbeddedTerminal: NSViewRepresentable {
    let handle: TerminalHandle
    func makeNSView(context: Context) -> NSView {
        let container = NSView()
        install(in: container)
        return container
    }
    func updateNSView(_ container: NSView, context: Context) {
        if handle.view.superview !== container { install(in: container) }
    }
    private func install(in container: NSView) {
        container.subviews.forEach { $0.removeFromSuperview() }
        let terminal = handle.view
        terminal.removeFromSuperview()
        terminal.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(terminal)
        NSLayoutConstraint.activate([
            terminal.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            terminal.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            terminal.topAnchor.constraint(equalTo: container.topAnchor),
            terminal.bottomAnchor.constraint(equalTo: container.bottomAnchor)
        ])
        DispatchQueue.main.async { terminal.window?.makeFirstResponder(terminal) }
    }
}
