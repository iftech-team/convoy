import Foundation
import Testing
@testable import Convoy

@Test func importsNestedGroupsWithoutDependenciesOrSymlinks() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    for path in ["olucha-cargo/api/.git", "olucha-cargo/admin/node_modules/ignored/.git", ".hidden/repo/.git", "standalone/.git"] {
        try FileManager.default.createDirectory(at: root.appendingPathComponent(path), withIntermediateDirectories: true)
    }
    try Data("{}".utf8).write(to: root.appendingPathComponent("olucha-cargo/admin/package.json"))
    try FileManager.default.createSymbolicLink(at: root.appendingPathComponent("loop"), withDestinationURL: root)
    let found = try ProjectDiscovery().scan(root)
    #expect(found.count == 5)
    #expect(found.filter(\.isGroup).count == 2)
    let group = try #require(found.first { $0.name == "olucha-cargo" })
    let api = try #require(found.first { $0.name == "api" })
    #expect(api.parentID == group.id)
    #expect(found.first?.path == root.standardizedFileURL.path)
}

@Test func reimportPreservesGroupWorkAndReparentsExistingChild() {
    let savedSpec = Specification(title: "Across both projects")
    let savedSession = LinkedSession(agent: .claude, sessionID: "existing", title: "Build across group")
    let existing = Project(name: "Parent", path: "/tmp/group", specs: [savedSpec], sessions: [savedSession])
    let child = Project(name: "API", path: "/tmp/group/api")
    var workspace = Workspace(projects: [existing, child])
    let imported = Project(name: "Parent", path: "/tmp/group", group: true)
    let importedChild = Project(name: "API", path: "/tmp/group/api", parentID: imported.id)
    workspace.importProjects([imported, importedChild])
    workspace.importProjects([imported, importedChild])
    #expect(workspace.projects.count == 2)
    #expect(workspace.projects[0].id == existing.id)
    #expect(workspace.projects[0].specs == [savedSpec])
    #expect(workspace.projects[0].linkedSessions == [savedSession])
    #expect(workspace.projects[1].parentID == existing.id)
    workspace.removeProject(existing.id)
    #expect(workspace.projects.count == 1)
    #expect(workspace.projects[0].id == child.id)
    #expect(workspace.projects[0].parentID == nil)
}

@Test func olderWorkspaceLoadsWithoutGroupOrSessionFields() throws {
    let json = #"{"schemaVersion":1,"projects":[{"id":"AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA","name":"Old","path":"/tmp/old","specs":[]}]}"#
    let workspace = try JSONDecoder().decode(Workspace.self, from: Data(json.utf8))
    #expect(workspace.projects[0].linkedSessions.isEmpty)
    #expect(!workspace.projects[0].isGroup)
    #expect(workspace.projects[0].parentID == nil)
}

@Test func launchQuotesPathsAndMessagesAsLiteralArguments() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let directory = root.appendingPathComponent("group ' $(touch BAD)")
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let fake = root.appendingPathComponent("claude")
    try "#!/bin/sh\nprintf '%s\\n' \"$PWD\" \"$@\"\n".write(to: fake, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: fake.path)
    let prompt = "--literal $(touch BAD) `touch BAD` ' quoted\nsecond line"
    let session = LinkedSession(agent: .claude, sessionID: UUID().uuidString, title: "Fix 'quoted' title", initialPrompt: prompt)
    let process = Process(); process.executableURL = URL(fileURLWithPath: "/bin/zsh")
    process.arguments = ["-c", SessionCommand.script(session: session, directory: directory.path, resume: false)]
    process.environment = ["PATH": root.path + ":/usr/bin:/bin"]
    let pipe = Pipe(); process.standardOutput = pipe
    try process.run()
    let output = String(decoding: pipe.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
    process.waitUntilExit()
    #expect(process.terminationStatus == 0)
    #expect(output.contains(prompt))
    #expect(output.contains(session.title))
    #expect(output.contains("--session-id\n" + session.sessionID.lowercased()))
    #expect(!FileManager.default.fileExists(atPath: directory.appendingPathComponent("BAD").path))
}

@Test func resumeNeverSelectsAnUnrelatedLatestSession() {
    let session = LinkedSession(agent: .codex, sessionID: "", title: "API")
    let script = SessionCommand.script(session: session, directory: "/tmp/group", resume: true)
    #expect(script.contains("'resume'"))
    #expect(!script.contains("--last"))
    #expect(!script.contains("dangerously"))
    #expect(SessionCommand.codexID(in: "Continue with codex resume aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee") == "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
}

@Test func recognizesMissingConversationWithoutTreatingOtherFailuresAsMissing() {
    #expect(SessionCommand.missingConversation(in: "No conversation found with session ID: example"))
    #expect(!SessionCommand.missingConversation(in: "Connection timed out"))
}
