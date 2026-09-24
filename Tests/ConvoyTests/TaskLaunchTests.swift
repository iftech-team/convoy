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

@Test @MainActor func taskDependenciesBlockRunningAndRefuseCycles() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-deps-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let project = Project(name: "p", path: root.path)
    let file = WorkspaceFile(url: root.appendingPathComponent("ws.json"))
    try file.save(Workspace(projects: [project], selectedProjectID: project.id))
    let store = Store(workspaceURL: file.url)
    let a = AgentTask(projectID: project.id, title: "A")
    var b = AgentTask(projectID: project.id, title: "B"); b.dependsOn = [a.id]
    store.saveTask(a); store.saveTask(b)
    #expect(store.isBlocked(b))
    #expect(store.blockers(of: b).map(\.title) == ["A"])
    store.runTask(b)
    #expect(store.error?.contains("blocked by: A") == true)
    #expect(store.tasks.first { $0.id == b.id }?.status == .queued)
    #expect(store.wouldCycle(a, dependingOn: b.id))
    #expect(!store.wouldCycle(b, dependingOn: a.id))
    var done = a; done.status = .done; done.finishedAt = Date()
    store.saveTask(done)
    #expect(!store.isBlocked(b))
}

@Test func diagnosticsShellProbeUsesLoginShell() {
    let (status, out) = SetupDiagnostics.shell("command -v git")
    #expect(status == 0)
    #expect(out.hasSuffix("/git"))
    #expect(SetupDiagnostics.shell("exit 3").0 == 3)
}
