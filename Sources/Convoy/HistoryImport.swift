import SwiftUI

/// "Import all history": finds every Claude Code and Codex conversation on the machine, works out which project
/// each belongs to (by the folder it started in, or by the files its tool calls touched), and links them as sessions.
enum HistoryImport {
    enum Reason: Equatable, Sendable { case folder, paths(Int), none }

    struct Candidate: Identifiable, Equatable, Sendable {
        let agent: Agent
        let sessionID: String
        let title: String
        let date: Date
        let file: URL
        /// Normalised folder the conversation started in; empty when the transcript never says.
        let cwd: String
        var projectID: UUID?
        var reason: Reason
        /// Best guesses for an unassigned conversation, most votes first.
        var suggestions: [UUID] = []
        var id: String { agent.rawValue + sessionID }
    }

    /// A folder that maps to a project: the project itself or one of its sessions' worktrees.
    struct Target: Sendable { let path: String; let projectID: UUID }

    // MARK: Paths

    /// Claude's folder name for a cwd: every non-alphanumeric character becomes `-`.
    static func slug(_ path: String) -> String {
        path.replacingOccurrences(of: "[^A-Za-z0-9]", with: "-", options: .regularExpression)
    }

    /// Expands `~/`, resolves relative paths against `base`, collapses `.`/`..`, drops trailing `/`. Nil if not a path.
    static func normalize(_ raw: String, base: String? = nil, home: String) -> String? {
        var path = raw
        if path == "~" { path = home } else if path.hasPrefix("~/") { path = home + path.dropFirst(1) }
        if !path.hasPrefix("/") {
            guard let base, base.hasPrefix("/") else { return nil }
            path = base + "/" + path
        }
        var parts: [Substring] = []
        for part in path.split(separator: "/", omittingEmptySubsequences: true) {
            if part == "." { continue }
            if part == ".." { _ = parts.popLast(); continue }
            parts.append(part)
        }
        return "/" + parts.joined(separator: "/")
    }

    static func contains(_ parent: String, _ child: String) -> Bool {
        child == parent || child.hasPrefix(parent + "/")
    }

    /// The deepest target containing `path`.
    static func owner(of path: String, in targets: [Target]) -> Target? {
        targets.filter { contains($0.path, path) }.max { $0.path.count < $1.path.count }
    }

    /// Every non-group project folder plus each of their sessions' working directories (worktrees).
    static func targets(for projects: [Project], home: String) -> [Target] {
        var out: [Target] = []
        for project in projects where !project.isGroup {
            if let path = normalize(project.path, home: home) { out.append(Target(path: path, projectID: project.id)) }
            for dir in project.linkedSessions.compactMap(\.workingDirectory) {
                if let path = normalize(dir, home: home) { out.append(Target(path: path, projectID: project.id)) }
            }
        }
        return out
    }

    // MARK: Scan

    static var home: String { FileManager.default.homeDirectoryForCurrentUser.path }

    /// Every conversation not already linked to a session, with a project assigned where the rules allow. Newest first.
    static func analyze(projects: [Project], claudeRoot: URL? = nil, codexRoot: URL? = nil, home: String = HistoryImport.home) -> [Candidate] {
        let known = Set(projects.flatMap(\.linkedSessions).map { $0.sessionID.lowercased() }.filter { !$0.isEmpty })
        let targets = targets(for: projects, home: home)
        var out: [Candidate] = []
        for raw in scan(claudeRoot: claudeRoot ?? TranscriptScanner.claudeRoot, codexRoot: codexRoot ?? TranscriptScanner.codexRoot, home: home)
        where !known.contains(raw.sessionID.lowercased()) {
            out.append(assign(raw, targets: targets, home: home))
        }
        return out.sorted { $0.date > $1.date }
    }

