import SwiftUI

/// Project documentation lives in the repo so agents read it as plain files: `.specdesk/PROJECT.md` and `.specdesk/specs/*.md`.
enum Docs {
    static let folder = ".specdesk"
    static let projectDoc = ".specdesk/PROJECT.md"

    static func specFiles(in root: String) -> [String] {
        let dir = root + "/.specdesk/specs"
        return ((try? FileManager.default.contentsOfDirectory(atPath: dir)) ?? []).filter { $0.hasSuffix(".md") }.sorted().map { ".specdesk/specs/\($0)" }
    }
    /// Everything worth reading: the project doc, specs, then well-known repo docs.
    static func files(in root: String) -> [String] {
        var list: [String] = []
        if FileManager.default.fileExists(atPath: root + "/" + projectDoc) { list.append(projectDoc) }
        list += specFiles(in: root)
        for name in ["README.md", "CLAUDE.md", "AGENTS.md", "CONTRIBUTING.md", "ARCHITECTURE.md"] where FileManager.default.fileExists(atPath: root + "/" + name) { list.append(name) }
        let docs = ((try? FileManager.default.contentsOfDirectory(atPath: root + "/docs")) ?? []).filter { $0.hasSuffix(".md") }.sorted().prefix(30)
        list += docs.map { "docs/\($0)" }
        return list
    }
    static func specTemplate(title: String) -> String {
        """
        # \(title)

        ## Problem
        What is wrong or missing today, and for whom.

        ## Goal
        The outcome in one or two sentences.

        ## Requirements
        - 

        ## Acceptance criteria
        - [ ] 

        ## Out of scope
        - 

        ## Notes
        Constraints, links, decisions.
        """
    }
    static func projectDocPrompt() -> String {
        """
        Write the project document at .specdesk/PROJECT.md (create the folder if needed). It is read by every agent before it starts a task and by the team as the single overview. Study the repository first. Include: purpose and domain in plain words; architecture and main modules with paths; how to build, run and test; conventions (code style, branching, commit messages, review expectations); environment and secrets handling (names only, never values); known pitfalls; and a short glossary. Keep it under ~250 lines, accurate to the code, and note anything you could not verify. Do not change any other file.
        """
    }
    static func specPrompt(path: String, title: String) -> String {
        """
        Draft the specification at \(path) for: \(title). Keep the existing section headings (Problem, Goal, Requirements, Acceptance criteria, Out of scope, Notes). Study the codebase so requirements reference real modules and current behaviour. Acceptance criteria must be checkable. Ask me in the terminal if a decision is genuinely ambiguous; otherwise state your assumption in Notes. Do not implement anything and do not change other files.
        """
    }
}

struct DocsPanel: View {
    @EnvironmentObject var store: Store
    let project: Project
    @State private var files: [String] = []
    @State private var selected: String?
    @State private var text = ""
    @State private var editing = false
    @State private var dirty = false
    @State private var newSpec = false
    @State private var specTitle = ""

