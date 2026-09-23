import SwiftUI

/// 1 / 2 / 4 terminals on one screen. Each pane carries its project's colour so four agents stay distinguishable.
struct PaneGrid: View {
    @EnvironmentObject var store: Store
    @ObservedObject var manager: TerminalManager

    var body: some View {
        Group {
            switch store.paneCount {
            case 2:
                EvenSplit(axis: .horizontal, key: "panes.h") { pane(0) } second: { pane(1) }
            case 4:
                EvenSplit(axis: .vertical, key: "panes.v") {
                    EvenSplit(axis: .horizontal, key: "panes.h1") { pane(0) } second: { pane(1) }
                } second: {
                    EvenSplit(axis: .horizontal, key: "panes.h2") { pane(2) } second: { pane(3) }
                }
            default:
                pane(0)
            }
        }
    }

    @ViewBuilder private func pane(_ index: Int) -> some View {
        let id = store.panes.indices.contains(index) ? store.panes[index] : nil
        Group {
            if let id, store.tabOrder.contains(id), let project = store.project(ofSession: id), let session = store.session(id) {
                if store.paneCount == 1 {
                    ProjectWorkbench(project: project).id(project.id)
                } else {
                    SessionPaneView(index: index, project: project, session: session, manager: manager)
                }
            } else {
                EmptyPane(index: index)
            }
        }
        .frame(minWidth: 320, maxWidth: .infinity, minHeight: 200, maxHeight: .infinity)
        .overlay {
            if store.paneCount > 1 {
                Rectangle().strokeBorder(store.focusedPane == index ? AppTheme.accent.opacity(0.8) : Color.clear, lineWidth: 2).allowsHitTesting(false)
            }
        }
        .dropDestination(for: String.self) { items, _ in
            guard let raw = items.first, let dropped = UUID(uuidString: raw), let project = store.project(ofSession: dropped) else { return false }
            store.terminals.openTab(dropped)
            if let other = store.panes.firstIndex(of: dropped), other != index { store.panes[other] = nil }
            store.panes[index] = dropped
            store.focusPane(index)
            _ = project
            return true
        }
    }
}

/// Two children split down the middle, with a draggable divider. Unlike HSplitView the first
/// child never swallows the whole width; the ratio is remembered per key.
struct EvenSplit<First: View, Second: View>: View {
    let axis: Axis
    let key: String
    let minimumFirst: CGFloat
    let minimumSecond: CGFloat
    let defaultFraction: CGFloat
    @ViewBuilder let first: () -> First
    @ViewBuilder let second: () -> Second
    @State private var fraction: CGFloat
    @State private var dragStart: CGFloat?
    @State private var hovering = false

    init(axis: Axis, key: String, minimumFirst: CGFloat? = nil, minimumSecond: CGFloat? = nil, defaultFraction: CGFloat = 0.5,
         @ViewBuilder first: @escaping () -> First, @ViewBuilder second: @escaping () -> Second) {
        self.axis = axis; self.key = key; self.first = first; self.second = second
        let fallback: CGFloat = axis == .horizontal ? 320 : 200
        self.minimumFirst = minimumFirst ?? fallback; self.minimumSecond = minimumSecond ?? fallback; self.defaultFraction = defaultFraction
        let saved = UserDefaults.standard.double(forKey: "split." + key)
        _fraction = State(initialValue: saved > 0.1 && saved < 0.9 ? saved : defaultFraction)
    }

    var body: some View {
        GeometryReader { geo in
            let total = axis == .horizontal ? geo.size.width : geo.size.height
            let minimum = minimumFirst
            let firstSize = max(minimum, min(max(minimum, total - minimumSecond), (total * fraction).rounded()))
            let divider = Rectangle().fill(Color.primary.opacity(hovering ? 0.25 : 0.1))
                .frame(width: axis == .horizontal ? 1 : nil, height: axis == .vertical ? 1 : nil)
                .padding(axis == .horizontal ? .horizontal : .vertical, 3)
                .contentShape(Rectangle())
                .onHover { inside in hovering = inside; if inside { (axis == .horizontal ? NSCursor.resizeLeftRight : NSCursor.resizeUpDown).push() } else { NSCursor.pop() } }
                .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .named("split." + key)).onChanged { value in
                    if dragStart == nil { dragStart = firstSize }
                    let delta = axis == .horizontal ? value.translation.width : value.translation.height
                    let next = max(minimum, min(max(minimum, total - minimumSecond), (dragStart ?? firstSize) + delta))
                    fraction = next / max(total, 1)
                }.onEnded { _ in dragStart = nil; UserDefaults.standard.set(Double(fraction), forKey: "split." + key) })
                .onTapGesture(count: 2) { fraction = defaultFraction; UserDefaults.standard.set(Double(defaultFraction), forKey: "split." + key) }
            if axis == .horizontal {
                HStack(spacing: 0) {
                    first().frame(width: firstSize)
                    divider
                    second().frame(maxWidth: .infinity)
                }
            } else {
                VStack(spacing: 0) {
                    first().frame(height: firstSize)
                    divider
                    second().frame(maxHeight: .infinity)
                }
            }
        }.coordinateSpace(name: "split." + key)
    }
}

struct SessionPaneView: View {
    @EnvironmentObject var store: Store
    let index: Int
    let project: Project
    let session: LinkedSession
    @ObservedObject var manager: TerminalManager
    @State private var stopConfirm = false

