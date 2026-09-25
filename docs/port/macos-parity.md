# Retiring the Swift app: parity checklist

September 25, 2026 · compared against Convoy 0.6.4

The goal is one codebase: the Rust core (`apps/linux/crates/convoy-core`) plus the
Tauri client (`apps/convoy`) on macOS, Windows and Linux. The Swift app
(`Sources/Convoy`) keeps shipping until every **P0** and **P1** row below is done.
The GTK client (`apps/linux/crates/convoy-gtk`) retires alongside it: Tauri already
covers Linux.

Status: **MISSING**, **PARTIAL**, or **done** (kept only where a row was fixed
after the survey). Swift file and symbol names are given so each row can be
checked against the original.

## Moving the data across

Done. `convoy_core::macos_import` reads `~/Library/Application Support/Convoy`
(workspace, activity, saved terminal output, icons) and the app's UserDefaults.
It merges them into the Rust workspace. The Tauri client offers the import once,
when its workspace is still empty, and keeps it under Settings → Setup for later
runs. Running it again adds only what is new; a project already here keeps its
own settings.

- **Brought across:** projects, with a parent's name as the group label; sessions, marked started when the provider still has the conversation; specs; tasks, with their Linear/Jira source; quick commands; activity; settings; shortcuts; Linear and Jira connections.
- **Left behind:**
  - Tracker keys stay in the macOS Keychain, so each connection asks for its key once.
  - Claude login sessions have no conversation to resume.
  - Window, pane and panel layout.
- On this Mac the dry run brought 62 projects, 40 sessions and 11 tasks, and the workspace validated.

## Before the switch (not features)

| Item | Status | Note |
|---|---|---|
| Storage location on macOS | MISSING | Tauri keeps its data in `~/.config/Convoy Desktop Preview/`. Move it to `~/Library/Application Support/Convoy Tauri/` (or similar), and never write into the Swift folder while both apps exist. |
| Signed, notarized macOS build in `convoy-release.yml` | MISSING | Today only Windows and Linux are built there. `Convoy.zip` is still the Swift app. |
| Bundle identity | MISSING | Swift uses `com.iftech.convoy`. Tauri should take it over at the switch so notification permission and the Dock entry carry over. |
| Update path for existing Swift users | MISSING | The last Swift release should point at the Tauri download, and the Tauri app should run the import on first launch (already built). |
| Quit keeps agents running when the window closes | PARTIAL | Swift asks only on Quit. Tauri asks to stop everything when the window closes (`lib.rs` `on_window_event`). On macOS, closing the window should not quit. |

## P0 — the daily loop

All done on September 25, 2026. Each row is covered by `apps/convoy/test/flows.mjs`, except where the note says otherwise.

| Swift capability | Status | Note |
|---|---|---|
| New Session sheet starts the agent at once (`NewSessionSheet`) | done | Any project, optional name (auto-titled), worktree from any branch/remote/tag with a branch that follows the name, account, note, run/skip setup, keep open. |
| Find in terminal ⌘F: next/previous, match case, "n of m" (`TerminalSearchBar`) | done | xterm's search add-on. With no terminal on screen, ⌘F still finds a project. |
| Switch terminal, recent first ⌘E (`Store.recentTabs`) | done | Pressing ⌘E again moves down the list. |
| Reopen closed tab ⇧⌘T (`Store.reopenClosedTab`) | done | Tabs closed this run. |
| Drop files onto the terminal to paste quoted paths | done | Through Tauri's native drop event. The test drives the handler; a real drag has not been tried on each platform. On Windows, Tauri's native drop can stop HTML5 drag inside the page, so check tab reordering there. |
| Pick an account for a new session; log in from a terminal (Settings → Accounts) | done | "Use" is remembered per machine. "Log in…" runs `claude /login` or `codex login` in the account's home. Not yet tried against the real CLIs. |
| Pinned sessions from every project in the sidebar (`Store.pinnedSessions`) | done | |
| Sessions under every project in the sidebar, collapsed state kept | done | |
| Command palette over all projects' sessions and about 40 actions, fuzzy match | done | The macOS ranking, with shortcuts shown. |
| Clicking a notification opens its session (`Notifier.onOpen`) | done, differently | Tauri's desktop notifications have no click callback. Coming back to the window while the notified session still waits opens it; clicking the notification does exactly that. |
| Dock badge with the count of sessions waiting | done | macOS and Linux. Windows has no badge; an overlay icon would be the equivalent. |

## P1 — used every week

All done on September 25, 2026, except the side panel, which was declined. Each row is covered by `apps/convoy/test/flows.mjs` or a Rust test.

