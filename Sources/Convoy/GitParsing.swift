import Foundation

// Pure parsers for git's machine-readable output. No process calls, so everything here is unit-testable.

enum GitFileCode: Character, Sendable, Equatable {
    case unchanged = ".", modified = "M", added = "A", deleted = "D", renamed = "R", copied = "C", typeChanged = "T", unmerged = "U", untracked = "?", ignored = "!"

    init(_ character: Character) { self = GitFileCode(rawValue: character) ?? .modified }
    var label: String {
        switch self {
        case .unchanged: return ""
        case .modified: return "M"; case .added: return "A"; case .deleted: return "D"; case .renamed: return "R"
        case .copied: return "C"; case .typeChanged: return "T"; case .unmerged: return "U"; case .untracked: return "?"; case .ignored: return "!"
        }
    }
}

struct GitFileStatus: Identifiable, Equatable, Sendable {
    let path: String
    var originalPath: String?
    var index: GitFileCode
    var worktree: GitFileCode
    var conflicted = false
    var id: String { path }
    var isUntracked: Bool { worktree == .untracked }
    var hasStaged: Bool { !conflicted && !isUntracked && index != .unchanged }
    var hasUnstaged: Bool { !conflicted && !isUntracked && worktree != .unchanged }
    /// Single code for tree colouring: conflicts win, then untracked, then whichever side changed.
    var displayCode: GitFileCode {
        if conflicted { return .unmerged }
        if isUntracked { return .untracked }
        return worktree != .unchanged ? worktree : index
    }
}

struct GitStatusSnapshot: Equatable, Sendable {
    var branch = ""
    var upstream: String?
    var ahead = 0
    var behind = 0
    var detached = false
    var files: [GitFileStatus] = []
    var staged: [GitFileStatus] { files.filter(\.hasStaged) }
    var unstaged: [GitFileStatus] { files.filter(\.hasUnstaged) }
    var untracked: [GitFileStatus] { files.filter(\.isUntracked) }
    var conflicted: [GitFileStatus] { files.filter(\.conflicted) }
    var isEmpty: Bool { files.isEmpty }
}

/// `git status --porcelain=v2 -z --branch --untracked-files=all`
enum GitStatusParser {
    static func parse(porcelainV2Z text: String) -> GitStatusSnapshot {
        var snapshot = GitStatusSnapshot()
        let records = text.split(separator: "\0", omittingEmptySubsequences: false).map(String.init)
        var i = 0
        while i < records.count {
            let record = records[i]; i += 1
            guard !record.isEmpty else { continue }
            if record.hasPrefix("# ") {
                let parts = record.dropFirst(2).split(separator: " ", maxSplits: 1).map(String.init)
                guard parts.count == 2 else { continue }
                switch parts[0] {
                case "branch.head":
                    if parts[1] == "(detached)" { snapshot.detached = true; snapshot.branch = "detached" } else { snapshot.branch = parts[1] }
                case "branch.upstream": snapshot.upstream = parts[1]
                case "branch.ab":
                    let ab = parts[1].split(separator: " ")
                    snapshot.ahead = Int(ab.first?.dropFirst() ?? "") ?? 0
                    snapshot.behind = Int(ab.last?.dropFirst() ?? "") ?? 0
                default: break
                }
                continue
            }
            let fields = record.split(separator: " ", omittingEmptySubsequences: false).map(String.init)
            switch fields.first {
            case "1":
                // 1 XY sub mH mI mW hH hI path
                guard fields.count >= 9, let xy = fields.dropFirst().first, xy.count == 2 else { continue }
                let path = fields[8...].joined(separator: " ")
                snapshot.files.append(GitFileStatus(path: path, index: GitFileCode(xy.first!), worktree: GitFileCode(xy.last!)))
            case "2":
                // 2 XY sub mH mI mW hH hI Xscore path NUL origPath
                guard fields.count >= 10, let xy = fields.dropFirst().first, xy.count == 2 else { continue }
                let path = fields[9...].joined(separator: " ")
                let original = i < records.count ? records[i] : nil; i += 1
                snapshot.files.append(GitFileStatus(path: path, originalPath: original, index: GitFileCode(xy.first!), worktree: GitFileCode(xy.last!)))
            case "u":
                // u XY sub m1 m2 m3 mW h1 h2 h3 path
                guard fields.count >= 11, let xy = fields.dropFirst().first, xy.count == 2 else { continue }
                let path = fields[10...].joined(separator: " ")
                snapshot.files.append(GitFileStatus(path: path, index: GitFileCode(xy.first!), worktree: GitFileCode(xy.last!), conflicted: true))
            case "?":
                let path = String(record.dropFirst(2))
                snapshot.files.append(GitFileStatus(path: path, index: .unchanged, worktree: .untracked))
            default: continue
            }
        }
        snapshot.files.sort { $0.path.localizedStandardCompare($1.path) == .orderedAscending }
        return snapshot
    }
}

