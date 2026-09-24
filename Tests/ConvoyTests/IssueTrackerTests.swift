import Foundation
import Testing
@testable import Convoy

private func body(_ request: URLRequest) throws -> [String: Any] {
    try JSONSerialization.jsonObject(with: request.httpBody ?? Data()) as? [String: Any] ?? [:]
}

@Test func linearRequestUsesRawKeyAndAssignedOpenFilter() throws {
    let c = TrackerConnection(kind: .linear, auth: .apiKey, name: "L")
    let request = try TrackerClient.request(for: c, secret: "lin_api_x", query: "", mineOnly: true)
    #expect(request.url?.absoluteString == "https://api.linear.app/graphql")
    #expect(request.value(forHTTPHeaderField: "Authorization") == "lin_api_x")
    let filter = try #require((try body(request)["variables"] as? [String: Any])?["filter"] as? [String: Any])
    #expect(filter["assignee"] != nil)
    #expect(filter["title"] == nil)
    // A key searches that one issue regardless of assignee.
    let byKey = TrackerClient.linearFilter("eng-42", mineOnly: true)
    #expect((byKey["number"] as? [String: Int])?["eq"] == 42)
    #expect(((byKey["team"] as? [String: Any])?["key"] as? [String: String])?["eq"] == "ENG")
    #expect(byKey["assignee"] == nil)
    let byURL = TrackerClient.linearFilter("https://linear.app/acme/issue/ENG-7/fix-it", mineOnly: false)
    #expect((byURL["number"] as? [String: Int])?["eq"] == 7)
    #expect(TrackerClient.linearFilter("fix eng-7 crash", mineOnly: false)["title"] != nil)
}

@Test func jiraCloudAndServerRequests() throws {
    let cloud = TrackerConnection(kind: .jira, auth: .apiKey, name: "J", site: "acme.atlassian.net/", username: "me@acme.com")
    let r1 = try TrackerClient.request(for: cloud, secret: "tok", query: "", mineOnly: true)
    #expect(r1.url?.path == "/rest/api/3/search/jql")
    #expect(r1.url?.host == "acme.atlassian.net")
    #expect(r1.value(forHTTPHeaderField: "Authorization") == "Basic " + Data("me@acme.com:tok".utf8).base64EncodedString())
    let jql = URLComponents(url: r1.url!, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "jql" }?.value
    #expect(jql == "assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC")

    let server = TrackerConnection(kind: .jira, auth: .password, name: "J", site: "https://jira.acme.com", username: "bob")
    let r2 = try TrackerClient.request(for: server, secret: "pw", query: "PAY-12", mineOnly: true)
    #expect(r2.url?.path == "/rest/api/2/search")
    #expect(r2.value(forHTTPHeaderField: "Authorization")?.hasPrefix("Basic ") == true)

    let pat = TrackerConnection(kind: .jira, auth: .apiKey, name: "J", site: "https://jira.acme.com")
    #expect(try TrackerClient.request(for: pat, secret: "pat", query: "", mineOnly: false).value(forHTTPHeaderField: "Authorization") == "Bearer pat")
    #expect(throws: TrackerError.self) { try TrackerClient.request(for: TrackerConnection(kind: .jira, auth: .apiKey, name: "J"), secret: "x", query: "", mineOnly: true) }

    #expect(TrackerClient.jql("PAY-12", mineOnly: true) == "key = PAY-12")
    #expect(TrackerClient.jql("project = PAY ORDER BY created", mineOnly: true) == "project = PAY ORDER BY created")
    #expect(TrackerClient.jql("say \"hi\"", mineOnly: false) == "statusCategory != Done AND text ~ \"say \\\"hi\\\"\" ORDER BY updated DESC")
}

@Test func parsesLinearIssuesAndErrors() throws {
    let json = """
    {"data":{"issues":{"nodes":[{"identifier":"ENG-1","title":"Fix login","description":"Steps","url":"https://linear.app/a/issue/ENG-1","priorityLabel":"High","state":{"name":"Todo"}}]}}}
    """
    let issues = try TrackerClient.parse(.linear, data: Data(json.utf8))
    #expect(issues == [TrackerIssue(key: "ENG-1", title: "Fix login", details: "Steps", url: "https://linear.app/a/issue/ENG-1", status: "Todo", priority: "High")])
    #expect(throws: TrackerError.self) { try TrackerClient.parse(.linear, data: Data(#"{"errors":[{"message":"Authentication required"}]}"#.utf8)) }
}

