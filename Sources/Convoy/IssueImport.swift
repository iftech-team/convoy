import SwiftUI

/// Model presets offered next to the free-form model field.
enum ModelPresets {
    static let claude: [(String, String)] = [("opus", "Opus · most capable"), ("sonnet", "Sonnet · balanced"), ("haiku", "Haiku · fastest")]
    static let codex: [(String, String)] = [("gpt-5-codex", "GPT-5 Codex"), ("gpt-5", "GPT-5"), ("o3", "o3")]
    static func `for`(_ agent: Agent) -> [(String, String)] { agent == .claude ? claude : codex }
}

/// Small key chip linking back to the tracker issue.
struct IssueSourceBadge: View {
    let source: IssueSource
    var body: some View {
        let chip = HStack(spacing: 3) {
            Image(systemName: source.tracker.symbol).font(.system(size: 9))
            Text(source.key).font(.system(size: 10, weight: .medium, design: .monospaced))
        }.foregroundStyle(.secondary).padding(.horizontal, 6).padding(.vertical, 2).background(Color.primary.opacity(0.06), in: Capsule())
        if let url = source.url, let link = URL(string: url) {
            Link(destination: link) { chip }.help("Open \(source.key) in \(source.tracker.rawValue)")
        } else {
            chip.help(source.viaMCP == true ? "\(source.tracker.rawValue) \(source.key) — fetched by the agent via MCP" : "\(source.tracker.rawValue) \(source.key)")
        }
    }
}

/// Compact agent + model picker; used for the import defaults and per-issue overrides.
private struct AgentModelPicker: View {
    @Binding var pick: AgentPick
    var compact = false
    var body: some View {
        HStack(spacing: 6) {
            Menu {
                ForEach(Agent.allCases) { agent in
                    Button { if pick.agent != agent { pick = AgentPick(agent: agent, model: nil) } } label: {
                        if pick.agent == agent { Label(agent.rawValue, systemImage: "checkmark") } else { Text(agent.rawValue) }
                    }
                }
            } label: {
                HStack(spacing: 5) {
                    AgentIcon(agent: pick.agent, size: 12)
                    if !compact { Text(pick.agent.rawValue).font(.system(size: 12)) }
                }
            }.menuStyle(.borderlessButton).fixedSize().help("Agent")
            HStack(spacing: 4) {
                TextField("default", text: Binding(get: { pick.model ?? "" }, set: { pick.model = $0.isEmpty ? nil : $0 }))
                    .textFieldStyle(.plain).font(.system(size: 11.5, design: .monospaced))
                Menu {
                    Button("Agent default") { pick.model = nil }
                    Divider()
                    ForEach(ModelPresets.for(pick.agent), id: \.0) { id, name in Button(name) { pick.model = id } }
                } label: { Image(systemName: "chevron.up.chevron.down").font(.system(size: 9, weight: .semibold)).foregroundStyle(.secondary) }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
            }.padding(.horizontal, 8).frame(width: compact ? 130 : 170, height: 26)
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(AppTheme.stroke))
                .help(pick.agent == .claude ? "Model passed as claude --model" : "Model passed as codex --model")
        }
    }
}

// MARK: - Import sheet

