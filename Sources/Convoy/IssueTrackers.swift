import Foundation
import Security

// MARK: - Model

enum TrackerKind: String, Codable, CaseIterable, Identifiable, Sendable {
    case linear = "Linear", jira = "Jira"
    var id: String { rawValue }
    var symbol: String { self == .linear ? "circle.hexagongrid" : "square.stack.3d.up" }
}

/// How Convoy reaches the tracker. `mcp` never touches the network from Convoy:
/// the agent fetches the issue itself through its own configured MCP server.
enum TrackerAuth: String, Codable, CaseIterable, Identifiable, Sendable {
    case apiKey, password, mcp
    var id: String { rawValue }
    func label(for kind: TrackerKind) -> String {
        switch self {
        case .apiKey: kind == .linear ? "API key" : "API token"
        case .password: "Login & password"
        case .mcp: "Agent MCP server"
        }
    }
    static func options(for kind: TrackerKind) -> [TrackerAuth] { kind == .linear ? [.apiKey, .mcp] : [.apiKey, .password, .mcp] }
}

/// A saved tracker connection. The secret lives in the Keychain, never in UserDefaults.
struct TrackerConnection: Codable, Identifiable, Equatable, Sendable {
    var id = UUID()
    var kind: TrackerKind
    var auth: TrackerAuth
    var name: String
    /// Jira site, e.g. https://acme.atlassian.net or https://jira.acme.com.
    var site: String?
    /// Jira Cloud account email, or Server/Data Center username. Empty with a token means a bearer PAT.
    var username: String?

    var siteURL: URL? {
        guard var raw = site?.trimmingCharacters(in: .whitespacesAndNewlines), !raw.isEmpty else { return nil }
        if !raw.contains("://") { raw = "https://" + raw }
        while raw.hasSuffix("/") { raw.removeLast() }
        return URL(string: raw)
    }
    /// Atlassian Cloud dropped the v2 search endpoint; Server/Data Center still only has it.
    var isJiraCloud: Bool { siteURL?.host?.hasSuffix(".atlassian.net") == true }
}

/// Where an imported task came from; used for the prompt, dedupe and the link on the task.
struct IssueSource: Codable, Equatable, Sendable {
    var tracker: TrackerKind
    var key: String
    /// Site or workspace the key belongs to (`acme.atlassian.net`, `linear.app/acme`):
    /// the same key on two sites is two issues.
    var origin: String?
    var url: String?
    /// Imported by key only; the agent must fetch the issue through MCP.
    var viaMCP: Bool?
}

struct TrackerIssue: Identifiable, Equatable, Sendable {
    var key: String
    var title: String
    var details = ""
    var url: String?
    var status: String?
    var priority: String?
    var id: String { key }
}

// MARK: - Secrets

enum TrackerSecrets {
    static let service = "com.iftech.convoy.trackers"

    private static func query(_ id: UUID) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: service, kSecAttrAccount as String: id.uuidString]
    }

    static func read(_ id: UUID) -> String? {
        var q = query(id)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(q as CFDictionary, &item) == errSecSuccess, let data = item as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }

    static func write(_ secret: String, for id: UUID) throws {
        let data = Data(secret.utf8)
        let status = SecItemUpdate(query(id) as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if status == errSecItemNotFound {
            var add = query(id)
            add[kSecValueData as String] = data
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
            let added = SecItemAdd(add as CFDictionary, nil)
            guard added == errSecSuccess else { throw GitError("Could not save the secret to the Keychain (\(added)).") }
        } else if status != errSecSuccess {
            throw GitError("Could not update the secret in the Keychain (\(status)).")
        }
    }

    static func delete(_ id: UUID) { SecItemDelete(query(id) as CFDictionary) }
}

// MARK: - Connections

@MainActor
final class TrackerConnections: ObservableObject {
    static let shared = TrackerConnections()
    private static let defaultsKey = "trackerConnections"

    @Published private(set) var connections: [TrackerConnection] = []

    init() {
        if let data = UserDefaults.standard.data(forKey: Self.defaultsKey),
           let saved = try? JSONDecoder().decode([TrackerConnection].self, from: data) { connections = saved }
    }

    private func persist() {
        if let data = try? JSONEncoder().encode(connections) { UserDefaults.standard.set(data, forKey: Self.defaultsKey) }
    }

    /// Saves the connection; an empty `secret` keeps the stored one when editing.
    func save(_ connection: TrackerConnection, secret: String?) throws {
        if connection.auth != .mcp, let secret, !secret.isEmpty { try TrackerSecrets.write(secret, for: connection.id) }
        if connection.auth == .mcp { TrackerSecrets.delete(connection.id) }
        if let i = connections.firstIndex(where: { $0.id == connection.id }) { connections[i] = connection } else { connections.append(connection) }
        persist()
    }

