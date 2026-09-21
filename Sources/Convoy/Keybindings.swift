import AppKit
import SwiftUI

/// One remappable action. Bindings are strings like "cmd+shift+r"; `key` uses SwiftUI KeyEquivalent names.
struct KeyAction: Identifiable {
    let id: String
    let title: String
    let group: String
    let defaultBinding: String?
}

struct KeyBinding: Equatable {
    var key: String
    var modifiers: Set<String>   // "cmd" "shift" "opt" "ctrl"

    static func parse(_ text: String) -> KeyBinding? {
        let parts = text.lowercased().split(separator: "+").map(String.init)
        guard let key = parts.last, !key.isEmpty else { return nil }
        return KeyBinding(key: key, modifiers: Set(parts.dropLast()))
    }
    var string: String { (["cmd", "shift", "opt", "ctrl"].filter { modifiers.contains($0) } + [key]).joined(separator: "+") }
    var display: String {
        let mods = (modifiers.contains("ctrl") ? "⌃" : "") + (modifiers.contains("opt") ? "⌥" : "") + (modifiers.contains("shift") ? "⇧" : "") + (modifiers.contains("cmd") ? "⌘" : "")
        let names = ["tab": "⇥", "return": "↩", "escape": "⎋", "delete": "⌫", "space": "␣", "up": "↑", "down": "↓", "left": "←", "right": "→"]
        return mods + (names[key] ?? key.uppercased())
    }
    var keyEquivalent: KeyEquivalent {
        switch key {
        case "tab": return .tab
        case "return": return .return
        case "escape": return .escape
        case "delete": return .delete
        case "space": return .space
        case "up": return .upArrow
        case "down": return .downArrow
        case "left": return .leftArrow
        case "right": return .rightArrow
        default: return KeyEquivalent(Character(key))
        }
    }
    var eventModifiers: EventModifiers {
        var m: EventModifiers = []
        if modifiers.contains("cmd") { m.insert(.command) }
        if modifiers.contains("shift") { m.insert(.shift) }
        if modifiers.contains("opt") { m.insert(.option) }
        if modifiers.contains("ctrl") { m.insert(.control) }
        return m
    }

    /// Builds a binding from an AppKit key event (used by the recorder).
    static func from(_ event: NSEvent) -> KeyBinding? {
        var mods: Set<String> = []
        if event.modifierFlags.contains(.command) { mods.insert("cmd") }
        if event.modifierFlags.contains(.shift) { mods.insert("shift") }
        if event.modifierFlags.contains(.option) { mods.insert("opt") }
        if event.modifierFlags.contains(.control) { mods.insert("ctrl") }
        let special: [UInt16: String] = [48: "tab", 36: "return", 53: "escape", 51: "delete", 49: "space", 126: "up", 125: "down", 123: "left", 124: "right"]
        if let name = special[event.keyCode] { return KeyBinding(key: name, modifiers: mods) }
        guard let chars = event.charactersIgnoringModifiers?.lowercased(), chars.count == 1, let c = chars.first, c.isLetter || c.isNumber || c.isPunctuation || c.isSymbol else { return nil }
        guard !mods.isEmpty else { return nil }
        return KeyBinding(key: String(c), modifiers: mods)
    }
}

/// Registry of actions plus user overrides saved in UserDefaults ("keybindings", [id: binding]).
@MainActor
final class Keybindings: ObservableObject {
    static let shared = Keybindings()
    @Published private(set) var overrides: [String: String] = UserDefaults.standard.dictionary(forKey: "keybindings") as? [String: String] ?? [:]

