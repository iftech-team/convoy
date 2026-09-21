import SwiftUI

/// A conversation the provider saved on disk, whether or not Convoy started it.
struct TranscriptEntry: Identifiable, Equatable {
    let agent: Agent
    let sessionID: String
    let title: String
    let date: Date
    let bytes: Int
    let directory: String
    var id: String { agent.rawValue + sessionID }
}

enum TranscriptScanner {
    static var claudeRoot: URL {
        (ProcessInfo.processInfo.environment["CLAUDE_CONFIG_DIR"].map { URL(fileURLWithPath: $0) } ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude")).appendingPathComponent("projects")
    }
    static var codexRoot: URL { FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".codex/sessions") }

    /// Claude stores `<encoded cwd>/<session>.jsonl`; Codex stores dated rollout files whose first line names the cwd.
    nonisolated static func scan(directories: [String], limit: Int = 60, claudeRoot: URL? = nil, codexRoot: URL? = nil) -> [TranscriptEntry] {
        let claudeRoot = claudeRoot ?? Self.claudeRoot
        let codexRoot = codexRoot ?? Self.codexRoot
        var out: [TranscriptEntry] = []
        let fm = FileManager.default
        for dir in directories {
            let encoded = dir.replacingOccurrences(of: "[^A-Za-z0-9]", with: "-", options: .regularExpression)
            let folder = claudeRoot.appendingPathComponent(encoded)
            for file in (try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.contentModificationDateKey, .fileSizeKey])) ?? [] where file.pathExtension == "jsonl" {
                let attrs = (try? file.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey]))
                let id = file.deletingPathExtension().lastPathComponent
                guard let size = attrs?.fileSize, size > 0 else { continue }
                out.append(TranscriptEntry(agent: .claude, sessionID: id, title: claudeTitle(file) ?? "Session \(id.prefix(8))", date: attrs?.contentModificationDate ?? .distantPast, bytes: size, directory: dir))
            }
        }
        let wanted = Set(directories)
        if let enumerator = fm.enumerator(at: codexRoot, includingPropertiesForKeys: [.contentModificationDateKey, .fileSizeKey], options: [.skipsHiddenFiles]) {
            for case let file as URL in enumerator where file.lastPathComponent.hasPrefix("rollout-") && file.pathExtension == "jsonl" {
                guard let meta = codexMeta(file), wanted.contains(meta.cwd), !meta.isSubagent else { continue }
                let attrs = try? file.resourceValues(forKeys: [.contentModificationDateKey, .fileSizeKey])
                out.append(TranscriptEntry(agent: .codex, sessionID: meta.id, title: meta.title ?? "Codex \(meta.id.prefix(8))", date: attrs?.contentModificationDate ?? .distantPast, bytes: attrs?.fileSize ?? 0, directory: meta.cwd))
            }
        }
        return Array(out.sorted { $0.date > $1.date }.prefix(limit))
    }

    private nonisolated static func head(_ file: URL, bytes: Int = 96_000) -> String? {
        guard let handle = try? FileHandle(forReadingFrom: file) else { return nil }
        defer { try? handle.close() }
        return String(data: (try? handle.read(upToCount: bytes)) ?? Data(), encoding: .utf8)
    }

    private nonisolated static func claudeTitle(_ file: URL) -> String? {
        guard let text = head(file) else { return nil }
        for line in text.split(separator: "\n") {
            guard let data = line.data(using: .utf8), let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  json["type"] as? String == "user", let message = json["message"] as? [String: Any] else { continue }
            var content = ""
            if let s = message["content"] as? String { content = s }
            else if let parts = message["content"] as? [[String: Any]] { content = parts.compactMap { $0["type"] as? String == "text" ? $0["text"] as? String : nil }.joined(separator: " ") }
            let clean = content.replacingOccurrences(of: "<[^>]+>[^<]*</[^>]+>", with: "", options: .regularExpression).trimmingCharacters(in: .whitespacesAndNewlines)
            if clean.isEmpty || clean.hasPrefix("<") { continue }
            return String(clean.prefix(90)).replacingOccurrences(of: "\n", with: " ")
        }
        return nil
    }

    private nonisolated static func codexMeta(_ file: URL) -> (id: String, cwd: String, title: String?, isSubagent: Bool)? {
        guard let text = head(file, bytes: 32_000) else { return nil }
        var id = "", cwd = "", title: String?, sub = false
        for line in text.split(separator: "\n") {
            guard let data = line.data(using: .utf8), let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { continue }
            let type = json["type"] as? String ?? ""
            let payload = json["payload"] as? [String: Any] ?? [:]
            if type == "session_meta" {
                id = payload["id"] as? String ?? ""; cwd = payload["cwd"] as? String ?? ""
                if let source = payload["source"] as? [String: Any], source["subagent"] != nil { sub = true }
                if payload["parent_thread_id"] != nil { sub = true }
            } else if type == "event_msg" || type == "response_item", title == nil {
                if payload["type"] as? String == "user_message", let m = payload["message"] as? String { title = String(m.prefix(90)) }
                if payload["role"] as? String == "user", let parts = payload["content"] as? [[String: Any]] {
                    let t = parts.compactMap { $0["text"] as? String }.joined(separator: " ")
                    if !t.isEmpty, !t.hasPrefix("<") { title = String(t.prefix(90)) }
                }
            }
            if !id.isEmpty, title != nil { break }
        }
        return id.isEmpty ? nil : (id, cwd, title?.replacingOccurrences(of: "\n", with: " "), sub)
    }
}

