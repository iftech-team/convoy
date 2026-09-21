import SwiftUI

/// In-window settings, Orca-style: searchable left navigation, one page per section.
struct SettingsPage: View {
    @EnvironmentObject var store: Store
    @State private var query = ""
    @FocusState private var searchFocused: Bool

    static let sections: [(String, String, [String])] = [
        ("General", "slider.horizontal.3", ["keep awake", "sleep", "sidebar", "compact", "group", "sort", "worktree default", "branch prefix"]),
        ("Appearance", "paintpalette", ["theme", "dark", "light", "system", "accent"]),
        ("Terminal", "terminal", ["font", "size", "scrollback", "lines", "snapshot"]),
        ("Agents", "sparkles", ["claude", "codex", "hooks", "status", "working", "waiting", "done", "status line", "hibernate", "sleep", "idle", "review", "template", "brief", "default agent", "yolo", "permissions", "skip"]),
        ("Accounts", "person.crop.circle", ["account", "login", "switch", "hot swap", "claude", "codex"]),
        ("Quick Commands", "bolt", ["prompt", "preset", "command", "snippet", "quick"]),
        ("Git", "arrow.triangle.branch", ["branch", "worktree", "dirty", "poll", "refresh", "setup", "install", "hooks"]),
        ("Notifications", "bell", ["notify", "sound", "badge", "waiting", "finished", "done", "focus"]),
        ("AI Limits", "chart.bar.xaxis", ["usage", "limits", "codex", "claude", "quota"]),
        ("Shortcuts", "keyboard", ["keyboard", "keys", "bindings", "palette"]),
    ]

    private var visibleSections: [(String, String, [String])] {
        let q = query.lowercased().trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return Self.sections }
        return Self.sections.filter { $0.0.lowercased().contains(q) || $0.2.contains { $0.contains(q) } }
    }

    var body: some View {
        HStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 0) {
                Button { store.showSettings = false } label: {
                    Label("Back to app", systemImage: "arrow.left").font(.system(size: 12, weight: .medium))
                }.buttonStyle(.plain).foregroundStyle(.secondary).padding(.horizontal, 16).frame(height: 40)
                HStack(spacing: 6) {
                    Image(systemName: "magnifyingglass").font(.system(size: 11)).foregroundStyle(.secondary)
                    TextField("Search settings", text: $query).textFieldStyle(.plain).font(.system(size: 12)).focused($searchFocused)
                    Text("⌘F").font(.system(size: 9, weight: .medium)).foregroundStyle(.tertiary)
                }.padding(.horizontal, 8).frame(height: 28)
                    .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
                    .padding(.horizontal, 12).padding(.bottom, 10)
                ScrollView {
                    VStack(alignment: .leading, spacing: 1) {
                        ForEach(visibleSections, id: \.0) { name, icon, _ in
                            Button { store.settingsSection = name } label: {
                                Label(name, systemImage: icon).font(.system(size: 12.5, weight: store.settingsSection == name ? .semibold : .regular))
                                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 10).padding(.vertical, 6).contentShape(Rectangle())
                            }.buttonStyle(.plain)
                                .background(store.settingsSection == name ? AppTheme.accent.opacity(0.16) : .clear, in: RoundedRectangle(cornerRadius: 7))
                        }
                        if visibleSections.isEmpty { Text("No matching settings").font(.caption).foregroundStyle(.secondary).padding(10) }
                    }.padding(.horizontal, 8)
                }
                Spacer()
                Text("Convoy \(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "")").font(.system(size: 10)).foregroundStyle(.tertiary).padding(16)
            }.frame(width: 220).background(AppTheme.sidebar)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 22) {
                    Text(store.settingsSection).font(.system(size: 24, weight: .bold))
                    switch store.settingsSection {
                    case "Appearance": AppearanceSettings()
                    case "Terminal": TerminalSettings()
                    case "Agents": AgentSettings()
                    case "Accounts": AccountSettings()
                    case "Quick Commands": QuickCommandSettings()
                    case "Git": GitSettings()
                    case "Notifications": NotificationSettings()
                    case "AI Limits": LimitsSettings()
                    case "Shortcuts": ShortcutSettings()
                    default: GeneralSettings()
                    }
                }.padding(32).frame(maxWidth: 720, alignment: .leading)
            }.frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .onAppear { if !Self.sections.contains(where: { $0.0 == store.settingsSection }) { store.settingsSection = "General" } }
        .onChange(of: store.searchFocusRequest) { _, _ in searchFocused = true }
        .onExitCommand { store.showSettings = false }
    }
}

