import SwiftUI
import AppKit

@main
struct SpecDeskApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var store = Store()
    @AppStorage("appearance") private var appearance = "system"

    var body: some Scene {
        Window("Convoy", id: "workspace") {
            WorkspaceView().environmentObject(store)
                .onAppear {
                    appDelegate.terminals = store.terminals
                    appDelegate.usage = store.usage
                    appDelegate.wake = store.wake
                    AppTheme.applyAppearance(appearance)
                }
                .onChange(of: appearance) { _, value in AppTheme.applyAppearance(value) }
                .frame(minWidth: 960, maxWidth: .infinity, minHeight: 640, maxHeight: .infinity)
                .tint(AppTheme.accent)
                .alert("Couldn’t complete that action", isPresented: Binding(get: { store.error != nil }, set: { if !$0 { store.error = nil } })) {
                    Button("OK") { store.error = nil }
                } message: { Text(store.error ?? "") }
        }
        .defaultSize(width: 1240, height: 820)
        .windowResizability(.contentMinSize)
        .commands { AppCommands(store: store, keys: store.keys) }
        Window("Agent Dashboard", id: "dashboard") {
            DashboardView(manager: store.terminals, status: store.agentStatus).environmentObject(store).tint(AppTheme.accent)
        }.defaultSize(width: 1000, height: 560)
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    weak var terminals: TerminalManager?
    weak var usage: UsageLimits?
    weak var wake: WakeControl?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if let terminals, terminals.runningCount > 0 {
            let alert = NSAlert()
            alert.messageText = "Quit and stop \(terminals.runningCount) running sessions?"
            alert.informativeText = "Closing the window keeps agents running. Quitting stops them; saved sessions and terminal snapshots remain available to reopen."
            alert.addButton(withTitle: "Keep running")
            alert.addButton(withTitle: "Stop sessions and quit")
            guard alert.runModal() == .alertSecondButtonReturn else { return .terminateCancel }
            terminals.stopAll()
        }
        usage?.finish(); wake?.stop()
        return .terminateNow
    }
}

struct WorkspaceView: View {
    @EnvironmentObject var store: Store
    @Environment(\.openWindow) private var openWindow
    @State private var projectToRemove: Project?
    @AppStorage("showSidebar") private var showSidebar = true

    @ViewBuilder private var mainContent: some View {
        if store.showSettings {
            SettingsPage()
        } else if let pid = store.projectSettingsID, let project = store.workspace.projects.first(where: { $0.id == pid }) {
            ProjectSettingsPage(project: project).id(pid)
        } else if !store.visiblePaneSessions.isEmpty && !store.showProjectPage {
            PaneGrid(manager: store.terminals)
        } else if store.showProjectPage, let project = store.project {
            ProjectWorkbench(project: project).id(project.id)
        } else {
            HomePage(manager: store.terminals, usage: store.usage)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            HSplitView {
                if showSidebar {
                    SidebarView(projectToRemove: $projectToRemove)
                        .frame(minWidth: 220, idealWidth: 260, maxWidth: 380)
                }
                VStack(spacing: 0) {
                    SessionTabBar(manager: store.terminals, showSidebar: $showSidebar)
                    Divider()
                    if store.showGitPanel && !store.showSettings && store.projectSettingsID == nil {
                        EvenSplit(axis: .horizontal, key: "git.panel", minimumFirst: 520, minimumSecond: 340, defaultFraction: 0.6) {
                            mainContent
                        } second: {
                            if let dir = store.gitPanelDirectory { GitPanelView(model: store.gitPanel(for: dir)).id(dir) } else { GitPanelEmpty() }
                        }
                    } else {
                        mainContent
                    }
                }.frame(minWidth: 620, maxWidth: .infinity, maxHeight: .infinity)
            }
            if let notice = store.notice {
                HStack {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(AppTheme.accent)
                    Text(notice).font(.callout)
                    Spacer()
                    Button { store.notice = nil } label: { Image(systemName: "xmark") }.buttonStyle(.plain)
                }.padding(10).background(AppTheme.raised).overlay(alignment: .top) { Divider() }
            }
            WorkspaceStatusBar(usage: store.usage, terminals: store.terminals, wake: store.wake)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(AppTheme.window)
        .background(WindowStyler())
        .overlay { if store.showPalette { CommandPalette() } }
        .onChange(of: store.searchFocusRequest) { _, _ in showSidebar = true }
        .onChange(of: store.dashboardRequest) { _, _ in openWindow(id: "dashboard") }
        .sheet(isPresented: Binding(get: { store.taskFromSpec != nil }, set: { if !$0 { store.taskFromSpec = nil } })) {
            if let (pid, spec) = store.taskFromSpec, let project = store.workspace.projects.first(where: { $0.id == pid }) {
                let title = (try? String(contentsOfFile: project.path + "/" + spec, encoding: .utf8))?.split(separator: "\n").first.map { $0.replacingOccurrences(of: "# ", with: "") } ?? spec
                TaskSheet(task: AgentTask(projectID: pid, title: String(title), spec: spec, mode: project.taskMode ?? "pr"))
            }
        }
        .onAppear { store.trackGit() }
        .onChange(of: store.workspace.projects.count) { _, _ in store.trackGit() }
        .onChange(of: store.workspace.selectedSessionID) { _, id in if id != nil { store.showSettings = false; store.projectSettingsID = nil } }
        .onChange(of: store.showProjectPage) { _, v in if v { store.projectSettingsID = nil } }
        .onChange(of: store.showSettings) { _, v in if v { store.projectSettingsID = nil } }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in store.updateBadge(); store.agentStatus.reload() }
        .sheet(item: $store.sessionCreationProject) { project in
            NewSessionSheet(project: project, pickProject: true)
        }
        .alert("Remove from Convoy?", isPresented: Binding(get: { projectToRemove != nil }, set: { if !$0 { projectToRemove = nil } }), presenting: projectToRemove) { project in
            Button("Cancel", role: .cancel) { projectToRemove = nil }
            Button("Remove", role: .destructive) { store.removeProject(project.id); projectToRemove = nil }
        } message: { project in
            Text("This removes \(project.name)'s saved specs, tasks, and session records from this app. Export anything you want to keep first. Child projects stay in the sidebar. Files and the agents’ own history stay on disk.")
        }
    }
}

/// Terminal-style tabs for every open session across all projects.
struct SessionTabBar: View {
    @EnvironmentObject var store: Store
    @ObservedObject var manager: TerminalManager
    @Binding var showSidebar: Bool
    @State private var closing: UUID?

    private var tabs: [(Project, LinkedSession)] {
        manager.openOrder.compactMap { id in
            for project in store.workspace.projects {
                if let session = project.linkedSessions.first(where: { $0.id == id && $0.archived != true }) { return (project, session) }
            }
            return nil
        }
    }