    var body: some View {
        HSplitView {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("DOCS & SPECS").font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
                    Spacer()
                    Button { reload() } label: { Image(systemName: "arrow.clockwise") }.buttonStyle(.plain).foregroundStyle(.secondary)
                }
                if !files.contains(Docs.projectDoc) {
                    Button { writeProjectDoc() } label: { Label("Write project doc with AI", systemImage: "sparkles") }.controlSize(.small)
                }
                Button { specTitle = ""; newSpec = true } label: { Label("New spec…", systemImage: "doc.badge.plus") }.controlSize(.small)
                List(selection: $selected) {
                    ForEach(files, id: \.self) { file in
                        HStack(spacing: 6) {
                            Image(systemName: file == Docs.projectDoc ? "book" : (file.hasPrefix(".specdesk/specs/") ? "doc.text" : "doc")).foregroundStyle(file.hasPrefix(".specdesk") ? AppTheme.accent : .secondary).frame(width: 14)
                            Text(file.replacingOccurrences(of: ".specdesk/specs/", with: "").replacingOccurrences(of: ".specdesk/", with: "")).font(.system(size: 12)).lineLimit(1)
                        }.tag(file)
                    }
                }.listStyle(.sidebar)
                Text("Stored in the repo under .specdesk so agents read them as files.").font(.system(size: 10)).foregroundStyle(.tertiary)
            }.padding(12).frame(minWidth: 200, idealWidth: 240, maxWidth: 320)
            VStack(spacing: 0) {
                if let selected {
                    HStack(spacing: 8) {
                        Text(selected).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                        Spacer()
                        if selected == Docs.projectDoc { Button("Refresh with AI") { writeProjectDoc() }.controlSize(.small) }
                        if selected.hasPrefix(".specdesk/specs/") {
                            Button("Draft with AI") { draftSpec(selected) }.controlSize(.small)
                            Button("Task from spec") { store.taskFromSpec = (project.id, selected) }.controlSize(.small)
                        }
                        if editing && dirty {
                            Button("Save") { save(); editing = false }.controlSize(.small).buttonStyle(.borderedProminent)
                        } else {
                            Button(editing ? "Done" : "Edit") { editing.toggle() }.controlSize(.small)
                        }
                        Button { store.copy(project.path + "/" + selected) } label: { Image(systemName: "doc.on.doc") }.buttonStyle(.plain).foregroundStyle(.secondary).help("Copy path")
                    }.padding(.horizontal, 12).frame(height: 36).background(AppTheme.raised)
                    Divider()
                    if editing {
                        TextEditor(text: $text).font(.system(size: 12.5, design: .monospaced)).padding(8)
                            .onChange(of: text) { _, _ in dirty = true }
                    } else {
                        ScrollView { MarkdownView(text: text).padding(24).frame(maxWidth: 820, alignment: .leading) }.frame(maxWidth: .infinity)
                    }
                } else {
                    VStack(spacing: 10) {
                        Image(systemName: "book").font(.system(size: 28)).foregroundStyle(.tertiary)
                        Text("Project doc and specs").font(.system(size: 15, weight: .semibold))
                        Text("The project doc gives every agent the context it needs. Specs describe one piece of work each; tasks link to them.").font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 380)
                    }.frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }.frame(minWidth: 400)
        }
        .onAppear(perform: reload)
        .onChange(of: selected) { _, file in load(file) }
        .sheet(isPresented: $newSpec) {
            VStack(alignment: .leading, spacing: 14) {
                Text("New specification").font(.title2.bold())
                TextField("Feature or problem, e.g. Checkout retries", text: $specTitle).textFieldStyle(.roundedBorder)
                Text("Creates .specdesk/specs/<slug>.md from a template. Draft with AI afterwards, or write it yourself.").font(.caption).foregroundStyle(.secondary)
                HStack { Spacer(); Button("Cancel") { newSpec = false }
                    Button("Create") { createSpec(); newSpec = false }.buttonStyle(.borderedProminent).disabled(specTitle.trimmingCharacters(in: .whitespaces).isEmpty) }
            }.padding(24).frame(width: 480)
        }
    }

    private func reload() { files = Docs.files(in: project.path); if let s = selected, !files.contains(s) { selected = nil } }
    private func load(_ file: String?) {
        guard let file else { text = ""; return }
        text = (try? String(contentsOfFile: project.path + "/" + file, encoding: .utf8)) ?? ""
        editing = false; dirty = false
    }
    private func save() {
        guard let selected else { return }
        do { try text.write(toFile: project.path + "/" + selected, atomically: true, encoding: .utf8); dirty = false }
        catch { store.error = error.localizedDescription }
    }
    private func createSpec() {
        let slug = GitWorktree.slug(specTitle)
        let rel = ".specdesk/specs/\(slug).md"
        do {
            try FileManager.default.createDirectory(atPath: project.path + "/.specdesk/specs", withIntermediateDirectories: true)
            if !FileManager.default.fileExists(atPath: project.path + "/" + rel) {
                try Docs.specTemplate(title: specTitle).write(toFile: project.path + "/" + rel, atomically: true, encoding: .utf8)
            }
            reload(); selected = rel; load(rel); editing = true
        } catch { store.error = error.localizedDescription }
    }
    private func writeProjectDoc() {
        let session = LinkedSession(agent: .claude, sessionID: UUID().uuidString.lowercased(), title: "Write project doc", initialPrompt: Docs.projectDocPrompt())
        store.startSession(session, in: project)
    }
    private func draftSpec(_ path: String) {
        let title = text.split(separator: "\n").first.map { $0.replacingOccurrences(of: "# ", with: "") } ?? path
        let session = LinkedSession(agent: .claude, sessionID: UUID().uuidString.lowercased(), title: "Spec: \(title)", initialPrompt: Docs.specPrompt(path: path, title: title))
        store.startSession(session, in: project)
    }
}

