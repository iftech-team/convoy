import Foundation
import Testing
@testable import Convoy
import SwiftTerm

@Test @MainActor func droppedFilePathsAreShellQuoted() {
    #expect(CapturingTerminal.shellQuote("/Users/me/shot.png") == "/Users/me/shot.png")
    #expect(CapturingTerminal.shellQuote("/Users/me/Screen Shot 1.png") == "'/Users/me/Screen Shot 1.png'")
    #expect(CapturingTerminal.shellQuote("/tmp/it's.png") == "'/tmp/it'\\''s.png'")
    let text = CapturingTerminal.droppedText(for: [URL(fileURLWithPath: "/a/b.png"), URL(fileURLWithPath: "/a/c d.png")])
    #expect(text == "/a/b.png '/a/c d.png' ")
}

@Test @MainActor func terminalAcceptsFileDrops() {
    let view = CapturingTerminal(frame: .init(x: 0, y: 0, width: 200, height: 100), options: TerminalOptions(scrollback: 100))
    #expect(view.registeredDraggedTypes.contains(.fileURL))
}