    var body: some View {
        HStack(spacing: 0) {
            Button { withAnimation(.easeInOut(duration: 0.15)) { showSidebar.toggle() } } label: {
                Image(systemName: "sidebar.left").font(.system(size: 13, weight: .medium))
                    .foregroundStyle(showSidebar ? AppTheme.accent : .secondary)
                    .frame(width: 30, height: 30).contentShape(Rectangle())
            }.buttonStyle(.plain).help(showSidebar ? "Hide sidebar" : "Show sidebar")
                .accessibilityLabel("Toggle sidebar").padding(.leading, 6)
            Button { store.goHome() } label: {
                Image(systemName: "house").font(.system(size: 12, weight: .medium))
                    .foregroundStyle(store.displayedSession == nil && !store.showProjectPage && !store.showSettings ? AppTheme.accent : .secondary)
                    .frame(width: 28, height: 30).contentShape(Rectangle())
            }.buttonStyle(.plain).help("Home (\(store.keys.display("go.home")))").accessibilityLabel("Home")
            Divider().frame(height: 18).padding(.horizontal, 6)
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 2) {
                    ForEach(Array(tabs.enumerated()), id: \.element.1.id) { index, pair in
                        let (project, session) = pair
                        SessionTab(project: project, session: session, number: index < 9 ? index + 1 : nil,
                                   running: manager.handles[session.id]?.running == true,
                                   state: store.agentState(session),
                                   paneIndex: store.paneCount > 1 ? store.panes.firstIndex(of: session.id) : nil,
                                   selected: store.workspace.selectedSessionID == session.id && store.panes.contains(session.id),
                                   select: { store.openSession(session.id, in: project.id) },
                                   close: {
                                       if manager.handles[session.id]?.running == true { closing = session.id }
                                       else { manager.close(session.id); moveSelectionAfterClosing(session.id) }
                                   })
                    }
                }.padding(.vertical, 4)
            }
            if tabs.isEmpty {
                Text("No open terminals").font(.system(size: 11)).foregroundStyle(.tertiary).padding(.leading, 4)
            }
            Spacer(minLength: 8)
            HStack(spacing: 2) {
                ForEach([(1, "rectangle"), (2, "rectangle.split.2x1"), (4, "rectangle.split.2x2")], id: \.0) { count, icon in
                    Button { store.setLayout(count) } label: {
                        Image(systemName: icon).font(.system(size: 12, weight: .medium))
                            .foregroundStyle(store.paneCount == count ? AppTheme.accent : .secondary)
                            .frame(width: 26, height: 24).contentShape(Rectangle())
                            .background(store.paneCount == count ? AppTheme.accent.opacity(0.14) : .clear, in: RoundedRectangle(cornerRadius: 5))
                    }.buttonStyle(.plain).help("\(count == 1 ? "One pane" : "\(count) panes") (\(store.keys.display("view.layout\(count)")))")
                }
            }.padding(.trailing, 6)
            Button { store.toggleGitPanel() } label: {
                ZStack(alignment: .topTrailing) {
                    Image(systemName: "sidebar.right").font(.system(size: 12, weight: .medium)).foregroundStyle(store.showGitPanel ? AppTheme.accent : .secondary)
                    if let changed = store.gitPanelDirectory.flatMap({ store.git.info[$0]?.changed }), changed > 0 {
                        Text("\(min(99, changed))").font(.system(size: 8, weight: .bold)).foregroundStyle(.white)
                            .padding(.horizontal, 3).padding(.vertical, 1).background(AppTheme.accent, in: Capsule()).offset(x: 7, y: -5)
                    }
                }.frame(width: 28, height: 28).contentShape(Rectangle())
            }.buttonStyle(.plain).help("Files & Changes (\(store.keys.display("git.panel")))").accessibilityLabel("Toggle Files & Changes panel")
            Button { store.showActivity.toggle() } label: {
                ZStack(alignment: .topTrailing) {
                    Image(systemName: "bell").font(.system(size: 12, weight: .medium)).foregroundStyle(store.showActivity ? AppTheme.accent : .secondary)
                    if store.unreadActivity > 0 {
                        Text("\(min(99, store.unreadActivity))").font(.system(size: 8, weight: .bold)).foregroundStyle(.white)
                            .padding(.horizontal, 3).padding(.vertical, 1).background(Color.orange, in: Capsule()).offset(x: 7, y: -5)
                    }
                }.frame(width: 28, height: 28).contentShape(Rectangle())
            }.buttonStyle(.plain).help("Activity (\(store.keys.display("go.activity")))").accessibilityLabel("Activity feed")
                .popover(isPresented: $store.showActivity, arrowEdge: .bottom) { ActivityFeed().environmentObject(store) }
            if !store.workspace.projects.isEmpty {
                Button { store.newSessionInCurrentProject() } label: {
                    Image(systemName: "plus").font(.system(size: 12, weight: .semibold)).foregroundStyle(.secondary)
                        .frame(width: 28, height: 28).contentShape(Rectangle())
                }.buttonStyle(.plain).help("New session… (\(store.keys.display("session.new")))").accessibilityLabel("New session")
                    .padding(.trailing, 6)
            }
        }
        .frame(height: 38).background(AppTheme.raised)
        .onChange(of: store.closeTabRequest) { _, id in
            guard let id else { return }
            store.closeTabRequest = nil
            if manager.handles[id]?.running == true { closing = id } else { manager.close(id); moveSelectionAfterClosing(id) }
        }
        .alert("Stop this agent and close the tab?", isPresented: Binding(get: { closing != nil }, set: { if !$0 { closing = nil } }), presenting: closing) { id in
            Button("Cancel", role: .cancel) { closing = nil }
            Button("Stop and close", role: .destructive) {
                manager.close(id); closing = nil; moveSelectionAfterClosing(id)
            }
        } message: { _ in
            Text("Recent output stays saved and the session can be resumed later.")
        }
    }
}

extension SessionTabBar {
    /// After closing the selected tab, show the nearest remaining one.
    func moveSelectionAfterClosing(_ id: UUID) {
        if let index = store.panes.firstIndex(of: id) { store.panes[index] = nil }
        guard store.workspace.selectedSessionID == id else { return }
        if let other = store.panes.firstIndex(where: { $0 != nil }) { store.focusPane(other) }
        else if let last = store.tabOrder.last, let project = store.project(ofSession: last) { store.openSession(last, in: project.id) } else { store.goHome() }
    }
}

struct SessionTab: View {
    @EnvironmentObject var store: Store
    let project: Project
    let session: LinkedSession
    var number: Int? = nil
    let running: Bool
    var state: AgentState? = nil
    var paneIndex: Int? = nil
    let selected: Bool
    let select: () -> Void
    let close: () -> Void
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 7) {
            RoundedRectangle(cornerRadius: 1.5).fill(project.tint).frame(width: 3, height: 18)
            AgentStateGlyph(state: state, running: running, size: 6).frame(width: 10)
            AgentIcon(agent: session.agent, size: 12).opacity(0.85)
            VStack(alignment: .leading, spacing: 0) {
                Text(session.title).font(.system(size: 12, weight: selected ? .semibold : .medium)).lineLimit(1)
                Text(project.name).font(.system(size: 9.5)).foregroundStyle(.secondary).lineLimit(1)
            }
            if let paneIndex, !hovered {
                Text("\(paneIndex + 1)").font(.system(size: 9, weight: .bold)).foregroundStyle(.white)
                    .frame(width: 14, height: 14).background(project.tint, in: RoundedRectangle(cornerRadius: 3)).help("In pane \(paneIndex + 1)")
            } else if let number, !hovered, !selected {
                Text("⌘\(number)").font(.system(size: 9, weight: .medium)).foregroundStyle(.tertiary).frame(width: 16, height: 16)
            } else {
            Button(action: close) {
                Image(systemName: "xmark").font(.system(size: 9, weight: .bold))
                    .foregroundStyle(.secondary).frame(width: 16, height: 16)
                    .background(hovered ? Color.primary.opacity(0.08) : .clear, in: RoundedRectangle(cornerRadius: 4))
            }.buttonStyle(.plain).opacity(hovered || selected ? 1 : 0)
                .help(running ? "Stop and close" : "Close tab").accessibilityLabel("Close \(session.title)")
            }
        }
        .padding(.leading, 10).padding(.trailing, 6).frame(height: 30).frame(minWidth: 120, maxWidth: 220)
        .background(selected ? AppTheme.accent.opacity(0.16) : (hovered ? Color.primary.opacity(0.05) : .clear), in: RoundedRectangle(cornerRadius: 7))
        .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(selected ? AppTheme.accent.opacity(0.35) : .clear, lineWidth: 1))
        .contentShape(RoundedRectangle(cornerRadius: 7))
        .onTapGesture(perform: select)
        .draggable(session.id.uuidString)
        .contextMenu {
            ForEach(0..<store.paneCount, id: \.self) { i in Button("Open in pane \(i + 1)") { store.place(session.id, in: i); store.focusPane(i) } }
            Button("Close tab", action: close)
            Button("Close other tabs") { for id in store.tabOrder where id != session.id && store.terminals.handles[id]?.running != true { store.terminals.close(id) }; store.openSession(session.id, in: project.id) }
        }
        .onHover { hovered = $0 }
        .help("\(project.name) · \(session.agent.rawValue)\(running ? "" : " · not running")")
        .accessibilityElement(children: .combine).accessibilityAddTraits(selected ? .isSelected : [])
    }
}

