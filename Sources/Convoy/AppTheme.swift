import AppKit
import SwiftUI

/// App palette, modelled on Linear: near-black cool-neutral surfaces with an indigo accent.
enum AppTheme {
    private static func dynamic(dark: (CGFloat, CGFloat, CGFloat), light: (CGFloat, CGFloat, CGFloat)) -> NSColor {
        NSColor(name: nil) { appearance in
            let c = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
            return NSColor(srgbRed: c.0, green: c.1, blue: c.2, alpha: 1)
        }
    }

    /// Linear-style indigo (#5E6AD2), lifted slightly on dark surfaces.
    static let accentColor = dynamic(dark: (0.55, 0.60, 0.92), light: (0.37, 0.42, 0.82))
    static let accent = Color(nsColor: accentColor)

    /// Main content background (window body, terminal chrome).
    static let windowColor = dynamic(dark: (0.055, 0.059, 0.063), light: (0.980, 0.980, 0.984))
    static let window = Color(nsColor: windowColor)
    /// Sidebar and other secondary surfaces, one step lighter than the window in dark mode.
    static let sidebarColor = dynamic(dark: (0.078, 0.082, 0.090), light: (0.953, 0.953, 0.961))
    static let sidebar = Color(nsColor: sidebarColor)
    /// Bars: tab strip, session header, status bar.
    static let raisedColor = dynamic(dark: (0.090, 0.094, 0.102), light: (0.965, 0.965, 0.972))
    static let raised = Color(nsColor: raisedColor)
    /// Cards and popover sections.
    static let cardColor = dynamic(dark: (0.118, 0.122, 0.133), light: (1, 1, 1))
    static let card = Color(nsColor: cardColor)
    /// Hairlines.
    static let stroke = Color.primary.opacity(0.08)

    /// The embedded terminal stays dark in both appearances; tuned to sit next to the dark window colour.
    /// Applies the chosen theme to every window, toolbar, popover and alert at once.
    static func applyAppearance(_ value: String) {
        NSApp.appearance = value == "dark" ? NSAppearance(named: .darkAqua) : (value == "light" ? NSAppearance(named: .aqua) : nil)
    }

    static let terminalBackground = NSColor(srgbRed: 0.031, green: 0.035, blue: 0.039, alpha: 1)
    static let terminalForeground = NSColor(srgbRed: 0.85, green: 0.86, blue: 0.88, alpha: 1)
}

/// Grabs the hosting NSWindow once so the title bar matches the app surfaces.
/// Applied a single time: re-applying window properties during a full-screen
/// transition cancels the transition.
struct WindowStyler: NSViewRepresentable {
    final class Coordinator { var styled = false }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async { style(view.window, context.coordinator) }
        return view
    }
    func updateNSView(_ view: NSView, context: Context) {
        if !context.coordinator.styled { DispatchQueue.main.async { style(view.window, context.coordinator) } }
    }
    private func style(_ window: NSWindow?, _ coordinator: Coordinator) {
        guard let window, !coordinator.styled else { return }
        coordinator.styled = true
        window.backgroundColor = AppTheme.windowColor
        window.titlebarAppearsTransparent = true
        window.titleVisibility = .hidden
    }
}