struct IssueImportSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @ObservedObject private var trackers = TrackerConnections.shared
    let project: Project

    @State private var connectionID: UUID?
    @State private var query = ""
    @State private var mineOnly = true
    @State private var manual = ""
    @State private var results: [TrackerIssue] = []
    @State private var selected: Set<String> = []
    @State private var overrides: [String: AgentPick] = [:]
    @State private var defaults = AgentPick(agent: .claude, model: nil)
    @State private var mode = "pr"
    @State private var autoReview = false
    @State private var runNow = false
    @State private var loading = false
    @State private var message: String?

    private var connection: TrackerConnection? { trackers.connections.first { $0.id == connectionID } ?? trackers.connections.first }
    private var isMCP: Bool { connection?.auth == .mcp }
    private var candidates: [TrackerIssue] { isMCP ? TrackerClient.manualIssues(manual, kind: connection?.kind ?? .linear) : results }
    private var alreadyImported: Set<String> {
        Set(store.tasks(for: project).compactMap { s in s.source.flatMap { $0.tracker == connection?.kind ? $0.key : nil } })
    }
    private var chosen: [TrackerIssue] { candidates.filter { selected.contains($0.key) && !alreadyImported.contains($0.key) } }

    private func label(_ text: String) -> some View {
        Text(text.uppercased()).font(.system(size: 10, weight: .semibold)).tracking(0.6).foregroundStyle(.secondary)
    }

    private func search() {
        guard let connection, !isMCP else { return }
        guard let secret = trackers.secret(for: connection), !secret.isEmpty else { message = "No saved credentials for “\(connection.name)”. Edit it in Settings → Integrations."; return }
        loading = true; message = nil
        Task {
            do {
                let found = try await TrackerClient.fetch(connection, secret: secret, query: query, mineOnly: mineOnly)
                results = found
                selected = Set(found.map(\.key)).subtracting(alreadyImported)
                if found.isEmpty { message = "No matching open issues." }
            } catch {
                results = []; message = error.localizedDescription
            }
            loading = false
        }
    }

    private func importNow() {
        guard let connection else { return }
        let options = IssueImportOptions(agent: defaults.agent, model: defaults.model, mode: mode, autoReview: autoReview, runNow: runNow,
                                         overrides: overrides.filter { key, _ in selected.contains(key) })
        store.importIssues(chosen, from: connection, into: project, options: options)
        dismiss()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 10) {
                Text("Import issues").font(.system(size: 18, weight: .bold))
                Spacer()
                HStack(spacing: 6) {
                    ProjectIconView(project: project, size: 14)
                    Text(project.name).font(.system(size: 12, weight: .semibold)).foregroundStyle(project.tint)
                }.padding(.horizontal, 8).padding(.vertical, 4).background(project.tint.opacity(0.12), in: Capsule())
            }
            if trackers.connections.isEmpty {
                emptyState
            } else {
                sourceRow
                if isMCP { manualEntry } else { searchRow }
                issueList
                defaultsSection
            }
            footer
        }
        .padding(24).frame(width: 720)
        .onAppear {
            connectionID = connectionID ?? trackers.connections.first?.id
            mode = project.taskMode ?? "pr"
            defaults.agent = project.defaultAgent ?? Agent(rawValue: UserDefaults.standard.string(forKey: "defaultAgent") ?? "") ?? .claude
            if !isMCP, connection != nil { search() }
        }
    }

    private var emptyState: some View {
        VStack(spacing: 10) {
            Image(systemName: "link").font(.system(size: 24)).foregroundStyle(.tertiary)
            Text("Connect Linear or Jira").font(.system(size: 13, weight: .semibold))
            Text("Add an API key, a Jira login, or use the agent’s own MCP server. Imported issues become tasks you can run with any agent and model.")
                .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center).frame(maxWidth: 420)
            Button("Open Settings → Integrations") { store.settingsSection = "Integrations"; store.showSettings = true; dismiss() }.buttonStyle(.borderedProminent)
        }.frame(maxWidth: .infinity).padding(.vertical, 30)
    }

    private var sourceRow: some View {
        HStack(spacing: 10) {
            label("From")
            Picker("", selection: Binding(get: { connection?.id }, set: { connectionID = $0; results = []; selected = []; overrides = [:]; message = nil; if !isMCP { search() } })) {
                ForEach(trackers.connections) { c in Label("\(c.name) · \(c.auth.label(for: c.kind))", systemImage: c.kind.symbol).tag(Optional(c.id)) }
            }.labelsHidden().frame(maxWidth: 320)
            Spacer()
            Button("Manage…") { store.settingsSection = "Integrations"; store.showSettings = true; dismiss() }.controlSize(.small)
        }
    }

    private var searchRow: some View {
        HStack(spacing: 8) {
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass").font(.system(size: 11)).foregroundStyle(.secondary)
                TextField(connection?.kind == .jira ? "Text, issue key, or JQL" : "Title text or issue key, e.g. ENG-123", text: $query)
                    .textFieldStyle(.plain).font(.system(size: 12.5)).onSubmit(search)
            }.padding(.horizontal, 8).frame(height: 28)
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
            Toggle("Assigned to me", isOn: $mineOnly).toggleStyle(.checkbox).font(.system(size: 12)).onChange(of: mineOnly) { _, _ in search() }
            Button(action: search) { if loading { ProgressView().controlSize(.small) } else { Text("Search") } }.disabled(loading)
        }
    }

    private var manualEntry: some View {
        VStack(alignment: .leading, spacing: 6) {
            label("Issue keys or links")
            TextEditor(text: $manual).font(.system(size: 12.5, design: .monospaced)).scrollContentBackground(.hidden).padding(6).frame(height: 80)
                .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 7))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(AppTheme.stroke))
                .overlay(alignment: .topLeading) {
                    if manual.isEmpty {
                        Text("ENG-123 Optional title\nhttps://linear.app/acme/issue/ENG-124/…").font(.system(size: 12.5, design: .monospaced)).foregroundStyle(.tertiary)
                            .padding(.horizontal, 11).padding(.vertical, 6).allowsHitTesting(false)
                    }
                }
                .onChange(of: manual) { _, _ in selected = Set(candidates.map(\.key)) }
            Text("The agent reads each issue itself through its \(connection?.kind.rawValue ?? "") MCP server, so it must be configured in Claude Code or Codex (e.g. `claude mcp add --transport http linear https://mcp.linear.app/mcp`).")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
        }
    }

    private var issueList: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                label("Issues")
                if !candidates.isEmpty {
                    Text("\(chosen.count) of \(candidates.count) selected").font(.system(size: 11)).foregroundStyle(.tertiary)
                    Spacer()
                    Button(selected.isSuperset(of: candidates.map(\.key)) ? "Select none" : "Select all") {
                        selected = selected.isSuperset(of: candidates.map(\.key)) ? [] : Set(candidates.map(\.key))
                    }.buttonStyle(.link).font(.system(size: 11))
                }
            }
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(candidates) { issue in row(issue) }
                    if candidates.isEmpty {
                        Text(message ?? (loading ? "Loading…" : (isMCP ? "Paste issue keys or links above." : "No issues loaded.")))
                            .font(.caption).foregroundStyle(message == nil ? AnyShapeStyle(.tertiary) : AnyShapeStyle(.orange))
                            .frame(maxWidth: .infinity).padding(.vertical, 28)
                    }
                }
            }.frame(height: 240)
                .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 9))
                .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(AppTheme.stroke))
            if let message, !candidates.isEmpty { Text(message).font(.caption).foregroundStyle(.orange) }
        }
    }

    private func row(_ issue: TrackerIssue) -> some View {
        let imported = alreadyImported.contains(issue.key)
        let isOn = Binding(get: { selected.contains(issue.key) && !imported }, set: { if $0 { selected.insert(issue.key) } else { selected.remove(issue.key) } })
        let pick = Binding(get: { overrides[issue.key] ?? defaults }, set: { overrides[issue.key] = $0 == defaults ? nil : $0 })
        return HStack(spacing: 10) {
            Toggle("", isOn: isOn).labelsHidden().toggleStyle(.checkbox).disabled(imported)
            Text(issue.key).font(.system(size: 11, weight: .medium, design: .monospaced)).foregroundStyle(.secondary).frame(width: 78, alignment: .leading)
            VStack(alignment: .leading, spacing: 1) {
                Text(issue.title).font(.system(size: 12.5)).lineLimit(1)
                if imported { Text("Already imported").font(.system(size: 10)).foregroundStyle(.tertiary) }
                else if let status = issue.status { Text([status, issue.priority].compactMap { $0 }.joined(separator: " · ")).font(.system(size: 10)).foregroundStyle(.tertiary) }
            }
            Spacer(minLength: 8)
            if !imported {
                AgentModelPicker(pick: pick, compact: true).opacity(overrides[issue.key] == nil ? 0.55 : 1)
                    .help("Agent and model for this issue only")
            }
        }.padding(.horizontal, 12).frame(height: 40)
            .overlay(alignment: .bottom) { Divider().padding(.leading, 36).opacity(0.6) }
            .opacity(imported ? 0.6 : 1)
    }

    private var defaultsSection: some View {
        HStack(alignment: .top, spacing: 18) {
            VStack(alignment: .leading, spacing: 6) {
                label("Agent & model for all")
                AgentModelPicker(pick: $defaults)
            }
            VStack(alignment: .leading, spacing: 6) {
                label("When done")
                Picker("", selection: $mode) {
                    Text("Pull request").tag("pr"); Text("Push to main").tag("push"); Text("Commit only").tag("none")
                }.labelsHidden().frame(width: 150)
            }
            VStack(alignment: .leading, spacing: 6) {
                label("Review")
                Toggle("Auto-review", isOn: $autoReview).toggleStyle(.checkbox).font(.system(size: 12)).frame(height: 26)
            }
        }
    }

    private var footer: some View {
        HStack(spacing: 12) {
            if !trackers.connections.isEmpty {
                Toggle(isOn: $runNow) { Text("Run immediately").font(.system(size: 12)) }.toggleStyle(.checkbox)
                Text(runNow ? "Each task starts in its own worktree." : "Tasks wait in the queue.").font(.caption).foregroundStyle(.tertiary)
            }
            Spacer()
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            if !trackers.connections.isEmpty {
                Button { importNow() } label: {
                    Label(chosen.isEmpty ? "Import" : "Import \(chosen.count)", systemImage: runNow ? "play.fill" : "square.and.arrow.down")
                }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(chosen.isEmpty)
            }
        }
    }
}