struct SidebarView: View {
    @EnvironmentObject var store: Store
    @Binding var projectToRemove: Project?
    @State private var projectSearch = ""
    @AppStorage("showGroupHierarchy") private var showGroups = true
    @AppStorage("sortProjectsByName") private var sortByName = false
    @AppStorage("compactSidebar") private var compactSidebar = true
    @FocusState private var searchFocused: Bool

    var visibleProjects: [Project] {
        let candidates = showGroups ? store.workspace.rootProjects : store.workspace.projects
        let filtered = candidates.filter { matches($0) }
        return sortByName ? filtered.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending } : filtered
    }

    func matches(_ project: Project) -> Bool {
        projectSearch.isEmpty || project.name.localizedCaseInsensitiveContains(projectSearch) ||
        (showGroups && store.workspace.children(of: project.id).contains { matches($0) })
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Button { store.goHome() } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "square.stack.3d.up.fill").foregroundStyle(AppTheme.accent)
                        Text("Convoy").font(.system(size: 15, weight: .semibold))
                    }.contentShape(Rectangle())
                }.buttonStyle(.plain).help("Home")
                Spacer()
                Menu {
                    Text("Workspace options")
                    Toggle("Show group hierarchy", isOn: $showGroups)
                    Toggle("Sort by name", isOn: $sortByName)
                    Toggle("Compact rows", isOn: $compactSidebar)
                } label: { Image(systemName: "slider.horizontal.3").frame(width: 24, height: 24) }
                    .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                    .foregroundStyle(.secondary).help("Workspace options").accessibilityLabel("Workspace options")
            }.padding(.horizontal, 14).frame(height: 38)
            HStack(spacing: 6) {
                Image(systemName: "magnifyingglass").font(.system(size: 11)).foregroundStyle(.secondary)
                TextField("Find a project", text: $projectSearch).textFieldStyle(.plain).font(.system(size: 12)).focused($searchFocused)
                    .onChange(of: store.searchFocusRequest) { _, _ in searchFocused = true }
                if !projectSearch.isEmpty {
                    Button { projectSearch = "" } label: { Image(systemName: "xmark.circle.fill").foregroundStyle(.secondary) }
                        .buttonStyle(.plain).help("Clear project search")
                }
            }.padding(.horizontal, 8).frame(height: 28)
                .background(Color.primary.opacity(0.05), in: RoundedRectangle(cornerRadius: 7))
                .padding(.horizontal, 12).padding(.bottom, 10)
            HStack {
                Text("PROJECTS").font(.system(size: 10, weight: .semibold)).tracking(0.7).foregroundStyle(.secondary)
                Spacer()
                Button(action: store.addProject) { Image(systemName: "plus").font(.system(size: 10, weight: .bold)).frame(width: 20, height: 20) }
                    .buttonStyle(.plain).foregroundStyle(.secondary).disabled(store.isImporting).help("Open folder")
            }.padding(.horizontal, 16).padding(.bottom, 4)
            ScrollView {
                VStack(alignment: .leading, spacing: 1) {
                    let pinned = store.pinnedSessions
                    if !pinned.isEmpty && projectSearch.isEmpty {
                        Text("PINNED").font(.system(size: 10, weight: .semibold)).tracking(0.7).foregroundStyle(.secondary).padding(.horizontal, 8).padding(.bottom, 2)
                        ForEach(pinned, id: \.1.id) { project, session in
                            SidebarSessionRow(project: project, session: session, showProject: true)
                        }
                        Divider().padding(.vertical, 6)
                    }
                    ForEach(visibleProjects) { project in
                        ProjectTreeRow(project: project, remove: { projectToRemove = $0 }, query: projectSearch, showChildren: showGroups)
                            .draggable(project.id.uuidString)
                            .dropDestination(for: String.self) { items, _ in
                                guard let raw = items.first, let id = UUID(uuidString: raw), id != project.id else { return false }
                                store.moveProject(id, before: project.id); return true
                            }
                    }
                    if visibleProjects.isEmpty {
                        Text(projectSearch.isEmpty ? "No projects yet." : "No matching projects")
                            .font(.caption).foregroundStyle(.secondary).padding(.horizontal, 10).padding(.vertical, 12)
                    }
                }.padding(.horizontal, 8).padding(.bottom, 8)
            }
            Divider()
            HStack(alignment: .center, spacing: 8) {
                VStack(alignment: .leading, spacing: 4) {
                    if store.isImporting { ProgressView("Importing projects…").font(.caption).controlSize(.small) }
                    Button(action: store.addProject) { Label("Open folder…", systemImage: "folder.badge.plus").font(.system(size: 12, weight: .medium)) }
                        .buttonStyle(.plain).foregroundStyle(AppTheme.accent).disabled(store.isImporting)
                    Text("A repository, or a folder of related projects.").font(.system(size: 10.5)).foregroundStyle(.tertiary)
                }
                Spacer()
                Button { store.showSettings.toggle() } label: {
                    Image(systemName: "gearshape").font(.system(size: 13)).foregroundStyle(store.showSettings ? AppTheme.accent : .secondary)
                        .frame(width: 26, height: 26).contentShape(Rectangle())
                }.buttonStyle(.plain).help("Settings (⌘,)").accessibilityLabel("Settings")
            }.padding(.horizontal, 16).padding(.vertical, 12)
        }
        .background(AppTheme.sidebar)
    }
}

struct ProjectTreeRow: View {
    @EnvironmentObject var store: Store
    let project: Project
    let remove: (Project) -> Void
    var query = ""
    var showChildren = true
    @AppStorage("compactSidebar") private var compactSidebar = true
    @AppStorage("sortProjectsByName") private var sortByName = false
    private var expanded: Bool { store.isExpanded(project.id) }
    @State private var hovered = false
    @State private var iconSheet = false
    @State private var setupSheet = false
    @State private var workflowSheet = false

