import AppKit
import SwiftUI

/// Files & Changes: a JetBrains-style source-control tool window beside the terminal.
struct GitPanelView: View {
    @EnvironmentObject var store: Store
    @ObservedObject var model: GitPanelModel
    @State private var newBranch = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if let busy = model.busy {
                HStack(spacing: 6) { ProgressView().controlSize(.mini); Text(busy).font(.system(size: 10.5)).foregroundStyle(.secondary); Spacer() }
                    .padding(.horizontal, 10).frame(height: 22).background(AppTheme.raised.opacity(0.6))
                Divider()
            }
            if !model.resolved {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                switch model.tab {
                case .changes: if model.isRepo { GitChangesTab(model: model) } else { notRepo }
                case .files: GitFilesTab(model: model)
                case .log: if model.isRepo { GitLogTab(model: model) } else { notRepo }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity).background(AppTheme.window)
        .onAppear { model.activate() }
        .onDisappear { model.deactivate() }
        .onReceive(store.git.$info) { _ in model.refresh() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in model.refresh() }
        .sheet(isPresented: $newBranch) { NewBranchSheet(model: model) }
    }

    /// One row when there is room, otherwise the section picker on its own line and the chips below it.
    private var header: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) { sectionPicker; Spacer(minLength: 4); chips; controls }
                .padding(.horizontal, 10).frame(height: 36).background(AppTheme.raised)
            VStack(spacing: 4) {
                HStack(spacing: 8) { sectionPicker; Spacer(minLength: 4); controls }
                HStack(spacing: 8) { chips; Spacer(minLength: 0) }
            }.padding(.horizontal, 10).padding(.vertical, 6).background(AppTheme.raised)
        }
    }

    private var sectionPicker: some View {
        Picker("Section", selection: $model.tab) { ForEach(GitPanelTab.allCases) { Text($0.rawValue).tag($0) } }
            .pickerStyle(.segmented).labelsHidden().controlSize(.small).frame(width: 190)
    }

    @ViewBuilder private var controls: some View {
        if model.refreshing { ProgressView().controlSize(.mini) }
        Button { model.refresh(); model.loadBranches() } label: { Image(systemName: "arrow.clockwise").font(.system(size: 11, weight: .medium)).frame(width: 22, height: 22) }
            .buttonStyle(.plain).foregroundStyle(.secondary).help("Refresh").accessibilityLabel("Refresh git status")
        Button { store.showGitPanel = false } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).frame(width: 22, height: 22) }
            .buttonStyle(.plain).foregroundStyle(.secondary).help("Close (\(store.keys.display("git.panel")))").accessibilityLabel("Close git panel")
    }

    @ViewBuilder private var chips: some View {
        HStack(spacing: 6) {
            if let base = store.gitPanelBase, !FileManager.default.fileExists(atPath: base + "/.git") {
                let candidates = store.gitRepoCandidates(under: base)
                if !candidates.isEmpty {
                    Menu {
                        ForEach(candidates, id: \.self) { path in
                            Button { store.gitPanelRepoChoice[base] = path } label: {
                                if path == model.directory { Label(URL(fileURLWithPath: path).lastPathComponent, systemImage: "checkmark") } else { Text(URL(fileURLWithPath: path).lastPathComponent) }
                            }
                        }
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "shippingbox").font(.system(size: 10))
                            Text(URL(fileURLWithPath: model.directory).lastPathComponent).font(.system(size: 11, weight: .medium)).lineLimit(1)
                            Image(systemName: "chevron.up.chevron.down").font(.system(size: 8, weight: .semibold)).foregroundStyle(.secondary)
                        }.padding(.horizontal, 7).frame(height: 22).background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 6))
                    }.menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                        .help("This folder is a group; choose which repository the panel shows")
                }
            }
            if model.isRepo { BranchChip(model: model, newBranch: $newBranch) }
        }
    }

    private var notRepo: some View {
        VStack(spacing: 10) {
            Image(systemName: "arrow.triangle.branch").font(.system(size: 22, weight: .medium)).foregroundStyle(.tertiary)
            Text("Not a git repository").font(.system(size: 13, weight: .semibold))
            Text("Convoy shows changes for a single repository. This folder has no repositories inside it, so only the Files tab is available.")
                .font(.system(size: 11.5)).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 260)
            Text(model.directory).font(.system(size: 10, design: .monospaced)).foregroundStyle(.tertiary).lineLimit(1).truncationMode(.middle)
        }.padding(24).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// Shown when nothing on screen has a folder (Home).
struct GitPanelEmpty: View {
    @EnvironmentObject var store: Store
    var body: some View {
        VStack(spacing: 10) {
            HStack {
                Spacer()
                Button { store.showGitPanel = false } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).frame(width: 22, height: 22) }.buttonStyle(.plain).foregroundStyle(.secondary)
            }.padding(.horizontal, 10).frame(height: 36).background(AppTheme.raised)
            Spacer()
            Image(systemName: "sidebar.right").font(.system(size: 22)).foregroundStyle(.tertiary)
            Text("Files & Changes").font(.system(size: 13, weight: .semibold))
            Text("Open a project or a session to browse its files and git changes here.").font(.system(size: 11.5)).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 240)
            Spacer()
        }.frame(maxWidth: .infinity, maxHeight: .infinity).background(AppTheme.window)
    }
}