// MARK: - Settings

struct IntegrationSettings: View {
    @ObservedObject private var trackers = TrackerConnections.shared
    @State private var editing: TrackerConnection?
    @State private var removing: TrackerConnection?

    var body: some View {
        HStack {
            Text("Import Linear and Jira issues as tasks from the Tasks page. Secrets are stored in the macOS Keychain.").font(.caption).foregroundStyle(.secondary)
            Spacer()
            Menu {
                ForEach(TrackerKind.allCases) { kind in
                    Button { editing = TrackerConnection(kind: kind, auth: .apiKey, name: kind.rawValue) } label: { Label(kind.rawValue, systemImage: kind.symbol) }
                }
            } label: { Label("Add", systemImage: "plus") }.fixedSize().controlSize(.small)
        }
        SettingsGroup(title: "Connections", footer: "MCP connections store no credentials: the agent fetches each issue with its own Linear or Atlassian MCP server.") {
            if trackers.connections.isEmpty { Text("None yet").font(.caption).foregroundStyle(.secondary).padding(14) }
            ForEach(trackers.connections) { c in
                SettingsRow(title: c.name, detail: [c.kind.rawValue, c.auth.label(for: c.kind), c.site, c.username].compactMap { $0?.isEmpty == false ? $0 : nil }.joined(separator: " · ")) {
                    HStack(spacing: 8) {
                        Button("Edit") { editing = c }.controlSize(.small)
                        Button(role: .destructive) { removing = c } label: { Image(systemName: "trash") }.controlSize(.small)
                    }
                }
            }
        }
        .sheet(item: $editing) { TrackerConnectionSheet(connection: $0) }
        .alert("Remove this connection?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), presenting: removing) { c in
            Button("Cancel", role: .cancel) { removing = nil }
            Button("Remove", role: .destructive) { trackers.remove(c); removing = nil }
        } message: { c in Text("Deletes “\(c.name)” and its saved secret. Already imported tasks are kept.") }
    }
}