    @ViewBuilder private var actions: some View {
        Button {
            store.selectProject(project.id)
            store.sessionCreationProject = project
        } label: { Label("New session…", systemImage: "plus.rectangle") }
        Divider()
        Button { store.importSubprojects(project) } label: {
            Label(project.isGroup ? "Refresh projects" : "Import projects from this folder", systemImage: "arrow.clockwise")
        }.disabled(store.isImporting)
        Button { NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: project.path) } label: {
            Label("Show in Finder", systemImage: "folder")
        }
        Button { store.copy(project.path) } label: { Label("Copy folder path", systemImage: "doc.on.doc") }
        Button { store.selectProject(project.id); store.reconnectProject() } label: {
            Label("Reconnect folder…", systemImage: "link")
        }
        Divider()
        Button { store.projectSettingsID = project.id; store.showSettings = false } label: { Label("Project settings…", systemImage: "gearshape") }
        if project.parentID == nil {
            Button { store.moveProject(project.id, by: -1) } label: { Label("Move up", systemImage: "arrow.up") }
            Button { store.moveProject(project.id, by: 1) } label: { Label("Move down", systemImage: "arrow.down") }
        }
        Divider()
        Button(role: .destructive) { remove(project) } label: {
            Label("Remove from SpecDesk…", systemImage: "trash")
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 6) {
                if project.isGroup || !project.linkedSessions.filter({ $0.archived != true }).isEmpty {
                    Button { store.toggleExpanded(project.id) } label: {
                        Image(systemName: expanded ? "chevron.down" : "chevron.right").font(.system(size: 9, weight: .bold)).foregroundStyle(.secondary).frame(width: 16, height: 24)
                    }.buttonStyle(.plain).help(expanded ? "Collapse" : "Expand")
                        .accessibilityLabel("\(expanded ? "Collapse" : "Expand") \(project.name)")
                } else {
                    Color.clear.frame(width: 16, height: 24)
                }
                Button { store.selectProject(project.id) } label: {
                    HStack(spacing: 8) {
                        ProjectIconView(project: project, size: 14)
                        VStack(alignment: .leading, spacing: 3) {
                            Text(project.name).font(.system(size: 12.5, weight: store.project?.id == project.id ? .semibold : .regular)).lineLimit(1).help(project.path)
                                .foregroundStyle(project.color.map { Color(hex: $0) } ?? Color.primary)
                            if project.isGroup && !compactSidebar {
                                Text("Group · \(store.workspace.children(of: project.id).count) projects").font(.caption2).foregroundStyle(.secondary)
                            } else if !compactSidebar {
                                let git = store.git.info[project.path]
                                Text([git.map { "⎇ \($0.branch)" } ?? "", git?.summary ?? ""].filter { !$0.isEmpty }.joined(separator: " · ").isEmpty ? "\(project.linkedSessions.filter { $0.archived != true }.count) sessions" : [git.map { "⎇ \($0.branch)" } ?? "", git?.summary ?? ""].filter { !$0.isEmpty }.joined(separator: " · "))
                                    .font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                            }
                        }
                        Spacer(minLength: 0)
                    }.contentShape(Rectangle())
                }.buttonStyle(.plain)
                if let waiting = project.linkedSessions.first(where: { store.agentState($0)?.needsYou == true && store.terminals.handles[$0.id]?.running == true }) {
                    Circle().fill(Color.orange).frame(width: 6, height: 6).help("\(waiting.title) needs you")
                } else if store.terminals.hasRunningSessions(in: project.id) {
                    Circle().fill(Color.green).frame(width: 5, height: 5).help("Running session")
                }
                Menu { actions } label: {
                    Image(systemName: "ellipsis").font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.secondary).frame(width: 24, height: 24)
                        .contentShape(RoundedRectangle(cornerRadius: 6))
                }.menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
                    .opacity(hovered || store.project?.id == project.id ? 1 : 0)
                    .help("Actions for \(project.name)")
                    .accessibilityLabel("Actions for \(project.name)")
            }.padding(.leading, 4).padding(.trailing, 4).padding(.vertical, compactSidebar ? 3 : 7)
                .background(store.project?.id == project.id ? AppTheme.accent.opacity(0.16) : (hovered ? Color.primary.opacity(0.05) : .clear), in: RoundedRectangle(cornerRadius: 7))
                .onHover { hovered = $0 }
                .contextMenu { actions }
                .sheet(isPresented: $iconSheet) { ProjectIconSheet(project: project) }
                .sheet(isPresented: $setupSheet) { WorktreeSetupSheet(project: project) }
                .sheet(isPresented: $workflowSheet) { ProjectWorkflowSheet(project: project) }
            if expanded && query.isEmpty {
                ForEach(project.linkedSessions.filter { $0.archived != true }.sorted { $0.createdAt > $1.createdAt }) { session in
                    SidebarSessionRow(project: project, session: session).padding(.leading, 22)
                }
            }
            if project.isGroup && expanded && showChildren {
                ForEach(visibleChildren) { child in
                    ProjectTreeRow(project: child, remove: remove, query: project.name.localizedCaseInsensitiveContains(query) ? "" : query).padding(.leading, 14)
                }
            }
        }
    }

    private var visibleChildren: [Project] {
        func matches(_ candidate: Project) -> Bool {
            query.isEmpty || candidate.name.localizedCaseInsensitiveContains(query) || store.workspace.children(of: candidate.id).contains { matches($0) }
        }
        let children = store.workspace.children(of: project.id).filter { project.name.localizedCaseInsensitiveContains(query) || matches($0) }
        return sortByName ? children.sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending } : children
    }
}

struct ProjectWorkbench: View {
    @EnvironmentObject var store: Store
    let project: Project
    private var area: String {
        get { store.area(of: project) }
        nonmutating set { store.areaByProject[project.id] = newValue }
    }
    private var areaBinding: Binding<String> { Binding(get: { area }, set: { area = $0 }) }
    @State private var newSpec = false
    @State private var title = ""
    @State private var search = ""
    @State private var newSession = false