// MARK: Branch chip

struct BranchChip: View {
    @ObservedObject var model: GitPanelModel
    @Binding var newBranch: Bool
    private var status: GitStatusSnapshot? { model.status }

    var body: some View {
        Menu {
            Section {
                Button { model.fetch() } label: { Label("Fetch", systemImage: "arrow.down.circle") }
                Button { model.pull() } label: { Label("Pull (fast-forward)", systemImage: "arrow.down.to.line") }
                Button { model.push() } label: { Label(status.map { $0.ahead > 0 ? "Push \($0.ahead) commit\($0.ahead == 1 ? "" : "s")" : "Push" } ?? "Push", systemImage: "arrow.up.to.line") }.disabled(status?.detached == true)
            }
            let locals = model.branches.filter { !$0.isRemote }, remotes = model.branches.filter(\.isRemote)
            if !locals.isEmpty {
                Section("Branches") { ForEach(locals) { branch in Button { model.checkout(branch) } label: { Label(branch.name, systemImage: branch.isCurrent ? "checkmark" : "arrow.triangle.branch") }.disabled(branch.isCurrent) } }
            }
            if !remotes.isEmpty {
                Section("Remote") { ForEach(remotes.prefix(60)) { branch in Button { model.checkout(branch) } label: { Label(branch.name, systemImage: "cloud") } } }
            }
            if model.branches.isEmpty { Text("Loading branches…") }
            Divider()
            Button { newBranch = true } label: { Label("New branch…", systemImage: "plus") }
            Button { model.openPullRequest() } label: { Label("Open pull request (gh)", systemImage: "arrow.triangle.pull") }
        } label: {
            HStack(spacing: 5) {
                Image(systemName: "arrow.triangle.branch").font(.system(size: 10, weight: .semibold))
                Text(status?.branch ?? "…").font(.system(size: 11.5, weight: .medium)).lineLimit(1)
                if let s = status, s.ahead > 0 { Text("↑\(s.ahead)").font(.system(size: 10, weight: .semibold)) }
                if let s = status, s.behind > 0 { Text("↓\(s.behind)").font(.system(size: 10, weight: .semibold)) }
                Image(systemName: "chevron.down").font(.system(size: 8, weight: .bold)).opacity(0.7)
            }.foregroundStyle(AppTheme.accent).padding(.horizontal, 8).padding(.vertical, 4)
                .background(AppTheme.accent.opacity(0.12), in: Capsule()).contentShape(Capsule())
        }.menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().frame(maxWidth: 220)
            .help(status.map { "\($0.branch)\($0.upstream.map { " → " + $0 } ?? "") · \(model.directory)" } ?? model.directory)
            .disabled(model.busy != nil)
    }
}

struct NewBranchSheet: View {
    @ObservedObject var model: GitPanelModel
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var start = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("New branch").font(.system(size: 18, weight: .bold))
            VStack(alignment: .leading, spacing: 6) {
                Text("NAME").font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
                TextField("feature/thing", text: $name).textFieldStyle(.roundedBorder).font(.system(size: 12.5, design: .monospaced))
                    .onSubmit { create() }
            }
            VStack(alignment: .leading, spacing: 6) {
                Text("CREATE FROM").font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
                Picker("", selection: $start) {
                    Text("Current branch (\(model.status?.branch ?? "HEAD"))").tag("")
                    ForEach(model.branches) { Text($0.name).tag($0.name) }
                }.labelsHidden()
            }
            Text("Switches to the new branch. Uncommitted changes come along.").font(.caption).foregroundStyle(.tertiary)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Create branch") { create() }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
                    .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty || name.contains(" "))
            }
        }.padding(24).frame(width: 440)
    }
    private func create() {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        model.createBranch(name, from: start.isEmpty ? nil : start); dismiss()
    }
}

// MARK: Changes tab

struct GitChangesTab: View {
    @EnvironmentObject var store: Store
    @ObservedObject var model: GitPanelModel
    @State private var confirmDiscard: GitFileStatus?
    @State private var discardIncludesStaged = false
    @State private var confirmDelete: GitFileStatus?
    @State private var confirmHunk: DiffHunk?