// MARK: - Building blocks

struct SettingsGroup<Content: View>: View {
    let title: String
    var footer: String? = nil
    @ViewBuilder let content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased()).font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
            VStack(spacing: 0) { content() }
                .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 10))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(AppTheme.stroke))
            if let footer { Text(footer).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
        }
    }
}

struct SettingsRow<Control: View>: View {
    let title: String
    var detail: String? = nil
    @ViewBuilder let control: () -> Control
    var body: some View {
        HStack(alignment: .center, spacing: 16) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.system(size: 13))
                if let detail { Text(detail).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }
            }
            Spacer()
            control()
        }.padding(.horizontal, 14).padding(.vertical, 10)
            .overlay(alignment: .bottom) { Divider().padding(.leading, 14) }
    }
}

// MARK: - Sections

struct GeneralSettings: View {
    @EnvironmentObject var store: Store
    @AppStorage("showGroupHierarchy") private var showGroups = true
    @AppStorage("sortProjectsByName") private var sortByName = false
    @AppStorage("compactSidebar") private var compactSidebar = true
    @AppStorage("worktreeByDefault") private var worktreeByDefault = false
    @AppStorage("branchPrefix") private var branchPrefix = ""
    var body: some View {
        SettingsGroup(title: "Sidebar") {
            SettingsRow(title: "Show group hierarchy") { Toggle("", isOn: $showGroups).labelsHidden().toggleStyle(.switch) }
            SettingsRow(title: "Sort projects by name") { Toggle("", isOn: $sortByName).labelsHidden().toggleStyle(.switch) }
            SettingsRow(title: "Compact rows", detail: "Hides branch and status subtitles under projects and sessions.") { Toggle("", isOn: $compactSidebar).labelsHidden().toggleStyle(.switch) }
        }
        SettingsGroup(title: "Keep computer awake", footer: "A running session may be waiting for input; keeping the Mac awake lets it finish.") {
            SettingsRow(title: "Mode") {
                Picker("", selection: Binding(get: { store.wake.mode }, set: { store.wake.mode = $0 })) {
                    Text("Always while SpecDesk is open").tag("always"); Text("While a session is running").tag("sessions"); Text("Off").tag("off")
                }.labelsHidden().frame(width: 260)
            }
        }
        SettingsGroup(title: "New sessions", footer: "Worktrees give each session an isolated checkout on its own branch under Application Support/Convoy/worktrees.") {
            SettingsRow(title: "Run new sessions in a git worktree by default") { Toggle("", isOn: $worktreeByDefault).labelsHidden().toggleStyle(.switch) }
            SettingsRow(title: "Branch prefix", detail: "Prepended to generated branch names, e.g. feature → feature/fix-delivery-status") {
                TextField("none", text: $branchPrefix).textFieldStyle(.roundedBorder).frame(width: 180)
            }
        }
    }
}

struct AppearanceSettings: View {
    @AppStorage("appearance") private var appearance = "system"
    var body: some View {
        SettingsGroup(title: "Theme", footer: "Applies to every window, popover and alert. Also ⌥⌘T to cycle.") {
            SettingsRow(title: "Appearance") {
                Picker("", selection: $appearance) { Text("System").tag("system"); Text("Light").tag("light"); Text("Dark").tag("dark") }
                    .pickerStyle(.segmented).labelsHidden().frame(width: 240)
            }
        }
    }
}

struct TerminalSettings: View {
    @AppStorage("terminalFontSize") private var fontSize = 13.0
    @AppStorage("terminalScrollback") private var scrollback = 10_000
    var body: some View {
        SettingsGroup(title: "Terminal", footer: "Applies to terminals started after the change. Snapshots of recent output are saved every few seconds for read-only viewing after a restart.") {
            SettingsRow(title: "Font size") {
                HStack { Slider(value: $fontSize, in: 10...20, step: 1).frame(width: 160); Text("\(Int(fontSize)) pt").monospacedDigit().frame(width: 40, alignment: .trailing) }
            }
            SettingsRow(title: "Scrollback lines", detail: "More lines use more memory per open terminal.") {
                Picker("", selection: $scrollback) { Text("5,000").tag(5_000); Text("10,000").tag(10_000); Text("25,000").tag(25_000); Text("50,000").tag(50_000) }.labelsHidden().frame(width: 120)
            }
        }
    }
}