    private var hasOpenSession: Bool {
        !["Specs", "History", "Tasks", "Docs"].contains(area) && project.linkedSessions.contains {
            $0.id == store.workspace.selectedSessionID && store.tabOrder.contains($0.id) && $0.archived != true && (area != "Reviews" || $0.reviewOf != nil)
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if !hasOpenSession {
            VStack(alignment: .leading, spacing: 12) {
                HStack {
                    VStack(alignment: .leading, spacing: 5) {
                        HStack(spacing: 10) {
                            Text(project.name).font(.system(size: 24, weight: .bold)).lineLimit(1)
                            Text(project.isGroup ? "GROUP" : "PROJECT")
                                .font(.system(size: 9, weight: .semibold)).tracking(0.5)
                                .foregroundStyle(AppTheme.accent).padding(.horizontal, 8).padding(.vertical, 4)
                                .background(AppTheme.accent.opacity(0.09), in: Capsule())
                        }
                        Text(project.path).font(.system(size: 11, design: .monospaced))
                            .foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                            .textSelection(.enabled).help(project.path)
                    }
                    Spacer()
                    Button { store.projectSettingsID = project.id } label: { Image(systemName: "gearshape") }.controlSize(.large).help("Project settings")
                    Button { area = "Sessions"; newSession = true } label: { Label("New session", systemImage: "plus") }
                        .buttonStyle(.borderedProminent).controlSize(.large)
                }
                Picker("Workspace section", selection: areaBinding) {
                    Text("Sessions").tag("Sessions")
                    Text("Reviews").tag("Reviews")
                    Text("Specs").tag("Specs")
                    Text("History").tag("History")
                    Text("Tasks").tag("Tasks")
                    Text("Docs").tag("Docs")
                }.pickerStyle(.segmented).labelsHidden().frame(width: 560)
                    .help("⌥⌘1–6 jump to a section · \(store.keys.display("project.previousArea")) / \(store.keys.display("project.nextArea")) cycle")
            }.padding(.horizontal, 24).padding(.vertical, 20)
            Divider()
            }
            if area == "History" {
                HistoryPanel(project: project)
            } else if area == "Tasks" {
                TasksPanel(project: project)
            } else if area == "Docs" {
                DocsPanel(project: project)
            } else if area == "Specs" {
                HSplitView {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack {
                            Text("Specifications").font(.headline)
                            Spacer()
                            Button { title = ""; newSpec = true } label: { Image(systemName: "plus") }
                        }
                        TextField("Search specifications", text: $search).textFieldStyle(.roundedBorder)
                        ScrollView {
                            VStack(spacing: 8) {
                                let visible = project.specs.filter { search.isEmpty || $0.title.localizedCaseInsensitiveContains(search) }
                                if visible.isEmpty {
                                    Text(project.specs.isEmpty ? "No specifications yet." : "No matches.").font(.caption).foregroundStyle(.tertiary)
                                        .frame(maxWidth: .infinity, alignment: .leading).padding(.top, 4)
                                }
                                ForEach(visible) { spec in
                                    Button { store.selectSpec(spec.id) } label: {
                                        VStack(alignment: .leading, spacing: 5) {
                                            Text(spec.title).font(.headline)
                                            Text(spec.isApproved ? "Approved" : "Draft").font(.caption).foregroundStyle(.secondary)
                                        }.frame(maxWidth: .infinity, alignment: .leading).padding(12)
                                            .background(store.spec?.id == spec.id ? AppTheme.accent.opacity(0.12) : Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 8))
                                    }.buttonStyle(.plain)
                                }
                            }
                        }
                        Button("Create specification") { title = ""; newSpec = true }
                    }.padding(16).frame(minWidth: 200, idealWidth: 230, maxWidth: 280, maxHeight: .infinity, alignment: .top)
                    Group {
                        if let spec = store.spec {
                            SpecView(spec: spec).id(spec.id)
                        } else {
                            ContentUnavailableView("Specs are optional", systemImage: "doc.text", description: Text("Use a spec when work benefits from written requirements and acceptance criteria. Sessions do not require one."))
                        }
                    }.frame(maxWidth: .infinity, maxHeight: .infinity)
                }.frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                SessionWorkspace(project: project, area: areaBinding, newSession: $newSession)
            }
        }
        .onChange(of: store.sessionCreationProject?.id) { _, id in
            if id == project.id { area = "Sessions" }
        }
        .onChange(of: store.areaRequest) { _, value in
            guard let value else { return }
            store.areaRequest = nil; area = value
        }
        .onChange(of: store.newSpecRequest) { _, _ in area = "Specs"; title = ""; newSpec = true }
        .onChange(of: store.workspace.selectedSessionID) { _, id in
            if id != nil && ["Specs", "History", "Tasks", "Docs"].contains(area) { area = "Sessions" }
        }
        .onChange(of: store.sessionFocusRequest) { _, _ in
            let selected = project.linkedSessions.first { $0.id == store.workspace.selectedSessionID }
            if area != "Reviews" || selected?.reviewOf == nil { area = "Sessions" }
        }
        .sheet(isPresented: $newSession) { NewSessionSheet(project: project) }
        .sheet(isPresented: $newSpec) {
            VStack(alignment: .leading, spacing: 20) {
                Text("New specification").font(.title2.bold())
                TextField("Feature or problem", text: $title).textFieldStyle(.roundedBorder)
                HStack {
                    Spacer(); Button("Cancel") { newSpec = false }
                    Button("Create spec") { store.addSpec(title: title); newSpec = false }
                        .buttonStyle(.borderedProminent).disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }.padding(24).frame(width: 460)
        }
    }
}

struct SpecView: View {
    @EnvironmentObject var store: Store
    let spec: Specification
    @State private var tab = "Spec"
    @State private var taskTitle = ""
    @State private var editing = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 14) {
                HStack {
                    Label(spec.isApproved ? "APPROVED · v\(spec.revision)" : "DRAFT · v\(spec.revision)", systemImage: "doc.text")
                        .font(.caption.weight(.semibold)).foregroundStyle(AppTheme.accent)
                    Spacer()
                    Text("Saved locally").font(.caption).foregroundStyle(.secondary)
                }
                Text(spec.title).font(.system(size: 27, weight: .bold))
                HStack {
                    Button("Edit spec") { editing = true }
                    Button("Export Markdown…", action: store.exportSpec)
                    Spacer()
                    if !spec.isApproved {
                        Button("Approve spec") { var value = spec; value.approvedRevision = value.revision; store.updateSpec(value) }
                            .buttonStyle(.borderedProminent)
                            .disabled(spec.requirements.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || spec.acceptance.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                            .help("Add requirements and acceptance criteria before approval")
                    }
                }
                Picker("Section", selection: $tab) {
                    Text("Spec").tag("Spec")
                    Text("Tasks (\(spec.tasks.count))").tag("Tasks")
                    Text("Sessions (\(spec.tasks.flatMap(\.sessions).count))").tag("Sessions")
                }.pickerStyle(.segmented)
            }.padding(24)
            Divider()
            ScrollView {
                VStack(alignment: .leading, spacing: 22) {
                    if tab == "Spec" {
                        section("Problem", text: spec.problem, placeholder: "What problem are we solving?")
                        section("Requirements", text: spec.requirements, placeholder: "What must the feature do?")
                        section("Acceptance criteria", text: spec.acceptance, placeholder: "How will we know it works?")
                        section("Constraints & out of scope", text: spec.constraints, placeholder: "Set the boundaries of this feature.")
                        section("Implementation plan", text: spec.plan, placeholder: "Outline the approach and verification steps.")
                    } else if tab == "Tasks" {
                        HStack {
                            TextField("New task", text: $taskTitle).textFieldStyle(.roundedBorder).onSubmit(addTask)
                            Button("Add", action: addTask).disabled(taskTitle.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                        }
                        if spec.tasks.isEmpty { Text("Break the spec into focused tasks. Each task can have a builder, reviewer, and linked sessions.").foregroundStyle(.secondary) }
                        ForEach(spec.tasks) { task in TaskCard(task: task, spec: spec).id(task.id) }
                    } else {
                        Text("Link existing session IDs from your terminal. These records preserve your place; live connections and in-app resume are not implemented yet.")
                            .font(.callout).foregroundStyle(.secondary)
                        if spec.tasks.flatMap(\.sessions).isEmpty {
                            ContentUnavailableView("No linked sessions", systemImage: "terminal", description: Text("Open a task to link its Claude Code or Codex session."))
                        }
                        ForEach(spec.tasks) { task in
                            ForEach(task.sessions) { session in
                                VStack(alignment: .leading, spacing: 8) {
                                    Label(session.title, systemImage: "terminal").font(.headline)
                                    Text("\(session.agent.rawValue) · \(task.title)").font(.caption).foregroundStyle(.secondary)
                                    Text(session.sessionID).font(.system(.caption, design: .monospaced)).textSelection(.enabled)
                                    if !session.notes.isEmpty { Text(session.notes).font(.callout) }
                                    Button("Copy session ID") { store.copy(session.sessionID) }
                                }.padding(16).frame(maxWidth: .infinity, alignment: .leading)
                                    .background(.quaternary.opacity(0.35), in: RoundedRectangle(cornerRadius: 10))
                            }
                        }
                    }
                }.padding(24).frame(maxWidth: .infinity, alignment: .leading)
            }
            Divider()
            Label("Tasks are tracked manually · Start agents from Sessions", systemImage: "info.circle")
                .font(.caption).foregroundStyle(.secondary).padding(12)
        }
        .sheet(isPresented: $editing) { SpecEditor(original: spec) }
    }

    func section(_ title: String, text: String, placeholder: String) -> some View {
        VStack(alignment: .leading, spacing: 9) {
            Text(title).font(.headline)
            Text(text.isEmpty ? placeholder : text).foregroundStyle(text.isEmpty ? .secondary : .primary)
                .textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    func addTask() {
        let title = taskTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return }
        var value = spec; value.tasks.append(WorkTask(title: title)); store.updateSpec(value); taskTitle = ""
    }
}

