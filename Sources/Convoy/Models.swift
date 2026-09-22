import Foundation

enum Agent: String, Codable, CaseIterable, Identifiable, Sendable {
    case claude = "Claude Code", codex = "Codex"
    var id: String { rawValue }
    var other: Agent { self == .claude ? .codex : .claude }
}

enum TaskStatus: String, Codable, CaseIterable, Identifiable, Sendable {
    case planned = "Planned", building = "Building", review = "Needs review", changes = "Changes requested", done = "Done"
    var id: String { rawValue }
}

struct LinkedSession: Codable, Identifiable, Equatable, Sendable {
    var id = UUID()
    var agent: Agent
    var sessionID: String
    var title: String
    var notes = ""
    var createdAt = Date()
    var initialPrompt: String?
    var reviewOf: UUID?
    var archived: Bool?
    /// Set when the session runs in its own git worktree (Orca-style task isolation).
    var workingDirectory: String?
    var branch: String?
    var baseRef: String?
    var pinned: Bool?
    /// Provider configuration home selected on first launch; retained for resume.
    var agentHome: String?
    /// CLI model override (`claude --model` / `codex --model`); nil means the agent default.
    var model: String?

    /// Recovery preserves scope, but never replays a possibly completed task.
    func freshConversation() -> LinkedSession {
        var fresh = self
        fresh.id = UUID()
        fresh.sessionID = agent == .claude ? UUID().uuidString.lowercased() : ""
        fresh.title += " — new"
        fresh.createdAt = Date()
        fresh.initialPrompt = nil
        fresh.archived = false
        return fresh
    }
}

/// A unit of work an agent takes end to end: worktree, implementation, then PR or push.
struct AgentTask: Codable, Identifiable, Equatable, Sendable {
    enum Status: String, Codable, CaseIterable { case queued, running, review, pr, done, failed }
    var id = UUID()
    var projectID: UUID
    var title: String
    var details: String = ""
    var spec: String?             // relative path of a spec file in the repo, e.g. .specdesk/specs/checkout.md
    var mode: String = "pr"       // "pr" | "push" | "none"
    var agent: Agent = Agent(rawValue: UserDefaults.standard.string(forKey: "defaultAgent") ?? "") ?? .claude
    var status: Status = .queued
    var createdAt = Date()
    var startedAt: Date?
    var finishedAt: Date?
    var sessionID: UUID?
    var branch: String?
    var prURL: String?
    var autoReview: Bool = false
    var reviewSessionID: UUID?
    var model: String?
}

/// Saved terminal command or agent prompt; `projectID == nil` means global.
struct QuickCommand: Codable, Identifiable, Equatable, Sendable {
    var id = UUID()
    var title: String
    var text: String
    var projectID: UUID?
    var submit: Bool = false
}

struct WorkTask: Codable, Identifiable, Equatable, Sendable {
    var id = UUID()
    var title: String
    var status: TaskStatus = .planned
    var builder: Agent = .claude
    var reviewer: Agent = .codex
    var notes = ""
    var findings = ""
    var sessions: [LinkedSession] = []
}

struct Specification: Codable, Identifiable, Equatable, Sendable {
    var id = UUID()
    var title: String
    var problem = ""
    var requirements = ""
    var acceptance = ""
    var constraints = ""
    var plan = ""
    var revision = 1
    var approvedRevision: Int?
    var tasks: [WorkTask] = []
    var updatedAt = Date()
    var isApproved: Bool { approvedRevision == revision }

    mutating func revise() {
        revision += 1
        if approvedRevision != nil {
            for i in tasks.indices where tasks[i].status == .done { tasks[i].status = .review }
        }
        updatedAt = Date()
    }

    var markdown: String {
        """
        # \(title)

        Revision: \(revision) · \(isApproved ? "Approved" : "Draft")

        ## Problem
        \(problem)

        ## Requirements
        \(requirements)

        ## Acceptance criteria
        \(acceptance)

        ## Constraints and out of scope
        \(constraints)

        ## Implementation plan
        \(plan)

        ## Tasks
        \(tasks.map { "- [\($0.status == .done ? "x" : " ")] \($0.title) — \($0.status.rawValue)" }.joined(separator: "\n"))
        """
    }