/// History area: every saved conversation for this project and its worktrees, resumable in one click.
struct HistoryPanel: View {
    @EnvironmentObject var store: Store
    let project: Project
    @State private var entries: [TranscriptEntry] = []
    @State private var loading = false
    @State private var filter = ""

    private var known: Set<String> { Set(project.linkedSessions.map { $0.sessionID.lowercased() }) }
    private var shown: [TranscriptEntry] {
        let q = filter.lowercased().trimmingCharacters(in: .whitespaces)
        return q.isEmpty ? entries : entries.filter { $0.title.lowercased().contains(q) || $0.sessionID.hasPrefix(q) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Conversations saved by Claude Code and Codex for \(project.name)\(project.isGroup ? "" : " and its worktrees"). Resume any of them here, even ones started in a plain terminal.")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                if loading { ProgressView().controlSize(.small) }
                Button { load() } label: { Image(systemName: "arrow.clockwise") }.buttonStyle(.plain).foregroundStyle(.secondary)
            }
            TextField("Filter by first message or ID", text: $filter).textFieldStyle(.roundedBorder)
            if shown.isEmpty && !loading {
                Text(filter.isEmpty ? "No saved conversations found for this folder." : "No matches").font(.caption).foregroundStyle(.secondary).padding(.top, 12)
            }
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(shown) { entry in row(entry) }
                }
                .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(AppTheme.stroke))
            }
        }.padding(20)
            .onAppear(perform: load)
    }

    private func row(_ entry: TranscriptEntry) -> some View {
        let linked = known.contains(entry.sessionID.lowercased())
        return HStack(spacing: 12) {
            Image(systemName: entry.agent == .claude ? "sparkle" : "chevron.left.forwardslash.chevron.right").foregroundStyle(AppTheme.accent).frame(width: 16)
            VStack(alignment: .leading, spacing: 2) {
                Text(entry.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                Text("\(entry.agent.rawValue) · \(entry.date.formatted(date: .abbreviated, time: .shortened)) · \(ByteCountFormatter.string(fromByteCount: Int64(entry.bytes), countStyle: .file)) · \(entry.sessionID.prefix(8))\(entry.directory != project.path ? " · " + URL(fileURLWithPath: entry.directory).lastPathComponent : "")")
                    .font(.system(size: 10.5)).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer()
            if linked { Text("In sidebar").font(.caption).foregroundStyle(.tertiary) }
            Button(linked ? "Open" : "Resume") { resume(entry, linked: linked) }.controlSize(.small)
            Button { store.copy(entry.sessionID) } label: { Image(systemName: "doc.on.doc") }.buttonStyle(.plain).foregroundStyle(.secondary).help("Copy session ID")
        }.padding(.horizontal, 14).padding(.vertical, 8).overlay(alignment: .bottom) { Divider().padding(.leading, 14) }
    }

    private func resume(_ entry: TranscriptEntry, linked: Bool) {
        if linked, let existing = project.linkedSessions.first(where: { $0.sessionID.lowercased() == entry.sessionID.lowercased() }) {
            if store.terminals.handles[existing.id]?.running == true { store.openSession(existing.id, in: project.id) } else { store.startSession(existing, in: project, resume: true) }
            return
        }
        var session = LinkedSession(agent: entry.agent, sessionID: entry.sessionID, title: String(entry.title.prefix(60)))
        if entry.directory != project.path { session.workingDirectory = entry.directory; session.branch = store.git.info[entry.directory]?.branch }
        store.startSession(session, in: project, resume: true)
    }

    private func load() {
        loading = true
        var dirs = [project.path] + project.linkedSessions.compactMap(\.workingDirectory)
        if project.isGroup { dirs += store.workspace.children(of: project.id).map(\.path) }
        let unique = Array(Set(dirs))
        Task.detached(priority: .userInitiated) {
            let found = TranscriptScanner.scan(directories: unique)
            await MainActor.run { entries = found; loading = false }
        }
    }
}