struct SpecEditor: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    let original: Specification
    @State private var draft: Specification

    init(original: Specification) { self.original = original; _draft = State(initialValue: original) }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Edit specification").font(.title2.bold())
            if original.isApproved { Text("Saving changes creates a new draft revision. Completed tasks will need review again.").font(.callout).foregroundStyle(.secondary) }
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    TextField("Title", text: $draft.title).textFieldStyle(.roundedBorder)
                    field("Problem", text: $draft.problem)
                    field("Requirements", text: $draft.requirements)
                    field("Acceptance criteria", text: $draft.acceptance)
                    field("Constraints & out of scope", text: $draft.constraints)
                    field("Implementation plan", text: $draft.plan)
                }
            }
            HStack {
                Spacer(); Button("Cancel") { dismiss() }
                Button("Save changes") {
                    if draft != original { store.updateSpec(draft, requirementsChanged: true) }
                    if store.error == nil { dismiss() }
                }.buttonStyle(.borderedProminent).disabled(draft.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }.padding(24).frame(width: 620, height: 680)
    }

    func field(_ title: String, text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            TextEditor(text: text).font(.body).frame(height: 90).padding(6)
                .background(.background, in: RoundedRectangle(cornerRadius: 6))
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(.quaternary))
                .accessibilityLabel(title)
        }
    }
}

struct TaskCard: View {
    @EnvironmentObject var store: Store
    let task: WorkTask
    let spec: Specification
    @State private var edit = false
    @State private var linkSession = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top) {
                Text(task.title).font(.headline)
                Spacer()
                Picker("Status", selection: Binding(get: { task.status }, set: { var value = task; value.status = $0; store.updateTask(value) })) {
                    ForEach(TaskStatus.allCases) { Text($0.rawValue).tag($0) }
                }.labelsHidden().frame(width: 165)
            }
            Text("\(task.builder.rawValue) → \(task.reviewer.rawValue)").font(.callout).foregroundStyle(AppTheme.accent)
            if !task.notes.isEmpty { Text(task.notes).lineLimit(3).font(.callout).foregroundStyle(.secondary) }
            if !task.findings.isEmpty { Label("Review findings recorded", systemImage: "text.bubble").font(.caption).foregroundStyle(.orange) }
            Text("\(task.sessions.count) linked sessions · Status is set manually").font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("Edit task") { edit = true }
                Button("Link session") { linkSession = true }
                Button("Copy review brief") { store.copy(store.reviewBrief(for: task, spec: spec)) }
            }
        }.padding(16).background(Color.primary.opacity(0.035), in: RoundedRectangle(cornerRadius: 12))
            .sheet(isPresented: $edit) { TaskEditor(task: task) }
            .sheet(isPresented: $linkSession) { SessionEditor(task: task) }
    }
}

struct TaskEditor: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State var task: WorkTask

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Task & review").font(.title2.bold())
            TextField("Task title", text: $task.title).textFieldStyle(.roundedBorder)
            HStack {
                Picker("Builder", selection: $task.builder) { ForEach(Agent.allCases) { Text($0.rawValue).tag($0) } }
                Picker("Reviewer", selection: $task.reviewer) { ForEach(Agent.allCases) { Text($0.rawValue).tag($0) } }
            }
            Text("Implementation notes & verification evidence").font(.headline)
            TextEditor(text: $task.notes).frame(height: 130).border(.quaternary)
            Text("Review findings").font(.headline)
            TextEditor(text: $task.findings).frame(height: 130).border(.quaternary)
            HStack { Spacer(); Button("Cancel") { dismiss() }; Button("Save task") { store.updateTask(task); if store.error == nil { dismiss() } }.buttonStyle(.borderedProminent).disabled(task.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty) }
        }.padding(24).frame(width: 580)
    }
}

struct SessionEditor: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    let task: WorkTask
    @State private var agent: Agent = .claude
    @State private var title = ""
    @State private var sessionID = ""
    @State private var notes = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("Link an existing session").font(.title2.bold())
            Text("Save the session ID from your agent so you can find it again. This does not start or connect to a running process.").foregroundStyle(.secondary)
            Picker("Agent", selection: $agent) { ForEach(Agent.allCases) { Text($0.rawValue).tag($0) } }
            TextField("Session name", text: $title).textFieldStyle(.roundedBorder)
            TextField("Session ID", text: $sessionID).textFieldStyle(.roundedBorder)
            TextField("Where did you leave off?", text: $notes, axis: .vertical).textFieldStyle(.roundedBorder)
            HStack {
                Spacer(); Button("Cancel") { dismiss() }
                Button("Save session") {
                    var value = task
                    value.sessions.append(LinkedSession(agent: agent, sessionID: sessionID.trimmingCharacters(in: .whitespacesAndNewlines), title: title.trimmingCharacters(in: .whitespacesAndNewlines), notes: notes))
                    store.updateTask(value)
                    if store.error == nil { dismiss() }
                }.buttonStyle(.borderedProminent).disabled(title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || sessionID.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }.padding(24).frame(width: 480)
    }
}

/// A session nested under its project in the sidebar, Orca-style.
struct SidebarSessionRow: View {
    @EnvironmentObject var store: Store
    let project: Project
    let session: LinkedSession
    var showProject = false
    @AppStorage("compactSidebar") private var compactSidebar = true
    @State private var hovered = false
    @State private var stopConfirm = false

    private var handle: TerminalHandle? { store.terminals.handles[session.id] }
    private var running: Bool { handle?.running == true }
    private var selected: Bool { store.project?.id == project.id && store.workspace.selectedSessionID == session.id }
    private var multi: Bool { store.multiSelection.contains(session.id) }

