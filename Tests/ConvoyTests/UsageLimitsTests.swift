import Foundation
import Testing
import UsageTelemetry
@testable import Convoy

@Test func parsesMultiBucketLimitsWithoutDuplicatingLegacyBucket() throws {
    let data = Data(#"{"rateLimits":{"primary":{"usedPercent":10}},"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":71,"windowDurationMins":10080,"resetsAt":1900000000},"secondary":null},"other":{"limitName":"Other model","primary":{"usedPercent":8,"windowDurationMins":300}}}}"#.utf8)
    let response = try JSONDecoder().decode(LimitsResponse.self, from: data)
    #expect(response.buckets.count == 2)
    let codex = try #require(response.buckets.first { $0.id == "codex" })
    #expect(codex.primary?.percent == 71)
    #expect(codex.primary?.label == "7d window")
    #expect(codex.primary?.resetDate == Date(timeIntervalSince1970: 1900000000))
    #expect(codex.secondary == nil)
}

@Test func unavailableUsageIsNotZeroUsage() throws {
    let response = try JSONDecoder().decode(LimitsResponse.self, from: Data(#"{"rateLimits":null,"rateLimitsByLimitId":null}"#.utf8))
    #expect(response.buckets.isEmpty)
    let legacy = try JSONDecoder().decode(LimitsResponse.self, from: Data(#"{"rateLimits":{"primary":{"usedPercent":0,"windowDurationMins":300}}}"#.utf8))
    #expect(legacy.buckets.first?.primary?.percent == 0)
    #expect(legacy.buckets.first?.primary?.label == "5h window")
    #expect(legacy.buckets.first?.primary?.resetDate == nil)
}

@Test(.enabled(if: ProcessInfo.processInfo.environment["SPECDESK_LIVE_LIMITS_TEST"] == "1"))
@MainActor func installedCodexLimitsIntegration() async throws {
    let usage = UsageLimits()
    defer { usage.finish() }
    usage.refresh()
    for _ in 0..<300 {
        if !usage.loading { break }
        try await Task.sleep(for: .milliseconds(100))
    }
    #expect(!usage.loading)
    #expect(usage.error == nil)
    #expect(usage.updatedAt != nil)
    #expect(!usage.buckets.isEmpty)
}

@Test func claudeTelemetryRetainsOnlyQuotasAndExpiresWindows() throws {
    let now = Date(timeIntervalSince1970: 1900000000)
    let raw = Data(#"{"transcript_path":"private-path","session_id":"private-id","rate_limits":{"five_hour":{"used_percentage":11,"resets_at":1900001000},"seven_day":{"used_percentage":25,"resets_at":1899999999}}}"#.utf8)
    let snapshot = try ClaudeSnapshot.capture(raw, source: "Test session", now: now)
    #expect(snapshot.windows.count == 1)
    #expect(snapshot.windows["five_hour"]?.used_percentage == 11)
    #expect(snapshot.availableWindows(at: Date(timeIntervalSince1970: 1900001001)).isEmpty)
    let saved = String(decoding: try JSONEncoder().encode(snapshot), as: UTF8.self)
    #expect(!saved.contains("private-path"))
    #expect(!saved.contains("private-id"))
    #expect(try ClaudeSnapshot.capture(Data("{}".utf8), source: "Empty", now: now).windows.isEmpty)
}

@Test func claudeStatusSettingsAreSessionScopedAndShellQuoted() throws {
    let session = LinkedSession(agent: .claude, sessionID: UUID().uuidString, title: "Example", notes: "")
    let command = "'/a path/helper' 'session name'"
    let script = SessionCommand.script(session: session, directory: "/tmp", resume: false, claudeStatusCommand: command)
    #expect(script.contains("'--settings'"))
    #expect(script.contains("statusLine"))
    #expect(!SessionCommand.script(session: session, directory: "/tmp", resume: false).contains("--settings"))
}