    private var running: Bool { manager.handles[session.id]?.running == true }
    private var focused: Bool { store.focusedPane == index }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                RoundedRectangle(cornerRadius: 2).fill(project.tint).frame(width: 4, height: 18)
                ProjectIconView(project: project, size: 13)
                Text(project.name).font(.system(size: 11.5, weight: .semibold)).foregroundStyle(project.tint).lineLimit(1)
                Text("/").foregroundStyle(.tertiary)
                AgentIcon(agent: session.agent, size: 12)
                Text(session.title).font(.system(size: 12, weight: .medium)).lineLimit(1)
                if let branch = session.branch ?? store.git.info[store.directory(for: session, in: project)]?.branch {
                    Text("⎇ " + branch).font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                HStack(spacing: 5) {
                    AgentStateGlyph(state: store.agentState(session), running: running, size: 6).frame(width: 10)
                    Text(AgentStateGlyph(state: store.agentState(session), running: running).label).font(.system(size: 10.5)).foregroundStyle(.secondary).lineLimit(1)
                }
                if !running {
                    Button("Resume") { store.startSession(session, in: project, resume: true) }.controlSize(.small)
                }
                Menu {
                    ForEach(store.quickCommands(for: project)) { c in Button(c.title) { store.send(c, to: session.id) } }
                    if store.quickCommands(for: project).isEmpty { Text("No quick commands") }
                } label: { Image(systemName: "bolt").frame(width: 20, height: 20) }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize().help("Quick commands")
                Menu {
                    Button("Maximize (one pane)") { store.setLayout(1); store.place(session.id, in: 0); store.focusPane(0) }
                    Divider()
                    if running { Button("Stop session…", role: .destructive) { stopConfirm = true } }
                    Button("Close pane") { store.closePane(index) }
                } label: { Image(systemName: "ellipsis").frame(width: 20, height: 20) }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                Button { store.closePane(index) } label: { Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).frame(width: 18, height: 18) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).help("Close pane")
            }
            .font(.system(size: 11)).padding(.horizontal, 10).frame(height: 34)
            .background(project.tint.opacity(focused ? 0.18 : 0.09))
            .overlay(alignment: .bottom) { Rectangle().fill(project.tint.opacity(0.5)).frame(height: 1) }
            .contentShape(Rectangle())
            .onTapGesture { store.focusPane(index) }
            if let handle = manager.handles[session.id] {
                TerminalPane(handle: handle, primary: focused).id(ObjectIdentifier(handle))
                    .onTapGesture { if !focused { store.focusPane(index) } }
                    .overlay {
                        if !focused {
                            Color.clear.contentShape(Rectangle()).onTapGesture { store.focusPane(index) }
                        }
                    }
            } else {
                ScrollView {
                    Text(manager.snapshot(for: session)).font(.system(size: 12, design: .monospaced)).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(16)
                }.background(Color(nsColor: AppTheme.terminalBackground))
                    .onTapGesture { store.focusPane(index) }
            }
        }
        .alert("Stop this agent?", isPresented: $stopConfirm) {
            Button("Cancel", role: .cancel) {}
            Button("Stop", role: .destructive) { manager.handles[session.id]?.stop() }
        }
    }
}

struct EmptyPane: View {
    @EnvironmentObject var store: Store
    let index: Int
    @State private var hovered = false

    private var suggestions: [(Project, LinkedSession)] {
        store.workspace.projects.flatMap { p in p.linkedSessions.filter { $0.archived != true && !$0.title.hasPrefix("Login:") && !store.panes.contains($0.id) }.map { (p, $0) } }
            .sorted { a, b in
                let ra = store.terminals.handles[a.1.id]?.running == true, rb = store.terminals.handles[b.1.id]?.running == true
                return ra != rb ? ra : a.1.createdAt > b.1.createdAt
            }.prefix(6).map { $0 }
    }

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: "rectangle.dashed").font(.system(size: 26)).foregroundStyle(.tertiary)
            Text("Pane \(index + 1)").font(.system(size: 13, weight: .semibold)).foregroundStyle(.secondary)
            Text("Drop a session here, or pick one").font(.system(size: 11)).foregroundStyle(.tertiary)
            HStack(spacing: 8) {
                Button { store.focusedPane = index; store.openPalette(.terminals) } label: { Label("Choose…", systemImage: "rectangle.on.rectangle") }
                Button { store.focusedPane = index; store.newSessionInCurrentProject() } label: { Label("New session", systemImage: "plus") }.disabled(store.workspace.projects.isEmpty)
            }.controlSize(.small)
            if !suggestions.isEmpty {
                VStack(spacing: 2) {
                    ForEach(suggestions, id: \.1.id) { project, session in
                        Button {
                            store.openSession(session.id, in: project.id); store.place(session.id, in: index); store.focusPane(index)
                        } label: {
                            HStack(spacing: 8) {
                                RoundedRectangle(cornerRadius: 1.5).fill(project.tint).frame(width: 3, height: 14)
                                AgentStateGlyph(state: store.agentState(session), running: store.terminals.handles[session.id]?.running == true, size: 6).frame(width: 10)
                                Text(project.name).font(.system(size: 11)).foregroundStyle(project.tint)
                                Text(session.title).font(.system(size: 11.5, weight: .medium)).lineLimit(1)
                                Spacer()
                            }.padding(.horizontal, 10).padding(.vertical, 5).frame(width: 320).contentShape(Rectangle())
                                .background(Color.primary.opacity(0.04), in: RoundedRectangle(cornerRadius: 6))
                        }.buttonStyle(.plain)
                    }
                }.padding(.top, 6)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(hovered ? AppTheme.accent.opacity(0.06) : AppTheme.window)
        .onHover { hovered = $0 }
        .contentShape(Rectangle())
        .onTapGesture { store.focusedPane = index }
    }
}
