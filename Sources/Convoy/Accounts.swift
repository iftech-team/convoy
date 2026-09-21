import Foundation

/// Managed provider logins. Claude honours CLAUDE_CONFIG_DIR and Codex honours CODEX_HOME, so each
/// managed account is just an isolated home directory; switching rewrites nothing in the real ~/.claude or ~/.codex.
struct ManagedAccount: Identifiable, Equatable {
    let agent: Agent
    let label: String
    var id: String { "\(agent.rawValue)/\(label)" }
    var home: URL { accountsRoot.appendingPathComponent(agent == .claude ? "claude" : "codex").appendingPathComponent(label) }
}

let accountsRoot = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/accounts")

@MainActor
final class Accounts: ObservableObject {
    static var root: URL { accountsRoot }
    @Published private(set) var accounts: [ManagedAccount] = []
    @Published var activeClaude: String? = UserDefaults.standard.string(forKey: "activeClaudeAccount") { didSet { UserDefaults.standard.set(activeClaude, forKey: "activeClaudeAccount") } }
    @Published var activeCodex: String? = UserDefaults.standard.string(forKey: "activeCodexAccount") { didSet { UserDefaults.standard.set(activeCodex, forKey: "activeCodexAccount") } }

    init() { reload() }

    func reload() {
        var found: [ManagedAccount] = []
        for agent in Agent.allCases {
            let dir = accountsRoot.appendingPathComponent(agent == .claude ? "claude" : "codex")
            let names = (try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []
            found += names.filter { !$0.hasPrefix(".") }.sorted().map { ManagedAccount(agent: agent, label: $0) }
        }
        accounts = found
        if let a = activeClaude, !found.contains(where: { $0.agent == .claude && $0.label == a }) { activeClaude = nil }
        if let a = activeCodex, !found.contains(where: { $0.agent == .codex && $0.label == a }) { activeCodex = nil }
    }

    func accounts(for agent: Agent) -> [ManagedAccount] { accounts.filter { $0.agent == agent } }
    func active(for agent: Agent) -> ManagedAccount? {
        let label = agent == .claude ? activeClaude : activeCodex
        return label.flatMap { l in accounts.first { $0.agent == agent && $0.label == l } }
    }
    func setActive(_ agent: Agent, label: String?) { if agent == .claude { activeClaude = label } else { activeCodex = label } }

    /// Creates the isolated home. For Codex, the user's config.toml is mirrored so behaviour matches the terminal.
    func add(_ agent: Agent, label: String) throws -> ManagedAccount {
        let clean = label.trimmingCharacters(in: .whitespaces).replacingOccurrences(of: "[^A-Za-z0-9._ -]", with: "", options: .regularExpression)
        guard !clean.isEmpty else { throw GitError("Give the account a label.") }
        let account = ManagedAccount(agent: agent, label: clean)
        try FileManager.default.createDirectory(at: account.home, withIntermediateDirectories: true)
        if agent == .codex {
            let source = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".codex/config.toml")
            let target = account.home.appendingPathComponent("config.toml")
            if FileManager.default.fileExists(atPath: source.path), !FileManager.default.fileExists(atPath: target.path) { try? FileManager.default.copyItem(at: source, to: target) }
        }
        reload()
        return account
    }

    func remove(_ account: ManagedAccount) throws {
        try FileManager.default.removeItem(at: account.home)
        if active(for: account.agent) == account { setActive(account.agent, label: nil) }
        reload()
    }

    /// Environment for launching an agent under the active managed account (empty = system default login).
    func environment(for agent: Agent) -> [String: String] {
        guard let account = active(for: agent) else { return [:] }
        return agent == .claude ? ["CLAUDE_CONFIG_DIR": account.home.path] : ["CODEX_HOME": account.home.path]
    }

    /// Whether the managed home holds credentials yet.
    func isLoggedIn(_ account: ManagedAccount) -> Bool {
        let file = account.agent == .claude ? ".credentials.json" : "auth.json"
        return FileManager.default.fileExists(atPath: account.home.appendingPathComponent(file).path)
    }
}


/// Which agent CLIs are installed, resolved through the login shell like the terminals do.
@MainActor
final class AgentDetector: ObservableObject {
    struct Info: Equatable { var path: String?; var version: String? }
    @Published private(set) var info: [Agent: Info] = [:]
    @Published private(set) var checked = false

    func detect() {
        Task.detached(priority: .utility) {
            var result: [Agent: Info] = [:]
            for agent in Agent.allCases {
                let bin = agent == .claude ? "claude" : "codex"
                let p = Process(); p.executableURL = URL(fileURLWithPath: "/bin/zsh")
                p.arguments = ["-ilc", "command -v \(bin) && \(bin) --version 2>/dev/null | head -1"]
                var env = ProcessInfo.processInfo.environment
                for key in env.keys where key.hasPrefix("CLAUDE") { env.removeValue(forKey: key) }
                p.environment = env
                let pipe = Pipe(); p.standardOutput = pipe; p.standardError = Pipe()
                var path: String?, version: String?
                if (try? p.run()) != nil {
                    let out = String(data: pipe.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
                    p.waitUntilExit()
                    let lines = out.split(separator: "\n").map(String.init)
                    path = lines.first; version = lines.dropFirst().first
                }
                result[agent] = Info(path: path, version: version)
            }
            await MainActor.run { self.info = result; self.checked = true }
        }
    }
}