    var body: some View {
        Button {
            if NSEvent.modifierFlags.contains(.command) {
                if multi { store.multiSelection.remove(session.id) } else { store.multiSelection.insert(session.id) }
            } else { store.multiSelection = []; store.openSession(session.id, in: project.id) }
        } label: {
            HStack(spacing: 7) {
                if store.isHibernated(session.id) {
                    Image(systemName: "moon.zzz.fill").font(.system(size: 8)).foregroundStyle(.secondary).frame(width: 10).help("Sleeping — opens and resumes on click")
                } else {
                    AgentStateGlyph(state: store.agentState(session), running: running, size: 6).frame(width: 10)
                }
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        if session.pinned == true { Image(systemName: "pin.fill").font(.system(size: 8)).foregroundStyle(.secondary) }
                        Text((showProject ? project.name + " / " : "") + session.title).font(.system(size: 12, weight: selected ? .semibold : .regular)).lineLimit(1)
                        if let branch = session.branch {
                            Label(branch, systemImage: "arrow.triangle.branch").font(.system(size: 9, weight: .medium)).foregroundStyle(AppTheme.accent).lineLimit(1)
                                .padding(.horizontal, 5).padding(.vertical, 1.5)
                                .background(AppTheme.accent.opacity(0.12), in: RoundedRectangle(cornerRadius: 4))
                        } else {
                            AgentIcon(agent: session.agent, size: 11).opacity(0.85).help(session.agent.rawValue)
                            if session.reviewOf != nil { Text("review").font(.system(size: 9, weight: .medium)).foregroundStyle(.secondary) }
                        }
                    }
                    if !compactSidebar {
                        let git = session.workingDirectory.flatMap { store.git.info[$0] }
                        Text([running ? AgentStateGlyph(state: store.agentState(session), running: true).label : (handle == nil ? "Saved" : "Stopped"), git?.summary ?? ""].filter { !$0.isEmpty }.joined(separator: " · "))
                            .font(.system(size: 10)).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
                Spacer(minLength: 0)
            }.contentShape(Rectangle())
        }.buttonStyle(.plain)
            .draggable(session.id.uuidString)
            .padding(.horizontal, 8).padding(.vertical, compactSidebar ? 3 : 5)
            .background(multi ? AppTheme.accent.opacity(0.28) : (selected ? AppTheme.accent.opacity(0.16) : (hovered ? Color.primary.opacity(0.05) : .clear)), in: RoundedRectangle(cornerRadius: 7))
            .onHover { hovered = $0 }
            .help(session.notes.isEmpty ? session.title + " (⌘-click to multi-select)" : session.notes)
            .contextMenu {
                if store.multiSelection.count > 1 && multi {
                    Text("\(store.multiSelection.count) selected")
                    Button { store.archive(sessions: store.multiSelection) } label: { Label("Archive selected (stopped only)", systemImage: "archivebox") }
                    Button { store.closeTabs(store.multiSelection) } label: { Label("Close selected tabs", systemImage: "xmark.rectangle") }
                    Button { store.multiSelection = [] } label: { Label("Clear selection", systemImage: "xmark") }
                    Divider()
                }
                Button { store.togglePin(session, in: project) } label: { Label(session.pinned == true ? "Unpin" : "Pin to top", systemImage: session.pinned == true ? "pin.slash" : "pin") }
                if running {
                    Button { store.sleep(session, in: project) } label: { Label("Sleep", systemImage: "moon.zzz") }
                }
                if store.paneCount > 1 {
                    ForEach(0..<store.paneCount, id: \.self) { i in Button("Open in pane \(i + 1)") { store.openSession(session.id, in: project.id); store.place(session.id, in: i); store.focusPane(i) } }
                }
                if !running {
                    Button { store.startSession(session, in: project, resume: true) } label: { Label("Resume", systemImage: "play") }
                } else {
                    Button(role: .destructive) { stopConfirm = true } label: { Label("Stop session…", systemImage: "stop") }
                }
                Button {
                    var value = session; value.archived = true
                    store.updateSession(value, project: project.id)
                } label: { Label("Archive", systemImage: "archivebox") }.disabled(running)
                if let dir = session.workingDirectory {
                    Divider()
                    Button { NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: dir) } label: { Label("Show worktree in Finder", systemImage: "folder") }
                    Button { store.copy(dir) } label: { Label("Copy worktree path", systemImage: "doc.on.doc") }
                    Button(role: .destructive) { store.removeWorktree(session, in: project, deleteBranch: false) } label: { Label("Remove worktree…", systemImage: "trash") }.disabled(running)
                    Button(role: .destructive) { store.removeWorktree(session, in: project, deleteBranch: true) } label: { Label("Remove worktree & delete branch…", systemImage: "trash.slash") }.disabled(running)
                }
            }
            .alert("Stop this agent?", isPresented: $stopConfirm) {
                Button("Cancel", role: .cancel) {}
                Button("Stop", role: .destructive) { handle?.stop() }
            } message: { Text("This interrupts current work. Saved conversation history remains with the agent.") }
    }
}

