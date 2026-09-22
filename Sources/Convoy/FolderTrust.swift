import Foundation

/// Pre-approves a working folder for the agent CLIs so a session opened by Convoy never stops on
/// "Do you trust the files in this folder?". Choosing the project in Convoy is that consent; worktrees
/// are new paths every time and would otherwise ask again on each task.
enum FolderTrust {
    /// Claude Code keeps per-project state in `<config dir>/.claude.json` (default `~/.claude.json`).
    static func approveClaude(directory: String, configDir: String?) {
        let file = configDir.map { URL(fileURLWithPath: $0).appendingPathComponent(".claude.json") }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude.json")
        var root: [String: Any] = [:]
        if let data = try? Data(contentsOf: file), let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] { root = parsed }
        else if FileManager.default.fileExists(atPath: file.path) { return }   // unreadable: leave it alone
        var projects = root["projects"] as? [String: Any] ?? [:]
        var entry = projects[directory] as? [String: Any] ?? [:]
        if entry["hasTrustDialogAccepted"] as? Bool == true { return }
        entry["hasTrustDialogAccepted"] = true
        if entry["allowedTools"] == nil { entry["allowedTools"] = [] }
        projects[directory] = entry
        root["projects"] = projects
        guard let data = try? JSONSerialization.data(withJSONObject: root, options: [.prettyPrinted, .sortedKeys]) else { return }
        try? FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: file, options: .atomic)
    }

    /// Codex keeps trust in `<CODEX_HOME>/config.toml` as `[projects."<path>"] trust_level = "trusted"`.
    static func approveCodex(directory: String, home: String?) {
        let dir = home.map { URL(fileURLWithPath: $0) } ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".codex")
        let file = dir.appendingPathComponent("config.toml")
        var text = (try? String(contentsOf: file, encoding: .utf8)) ?? ""
        let header = "[projects.\"\(directory)\"]"
        if text.contains(header) { return }
        if !text.isEmpty && !text.hasSuffix("\n") { text += "\n" }
        text += "\n\(header)\ntrust_level = \"trusted\"\n"
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? text.write(to: file, atomically: true, encoding: .utf8)
    }

    static func approve(agent: Agent, directory: String, home: String?) {
        switch agent {
        case .claude: approveClaude(directory: directory, configDir: home)
        case .codex: approveCodex(directory: directory, home: home)
        }
    }
}