private struct TrackerConnectionSheet: View {
    @Environment(\.dismiss) private var dismiss
    @ObservedObject private var trackers = TrackerConnections.shared
    @State var connection: TrackerConnection
    @State private var secret = ""
    @State private var testing = false
    @State private var result: (ok: Bool, text: String)?

    private var isExisting: Bool { trackers.connections.contains { $0.id == connection.id } }
    private var hasStoredSecret: Bool { isExisting && trackers.secret(for: connection) != nil }
    private var needsSecret: Bool { connection.auth != .mcp }
    private var canSave: Bool {
        guard !connection.name.trimmingCharacters(in: .whitespaces).isEmpty else { return false }
        guard needsSecret else { return true }
        if connection.kind == .jira && connection.siteURL == nil { return false }
        if connection.auth == .password && (connection.username ?? "").trimmingCharacters(in: .whitespaces).isEmpty { return false }
        return !secret.isEmpty || hasStoredSecret
    }
    private var secretLabel: String {
        switch (connection.kind, connection.auth) {
        case (.linear, _): "Personal API key"
        case (.jira, .password): "Password"
        default: "API token or personal access token"
        }
    }
    private var help: String {
        switch (connection.kind, connection.auth) {
        case (.linear, .mcp): "Add Linear’s MCP server to your agent, e.g. `claude mcp add --transport http linear https://mcp.linear.app/mcp`, then import by issue key or link."
        case (.jira, .mcp): "Add the Atlassian MCP server to your agent, then import by issue key or link."
        case (.linear, _): "Create a key in Linear → Settings → Security & access → Personal API keys."
        case (.jira, .password): "Jira Server / Data Center with basic authentication. Jira Cloud needs an API token instead."
        default: "Jira Cloud: your account email plus an API token from id.atlassian.com → Security → API tokens. Data Center: leave the email empty and paste a personal access token."
        }
    }