struct AgentSettings: View {
    @AppStorage("defaultAgent") private var defaultAgent = "Claude Code"
    @AppStorage("yoloClaude") private var yoloClaude = false
    @AppStorage("yoloCodex") private var yoloCodex = false
    @StateObject private var detector = AgentDetector()
    @AppStorage("agentStatusHooks") private var hooks = true
    @AppStorage("claudeLimitsIntegration") private var claudeLimitsIntegration = false
    @AppStorage("hibernateAfterMinutes") private var hibernate = 0
    @AppStorage("reviewTemplate") private var reviewTemplate = ""
    var body: some View {
        SettingsGroup(title: "Default agent", footer: "Preselected in New session and new tasks. Reviews still default to the other agent. Detection runs your login shell, the same way terminals find the CLIs.") {
            HStack(spacing: 10) {
                ForEach(Agent.allCases) { agent in
                    let info = detector.info[agent]
                    Button { defaultAgent = agent.rawValue } label: {
                        HStack(spacing: 10) {
                            AgentIcon(agent: agent, size: 18).frame(width: 30, height: 30)
                                .background(Color.primary.opacity(0.06), in: RoundedRectangle(cornerRadius: 7))
                            VStack(alignment: .leading, spacing: 2) {
                                Text(agent.rawValue).font(.system(size: 13, weight: .semibold))
                                Text(!detector.checked ? "Checking…" : (info?.path == nil ? "Not found in PATH" : (info?.version ?? info?.path ?? ""))).font(.system(size: 10.5, design: .monospaced)).foregroundStyle(info?.path == nil && detector.checked ? .orange : .secondary).lineLimit(1)
                            }
                            Spacer()
                            if defaultAgent == agent.rawValue { Image(systemName: "checkmark.circle.fill").foregroundStyle(AppTheme.accent) }
                        }.padding(12).frame(maxWidth: .infinity).contentShape(Rectangle())
                            .background(defaultAgent == agent.rawValue ? AppTheme.accent.opacity(0.12) : Color.primary.opacity(0.03), in: RoundedRectangle(cornerRadius: 9))
                            .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(defaultAgent == agent.rawValue ? AppTheme.accent.opacity(0.6) : AppTheme.stroke))
                    }.buttonStyle(.plain)
                }
            }.padding(14)
        }
        SettingsGroup(title: "Permissions", footer: "Yolo mode launches Claude with --dangerously-skip-permissions and Codex with --dangerously-bypass-approvals-and-sandbox. Agents then edit files and run commands without asking. Applies to sessions started afterwards. Use it only in folders you can afford to have changed unattended.") {
            SettingsRow(title: "Claude Code: skip permission prompts (Yolo)") { Toggle("", isOn: $yoloClaude).labelsHidden().toggleStyle(.switch) }
            SettingsRow(title: "Codex: bypass approvals and sandbox (Yolo)") { Toggle("", isOn: $yoloCodex).labelsHidden().toggleStyle(.switch) }
        }
        .onAppear { if !detector.checked { detector.detect() } }
        SettingsGroup(title: "Review brief template", footer: "Used by Start review and by auto-review of tasks unless a project overrides it (project ⋯ menu → Review & task defaults). Placeholders: {{title}} {{path}} {{scope}} {{task}} {{notes}} {{spec}} {{output}} {{branch}}. Empty = built-in default.") {
            VStack(alignment: .leading, spacing: 6) {
                TextEditor(text: $reviewTemplate).font(.system(size: 12, design: .monospaced)).frame(height: 160)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke))
                HStack { Button("Insert default") { reviewTemplate = ReviewTemplate.default }.controlSize(.small); Button("Clear") { reviewTemplate = "" }.controlSize(.small) }
            }.padding(14)
        }
        SettingsGroup(title: "Hibernation", footer: "Stops Claude agents that finished and sat idle, unless you are looking at them. Opening a sleeping session resumes it automatically. Free memory, same conversation.") {
            SettingsRow(title: "Sleep finished agents after") {
                Picker("", selection: $hibernate) { Text("Never").tag(0); Text("15 min").tag(15); Text("30 min").tag(30); Text("1 hour").tag(60); Text("3 hours").tag(180) }.labelsHidden().frame(width: 110)
            }
        }
        SettingsGroup(title: "Claude Code", footer: "Hooks are passed per session with --settings, so your global ~/.claude/settings.json is never modified. State comes only from hooks, never guessed from terminal output.") {
            SettingsRow(title: "Agent status hooks", detail: "Shows working / waiting / done in tabs and the sidebar, and powers notifications.") { Toggle("", isOn: $hooks).labelsHidden().toggleStyle(.switch) }
            SettingsRow(title: "Usage in status line", detail: "Adds a SpecDesk status line to new Claude sessions so limits appear in AI Limits.") { Toggle("", isOn: $claudeLimitsIntegration).labelsHidden().toggleStyle(.switch) }
        }
        SettingsGroup(title: "Codex", footer: "Codex has no hook API, so its sessions show running / stopped only. Its session ID is captured when it prints its resume command.") {
            SettingsRow(title: "Status detection") { Text("Process state only").font(.caption).foregroundStyle(.secondary) }
        }
    }
}