| Swift capability | Status | Note |
|---|---|---|
| Home page: Needs you, Running, Recent, task counts, limits, activity, projects | done | The window opens on it; ⇧⌘H. A project's own page is a click on the project. |
| Agent Dashboard ⌥⌘D: every session by state | done | A page rather than a second window. |
| Codex session ID captured from `codex resume <uuid>` output | done | Read from the output by the backend. |
| Saved output shown in place when the session is stopped | done | The page now saves each running terminal's rendered text; before this, the Tauri client never wrote history. |
| "Conversation not found → Start fresh" banner | done | |
| Feedback to the builder, filled with the reviewer's output | done | |
| Files & Changes as a side panel beside the terminal ⇧⌘G | declined | Files & Changes stays a full page by choice. |
| Repo picker for group folders; ahead/behind counts | done | Here each repository in a group is its own project, so the picker switches between the session's worktree, its project and the group's other projects. |
| Commit & Push; unstage all; mark resolved; reveal, open, copy path | done | |
| Tasks: drag between board columns | done | Onto Running runs the task. |
| Tasks: PR URL detected → `pr` status, "Open pull request" | done | New `pr` status in the core; the macOS importer keeps it. |
| Tasks: delete; set any status; open reviewer; Run now in the form | done | |
| Tasks: list grouped by status, hide done, all projects | done | |
| Default task mode `pr` for new projects | done | |
| Project order: drag, Move up/down | done | Within a group. Sorting by name still wins when on. |
| Refresh projects / import subprojects ⌃⌘R | done | Bound to ⌥⇧⌘R: ⌃⌘R is Ctrl+R on Linux and Windows, which terminals need. |
| Transcript history panel: every saved conversation, one-click Resume | done | A History tab: project folder and worktrees, both agents, every account. |
| Setup checks: gh auth, Claude/Codex logged in, hooks, support folder | done | For the accounts new sessions use. |

## P2 — polish

| Swift capability | Status | Note |
|---|---|---|
| Project hierarchy (parent/child, "Show group hierarchy") | PARTIAL | `Project.group` is a heading label. |
| ⌘-click multi-select of sessions: archive, close tabs | MISSING | |
| Move tab left/right shortcuts ⇧⌘←/→ | MISSING | In the tab menu only. |
| Panes: Maximize, per-pane quick commands, suggestions in an empty pane, sessions from other projects | PARTIAL | |
| Quick commands: edit in place, palette mode ⌘/ | PARTIAL | Add and remove only. |
| Status-bar account switcher per agent | MISSING | |
| Open Claude /usage ⌥⌘U; search sessions & projects ⇧⌘P | MISSING | Home ⇧⌘H is done. |
| Spec WorkTasks (builder/reviewer pairs inside a spec) | MISSING | The import turns them into ordinary tasks linked to the spec. |
| Task dependencies "Blocked by" | MISSING | The import keeps no dependency. |
| Spec search box | MISSING | |
| Diff: unified/split and wrap toggles, load all, rendered Markdown | PARTIAL | |
| Log: copy SHA or subject | MISSING | |
| Branch from a chosen base | PARTIAL | Name only. |
| Review template "Insert default" (project and global) | MISSING | |
| Shortcuts: reset one, unbind, show conflicts | PARTIAL | The core rejects duplicates. |
| Keep-awake toggle target | PARTIAL | Swift toggles to "while running", Tauri to "always". |
| Workspace options menu in the sidebar | MISSING | They are in Settings. |

## Shortcuts still to add

`tab.moveLeft`/`tab.moveRight` ⇧⌘←/→, `go.search` ⇧⌘P and `limits.claudeUsage` ⌥⌘U. The import leaves out an override of
any of these until the action exists here.

Layouts differ on purpose: Tauri uses `ctrl+shift+1/2/4`, because on Linux and Windows
`ctrl+alt+digit` and `mod+alt+digit` are the same keys. Swift uses `ctrl+1/2/4`.

## Already at parity

Projects with icons and colors, and project settings. Sessions in a real PTY with
resume, pin, archive, sleep and recovery. Tabs with drag reorder. 1/2/4 panes.
Reviews. Worktrees with setup and shared files. Files & Changes: stage, hunks,
commit with a generated message, push, pull, PR, log. Docs and specs from
`.specdesk/`. Tasks with the auto-run queue and auto-review. Linear/Jira import.
Hook-driven agent state. Notifications. AI limits. Keep awake. Folder trust.
Activity. Per-terminal zoom. The Settings page. Most shortcuts.
