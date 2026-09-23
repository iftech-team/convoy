import AppKit
import SwiftUI

/// Colours shared by the diff and the file tree.
enum DiffPalette {
    static let added = Color.green.opacity(0.13)
    static let removed = Color.red.opacity(0.13)
    static let hunk = AppTheme.accent.opacity(0.08)
    static let pad = Color.primary.opacity(0.035)
    static let font = Font.system(size: 11, design: .monospaced)

    static func color(for code: GitFileCode) -> Color {
        switch code {
        case .added, .untracked, .copied: return .green
        case .deleted: return .red
        case .unmerged: return .orange
        case .renamed, .typeChanged: return .purple
        case .modified: return AppTheme.accent
        case .unchanged, .ignored: return .secondary
        }
    }
}

/// One file's diff, unified or two-column. Rows are lazy so a 5,000-line diff scrolls fine.
/// Indentation shared by every line is stripped for display, so deeply nested code stays readable in a narrow panel.
struct DiffView: View {
    let diff: FileDiff
    let layout: DiffLayout
    let wrap: Bool
    var onDiscardHunk: ((DiffHunk) -> Void)? = nil
    var onLoadAll: (() -> Void)? = nil
    /// Two columns need room; below this the split request falls back to unified with a note.
    static let splitMinimumWidth: CGFloat = 560

    var body: some View {
        if diff.isBinary {
            placeholder("Binary file", detail: diff.isNew ? "New binary file; no text diff." : "Binary contents differ.", icon: "doc.zipper")
        } else if diff.hunks.isEmpty {
            placeholder(diff.isRename ? "Renamed without changes" : "No text changes", detail: diff.isRename ? "\(diff.oldPath ?? "") → \(diff.newPath ?? "")" : "Only the mode or metadata changed.", icon: "doc")
        } else {
            let indent = DiffDisplay.commonIndent(diff)
            GeometryReader { geo in
                let split = layout == .sideBySide && geo.size.width >= Self.splitMinimumWidth
                let wrapping = wrap || split   // two columns always wrap: there is no room to scroll each one
                ScrollView(wrapping ? [.vertical] : [.vertical, .horizontal]) {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        if layout == .sideBySide && !split { tooNarrow }
                        if !indent.isEmpty { indentNote(indent) }
                        ForEach(diff.hunks) { hunk in
                            hunkHeader(hunk)
                            let spans = DiffDisplay.spans(for: hunk)
                            if split {
                                ForEach(SideBySide.rows(for: hunk)) { row in SplitRow(row: row, indent: indent, spans: spans) }
                            } else {
                                ForEach(hunk.lines) { line in UnifiedRow(line: line, wrap: wrapping, indent: indent, span: spans[line.id]) }
                            }
                        }
                        if diff.truncated { truncatedNotice }
                    }.frame(minWidth: geo.size.width, minHeight: geo.size.height, alignment: .topLeading).frame(width: wrapping ? geo.size.width : nil, alignment: .topLeading)
                }.textSelection(.enabled)
            }
        }
    }

    private var tooNarrow: some View {
        HStack(spacing: 6) {
            Image(systemName: "rectangle.split.2x1").font(.system(size: 10))
            Text("Showing unified diff. Widen or maximize for two columns.").font(.system(size: 10.5)).fixedSize(horizontal: false, vertical: true)
        }.foregroundStyle(.secondary).padding(.horizontal, 10).padding(.vertical, 5).frame(maxWidth: .infinity, alignment: .leading).background(Color.orange.opacity(0.08))
    }

    private func indentNote(_ indent: String) -> some View {
        let tabs = indent.filter { $0 == "\t" }.count, spaces = indent.count - tabs
        let what = [tabs > 0 ? "\(tabs) tab\(tabs == 1 ? "" : "s")" : nil, spaces > 0 ? "\(spaces) space\(spaces == 1 ? "" : "s")" : nil].compactMap { $0 }.joined(separator: " + ")
        return Text("Common indentation of \(what) hidden").font(.system(size: 9.5)).foregroundStyle(.tertiary)
            .padding(.horizontal, 10).padding(.vertical, 3).frame(maxWidth: .infinity, alignment: .leading)
    }

    private func hunkHeader(_ hunk: DiffHunk) -> some View {
        HStack(spacing: 8) {
            Text(hunk.header).font(DiffPalette.font).foregroundStyle(.secondary).lineLimit(1).truncationMode(.tail)
            Spacer(minLength: 8)
            if let onDiscardHunk {
                Button { onDiscardHunk(hunk) } label: { Label("Discard", systemImage: "arrow.uturn.backward").font(.system(size: 10)) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).help("Revert only these lines").fixedSize()
            }
        }.padding(.horizontal, 8).padding(.vertical, 3).frame(maxWidth: .infinity, alignment: .leading).background(DiffPalette.hunk)
    }

    private var truncatedNotice: some View {
        HStack(spacing: 8) {
            Image(systemName: "ellipsis.circle").foregroundStyle(.secondary)
            Text("Showing the first \(DiffParser.defaultMaxLines.formatted()) lines.").font(.system(size: 11)).foregroundStyle(.secondary)
            if let onLoadAll { Button("Load all", action: onLoadAll).controlSize(.small) }
        }.padding(10).frame(maxWidth: .infinity, alignment: .leading)
    }

    private func placeholder(_ title: String, detail: String, icon: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: icon).font(.system(size: 22)).foregroundStyle(.tertiary)
            Text(title).font(.system(size: 13, weight: .medium))
            Text(detail).font(.system(size: 11)).foregroundStyle(.secondary).multilineTextAlignment(.center).lineLimit(2).truncationMode(.middle)
        }.padding(24).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Line text with the changed span (if any) emphasised.
