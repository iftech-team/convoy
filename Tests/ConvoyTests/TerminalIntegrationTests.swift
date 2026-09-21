import AppKit
import SwiftUI
import Testing
@testable import Convoy

@Test(.enabled(if: ProcessInfo.processInfo.environment["SPECDESK_UI_TEST_DIR"] != nil))
@MainActor func terminalInputOutputAndNativeLayout() async throws {
    let root = URL(fileURLWithPath: try #require(ProcessInfo.processInfo.environment["SPECDESK_UI_TEST_DIR"]))
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    _ = NSApplication.shared
    NSApplication.shared.finishLaunching()
    NSApplication.shared.appearance = NSAppearance(named: .aqua)
    let session = LinkedSession(agent: .claude, sessionID: UUID().uuidString, title: "Cross-project delivery status")
    let group = Project(name: "olucha-cargo", path: root.path, group: true, sessions: [session])
    let api = Project(name: "olucha-cargo-api", path: root.appendingPathComponent("api").path, parentID: group.id)
    let admin = Project(name: "olucha-cargo-admin", path: root.appendingPathComponent("admin").path, parentID: group.id)
    let dataFile = WorkspaceFile(url: root.appendingPathComponent("qa-workspace.json"))
    try dataFile.save(Workspace(projects: [group, api, admin], selectedProjectID: group.id, selectedSessionID: session.id))
    let store = Store(workspaceURL: dataFile.url)
    let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1240, height: 820), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
    window.isReleasedWhenClosed = false
    defer { window.close() }
    window.title = "SpecDesk layout verification"
    let hosting = NSHostingView(rootView: WorkspaceView().environmentObject(store))
    window.contentView = hosting
    window.orderFront(nil)
    try await Task.sleep(for: .milliseconds(400))
    try capture(hosting, to: root.appendingPathComponent("workspace.png"))

    let handle = TerminalHandle(session: session, project: group, snapshotURL: root.appendingPathComponent("terminal-history.txt"))
    let terminalView = NSHostingView(rootView: TerminalPane(handle: handle))
    window.contentView = terminalView
    handle.start(script: "printf 'SPECDESK_READY\\n'; printf 'cwd=%s\\n' \"$PWD\"; read -r reply; printf 'received=%s\\n' \"$reply\"", directory: root.path)
    defer { if handle.running { handle.stop() } }
    for _ in 0..<100 {
        if handle.text.contains("SPECDESK_READY") { break }
        try await Task.sleep(for: .milliseconds(50))
    }
    #expect(handle.running)
    #expect(handle.text.contains("SPECDESK_READY"))
    // Cursor movement leaves empty cells, as Claude's TUI does between words.
    handle.view.feed(text: "\r\nHello\u{1b}[1Cworkspace\r\nWide:界 ok\r\n")
    #expect(handle.text.contains("Hello workspace"))
    #expect(handle.text.contains("Wide:界 ok"))
    handle.view.send(txt: "embedded-input-ok\r")
    for _ in 0..<100 {
        if !handle.running { break }
        try await Task.sleep(for: .milliseconds(50))
    }
    #expect(!handle.running)
    #expect(handle.text.contains("received=embedded-input-ok"))
    #expect(handle.exitCode == 0)
    #expect(try String(contentsOf: root.appendingPathComponent("terminal-history.txt"), encoding: .utf8).contains("embedded-input-ok"))
    try capture(terminalView, to: root.appendingPathComponent("terminal.png"))
}

@MainActor private func capture(_ view: NSView, to url: URL) throws {
    view.layoutSubtreeIfNeeded()
    let bitmap = try #require(view.bitmapImageRepForCachingDisplay(in: view.bounds))
    view.cacheDisplay(in: view.bounds, to: bitmap)
    // View snapshots can be transparent even inside an opaque native window.
    let opaque = NSImage(size: view.bounds.size)
    opaque.lockFocus()
    NSColor.windowBackgroundColor.setFill()
    view.bounds.fill()
    bitmap.draw(in: view.bounds)
    opaque.unlockFocus()
    let tiff = try #require(opaque.tiffRepresentation)
    let representation = try #require(NSBitmapImageRep(data: tiff))
    let png = try #require(representation.representation(using: .png, properties: [:]))
    try png.write(to: url)
}
