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
struct DiffView: View {
    let diff: FileDiff
    let layout: DiffLayout
    let wrap: Bool
    var onDiscardHunk: ((DiffHunk) -> Void)? = nil
    var onLoadAll: (() -> Void)? = nil

    var body: some View {
        if diff.isBinary {
            placeholder("Binary file", detail: diff.isNew ? "New binary file; no text diff." : "Binary contents differ.", icon: "doc.zipper")
        } else if diff.hunks.isEmpty {
            placeholder(diff.isRename ? "Renamed without changes" : "No text changes", detail: diff.isRename ? "\(diff.oldPath ?? "") → \(diff.newPath ?? "")" : "Only the mode or metadata changed.", icon: "doc")
        } else {
            GeometryReader { geo in
                ScrollView(wrap ? [.vertical] : [.vertical, .horizontal]) {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        if layout == .sideBySide { sideBySide } else { unified }
                        if diff.truncated { truncatedNotice }
                    }.frame(minWidth: geo.size.width, minHeight: geo.size.height, alignment: .topLeading).frame(width: wrap ? geo.size.width : nil, alignment: .topLeading)
                }.textSelection(.enabled)
            }
        }
    }

    @ViewBuilder private var unified: some View {
        ForEach(diff.hunks) { hunk in
            hunkHeader(hunk)
            ForEach(hunk.lines) { line in UnifiedRow(line: line, wrap: wrap) }
        }
    }

    @ViewBuilder private var sideBySide: some View {
        ForEach(diff.hunks) { hunk in
            hunkHeader(hunk)
            ForEach(SideBySide.rows(for: hunk)) { row in SplitRow(row: row, wrap: wrap) }
        }
    }

    private func hunkHeader(_ hunk: DiffHunk) -> some View {
        HStack(spacing: 8) {
            Text(hunk.header).font(DiffPalette.font).foregroundStyle(.secondary).lineLimit(1)
            Spacer(minLength: 8)
            if let onDiscardHunk {
                Button { onDiscardHunk(hunk) } label: { Label("Discard hunk", systemImage: "arrow.uturn.backward").font(.system(size: 10)) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).help("Revert only these lines")
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

struct UnifiedRow: View {
    let line: DiffLine
    let wrap: Bool
    private var background: Color {
        switch line.kind { case .added: return DiffPalette.added; case .removed: return DiffPalette.removed; default: return .clear }
    }
    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            Text(line.oldNumber.map(String.init) ?? "").frame(width: 40, alignment: .trailing).foregroundStyle(.tertiary)
            Text(line.newNumber.map(String.init) ?? "").frame(width: 40, alignment: .trailing).foregroundStyle(.tertiary)
            Text(line.kind == .added ? "+" : (line.kind == .removed ? "−" : " ")).frame(width: 16)
                .foregroundStyle(line.kind == .added ? .green : (line.kind == .removed ? .red : .secondary))
            Text(line.kind == .noNewline ? "\\ " + line.text : line.text).lineLimit(wrap ? nil : 1)
                .foregroundStyle(line.kind == .noNewline ? .tertiary : .primary)
                .fixedSize(horizontal: !wrap, vertical: false)
            Spacer(minLength: 8)
        }.font(DiffPalette.font).padding(.vertical, 1).frame(maxWidth: .infinity, alignment: .leading).background(background)
    }
}

struct SplitRow: View {
    let row: SideBySide.Row
    let wrap: Bool
    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            cell(row.left, number: row.left?.oldNumber, removed: true)
            Rectangle().fill(AppTheme.stroke).frame(width: 1)
            cell(row.right, number: row.right?.newNumber, removed: false)
        }.font(DiffPalette.font).frame(maxWidth: .infinity)
    }
    private func cell(_ line: DiffLine?, number: Int?, removed: Bool) -> some View {
        let background: Color = line == nil ? DiffPalette.pad : (line?.kind == .removed ? DiffPalette.removed : (line?.kind == .added ? DiffPalette.added : .clear))
        return HStack(alignment: .top, spacing: 0) {
            Text(number.map(String.init) ?? "").frame(width: 40, alignment: .trailing).foregroundStyle(.tertiary)
            Text(line?.text ?? "").lineLimit(wrap ? nil : 1).padding(.leading, 8).truncationMode(.tail)
            Spacer(minLength: 0)
        }.padding(.vertical, 1).frame(maxWidth: .infinity, alignment: .leading).background(background)
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