    var body: some View {
        Group {
            if model.maximizeDiff {
                GitDiffArea(model: model, discard: requestDiscard, discardHunk: { confirmHunk = $0 })
            } else {
                EvenSplit(axis: .vertical, key: "git.changes", minimumFirst: 170, minimumSecond: 120, defaultFraction: 0.42) {
                    VStack(spacing: 0) {
                        list
                        Divider()
                        CommitComposer(model: model)
                    }
                } second: {
                    GitDiffArea(model: model, discard: requestDiscard, discardHunk: { confirmHunk = $0 })
                }
            }
        }
        .alert("Discard changes in \(confirmDiscard?.path ?? "")?", isPresented: Binding(get: { confirmDiscard != nil }, set: { if !$0 { confirmDiscard = nil } }), presenting: confirmDiscard) { file in
            Button("Cancel", role: .cancel) { confirmDiscard = nil }
            Button("Discard", role: .destructive) { model.discard(file, includeStaged: discardIncludesStaged); confirmDiscard = nil }
        } message: { file in
            Text(discardIncludesStaged ? "Staged and unstaged edits are reset to the last commit. This cannot be undone." : "Unstaged edits are reset with git restore. Staged changes stay. This cannot be undone.")
        }
        .alert("Move \(confirmDelete?.path ?? "") to the Trash?", isPresented: Binding(get: { confirmDelete != nil }, set: { if !$0 { confirmDelete = nil } }), presenting: confirmDelete) { file in
            Button("Cancel", role: .cancel) { confirmDelete = nil }
            Button("Move to Trash", role: .destructive) { model.deleteUntracked([file.path]); confirmDelete = nil }
        } message: { _ in Text("The file is untracked, so git cannot restore it. It goes to the Trash rather than being deleted.") }
        .alert("Discard this hunk?", isPresented: Binding(get: { confirmHunk != nil }, set: { if !$0 { confirmHunk = nil } }), presenting: confirmHunk) { hunk in
            Button("Cancel", role: .cancel) { confirmHunk = nil }
            Button("Discard hunk", role: .destructive) { model.discardHunk(hunk); confirmHunk = nil }
        } message: { hunk in Text("Reverts only the lines in \(hunk.header.split(separator: "@@").dropFirst().first?.trimmingCharacters(in: .whitespaces) ?? "this hunk"). This cannot be undone.") }
    }

    private func requestDiscard(_ file: GitFileStatus, includeStaged: Bool) {
        if file.isUntracked { confirmDelete = file } else { discardIncludesStaged = includeStaged; confirmDiscard = file }
    }

    @ViewBuilder private var list: some View {
        if let status = model.status, !status.isEmpty {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 1) {
                    group("Conflicts", files: status.conflicted, kind: .conflicted, action: ("Mark all resolved", { model.stage(status.conflicted.map(\.path)) }))
                    group("Staged", files: status.staged, kind: .staged, action: ("Unstage all", { model.unstage(status.staged.map(\.path)) }))
                    group("Unstaged", files: status.unstaged, kind: .unstaged, action: ("Stage all", { model.stage(status.unstaged.map(\.path)) }))
                    group("Untracked", files: status.untracked, kind: .untracked, action: ("Stage all", { model.stage(status.untracked.map(\.path)) }))
                }.padding(.horizontal, 6).padding(.vertical, 4)
            }
        } else {
            VStack(spacing: 8) {
                Image(systemName: "checkmark.circle").font(.system(size: 22)).foregroundStyle(.green.opacity(0.8))
                Text("Working tree clean").font(.system(size: 12.5, weight: .medium))
                Text("Edits made by the agent or you show up here as they happen.").font(.system(size: 11)).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 240)
            }.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    enum GroupKind { case conflicted, staged, unstaged, untracked }

    @ViewBuilder private func group(_ title: String, files: [GitFileStatus], kind: GroupKind, action: (String, () -> Void)) -> some View {
        if !files.isEmpty {
            HStack(spacing: 6) {
                Text(title.uppercased()).font(.system(size: 9.5, weight: .semibold)).tracking(0.5).foregroundStyle(kind == .conflicted ? .orange : .secondary)
                Text("\(files.count)").font(.system(size: 9.5, weight: .semibold)).foregroundStyle(.tertiary)
                Spacer()
                Button(action.0, action: action.1).buttonStyle(.plain).font(.system(size: 10.5, weight: .medium)).foregroundStyle(AppTheme.accent).disabled(model.busy != nil)
            }.padding(.horizontal, 8).padding(.top, 8).padding(.bottom, 3)
            ForEach(files) { file in
                let target = target(for: file, kind: kind)
                ChangeRow(file: file, kind: kind, counts: model.counts[target], selected: model.target == target, busy: model.busy != nil,
                          select: { model.target = target }, toggle: { model.toggleStaged(file) })
                    .contextMenu { rowMenu(file, kind: kind) }
            }
        }
    }

    private func target(for file: GitFileStatus, kind: GroupKind) -> DiffTarget {
        switch kind {
        case .conflicted: return .conflicted(file.path)
        case .staged: return .staged(file.path)
        case .unstaged: return .unstaged(file.path)
        case .untracked: return .untracked(file.path)
        }
    }

    @ViewBuilder private func rowMenu(_ file: GitFileStatus, kind: GroupKind) -> some View {
        switch kind {
        case .conflicted: Button { model.markResolved(file.path) } label: { Label("Mark resolved", systemImage: "checkmark") }
        case .staged: Button { model.unstage([file.path]) } label: { Label("Unstage", systemImage: "minus.square") }
        case .unstaged, .untracked: Button { model.stage([file.path]) } label: { Label("Stage", systemImage: "plus.square") }
        }
        Divider()
        if kind == .untracked {
            Button(role: .destructive) { requestDiscard(file, includeStaged: false) } label: { Label("Move to Trash…", systemImage: "trash") }
        } else if kind == .unstaged {
            Button(role: .destructive) { requestDiscard(file, includeStaged: false) } label: { Label("Discard unstaged changes…", systemImage: "arrow.uturn.backward") }
            if file.hasStaged { Button(role: .destructive) { requestDiscard(file, includeStaged: true) } label: { Label("Discard all changes…", systemImage: "arrow.uturn.backward.circle") } }
        } else if kind == .staged {
            Button(role: .destructive) { requestDiscard(file, includeStaged: true) } label: { Label("Discard all changes…", systemImage: "arrow.uturn.backward.circle") }
        }
        Divider()
        Button { model.tab = .files; model.selectedFile = file.path } label: { Label("Show in Files", systemImage: "folder") }
        Button { model.openExternally(file.path) } label: { Label("Open in default app", systemImage: "arrow.up.forward.app") }
        Button { model.revealInFinder(file.path) } label: { Label("Show in Finder", systemImage: "finder") }
        Button { store.copy(model.absolutePath(file.path)) } label: { Label("Copy path", systemImage: "doc.on.doc") }
    }
}