enum DiffText {
    static func attributed(_ text: String, kind: DiffLineKind, span: DiffDisplay.Span?) -> AttributedString {
        var result = AttributedString(text)
        guard let span, kind == .added || kind == .removed, text.count > span.prefix + span.suffix else { return result }
        let start = result.index(result.startIndex, offsetByCharacters: span.prefix)
        let end = result.index(result.endIndex, offsetByCharacters: -span.suffix)
        result[start..<end].backgroundColor = kind == .added ? Color.green.opacity(0.32) : Color.red.opacity(0.3)
        return result
    }
}

struct UnifiedRow: View {
    let line: DiffLine
    let wrap: Bool
    var indent = ""
    var span: DiffDisplay.Span? = nil
    private var background: Color {
        switch line.kind { case .added: return DiffPalette.added; case .removed: return DiffPalette.removed; default: return .clear }
    }
    var body: some View {
        let shown = line.kind == .noNewline ? "\\ " + line.text : DiffDisplay.strip(line.text, indent: indent)
        let adjusted = span.map { DiffDisplay.Span(prefix: max(0, $0.prefix - (line.text.count - shown.count)), suffix: $0.suffix) }
        HStack(alignment: .top, spacing: 0) {
            Text(line.oldNumber.map(String.init) ?? "").frame(width: 34, alignment: .trailing).foregroundStyle(.tertiary)
            Text(line.newNumber.map(String.init) ?? "").frame(width: 34, alignment: .trailing).foregroundStyle(.tertiary)
            Text(line.kind == .added ? "+" : (line.kind == .removed ? "−" : " ")).frame(width: 14)
                .foregroundStyle(line.kind == .added ? .green : (line.kind == .removed ? .red : .secondary))
            Text(DiffText.attributed(shown, kind: line.kind, span: adjusted)).lineLimit(wrap ? nil : 1)
                .foregroundStyle(line.kind == .noNewline ? .tertiary : .primary)
                .fixedSize(horizontal: !wrap, vertical: false)
            Spacer(minLength: 8)
        }.font(DiffPalette.font).padding(.vertical, 1).frame(maxWidth: .infinity, alignment: .leading).background(background)
    }
}