struct GitSettings: View {
    @EnvironmentObject var store: Store
    @AppStorage("gitPollSeconds") private var poll = 10.0
    @AppStorage("gitEnabled") private var enabled = true
    @AppStorage("worktreeSetupCommands") private var setup = ""
    @AppStorage("worktreeSharedPaths") private var sharedPaths = ""
    var body: some View {
        SettingsGroup(title: "Branch & changes", footer: "Read with optional locks disabled so polling never races an agent’s own git commands.") {
            SettingsRow(title: "Show branch and changed-file counts") { Toggle("", isOn: $enabled).labelsHidden().toggleStyle(.switch).onChange(of: enabled) { _, v in store.git.enabled = v } }
            SettingsRow(title: "Refresh every") {
                Picker("", selection: $poll) { Text("5 s").tag(5.0); Text("10 s").tag(10.0); Text("30 s").tag(30.0); Text("60 s").tag(60.0) }.labelsHidden().frame(width: 90)
                    .onChange(of: poll) { _, v in store.git.interval = v }
            }
        }
        SettingsGroup(title: "Worktree setup hooks", footer: "Run once in every new worktree before the agent starts, e.g. pnpm install or cp ../.env .env. Per-project commands are set from the project’s ⋯ menu and run after these.") {
            VStack(alignment: .leading, spacing: 6) {
                Text("Global commands").font(.system(size: 13))
                TextEditor(text: $setup).font(.system(size: 12, design: .monospaced)).frame(height: 90)
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke))
            }.padding(14)
        }
        SettingsGroup(title: "Worktree shared paths", footer: "Gitignored paths to bring into every new worktree from the primary checkout, one per line, e.g. .env or node_modules. Cloned with APFS (instant, no extra space) or symlinked. Per-project paths are in Project settings.") {
            TextEditor(text: $sharedPaths).font(.system(size: 12, design: .monospaced)).frame(height: 70)
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke)).padding(14)
        }
        SettingsGroup(title: "Worktrees") {
            SettingsRow(title: "Location") { Text(GitWorktree.root.path).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled) }
            SettingsRow(title: "Open folder") { Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([GitWorktree.root]) } }
        }
        .onAppear { store.git.enabled = enabled; store.git.interval = poll }
    }
}

