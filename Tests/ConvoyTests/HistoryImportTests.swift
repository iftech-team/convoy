import Foundation
import Testing
@testable import Convoy

/// Fake home with `.claude/projects` and `.codex/sessions` trees.
private struct Fixture {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("history-\(UUID().uuidString)")
    var home: String { root.appendingPathComponent("home").path }
    var claude: URL { root.appendingPathComponent("home/.claude/projects") }
    var codex: URL { root.appendingPathComponent("home/.codex/sessions") }

    func mkdir(_ path: String) throws -> String {
        try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
        return path
    }

    /// Writes a Claude transcript filed under `folder`'s slug (defaults to the cwd's).
    @discardableResult
    func claudeFile(id: String = UUID().uuidString.lowercased(), cwd: String, title: String = "Do the thing", tools: [[String: Any]] = [], folder: String? = nil) throws -> URL {
        let dir = claude.appendingPathComponent(HistoryImport.slug(folder ?? cwd))
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var lines: [[String: Any]] = [["type": "user", "cwd": cwd, "message": ["role": "user", "content": title]]]
        for input in tools {
            lines.append(["type": "assistant", "cwd": cwd, "message": ["role": "assistant", "content": [["type": "tool_use", "id": "t", "name": "Bash", "input": input]]]])
        }
        let text = try lines.map { String(data: try JSONSerialization.data(withJSONObject: $0), encoding: .utf8)! }.joined(separator: "\n")
        let file = dir.appendingPathComponent("\(id).jsonl")
        try text.write(to: file, atomically: true, encoding: .utf8)
        return file
    }

    @discardableResult
    func codexFile(id: String, cwd: String, meta extra: [String: Any] = [:], calls: [[String: Any]] = []) throws -> URL {
        let dir = codex.appendingPathComponent("2026/10/01")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var payload: [String: Any] = ["id": id, "cwd": cwd]
        payload.merge(extra) { $1 }
        var lines: [[String: Any]] = [["type": "session_meta", "payload": payload],
                                      ["type": "event_msg", "payload": ["type": "user_message", "message": "Codex work"]]]
        for call in calls { lines.append(["type": "response_item", "payload": call]) }
        let text = try lines.map { String(data: try JSONSerialization.data(withJSONObject: $0), encoding: .utf8)! }.joined(separator: "\n")
        let file = dir.appendingPathComponent("rollout-2026-10-01T10-00-00-\(id).jsonl")
        try text.write(to: file, atomically: true, encoding: .utf8)
        return file
    }

    func analyze(_ projects: [Project]) -> [HistoryImport.Candidate] {
        HistoryImport.analyze(projects: projects, claudeRoot: claude, codexRoot: codex, home: home)
    }
}

@Test func slugReplacesEveryNonAlphanumeric() {
    #expect(HistoryImport.slug("/Users/me/my.app_v2") == "-Users-me-my-app-v2")
}

@Test func normalizeExpandsTildeAndResolvesDots() {
    #expect(HistoryImport.normalize("~/a/./b/../c/", home: "/h") == "/h/a/c")
    #expect(HistoryImport.normalize("src/x.swift", base: "/h/p", home: "/h") == "/h/p/src/x.swift")
    #expect(HistoryImport.normalize("relative", home: "/h") == nil)
    #expect(HistoryImport.contains("/h/p", "/h/p/x") && HistoryImport.contains("/h/p", "/h/p") && !HistoryImport.contains("/h/p", "/h/pp"))
}

@Test func folderMatchPicksDeepestProjectAndWorktreeAlias() throws {
    let f = Fixture()
    let outer = try f.mkdir(f.home + "/Projects/mono"), inner = try f.mkdir(f.home + "/Projects/mono/app")
    let worktree = try f.mkdir(f.home + "/worktrees/api-feature")
    let a = Project(name: "mono", path: outer), b = Project(name: "app", path: inner)
    let c = Project(name: "api", path: try f.mkdir(f.home + "/Projects/api"),
                    sessions: [LinkedSession(agent: .claude, sessionID: "x", title: "wt", workingDirectory: worktree)])
    let inApp = try f.claudeFile(cwd: inner + "/src")
    let inMono = try f.claudeFile(cwd: outer + "/docs")
    let inWorktree = try f.claudeFile(cwd: worktree)

    let found = f.analyze([a, b, c])
    func match(_ file: URL) -> HistoryImport.Candidate? { found.first { $0.file.lastPathComponent == file.lastPathComponent } }
    #expect(match(inApp)?.projectID == b.id && match(inApp)?.reason == .folder)
    #expect(match(inMono)?.projectID == a.id)
    #expect(match(inWorktree)?.projectID == c.id)
}

@Test func pathVotesAssignHomeConversations() throws {
    let f = Fixture()
    let app = Project(name: "app", path: try f.mkdir(f.home + "/Projects/app"))
    let other = Project(name: "other", path: try f.mkdir(f.home + "/Projects/other"))
    // Absolute, ~/ and relative-to-~ mentions; two in one call still count once.
    let voted = try f.claudeFile(cwd: f.home, tools: [
        ["command": "cat \(f.home)/Projects/app/README.md && ls ~/Projects/app/src"],
        ["file_path": "~/Projects/app/Sources/main.swift"],
        ["command": "git -C Projects/app status"],
    ])
    let noisy = try f.claudeFile(cwd: f.home, tools: [["command": "ls ~/Projects/app"], ["command": "echo hi"]])
    let tie = try f.claudeFile(cwd: f.home, tools: [
        ["file_path": "~/Projects/app/a"], ["file_path": "~/Projects/app/b"],
        ["file_path": "~/Projects/other/a"], ["file_path": "~/Projects/other/b"],
    ])

    let found = f.analyze([app, other])
    func match(_ file: URL) -> HistoryImport.Candidate? { found.first { $0.file.lastPathComponent == file.lastPathComponent } }
    #expect(match(voted)?.projectID == app.id)
    #expect(match(voted)?.reason == .paths(3))
    #expect(match(noisy)?.projectID == nil)
    #expect(match(noisy)?.suggestions == [app.id])
    #expect(match(tie)?.projectID == nil)
    #expect(Set(match(tie)?.suggestions ?? []) == [app.id, other.id])
}

