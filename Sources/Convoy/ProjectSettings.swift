import SwiftUI
import AppKit

/// Everything about one project in one page: identity, agent, git & worktrees, review & tasks, quick commands.
struct ProjectSettingsPage: View {
    @EnvironmentObject var store: Store
    @State var project: Project
    @State private var iconTab = "emoji"
    @State private var customHex = ""
    @State private var favicon = ""
    @State private var busy = false
    @State private var message: String?
    @State private var editingCommand: QuickCommand?
    @State private var confirmRemove = false
    @AppStorage("branchPrefix") private var globalPrefix = ""
    @AppStorage("defaultAgent") private var globalAgent = "Claude Code"

    private var saved: Project? { store.workspace.projects.first { $0.id == project.id } }
    private var dirty: Bool { saved != project }
    static let emojis = ["🚀", "🧪", "📦", "🛒", "💳", "📱", "🌐", "🤖", "🧭", "🛠️", "🏦", "🚚", "📊", "🔐", "🧾", "🎯", "🧠", "⚙️", "🧱", "🛰️", "🗂️", "🧩", "📡", "🏗️"]
    static let symbols = ["folder", "shippingbox", "cart", "creditcard", "iphone", "globe", "server.rack", "cpu", "terminal", "gearshape", "building.2", "truck.box", "chart.bar", "lock.shield", "doc.text", "network", "cloud", "wrench.and.screwdriver"]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                HStack(alignment: .firstTextBaseline) {
                    VStack(alignment: .leading, spacing: 4) {
                        HStack(spacing: 8) {
                            Button { store.projectSettingsID = nil } label: { Label("Back", systemImage: "arrow.left") }.buttonStyle(.plain).foregroundStyle(.secondary)
                            Text("Project settings").font(.system(size: 13)).foregroundStyle(.secondary)
                        }
                        HStack(spacing: 10) {
                            ProjectIconView(project: project, size: 24)
                            Text(project.name).font(.system(size: 24, weight: .bold)).foregroundStyle(project.tint)
                            Text(project.isGroup ? "GROUP" : "PROJECT").font(.system(size: 9, weight: .semibold)).tracking(0.5).foregroundStyle(.secondary).padding(.horizontal, 7).padding(.vertical, 3).background(Color.primary.opacity(0.06), in: Capsule())
                        }
                        Text(project.path).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
                    }
                    Spacer()
                    if let message { Text(message).font(.caption).foregroundStyle(.secondary) }
                    Button("Revert") { if let saved { project = saved } }.disabled(!dirty)
                    Button("Save") { store.updateProject(project); message = "Saved" }.buttonStyle(.borderedProminent).disabled(!dirty)
                }
                identity
                if !project.isGroup { agentSection; gitSection }
                workflowSection
                quickCommandsSection
                dangerSection
            }.padding(32).frame(maxWidth: 820, alignment: .leading)
        }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .onAppear { customHex = project.color ?? ""; if let icon = project.icon { iconTab = icon.hasPrefix("gh:") || icon.hasPrefix("img:") ? "image" : (icon.hasPrefix("sf:") ? "symbol" : "emoji") } }
            .sheet(item: $editingCommand) { c in QuickCommandSheet(command: c) }
            .alert("Remove \(project.name) from Convoy?", isPresented: $confirmRemove) {
                Button("Cancel", role: .cancel) {}
                Button("Remove", role: .destructive) { store.removeProject(project.id); store.projectSettingsID = nil; store.goHome() }
            } message: { Text("Removes saved sessions, tasks and specs records from the app. Files, worktrees and the agents' own history stay on disk.") }
    }

    // MARK: Identity
    private var identity: some View {
        SettingsGroup(title: "Identity", footer: "Shown in the sidebar, tabs, pane headers and the dashboard so parallel projects stay distinguishable.") {
            SettingsRow(title: "Display name") { TextField("Name", text: $project.name).textFieldStyle(.roundedBorder).frame(width: 280) }
            VStack(alignment: .leading, spacing: 10) {
                Text("Color").font(.system(size: 13))
                HStack(spacing: 8) {
                    ForEach(ProjectIconSheet.colors, id: \.self) { hex in
                        Circle().fill(Color(hex: hex)).frame(width: 24, height: 24)
                            .overlay(Circle().strokeBorder(Color.primary, lineWidth: project.color == hex ? 2 : 0))
                            .onTapGesture { project.color = hex; customHex = hex }
                    }
                    TextField("#5E6AD2", text: $customHex).textFieldStyle(.roundedBorder).frame(width: 90).font(.system(size: 11, design: .monospaced))
                        .onSubmit { if customHex.range(of: "^#?[0-9A-Fa-f]{6}$", options: .regularExpression) != nil { project.color = customHex.hasPrefix("#") ? customHex : "#" + customHex } }
                    Button("Auto") { project.color = nil; customHex = "" }.controlSize(.small).help("Derive a colour from the name")
                }
                Picker("", selection: $iconTab) { Text("Emoji").tag("emoji"); Text("Icon").tag("symbol"); Text("Image").tag("image") }.pickerStyle(.segmented).labelsHidden().frame(width: 240)
                switch iconTab {
                case "symbol":
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 34))], spacing: 6) {
                        ForEach(Self.symbols, id: \.self) { name in
                            Button { project.icon = "sf:" + name } label: {
                                Image(systemName: name).font(.system(size: 14)).frame(width: 32, height: 30)
                                    .background(project.icon == "sf:" + name ? AppTheme.accent.opacity(0.2) : Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6))
                            }.buttonStyle(.plain)
                        }
                    }
                case "image":
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 8) {
                            Button { fetchAvatar() } label: { Label("Use GitHub avatar", systemImage: "person.crop.circle") }.disabled(busy)
                            Button { uploadImage() } label: { Label("Upload PNG…", systemImage: "photo") }
                        }
                        HStack(spacing: 8) {
                            TextField("example.com", text: $favicon).textFieldStyle(.roundedBorder).frame(width: 220)
                            Button { fetchFavicon() } label: { Label("Favicon", systemImage: "link") }.disabled(favicon.isEmpty || busy)
                        }
                        if busy { ProgressView().controlSize(.small) }
                    }
                default:
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 34))], spacing: 6) {
                        ForEach(Self.emojis, id: \.self) { e in
                            Button { project.icon = e } label: {
                                Text(e).font(.system(size: 17)).frame(width: 32, height: 30)
                                    .background(project.icon == e ? AppTheme.accent.opacity(0.2) : Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6))
                            }.buttonStyle(.plain)
                        }
                    }
                }
                Button("Reset to folder icon") { project.icon = nil }.controlSize(.small).disabled(project.icon == nil)
            }.padding(14)
        }
    }

    // MARK: Agent
    private var agentSection: some View {
        SettingsGroup(title: "Agent", footer: "Preselected for new sessions and tasks in this project. Global default: \(globalAgent).") {
            SettingsRow(title: "Default agent") {
                Picker("", selection: $project.defaultAgent) {
                    Text("Use global (\(globalAgent))").tag(Agent?.none)
                    ForEach(Agent.allCases) { Text($0.rawValue).tag(Agent?.some($0)) }
                }.labelsHidden().frame(width: 220)
            }
        }
    }

    // MARK: Git & worktrees
    private var gitSection: some View {
        SettingsGroup(title: "Git & worktrees", footer: "Base ref is the branch new worktrees start from (empty = current HEAD). Shared paths are gitignored files brought into each new worktree, cloned with APFS or symlinked. Setup commands run once before the agent starts.") {
            SettingsRow(title: "Base ref for worktrees") { TextField("main", text: Binding(get: { project.baseRef ?? "" }, set: { project.baseRef = $0.isEmpty ? nil : $0 })).textFieldStyle(.roundedBorder).frame(width: 200).font(.system(size: 12, design: .monospaced)) }
            SettingsRow(title: "Branch prefix", detail: "Global: \(globalPrefix.isEmpty ? "none" : globalPrefix)") { TextField("feature", text: Binding(get: { project.branchPrefix ?? "" }, set: { project.branchPrefix = $0.isEmpty ? nil : $0 })).textFieldStyle(.roundedBorder).frame(width: 200) }
            VStack(alignment: .leading, spacing: 6) {
                Text("Shared paths (one per line)").font(.system(size: 13))
                TextEditor(text: Binding(get: { (project.sharedPaths ?? []).joined(separator: "\n") }, set: { project.sharedPaths = $0.split(separator: "\n").map { String($0).trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty } }))
                    .font(.system(size: 12, design: .monospaced)).frame(height: 64).overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke))
                Text("e.g. .env, node_modules, vendor").font(.caption).foregroundStyle(.tertiary)
            }.padding(14).overlay(alignment: .bottom) { Divider().padding(.leading, 14) }
            VStack(alignment: .leading, spacing: 6) {
                Text("Worktree setup commands").font(.system(size: 13))
                TextEditor(text: Binding(get: { project.setupCommands ?? "" }, set: { project.setupCommands = $0.isEmpty ? nil : $0 }))
                    .font(.system(size: 12, design: .monospaced)).frame(height: 80).overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke))
                Text("e.g. pnpm install · composer install · direnv allow").font(.caption).foregroundStyle(.tertiary)
            }.padding(14)
        }
    }

    // MARK: Review & tasks
    private var workflowSection: some View {
        SettingsGroup(title: "Review & tasks", footer: "Review template placeholders: {{title}} {{path}} {{scope}} {{task}} {{notes}} {{spec}} {{output}} {{branch}}. Empty uses the global template from Settings → Agents.") {
            SettingsRow(title: "Tasks finish with") {
                Picker("", selection: Binding(get: { project.taskMode ?? "pr" }, set: { project.taskMode = $0 })) { Text("Pull request").tag("pr"); Text("Push to main").tag("push"); Text("Commit only").tag("none") }.labelsHidden().frame(width: 180)
            }
            SettingsRow(title: "Auto-run task queue", detail: "Start the next queued task when one finishes") { Toggle("", isOn: Binding(get: { project.autoRunTasks == true }, set: { project.autoRunTasks = $0 })).labelsHidden().toggleStyle(.switch) }
            VStack(alignment: .leading, spacing: 6) {
                Text("Review brief template").font(.system(size: 13))
                TextEditor(text: Binding(get: { project.reviewTemplate ?? "" }, set: { project.reviewTemplate = $0.isEmpty ? nil : $0 }))
                    .font(.system(size: 12, design: .monospaced)).frame(height: 140).overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke))
                HStack { Button("Insert default") { project.reviewTemplate = ReviewTemplate.default }.controlSize(.small); Button("Use global") { project.reviewTemplate = nil }.controlSize(.small) }
            }.padding(14)
        }
    }

    // MARK: Quick commands
    private var quickCommandsSection: some View {
        SettingsGroup(title: "Quick commands for this project", footer: "Sent to a terminal from the ⚡ menu. Global commands are managed in Settings → Quick Commands.") {
            let mine = store.quickCommands.filter { $0.projectID == project.id }
            if mine.isEmpty { Text("None yet").font(.caption).foregroundStyle(.secondary).padding(14).overlay(alignment: .bottom) { Divider().padding(.leading, 14) } }
            ForEach(mine) { c in
                SettingsRow(title: c.title, detail: String(c.text.prefix(100)).replacingOccurrences(of: "\n", with: " ")) {
                    HStack(spacing: 8) {
                        Button("Edit") { editingCommand = c }.controlSize(.small)
                        Button(role: .destructive) { store.deleteQuickCommand(c.id) } label: { Image(systemName: "trash") }.controlSize(.small)
                    }
                }
            }
            HStack { Button { editingCommand = QuickCommand(title: "", text: "", projectID: project.id) } label: { Label("Add", systemImage: "plus") }.controlSize(.small); Spacer() }.padding(14)
        }
    }

    private var dangerSection: some View {
        SettingsGroup(title: "Remove", footer: "Stop this project's running sessions first.") {
            SettingsRow(title: "Remove project from Convoy") { Button("Remove…", role: .destructive) { confirmRemove = true }.disabled(store.terminals.hasRunningSessions(in: project.id)) }
        }
    }

    // MARK: Image sources
    private var iconDir: URL { FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/icons") }

    private func fetchAvatar() {
        guard let (s, out) = GitInfoService.run(["remote", "get-url", "origin"], in: project.path), s == 0,
              let range = out.range(of: #"github\.com[:/]([^/]+)/"#, options: .regularExpression) else { message = "No GitHub origin remote"; return }
        let owner = out[range].split(whereSeparator: { $0 == ":" || $0 == "/" })[1]
        download(URL(string: "https://github.com/\(owner).png?size=64")!, name: "gh-\(owner).png", prefix: "gh:")
    }
    private func fetchFavicon() {
        let host = favicon.replacingOccurrences(of: "https://", with: "").replacingOccurrences(of: "http://", with: "").split(separator: "/").first.map(String.init) ?? favicon
        download(URL(string: "https://www.google.com/s2/favicons?domain=\(host)&sz=64")!, name: "favicon-\(host).png", prefix: "img:")
    }
    private func download(_ url: URL, name: String, prefix: String) {
        busy = true; message = nil
        Task {
            do {
                try FileManager.default.createDirectory(at: iconDir, withIntermediateDirectories: true)
                let (data, _) = try await URLSession.shared.data(from: url)
                guard NSImage(data: data) != nil else { throw GitError("Not an image") }
                let target = iconDir.appendingPathComponent(name)
                try data.write(to: target)
                await MainActor.run { project.icon = prefix + target.path; busy = false; message = "Image set — press Save" }
            } catch { await MainActor.run { busy = false; message = error.localizedDescription } }
        }
    }
    private func uploadImage() {
        let panel = NSOpenPanel(); panel.allowedContentTypes = [.png, .jpeg]; panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url, let data = try? Data(contentsOf: url), data.count <= 512_000 else { message = "Pick a PNG or JPEG up to 512 KB"; return }
        do {
            try FileManager.default.createDirectory(at: iconDir, withIntermediateDirectories: true)
            let target = iconDir.appendingPathComponent("\(project.id.uuidString).png")
            try data.write(to: target)
            project.icon = "img:" + target.path; message = "Image set — press Save"
        } catch { message = error.localizedDescription }
    }
}