// MARK: Diffs

enum DiffLineKind: Sendable { case context, added, removed, noNewline }

struct DiffLine: Identifiable, Equatable, Sendable {
    let id: Int
    let kind: DiffLineKind
    let text: String
    let oldNumber: Int?
    let newNumber: Int?
}

struct DiffHunk: Identifiable, Equatable, Sendable {
    let id: Int
    let header: String
    let oldStart: Int
    let oldCount: Int
    let newStart: Int
    let newCount: Int
    var lines: [DiffLine] = []
    var additions: Int { lines.filter { $0.kind == .added }.count }
    var deletions: Int { lines.filter { $0.kind == .removed }.count }
}

struct FileDiff: Equatable, Sendable {
    var oldPath: String?
    var newPath: String?
    var isBinary = false
    var isNew = false
    var isDeleted = false
    var isRename = false
    var hunks: [DiffHunk] = []
    var truncated = false
    /// "diff --git", "index", "---", "+++" lines, kept so a hunk can be turned back into a patch.
    var rawHeader: [String] = []
    var additions: Int { hunks.reduce(0) { $0 + $1.additions } }
    var deletions: Int { hunks.reduce(0) { $0 + $1.deletions } }
    var path: String { newPath ?? oldPath ?? "" }
    var lineCount: Int { hunks.reduce(0) { $0 + $1.lines.count } }
}

enum DiffParser {
    static let defaultMaxLines = 5000

