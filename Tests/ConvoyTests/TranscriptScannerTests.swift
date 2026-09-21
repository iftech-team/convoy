import Foundation
import Testing
@testable import Convoy

@Test func scannerFindsClaudeAndCodexTranscriptsForDirectory() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    let project = root.appendingPathComponent("repo").path
    try FileManager.default.createDirectory(atPath: project, withIntermediateDirectories: true)
    // Claude: <encoded cwd>/<id>.jsonl under a fake config dir.
    let config = root.appendingPathComponent("claude")
    let encoded = project.replacingOccurrences(of: "[^A-Za-z0-9]", with: "-", options: .regularExpression)
    let claudeDir = config.appendingPathComponent("projects").appendingPathComponent(encoded)
    try FileManager.default.createDirectory(at: claudeDir, withIntermediateDirectories: true)
    let claudeLines = [
        #"{"type":"summary","summary":"x"}"#,
        #"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
        #"{"type":"user","message":{"role":"user","content":"Fix the delivery status bug"}}"#,
        #"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
    ].joined(separator: "\n")
    try claudeLines.write(to: claudeDir.appendingPathComponent("aaaaaaaa-0000-0000-0000-000000000001.jsonl"), atomically: true, encoding: .utf8)
    // Codex: dated rollout with session_meta.cwd.
    let codexDir = root.appendingPathComponent("codex/sessions/2026/09/21")
    try FileManager.default.createDirectory(at: codexDir, withIntermediateDirectories: true)
    let codexLines = [
        #"{"timestamp":"t","type":"session_meta","payload":{"id":"01aa-codex","cwd":"\#(project)","originator":"codex-tui"}}"#,
        #"{"timestamp":"t","type":"event_msg","payload":{"type":"user_message","message":"Add pagination to orders"}}"#,
    ].joined(separator: "\n")
    try codexLines.write(to: codexDir.appendingPathComponent("rollout-2026-09-21T10-00-00-01aa-codex.jsonl"), atomically: true, encoding: .utf8)
    // Subagent rollout for the same cwd must be skipped.
    let sub = #"{"timestamp":"t","type":"session_meta","payload":{"id":"01bb-sub","cwd":"\#(project)","parent_thread_id":"01aa-codex"}}"#
    try sub.write(to: codexDir.appendingPathComponent("rollout-2026-09-21T10-01-00-01bb-sub.jsonl"), atomically: true, encoding: .utf8)

    let entries = TranscriptScanner.scan(directories: [project], claudeRoot: config.appendingPathComponent("projects"), codexRoot: root.appendingPathComponent("codex/sessions"))
    #expect(entries.count == 2)
    let claude = try #require(entries.first { $0.agent == .claude })
    #expect(claude.sessionID == "aaaaaaaa-0000-0000-0000-000000000001")
    #expect(claude.title == "Fix the delivery status bug")
    let codex = try #require(entries.first { $0.agent == .codex })
    #expect(codex.sessionID == "01aa-codex")
    #expect(codex.title == "Add pagination to orders")
    #expect(!entries.contains { $0.sessionID == "01bb-sub" })
}
