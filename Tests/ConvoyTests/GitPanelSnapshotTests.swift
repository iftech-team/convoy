import AppKit
import SwiftUI
import Testing
@testable import Convoy

/// Renders the Files & Changes panel against a throwaway repository and saves PNGs for visual review.
@Test(.enabled(if: ProcessInfo.processInfo.environment["SPECDESK_UI_TEST_DIR"] != nil))
@MainActor func gitPanelSnapshots() async throws {
    let root = URL(fileURLWithPath: try #require(ProcessInfo.processInfo.environment["SPECDESK_UI_TEST_DIR"])).appendingPathComponent("gitpanel")
    try? FileManager.default.removeItem(at: root)
    try FileManager.default.createDirectory(at: root.appendingPathComponent("repo/Sources/App"), withIntermediateDirectories: true)
    let repo = root.appendingPathComponent("repo").path
    func git(_ args: [String]) { let r = GitInfoService.run(args, in: repo, timeout: 30); #expect(r?.0 == 0, "git \(args) -> \(r?.1 ?? "nil")") }
    func write(_ text: String, _ path: String) throws { try text.write(toFile: repo + "/" + path, atomically: true, encoding: .utf8) }
    git(["init", "-q", "-b", "main"]); git(["config", "user.email", "t@example.com"]); git(["config", "user.name", "Ada Lovelace"])
    let body = (1...30).map { "    let value\($0) = compute(\($0))" }
    try write("# Demo\n\nA sample project for the panel.\n\n- one\n- two\n", "README.md")
    try write("import Foundation\n\nstruct App {\n" + body.joined(separator: "\n") + "\n}\n", "Sources/App/Main.swift")
    try write("old\n", "Sources/App/Legacy.swift")
    git(["add", "-A"]); git(["commit", "-q", "-m", "Initial import"])
    try write("import Foundation\n\nstruct App {\n    let name = \"Convoy\"\n" + body.joined(separator: "\n") + "\n}\n", "Sources/App/Main.swift")
    git(["add", "-A"]); git(["commit", "-q", "-m", "Add app name"])
    var edited = body; edited[1] = "    let value2 = compute(2) * 2"; edited[27] = "    let value28 = compute(28) + offset"
    try write("import Foundation\n\nstruct App {\n    let name = \"Convoy\"\n    let version = \"1.0\"\n" + edited.joined(separator: "\n") + "\n}\n", "Sources/App/Main.swift")
    try write("struct Model {}\n", "Sources/App/Model.swift"); git(["add", "Sources/App/Model.swift"])
    try FileManager.default.removeItem(atPath: repo + "/Sources/App/Legacy.swift")
    try write("notes\n", "TODO.md")

    _ = NSApplication.shared
    NSApplication.shared.finishLaunching()
    NSApplication.shared.appearance = NSAppearance(named: .darkAqua)
    let project = Project(name: "demo", path: repo)
    let dataFile = WorkspaceFile(url: root.appendingPathComponent("workspace.json"))
    try dataFile.save(Workspace(projects: [project], selectedProjectID: project.id))
    let store = Store(workspaceURL: dataFile.url)
    store.showProjectPage = true
    store.showGitPanel = true
    defer { UserDefaults.standard.removeObject(forKey: "showGitPanel"); UserDefaults.standard.removeObject(forKey: "git.diffLayout") }
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1400, height: 860), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    defer { window.close() }
    window.appearance = NSAppearance(named: .darkAqua)
    let hosting = NSHostingView(rootView: WorkspaceView().environmentObject(store).tint(AppTheme.accent))
    window.contentView = hosting
    window.orderFront(nil)
    try await Task.sleep(for: .seconds(2))
    try capture(hosting, to: root.appendingPathComponent("changes.png"))
    let model = store.gitPanel(for: repo)
    #expect(model.status?.staged.map(\.path) == ["Sources/App/Model.swift"])
    UserDefaults.standard.set("sideBySide", forKey: "git.diffLayout")
    try await Task.sleep(for: .seconds(1))
    try capture(hosting, to: root.appendingPathComponent("changes-split.png"))
    model.tab = .log
    try await Task.sleep(for: .seconds(1))
    model.selectedCommit = model.commits.first?.sha
    try await Task.sleep(for: .seconds(1))
    try capture(hosting, to: root.appendingPathComponent("log.png"))
    model.tab = .files
    try await Task.sleep(for: .seconds(1))
    model.selectedFile = "README.md"
    try await Task.sleep(for: .seconds(1))
    try capture(hosting, to: root.appendingPathComponent("files.png"))
}

@MainActor private func capture(_ view: NSView, to url: URL) throws {
    view.layoutSubtreeIfNeeded()
    let bitmap = try #require(view.bitmapImageRepForCachingDisplay(in: view.bounds))
    view.cacheDisplay(in: view.bounds, to: bitmap)
    let opaque = NSImage(size: view.bounds.size)
    opaque.lockFocus()
    NSColor.windowBackgroundColor.setFill()
    view.bounds.fill()
    bitmap.draw(in: view.bounds)
    opaque.unlockFocus()
    let tiff = try #require(opaque.tiffRepresentation)
    let representation = try #require(NSBitmapImageRep(data: tiff))
    try #require(representation.representation(using: .png, properties: [:])).write(to: url)
}
