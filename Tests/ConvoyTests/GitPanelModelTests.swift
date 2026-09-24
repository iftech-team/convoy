import Foundation
import Testing
@testable import Convoy

/// Polls a main-actor condition so the model's detached git work can land.
@MainActor private func waitUntil(_ timeout: TimeInterval = 15, _ condition: @MainActor () -> Bool) async -> Bool {
    let start = Date()
    while !condition() {
        if Date().timeIntervalSince(start) > timeout { return false }
        try? await Task.sleep(for: .milliseconds(40))
    }
    return true
}

@discardableResult private func git(_ args: [String], in dir: String) -> String {
    let r = GitInfoService.run(args, in: dir, timeout: 30)
    #expect(r?.0 == 0, "git \(args.joined(separator: " ")) failed: \(r?.1 ?? "timeout")")
    return r?.1 ?? ""
}

private func write(_ text: String, to path: String) throws { try text.write(toFile: path, atomically: true, encoding: .utf8) }

@Suite(.serialized) struct GitPanelModelTests {

@Test @MainActor func stagesCommitsDiscardsAndBrowsesHistory() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-git-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root.appendingPathComponent("repo/sub"), withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let repo = root.appendingPathComponent("repo").path
    git(["init", "-q", "-b", "main"], in: repo)
    git(["config", "user.email", "t@example.com"], in: repo); git(["config", "user.name", "Test"], in: repo)
    let lines = (1...12).map { "line \($0)" }
    try write(lines.joined(separator: "\n") + "\n", to: repo + "/a.txt")
    git(["add", "-A"], in: repo); git(["commit", "-q", "-m", "first"], in: repo)

    let store = Store(workspaceURL: root.appendingPathComponent("workspace.json"))
    // Point the model at a subfolder: everything must still be root-relative.
    let model = GitPanelModel(directory: repo + "/sub", store: store)
    model.refresh()
    #expect(await waitUntil { model.resolved && model.status?.isEmpty == true })
    #expect(model.isRepo); #expect(model.directory == repo || model.directory == "/private" + repo)
    #expect(model.status?.branch == "main")

    // Edit two places far apart (two hunks) and add an untracked file.
    var edited = lines; edited[0] = "LINE 1"; edited[11] = "LINE 12"
    try write(edited.joined(separator: "\n") + "\n", to: repo + "/a.txt")
    try write("hello\n", to: repo + "/sub/new.txt")
    model.refresh()
    #expect(await waitUntil { model.status?.unstaged.map(\.path) == ["a.txt"] && model.status?.untracked.map(\.path) == ["sub/new.txt"] })
    #expect(model.counts[.unstaged("a.txt")] == LineCounts(added: 2, removed: 2))
    #expect(model.target == .unstaged("a.txt"), "first change is selected automatically")
    #expect(await waitUntil { model.diff?.hunks.count == 2 })
    #expect(model.diff?.hunks[0].lines.contains { $0.kind == .added && $0.text == "LINE 1" } == true)

