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

    nonisolated static func claudeTitle(_ file: URL) -> String? {
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

    nonisolated static func codexMeta(_ file: URL) -> (id: String, cwd: String, title: String?, isSubagent: Bool)? {
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

extension Store {
    /// Folders whose saved conversations belong to the project: its own, its worktrees, and a group's children.
    func transcriptDirectories(for project: Project) -> [String] {
        var dirs = [project.path] + project.linkedSessions.compactMap(\.workingDirectory)
        if project.isGroup { dirs += workspace.children(of: project.id).map(\.path) }
        return Array(Set(dirs))
    }

    /// Resume a conversation found on disk: link it as a session (or reuse the one already linked) and start it.
    func resumeTranscript(_ entry: TranscriptEntry, in project: Project) {
        if let existing = project.linkedSessions.first(where: { $0.sessionID.lowercased() == entry.sessionID.lowercased() }) {
            if terminals.handles[existing.id]?.running == true { openSession(existing.id, in: project.id) } else { startSession(existing, in: project, resume: true) }
            return
        }
        var session = LinkedSession(agent: entry.agent, sessionID: entry.sessionID, title: String(entry.title.prefix(60)))
        if entry.directory != project.path { session.workingDirectory = entry.directory; session.branch = git.info[entry.directory]?.branch }
        startSession(session, in: project, resume: true)
    }
}

/// A conversation saved on disk that is not a Convoy session yet; resumes on double-click.
struct TranscriptRow: View {
    @EnvironmentObject var store: Store
    let entry: TranscriptEntry
    let project: Project
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 12) {
            AgentIcon(agent: entry.agent, size: 16).frame(width: 26, height: 26)
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
            VStack(alignment: .leading, spacing: 3) {
                Text(entry.title).font(.system(size: 13, weight: .medium)).lineLimit(1)
                HStack(spacing: 6) {
                    Text(entry.agent.rawValue)
                    Text("·").foregroundStyle(.quaternary)
                    Text(entry.date.formatted(date: .abbreviated, time: .shortened))
                    Text("·").foregroundStyle(.quaternary)
                    Text(ByteCountFormatter.string(fromByteCount: Int64(entry.bytes), countStyle: .file))
                    Text("·").foregroundStyle(.quaternary)
                    Text(String(entry.sessionID.prefix(8))).font(.system(size: 10.5, design: .monospaced))
                    if entry.directory != project.path {
                        Text("·").foregroundStyle(.quaternary)
                        Label(URL(fileURLWithPath: entry.directory).lastPathComponent, systemImage: "folder").lineLimit(1)
                    }
                }.font(.system(size: 10.5)).foregroundStyle(.secondary)
            }
            Spacer(minLength: 12)
            Button { store.resumeTranscript(entry, in: project) } label: { Label("Resume", systemImage: "play.fill").font(.system(size: 11, weight: .medium)) }
                .buttonStyle(.bordered).tint(hovered ? AppTheme.accent : nil).controlSize(.small)
            Button { store.copy(entry.sessionID) } label: { Image(systemName: "doc.on.doc").font(.system(size: 11)) }
                .buttonStyle(.plain).foregroundStyle(.secondary).help("Copy session ID").opacity(hovered ? 1 : 0.4)
        }
        .padding(.horizontal, 20).padding(.vertical, 9)
        .background(hovered ? Color.primary.opacity(0.04) : .clear)
        .overlay(alignment: .bottom) { Divider().padding(.leading, 58).opacity(0.6) }
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture(count: 2) { store.resumeTranscript(entry, in: project) }
        .contextMenu {
            Button("Resume") { store.resumeTranscript(entry, in: project) }
            Button("Copy session ID") { store.copy(entry.sessionID) }
            Button("Show folder in Finder") { NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: entry.directory) }
        }
    }
}
