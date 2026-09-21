import Foundation
import Testing
@testable import Convoy

@Test func workspaceSurvivesRelaunch() throws {
    let folder = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = WorkspaceFile(url: folder.appendingPathComponent("workspace.json"))
    var task = WorkTask(title: "Restore sessions", status: .review)
    task.sessions = [LinkedSession(agent: .codex, sessionID: "saved-thread-id", title: "Review persistence", notes: "Check interruption recovery")]
    task.findings = "Verify corrupted file handling"
    var spec = Specification(title: "Persistent tasks")
    spec.requirements = "Retain both agent session IDs"
    spec.tasks = [task]
    let project = Project(name: "Example", path: "/tmp/example", bookmark: Data([1, 2, 3]), specs: [spec])
    let workspace = Workspace(projects: [project], selectedProjectID: project.id, selectedSpecID: spec.id)
    try file.save(workspace)
    #expect(try WorkspaceFile(url: file.url).load() == workspace)
}

@Test func corruptWorkspaceIsNotReplacedByEmptyState() throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let original = Data("broken JSON".utf8)
    try original.write(to: url)
    #expect(throws: (any Error).self) { try WorkspaceFile(url: url).load() }
    #expect(try Data(contentsOf: url) == original)
}

@Test func newerSchemaIsRejected() throws {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    try Data(#"{"schemaVersion":99,"projects":[]}"#.utf8).write(to: url)
    #expect(throws: (any Error).self) { try WorkspaceFile(url: url).load() }
}

@Test func requirementChangeInvalidatesApprovalAndCompletedWork() {
    var spec = Specification(title: "Folder permissions")
    spec.approvedRevision = 1
    spec.tasks = [WorkTask(title: "Folder picker", status: .done), WorkTask(title: "Reconnect", status: .building)]
    #expect(spec.isApproved)
    spec.requirements = "Support reconnecting a moved folder"
    spec.revise()
    #expect(!spec.isApproved)
    #expect(spec.revision == 2)
    #expect(spec.tasks[0].status == .review)
    #expect(spec.tasks[1].status == .building)
}

@Test func handoffIncludesRequirementsAndFindings() {
    var spec = Specification(title: "Permissions")
    spec.requirements = "Never request Full Disk Access"
    spec.acceptance = "Opening a project uses the folder picker"
    var task = WorkTask(title: "Implement picker")
    task.findings = "Reconnect fails after moving the project"
    task.notes = "Manual picker check passed"
    let brief = spec.handoff(for: task)
    #expect(brief.contains(spec.requirements))
    #expect(brief.contains(spec.acceptance))
    #expect(brief.contains(task.findings))
    #expect(brief.contains(task.notes))
}

@Test func removingProjectRetainsOtherWorkAndRestoresValidSelection() throws {
    let spec = Specification(title: "Keep this specification")
    let first = Project(name: "Remove", path: "/tmp/remove")
    let second = Project(name: "Keep", path: "/tmp/keep", specs: [spec])
    var workspace = Workspace(projects: [first, second], selectedProjectID: first.id)
    workspace.removeProject(first.id)
    #expect(workspace.projects == [second])
    #expect(workspace.selectedProjectID == second.id)
    #expect(workspace.selectedSpecID == spec.id)
    let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: url) }
    let file = WorkspaceFile(url: url)
    try file.save(workspace)
    #expect(try file.load() == workspace)
    workspace.removeProject(second.id)
    #expect(workspace.projects.isEmpty)
    #expect(workspace.selectedProjectID == nil)
    #expect(workspace.selectedSpecID == nil)
}

@Test func removingUnselectedProjectPreservesCurrentSpec() {
    let spec = Specification(title: "Working spec")
    let selected = Project(name: "Selected", path: "/tmp/selected", specs: [spec])
    let other = Project(name: "Other", path: "/tmp/other")
    var workspace = Workspace(projects: [selected, other], selectedProjectID: selected.id, selectedSpecID: spec.id)
    workspace.removeProject(other.id)
    #expect(workspace.selectedProjectID == selected.id)
    #expect(workspace.selectedSpecID == spec.id)
    #expect(workspace.projects == [selected])
}