    /// Parses `git diff` output with any number of files.
    static func parse(_ text: String, maxLines: Int = defaultMaxLines) -> [FileDiff] {
        var files: [FileDiff] = []
        var current: FileDiff?
        var hunk: DiffHunk?
        var old = 0, new = 0, lineID = 0, hunkID = 0, budget = maxLines
        func closeHunk() { if let h = hunk { current?.hunks.append(h) }; hunk = nil }
        func closeFile() { closeHunk(); if let f = current { files.append(f) }; current = nil }

        var rawLines = text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        if rawLines.last == "" { rawLines.removeLast() }
        for line in rawLines {
            if line.hasPrefix("diff --git ") {
                closeFile()
                current = FileDiff(rawHeader: [line])
                let names = line.dropFirst("diff --git ".count)
                // "a/old b/new" — paths with spaces are rare; take the split at " b/".
                if let range = names.range(of: " b/") {
                    current?.oldPath = String(names[names.startIndex..<range.lowerBound].dropFirst(2))
                    current?.newPath = String(names[range.upperBound...])
                }
                continue
            }
            guard current != nil else { continue }
            if hunk == nil || line.hasPrefix("@@") || line.hasPrefix("diff ") {
                if line.hasPrefix("@@") {
                    closeHunk()
                    guard budget > 0 else { current?.truncated = true; continue }
                    let numbers = parseHunkHeader(line)
                    hunkID += 1
                    hunk = DiffHunk(id: hunkID, header: line, oldStart: numbers.0, oldCount: numbers.1, newStart: numbers.2, newCount: numbers.3)
                    old = numbers.0; new = numbers.2
                    continue
                }
                if line.hasPrefix("Binary files") || line.hasPrefix("GIT binary patch") { current?.isBinary = true; continue }
                if line.hasPrefix("new file mode") { current?.isNew = true }
                else if line.hasPrefix("deleted file mode") { current?.isDeleted = true }
                else if line.hasPrefix("rename from ") { current?.isRename = true; current?.oldPath = String(line.dropFirst("rename from ".count)) }
                else if line.hasPrefix("rename to ") { current?.isRename = true; current?.newPath = String(line.dropFirst("rename to ".count)) }
                else if line.hasPrefix("--- ") { if line == "--- /dev/null" { current?.isNew = true } }
                else if line.hasPrefix("+++ ") { if line == "+++ /dev/null" { current?.isDeleted = true } }
                if hunk == nil { current?.rawHeader.append(line) }
                continue
            }
            if current?.truncated == true { continue }
            if budget <= 0 { current?.truncated = true; closeHunk(); continue }
            lineID += 1; budget -= 1
            if line.hasPrefix("+") { hunk?.lines.append(DiffLine(id: lineID, kind: .added, text: String(line.dropFirst()), oldNumber: nil, newNumber: new)); new += 1 }
            else if line.hasPrefix("-") { hunk?.lines.append(DiffLine(id: lineID, kind: .removed, text: String(line.dropFirst()), oldNumber: old, newNumber: nil)); old += 1 }
            else if line.hasPrefix("\\") { hunk?.lines.append(DiffLine(id: lineID, kind: .noNewline, text: String(line.dropFirst(2)), oldNumber: nil, newNumber: nil)) }
            else { hunk?.lines.append(DiffLine(id: lineID, kind: .context, text: String(line.dropFirst(line.hasPrefix(" ") ? 1 : 0)), oldNumber: old, newNumber: new)); old += 1; new += 1 }
        }
        closeFile()
        return files
    }

    static func parseSingle(_ text: String, maxLines: Int = defaultMaxLines) -> FileDiff? { parse(text, maxLines: maxLines).first }

    /// (oldStart, oldCount, newStart, newCount) from "@@ -a,b +c,d @@ …"; missing counts default to 1.
    static func parseHunkHeader(_ line: String) -> (Int, Int, Int, Int) {
        guard let close = line.range(of: " @@", range: line.index(line.startIndex, offsetBy: 2)..<line.endIndex) else { return (0, 0, 0, 0) }
        let body = line[line.index(line.startIndex, offsetBy: 2)..<close.lowerBound].trimmingCharacters(in: .whitespaces)
        var result = [0, 1, 0, 1]
        for (index, part) in body.split(separator: " ").prefix(2).enumerated() {
            let nums = part.dropFirst().split(separator: ",").compactMap { Int($0) }
            result[index * 2] = nums.first ?? 0
            result[index * 2 + 1] = nums.count > 1 ? nums[1] : 1
        }
        return (result[0], result[1], result[2], result[3])
    }

    /// A patch containing just one hunk, suitable for `git apply --reverse -`.
    static func patch(for file: FileDiff, hunk: DiffHunk) -> String {
        let oldName = file.oldPath ?? file.newPath ?? "file", newName = file.newPath ?? file.oldPath ?? "file"
        var lines = ["diff --git a/\(oldName) b/\(newName)", file.isNew ? "--- /dev/null" : "--- a/\(oldName)", file.isDeleted ? "+++ /dev/null" : "+++ b/\(newName)"]
        lines.append(hunk.header)
        for line in hunk.lines {
            switch line.kind {
            case .added: lines.append("+" + line.text)
            case .removed: lines.append("-" + line.text)
            case .context: lines.append(" " + line.text)
            case .noNewline: lines.append("\\ " + line.text)
            }
        }
        return lines.joined(separator: "\n") + "\n"
    }