/// Lightweight Markdown rendering: headings, lists, code blocks and inline styles via AttributedString.
struct MarkdownView: View {
    let text: String
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                switch block {
                case .code(let code):
                    Text(code).font(.system(size: 11.5, design: .monospaced)).textSelection(.enabled)
                        .padding(10).frame(maxWidth: .infinity, alignment: .leading).background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 6))
                case .heading(let level, let s):
                    Text(inline(s)).font(.system(size: level == 1 ? 22 : (level == 2 ? 17 : 14), weight: .semibold)).padding(.top, level == 1 ? 4 : 8).textSelection(.enabled)
                case .bullet(let s, let checked):
                    HStack(alignment: .top, spacing: 8) {
                        if let checked { Image(systemName: checked ? "checkmark.square" : "square").foregroundStyle(checked ? .green : .secondary).padding(.top, 2) }
                        else { Text("•").foregroundStyle(.secondary) }
                        Text(inline(s)).textSelection(.enabled)
                    }.padding(.leading, 8)
                case .paragraph(let s):
                    Text(inline(s)).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                }
            }
        }.font(.system(size: 13)).lineSpacing(3)
    }

    enum Block { case heading(Int, String), bullet(String, Bool?), code(String), paragraph(String) }

    private var blocks: [Block] {
        var out: [Block] = []; var para: [String] = []; var code: [String]?
        func flush() { if !para.isEmpty { out.append(.paragraph(para.joined(separator: " "))); para = [] } }
        for raw in text.split(separator: "\n", omittingEmptySubsequences: false) {
            let line = String(raw)
            if line.hasPrefix("```") { if let c = code { out.append(.code(c.joined(separator: "\n"))); code = nil } else { flush(); code = [] }; continue }
            if code != nil { code?.append(line); continue }
            if line.hasPrefix("#") {
                flush(); let level = line.prefix(while: { $0 == "#" }).count
                out.append(.heading(level, line.drop(while: { $0 == "#" }).trimmingCharacters(in: .whitespaces)))
            } else if line.hasPrefix("- ") || line.hasPrefix("* ") {
                flush(); var s = String(line.dropFirst(2)); var checked: Bool? = nil
                if s.hasPrefix("[ ] ") { checked = false; s = String(s.dropFirst(4)) } else if s.lowercased().hasPrefix("[x] ") { checked = true; s = String(s.dropFirst(4)) }
                out.append(.bullet(s, checked))
            } else if line.trimmingCharacters(in: .whitespaces).isEmpty { flush() }
            else { para.append(line) }
        }
        flush(); if let c = code { out.append(.code(c.joined(separator: "\n"))) }
        return out
    }
    private func inline(_ s: String) -> AttributedString {
        (try? AttributedString(markdown: s, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(s)
    }
}