    func remove(_ connection: TrackerConnection) {
        TrackerSecrets.delete(connection.id)
        connections.removeAll { $0.id == connection.id }
        persist()
    }

    func secret(for connection: TrackerConnection) -> String? { TrackerSecrets.read(connection.id) }
}

// MARK: - API client

struct TrackerError: LocalizedError { let message: String; var errorDescription: String? { message } }

enum TrackerClient {
    static let pageSize = 50

    /// Builds the search request. `query` is free text, an issue key, or (Jira) JQL; `rawJQL` sends it
    /// verbatim. Empty means open issues.
    static func request(for connection: TrackerConnection, secret: String, query: String, mineOnly: Bool, rawJQL: Bool = false) throws -> URLRequest {
        let q = query.trimmingCharacters(in: .whitespacesAndNewlines)
        switch connection.kind {
        case .linear:
            var request = URLRequest(url: URL(string: "https://api.linear.app/graphql")!)
            request.httpMethod = "POST"
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            // Personal API keys go in raw; OAuth tokens need the Bearer scheme.
            request.setValue(secret.hasPrefix("lin_oauth_") ? "Bearer \(secret)" : secret, forHTTPHeaderField: "Authorization")
            let body: [String: Any] = ["query": linearQuery, "variables": ["first": pageSize, "filter": linearFilter(q, mineOnly: mineOnly)]]
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
            return request
        case .jira:
            guard let site = connection.siteURL else { throw TrackerError(message: "Set the Jira site URL first.") }
            let path = connection.isJiraCloud ? "/rest/api/3/search/jql" : "/rest/api/2/search"
            var components = URLComponents(url: site.appendingPathComponent(path), resolvingAgainstBaseURL: false)!
            components.queryItems = [
                URLQueryItem(name: "jql", value: rawJQL && !q.isEmpty ? q : jql(q, mineOnly: mineOnly)),
                URLQueryItem(name: "maxResults", value: String(pageSize)),
                URLQueryItem(name: "fields", value: "summary,description,status,priority"),
            ]
            var request = URLRequest(url: components.url!)
            request.setValue("application/json", forHTTPHeaderField: "Accept")
            let user = connection.username?.trimmingCharacters(in: .whitespaces) ?? ""
            if user.isEmpty {
                guard connection.auth == .apiKey else { throw TrackerError(message: "Enter the Jira username.") }
                request.setValue("Bearer \(secret)", forHTTPHeaderField: "Authorization")   // Data Center personal access token
            } else {
                request.setValue("Basic " + Data("\(user):\(secret)".utf8).base64EncodedString(), forHTTPHeaderField: "Authorization")
            }
            return request
        }
    }

    static let linearQuery = """
    query Issues($filter: IssueFilter, $first: Int) {
      issues(filter: $filter, first: $first, orderBy: updatedAt) {
        nodes { identifier title description url priorityLabel state { name } }
      }
    }
    """

    static func linearFilter(_ q: String, mineOnly: Bool) -> [String: Any] {
        var filter: [String: Any] = [:]
        if let (team, number) = issueKey(q) {
            filter["team"] = ["key": ["eq": team]]
            filter["number"] = ["eq": number]
            return filter
        }
        if mineOnly { filter["assignee"] = ["isMe": ["eq": true]] }
        filter["state"] = ["type": ["nin": ["completed", "canceled"]]]
        if !q.isEmpty { filter["title"] = ["containsIgnoreCase": q] }
        return filter
    }