struct NotificationSettings: View {
    @AppStorage("notificationsEnabled") private var enabled = true
    @AppStorage("notifyWaiting") private var notifyWaiting = true
    @AppStorage("notifyDone") private var notifyDone = true
    @AppStorage("suppressWhenFocused") private var suppressWhenFocused = true
    @AppStorage("notificationSound") private var sound = "default"
    var body: some View {
        SettingsGroup(title: "Agent notifications", footer: "macOS may ask for permission the first time. The Dock badge counts sessions waiting for you.") {
            SettingsRow(title: "Enable notifications") { Toggle("", isOn: $enabled).labelsHidden().toggleStyle(.switch) }
            SettingsRow(title: "When an agent needs input or permission") { Toggle("", isOn: $notifyWaiting).labelsHidden().toggleStyle(.switch).disabled(!enabled) }
            SettingsRow(title: "When an agent finishes") { Toggle("", isOn: $notifyDone).labelsHidden().toggleStyle(.switch).disabled(!enabled) }
            SettingsRow(title: "Stay quiet for the session you’re looking at") { Toggle("", isOn: $suppressWhenFocused).labelsHidden().toggleStyle(.switch).disabled(!enabled) }
            SettingsRow(title: "Sound") {
                Picker("", selection: $sound) { Text("Default").tag("default"); Text("None").tag("none") }.labelsHidden().frame(width: 110).disabled(!enabled)
            }
        }
    }
}

struct LimitsSettings: View {
    @EnvironmentObject var store: Store
    @AppStorage("claudeLimitsIntegration") private var claudeLimitsIntegration = false
    var body: some View {
        SettingsGroup(title: "Claude", footer: "Shows after the next response in a Claude session started from SpecDesk. Requires Claude Code 2.1.251+ and an eligible subscription.") {
            SettingsRow(title: "Claude limits integration") { Toggle("", isOn: $claudeLimitsIntegration).labelsHidden().toggleStyle(.switch) }
        }
        SettingsGroup(title: "Codex", footer: "Read from your installed Codex login. SpecDesk never copies account tokens.") {
            SettingsRow(title: "Refresh now") { Button("Refresh") { store.usage.refresh() }.disabled(store.usage.loading) }
            if let date = store.usage.updatedAt { SettingsRow(title: "Last updated") { Text(date.formatted(date: .abbreviated, time: .shortened)).font(.caption).foregroundStyle(.secondary) } }
        }
    }
}

struct ShortcutSettings: View {
    @ObservedObject private var keys = Keybindings.shared
    var body: some View {
        HStack {
            Text("Click a shortcut and press the new keys. Backspace unbinds, Escape cancels. ⌘1–9 always jump to tabs.").font(.caption).foregroundStyle(.secondary)
            Spacer()
            Button("Reset all") { keys.resetAll() }.controlSize(.small).disabled(keys.overrides.isEmpty)
        }
        ForEach(["General", "Sessions & tabs", "Panes", "Project", "Limits"], id: \.self) { group in
            SettingsGroup(title: group) {
                ForEach(Keybindings.actions.filter { $0.group == group }) { action in
                    SettingsRow(title: action.title) { ShortcutRecorder(actionID: action.id, keys: keys) }
                }
            }
        }
    }
}

struct AccountSettings: View {
    @EnvironmentObject var store: Store
    @State private var newLabel = ""
    @State private var newAgent: Agent = .claude
    @State private var removing: ManagedAccount?

    private func add() {
        store.addAccount(newAgent, label: newLabel)
        newLabel = ""
        if store.error == nil { store.showSettings = false }
    }

