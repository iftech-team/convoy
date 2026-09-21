import Foundation
import SwiftUI
import UsageTelemetry

struct LimitWindow: Decodable, Equatable {
    let usedPercent: Double
    let windowDurationMins: Int?
    let resetsAt: Int64?

    var percent: Double { min(100, max(0, usedPercent)) }
    var label: String {
        guard let minutes = windowDurationMins else { return "Usage window" }
        if minutes % 1440 == 0 { return "\(minutes / 1440)d window" }
        if minutes % 60 == 0 { return "\(minutes / 60)h window" }
        return "\(minutes)m window"
    }
    var resetDate: Date? { resetsAt.map { Date(timeIntervalSince1970: Double($0)) } }
}

struct LimitBucket: Decodable, Identifiable {
    var limitId: String?
    var limitName: String?
    var primary: LimitWindow?
    var secondary: LimitWindow?
    var id: String { limitId ?? "codex" }
}

struct LimitsResponse: Decodable {
    var rateLimits: LimitBucket?
    var rateLimitsByLimitId: [String: LimitBucket]?

    var buckets: [LimitBucket] {
        if let all = rateLimitsByLimitId, !all.isEmpty {
            return all.keys.sorted().compactMap { key in
                guard var bucket = all[key] else { return nil }
                bucket.limitId = key
                return bucket
            }
        }
        return rateLimits.map { [$0] } ?? []
    }
}

/// Reads only the supported rate-limit method. No account tokens are read or
/// copied by SpecDesk; the installed Codex process handles its own login.
@MainActor
final class UsageLimits: ObservableObject {
    @Published private(set) var buckets: [LimitBucket] = []
    @Published private(set) var loading = false
    @Published private(set) var error: String?
    @Published private(set) var updatedAt: Date?
    @Published private(set) var claudeSnapshot: ClaudeSnapshot?

    func readClaudeSnapshot() {
        claudeSnapshot = (try? Data(contentsOf: ClaudeSnapshot.cacheURL)).flatMap { try? JSONDecoder().decode(ClaudeSnapshot.self, from: $0) }
    }
    private var process: Process?
    private var output: Pipe?
    private var input: Pipe?
    private var buffer = Data()
    private var timeout: Task<Void, Never>?
    private var requestID = UUID()

    var summary: String {
        if error != nil { return "Codex · refresh needed" }
        guard let window = (buckets.first { $0.id == "codex" } ?? buckets.first)?.primary else { return "Codex limits" }
        return "Codex \(Int(window.percent))% used"
    }

    func refresh() {
        guard !loading else { return }
        let requestID = UUID(); self.requestID = requestID
        loading = true; error = nil; buffer = Data()
        let process = Process(), output = Pipe(), input = Pipe()
        self.process = process; self.output = output; self.input = input
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-ilc", "exec codex app-server --listen stdio://"]
        process.currentDirectoryURL = FileManager.default.homeDirectoryForCurrentUser
        process.standardInput = input; process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        output.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let data = handle.availableData
            Task { @MainActor in
                guard self?.requestID == requestID else { return }
                self?.receive(data)
            }
        }
        process.terminationHandler = { [weak self] _ in
            Task { @MainActor in
                guard let self, self.loading, self.requestID == requestID else { return }
                self.fail("Codex could not read limits. Check that the CLI is installed and signed in.")
            }
        }
        do {
            try process.run()
            send(["id": 1, "method": "initialize", "params": ["clientInfo": ["name": "specdesk", "version": "0.3.0"]]])
            timeout = Task { [weak self] in
                do { try await Task.sleep(for: .seconds(25)) } catch { return }
                if self?.requestID == requestID {
                    self?.fail("The usage request timed out. Check your connection and Codex login, then retry.")
                }
            }
        } catch { fail("Could not start Codex: \(error.localizedDescription)") }
    }

    private func send(_ message: [String: Any]) {
        do {
            var data = try JSONSerialization.data(withJSONObject: message)
            data.append(10)
            try input?.fileHandleForWriting.write(contentsOf: data)
        } catch { fail("Could not communicate with Codex.") }
    }

    private func receive(_ data: Data) {
        guard loading, !data.isEmpty else { return }
        buffer.append(data)
        guard buffer.count < 2_000_000 else { fail("Codex returned an unexpectedly large response."); return }
        while let newline = buffer.firstIndex(of: 10) {
            let line = Data(buffer[..<newline]); buffer.removeSubrange(...newline)
            guard let message = try? JSONSerialization.jsonObject(with: line) as? [String: Any], let id = message["id"] as? Int else { continue }
            if let failure = message["error"] as? [String: Any] {
                fail(failure["message"] as? String ?? "Usage limits are unavailable for this account."); return
            }
            if id == 1 {
                send(["method": "initialized"])
                send(["id": 2, "method": "account/rateLimits/read"])
            } else if id == 2 {
                do {
                    let raw = try JSONSerialization.data(withJSONObject: message["result"] ?? [:])
                    buckets = try JSONDecoder().decode(LimitsResponse.self, from: raw).buckets
                    updatedAt = Date()
                    if buckets.allSatisfy({ $0.primary == nil && $0.secondary == nil }) {
                        error = "This account did not return usage windows. API billing and subscription limits are different."
                    }
                    finish()
                } catch { fail("The installed Codex version returned an unsupported limits response.") }
                return
            }
        }
    }

    private func fail(_ message: String) { error = message; finish() }
    func finish() {
        loading = false
        timeout?.cancel(); timeout = nil
        output?.fileHandleForReading.readabilityHandler = nil
        process?.terminationHandler = nil
        try? input?.fileHandleForWriting.close()
        if process?.isRunning == true { process?.terminate() }
        input = nil; output = nil; process = nil
    }
}