    private func test() {
        let token = secret.isEmpty ? (trackers.secret(for: connection) ?? "") : secret
        testing = true; result = nil
        Task {
            do {
                let issues = try await TrackerClient.fetch(connection, secret: token, query: "", mineOnly: true)
                result = (true, "Connected — \(issues.count) open issue\(issues.count == 1 ? "" : "s") assigned to you.")
            } catch { result = (false, error.localizedDescription) }
            testing = false
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 8) {
                Image(systemName: connection.kind.symbol)
                Text("\(isExisting ? "Edit" : "Add") \(connection.kind.rawValue) connection").font(.system(size: 17, weight: .bold))
            }
            Form {
                TextField("Name", text: $connection.name)
                Picker("Connect with", selection: $connection.auth) {
                    ForEach(TrackerAuth.options(for: connection.kind)) { Text($0.label(for: connection.kind)).tag($0) }
                }
                if connection.kind == .jira && needsSecret {
                    TextField("Site URL", text: Binding(get: { connection.site ?? "" }, set: { connection.site = $0 }), prompt: Text("https://acme.atlassian.net"))
                    TextField(connection.auth == .password ? "Username" : "Email", text: Binding(get: { connection.username ?? "" }, set: { connection.username = $0 }))
                }
                if needsSecret {
                    SecureField(secretLabel, text: $secret, prompt: Text(hasStoredSecret ? "Saved — leave empty to keep" : ""))
                }
            }.formStyle(.grouped).frame(height: needsSecret ? (connection.kind == .jira ? 250 : 160) : 120)
            Text(help).font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            if let result { Label(result.text, systemImage: result.ok ? "checkmark.circle.fill" : "exclamationmark.triangle.fill").font(.caption).foregroundStyle(result.ok ? .green : .orange) }
            HStack {
                if needsSecret {
                    Button(action: test) { if testing { ProgressView().controlSize(.small) } else { Text("Test connection") } }.disabled(!canSave || testing)
                }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") {
                    do { try trackers.save(connection, secret: secret); dismiss() } catch { result = (false, error.localizedDescription) }
                }.buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction).disabled(!canSave)
            }
        }.padding(22).frame(width: 520)
    }
}