    var body: some View {
        ForEach(Agent.allCases) { agent in
            SettingsGroup(title: agent.rawValue, footer: agent == .claude
                          ? "Each managed account is an isolated CLAUDE_CONFIG_DIR. Your normal ~/.claude login is the “System default” and is never modified. Switching applies to sessions started afterwards."
                          : "Each managed account is an isolated CODEX_HOME with your config.toml mirrored in. Your normal ~/.codex login is the “System default”.") {
                SettingsRow(title: "System default", detail: "Your current terminal login") {
                    if store.accounts.active(for: agent) == nil { Label("Active", systemImage: "checkmark.circle.fill").font(.caption).foregroundStyle(.green) }
                    else { Button("Use") { store.accounts.setActive(agent, label: nil) }.controlSize(.small) }
                }
                ForEach(store.accounts.accounts(for: agent)) { account in
                    SettingsRow(title: account.label, detail: store.accounts.isLoggedIn(account) ? account.home.path : "Not logged in yet — open a login terminal") {
                        HStack(spacing: 8) {
                            if store.accounts.active(for: agent)?.label == account.label { Label("Active", systemImage: "checkmark.circle.fill").font(.caption).foregroundStyle(.green) }
                            else { Button("Use") { store.accounts.setActive(agent, label: account.label) }.controlSize(.small) }
                            Button("Log in…") { if let p = store.project ?? store.workspace.projects.first { let s = LinkedSession(agent: agent, sessionID: "", title: "Login: \(agent.rawValue) · \(account.label)"); store.linkSession(s, to: p.id); store.terminals.startLogin(for: account, project: p, session: s); store.openSession(s.id, in: p.id); store.showSettings = false } }.controlSize(.small)
                            Button(role: .destructive) { removing = account } label: { Image(systemName: "trash") }.controlSize(.small)
                        }
                    }
                }
            }
        }
        SettingsGroup(title: "Add account", footer: "Creates the isolated home and opens a terminal running the provider’s login. Log in there, then pick the account in the status bar.") {
            HStack(spacing: 10) {
                Picker("", selection: $newAgent) { ForEach(Agent.allCases) { Text($0.rawValue).tag($0) } }.labelsHidden().frame(width: 140)
                TextField("Label, e.g. work", text: $newLabel).textFieldStyle(.roundedBorder)
                    .onSubmit { if !newLabel.trimmingCharacters(in: .whitespaces).isEmpty { add() } }
                Button("Add & log in", action: add).buttonStyle(.borderedProminent)
                    .disabled(newLabel.trimmingCharacters(in: .whitespaces).isEmpty)
            }.padding(14)
        }
        .alert("Remove this account?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), presenting: removing) { account in
            Button("Cancel", role: .cancel) { removing = nil }
            Button("Remove", role: .destructive) { try? store.accounts.remove(account); removing = nil }
        } message: { account in Text("Deletes the isolated login at \(account.home.path). Your system login is untouched.") }
    }
}

struct QuickCommandSettings: View {
    @EnvironmentObject var store: Store
    @State private var editing: QuickCommand?
    var body: some View {
        HStack {
            Text("Saved commands and prompts you can send to any terminal from the ⚡ menu or \(store.keys.display("session.quick")). Global ones appear in every project.").font(.caption).foregroundStyle(.secondary)
            Spacer()
            Button { editing = QuickCommand(title: "", text: "", projectID: nil) } label: { Label("Add", systemImage: "plus") }.controlSize(.small)
        }
        SettingsGroup(title: "Commands") {
            if store.quickCommands.isEmpty { Text("None yet").font(.caption).foregroundStyle(.secondary).padding(14) }
            ForEach(store.quickCommands) { command in
                SettingsRow(title: command.title, detail: String(command.text.prefix(120)).replacingOccurrences(of: "\n", with: " ")) {
                    HStack(spacing: 8) {
                        Text(command.projectID.flatMap { id in store.workspace.projects.first { $0.id == id }?.name } ?? "Global").font(.caption).foregroundStyle(.secondary)
                        if command.submit { Image(systemName: "return").font(.caption).foregroundStyle(.secondary).help("Presses Enter") }
                        Button("Edit") { editing = command }.controlSize(.small)
                        Button(role: .destructive) { store.deleteQuickCommand(command.id) } label: { Image(systemName: "trash") }.controlSize(.small)
                    }
                }
            }
        }
        .sheet(item: $editing) { command in QuickCommandSheet(command: command) }
    }
}

struct QuickCommandSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State var command: QuickCommand
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(command.title.isEmpty ? "New quick command" : "Edit quick command").font(.title2.bold())
            TextField("Title", text: $command.title).textFieldStyle(.roundedBorder)
            TextEditor(text: $command.text).font(.system(size: 12, design: .monospaced)).frame(height: 160).border(.quaternary)
            HStack {
                Picker("Scope", selection: $command.projectID) {
                    Text("Global").tag(UUID?.none)
                    ForEach(store.workspace.projects) { Text($0.name).tag(UUID?.some($0.id)) }
                }.frame(width: 260)
                Spacer()
                Toggle("Press Enter after inserting", isOn: $command.submit)
            }
            HStack { Spacer(); Button("Cancel") { dismiss() }; Button("Save") { store.saveQuickCommand(command); dismiss() }.buttonStyle(.borderedProminent).disabled(command.title.trimmingCharacters(in: .whitespaces).isEmpty || command.text.isEmpty) }
        }.padding(24).frame(width: 560)
    }
}
