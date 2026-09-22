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
            if let model = session.model, !model.isEmpty { args += ["--model", model] }
            if resume {
                args += session.sessionID.isEmpty ? ["--resume"] : ["--resume", session.sessionID.lowercased()]
            } else {
                args += ["--session-id", session.sessionID.lowercased(), "--name", session.title]
            }
        } else {
            args = ["codex"]
            if yolo { args.append("--dangerously-bypass-approvals-and-sandbox") }
            if let model = session.model, !model.isEmpty { args += ["--model", model] }
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

    // SwiftTerm does not accept drops. Dragging a file (screenshot, photo, log) from Finder
    // onto the terminal types its shell-quoted path, the way Terminal.app and iTerm do, so the
    // agent can read it.
    override init(frame: CGRect, font: NSFont? = nil, options: TerminalOptions) {
        super.init(frame: frame, font: font, options: options)
        registerForDraggedTypes([.fileURL])
    }
    required init?(coder: NSCoder) {
        super.init(coder: coder)
        registerForDraggedTypes([.fileURL])
    }

    private func droppedURLs(_ sender: NSDraggingInfo) -> [URL] {
        (sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]) ?? []
    }
    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        droppedURLs(sender).isEmpty ? [] : .copy
    }
    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        droppedURLs(sender).isEmpty ? [] : .copy
    }
    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool { !droppedURLs(sender).isEmpty }
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        let urls = droppedURLs(sender)
        guard !urls.isEmpty else { return false }
        send(txt: Self.droppedText(for: urls))
        window?.makeFirstResponder(self)
        return true
    }

    /// Paths joined by spaces, each quoted for the shell, with a trailing space so the user can keep typing.
    static func droppedText(for urls: [URL]) -> String {
        urls.map { shellQuote($0.path) }.joined(separator: " ") + " "
    }
    static func shellQuote(_ path: String) -> String {
        let safe = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "/._-+=:@%,~"))
        if !path.isEmpty, path.unicodeScalars.allSatisfy(safe.contains) { return path }
        return "'" + path.replacingOccurrences(of: "'", with: "'\\''") + "'"
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
    /// The SwiftUI host currently allowed to display `view`. See TerminalContainer.
    weak var owner: TerminalContainer?

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
        guard running else { return }
        saveSnapshot()
        let pid = view.process.shellPid
        // Interactive shells can ignore SIGTERM. Signal the PTY's own process
        // group, then reap the child after SwiftTerm cancels its exit watcher.
        if pid > 0 { kill(getpgid(pid) == pid ? -pid : pid, SIGHUP) }
        view.terminate()
        if pid > 0 {
            Task.detached {
                var status: Int32 = 0
                for _ in 0..<40 {
                    let result = waitpid(pid, &status, WNOHANG)
                    if result != 0 { return }
                    try? await Task.sleep(for: .milliseconds(50))
                }
                // waitpid == 0 means this is still our unreaped child, so its
                // PID cannot have been reused by an unrelated process.
                if waitpid(pid, &status, WNOHANG) == 0 {
                    kill(getpgid(pid) == pid ? -pid : pid, SIGKILL)
                    _ = waitpid(pid, &status, 0)
                }
            }
        }
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
        let key = session.agent == .claude ? "CLAUDE_CONFIG_DIR" : "CODEX_HOME"
        if let home = session.agentHome {
            // Only override when the session really uses a managed or custom home. Setting CLAUDE_CONFIG_DIR
            // to the default ~/.claude makes Claude look for a different keychain entry and demand a new login.
            let standard = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(session.agent == .claude ? ".claude" : ".codex").path
            if home == standard { extra.removeValue(forKey: key) } else { extra[key] = home }
        }
        if UserDefaults.standard.object(forKey: "autoTrustFolders") as? Bool ?? true {
            FolderTrust.approve(agent: session.agent, directory: directory, home: extra[key])
        }
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
        let continues = terminal.getScrollInvariantLine(row: row + 1)?.isWrapped == true
        let text = line.translateToString(trimRight: !continues, skipNullCellsFollowingWide: true) { cell in
            let character = terminal.getCharacter(for: cell)
            return character == "\0" ? " " : character
        }
        if line.isWrapped && !lines.isEmpty { lines[lines.count - 1] += text }
        else { lines.append(text) }
        row += 1
    }
    return lines.joined(separator: "\n")
}

/// Container that owns the terminal view. SwiftUI may briefly keep an old host alive while creating a
/// new one for the same handle (for example when the pane layout changes); only the newest container
/// may claim the view, so a dying host never steals it back and leaves the pane blank.
final class TerminalContainer: NSView {
    weak var handle: TerminalHandle?
    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window != nil { claimIfOwner() }
    }
    func claimIfOwner() {
        guard let handle, handle.owner === self else { return }
        let terminal = handle.view
        if terminal.superview !== self {
            terminal.removeFromSuperview()
            terminal.translatesAutoresizingMaskIntoConstraints = false
            addSubview(terminal)
            NSLayoutConstraint.activate([
                terminal.leadingAnchor.constraint(equalTo: leadingAnchor),
                terminal.trailingAnchor.constraint(equalTo: trailingAnchor),
                terminal.topAnchor.constraint(equalTo: topAnchor),
                terminal.bottomAnchor.constraint(equalTo: bottomAnchor)
            ])
        }
        if window != nil { DispatchQueue.main.async { [weak self] in self?.window?.makeFirstResponder(terminal) } }
    }
}

struct EmbeddedTerminal: NSViewRepresentable {
    let handle: TerminalHandle
    func makeNSView(context: Context) -> TerminalContainer {
        let container = TerminalContainer()
        container.handle = handle
        handle.owner = container
        container.claimIfOwner()
        return container
    }
    func updateNSView(_ container: TerminalContainer, context: Context) {
        if handle.owner == nil { handle.owner = container }
        container.claimIfOwner()
    }
}