    /// An all-added diff for a file git does not know yet.
    static func untrackedDiff(path: String, contents: String) -> FileDiff {
        var lines = contents.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        if lines.last == "" { lines.removeLast() }
        var hunk = DiffHunk(id: 1, header: "@@ -0,0 +1,\(lines.count) @@", oldStart: 0, oldCount: 0, newStart: 1, newCount: lines.count)
        hunk.lines = lines.enumerated().map { DiffLine(id: $0.offset + 1, kind: .added, text: $0.element, oldNumber: nil, newNumber: $0.offset + 1) }
        if !contents.isEmpty && !contents.hasSuffix("\n") { hunk.lines.append(DiffLine(id: lines.count + 1, kind: .noNewline, text: "No newline at end of file", oldNumber: nil, newNumber: nil)) }
        return FileDiff(oldPath: nil, newPath: path, isNew: true, hunks: [hunk])
    }
}

/// Pairs removed and added runs of a hunk for two-column rendering.
enum SideBySide {
    struct Row: Identifiable, Equatable, Sendable {
        let id: Int
        let left: DiffLine?
        let right: DiffLine?
    }

    static func rows(for hunk: DiffHunk) -> [Row] {
        var rows: [Row] = []
        var removed: [DiffLine] = [], added: [DiffLine] = []
        var id = 0
        func flush() {
            for i in 0..<max(removed.count, added.count) {
                id += 1
                rows.append(Row(id: id, left: i < removed.count ? removed[i] : nil, right: i < added.count ? added[i] : nil))
            }
            removed = []; added = []
        }
        for line in hunk.lines {
            switch line.kind {
            case .removed: if !added.isEmpty { flush() }; removed.append(line)
            case .added: added.append(line)
            case .context: flush(); id += 1; rows.append(Row(id: id, left: line, right: line))
            case .noNewline: continue
            }
        }
        flush()
        return rows
    }
}

/// Display helpers: strip the indentation every line shares, and find the changed span inside a
/// removed/added pair so the split and unified views can highlight just what differs.
enum DiffDisplay {
    /// Longest whitespace prefix common to every non-blank line of the diff.
    static func commonIndent(_ diff: FileDiff) -> String {
        var indent: String? = nil
        for hunk in diff.hunks {
            for line in hunk.lines where line.kind != .noNewline {
                let text = line.text
                if text.allSatisfy({ $0 == " " || $0 == "\t" }) { continue }
                let lead = String(text.prefix { $0 == " " || $0 == "\t" })
                guard let current = indent else { indent = lead; continue }
                indent = String(zip(current, lead).prefix { $0 == $1 }.map { $0.0 })
                if indent?.isEmpty == true { return "" }
            }
        }
        return indent ?? ""
    }
    static func strip(_ text: String, indent: String) -> String {
        indent.isEmpty || !text.hasPrefix(indent) ? text : String(text.dropFirst(indent.count))
    }

    /// Character ranges that differ between a removed line and the added line it is paired with.
    struct Span: Equatable { let prefix: Int; let suffix: Int }
    static func changedSpan(_ old: String, _ new: String) -> Span? {
        let a = Array(old), b = Array(new)
        var prefix = 0
        while prefix < a.count, prefix < b.count, a[prefix] == b[prefix] { prefix += 1 }
        var suffix = 0
        while suffix < a.count - prefix, suffix < b.count - prefix, a[a.count - 1 - suffix] == b[b.count - 1 - suffix] { suffix += 1 }
        // No highlight when everything or (almost) nothing is shared: a whole-line change reads better plain.
        let shared = prefix + suffix
        if shared == 0 || (shared * 4 < max(a.count, b.count)) { return nil }
        return Span(prefix: prefix, suffix: suffix)
    }
    /// Spans keyed by DiffLine.id for every removed/added pair in a hunk (same pairing as SideBySide).
    static func spans(for hunk: DiffHunk) -> [Int: Span] {
        var out: [Int: Span] = [:]
        for row in SideBySide.rows(for: hunk) {
            guard let l = row.left, let r = row.right, l.kind == .removed, r.kind == .added, let span = changedSpan(l.text, r.text) else { continue }
            out[l.id] = span; out[r.id] = span
        }
        return out
    }
}

