import Foundation
import Testing
@testable import Convoy

@Test func claudeTrustIsSeededWithoutTouchingOtherProjects() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-trust-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: dir) }
    let file = dir.appendingPathComponent(".claude.json")
    try #"{"projects":{"/other":{"hasTrustDialogAccepted":false,"allowedTools":["Bash"]}},"theme":"dark"}"#.write(to: file, atomically: true, encoding: .utf8)
    FolderTrust.approveClaude(directory: "/work/tree one", configDir: dir.path)
    let root = try #require(try JSONSerialization.jsonObject(with: Data(contentsOf: file)) as? [String: Any])
    let projects = try #require(root["projects"] as? [String: Any])
    #expect((projects["/work/tree one"] as? [String: Any])?["hasTrustDialogAccepted"] as? Bool == true)
    #expect((projects["/other"] as? [String: Any])?["hasTrustDialogAccepted"] as? Bool == false)
    #expect(((projects["/other"] as? [String: Any])?["allowedTools"] as? [String]) == ["Bash"])
    #expect(root["theme"] as? String == "dark")
}

@Test func codexTrustIsAppendedOnce() throws {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("convoy-codex-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: dir) }
    FolderTrust.approveCodex(directory: "/work/a", home: dir.path)
    FolderTrust.approveCodex(directory: "/work/a", home: dir.path)
    let text = try String(contentsOf: dir.appendingPathComponent("config.toml"), encoding: .utf8)
    #expect(text.components(separatedBy: "[projects.\"/work/a\"]").count == 2)
    #expect(text.contains("trust_level = \"trusted\""))
}
