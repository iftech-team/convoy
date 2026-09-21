import Foundation
import UsageTelemetry

// Claude invokes this helper locally in two roles:
//  1. `ConvoyStatus <title>`            — status line: persist only quota fields (never the full payload).
//  2. `ConvoyStatus hook <EventName>`   — Claude Code hook: record agent state for the session so the
//                                            app can show working / waiting / done and notify. Prints "{}"
//                                            so permission-style hooks never fail closed on empty stdout.
let arguments = Array(CommandLine.arguments.dropFirst())
if arguments.first == "hook" {
    let event = arguments.dropFirst().first ?? ""
    let input = FileHandle.standardInput.readDataToEndOfFile()
    print("{}")
    guard input.count <= 2_000_000,
          let json = try? JSONSerialization.jsonObject(with: input) as? [String: Any],
          let sessionID = (json["session_id"] as? String)?.lowercased(), !sessionID.isEmpty,
          sessionID.range(of: "^[0-9a-f-]{8,64}$", options: .regularExpression) != nil else { exit(0) }
    let tool = json["tool_name"] as? String ?? ""
    let notification = json["notification_type"] as? String ?? ""
    let state: String
    switch event {
    case "SessionStart": state = "idle"
    case "UserPromptSubmit", "PostToolUse": state = "working"
    case "PreToolUse": state = tool == "AskUserQuestion" ? "waiting" : "working"
    case "PermissionRequest": state = "waiting"
    case "Notification": state = notification == "idle_prompt" ? "done" : "waiting"
    case "Stop": state = "done"
    case "SessionEnd": state = "ended"
    default: exit(0)
    }
    let record: [String: Any] = ["state": state, "event": event, "tool": tool, "at": Date().timeIntervalSince1970]
    let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("Convoy/AgentStatus")
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    if let data = try? JSONSerialization.data(withJSONObject: record) {
        try? data.write(to: dir.appendingPathComponent("\(sessionID).json"), options: [.atomic])
    }
    exit(0)
}

do {
    let input = FileHandle.standardInput.readDataToEndOfFile()
    guard input.count <= 2_000_000 else { exit(0) }
    let snapshot = try ClaudeSnapshot.capture(input, source: arguments.first ?? "Claude session")
    let target = ClaudeSnapshot.cacheURL
    try FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
    try JSONEncoder().encode(snapshot).write(to: target, options: [.atomic])
    try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: target.path)
    let text = [("five_hour", "5h"), ("seven_day", "7d")].compactMap { key, label in
        snapshot.windows[key].map { "\(label): \(Int($0.used_percentage))% used" }
    }.joined(separator: " · ")
    print(text.isEmpty ? "Convoy · limits available after a response" : text)
} catch {
    print("Convoy · limits unavailable")
}