struct ChangeRow: View {
    let file: GitFileStatus
    let kind: GitChangesTab.GroupKind
    let counts: LineCounts?
    let selected: Bool
    let busy: Bool
    let select: () -> Void
    let toggle: () -> Void
    @State private var hovered = false

    private var code: GitFileCode {
        switch kind { case .staged: return file.index; case .unstaged: return file.worktree; case .untracked: return .untracked; case .conflicted: return .unmerged }
    }
    private var name: String { (file.path as NSString).lastPathComponent }
    private var folder: String { (file.path as NSString).deletingLastPathComponent }

    var body: some View {
        HStack(spacing: 7) {
            Button(action: toggle) {
                Image(systemName: kind == .conflicted ? "exclamationmark.triangle.fill" : (kind == .staged ? "checkmark.square.fill" : "square"))
                    .font(.system(size: 12)).foregroundStyle(kind == .conflicted ? .orange : (kind == .staged ? AppTheme.accent : .secondary)).frame(width: 16)
            }.buttonStyle(.plain).disabled(busy).help(kind == .conflicted ? "Mark resolved" : (kind == .staged ? "Unstage" : "Stage"))
            Text(code.label).font(.system(size: 9, weight: .bold, design: .monospaced)).foregroundStyle(DiffPalette.color(for: code)).frame(width: 12)
            HStack(spacing: 5) {
                if file.originalPath != nil { Text((file.originalPath! as NSString).lastPathComponent).font(.system(size: 11.5)).foregroundStyle(.secondary).strikethrough(); Text("→").font(.system(size: 9)).foregroundStyle(.tertiary) }
                Text(name).font(.system(size: 11.5, weight: selected ? .medium : .regular)).lineLimit(1)
                    .foregroundStyle(code == .deleted ? .secondary : .primary).strikethrough(code == .deleted)
                if !folder.isEmpty { Text(folder).font(.system(size: 10)).foregroundStyle(.tertiary).lineLimit(1).truncationMode(.middle) }
            }
            Spacer(minLength: 4)
            if let counts {
                HStack(spacing: 4) {
                    if counts.added > 0 { Text("+\(counts.added)").foregroundStyle(.green) }
                    if counts.removed > 0 { Text("−\(counts.removed)").foregroundStyle(.red) }
                }.font(.system(size: 9.5, design: .monospaced))
            }
        }
        .padding(.horizontal, 8).frame(height: 26).contentShape(Rectangle())
        .background(selected ? AppTheme.accent.opacity(0.14) : (hovered ? Color.primary.opacity(0.05) : .clear), in: RoundedRectangle(cornerRadius: 6))
        .onHover { hovered = $0 }
        .onTapGesture(perform: select)
        .help(file.path)
        .accessibilityElement(children: .combine).accessibilityAddTraits(selected ? .isSelected : [])
    }
}

