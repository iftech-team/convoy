import Foundation

public struct ClaudeWindow: Codable, Sendable {
    public let used_percentage: Double
    public let resets_at: Int64?
}

public struct ClaudeSnapshot: Codable, Sendable {
    public let windows: [String: ClaudeWindow]
    public let updatedAt: Date
    public let source: String

    public static var cacheURL: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Convoy/claude-limits.json")
    }

    public static func capture(_ input: Data, source: String, now: Date = Date()) throws -> ClaudeSnapshot {
        struct Payload: Decodable { let rate_limits: [String: ClaudeWindow]? }
        let payload = try JSONDecoder().decode(Payload.self, from: input)
        let windows = (payload.rate_limits ?? [:]).filter { key, value in
            ["five_hour", "seven_day"].contains(key) && value.used_percentage.isFinite
                && (0...100).contains(value.used_percentage)
                && (value.resets_at.map { Double($0) > now.timeIntervalSince1970 } ?? true)
        }
        return ClaudeSnapshot(windows: windows, updatedAt: now, source: source)
    }

    public func availableWindows(at date: Date = Date()) -> [String: ClaudeWindow] {
        windows.filter { $0.value.resets_at.map { Double($0) > date.timeIntervalSince1970 } ?? true }
    }
}
