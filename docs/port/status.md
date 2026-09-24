# Port and native review — September 23, 2026

> **September 24, 2026.** Linux moved off Electron twice in one day. First to
> `apps/linux`, a native GTK4 client in Rust; then, once the design was seen
> beside the macOS app, to `apps/convoy` — a Tauri client that now covers both
> Windows and Linux with one interface. `apps/linux` still builds and still
> passes its tests; `apps/desktop` is the Electron build it replaced. All of
> them read the same schema-3 `workspace.json`, so any one can be run against
> existing data — but not two at the same time. See
> [the Tauri client](#convoy-tauri-client) and
> [the Linux client](#linux-gtk-client) below, and
> [`docs/plan-refactor/`](../plan-refactor/) for why and how.

Reference: native macOS 0.5.8 (`2732d28`). Its Files & Changes source changes were
applied to this worktree while retaining the Electron implementation. Main 0.5.9
(`ee04717`) was subsequently merged, preserving its newer History and optional
session-name behavior. No release, model request, or real task publication was
performed during validation.

## Implemented in the Electron preview

Projects/discovery/group labels/reconnect/removal; provider conversation history;
sessions/resume/recovery/models/accounts; retained terminals/split view;
reviews/feedback; Git worktrees/setup/shared-file copies; Files & Changes with
diffs/staging/commits/hunk discard/branches/remotes/PR creation/files/history;
approved specs/export/tasks; explicit task queues and PR/push modes; automatic
review handoff; Claude hooks/usage/attention state; Codex quota reads; hibernation;
keep-awake; notifications/activity; command palette/custom shortcuts.

The UI deliberately differs from SwiftUI: a Git dialog, group labels rather than
nested sidebar trees, two terminal panes, and basic Markdown rendering. There is
no WSL backend, surviving PTY attachment after restart, or repeating autonomous
review/fix loop. Provider permissions stay enabled. See the
[desktop guide](../../apps/desktop/README.md) for behavior and storage boundaries.

## Native review fixes

- Clearing a file/diff selection invalidates outstanding asynchronous reads.
- Queued commits capture their Amend setting and retain newer commit drafts.
- Git calls clear inherited repository override environment variables and treat
  filenames as literal pathspecs.
- Large stdin writes run concurrently with stdout draining and timeout handling.
- File reads are bounded even if a file grows after its size check; large images
  obey the preview limit too.
- Staging can be undone before a repository's first commit.
- Narrow split-diff fallback text wraps and stays readable.

## Validation

- Native macOS: merged 0.5.9 suite passed with 52 tests; Git panel and terminal snapshot fixtures
  also passed in the preceding full UI-enabled run.
- JavaScript: 34 passed, one Windows-only legacy PowerShell check skipped locally.
- macOS Electron: renderer, real IPC/app lifecycle, explicit two-task queue and
  PTY input/output/exit smoke tests passed with harmless shell fixtures.
- Linux ARM64 (Debian Bookworm, Node 22, Electron 44.4.5): unit, renderer,
  application lifecycle and real PTY checks passed in a disposable Docker/Xvfb
  environment. The root container required `--no-sandbox`; this does not validate
  desktop sandbox installation or native notification delivery.
- Screenshots inspected for the native Files & Changes panel and Electron split
  diff; the preview uses bounded data and isolated temporary repositories.
- Windows x64 and Ubuntu x64 CI jobs are configured but have not run remotely.
  Native Windows/ConPTY behavior, installed-provider integration, notification
  delivery, signing and updates remain release validation.

Tests do not access production workspace files or send actual model requests.

## Linux GTK client

`apps/linux` replaces the Electron preview on Linux with `convoy-core` (every
rule, no toolkit dependency) and `convoy-gtk` (GTK4, libadwaita, VTE,
GtkSourceView). Feature parity with the Electron preview, minus Windows.

### What was actually run

- **Core**: 79 tests, no display. All 32 non-Windows tests from the JavaScript
  suite were ported; the four Windows PowerShell and npm-resolver cases were
  dropped with the platform. The rest are new, covering rules the Electron
  build had beside its IPC handlers and therefore never tested: the task queue,
  shortcut conversion, agent-state monitoring, worktree ownership and the
  side-by-side diff.
- **Window**: 70 headless checks under `xvfb-run`, driving a real repository
  and a real workspace file. They assert what the window shows and does, not
  what it was told.
- **Terminal**: 12 checks against VTE, including launching the installed
  Claude Code 2.1.273 and Codex 0.156.0 through the production launch path.
  Both render their interfaces correctly; emoji and CJK occupy two cells.
- **Start-up**: `scripts/smoke.sh` starts the app headlessly on a seeded
  workspace and requires a clean exit.
- **Metadata**: `desktop-file-validate` and `appstreamcli validate` both pass.

### Two corrections the terminal work forced

- Reading the buffer back must use `text_format`, not `text_range_format`.
  Agents draw with absolute cursor positioning, and a row range returned zero
  characters for a full screen of Codex output. The original plan specified the
  wrong call; a review brief would have been empty.
- `vte_terminal_watch_child` asserts if the child is already gone, which
  happens when a command fails instantly — a missing CLI exits 127. Without a
  fallback there is no exit code at all and the task stays `building`. A GLib
  child watch is registered instead when VTE has already dropped the pty.

### Not yet validated

**Nobody has used it.** Every check above is automated and headless. The window
has not been opened on a real display, no agent session has been started by
hand, and the client has never run against the real workspace file. That is the
largest remaining gap, and no amount of further automation closes it.

Mouse reporting and redraw under sustained output are not covered by the
terminal checks; `convoy-vte-probe` exists for them but has not been run
interactively. No distribution package has been installed from a clean system;
the PKGBUILD and Debian metadata are written but only CI builds them. Signing,
updates and a real release remain release work, as before.

## Convoy (Tauri) client

`apps/convoy` is the client for **Windows and Linux**, following the macOS
SwiftUI app as its design. macOS keeps its native app and is not touched.
`convoy-core` moved across from the GTK client unchanged — that was the point
of writing it with no toolkit dependency — and only the front end is new.

Feature parity with the Electron preview: projects and discovery, sessions with
resume, recovery, models and accounts, reviews and builder feedback, worktrees
with shared files and setup commands, Files & Changes with four views,
side-by-side diffs, staging, hunk discard, commits, branches, remotes and pull
requests, specifications with approval and export, tasks as a list and a board
with a queue and publish modes, transcript import, usage limits, the agent
monitor, hibernation, keep-awake, notifications, the activity log and a command
palette.

### What was actually run

- **Rust**: `cargo fmt`, `cargo clippy --all-targets -D warnings` and
  `cargo test` clean on Linux, and `fmt` and `clippy` clean on `windows-2022`
  in CI — which is the first time any Windows branch in this tree has been
  compiled rather than read.
- **Terminal**: five checks on both platforms, in `convoy-pty` — a command
  runs, input reaches it, the exit code comes back, writing to a session that
  is not running says so, stopping reports that it was asked for, hibernating
  is reported apart from a stop, and on Unix the whole process tree goes. The
  terminal is a separate package precisely so these run on Windows: a test
  binary that links the web view dies at load there.
- **Exit rules**: a task whose session exits cleanly is asserted to be in
  review afterwards, by reading the workspace file back. The assertion was
  checked against a deliberately broken build first, because a test of wiring
  that passes when the wiring is absent proves nothing — it failed, as it
  should.
- **Screens**: sessions, the workbench, the session menu, tasks as a list and
  as a board, specifications, activity, Files & Changes in all four views with
  a unified and a side-by-side diff, settings, the new-session and new-task
  dialogs, the command palette, and both themes — each opened under Xvfb and
  looked at.
- **An agent**: Claude Code started from the session list, drew its interface
  in the window, took keyboard input and exited; the exit code arrived and the
  session returned to stopped.
- **Core**: 87 tests. The ones that cannot mean anything on Windows — a login
  shell, Unix permission bits, symlinks — now say so with `cfg(unix)` rather
  than failing there. The Windows launch cases are covered by
  `tests/windows.rs`, which runs from either platform because the platform is
  a parameter rather than a `cfg!`.

### Not yet validated

**Nobody has used it on Windows.** CI compiles it, lints it, runs its tests and
builds an installer, but no one has installed that installer and started an
agent. Provider resolution against a real `claude.exe`, ConPTY rendering of an
agent's interface, notification delivery and the Recycle Bin all remain
unproven there.

Input latency on Linux — the one open question the design pivot named — is
still open. It needs somebody typing at a running agent, and no automation
closes it.

Signing, updates and a real release remain release work.

## Local Linux artifact

Built `apps/desktop/release/linux-validation/Convoy Preview-0.1.0-arm64.AppImage`
(122 MB, ignored build output). Extraction and the unpacked telemetry/PTY payload
were verified using Debian Bookworm. The minimal Node image lacked the runtime's
`libz.so`; extraction succeeded in the full Debian image. Validate runtime library
availability and FUSE support on the intended distribution before shipping.
This is ARM64, not the Windows/Linux x64 CI artifact.

SHA-256: `6f500c765537d697f7896e627ec85d64d1834098b951465ec861212b5dfc55bb`.