// MARK: Log, branches

struct GitCommit: Identifiable, Equatable, Sendable {
    let sha: String
    let shortSHA: String
    let author: String
    let email: String
    let date: Date
    let subject: String
    let parents: [String]
    let refs: [String]
    var id: String { sha }
    var isMerge: Bool { parents.count > 1 }
}

/// `git log --format=%H%x00%h%x00%an%x00%ae%x00%at%x00%s%x00%P%x00%D%x1e`
enum GitLogParser {
    static func parse(_ text: String) -> [GitCommit] {
        text.split(separator: "\u{1e}").compactMap { record in
            let fields = record.trimmingCharacters(in: .newlines).split(separator: "\0", omittingEmptySubsequences: false).map(String.init)
            guard fields.count >= 8, fields[0].count >= 7 else { return nil }
            let refs = fields[7].split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
                .map { $0.hasPrefix("HEAD -> ") ? String($0.dropFirst(8)) : $0 }
            return GitCommit(sha: fields[0], shortSHA: fields[1], author: fields[2], email: fields[3],
                             date: Date(timeIntervalSince1970: TimeInterval(fields[4]) ?? 0), subject: fields[5],
                             parents: fields[6].split(separator: " ").map(String.init), refs: refs)
        }
    }
}

struct GitCommitFile: Identifiable, Equatable, Sendable {
    let code: GitFileCode
    let path: String
    var originalPath: String?
    var id: String { path }
}

/// `git diff-tree --name-status -z` / `git diff --name-status -z`
enum GitNameStatusParser {
    static func parse(z text: String) -> [GitCommitFile] {
        let fields = text.split(separator: "\0", omittingEmptySubsequences: false).map(String.init)
        var result: [GitCommitFile] = []
        var i = 0
        while i + 1 < fields.count {
            let status = fields[i]; i += 1
            guard let first = status.first else { continue }
            let code = GitFileCode(first)
            if code == .renamed || code == .copied {
                guard i + 1 < fields.count else { break }
                result.append(GitCommitFile(code: code, path: fields[i + 1], originalPath: fields[i])); i += 2
            } else {
                result.append(GitCommitFile(code: code, path: fields[i])); i += 1
            }
        }
        return result
    }
}

struct GitBranch: Identifiable, Equatable, Sendable {
    let name: String
    let isRemote: Bool
    let isCurrent: Bool
    let upstream: String?
    let lastCommit: Date?
    var id: String { name }
    /// "feature/x" for "origin/feature/x".
    var shortName: String { isRemote ? name.split(separator: "/", maxSplits: 1).last.map(String.init) ?? name : name }
}

/// `git for-each-ref --format=%(refname:short)%00%(HEAD)%00%(upstream:short)%00%(committerdate:unix) refs/heads refs/remotes`
enum GitBranchParser {
    static func parse(_ text: String) -> [GitBranch] {
        text.split(separator: "\n").compactMap { line in
            let f = line.split(separator: "\0", omittingEmptySubsequences: false).map(String.init)
            guard f.count >= 4, !f[0].isEmpty, !f[0].hasSuffix("/HEAD") else { return nil }
            let isRemote = f[0].contains("/") && !f[0].hasPrefix("refs/") && remotePrefix(f[0])
            return GitBranch(name: f[0], isRemote: isRemote, isCurrent: f[1] == "*", upstream: f[2].isEmpty ? nil : f[2],
                             lastCommit: TimeInterval(f[3]).map { Date(timeIntervalSince1970: $0) })
        }
    }
    /// for-each-ref cannot tell us the ref namespace via refname:short; callers pass heads then remotes, so we
    /// detect remotes by an "origin/…"-style prefix that is not a local branch with a slash. Heads are listed first,
    /// so a duplicate short name means the second is remote.
    private static func remotePrefix(_ name: String) -> Bool { name.hasPrefix("origin/") || name.hasPrefix("upstream/") }
}