    static let actions: [KeyAction] = [
        KeyAction(id: "session.new", title: "New session", group: "Sessions & tabs", defaultBinding: "cmd+n"),
        KeyAction(id: "tab.switch", title: "Switch terminal (recent first)", group: "Sessions & tabs", defaultBinding: "cmd+e"),
        KeyAction(id: "tab.next", title: "Next tab", group: "Sessions & tabs", defaultBinding: "ctrl+tab"),
        KeyAction(id: "tab.previous", title: "Previous tab", group: "Sessions & tabs", defaultBinding: "ctrl+shift+tab"),
        KeyAction(id: "tab.close", title: "Close tab", group: "Sessions & tabs", defaultBinding: "cmd+w"),
        KeyAction(id: "tab.reopen", title: "Reopen closed tab", group: "Sessions & tabs", defaultBinding: "cmd+shift+t"),
        KeyAction(id: "tab.split", title: "Split with recent terminal", group: "Sessions & tabs", defaultBinding: "cmd+d"),
        KeyAction(id: "view.layout1", title: "One pane", group: "Panes", defaultBinding: "ctrl+1"),
        KeyAction(id: "view.layout2", title: "Two panes", group: "Panes", defaultBinding: "ctrl+2"),
        KeyAction(id: "view.layout4", title: "Four panes", group: "Panes", defaultBinding: "ctrl+4"),
        KeyAction(id: "pane.next", title: "Focus next pane", group: "Panes", defaultBinding: "cmd+opt+right"),
        KeyAction(id: "pane.previous", title: "Focus previous pane", group: "Panes", defaultBinding: "cmd+opt+left"),
        KeyAction(id: "pane.close", title: "Close focused pane", group: "Panes", defaultBinding: "cmd+shift+w"),
        KeyAction(id: "session.resume", title: "Resume session", group: "Sessions & tabs", defaultBinding: "cmd+shift+r"),
        KeyAction(id: "session.stop", title: "Stop session", group: "Sessions & tabs", defaultBinding: "cmd+."),
        KeyAction(id: "session.sleep", title: "Sleep session (hibernate now)", group: "Sessions & tabs", defaultBinding: "cmd+opt+z"),
        KeyAction(id: "session.pin", title: "Pin / unpin session", group: "Sessions & tabs", defaultBinding: "cmd+opt+p"),
        KeyAction(id: "session.edit", title: "Edit name & notes", group: "Sessions & tabs", defaultBinding: "cmd+i"),
        KeyAction(id: "session.review", title: "Start review", group: "Sessions & tabs", defaultBinding: "cmd+opt+r"),
        KeyAction(id: "session.feedback", title: "Send feedback to builder", group: "Sessions & tabs", defaultBinding: "cmd+shift+b"),
        KeyAction(id: "session.diff", title: "Toggle changes (diff) panel", group: "Sessions & tabs", defaultBinding: "cmd+shift+g"),
        KeyAction(id: "session.quick", title: "Quick commands", group: "Sessions & tabs", defaultBinding: "cmd+/"),
        KeyAction(id: "project.open", title: "Open folder", group: "Project", defaultBinding: "cmd+o"),
        KeyAction(id: "project.sessions", title: "Sessions area", group: "Project", defaultBinding: "cmd+opt+1"),
        KeyAction(id: "project.reviews", title: "Reviews area", group: "Project", defaultBinding: "cmd+opt+2"),
        KeyAction(id: "project.specs", title: "Specs area", group: "Project", defaultBinding: "cmd+opt+3"),
        KeyAction(id: "project.newSpec", title: "New specification", group: "Project", defaultBinding: "cmd+opt+n"),
        KeyAction(id: "project.finder", title: "Show in Finder", group: "Project", defaultBinding: "cmd+opt+f"),
        KeyAction(id: "project.copyPath", title: "Copy folder path", group: "Project", defaultBinding: "cmd+opt+c"),
        KeyAction(id: "project.refresh", title: "Refresh projects", group: "Project", defaultBinding: "cmd+ctrl+r"),
        KeyAction(id: "go.palette", title: "Command palette", group: "General", defaultBinding: "cmd+k"),
        KeyAction(id: "go.search", title: "Search sessions & projects", group: "General", defaultBinding: "cmd+shift+p"),
        KeyAction(id: "go.find", title: "Find (terminal when one is open, else sidebar)", group: "General", defaultBinding: "cmd+f"),
        KeyAction(id: "go.sidebar", title: "Toggle sidebar", group: "General", defaultBinding: "cmd+b"),
        KeyAction(id: "go.activity", title: "Activity feed", group: "General", defaultBinding: "cmd+shift+a"),
        KeyAction(id: "go.home", title: "Home", group: "General", defaultBinding: "cmd+shift+h"),
        KeyAction(id: "go.dashboard", title: "Agent dashboard window", group: "General", defaultBinding: "cmd+opt+d"),
        KeyAction(id: "project.history", title: "History area (saved conversations)", group: "Project", defaultBinding: "cmd+opt+4"),
        KeyAction(id: "project.tasks", title: "Tasks area", group: "Project", defaultBinding: "cmd+opt+5"),
        KeyAction(id: "project.docs", title: "Docs & specs area", group: "Project", defaultBinding: "cmd+opt+6"),
        KeyAction(id: "go.settings", title: "Settings", group: "General", defaultBinding: "cmd+,"),
        KeyAction(id: "go.theme", title: "Cycle theme", group: "General", defaultBinding: "cmd+opt+t"),
        KeyAction(id: "go.wake", title: "Toggle keep awake", group: "General", defaultBinding: "cmd+opt+k"),
        KeyAction(id: "limits.show", title: "AI Limits panel", group: "Limits", defaultBinding: "cmd+shift+l"),
        KeyAction(id: "limits.refresh", title: "Refresh Codex limits", group: "Limits", defaultBinding: "cmd+opt+l"),
        KeyAction(id: "limits.claudeUsage", title: "Open Claude /usage", group: "Limits", defaultBinding: "cmd+opt+u"),
        KeyAction(id: "git.refresh", title: "Refresh git status", group: "Project", defaultBinding: "cmd+opt+g"),
    ]