@Test func codexToolCallsVote() throws {
    let f = Fixture()
    let app = Project(name: "app", path: try f.mkdir(f.home + "/Projects/app"))
    try f.codexFile(id: "c-1", cwd: f.home, calls: [
        ["type": "function_call", "name": "shell", "arguments": #"{"command":["bash","-lc","ls src"],"workdir":"~/Projects/app"}"#],
        ["type": "custom_tool_call", "name": "apply_patch", "input": "*** Begin Patch\n*** Update File: \(f.home)/Projects/app/x.swift\n"],
    ])
    let found = f.analyze([app])
    #expect(found.first?.projectID == app.id)
    #expect(found.first?.reason == .paths(2))
}

@Test func scanSkipsKnownSubagentNonUUIDAndEmpty() throws {
    let f = Fixture()
    let app = Project(name: "app", path: try f.mkdir(f.home + "/Projects/app"),
                      sessions: [LinkedSession(agent: .claude, sessionID: "AAAAAAAA-0000-0000-0000-000000000001", title: "old", archived: true)])
    try f.claudeFile(id: "aaaaaaaa-0000-0000-0000-000000000001", cwd: app.path)
    try f.claudeFile(id: "agent-1234", cwd: app.path)
    let empty = f.claude.appendingPathComponent(HistoryImport.slug(app.path)).appendingPathComponent("bbbbbbbb-0000-0000-0000-000000000002.jsonl")
    try Data().write(to: empty)
    try f.codexFile(id: "sub-1", cwd: app.path, meta: ["parent_thread_id": "x"])
    try f.codexFile(id: "sub-2", cwd: app.path, meta: ["source": ["subagent": ["review": true]]])
    try f.codexFile(id: "main-1", cwd: app.path)
    let fresh = try f.claudeFile(cwd: app.path)

    let found = f.analyze([app])
    #expect(Set(found.map(\.sessionID)) == ["main-1", fresh.deletingPathExtension().lastPathComponent])
}

@Test func importCopiesWithoutOverwritingAndKeepsOriginal() throws {
    let f = Fixture()
    let app = Project(name: "app", path: try f.mkdir(f.home + "/Projects/app"))
    let id = UUID().uuidString.lowercased()
    let original = try f.claudeFile(id: id, cwd: f.home, tools: [["file_path": "~/Projects/app/a"], ["file_path": "~/Projects/app/b"]])
    let sidecar = original.deletingPathExtension().appendingPathComponent("subagents")
    try FileManager.default.createDirectory(at: sidecar, withIntermediateDirectories: true)
    try "side".write(to: sidecar.appendingPathComponent("s.jsonl"), atomically: true, encoding: .utf8)
    let before = try Data(contentsOf: original)

    let candidate = try #require(f.analyze([app]).first)
    let session = try HistoryImport.prepare(candidate, in: app, claudeRoot: f.claude, home: f.home)
    #expect(session.sessionID == id && session.workingDirectory == nil && session.createdAt == candidate.date)
    let target = f.claude.appendingPathComponent(HistoryImport.slug(app.path))
    #expect(try Data(contentsOf: target.appendingPathComponent("\(id).jsonl")) == before)
    #expect(FileManager.default.fileExists(atPath: target.appendingPathComponent("\(id)/subagents/s.jsonl").path))
    #expect(try Data(contentsOf: original) == before)

    // An existing destination is left alone.
    let other = UUID().uuidString.lowercased()
    try f.claudeFile(id: other, cwd: f.home)
    try "keep".write(to: target.appendingPathComponent("\(other).jsonl"), atomically: true, encoding: .utf8)
    let second = try #require(f.analyze([app]).first { $0.sessionID == other })
    _ = try HistoryImport.prepare(second, in: app, claudeRoot: f.claude, home: f.home)
    #expect(try String(contentsOf: target.appendingPathComponent("\(other).jsonl"), encoding: .utf8) == "keep")
}

@Test func importResumesExistingSubfolderInPlace() throws {
    let f = Fixture()
    let app = Project(name: "app", path: try f.mkdir(f.home + "/Projects/app"))
    let sub = try f.mkdir(app.path + "/server")
    try f.claudeFile(cwd: sub)
    try f.codexFile(id: "cx", cwd: sub)
    let gone = try f.claudeFile(cwd: app.path + "/deleted")

    let found = f.analyze([app])
    for c in found where c.file.lastPathComponent != gone.lastPathComponent {
        let session = try HistoryImport.prepare(c, in: app, claudeRoot: f.claude, home: f.home)
        #expect(session.workingDirectory == sub)
    }
    // A folder that no longer exists falls back to a copy into the project's own folder.
    let missing = try #require(found.first { $0.file.lastPathComponent == gone.lastPathComponent })
    let session = try HistoryImport.prepare(missing, in: app, claudeRoot: f.claude, home: f.home)
    #expect(session.workingDirectory == nil)
    #expect(FileManager.default.fileExists(atPath: f.claude.appendingPathComponent(HistoryImport.slug(app.path)).appendingPathComponent(gone.lastPathComponent).path))
}