// MARK: File tree

struct FileNode: Identifiable, Equatable, Sendable {
    let path: String
    let name: String
    let isDirectory: Bool
    var children: [FileNode]?
    var id: String { path }
}

enum FileTree {
    /// Nested nodes from root-relative paths. Directories first, then case-insensitive by name.
    static func build(paths: [String]) -> [FileNode] {
        final class Builder { var dirs: [String: Builder] = [:]; var files: Set<String> = [] }
        let root = Builder()
        for path in paths where !path.isEmpty {
            let parts = path.split(separator: "/").map(String.init)
            var node = root
            for part in parts.dropLast() {
                if let next = node.dirs[part] { node = next } else { let next = Builder(); node.dirs[part] = next; node = next }
            }
            if let last = parts.last { node.files.insert(last) }
        }
        func nodes(_ builder: Builder, prefix: String) -> [FileNode] {
            let dirs = builder.dirs.keys.sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }.map { name in
                FileNode(path: prefix + name, name: name, isDirectory: true, children: nodes(builder.dirs[name]!, prefix: prefix + name + "/"))
            }
            let files = builder.files.sorted { $0.localizedCaseInsensitiveCompare($1) == .orderedAscending }.map { FileNode(path: prefix + $0, name: $0, isDirectory: false) }
            return dirs + files
        }
        return nodes(root, prefix: "")
    }

    /// Every ancestor directory of a changed file, so folders can show a change marker.
    static func changedDirectories(_ files: [GitFileStatus]) -> Set<String> {
        var result: Set<String> = []
        for file in files {
            var parts = file.path.split(separator: "/").map(String.init); parts.removeLast()
            var prefix = ""
            for part in parts { prefix += (prefix.isEmpty ? "" : "/") + part; result.insert(prefix) }
        }
        return result
    }

    /// Filters the tree to nodes whose path contains `query`, keeping their ancestors.
    static func filter(_ nodes: [FileNode], query: String) -> [FileNode] {
        guard !query.isEmpty else { return nodes }
        return nodes.compactMap { node in
            if node.isDirectory {
                let kids = filter(node.children ?? [], query: query)
                return kids.isEmpty ? nil : FileNode(path: node.path, name: node.name, isDirectory: true, children: kids)
            }
            return node.path.localizedCaseInsensitiveContains(query) ? node : nil
        }
    }
}

enum TextFileInspector {
    static let maxBytes = 2_000_000
    static let imageExtensions: Set<String> = ["png", "jpg", "jpeg", "gif", "heic", "webp", "tiff", "bmp", "icns"]

    static func isBinary(_ data: Data) -> Bool { data.prefix(8192).contains(0) }
    static func isImage(_ path: String) -> Bool { imageExtensions.contains((path as NSString).pathExtension.lowercased()) }
    static func isMarkdown(_ path: String) -> Bool { ["md", "markdown"].contains((path as NSString).pathExtension.lowercased()) }
}

struct LineCounts: Equatable, Sendable {
    var added: Int
    var removed: Int
}

/// `git diff --numstat -M`: "added\tremoved\tpath" or "added\tremoved\t\0old\0new\0" with -z. Binary shows "-\t-".
enum GitNumstatParser {
    static func parse(_ text: String) -> [String: LineCounts] {
        var result: [String: LineCounts] = [:]
        for line in text.split(separator: "\n") {
            let parts = line.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
            guard parts.count == 3 else { continue }
            var path = String(parts[2])
            if let arrow = path.range(of: " => ") {
                // "dir/{old => new}.txt" or "old => new"
                if let open = path.range(of: "{"), let close = path.range(of: "}") {
                    path = String(path[..<open.lowerBound]) + String(path[arrow.upperBound..<close.lowerBound]) + String(path[close.upperBound...])
                } else { path = String(path[arrow.upperBound...]) }
            }
            result[path] = LineCounts(added: Int(parts[0]) ?? 0, removed: Int(parts[1]) ?? 0)
        }
        return result
    }
}