@Test func parsesJiraV3DocumentAndV2StringDescriptions() throws {
    let json = """
    {"issues":[
      {"key":"PAY-3","fields":{"summary":"Refunds","status":{"name":"In Progress"},"priority":{"name":"P1"},
        "description":{"type":"doc","content":[
          {"type":"paragraph","content":[{"type":"text","text":"Handle "},{"type":"text","text":"partial"},{"type":"text","text":" refunds."}]},
          {"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},
                                          {"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}}},
      {"key":"PAY-4","fields":{"summary":"Old","description":"plain text"}}
    ]}
    """
    let issues = try TrackerClient.parse(.jira, data: Data(json.utf8), site: URL(string: "https://acme.atlassian.net"))
    #expect(issues.count == 2)
    #expect(issues[0].details == "Handle partial refunds.\n\n- one\n- two")
    #expect(issues[0].url == "https://acme.atlassian.net/browse/PAY-3")
    #expect(issues[0].status == "In Progress")
    #expect(issues[1].details == "plain text")
    #expect(throws: TrackerError.self) { try TrackerClient.parse(.jira, data: Data(#"{"errorMessages":["Bad JQL"]}"#.utf8)) }
}

@Test func manualKeysForMCPImports() {
    let issues = TrackerClient.manualIssues("eng-1 Fix login\nhttps://linear.app/a/issue/ENG-2/slug, ENG-1\nnot a key", kind: .linear)
    #expect(issues.map(\.key) == ["ENG-1", "ENG-2"])
    #expect(issues[0].title == "Fix login")
    #expect(issues[1].title == "ENG-2" && issues[1].url == "https://linear.app/a/issue/ENG-2/slug")
}

@Test @MainActor func importCreatesTasksWithPerIssueModelsAndSkipsDuplicates() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-import-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: root) }
    let project = Project(name: "p", path: root.path)
    let file = WorkspaceFile(url: root.appendingPathComponent("ws.json"))
    try file.save(Workspace(projects: [project], selectedProjectID: project.id))
    let store = Store(workspaceURL: file.url)
    let linear = TrackerConnection(kind: .linear, auth: .apiKey, name: "L")
    let issues = [TrackerIssue(key: "ENG-1", title: "Fix login", details: "Steps", url: "https://linear.app/x"), TrackerIssue(key: "ENG-2", title: "Add export")]
    var options = IssueImportOptions(agent: .claude, model: "sonnet", mode: "push")
    options.overrides["ENG-2"] = AgentPick(agent: .codex, model: "gpt-5-codex")

    let first = store.importIssues(issues, from: linear, into: project, options: options)
    #expect(first.created.count == 2 && first.skipped == 0)
    let one = try #require(store.tasks.first { $0.source?.key == "ENG-1" })
    #expect(one.title == "ENG-1: Fix login" && one.agent == .claude && one.model == "sonnet" && one.mode == "push" && one.status == .queued)
    let two = try #require(store.tasks.first { $0.source?.key == "ENG-2" })
    #expect(two.agent == .codex && two.model == "gpt-5-codex")

    let again = store.importIssues(issues, from: linear, into: project, options: options)
    #expect(again.created.isEmpty && again.skipped == 2)
    #expect(store.tasks.count == 2)

    let prompt = TaskPrompt.build(task: one, project: project, docExists: false, baseBranch: "main")
    #expect(prompt.contains("Source issue: Linear ENG-1 — https://linear.app/x"))
    #expect(!prompt.contains("MCP"))

    let mcp = TrackerConnection(kind: .jira, auth: .mcp, name: "J")
    store.importIssues(TrackerClient.manualIssues("PAY-9", kind: .jira), from: mcp, into: project, options: options)
    let viaMCP = try #require(store.tasks.first { $0.source?.key == "PAY-9" })
    #expect(viaMCP.title == "PAY-9" && viaMCP.source?.viaMCP == true)
    #expect(TaskPrompt.build(task: viaMCP, project: project, docExists: false, baseBranch: "main").contains("with your Jira MCP tools"))

    // Saved workspaces keep the source link.
    let reloaded = try file.load()
    #expect(reloaded.tasks?.first { $0.source?.key == "ENG-1" }?.source?.url == "https://linear.app/x")
}