struct CommitComposer: View {
    @ObservedObject var model: GitPanelModel
    private var stagedCount: Int { model.status?.staged.count ?? 0 }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ZStack(alignment: .topLeading) {
                TextEditor(text: $model.message).font(.system(size: 12, design: .monospaced)).scrollContentBackground(.hidden)
                    .frame(minHeight: 44, maxHeight: 88).padding(4)
                if model.message.isEmpty {
                    Text(model.amend ? "Amended message" : "Commit message").font(.system(size: 12, design: .monospaced)).foregroundStyle(.tertiary).padding(.horizontal, 9).padding(.top, 5).allowsHitTesting(false)
                }
            }.background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7)).overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(AppTheme.stroke))
            HStack(spacing: 8) {
                Toggle("Amend", isOn: $model.amend).toggleStyle(.checkbox).controlSize(.small).font(.system(size: 11)).help("Replace the last commit")
                Button { model.generateMessage() } label: { Label("Generate", systemImage: "sparkles") }.controlSize(.small)
                    .disabled(model.busy != nil || (model.status?.isEmpty ?? true)).help("Ask Claude for a commit message from the diff")
                Spacer()
                Button("Commit") { model.commit(andPush: false) }.buttonStyle(.borderedProminent).controlSize(.small)
                    .keyboardShortcut(.return, modifiers: .command).disabled(!model.canCommit)
                    .help(stagedCount == 0 && !model.amend ? "Stage files first (⌘↩)" : "Commit \(stagedCount) staged file\(stagedCount == 1 ? "" : "s") (⌘↩)")
                Menu {
                    Button { model.commit(andPush: true) } label: { Label("Commit & Push", systemImage: "arrow.up.to.line") }.disabled(!model.canCommit)
                    Button { model.push() } label: { Label("Push", systemImage: "arrow.up.circle") }
                    Divider()
                    Button { model.fetch() } label: { Label("Fetch", systemImage: "arrow.down.circle") }
                    Button { model.pull() } label: { Label("Pull (fast-forward)", systemImage: "arrow.down.to.line") }
                    Divider()
                    Button { model.openPullRequest() } label: { Label("Open pull request (gh)", systemImage: "arrow.triangle.pull") }
                } label: { Image(systemName: "chevron.down").font(.system(size: 9, weight: .bold)).frame(width: 18, height: 18) }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().disabled(model.busy != nil).help("More: Commit & Push, Push, Fetch, Pull")
            }
            if let result = model.lastResult {
                HStack(spacing: 6) {
                    Image(systemName: result.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill").foregroundStyle(result.ok ? .green : .orange)
                    Text(result.text.split(separator: "\n").first.map(String.init) ?? result.text).lineLimit(1).truncationMode(.tail).textSelection(.enabled)
                    Spacer()
                }.font(.system(size: 10.5)).foregroundStyle(.secondary).help(result.text)
            }
        }.padding(10).background(AppTheme.raised)
    }
}

// MARK: Diff area

struct GitDiffArea: View {
    @ObservedObject var model: GitPanelModel
    let discard: (GitFileStatus, Bool) -> Void
    let discardHunk: (DiffHunk) -> Void
    @AppStorage("git.diffLayout") private var layoutRaw = DiffLayout.unified.rawValue
    @AppStorage("git.diffWrap") private var wrap = false
    private var layout: DiffLayout { DiffLayout(rawValue: layoutRaw) ?? .unified }
    private var file: GitFileStatus? { model.target.flatMap { t in model.status?.files.first { $0.path == t.path } } }

