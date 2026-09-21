import AppKit
import SwiftUI
import UserNotifications

/// What an agent is doing, reported by Claude Code hooks (never inferred from terminal titles).
enum AgentState: String, Codable {
    case idle, working, waiting, done, ended
    var needsYou: Bool { self == .waiting }
}

struct AgentStatusRecord: Equatable {
    var state: AgentState
    var event: String
    var tool: String
    var at: Date
}

/// Watches the hook drop folder and publishes per-session state. Also drives notifications and the Dock badge.
@MainActor
final class AgentStatusStore: ObservableObject {
    @Published private(set) var records: [String: AgentStatusRecord] = [:]   // keyed by provider session id (lowercased)
    @Published private(set) var unseenDone: Set<String> = []
    private var source: DispatchSourceFileSystemObject?
    private var fileDescriptor: Int32 = -1
    private var timer: Timer?
    private var lastNotified: [String: Date] = [:]
    var onAttention: ((String, AgentState) -> Void)?

    static let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/AgentStatus")
    static let staleAfter: TimeInterval = 30 * 60

    init() {
        try? FileManager.default.createDirectory(at: Self.directory, withIntermediateDirectories: true)
        fileDescriptor = open(Self.directory.path, O_EVTONLY)
        if fileDescriptor >= 0 {
            let source = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fileDescriptor, eventMask: [.write, .extend, .attrib], queue: .main)
            source.setEventHandler { [weak self] in self?.reload() }
            source.resume()
            self.source = source
        }
        timer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in Task { @MainActor in self?.reload() } }
        reload()
    }

    func state(for sessionID: String) -> AgentState? {
        guard let record = records[sessionID.lowercased()] else { return nil }
        if Date().timeIntervalSince(record.at) > Self.staleAfter && record.state == .working { return nil }
        return record.state
    }

    func acknowledge(_ sessionID: String) { unseenDone.remove(sessionID.lowercased()) }

    func forget(_ sessionID: String) {
        let id = sessionID.lowercased()
        records[id] = nil; unseenDone.remove(id)
        try? FileManager.default.removeItem(at: Self.directory.appendingPathComponent("\(id).json"))
    }

    func reload() {
        guard let files = try? FileManager.default.contentsOfDirectory(at: Self.directory, includingPropertiesForKeys: nil) else { return }
        var next: [String: AgentStatusRecord] = [:]
        for file in files where file.pathExtension == "json" {
            guard let data = try? Data(contentsOf: file),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let raw = json["state"] as? String, let state = AgentState(rawValue: raw) else { continue }
            let at = Date(timeIntervalSince1970: json["at"] as? Double ?? 0)
            next[file.deletingPathExtension().lastPathComponent] = AgentStatusRecord(state: state, event: json["event"] as? String ?? "", tool: json["tool"] as? String ?? "", at: at)
        }
        guard next != records else { return }
        for (id, record) in next {
            let previous = records[id]
            guard previous?.at != record.at, previous != nil || record.state != .idle else { continue }
            if record.state == .done, previous?.state == .working || previous?.state == .waiting {
                unseenDone.insert(id); onAttention?(id, .done)
            } else if record.state == .waiting, previous?.state != .waiting {
                onAttention?(id, .waiting)
            } else if record.state == .working { unseenDone.remove(id) }
        }
        records = next
    }

    /// Dedupe bursts: the same session is not notified twice within the cooldown.
    func shouldNotify(_ sessionID: String, cooldown: TimeInterval = 5) -> Bool {
        if let last = lastNotified[sessionID], Date().timeIntervalSince(last) < cooldown { return false }
        lastNotified[sessionID] = Date(); return true
    }
}

/// macOS notifications for sessions that need attention. Category identifier carries the Convoy session UUID.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    var onOpen: ((UUID) -> Void)?
    private var authorized = false

    override init() {
        super.init()
        // UserNotifications raises an Objective-C exception outside an app bundle
        // (including swift run and the native test host).
        guard Bundle.main.bundleURL.pathExtension == "app", Bundle.main.bundleIdentifier != nil else { return }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound, .badge]) { [weak self] granted, _ in
            Task { @MainActor in self?.authorized = granted }
        }
    }

    func post(title: String, body: String, sessionID: UUID, sound: String) {
        guard authorized else { return }
        let content = UNMutableNotificationContent()
        content.title = title; content.body = body
        content.userInfo = ["session": sessionID.uuidString]
        if sound != "none" { content.sound = sound == "default" ? .default : UNNotificationSound(named: UNNotificationSoundName(sound)) }
        UNUserNotificationCenter.current().add(UNNotificationRequest(identifier: "session-\(sessionID.uuidString)", content: content, trigger: nil))
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let raw = response.notification.request.content.userInfo["session"] as? String
        await MainActor.run {
            if let raw, let id = UUID(uuidString: raw) { self.onOpen?(id) }
            NSApp.activate(ignoringOtherApps: true)
        }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }
}

/// Status glyph shared by tabs, sidebar rows and the palette.
struct AgentStateGlyph: View {
    let state: AgentState?
    let running: Bool
    var size: CGFloat = 7
    @State private var pulse = false

    var body: some View {
        Group {
            switch (running, state) {
            case (false, _):
                Circle().fill(Color.secondary.opacity(0.4))
            case (true, .waiting):
                ZStack {
                    Circle().fill(Color.orange)
                    Text("?").font(.system(size: size * 1.1, weight: .heavy)).foregroundStyle(.black.opacity(0.8))
                }.scaleEffect(1.5)
            case (true, .done):
                ZStack {
                    Circle().fill(Color.green)
                    Image(systemName: "checkmark").font(.system(size: size * 0.8, weight: .heavy)).foregroundStyle(.black.opacity(0.7))
                }.scaleEffect(1.5)
            case (true, .working):
                Circle().fill(AppTheme.accent).opacity(pulse ? 0.35 : 1)
                    .onAppear { withAnimation(.easeInOut(duration: 0.9).repeatForever(autoreverses: true)) { pulse = true } }
            case (true, .ended):
                Circle().fill(Color.secondary.opacity(0.4))
            default:
                Circle().fill(Color.green.opacity(0.85))
            }
        }.frame(width: size, height: size)
            .help(label)
            .accessibilityLabel(label)
    }

    var label: String {
        guard running else { return "Not running" }
        switch state {
        case .waiting: return "Waiting for you"
        case .done: return "Finished — needs review"
        case .working: return "Working"
        case .ended: return "Session ended"
        default: return "Running"
        }
    }
}
