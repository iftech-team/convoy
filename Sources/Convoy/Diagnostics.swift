import AppKit
import SwiftUI

/// One probe of the machine or the workspace, with a hint on how to fix it.
struct SetupCheck: Identifiable, Equatable {
    enum Status: Equatable { case ok, warning, failed }
    let id: String
    let title: String
    let detail: String
    let status: Status
    var fix: String? = nil
}

/// Runs every probe through the login shell, the way terminals do, so PATH problems show up here
/// before a session fails at Start.
@MainActor
final class SetupDiagnostics: ObservableObject {
    @Published private(set) var checks: [SetupCheck] = []
    @Published private(set) var running = false
    @Published private(set) var lastRun: Date?

    var problems: [SetupCheck] { checks.filter { $0.status != .ok } }
    var failures: [SetupCheck] { checks.filter { $0.status == .failed } }
    func check(_ id: String) -> SetupCheck? { checks.first { $0.id == id } }
    /// Is this agent's CLI usable? nil until the first run completes.
    func agentAvailable(_ agent: Agent) -> Bool? {
        guard lastRun != nil else { return nil }
        return check(agent == .claude ? "cli.claude" : "cli.codex")?.status != .failed
    }

    nonisolated static func shell(_ command: String, timeout: TimeInterval = 15) -> (Int32, String) {
        let p = Process(); p.executableURL = URL(fileURLWithPath: "/bin/zsh")
        p.arguments = ["-ilc", command]
        var env = ProcessInfo.processInfo.environment
        for key in env.keys where key.hasPrefix("CLAUDE") { env.removeValue(forKey: key) }
        env["GIT_TERMINAL_PROMPT"] = "0"; env["GH_PROMPT_DISABLED"] = "1"
        p.environment = env
        let pipe = Pipe(); p.standardOutput = pipe; p.standardError = pipe
        do { try p.run() } catch { return (127, error.localizedDescription) }
        let group = DispatchGroup(); group.enter()
        DispatchQueue.global().async { p.waitUntilExit(); group.leave() }
        if group.wait(timeout: .now() + timeout) == .timedOut { p.terminate(); return (124, "timed out") }
        let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        return (p.terminationStatus, out.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    func run(store: Store) {
        guard !running else { return }
        running = true
        let projects = store.workspace.projects
        let activeClaude = store.accounts.active(for: .claude), activeCodex = store.accounts.active(for: .codex)
        let claudeLoggedIn = activeClaude.map { store.accounts.isLoggedIn($0) }
        let codexLoggedIn = activeCodex.map { store.accounts.isLoggedIn($0) }
        let hooksOn = UserDefaults.standard.object(forKey: "agentStatusHooks") as? Bool ?? true
        let trustOn = UserDefaults.standard.object(forKey: "autoTrustFolders") as? Bool ?? true
        let helper = Bundle.main.executableURL?.deletingLastPathComponent().appendingPathComponent("ConvoyStatus").path
        Task.detached(priority: .userInitiated) {
            var out: [SetupCheck] = []
            func tool(_ id: String, _ bin: String, title: String, required: Bool, hint: String) {
                let (s, text) = Self.shell("command -v \(bin) >/dev/null 2>&1 && \(bin) --version 2>/dev/null | head -1")
                let version = text.split(separator: "\n").first.map(String.init) ?? ""
                if s == 0, !version.isEmpty { out.append(SetupCheck(id: id, title: title, detail: version, status: .ok)) }
                else if s == 0 { out.append(SetupCheck(id: id, title: title, detail: "Found in PATH", status: .ok)) }
                else { out.append(SetupCheck(id: id, title: title, detail: "Not found in the login shell's PATH", status: required ? .failed : .warning, fix: hint)) }
            }
            tool("cli.claude", "claude", title: "Claude Code CLI", required: false, hint: "Install with `npm install -g @anthropic-ai/claude-code` or `brew install claude-code`, then reopen Convoy.")
            tool("cli.codex", "codex", title: "Codex CLI", required: false, hint: "Install with `npm install -g @openai/codex`, then reopen Convoy.")
            if out.filter({ $0.id.hasPrefix("cli.") }).allSatisfy({ $0.status != .ok }) {
                out.append(SetupCheck(id: "cli.any", title: "At least one agent", detail: "Neither claude nor codex is installed; sessions cannot start.", status: .failed, fix: "Install one of the CLIs above."))
            }
            tool("git", "git", title: "git", required: true, hint: "Install the Xcode Command Line Tools: `xcode-select --install`.")
            do {
                let (s, text) = Self.shell("command -v gh >/dev/null 2>&1 && gh auth status 2>&1 | head -3")
                if s != 0 { out.append(SetupCheck(id: "gh", title: "GitHub CLI (gh)", detail: "Not installed — tasks that open pull requests will fail at the last step.", status: .warning, fix: "`brew install gh` then `gh auth login`.")) }
                else if text.contains("Logged in") { out.append(SetupCheck(id: "gh", title: "GitHub CLI (gh)", detail: text.split(separator: "\n").first(where: { $0.contains("Logged in") }).map { $0.trimmingCharacters(in: .whitespaces) } ?? "Logged in", status: .ok)) }
                else { out.append(SetupCheck(id: "gh", title: "GitHub CLI (gh)", detail: "Installed but not logged in.", status: .warning, fix: "Run `gh auth login` in a terminal.")) }
            }
            // Logins. Claude keeps its token in the keychain; Codex in auth.json.
            let home = FileManager.default.homeDirectoryForCurrentUser
            if let activeClaude {
                out.append(SetupCheck(id: "login.claude", title: "Claude login (account \(activeClaude.label))", detail: claudeLoggedIn == true ? "Credentials present" : "No credentials in this managed account yet", status: claudeLoggedIn == true ? .ok : .warning, fix: "Settings → Accounts → open a login terminal for this account."))
            } else {
                let (s, _) = Self.shell("security find-generic-password -s 'Claude Code-credentials' >/dev/null 2>&1")
                let fileLogin = FileManager.default.fileExists(atPath: home.appendingPathComponent(".claude/.credentials.json").path)
                out.append(SetupCheck(id: "login.claude", title: "Claude login", detail: s == 0 || fileLogin ? "Logged in" : "No saved login found", status: s == 0 || fileLogin ? .ok : .warning, fix: "Run `claude` once in a terminal and complete the login."))
            }
            if let activeCodex {
                out.append(SetupCheck(id: "login.codex", title: "Codex login (account \(activeCodex.label))", detail: codexLoggedIn == true ? "Credentials present" : "No credentials in this managed account yet", status: codexLoggedIn == true ? .ok : .warning, fix: "Settings → Accounts → open a login terminal for this account."))
            } else {
                let codexHome = ProcessInfo.processInfo.environment["CODEX_HOME"].map { URL(fileURLWithPath: $0) } ?? home.appendingPathComponent(".codex")
                let ok = FileManager.default.fileExists(atPath: codexHome.appendingPathComponent("auth.json").path)
                out.append(SetupCheck(id: "login.codex", title: "Codex login", detail: ok ? "Logged in" : "No auth.json found", status: ok ? .ok : .warning, fix: "Run `codex login` in a terminal."))
            }
            // Convoy's own pieces.
            if let helper, FileManager.default.isExecutableFile(atPath: helper) {
                out.append(SetupCheck(id: "hooks", title: "Agent status hooks", detail: hooksOn ? "ConvoyStatus helper present; hooks on" : "Helper present but hooks are turned off", status: hooksOn ? .ok : .warning, fix: "Settings → Agents → Agent status hooks."))
            } else {
                out.append(SetupCheck(id: "hooks", title: "Agent status hooks", detail: "ConvoyStatus helper is missing from the app bundle; status stays unknown.", status: .warning, fix: "Reinstall Convoy from the release zip."))
            }
            out.append(SetupCheck(id: "trust", title: "Folder trust", detail: trustOn ? "Session folders are trusted automatically" : "Off: agents will ask to trust each new folder", status: trustOn ? .ok : .warning, fix: "Settings → Agents → Trust project folders automatically."))
            let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy")
            let writable = (try? FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)) != nil && FileManager.default.isWritableFile(atPath: support.path)
            out.append(SetupCheck(id: "support", title: "Application Support folder", detail: writable ? support.path : "Not writable: \(support.path)", status: writable ? .ok : .failed, fix: "Fix permissions on the folder."))
            // Workspace.
            let missing = projects.filter { !FileManager.default.fileExists(atPath: $0.path) }
            if projects.isEmpty {
                out.append(SetupCheck(id: "projects", title: "Projects", detail: "No projects yet.", status: .warning, fix: "⌘O to add a repository or a folder of projects."))
            } else if missing.isEmpty {
                out.append(SetupCheck(id: "projects", title: "Projects", detail: "\(projects.count) folder\(projects.count == 1 ? "" : "s") present", status: .ok))
            } else {
                out.append(SetupCheck(id: "projects", title: "Projects", detail: "Missing on disk: " + missing.map(\.name).joined(separator: ", "), status: .failed, fix: "Reconnect or remove them in the sidebar."))
            }
            let noGit = projects.filter { !$0.isGroup && FileManager.default.fileExists(atPath: $0.path) && !FileManager.default.fileExists(atPath: $0.path + "/.git") }
            if !noGit.isEmpty {
                out.append(SetupCheck(id: "projects.git", title: "Projects without git", detail: noGit.map(\.name).joined(separator: ", "), status: .warning, fix: "Worktrees, tasks that push, and the Files & Changes panel need a repository. `git init` if you want them."))
            }
            await MainActor.run { self.checks = out; self.running = false; self.lastRun = Date() }
        }
    }
}

struct SetupSettings: View {
    @EnvironmentObject var store: Store
    @ObservedObject var diagnostics: SetupDiagnostics