    var body: some View {
        VStack(spacing: 0) {
            if let target = model.target {
                HStack(spacing: 6) {
                    if let code = file?.displayCode ?? model.diff.map({ $0.isNew ? .added : ($0.isDeleted ? .deleted : ($0.isRename ? .renamed : .modified)) }) {
                        Text(code.label).font(.system(size: 9, weight: .bold, design: .monospaced)).foregroundStyle(DiffPalette.color(for: code))
                    }
                    Text((target.path as NSString).lastPathComponent).font(.system(size: 11, weight: .medium, design: .monospaced)).lineLimit(1).truncationMode(.middle).help(target.path)
                    if let diff = model.diff, !diff.isBinary, diff.additions + diff.deletions > 0 {
                        Text("+\(diff.additions)").foregroundStyle(.green).font(.system(size: 9.5, design: .monospaced)).fixedSize()
                        Text("−\(diff.deletions)").foregroundStyle(.red).font(.system(size: 9.5, design: .monospaced)).fixedSize()
                    }
                    Spacer(minLength: 4)
                    if model.diffLoading { ProgressView().controlSize(.mini) }
                    Button { layoutRaw = layout == .unified ? DiffLayout.sideBySide.rawValue : DiffLayout.unified.rawValue } label: {
                        Image(systemName: "rectangle.split.2x1").font(.system(size: 11)).foregroundStyle(layout == .sideBySide ? AppTheme.accent : .secondary).frame(width: 20, height: 20)
                    }.buttonStyle(.plain).help(layout == .sideBySide ? "Unified diff" : "Side-by-side diff")
                    Button { wrap.toggle() } label: { Image(systemName: "text.word.spacing").font(.system(size: 11)).foregroundStyle(wrap ? AppTheme.accent : .secondary).frame(width: 20, height: 20) }
                        .buttonStyle(.plain).help(wrap ? "Scroll long lines" : "Wrap long lines").disabled(layout == .sideBySide)
                    Button { model.maximizeDiff.toggle() } label: { Image(systemName: model.maximizeDiff ? "arrow.down.right.and.arrow.up.left" : "arrow.up.left.and.arrow.down.right").font(.system(size: 11)).foregroundStyle(model.maximizeDiff ? AppTheme.accent : .secondary).frame(width: 20, height: 20) }
                        .buttonStyle(.plain).help(model.maximizeDiff ? "Show the file list" : "Give the diff the whole panel")
                    if let file, target.isWorkingTree {
                        Menu {
                            Button { model.tab = .files; model.selectedFile = file.path } label: { Label("Show in Files", systemImage: "folder") }
                            Button { model.openExternally(file.path) } label: { Label("Open in default app", systemImage: "arrow.up.forward.app") }
                            Divider()
                            if file.isUntracked { Button(role: .destructive) { discard(file, false) } label: { Label("Move to Trash…", systemImage: "trash") } }
                            else {
                                if file.hasUnstaged { Button(role: .destructive) { discard(file, false) } label: { Label("Discard unstaged changes…", systemImage: "arrow.uturn.backward") } }
                                Button(role: .destructive) { discard(file, true) } label: { Label("Discard all changes…", systemImage: "arrow.uturn.backward.circle") }
                            }
                        } label: { Image(systemName: "ellipsis").font(.system(size: 12, weight: .semibold)).foregroundStyle(.secondary).frame(width: 20, height: 20) }
                            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().disabled(model.busy != nil)
                    }
                }.padding(.horizontal, 10).frame(height: 30).background(AppTheme.raised.opacity(0.7))
                Divider()
                if let diff = model.diff {
                    let hunkDiscard: ((DiffHunk) -> Void)? = target.isWorkingTree && file?.isUntracked == false && file?.conflicted == false && model.busy == nil ? discardHunk : nil
                    DiffView(diff: diff, layout: layout, wrap: wrap, onDiscardHunk: hunkDiscard, onLoadAll: diff.truncated ? { model.fullDiff = true } : nil).id(target)
                } else if model.diffLoading {
                    ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Color.clear
                }
            } else {
                VStack(spacing: 8) {
                    Image(systemName: "plus.forwardslash.minus").font(.system(size: 20)).foregroundStyle(.tertiary)
                    Text("Select a file to see its changes").font(.system(size: 11.5)).foregroundStyle(.secondary)
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
    }

    private func scopeLabel(_ target: DiffTarget) -> String {
        switch target {
        case .staged: return "staged"
        case .unstaged: return "unstaged"
        case .untracked: return "untracked"
        case .conflicted: return "conflict"
        case .commit(let sha, _): return String(sha.prefix(7))
        }
    }
}

// MARK: Files tab

struct GitFilesTab: View {
    @EnvironmentObject var store: Store
    @ObservedObject var model: GitPanelModel
    @State private var query = ""

    var body: some View {
        EvenSplit(axis: .vertical, key: "git.files", minimumFirst: 140, minimumSecond: 120, defaultFraction: 0.45) {
            VStack(spacing: 0) {
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass").font(.system(size: 10)).foregroundStyle(.secondary)
                    TextField("Filter files", text: $query).textFieldStyle(.plain).font(.system(size: 11.5))
                    if !query.isEmpty { Button { query = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }.buttonStyle(.plain) }
                    if model.treeLoading { ProgressView().controlSize(.mini) }
                    Button { model.loadTree() } label: { Image(systemName: "arrow.clockwise").font(.system(size: 10)) }.buttonStyle(.plain).foregroundStyle(.secondary).help("Reload file list")
                }.padding(.horizontal, 8).frame(height: 26).background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 6)).padding(8)
                let nodes = FileTree.filter(model.tree, query: query)
                if nodes.isEmpty && !model.treeLoading {
                    Text(query.isEmpty ? "No files" : "No matches").font(.system(size: 11)).foregroundStyle(.tertiary).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    let changedDirs = model.changedDirectories
                    List(nodes, children: \.children, selection: $model.selectedFile) { node in
                        FileTreeRow(node: node, code: node.isDirectory ? nil : model.statusCode(for: node.path), hasChanges: node.isDirectory && changedDirs.contains(node.path))
                            .contextMenu { fileMenu(node) }
                    }.listStyle(.inset).scrollContentBackground(.hidden).environment(\.defaultMinListRowHeight, 22)
                }
            }
        } second: {
            VStack(spacing: 0) {
                if let content = model.fileContent {
                    HStack(spacing: 8) {
                        if let code = model.statusCode(for: content.path) { Text(code.label).font(.system(size: 9, weight: .bold, design: .monospaced)).foregroundStyle(DiffPalette.color(for: code)) }
                        Text(content.path).font(.system(size: 11, design: .monospaced)).lineLimit(1).truncationMode(.middle).help(content.path)
                        Spacer()
                        if let file = model.status?.files.first(where: { $0.path == content.path }) {
                            Button("Show diff") { model.tab = .changes; model.target = model.defaultTarget(for: file) }.controlSize(.mini)
                        }
                        Button { model.openExternally(content.path) } label: { Image(systemName: "arrow.up.forward.app").font(.system(size: 11)) }.buttonStyle(.plain).foregroundStyle(.secondary).help("Open in default app")
                        Button { store.copy(model.absolutePath(content.path)) } label: { Image(systemName: "doc.on.doc").font(.system(size: 11)) }.buttonStyle(.plain).foregroundStyle(.secondary).help("Copy path")
                    }.padding(.horizontal, 10).frame(height: 30).background(AppTheme.raised.opacity(0.7))
                    Divider()
                    FileViewerView(content: content, onOpenExternally: { model.openExternally(content.path) }).id(content.path)
                } else {
                    VStack(spacing: 8) {
                        Image(systemName: "doc.text").font(.system(size: 20)).foregroundStyle(.tertiary)
                        Text("Select a file to read it").font(.system(size: 11.5)).foregroundStyle(.secondary)
                    }.frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }
        .onAppear { if model.tree.isEmpty { model.loadTree() } }
    }

    @ViewBuilder private func fileMenu(_ node: FileNode) -> some View {
        if !node.isDirectory, let file = model.status?.files.first(where: { $0.path == node.path }) {
            Button { model.tab = .changes; model.target = model.defaultTarget(for: file) } label: { Label("Show diff", systemImage: "plus.forwardslash.minus") }
            Divider()
        }
        Button { model.openExternally(node.path) } label: { Label(node.isDirectory ? "Open in Finder" : "Open in default app", systemImage: "arrow.up.forward.app") }
        Button { model.revealInFinder(node.path) } label: { Label("Show in Finder", systemImage: "finder") }
        Button { store.copy(model.absolutePath(node.path)) } label: { Label("Copy path", systemImage: "doc.on.doc") }
        Button { store.copy(node.path) } label: { Label("Copy relative path", systemImage: "doc.on.doc") }
    }
}

struct FileTreeRow: View {
    let node: FileNode
    let code: GitFileCode?
    let hasChanges: Bool
    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: node.isDirectory ? "folder.fill" : "doc").font(.system(size: 11)).foregroundStyle(node.isDirectory ? AppTheme.accent.opacity(0.75) : .secondary).frame(width: 14)
            Text(node.name).font(.system(size: 11.5)).lineLimit(1).strikethrough(code == .deleted)
                .foregroundStyle(code.map { DiffPalette.color(for: $0) } ?? (node.isDirectory ? Color.primary : Color.primary.opacity(0.9)))
            if hasChanges { Circle().fill(AppTheme.accent.opacity(0.7)).frame(width: 5, height: 5) }
            Spacer(minLength: 0)
            if let code, code != .unchanged { Text(code.label).font(.system(size: 9, weight: .bold, design: .monospaced)).foregroundStyle(DiffPalette.color(for: code)) }
        }.help(node.path)
    }
}