    static func jql(_ q: String, mineOnly: Bool) -> String {
        if looksLikeJQL(q) { return q }
        if let (project, number) = issueKey(q) { return "key = \(project)-\(number)" }
        var clauses: [String] = []
        if mineOnly { clauses.append("assignee = currentUser()") }
        clauses.append("statusCategory != Done")
        if !q.isEmpty { clauses.append("text ~ \"\(q.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\""))\"") }
        return clauses.joined(separator: " AND ") + " ORDER BY updated DESC"
    }

    /// Symbolic operators (`project=PAY`, `created >= -7d`), `ORDER BY`, or a function call. Word operators
    /// such as `in`/`is` also occur in plain text, so those need the explicit JQL mode.
    static func looksLikeJQL(_ q: String) -> Bool {
        q.range(of: #"[\w\]\)"]\s*(!=|!~|>=|<=|=|~|<|>)\s*\S|\bORDER\s+BY\b|\b\w+\(\s*\)"#, options: [.regularExpression, .caseInsensitive]) != nil
    }

    /// `host[/path]` naming the site of a link: the Linear workspace slug, or a Jira context path before `/browse/`.
    static func urlOrigin(_ url: String) -> String? {
        guard let components = URLComponents(string: url), let host = components.host?.lowercased(), !host.isEmpty else { return nil }
        let parts = components.path.split(separator: "/").map(String.init)
        if host == "linear.app" { return parts.first.map { "\(host)/\($0.lowercased())" } }
        let site = Array(parts.prefix { $0 != "browse" })
        return site.isEmpty ? host : "\(host)/\(site.joined(separator: "/"))"
    }

    /// Which site an issue lives on: from its link, else the connection's Jira site, else the connection itself.
    static func origin(of issue: TrackerIssue, in connection: TrackerConnection) -> String {
        issue.url.flatMap(urlOrigin) ?? connection.siteURL.flatMap { urlOrigin($0.absoluteString) } ?? "connection:\(connection.id.uuidString)"
    }

    /// `ENG-123`, or a Linear/Jira URL that contains one.
    static func issueKey(_ text: String) -> (String, Int)? {
        guard let range = text.range(of: #"\b[A-Za-z][A-Za-z0-9_]*-\d+\b"#, options: .regularExpression) else { return nil }
        let bare = text.trimmingCharacters(in: .whitespaces)
        // Only treat free text as a key when it is the key itself or a URL to one.
        guard bare.count == text[range].count || bare.contains("://") else { return nil }
        let parts = text[range].split(separator: "-")
        guard parts.count == 2, let n = Int(parts[1]) else { return nil }
        return (parts[0].uppercased(), n)
    }

    static func parse(_ kind: TrackerKind, data: Data, site: URL? = nil) throws -> [TrackerIssue] {
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw TrackerError(message: "Unexpected response.") }
        switch kind {
        case .linear:
            if let errors = json["errors"] as? [[String: Any]], !errors.isEmpty {
                throw TrackerError(message: errors.compactMap { $0["message"] as? String }.joined(separator: "; "))
            }
            let nodes = ((json["data"] as? [String: Any])?["issues"] as? [String: Any])?["nodes"] as? [[String: Any]] ?? []
            return nodes.compactMap { node in
                guard let key = node["identifier"] as? String, let title = node["title"] as? String else { return nil }
                return TrackerIssue(key: key, title: title, details: node["description"] as? String ?? "", url: node["url"] as? String,
                                    status: (node["state"] as? [String: Any])?["name"] as? String, priority: node["priorityLabel"] as? String)
            }
        case .jira:
            if let messages = json["errorMessages"] as? [String], !messages.isEmpty, json["issues"] == nil {
                throw TrackerError(message: messages.joined(separator: "; "))
            }
            let issues = json["issues"] as? [[String: Any]] ?? []
            return issues.compactMap { issue in
                guard let key = issue["key"] as? String, let fields = issue["fields"] as? [String: Any] else { return nil }
                let description: String
                if let text = fields["description"] as? String { description = text }
                else if let doc = fields["description"] as? [String: Any] { description = adfText(doc).trimmingCharacters(in: .whitespacesAndNewlines) }
                else { description = "" }
                return TrackerIssue(key: key, title: fields["summary"] as? String ?? key, details: description,
                                    url: site.map { $0.appendingPathComponent("browse/\(key)").absoluteString },
                                    status: (fields["status"] as? [String: Any])?["name"] as? String,
                                    priority: (fields["priority"] as? [String: Any])?["name"] as? String)
            }
        }
    }

    /// Plain-text rendering of Atlassian Document Format (Jira Cloud v3 descriptions).
    static func adfText(_ node: [String: Any], depth: Int = 0) -> String {
        let type = node["type"] as? String ?? ""
        let attrs = node["attrs"] as? [String: Any] ?? [:]
        let children = (node["content"] as? [[String: Any]] ?? [])
        func inner(_ d: Int = depth) -> String { children.map { adfText($0, depth: d) }.joined() }
        switch type {
        case "text": return node["text"] as? String ?? ""
        case "hardBreak": return "\n"
        case "mention": return attrs["text"] as? String ?? ""
        case "emoji": return attrs["text"] as? String ?? attrs["shortName"] as? String ?? ""
        case "inlineCard", "blockCard": return attrs["url"] as? String ?? ""
        case "paragraph": return inner() + "\n\n"
        case "heading": return String(repeating: "#", count: attrs["level"] as? Int ?? 2) + " " + inner() + "\n\n"
        case "codeBlock": return "```\n" + inner() + "\n```\n\n"
        case "rule": return "---\n\n"
        case "bulletList", "orderedList":
            let ordered = type == "orderedList"
            let items = children.enumerated().map { i, item in
                String(repeating: "  ", count: depth) + (ordered ? "\(i + 1). " : "- ") + adfText(item, depth: depth + 1).trimmingCharacters(in: .whitespacesAndNewlines)
            }
            return items.joined(separator: "\n") + "\n\n"
        case "listItem": return children.map { adfText($0, depth: depth) }.joined().replacingOccurrences(of: "\n\n", with: "\n")
        default: return inner()
        }
    }

    static func fetch(_ connection: TrackerConnection, secret: String, query: String, mineOnly: Bool, rawJQL: Bool = false) async throws -> [TrackerIssue] {
        let request = try request(for: connection, secret: secret, query: query, mineOnly: mineOnly, rawJQL: rawJQL)
        let (data, response) = try await URLSession.shared.data(for: request)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        switch code {
        case 200..<300: return try parse(connection.kind, data: data, site: connection.siteURL)
        case 401, 403: throw TrackerError(message: "\(connection.kind.rawValue) rejected the credentials (HTTP \(code)). Check the \(connection.auth.label(for: connection.kind).lowercased()).")
        default:
            // Error bodies still carry useful messages (bad JQL, GraphQL validation).
            do { _ = try parse(connection.kind, data: data) } catch let error as TrackerError { throw error } catch {}
            throw TrackerError(message: "\(connection.kind.rawValue) returned HTTP \(code).")
        }
    }

    /// Issues typed by key or URL for MCP connections, one per line or comma separated.
    /// Text after the key on the same line becomes the title.
    static func manualIssues(_ text: String, kind: TrackerKind) -> [TrackerIssue] {
        var seen: Set<String> = []
        return text.split(whereSeparator: { $0.isNewline || $0 == "," }).compactMap { raw in
            let line = raw.trimmingCharacters(in: .whitespaces)
            guard let range = line.range(of: #"\b[A-Za-z][A-Za-z0-9_]*-\d+\b"#, options: .regularExpression) else { return nil }
            let key = line[range].uppercased()
            guard seen.insert(key).inserted else { return nil }
            let isURL = line.contains("://")
            let rest = isURL ? "" : line[range.upperBound...].trimmingCharacters(in: CharacterSet(charactersIn: " :—-\t"))
            return TrackerIssue(key: key, title: rest.isEmpty ? key : rest, url: isURL ? line : nil)
        }
    }
}

// MARK: - Store: importing

struct AgentPick: Equatable {
    var agent: Agent
    var model: String?
}

/// Defaults applied to every imported issue; `overrides` replace agent/model per issue key.
struct IssueImportOptions {
    var agent: Agent
    var model: String?
    var mode: String
    var autoReview = false
    var runNow = false
    var overrides: [String: AgentPick] = [:]
}

extension Store {
    func task(importing issue: TrackerIssue, from connection: TrackerConnection, into project: Project, options: IssueImportOptions) -> AgentTask {
        let pick = options.overrides[issue.key] ?? AgentPick(agent: options.agent, model: options.model)
        let title = issue.title == issue.key ? issue.key : "\(issue.key): \(issue.title)"
        var details = issue.details
        if details.count > 12000 { details = String(details.prefix(12000)) + "\n…(truncated; see the issue)" }
        var task = AgentTask(projectID: project.id, title: String(title.prefix(200)), details: details, mode: options.mode, agent: pick.agent)
        task.model = pick.model?.trimmingCharacters(in: .whitespaces).isEmpty == false ? pick.model : nil
        task.autoReview = options.autoReview
        task.source = IssueSource(tracker: connection.kind, key: issue.key, origin: TrackerClient.origin(of: issue, in: connection),
                                  url: issue.url, viaMCP: connection.auth == .mcp ? true : nil)
        return task
    }

    /// Tracker, site and key: what makes an imported issue the same issue.
    func importIdentity(_ issue: TrackerIssue, from connection: TrackerConnection) -> String {
        "\(connection.kind.rawValue)|\(TrackerClient.origin(of: issue, in: connection))|\(issue.key)"
    }
    func importedIdentities(in project: Project) -> Set<String> {
        Set(tasks(for: project).compactMap { $0.source.map { "\($0.tracker.rawValue)|\($0.origin ?? "")|\($0.key)" } })
    }

    /// Creates tasks for the issues not already imported into `project`; returns (created, skipped).
    @discardableResult
    func importIssues(_ issues: [TrackerIssue], from connection: TrackerConnection, into project: Project, options: IssueImportOptions) -> (created: [AgentTask], skipped: Int) {
        var existing = importedIdentities(in: project)
        var created: [AgentTask] = []
        for issue in issues where existing.insert(importIdentity(issue, from: connection)).inserted {
            let task = task(importing: issue, from: connection, into: project, options: options)
            saveTask(task)
            created.append(task)
        }
        if options.runNow { for task in created { runTask(task) } }
        return (created, issues.count - created.count)
    }
}