    var body: some View {
        SettingsGroup(title: "Setup check", footer: "Runs at launch and whenever you open this page. Everything is probed through your login shell, exactly as sessions are started.") {
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    if diagnostics.running { ProgressView().controlSize(.small); Text("Checking…").font(.system(size: 12)).foregroundStyle(.secondary) }
                    else if let last = diagnostics.lastRun {
                        let problems = diagnostics.problems
                        Image(systemName: problems.isEmpty ? "checkmark.circle.fill" : (diagnostics.failures.isEmpty ? "exclamationmark.triangle.fill" : "xmark.octagon.fill"))
                            .foregroundStyle(problems.isEmpty ? .green : (diagnostics.failures.isEmpty ? .orange : .red))
                        Text(problems.isEmpty ? "Everything looks good" : "\(problems.count) thing\(problems.count == 1 ? "" : "s") to look at").font(.system(size: 12.5, weight: .medium))
                        Text("· \(last.formatted(date: .omitted, time: .shortened))").font(.system(size: 11)).foregroundStyle(.tertiary)
                    }
                    Spacer()
                    Button { diagnostics.run(store: store) } label: { Label("Run again", systemImage: "arrow.clockwise") }.controlSize(.small).disabled(diagnostics.running)
                }.padding(14)
                Divider()
                ForEach(diagnostics.checks) { check in
                    HStack(alignment: .top, spacing: 12) {
                        Image(systemName: check.status == .ok ? "checkmark.circle.fill" : (check.status == .warning ? "exclamationmark.triangle.fill" : "xmark.octagon.fill"))
                            .foregroundStyle(check.status == .ok ? .green : (check.status == .warning ? .orange : .red)).frame(width: 18).padding(.top, 1)
                        VStack(alignment: .leading, spacing: 3) {
                            Text(check.title).font(.system(size: 12.5, weight: .medium))
                            Text(check.detail).font(.system(size: 11, design: check.status == .ok ? .monospaced : .default)).foregroundStyle(.secondary).lineLimit(2).truncationMode(.middle)
                            if check.status != .ok, let fix = check.fix { Text(fix).font(.system(size: 11)).foregroundStyle(.primary.opacity(0.85)).textSelection(.enabled) }
                        }
                        Spacer()
                    }.padding(.horizontal, 14).padding(.vertical, 9)
                    Divider().padding(.leading, 44)
                }
            }
        }
    }
}

/// Inline warning for the new-session sheet when the chosen agent cannot start.
struct AgentAvailabilityNote: View {
    @EnvironmentObject var store: Store
    let agent: Agent
    var body: some View {
        if store.diagnostics.agentAvailable(agent) == false {
            HStack(spacing: 8) {
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.orange)
                Text("\(agent.rawValue) was not found in your PATH, so this session will fail to start.").font(.system(size: 11.5))
                Spacer()
                Button("Setup check") { store.showSettings = true; store.settingsSection = "Setup" }.controlSize(.small)
            }.padding(10).background(Color.orange.opacity(0.1), in: RoundedRectangle(cornerRadius: 8))
        }
    }
}
