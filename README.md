# Convoy

Convoy is a macOS workspace for Claude Code and Codex sessions. Sessions and cross-agent reviews come first; Spec Driven Development is optional.

Requires macOS 14+, Swift 6 to build, and your existing Claude Code / Codex CLI installation and login. Embedded terminals use SwiftTerm 1.20.0 (MIT).

## Build and open

```sh
bash scripts/build-app.sh
bash scripts/install.sh   # or: open dist/Convoy.app
```

The first build fetches SwiftTerm and its package dependencies. The script packages its resource bundle in the macOS Resources directory and adjusts SwiftPM's generated release resource accessor accordingly. The app is ad-hoc signed for local use, not notarized for distribution.

## Signing for distribution

Local builds are ad-hoc signed and need `xattr -dr com.apple.quarantine` (or the landing's one-line installer) on other Macs. For a build Gatekeeper accepts:

```sh
xcrun notarytool store-credentials convoy --apple-id you@example.com --team-id TEAMID   # once; uses an app-specific password
SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" NOTARY_PROFILE=convoy bash scripts/install.sh
```

This needs a paid Apple Developer Program membership and a Developer ID Application certificate (Xcode → Settings → Accounts → Manage Certificates).

## Start working

1. Choose **Open folder…**. Select a single repository or a parent such as `Projects` or `olucha-cargo`.
2. Parent folders containing recognizable projects become expandable groups. Click the group name itself to work across that folder; click a child for a single-project session.
3. Click **New session**, choose Claude Code or Codex, name it, and optionally add a first message.
4. Click **Start**. The agent runs in an embedded terminal at the selected folder. Type directly into it, including responding to the agent's trust and permission prompts.
5. Switch between projects/sessions without stopping their terminals. Right-click sessions to rename or archive them. Archive is disabled while a session runs.

Existing folders imported by the first prototype can be converted using **⋯ → Import projects from this folder**. Re-import preserves specs, sessions and IDs. Discovery recognizes Git and common project manifests, stops at a project boundary, skips hidden folders/dependencies/build output, and never follows directory symlinks. A repository containing nested projects can be explicitly scanned with the same context action. A bounded scan reports an error rather than silently dropping work when a tree is too large or unreadable. Non-project folders without recognized descendants are omitted.

Every project and group has a visible **⋯** menu: **New session…**, import/refresh projects, Show in Finder, Copy folder path, Reconnect folder, and Remove. New session from this menu always targets that row's folder. The same actions remain available on right-click. Session cards also expose a **⋯** menu for editing, reviewing and archiving.

Groups are real workspaces with their own sessions and optional specs, not just visual labels. A session in `olucha-cargo` runs there, with its child repositories beneath the agent's working directory. This does not automatically inject every file into the model's context; the agent discovers relevant files as it does in your terminal.

## Review handoff

Select a coding session and click **Start review**. The other agent is selected by default. Inspect/edit the brief, which includes the original task, notes and recent terminal output, then launch the reviewer. Review sessions are linked to their source and appear under **Reviews**.

**Send feedback to builder** captures recent reviewer output in an editable dialog, then inserts your edited feedback into the running builder terminal. Press Enter in that terminal to send it. There is no need to create a spec.

Let the builder finish editing before starting a review. Reviews use the same workspace, not an immutable Git snapshot. The prompt asks reviewers not to edit, but their existing CLI permissions still apply. Automatically repeating fix/review rounds, structured findings, revision pinning, and automatic acceptance are not implemented yet.

## Session lifetime and recovery

- Switching views or closing the window keeps agents running while Convoy remains open.
- Quitting warns about running sessions and stops them if confirmed.
- Names, notes, agent IDs, associations and recent text snapshots are stored locally and restored on relaunch.
- Claude gets a stable session UUID at launch; **Resume** uses that ID. The provider must have persisted a conversation for it to be resumable.
- Codex's ID is captured when it prints `codex resume <UUID>` on exit. If no ID was captured (for example after forced termination), **Open resume picker** runs Codex's picker at the same directory. **Edit** allows saving the exact ID manually. The app never guesses with `--last`.
- A saved terminal snapshot is recent output, not a complete provider transcript or a live process.
- Missing executables, authentication problems, and CLI errors appear in the terminal. A process running does not imply its agent is ready or has finished a task.

## Permissions and storage

As requested for embedded terminals, this version uses normal macOS user permissions rather than Apple App Sandbox. It does **not** require Full Disk Access, Accessibility, Automation, or screen recording. Agents retain their own permission settings; the app adds no permission-bypass flags. The selected working directory is not an OS-enforced filesystem boundary. A login shell loads your normal shell setup to find tools installed through NVM, Homebrew, or local installers.

State: `~/Library/Application Support/Convoy/workspace.json`.
Recent terminal snapshots: `~/Library/Application Support/Convoy/TerminalHistory/`.
On first launch, existing records from this app's earlier sandbox container are copied to the new location if no new workspace exists. The original file remains untouched. Unsupported schemas and corrupt data block saving rather than overwrite the file.

Right-click a project/group to remove its app records after confirmation. Files and provider history are untouched. Removing a group retains its children. Stop its own running sessions before removal. Export optional specs before removing their workspace if you want to keep them.

## Optional specs

The **Specs** tab retains requirements, acceptance criteria, approval revisions, task assignments, findings and Markdown export. Changes to an approved spec invalidate its approval and mark completed tasks for reassessment. Specs are currently stored in the workspace; Markdown export is a snapshot. Repository synchronization/import is not implemented.

## Tests

```sh
env CLANG_MODULE_CACHE_PATH="$PWD/.build/ModuleCache" \
  SWIFTPM_MODULECACHE_OVERRIDE="$PWD/.build/ModuleCache" \
  swift test --disable-sandbox --cache-path .build/cache \
  --config-path .build/config --security-path .build/security
```

To include the native-window and real PTY input/output test (uses a harmless shell, no model requests), also set `SPECDESK_UI_TEST_DIR="$PWD/.build/qa"`. This writes test-window snapshots and uses isolated test workspace data. Native tests require a normal macOS GUI session.

Tests cover persistence, schema migration, folder discovery, duplicate imports, group removal, shell argument escaping, review context, and terminal input/output. Full end-to-end model reviews are not exercised by tests.

References: [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm), [Codex CLI](https://learn.chatgpt.com/docs/developer-commands?surface=cli), [Claude Code CLI](https://code.claude.com/docs/en/cli-reference).

## Tabs, agent status and notifications

Every open session is a tab across the top, regardless of project; the sidebar lists sessions under their project. ⌘K opens a command palette (sessions, projects, actions), ⌘E a most-recent-first terminal switcher, ⌃Tab cycles tabs and ⌘1–9 jump. Settings → Shortcuts lists everything.

Claude sessions report working / waiting / done through Claude Code hooks that Convoy passes per session with `--settings`; your global `~/.claude/settings.json` is not modified and nothing is inferred from terminal output. Tabs and sidebar rows show an amber “?” when an agent needs input or permission and a green check when it finished. macOS notifications fire for sessions you are not looking at, the Dock badge counts sessions waiting on you, and the status bar links to the first. Codex has no hook API, so it shows running / stopped only.

## Git worktrees and branches

**New session → Run in a new git worktree** creates an isolated checkout under `~/Library/Application Support/Convoy/worktrees/<project>/<branch>` with `git worktree add --no-track -b <branch>`, so parallel sessions on one repo never overwrite each other. The branch name is generated from the session title (optional prefix in Settings) and can be edited. Reviews of a worktree session run in the same worktree. Right-click the session to show, copy or remove the worktree (optionally deleting the branch). The sidebar and session header show the current branch, changed-file count and ahead/behind, read with optional locks disabled so polling never races an agent's own git commands.

## Review, split view, quick commands

The **±** button in a session header (⇧⌘G) opens a Changes panel: every file changed in the session's directory versus its worktree base (merge-base) or HEAD, including untracked files, with a unified diff. Click **+** on a line to leave a note; **Send to agent** inserts all notes into that session's terminal as one message. **Split** (⌘D) shows another terminal beside the current one; ⇧⌘D closes it. **⚡ Quick commands** (⌘/) are saved prompts or shell commands, global or per project, inserted with bracketed paste and optionally submitted. The bell (⇧⌘A) opens the Activity feed of completions, questions, sleeps and worktree events.

## Sidebar extras, hibernation, accounts

Right-click a session to pin it to the top, put it to sleep, open it in a split, or manage its worktree. ⌘-click selects several sessions for bulk archive or tab close. Right-click a project for icon and color (emoji or GitHub avatar), worktree setup commands, and move up/down; root projects also drag to reorder. Settings → Agents → Hibernation stops Claude agents that finished and sat idle; opening a sleeping session resumes it. Settings → Accounts creates isolated Claude (`CLAUDE_CONFIG_DIR`) or Codex (`CODEX_HOME`) logins; the status bar chip hot-swaps which one new sessions use, without touching your system login.

## Settings

⌘, opens an in-window settings page with search: General, Appearance, Terminal (font size, scrollback), Agents (status hooks, hibernation, usage status line), Accounts, Quick Commands, Git (polling, worktree setup hooks), Notifications, AI Limits and Shortcuts (every binding remappable with a key recorder).

## Workspace controls and AI Limits

The sidebar includes search and a Workspace options menu for group hierarchy, alphabetical sorting and compact rows. Each project/group and session has a visible three-dot actions menu. The bottom bar shows running sessions, AI Limits, Keep awake, and System/Light/Dark appearance. Appearance is also available directly in the macOS menu bar; the choice persists across launches. The blue accent adapts to light and dark surfaces.

**AI Limits** reads actual Codex account windows through the installed CLI's supported `account/rateLimits/read` endpoint. It uses the CLI's existing login, not copied tokens or cookies, and sends no model request. The panel displays usage percentages, reset dates, refresh errors and last-updated time. Data older than five minutes is marked cached in the status bar. Refresh happens when the panel first opens or when requested, not on a background polling loop. Some API-authenticated accounts do not expose subscription windows.

Claude's **Claude limits integration** switch enables a session-scoped status-line helper for newly started or resumed sessions. It leaves global settings unchanged, but replaces a custom status line for that launch. Claude Code 2.1.251+ reports subscription limits after a response; the app stores only quota windows, source session name and update time, never the full status-line payload. Missing or expired windows are not shown as zero. The native panel reads the latest reporting session every five seconds; percentages are snapshots, not continuous account polling. **Open Claude usage** provides the supported `/usage` command in a dedicated embedded session for an on-demand check. Integration is optional and off initially. See [Claude status-line data](https://code.claude.com/docs/en/statusline).

**Keep awake** defaults to Off. Options are Always while Convoy is open, While a session is running, and Off. It prevents idle system sleep, not explicit sleep or lid closure. A running CLI may be idle at its input prompt; this is process-based, not inferred agent activity.

Read-only live Codex integration test: add `SPECDESK_LIVE_LIMITS_TEST=1` to the test command. This requires an installed, signed-in Codex CLI and network access; it reads account quotas without making a model request.

Opening a session collapses the project and session sidebars and replaces the workspace header with a 40-point terminal toolbar. Use the project menu to reach Reviews or Specs, the session title to switch conversations, the list button to browse sessions, and the three-dot menu for review, edit, archive and stop actions. The native sidebar button restores projects. Agent trust/permission prompts remain in the terminal.

Saved terminal output is labelled read-only. If Claude reports that a conversation ID does not exist (for example, a launch ended at the trust screen before any messages), Convoy offers **Start fresh** in the same workspace. This creates a new session record and preserves the old one; it does not bypass the agent's trust prompt or silently replay the original task. Snapshots preserve blank terminal cells as spaces and skip continuation cells for wide characters.

### Session reliability

Resume always asks the provider to resume the recorded conversation; it never silently replays the first prompt when a transcript lookup fails. Missing Claude conversations offer explicit fresh recovery, preserving the worktree, branch, review relationship and account configuration. Sessions bind to their provider configuration home on their next launch; older records without that information use the selected account once. Keep the original account selected when resuming a legacy record for the first time.

Stopping a terminal terminates its PTY process group and reaps the child, with a bounded fallback for an unresponsive process. Switching views retains the running terminal. Saved snapshots preserve spaces and join soft-wrapped lines. Review feedback switches focus to the original builder after insertion, without submitting it. Removed sessions are excluded from tab navigation; bulk closing does not remove running sessions from their panes.
