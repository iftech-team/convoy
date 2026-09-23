import Foundation
import Testing
@testable import Convoy

@Test @MainActor func modelOverrideReachesBothCLIs() {
    var claude = LinkedSession(agent: .claude, sessionID: UUID().uuidString.lowercased(), title: "t")
    claude.model = "opus"
    #expect(SessionCommand.script(session: claude, directory: "/tmp", resume: false).contains("'--model' 'opus'"))
    var codex = LinkedSession(agent: .codex, sessionID: "", title: "t")
    codex.model = "gpt-5-codex"
    #expect(SessionCommand.script(session: codex, directory: "/tmp", resume: false).contains("'--model' 'gpt-5-codex'"))
    let plain = LinkedSession(agent: .claude, sessionID: UUID().uuidString.lowercased(), title: "t")
    #expect(!SessionCommand.script(session: plain, directory: "/tmp", resume: false).contains("--model"))
}

@Test func groupFoldersFallBackToMainAndGetRepoAwareInstructions() {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-nonrepo-\(UUID().uuidString)")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let base = GitWorktree.defaultBase(dir.path)
    #expect(base == "main")
    #expect(!base.contains("fatal"))
    let group = Project(name: "g", path: dir.path, group: true)
    let task = AgentTask(projectID: group.id, title: "x", mode: "push")
    let prompt = TaskPrompt.build(task: task, project: group, docExists: false, baseBranch: base)
    #expect(prompt.contains("group of separate git repositories"))
    #expect(!prompt.contains("HEAD:fatal"))
}

@Test func unnamedSessionsGetAUsefulTitle() {
    #expect(SessionNaming.autoTitle(agent: .claude, prompt: "Fix the login bug\n\nMore details here") == "Fix the login bug")
    #expect(SessionNaming.autoTitle(agent: .claude, prompt: "# Refactor the payment module") == "Refactor the payment module")
    let long = SessionNaming.autoTitle(agent: .codex, prompt: String(repeating: "word ", count: 30))
    #expect(long.count <= 60 && long.hasSuffix("…"))
    let fallback = SessionNaming.autoTitle(agent: .codex, prompt: "   \n", date: Date(timeIntervalSince1970: 0))
    #expect(fallback.hasPrefix("Codex · "))
}