@MainActor
final class WakeControl: ObservableObject {
    @Published var mode: String = UserDefaults.standard.string(forKey: "wakeMode") ?? "off" {
        didSet { UserDefaults.standard.set(mode, forKey: "wakeMode"); reconcile() }
    }
    @Published private(set) var active = false
    var runningCount = 0 { didSet { reconcile() } }
    private var activity: NSObjectProtocol?

    func reconcile() {
        let needed = mode == "always" || (mode == "sessions" && runningCount > 0)
        if needed && activity == nil {
            activity = ProcessInfo.processInfo.beginActivity(options: [.idleSystemSleepDisabled], reason: "Keep SpecDesk sessions running")
        } else if !needed, let activity {
            ProcessInfo.processInfo.endActivity(activity); self.activity = nil
        }
        active = needed
    }
    func stop() {
        if let activity { ProcessInfo.processInfo.endActivity(activity); self.activity = nil }
        active = false
    }
}

struct WorkspaceStatusBar: View {
    @EnvironmentObject var store: Store
    @ObservedObject var usage: UsageLimits
    @ObservedObject var terminals: TerminalManager
    @ObservedObject var wake: WakeControl
    @State private var showLimits = false

    var body: some View {
        HStack(spacing: 16) {
            Button { showLimits.toggle() } label: {
                HStack(spacing: 8) {
                    Image(systemName: "chart.bar.xaxis")
                    Text("AI Limits").fontWeight(.medium)
                    Divider().frame(height: 12)
                    TimelineView(.periodic(from: .now, by: 60)) { context in
                        Text(usage.loading ? "Refreshing…" : usage.summary + ((usage.updatedAt.map { context.date.timeIntervalSince($0) > 300 } ?? false) ? " · cached" : ""))
                            .foregroundStyle(.secondary)
                    }
                }
            }.buttonStyle(.plain).help("Account usage limits and reset times")
                .popover(isPresented: $showLimits, arrowEdge: .bottom) {
                    LimitsPanel(usage: usage)
                        .environmentObject(store)
                }
            if let snapshot = usage.claudeSnapshot, let window = snapshot.availableWindows()["five_hour"] ?? snapshot.availableWindows()["seven_day"] {
                Text("Claude \(Int(window.used_percentage))% used\(Date().timeIntervalSince(snapshot.updatedAt) > 300 ? " · cached" : "")")
                    .foregroundStyle(.secondary)
            }
            ForEach(Agent.allCases) { agent in
                let managed = store.accounts.accounts(for: agent)
                if !managed.isEmpty {
                    Menu {
                        Text("\(agent.rawValue) account for new sessions")
                        Button { store.accounts.setActive(agent, label: nil) } label: { Label("System default", systemImage: store.accounts.active(for: agent) == nil ? "checkmark" : "person") }
                        ForEach(managed) { account in
                            Button { store.accounts.setActive(agent, label: account.label) } label: {
                                Label(account.label + (store.accounts.isLoggedIn(account) ? "" : " (not logged in)"), systemImage: store.accounts.active(for: agent)?.label == account.label ? "checkmark" : "person.crop.circle")
                            }
                        }
                        Divider()
                        Button("Manage accounts…") { store.showSettings = true; store.settingsSection = "Accounts" }
                    } label: {
                        Label(store.accounts.active(for: agent)?.label ?? (agent == .claude ? "Claude: default" : "Codex: default"), systemImage: "person.crop.circle").foregroundStyle(.secondary)
                    }.menuStyle(.borderlessButton).fixedSize().help("Hot-swap the \(agent.rawValue) login used by new sessions")
                }
            }
            Spacer()
            if !store.needsYou.isEmpty {
                Button { if let first = store.needsYou.first, let project = store.project(ofSession: first.id) { store.openSession(first.id, in: project.id) } } label: {
                    Label("\(store.needsYou.count) need\(store.needsYou.count == 1 ? "s" : "") you", systemImage: "questionmark.circle.fill").foregroundStyle(.orange)
                }.buttonStyle(.plain).help(store.needsYou.map(\.title).joined(separator: ", "))
            }
            Label("\(terminals.runningCount) running", systemImage: "terminal").foregroundStyle(.secondary)
            Menu {
                Picker("Keep awake", selection: $wake.mode) {
                    Text("Always while SpecDesk is open").tag("always")
                    Text("While a session is running").tag("sessions")
                    Text("Off — normal system sleep").tag("off")
                }
                Divider()
                Button { store.showSettings = true } label: { Label("Settings…", systemImage: "gearshape") }
            } label: {
                Label(wake.active ? "Awake" : "Keep awake", systemImage: wake.active ? "cup.and.saucer.fill" : "cup.and.saucer")
                    .foregroundStyle(wake.active ? AppTheme.accent : .secondary)
            }.menuStyle(.borderlessButton).fixedSize().help("Keep the Mac awake while agents work")
        }.font(.system(size: 11)).padding(.horizontal, 18).frame(height: 34)
            .background(AppTheme.raised).overlay(alignment: .top) { Divider() }
            .onReceive(terminals.objectWillChange) { _ in
                // Handles publish after start/stop; defer to read the settled count.
                Task { @MainActor in wake.runningCount = terminals.runningCount }
            }
            .onAppear { wake.runningCount = terminals.runningCount }
            .onChange(of: store.showLimitsRequest) { _, _ in showLimits = true }
            .task {
                while !Task.isCancelled {
                    usage.readClaudeSnapshot()
                    do { try await Task.sleep(for: .seconds(5)) } catch { break }
                }
            }
    }
}