    func handoff(for task: WorkTask) -> String {
        """
        \(markdown)

        ## Current task
        \(task.title)
        Builder: \(task.builder.rawValue)
        Reviewer: \(task.reviewer.rawValue)

        ## Implementation notes and verification evidence
        \(task.notes)

        ## Review findings
        \(task.findings)

        Review the actual code changes against this specification. Identify the exact revision reviewed and report actionable findings with file locations and severity. Distinguish verified results from unverified assumptions. Do not mark requirements satisfied without evidence.
        """
    }
}

struct Project: Codable, Identifiable, Equatable, Sendable {
    var id = UUID()
    var name: String
    var path: String
    var bookmark: Data?
    var specs: [Specification] = []
    // Optional fields keep workspaces from the first prototype readable.
    var parentID: UUID?
    var group: Bool?
    var sessions: [LinkedSession]?
    /// Orca-style customisation; all optional so older workspaces still load.
    var icon: String?          // emoji, or "gh:<path>" for a downloaded avatar
    var color: String?         // hex like "#5E6AD2"
    var order: Int?
    var setupCommands: String? // run after a worktree is created for this project
    var defaultAgent: Agent?      // preselected agent for this project
    var baseRef: String?          // default base for new worktrees
    var branchPrefix: String?     // overrides the global branch prefix
    var sharedPaths: [String]?    // gitignored paths copied (APFS clone) or symlinked into new worktrees
    var reviewTemplate: String?   // overrides the global review brief
    var taskMode: String?         // "pr" | "push" | "none" default for new tasks
    var autoRunTasks: Bool?       // start the next queued task when one finishes

    var isGroup: Bool { group == true }
    var linkedSessions: [LinkedSession] { sessions ?? [] }

    func launchCommand(for agent: Agent) -> String {
        let quotedPath = "'" + path.replacingOccurrences(of: "'", with: "'\"'\"'") + "'"
        return "cd -- \(quotedPath) && \(agent == .claude ? "claude" : "codex")"
    }
}

struct Workspace: Codable, Equatable {
    var schemaVersion = 2
    var projects: [Project] = []
    var selectedProjectID: UUID?
    var selectedSpecID: UUID?
    var selectedSessionID: UUID?
    var quickCommands: [QuickCommand]?
    var tasks: [AgentTask]?

    var rootProjects: [Project] { projects.filter { $0.parentID == nil }.sorted { ($0.order ?? Int.max, $0.name) < ($1.order ?? Int.max, $1.name) } }

    func children(of id: UUID) -> [Project] { projects.filter { $0.parentID == id } }

    mutating func importProjects(_ incoming: [Project]) {
        // Re-imports preserve IDs, specifications and session history.
        var ids: [UUID: UUID] = [:]
        for item in incoming {
            ids[item.id] = projects.first(where: { $0.path == item.path })?.id ?? item.id
        }
        for var item in incoming {
            let incomingID = item.id
            item.id = ids[incomingID]!
            item.parentID = item.parentID.flatMap { ids[$0] }
            if let index = projects.firstIndex(where: { $0.id == item.id }) {
                projects[index].bookmark = item.bookmark ?? projects[index].bookmark
                // Opening an already-imported child separately must not detach it.
                if let parentID = item.parentID { projects[index].parentID = parentID }
                if item.isGroup { projects[index].group = true }
            } else {
                projects.append(item)
            }
        }
        if let first = incoming.first, let id = ids[first.id] {
            selectedProjectID = id
            selectedSpecID = projects.first(where: { $0.id == id })?.specs.first?.id
        }
    }

    mutating func removeProject(_ id: UUID) {
        let parentID = projects.first(where: { $0.id == id })?.parentID
        // Removing a group preserves its children's records and moves them up.
        for i in projects.indices where projects[i].parentID == id {
            projects[i].parentID = parentID
        }
        projects.removeAll { $0.id == id }
        if selectedProjectID == id {
            selectedProjectID = projects.first?.id
            selectedSpecID = projects.first?.specs.first?.id
        }
    }
}

struct WorkspaceFile {
    let url: URL

    func load() throws -> Workspace {
        guard FileManager.default.fileExists(atPath: url.path) else { return Workspace() }
        let result = try JSONDecoder().decode(Workspace.self, from: Data(contentsOf: url))
        guard (1...2).contains(result.schemaVersion) else {
            throw NSError(domain: "Convoy", code: 1, userInfo: [NSLocalizedDescriptionKey: "This workspace was saved by a newer version of Convoy."])
        }
        return result
    }

    func save(_ workspace: Workspace) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(workspace).write(to: url, options: .atomic)
    }
}