    func binding(_ id: String) -> KeyBinding? {
        if let raw = overrides[id] { return raw.isEmpty ? nil : KeyBinding.parse(raw) }
        return Self.actions.first { $0.id == id }?.defaultBinding.flatMap(KeyBinding.parse)
    }
    func set(_ id: String, to binding: KeyBinding?) {
        overrides[id] = binding?.string ?? ""
        UserDefaults.standard.set(overrides, forKey: "keybindings")
    }
    func reset(_ id: String) { overrides[id] = nil; UserDefaults.standard.set(overrides, forKey: "keybindings") }
    func resetAll() { overrides = [:]; UserDefaults.standard.removeObject(forKey: "keybindings") }
    func conflict(for binding: KeyBinding, excluding id: String) -> KeyAction? {
        Self.actions.first { $0.id != id && self.binding($0.id) == binding }
    }
    func display(_ id: String) -> String { binding(id)?.display ?? "" }
}

extension View {
    /// Applies a remappable shortcut; unbound actions stay in the menu without a key.
    @ViewBuilder func bound(_ id: String, _ keys: Keybindings) -> some View {
        if let b = keys.binding(id) { self.keyboardShortcut(b.keyEquivalent, modifiers: b.eventModifiers) } else { self }
    }
}

/// Click, press a combination, done. Backspace clears; Escape cancels.
struct ShortcutRecorder: View {
    let actionID: String
    @ObservedObject var keys: Keybindings
    @State private var recording = false
    @State private var monitor: Any?
    @State private var conflict: String?

    var body: some View {
        HStack(spacing: 6) {
            if let conflict { Text("Also \(conflict)").font(.system(size: 10)).foregroundStyle(.orange) }
            Button { recording ? stop() : start() } label: {
                Text(recording ? "Press keys…" : (keys.binding(actionID)?.display ?? "Unbound"))
                    .font(.system(size: 12, weight: .medium, design: .rounded))
                    .foregroundStyle(recording ? AppTheme.accent : (keys.binding(actionID) == nil ? Color.secondary.opacity(0.6) : Color.primary))
                    .frame(minWidth: 90).padding(.horizontal, 8).padding(.vertical, 4)
                    .background(Color.primary.opacity(recording ? 0.1 : 0.05), in: RoundedRectangle(cornerRadius: 6))
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(recording ? AppTheme.accent : AppTheme.stroke))
            }.buttonStyle(.plain)
            if keys.overrides[actionID] != nil {
                Button { keys.reset(actionID); conflict = nil } label: { Image(systemName: "arrow.uturn.backward").font(.system(size: 10)) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).help("Reset to default")
            }
        }
    }

    private func start() {
        recording = true
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.keyCode == 53 { stop(); return nil }
            if event.keyCode == 51 && event.modifierFlags.intersection([.command, .shift, .option, .control]).isEmpty { keys.set(actionID, to: nil); stop(); return nil }
            guard let binding = KeyBinding.from(event) else { return nil }
            conflict = keys.conflict(for: binding, excluding: actionID)?.title
            keys.set(actionID, to: binding); stop(); return nil
        }
    }
    private func stop() {
        recording = false
        if let monitor { NSEvent.removeMonitor(monitor) }; monitor = nil
    }
}