    /// All conversations on disk, unassigned. Skips non-UUID Claude files, empty files and Codex subagents.
    static func scan(claudeRoot: URL, codexRoot: URL, home: String) -> [Candidate] {
        let fm = FileManager.default
        let keys: [URLResourceKey] = [.contentModificationDateKey, .fileSizeKey, .isDirectoryKey]
        var out: [Candidate] = []
        for folder in (try? fm.contentsOfDirectory(at: claudeRoot, includingPropertiesForKeys: keys)) ?? [] {
            for file in (try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: keys)) ?? [] where file.pathExtension == "jsonl" {
                let id = file.deletingPathExtension().lastPathComponent
                guard UUID(uuidString: id) != nil else { continue }
                let attrs = try? file.resourceValues(forKeys: Set(keys))
                guard let size = attrs?.fileSize, size > 0 else { continue }
                let cwd = claudeCwd(file).flatMap { normalize($0, home: home) } ?? ""
                out.append(Candidate(agent: .claude, sessionID: id, title: TranscriptScanner.claudeTitle(file) ?? "Session \(id.prefix(8))",
                                     date: attrs?.contentModificationDate ?? .distantPast, file: file, cwd: cwd, reason: .none))
            }
        }
        if let enumerator = fm.enumerator(at: codexRoot, includingPropertiesForKeys: keys, options: [.skipsHiddenFiles]) {
            for case let file as URL in enumerator where file.lastPathComponent.hasPrefix("rollout-") && file.pathExtension == "jsonl" {
                guard let meta = TranscriptScanner.codexMeta(file), !meta.isSubagent else { continue }
                let attrs = try? file.resourceValues(forKeys: Set(keys))
                out.append(Candidate(agent: .codex, sessionID: meta.id, title: meta.title ?? "Codex \(meta.id.prefix(8))",
                                     date: attrs?.contentModificationDate ?? .distantPast, file: file,
                                     cwd: normalize(meta.cwd, home: home) ?? "", reason: .none))
            }
        }
        return out
    }

    /// Folder match first; otherwise vote by the paths the conversation's tool calls mention.
    static func assign(_ candidate: Candidate, targets: [Target], home: String) -> Candidate {
        var c = candidate
        guard !c.cwd.isEmpty else { return c }
        if let target = owner(of: c.cwd, in: targets) {
            c.projectID = target.projectID; c.reason = .folder
            return c
        }
        let votes = pathVotes(c.file, agent: c.agent, cwd: c.cwd, targets: targets, home: home)
        let ranked = votes.sorted { $0.value != $1.value ? $0.value > $1.value : $0.key.uuidString < $1.key.uuidString }
        if let top = ranked.first, top.value >= 2, top.value > 2 * (ranked.dropFirst().first?.value ?? 0) {
            c.projectID = top.key; c.reason = .paths(top.value)
        } else {
            c.suggestions = ranked.prefix(3).map(\.key)
        }
        return c
    }

    // MARK: Path votes

    private static let separators = CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: "\"'`=,;:()[]{}<>|&"))

    /// Strings in a tool call that look like paths, normalised against the conversation's folder.
    static func candidatePaths(_ text: String, cwd: String, home: String) -> [String] {
        text.components(separatedBy: separators).compactMap { token in
            guard token.hasPrefix("/") || token.hasPrefix("~/") || token.contains("/") else { return nil }
            return normalize(token, base: cwd, home: home)
        }
    }

    /// Each tool call gives one vote to every project one of its paths falls in.
    static func pathVotes(_ file: URL, agent: Agent, cwd: String, targets: [Target], home: String) -> [UUID: Int] {
        var votes: [UUID: Int] = [:]
        for call in toolCalls(file, agent: agent) {
            var hit = Set<UUID>()
            for text in call {
                for path in candidatePaths(text, cwd: cwd, home: home) {
                    if let target = owner(of: path, in: targets) { hit.insert(target.projectID) }
                }
            }
            for id in hit { votes[id, default: 0] += 1 }
        }
        return votes
    }

    /// The string arguments of every tool call, one array per call.
    static func toolCalls(_ file: URL, agent: Agent) -> [[String]] {
        var calls: [[String]] = []
        let needles = agent == .claude ? [Data(#""tool_use""#.utf8)] : [Data("function_call".utf8), Data("custom_tool_call".utf8)]
        forEachLine(file, maxBytes: 32 << 20) { line in
            guard needles.contains(where: { line.range(of: $0) != nil }),
                  let json = try? JSONSerialization.jsonObject(with: line) as? [String: Any] else { return true }
            if agent == .claude {
                let parts = (json["message"] as? [String: Any])?["content"] as? [[String: Any]] ?? []
                for part in parts where part["type"] as? String == "tool_use" { calls.append(strings(in: part["input"] ?? [:])) }
            } else if json["type"] as? String == "response_item", let payload = json["payload"] as? [String: Any] {
                switch payload["type"] as? String {
                case "function_call":
                    let raw = payload["arguments"] as? String ?? ""
                    let parsed = raw.data(using: .utf8).flatMap { try? JSONSerialization.jsonObject(with: $0, options: [.fragmentsAllowed]) }
                    calls.append(parsed.map { strings(in: $0) } ?? [raw])
                case "custom_tool_call":
                    calls.append([payload["input"] as? String ?? ""])
                default: break
                }
            }
            return true
        }
        return calls
    }

    /// Every string value, recursively; an array of plain strings (a `command` argv) is joined by spaces.
    static func strings(in value: Any) -> [String] {
        if let s = value as? String { return [s] }
        if let list = value as? [Any] {
            if !list.isEmpty, let words = list as? [String] { return [words.joined(separator: " ")] }
            return list.flatMap { strings(in: $0) }
        }
        if let dict = value as? [String: Any] { return dict.keys.sorted().flatMap { strings(in: dict[$0]!) } }
        return []
    }

    /// `cwd` of the first Claude record that has one.
    static func claudeCwd(_ file: URL) -> String? {
        var cwd: String?
        let needle = Data(#""cwd""#.utf8)
        forEachLine(file, maxBytes: 4 << 20) { line in
            guard line.range(of: needle) != nil, let json = try? JSONSerialization.jsonObject(with: line) as? [String: Any],
                  let value = json["cwd"] as? String, !value.isEmpty else { return true }
            cwd = value
            return false
        }
        return cwd
    }

    /// Streams a file line by line, stopping after `maxBytes` or when `body` returns false.
    static func forEachLine(_ file: URL, maxBytes: Int, _ body: (Data) -> Bool) {
        guard let handle = try? FileHandle(forReadingFrom: file) else { return }
        defer { try? handle.close() }
        var buffer = Data(), read = 0, eof = false
        while read < maxBytes {
            guard let chunk = try? handle.read(upToCount: min(1 << 20, maxBytes - read)), !chunk.isEmpty else { eof = true; break }
            read += chunk.count
            buffer.append(chunk)
            var start = buffer.startIndex
            while let newline = buffer[start...].firstIndex(of: 0x0A) {
                if newline > start, !body(buffer.subdata(in: start..<newline)) { return }
                start = buffer.index(after: newline)
            }
            buffer = buffer.subdata(in: start..<buffer.endIndex)
        }
        if eof, !buffer.isEmpty { _ = body(buffer) }
    }

    // MARK: Import

    enum ImportError: LocalizedError {
        case copyFailed(String)
        var errorDescription: String? { if case .copyFailed(let m) = self { return m }; return nil }
    }

    /// Makes a conversation resumable from `project` and returns the session to link. Claude transcripts filed under
    /// another folder are copied (never moved or overwritten) into the project's folder so `claude --resume` finds them.
    static func prepare(_ candidate: Candidate, in project: Project, claudeRoot: URL, home: String = HistoryImport.home) throws -> LinkedSession {
        let fm = FileManager.default
        let projectPath = normalize(project.path, home: home) ?? project.path
        var session = LinkedSession(agent: candidate.agent, sessionID: candidate.sessionID, title: String(candidate.title.prefix(60)))
        session.createdAt = candidate.date
        let aliases = targets(for: [project], home: home).map(\.path)
        var isDir: ObjCBool = false
        let insideProject = !candidate.cwd.isEmpty && candidate.cwd != projectPath && aliases.contains { contains($0, candidate.cwd) }
        if insideProject, fm.fileExists(atPath: candidate.cwd, isDirectory: &isDir), isDir.boolValue {
            // Subfolder or worktree that still exists: resume right where it started.
            session.workingDirectory = candidate.cwd
            return session
        }
        guard candidate.agent == .claude, candidate.cwd != projectPath else { return session }
        let folder = claudeRoot.appendingPathComponent(slug(projectPath))
        let destination = folder.appendingPathComponent(candidate.file.lastPathComponent)
        guard destination.standardizedFileURL != candidate.file.standardizedFileURL else { return session }
        do {
            try fm.createDirectory(at: folder, withIntermediateDirectories: true)
            if !fm.fileExists(atPath: destination.path) { try fm.copyItem(at: candidate.file, to: destination) }
            let siblings = candidate.file.deletingPathExtension(), siblingTarget = folder.appendingPathComponent(siblings.lastPathComponent)
            if fm.fileExists(atPath: siblings.path, isDirectory: &isDir), isDir.boolValue, !fm.fileExists(atPath: siblingTarget.path) {
                try fm.copyItem(at: siblings, to: siblingTarget)
            }
        } catch {
            throw ImportError.copyFailed("Couldn’t copy \(candidate.file.lastPathComponent): \(error.localizedDescription)")
        }
        return session
    }
}

extension Store {
    /// Links the chosen conversations as resumable (not running) sessions with one save, then reports the outcome.
    func importHistory(_ picks: [(HistoryImport.Candidate, UUID)]) {
        let projects = workspace.projects
        let root = TranscriptScanner.claudeRoot
        Task {
            let (made, failed) = await Task.detached(priority: .userInitiated) { () -> ([(UUID, LinkedSession)], Int) in
                var made: [(UUID, LinkedSession)] = [], failed = 0
                for (candidate, projectID) in picks.sorted(by: { $0.0.date < $1.0.date }) {
                    guard let project = projects.first(where: { $0.id == projectID }) else { failed += 1; continue }
                    do { made.append((projectID, try HistoryImport.prepare(candidate, in: project, claudeRoot: root))) } catch { failed += 1 }
                }
                return (made, failed)
            }.value
            var next = workspace
            var known = Set(next.projects.flatMap(\.linkedSessions).map { $0.sessionID.lowercased() })
            var count = 0
            for (projectID, session) in made where !known.contains(session.sessionID.lowercased()) {
                guard let pi = next.projects.firstIndex(where: { $0.id == projectID }) else { continue }
                var session = session
                if let dir = session.workingDirectory { session.branch = git.info[dir]?.branch }
                next.projects[pi].sessions = next.projects[pi].linkedSessions + [session]
                known.insert(session.sessionID.lowercased()); count += 1
            }
            if count > 0 { commit(next) }
            guard error == nil else { return }
            notice = "Imported \(count) \(count == 1 ? "conversation" : "conversations")" + (failed > 0 ? " · \(failed) could not be copied" : "")
        }
    }
}

/// Review sheet for "Import all history…": one group per project, unassigned conversations last.
struct HistoryImportSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State private var candidates: [HistoryImport.Candidate]?
    /// Chosen project per row; starts as the assigned project.
    @State private var choice: [String: UUID] = [:]
    @State private var checked: Set<String> = []

    private var projects: [Project] {
        store.workspace.projects.filter { !$0.isGroup }.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
    }
    private func project(_ id: UUID?) -> Project? { id.flatMap { id in store.workspace.projects.first { $0.id == id } } }
    /// Groups by the matched project (sorted by name), then "Unassigned".
    private var groups: [(title: String, rows: [HistoryImport.Candidate])] {
        let list = candidates ?? []
        var out: [(String, [HistoryImport.Candidate])] = []
        for p in projects {
            let rows = list.filter { $0.projectID == p.id }
            if !rows.isEmpty { out.append((p.name, rows)) }
        }
        let rest = list.filter { project($0.projectID) == nil }
        if !rest.isEmpty { out.append(("Unassigned", rest)) }
        return out
    }
    private var picks: [(HistoryImport.Candidate, UUID)] {
        (candidates ?? []).compactMap { c in checked.contains(c.id) ? choice[c.id].map { (c, $0) } : nil }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Text("Import all history").font(.system(size: 18, weight: .bold))
                if let candidates, !candidates.isEmpty {
                    Text("\(picks.count) of \(candidates.count) selected").font(.system(size: 12)).foregroundStyle(.tertiary)
                }
                Spacer()
            }
            Text("Conversations Claude Code and Codex saved anywhere on this Mac, matched to your projects by the folder they started in or the files they worked on.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            content
                .frame(height: 420)
                .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 9))
                .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(AppTheme.stroke))
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Import \(picks.count)") { store.importHistory(picks); dismiss() }
                    .buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(picks.isEmpty)
            }
        }
        .padding(24).frame(width: 760)
        .task { await load() }
    }

    @ViewBuilder private var content: some View {
        if candidates == nil {
            VStack(spacing: 10) {
                ProgressView().controlSize(.small)
                Text("Scanning conversations…").font(.callout).foregroundStyle(.secondary)
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if candidates?.isEmpty == true {
            Text("Nothing to import — every conversation on this computer is already in Convoy.")
                .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center).padding(30)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                    ForEach(groups, id: \.title) { group in
                        Section { ForEach(group.rows) { row($0) } } header: { header(group.title, rows: group.rows) }
                    }
                }
            }
        }
    }

    private func header(_ title: String, rows: [HistoryImport.Candidate]) -> some View {
        let ids = Set(rows.map(\.id))
        let all = ids.isSubset(of: checked)
        return HStack(spacing: 8) {
            Text(title).font(.system(size: 12, weight: .semibold))
            Text("\(rows.count)").font(.system(size: 11)).foregroundStyle(.tertiary)
            Spacer()
            Button(all ? "Select none" : "Select all") {
                if all { checked.subtract(ids) } else { checked.formUnion(rows.filter { choice[$0.id] != nil }.map(\.id)) }
            }.buttonStyle(.link).font(.system(size: 11))
        }.padding(.horizontal, 14).padding(.vertical, 7).background(AppTheme.card).overlay(alignment: .bottom) { Divider() }
    }

    private func row(_ c: HistoryImport.Candidate) -> some View {
        let isOn = Binding(get: { checked.contains(c.id) }, set: { if $0 { checked.insert(c.id) } else { checked.remove(c.id) } })
        return HStack(spacing: 10) {
            Toggle("", isOn: isOn).labelsHidden().toggleStyle(.checkbox).disabled(choice[c.id] == nil)
            AgentIcon(agent: c.agent, size: 14).help(c.agent.rawValue)
            VStack(alignment: .leading, spacing: 2) {
                Text(c.title).font(.system(size: 12.5, weight: .medium)).lineLimit(1)
                Text(detail(c)).font(.system(size: 10.5)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            }
            Spacer(minLength: 8)
            if c.projectID == nil || project(c.projectID) == nil { picker(c) }
            Text(c.date.formatted(.relative(presentation: .named))).font(.system(size: 10.5)).foregroundStyle(.tertiary)
                .frame(width: 92, alignment: .trailing).help(c.date.formatted(date: .abbreviated, time: .shortened))
        }
        .padding(.horizontal, 14).padding(.vertical, 7)
        .overlay(alignment: .bottom) { Divider().padding(.leading, 40).opacity(0.6) }
    }

    private func picker(_ c: HistoryImport.Candidate) -> some View {
        let suggested = c.suggestions.compactMap { project($0) }
        return Menu {
            let rest = projects.filter { p in !suggested.contains { $0.id == p.id } }
            if suggested.isEmpty {
                ForEach(rest) { p in Button(p.name) { pick(p.id, for: c) } }
            } else {
                Section("Suggested") { ForEach(suggested) { p in Button(p.name) { pick(p.id, for: c) } } }
                Section("All projects") { ForEach(rest) { p in Button(p.name) { pick(p.id, for: c) } } }
            }
        } label: {
            Text(project(choice[c.id])?.name ?? "Choose project").font(.system(size: 11))
        }.menuStyle(.borderlessButton).fixedSize()
    }

    private func pick(_ id: UUID, for c: HistoryImport.Candidate) { choice[c.id] = id; checked.insert(c.id) }

    private func detail(_ c: HistoryImport.Candidate) -> String {
        let home = HistoryImport.home
        let place = c.cwd.isEmpty ? "Folder unknown" : "Started in " + (HistoryImport.contains(home, c.cwd) ? "~" + c.cwd.dropFirst(home.count) : c.cwd)
        switch c.reason {
        case .folder: return place + " · started in this project"
        case .paths(let n): return place + " · \(n) file references"
        case .none: return place
        }
    }

    private func load() async {
        let projects = store.workspace.projects
        let found = await Task.detached(priority: .userInitiated) { HistoryImport.analyze(projects: projects) }.value
        for c in found where c.projectID != nil { choice[c.id] = c.projectID; checked.insert(c.id) }
        candidates = found
    }
}