/// Two-column row: old on the left, new on the right, equal widths, lines wrapped, changed spans emphasised.
struct SplitRow: View {
    let row: SideBySide.Row
    var indent = ""
    var spans: [Int: DiffDisplay.Span] = [:]
    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            cell(row.left, number: row.left?.oldNumber)
            Rectangle().fill(AppTheme.stroke).frame(width: 1)
            cell(row.right, number: row.right?.newNumber)
        }.font(DiffPalette.font).frame(maxWidth: .infinity).fixedSize(horizontal: false, vertical: true)
    }
    private func cell(_ line: DiffLine?, number: Int?) -> some View {
        let background: Color = line == nil ? DiffPalette.pad : (line?.kind == .removed ? DiffPalette.removed : (line?.kind == .added ? DiffPalette.added : .clear))
        let shown = line.map { DiffDisplay.strip($0.text, indent: indent) } ?? ""
        let span = line.flatMap { l in spans[l.id].map { DiffDisplay.Span(prefix: max(0, $0.prefix - (l.text.count - shown.count)), suffix: $0.suffix) } }
        return HStack(alignment: .top, spacing: 0) {
            Text(number.map(String.init) ?? "").frame(width: 34, alignment: .trailing).foregroundStyle(.tertiary)
            Text(line.map { DiffText.attributed(shown, kind: $0.kind, span: span) } ?? AttributedString(""))
                .padding(.leading, 6).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }.padding(.vertical, 1).frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading).background(background)
    }
}

/// Read-only viewer for a file from the tree: text with line numbers, rendered Markdown, or an image.
struct FileViewerView: View {
    let content: FileContent
    var onOpenExternally: (() -> Void)? = nil
    @State private var raw = false
    @State private var showAll = false
    private static let lineCap = 20_000

    var body: some View {
        switch content.body {
        case .text(let lines):
            GeometryReader { geo in
            ScrollView([.vertical, .horizontal]) {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(Array((showAll ? lines : Array(lines.prefix(Self.lineCap))).enumerated()), id: \.offset) { index, line in
                        HStack(alignment: .top, spacing: 0) {
                            Text("\(index + 1)").frame(width: 48, alignment: .trailing).foregroundStyle(.tertiary).padding(.trailing, 12)
                            Text(line).fixedSize(horizontal: true, vertical: false)
                            Spacer(minLength: 8)
                        }.font(DiffPalette.font).padding(.vertical, 0.5)
                    }
                    if !showAll && lines.count > Self.lineCap {
                        HStack { Text("Showing the first \(Self.lineCap.formatted()) of \(lines.count.formatted()) lines.").font(.system(size: 11)).foregroundStyle(.secondary); Button("Show all") { showAll = true }.controlSize(.small) }.padding(10)
                    }
                }.padding(.vertical, 6).frame(minWidth: geo.size.width, minHeight: geo.size.height, alignment: .topLeading)
            }.textSelection(.enabled)
            }
        case .markdown(let text):
            VStack(spacing: 0) {
                HStack {
                    Spacer()
                    Picker("", selection: $raw) { Text("Rendered").tag(false); Text("Raw").tag(true) }.pickerStyle(.segmented).labelsHidden().controlSize(.small).frame(width: 140)
                }.padding(.horizontal, 10).padding(.vertical, 6)
                if raw { FileViewerView(content: FileContent(path: content.path, body: .text(text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)))) }
                else { ScrollView { MarkdownView(text: text).padding(20).frame(maxWidth: 820, alignment: .leading).frame(maxWidth: .infinity, alignment: .leading) } }
            }
        case .image(let url):
            ScrollView([.vertical, .horizontal]) {
                if let image = NSImage(contentsOf: url) {
                    Image(nsImage: image).resizable().aspectRatio(contentMode: .fit).frame(maxWidth: 1200, maxHeight: 1200).padding(20)
                } else { placeholder("Couldn’t load image", detail: url.lastPathComponent, icon: "photo") }
            }
        case .binary(let bytes):
            placeholder("Binary file", detail: ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file), icon: "doc.zipper")
        case .tooLarge(let bytes):
            placeholder("File is too large to preview", detail: ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file) + " · limit \(ByteCountFormatter.string(fromByteCount: Int64(TextFileInspector.maxBytes), countStyle: .file))", icon: "doc.text.magnifyingglass")
        case .missing:
            placeholder("File not on disk", detail: "It may be deleted or not checked out.", icon: "doc.badge.ellipsis")
        }
    }

    private func placeholder(_ title: String, detail: String, icon: String) -> some View {
        VStack(spacing: 10) {
            Image(systemName: icon).font(.system(size: 24)).foregroundStyle(.tertiary)
            Text(title).font(.system(size: 13, weight: .medium))
            Text(detail).font(.system(size: 11)).foregroundStyle(.secondary).lineLimit(2)
            if let onOpenExternally { Button("Open in default app", action: onOpenExternally).controlSize(.small) }
        }.padding(24).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