    // Discarding one hunk leaves the other.
    model.discardHunk(try #require(model.diff?.hunks.first))
    #expect(await waitUntil { model.busy == nil && model.diff?.hunks.count == 1 })
    let afterHunk = try String(contentsOfFile: repo + "/a.txt", encoding: .utf8)
    #expect(afterHunk.hasPrefix("line 1\n") && afterHunk.contains("LINE 12"))
    #expect(model.lastResult?.ok == true)

    // Stage both, commit, log.
    model.stage(["a.txt", "sub/new.txt"])
    #expect(await waitUntil { model.status?.staged.map(\.path) == ["a.txt", "sub/new.txt"] && model.status?.unstaged.isEmpty == true })
    #expect(model.counts[.staged("a.txt")] == LineCounts(added: 1, removed: 1))
    #expect(model.target == .staged("a.txt"), "keep the selected file visible after staging")
    #expect(await waitUntil { !model.diffLoading && model.diff?.additions == 1 })
    #expect(!model.canCommit, "no message yet")
    model.message = "second"
    #expect(model.canCommit)
    model.commit(andPush: false)
    #expect(await waitUntil { model.busy == nil && model.status?.isEmpty == true && model.message.isEmpty })
    #expect(model.lastResult?.text.hasPrefix("Committed ") == true)
    model.loadLog()
    #expect(await waitUntil { model.commits.count == 2 })
    #expect(model.commits[0].subject == "second" && model.commits[0].refs.contains("main"))
    model.selectedCommit = model.commits[0].sha
    #expect(await waitUntil { model.commitFiles.map(\.path) == ["a.txt", "sub/new.txt"] })
    #expect(await waitUntil { model.diff?.path == "a.txt" && model.diff?.additions == 1 })

    // Unstage / discard whole file.
    try write("changed\n", to: repo + "/a.txt")
    model.refresh()
    #expect(await waitUntil { model.status?.unstaged.map(\.path) == ["a.txt"] })
    model.discard(try #require(model.status?.unstaged.first), includeStaged: false)
    #expect(await waitUntil { model.busy == nil && model.status?.isEmpty == true })
    #expect(try String(contentsOfFile: repo + "/a.txt", encoding: .utf8).hasPrefix("LINE 1\n") == false)

    // Branches.
    model.createBranch("feature/x", from: nil)
    #expect(await waitUntil { model.busy == nil && model.status?.branch == "feature/x" })
    #expect(await waitUntil { model.branches.contains { $0.name == "feature/x" && $0.isCurrent } && model.branches.contains { $0.name == "main" && !$0.isCurrent } })
    model.checkout(try #require(model.branches.first { $0.name == "main" }))
    #expect(await waitUntil { model.busy == nil && model.status?.branch == "main" })

    // Revert then soft reset.
    model.revert(model.commits[0])
    #expect(await waitUntil { model.busy == nil && model.commits.count == 3 && model.commits[0].subject.hasPrefix("Revert") })
    #expect(!FileManager.default.fileExists(atPath: repo + "/sub/new.txt"))
    model.reset(to: model.commits[1], mode: .soft)
    #expect(await waitUntil { model.busy == nil && model.commits.count == 2 && model.status?.staged.map(\.path) == ["a.txt", "sub/new.txt"] })
    #expect(model.status?.staged.first { $0.path == "sub/new.txt" }?.index == .deleted)
    model.unstageAll()
    #expect(await waitUntil { model.busy == nil && model.status?.staged.isEmpty == true && model.status?.unstaged.count == 2 })

    // Files tab.
    model.loadTree()
    #expect(await waitUntil { !model.tree.isEmpty })
    #expect(model.tree.map(\.name) == ["sub", "a.txt"])
    model.selectedFile = "a.txt"
    #expect(await waitUntil { model.fileContent?.path == "a.txt" })
    if case .text(let fileLines) = try #require(model.fileContent).body { #expect(fileLines.first == "line 1") } else { Issue.record("expected text") }
    #expect(model.statusCode(for: "a.txt") == .modified)
}

@Test @MainActor func handlesFoldersThatAreNotRepositories() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-plain-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root.appendingPathComponent("src"), withIntermediateDirectories: true)
    try FileManager.default.createDirectory(at: root.appendingPathComponent("node_modules/x"), withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    try write("x", to: root.appendingPathComponent("src/main.swift").path)
    try write("x", to: root.appendingPathComponent("node_modules/x/index.js").path)
    let store = Store(workspaceURL: root.appendingPathComponent("workspace.json"))
    let model = GitPanelModel(directory: root.path, store: store)
    model.refresh()
    #expect(await waitUntil { model.resolved })
    #expect(!model.isRepo && model.status == nil)
    model.loadTree()
    #expect(await waitUntil { !model.tree.isEmpty })
    #expect(model.tree.map(\.name) == ["src"])
}

/// Regression: the runner used to wait on a DispatchGroup, which starves when called from detached tasks
/// and made concurrent git calls time out.
@Test func concurrentGitCallsFromDetachedTasksDoNotHang() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-concurrent-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    git(["init", "-q"], in: root.path)
    let results = await withTaskGroup(of: Bool.self) { group in
        for i in 0..<40 {
            group.addTask(priority: i % 2 == 0 ? .userInitiated : .utility) {
                GitInfoService.run(["rev-parse", "--is-inside-work-tree"], in: root.path, timeout: 5)?.0 == 0
            }
        }
        return await group.reduce(into: [Bool]()) { $0.append($1) }
    }
    #expect(results.count == 40 && results.allSatisfy { $0 })
}

}

@Test @MainActor func groupFolderResolvesToChildRepository() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-group-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: root) }
    for name in ["api", "admin"] {
        let dir = root.appendingPathComponent(name)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        _ = GitInfoService.run(["init", "-q"], in: dir.path)
    }
    try FileManager.default.createDirectory(at: root.appendingPathComponent("docs"), withIntermediateDirectories: true)
    let group = Project(name: "g", path: root.path, group: true)
    let file = WorkspaceFile(url: root.appendingPathComponent("ws.json"))
    try file.save(Workspace(projects: [group], selectedProjectID: group.id))
    let store = Store(workspaceURL: file.url)
    #expect(store.gitRepoCandidates(under: root.path) == [root.appendingPathComponent("admin").path, root.appendingPathComponent("api").path])
    #expect(store.gitPanelDirectory == root.appendingPathComponent("admin").path)
    store.gitPanelRepoChoice[root.path] = root.appendingPathComponent("api").path
    #expect(store.gitPanelDirectory == root.appendingPathComponent("api").path)
}

@Test @MainActor func clearingSelectionCancelsInFlightPreviews() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-selection-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    try "Preview content\n".write(to: root.appendingPathComponent("note.txt"), atomically: true, encoding: .utf8)
    let store = Store(workspaceURL: root.appendingPathComponent("workspace.json"))
    let model = GitPanelModel(directory: root.path, store: store)
    model.target = .untracked("note.txt")
    model.target = nil
    model.selectedFile = "note.txt"
    model.selectedFile = nil
    try await Task.sleep(for: .milliseconds(300))
    #expect(model.diff == nil)
    #expect(!model.diffLoading)
    #expect(model.fileContent == nil)
}

@Test @MainActor func unstagesBeforeFirstCommit() async throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-unborn-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    git(["init", "-q", "-b", "main"], in: root.path)
    try write("new\n", to: root.path + "/new.txt")
    git(["add", "new.txt"], in: root.path)
    let store = Store(workspaceURL: root.appendingPathComponent("workspace.json"))
    let model = GitPanelModel(directory: root.path, store: store)
    model.refresh()
    #expect(await waitUntil { model.status?.staged.count == 1 })
    model.unstage(["new.txt"])
    #expect(await waitUntil { model.status?.untracked.contains { $0.path == "new.txt" } == true })
    #expect(try String(contentsOfFile: root.path + "/new.txt", encoding: .utf8) == "new\n")
}

@Test func gitDrainsOutputWhileWritingLargeInput() throws {
    let root = FileManager.default.temporaryDirectory
    let input = Data(repeating: 65, count: 1_000_000)
    let result = GitInfoService.run(["-c", "alias.convoy-echo=!cat", "convoy-echo"], in: root.path, timeout: 10, input: input)
    #expect(result?.0 == 0)
    #expect(result?.1.utf8.count == input.count)
}