// MARK: Log tab

struct GitLogTab: View {
    @EnvironmentObject var store: Store
    @ObservedObject var model: GitPanelModel
    @State private var confirmRevert: GitCommit?
    @State private var confirmReset: ResetRequest?
    struct ResetRequest: Identifiable { let commit: GitCommit; let mode: ResetMode; var id: String { commit.sha + mode.rawValue } }

    var body: some View {
        EvenSplit(axis: .vertical, key: "git.log", minimumFirst: 120, minimumSecond: 140, defaultFraction: 0.4) {
            Group {
                if model.commits.isEmpty {
                    if model.logLoading { ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity) }
                    else { Text("No commits yet").font(.system(size: 11.5)).foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity) }
                } else {
                    List(model.commits, selection: $model.selectedCommit) { commit in
                        CommitRow(commit: commit).contextMenu { commitMenu(commit) }
                    }.listStyle(.inset).scrollContentBackground(.hidden)
                }
            }
        } second: {
            VStack(spacing: 0) {
                if let sha = model.selectedCommit, let commit = model.commits.first(where: { $0.sha == sha }) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(commit.subject).font(.system(size: 12, weight: .semibold)).lineLimit(2)
                        HStack(spacing: 6) {
                            Text(commit.author).font(.system(size: 10.5)).foregroundStyle(.secondary)
                            Text(commit.date.formatted(date: .abbreviated, time: .shortened)).font(.system(size: 10.5)).foregroundStyle(.tertiary)
                            Text(commit.shortSHA).font(.system(size: 10, design: .monospaced)).foregroundStyle(.tertiary)
                            Spacer()
                            Menu { commitMenu(commit) } label: { Image(systemName: "ellipsis").font(.system(size: 12, weight: .semibold)).foregroundStyle(.secondary).frame(width: 20, height: 20) }
                                .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                        }
                    }.padding(.horizontal, 10).padding(.vertical, 7).frame(maxWidth: .infinity, alignment: .leading).background(AppTheme.raised.opacity(0.7))
                    Divider()
                    EvenSplit(axis: .vertical, key: "git.log.files", minimumFirst: 60, minimumSecond: 80, defaultFraction: 0.3) {
                        ScrollView {
                            LazyVStack(spacing: 1) {
                                ForEach(model.commitFiles) { file in
                                    let target = DiffTarget.commit(sha: sha, path: file.path)
                                    HStack(spacing: 7) {
                                        Text(file.code.label).font(.system(size: 9, weight: .bold, design: .monospaced)).foregroundStyle(DiffPalette.color(for: file.code)).frame(width: 12)
                                        Text((file.path as NSString).lastPathComponent).font(.system(size: 11.5)).lineLimit(1)
                                        Text((file.path as NSString).deletingLastPathComponent).font(.system(size: 10)).foregroundStyle(.tertiary).lineLimit(1).truncationMode(.middle)
                                        Spacer(minLength: 0)
                                    }.padding(.horizontal, 8).frame(height: 24).contentShape(Rectangle())
                                        .background(model.target == target ? AppTheme.accent.opacity(0.14) : .clear, in: RoundedRectangle(cornerRadius: 6))
                                        .onTapGesture { model.target = target }.help(file.path)
                                }
                            }.padding(6)
                        }
                    } second: {
                        GitDiffArea(model: model, discard: { _, _ in }, discardHunk: { _ in })
                    }
                } else {
                    VStack(spacing: 8) {
                        Image(systemName: "clock.arrow.circlepath").font(.system(size: 20)).foregroundStyle(.tertiary)
                        Text("Select a commit to see what it changed").font(.system(size: 11.5)).foregroundStyle(.secondary)
                    }.frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
        }
        .onAppear { model.loadLog() }
        .alert("Revert \(confirmRevert?.shortSHA ?? "")?", isPresented: Binding(get: { confirmRevert != nil }, set: { if !$0 { confirmRevert = nil } }), presenting: confirmRevert) { commit in
            Button("Cancel", role: .cancel) { confirmRevert = nil }
            Button("Revert", role: .destructive) { model.revert(commit); confirmRevert = nil }
        } message: { commit in
            Text(commit.isMerge ? "Creates a new commit undoing this merge (keeping the first parent). Conflicts must be resolved in the terminal." : "Creates a new commit that undoes “\(commit.subject)”. History is kept.")
        }
        .alert("Reset to \(confirmReset?.commit.shortSHA ?? "")?", isPresented: Binding(get: { confirmReset != nil }, set: { if !$0 { confirmReset = nil } }), presenting: confirmReset) { request in
            Button("Cancel", role: .cancel) { confirmReset = nil }
            Button("Reset (\(request.mode.rawValue))", role: .destructive) { model.reset(to: request.commit, mode: request.mode); confirmReset = nil }
        } message: { request in
            Text(request.mode == .soft ? "Moves the branch to this commit and keeps all later changes staged. Nothing on disk changes." : "Moves the branch to this commit and keeps all later changes in the working tree, unstaged. Nothing on disk changes.")
        }
    }

    @ViewBuilder private func commitMenu(_ commit: GitCommit) -> some View {
        Button { confirmRevert = commit } label: { Label("Revert commit…", systemImage: "arrow.uturn.backward") }
        Button { confirmReset = ResetRequest(commit: commit, mode: .soft) } label: { Label("Reset to here (soft, keep staged)…", systemImage: "arrow.counterclockwise") }
        Button { confirmReset = ResetRequest(commit: commit, mode: .mixed) } label: { Label("Reset to here (mixed, keep in working tree)…", systemImage: "arrow.counterclockwise.circle") }
        Divider()
        Button { store.copy(commit.sha) } label: { Label("Copy SHA", systemImage: "doc.on.doc") }
        Button { store.copy(commit.subject) } label: { Label("Copy subject", systemImage: "text.quote") }
    }
}

struct CommitRow: View {
    let commit: GitCommit
    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text(commit.subject).font(.system(size: 11.5, weight: .medium)).lineLimit(1)
                HStack(spacing: 5) {
                    Text(commit.author).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                    Text(commit.date.formatted(.relative(presentation: .named))).font(.system(size: 10)).foregroundStyle(.tertiary)
                    ForEach(commit.refs.prefix(3), id: \.self) { ref in
                        Text(ref).font(.system(size: 9, weight: .medium)).foregroundStyle(AppTheme.accent).lineLimit(1)
                            .padding(.horizontal, 5).padding(.vertical, 1).background(AppTheme.accent.opacity(0.12), in: Capsule())
                    }
                }
            }
            Spacer(minLength: 4)
            Text(commit.shortSHA).font(.system(size: 10, design: .monospaced)).foregroundStyle(.tertiary)
        }.padding(.vertical, 2).help("\(commit.subject)\n\(commit.author) · \(commit.date.formatted())")
    }
}