struct LimitsPanel: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var usage: UsageLimits
    @AppStorage("claudeLimitsIntegration") private var claudeLimitsIntegration = false

    private var claudeWindows: [LimitWindow] {
        guard let snapshot = usage.claudeSnapshot else { return [] }
        let all = snapshot.availableWindows()
        return [("five_hour", 300), ("seven_day", 10080)].compactMap { key, minutes in
            all[key].map { LimitWindow(usedPercent: $0.used_percentage, windowDurationMins: minutes, resetsAt: $0.resets_at) }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .firstTextBaseline) {
                Text("AI Limits").font(.system(size: 15, weight: .semibold))
                Spacer()
                Button { usage.refresh() } label: {
                    if usage.loading { ProgressView().controlSize(.mini) } else { Image(systemName: "arrow.clockwise") }
                }.buttonStyle(.plain).foregroundStyle(.secondary).disabled(usage.loading).help("Refresh Codex limits")
            }
            card(title: "Codex", icon: "terminal", status: usage.updatedAt.map { "Updated \($0.formatted(date: .omitted, time: .shortened))" }) {
                if let error = usage.error {
                    Text(error).font(.caption).foregroundStyle(.orange).fixedSize(horizontal: false, vertical: true)
                } else if usage.buckets.isEmpty {
                    Text(usage.loading ? "Reading account limits…" : "No limits yet. Refresh to read your Codex account.")
                        .font(.caption).foregroundStyle(.secondary)
                } else {
                    ForEach(usage.buckets) { bucket in
                        if usage.buckets.count > 1 { Text(bucket.limitName ?? bucket.id).font(.caption.weight(.semibold)).foregroundStyle(.secondary) }
                        if let primary = bucket.primary { window(primary) }
                        if let secondary = bucket.secondary { window(secondary) }
                    }
                }
            }
            card(title: "Claude Code", icon: "sparkle", status: usage.claudeSnapshot.map { "Updated \($0.updatedAt.formatted(date: .omitted, time: .shortened))" }) {
                if !claudeWindows.isEmpty {
                    ForEach(Array(claudeWindows.enumerated()), id: \.offset) { _, w in window(w) }
                } else if claudeLimitsIntegration {
                    Text("Shows after the next response in a Claude session started from SpecDesk.")
                        .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                } else {
                    HStack(spacing: 6) {
                        Text("Integration is off.").font(.caption).foregroundStyle(.secondary)
                        Toggle("Turn on", isOn: $claudeLimitsIntegration).toggleStyle(.switch).controlSize(.mini).labelsHidden()
                        Text("Turn on").font(.caption)
                    }
                }
                Button { store.openClaudeUsage(); dismiss() } label: {
                    Label("Open /usage in a terminal", systemImage: "arrow.up.forward.square").font(.caption)
                }.buttonStyle(.plain).foregroundStyle(AppTheme.accent).disabled(store.project == nil)
                    .help(store.project == nil ? "Choose a project first" : "Runs Claude's /usage in the selected project")
            }
            HStack {
                Text("Usage is per account and shared with your other sessions.").font(.system(size: 10)).foregroundStyle(.tertiary)
                Spacer()
                Button { store.showSettings = true; store.settingsSection = "AI Limits"; dismiss() } label: { Text("Settings…").font(.system(size: 10)) }.buttonStyle(.plain).foregroundStyle(.secondary)
            }
        }.padding(16).frame(width: 340)
            .onAppear { if usage.updatedAt == nil && !usage.loading { usage.refresh() } }
    }

    private func card<Content: View>(title: String, icon: String, status: String?, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 6) {
                Image(systemName: icon).font(.system(size: 11, weight: .semibold)).foregroundStyle(AppTheme.accent)
                Text(title).font(.system(size: 12, weight: .semibold))
                Spacer()
                if let status { Text(status).font(.system(size: 10)).foregroundStyle(.tertiary) }
            }
            content()
        }.padding(12).frame(maxWidth: .infinity, alignment: .leading)
            .background(AppTheme.card, in: RoundedRectangle(cornerRadius: 9))
            .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(AppTheme.stroke))
    }

    private func window(_ window: LimitWindow) -> some View {
        let tint: Color = window.percent >= 90 ? .red : (window.percent >= 70 ? .orange : AppTheme.accent)
        return VStack(alignment: .leading, spacing: 5) {
            HStack(alignment: .firstTextBaseline) {
                Text(window.label.replacingOccurrences(of: " window", with: "")).font(.system(size: 12, weight: .medium))
                Text(resetText(window.resetDate)).font(.system(size: 10)).foregroundStyle(.secondary)
                    .help(window.resetDate.map { "Resets \($0.formatted(date: .abbreviated, time: .shortened))" } ?? "")
                Spacer()
                Text("\(Int(window.percent))%").font(.system(size: 12, weight: .semibold)).monospacedDigit().foregroundStyle(tint)
            }
            GeometryReader { geo in
                ZStack(alignment: .leading) {
                    Capsule().fill(Color.primary.opacity(0.08))
                    Capsule().fill(tint).frame(width: max(4, geo.size.width * window.percent / 100))
                }
            }.frame(height: 5)
        }
    }

    private func resetText(_ date: Date?) -> String {
        guard let date else { return "" }
        let seconds = date.timeIntervalSinceNow
        guard seconds > 0 else { return "· resets soon" }
        let minutes = Int(seconds / 60)
        if minutes < 60 { return "· resets in \(max(1, minutes))m" }
        let hours = minutes / 60
        if hours < 24 { return "· resets in \(hours)h \(minutes % 60)m" }
        return "· resets in \(hours / 24)d \(hours % 24)h"
    }
}