struct AppCommands: Commands {
    @ObservedObject var store: Store
    @ObservedObject var keys: Keybindings
    @AppStorage("appearance") private var appearance = "system"

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("New Session…") { store.newSessionInCurrentProject() }.bound("session.new", keys).disabled(store.workspace.projects.isEmpty)
            Button("New Specification…") { store.newSpecRequest = UUID() }.bound("project.newSpec", keys).disabled(store.project == nil)
            Button("Open Folder…") { store.addProject() }.bound("project.open", keys)
            Divider()
            Button("Close Tab") { store.closeCurrentTab() }.bound("tab.close", keys)
            Button("Reopen Closed Tab") { store.reopenClosedTab() }.bound("tab.reopen", keys)
        }
        CommandGroup(replacing: .appSettings) {
            Button("Settings…") { store.showSettings.toggle() }.bound("go.settings", keys)
        }
        CommandMenu("Session") {
            Button("Resume Session") { store.resumeCurrentSession() }.bound("session.resume", keys)
            Button("Stop Session…") { store.stopCurrentSession() }.bound("session.stop", keys)
            Button("Sleep Session") { if let id = store.workspace.selectedSessionID, let s = store.session(id), let p = store.project(ofSession: id) { store.sleep(s, in: p) } }.bound("session.sleep", keys)
            Button("Pin / Unpin Session") { if let id = store.workspace.selectedSessionID, let s = store.session(id), let p = store.project(ofSession: id) { store.togglePin(s, in: p) } }.bound("session.pin", keys)
            Divider()
            Button("Edit Name & Notes…") { store.editRequest = store.workspace.selectedSessionID }.bound("session.edit", keys)
            Button("Start Review…") { store.reviewRequest = store.workspace.selectedSessionID }.bound("session.review", keys)
            Button("Send Feedback to Builder…") { store.feedbackRequest = store.workspace.selectedSessionID }.bound("session.feedback", keys)
            Button("Quick Commands…") { store.openPalette(.quick) }.bound("session.quick", keys)
            Divider()
            Button("Switch Terminal…") { store.openPalette(.terminals) }.bound("tab.switch", keys)
            Button("Next Tab") { store.selectTab(offset: 1) }.bound("tab.next", keys)
            Button("Previous Tab") { store.selectTab(offset: -1) }.bound("tab.previous", keys)
            Button("Single Pane") { store.setLayout(1) }.bound("view.layout1", keys)
            Button("Two Panes") { store.setLayout(2) }.bound("view.layout2", keys)
            Button("Four Panes") { store.setLayout(4) }.bound("view.layout4", keys)
            Button("Focus Next Pane") { store.focusPane(offset: 1) }.bound("pane.next", keys)
            Button("Focus Previous Pane") { store.focusPane(offset: -1) }.bound("pane.previous", keys)
            Button("Close Pane") { store.closePane(store.focusedPane) }.bound("pane.close", keys)
            ForEach(1...9, id: \.self) { n in
                Button("Tab \(n)") { store.selectTab(index: n - 1) }.keyboardShortcut(KeyEquivalent(Character("\(n)")))
            }
        }
        CommandMenu("Project") {
            Button("Sessions") { store.areaRequest = "Sessions" }.bound("project.sessions", keys)
            Button("Reviews") { store.areaRequest = "Reviews" }.bound("project.reviews", keys)
            Button("Specs") { store.areaRequest = "Specs" }.bound("project.specs", keys)
            Button("History") { store.areaRequest = "History"; if !store.showProjectPage, let p = store.project { store.selectProject(p.id); store.areaRequest = "History" } }.bound("project.history", keys)
            Button("Tasks") { store.areaRequest = "Tasks"; if !store.showProjectPage, let p = store.project { store.selectProject(p.id); store.areaRequest = "Tasks" } }.bound("project.tasks", keys)
            Button("Docs & Specs") { store.areaRequest = "Docs"; if !store.showProjectPage, let p = store.project { store.selectProject(p.id); store.areaRequest = "Docs" } }.bound("project.docs", keys)
            Button("Next Section") { store.cycleArea(1) }.bound("project.nextArea", keys)
            Button("Previous Section") { store.cycleArea(-1) }.bound("project.previousArea", keys)
            Button("New Task…") { store.requestNewTask() }.bound("project.newTask", keys).disabled(store.project == nil)
            Divider()
            Button("Show in Finder") { if let p = store.project { NSWorkspace.shared.selectFile(nil, inFileViewerRootedAtPath: p.path) } }.bound("project.finder", keys)
            Button("Copy Folder Path") { if let p = store.project { store.copy(p.path) } }.bound("project.copyPath", keys)
            Button("Refresh Projects") { if let p = store.project { store.importSubprojects(p) } }.bound("project.refresh", keys)
            Button("Refresh Git Status") { store.trackGit(); store.git.refreshNow([]); store.refreshGitPanel() }.bound("git.refresh", keys)
            Button("Files & Changes") { store.toggleGitPanel() }.bound("git.panel", keys)
        }
        CommandMenu("Go") {
            Button("Command Palette…") { if store.showPalette { store.showPalette = false } else { store.openPalette(.all) } }.bound("go.palette", keys)
            Button("Search Sessions & Projects…") { store.openPalette(.all) }.bound("go.search", keys)
            Button("Find") { if store.displayedSession != nil && !store.showSettings { store.findRequest = UUID() } else { store.searchFocusRequest = UUID() } }.bound("go.find", keys)
            Button("Agent Dashboard") { store.dashboardRequest = UUID() }.bound("go.dashboard", keys)
            Button("Activity Feed") { store.showActivity.toggle() }.bound("go.activity", keys)
            Button("Home") { store.goHome() }.bound("go.home", keys)
            Divider()
            Button("Toggle Sidebar") { store.toggleSidebar() }.bound("go.sidebar", keys)
            Button("AI Limits") { store.showLimitsRequest = UUID() }.bound("limits.show", keys)
            Button("Refresh Codex Limits") { store.usage.refresh() }.bound("limits.refresh", keys)
            Button("Open Claude /usage") { store.openClaudeUsage() }.bound("limits.claudeUsage", keys)
            Divider()
            Button("Toggle Keep Awake") { store.wake.mode = store.wake.mode == "off" ? "sessions" : "off" }.bound("go.wake", keys)
            Button("Cycle Theme") { appearance = appearance == "system" ? "light" : (appearance == "light" ? "dark" : "system") }.bound("go.theme", keys)
        }
    }
}

/// Reference table shown in Settings.
enum ShortcutReference {
    static let groups: [(String, [(String, String)])] = [
        ("General", [("⌘K", "Command palette"), ("⇧⌘P", "Search sessions & projects"), ("⌘F", "Find project in sidebar"), ("⌘B", "Toggle sidebar"), ("⌘,", "Settings"), ("⌥⌘T", "Cycle theme"), ("⌥⌘K", "Toggle keep awake")]),
        ("Sessions & tabs", [("⌘N", "New session"), ("⌘E", "Switch terminal (recent first)"), ("⌃Tab / ⌃⇧Tab", "Next / previous tab"), ("⌘1–9", "Jump to tab"), ("⌘W", "Close tab"), ("⇧⌘T", "Reopen closed tab"), ("⇧⌘R", "Resume session"), ("⌘.", "Stop session"), ("⌘I", "Edit name & notes"), ("⌥⌘R", "Start review"), ("⇧⌘B", "Send feedback to builder")]),
        ("Project", [("⌘O", "Open folder"), ("⌥⌘1 / 2 / 3", "Sessions / Reviews / Specs"), ("⌥⌘N", "New specification"), ("⌥⌘F", "Show in Finder"), ("⌥⌘C", "Copy folder path"), ("⌃⌘R", "Refresh projects"), ("⌥⌘G", "Refresh git status"), ("⇧⌘G", "Files & Changes panel")]),
        ("Limits", [("⇧⌘L", "AI Limits panel"), ("⌥⌘L", "Refresh Codex limits"), ("⌥⌘U", "Open Claude /usage")]),
    ]
}

struct WorktreeSetupSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State var project: Project
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Worktree setup commands").font(.title2.bold())
            Text("Run in a new worktree of \(project.name) before the agent starts, after the global commands from Settings → Git. One command per line, e.g. `pnpm install` or `cp ../.env .env`.")
                .font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            TextEditor(text: Binding(get: { project.setupCommands ?? "" }, set: { project.setupCommands = $0.isEmpty ? nil : $0 }))
                .font(.system(size: 12, design: .monospaced)).frame(height: 140).border(.quaternary)
            HStack { Spacer(); Button("Cancel") { dismiss() }; Button("Save") { store.updateProject(project); dismiss() }.buttonStyle(.borderedProminent) }
        }.padding(24).frame(width: 520)
    }
}

struct ProjectWorkflowSheet: View {
    @EnvironmentObject var store: Store
    @Environment(\.dismiss) private var dismiss
    @State var project: Project
    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Review & task defaults · \(project.name)").font(.title2.bold())
            Text("Review brief template (leave empty to use the global one from Settings → Agents). Placeholders: {{title}} {{path}} {{scope}} {{task}} {{notes}} {{spec}} {{output}} {{branch}}").font(.caption).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            TextEditor(text: Binding(get: { project.reviewTemplate ?? "" }, set: { project.reviewTemplate = $0.isEmpty ? nil : $0 }))
                .font(.system(size: 12, design: .monospaced)).frame(height: 180).border(.quaternary)
            HStack(spacing: 14) {
                Picker("Tasks finish with", selection: Binding(get: { project.taskMode ?? "pr" }, set: { project.taskMode = $0 })) {
                    Text("Pull request").tag("pr"); Text("Push to main").tag("push"); Text("Commit only").tag("none")
                }.frame(width: 260)
                Toggle("Auto-run task queue", isOn: Binding(get: { project.autoRunTasks == true }, set: { project.autoRunTasks = $0 }))
            }
            HStack { Spacer(); Button("Cancel") { dismiss() }; Button("Save") { store.updateProject(project); dismiss() }.buttonStyle(.borderedProminent) }
        }.padding(24).frame(width: 620)
    }
}
