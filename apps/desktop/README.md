# Convoy desktop preview

Windows/Linux implementation in the same repository as the native macOS app.
The SwiftUI product stays in `Sources/`; this app uses Electron, sandboxed IPC,
xterm.js, and native PTYs. The feature reference is native Convoy 0.5.8.

## Included workflows

- Projects: open folders, discover nested projects while skipping dependencies,
  group labels, names/icons, reconnect moved folders, remove saved project records.
- Sessions: Claude/Codex launch and exact conversation resume, model overrides,
  pin/archive/search, editable notes, fresh recovery, bounded saved output, and
  import of provider conversations for the selected folder/account.
- Reviews: editable cross-agent briefs, shared worktrees, feedback inserted into
  the original builder without submitting, and optional task review handoff.
- Files & Changes: staged/unstaged/untracked status, unified/split diffs, line
  numbers, staging, commit/amend, Claude commit-message generation, file/hunk
  discard, Trash, branches, fetch/pull/push, and `gh` pull-request creation.
  Files has bounded text, basic Markdown and image previews; Log has commit
  diffs, revert and soft/mixed reset. Destructive actions ask before applying.
  Hunk discard rejects stale diffs. Refresh explicitly after external edits.
- Specs/tasks: revisioned approval, Markdown export, prepared briefs, stale-brief
  rejection, explicit queue execution, no-publish/PR/push modes and review handoff.
  PR queue tasks create separate worktrees. No-publish is the default. Queue failures
  pause execution; restart never silently resumes a queue. Claude Stop events or a
  successful CLI exit move tasks to review, **not done**. Acceptance stays explicit.
- Worktrees: create isolated branches, optional shared-file copies/setup commands,
  clean-only removal, linked-review preservation. Setup shows its command and asks
  before execution. Shared files never overwrite tracked files. Branches are retained
  after removal; removed session folders cannot silently fall back to the project.
- Accounts: isolated provider homes, session binding, removal of unused profile
  records. Credential files are retained. CLI sign-in and permission prompts remain
  in the terminal; Convoy does not copy tokens or disable provider permissions.
- Workspace: retained two-pane terminals, quick commands with explicit optional
  submit, theme/font/scrollback, custom shortcuts, command palette, activity,
  attention/completion notifications, keep-awake and Claude idle hibernation.

## Provider integration

Install current Claude Code/Codex CLIs and sign in through their normal interface.
Linux runs a Bash login shell. Windows resolves native executables or standard npm
package entry scripts directly, preserving prompt/model arguments without `.cmd`
shell interpolation. Restart Convoy after changing PATH. WSL is not a backend.
First messages are passed only on first launch; resume never replays them.

Claude status uses documented per-session hooks. Optional **Claude usage status
line** is off by default and takes effect on the next launch. It replaces that
launch's custom status line, leaves global settings unchanged, and stores only
validated quota windows. The installed Claude version must support exec-form hooks;
subscription rate-limit status-line fields require Claude Code 2.1.251+.
Codex usage invokes only `initialize` and `account/rateLimits/read` through the
installed CLI, using the selected session's bound account. No model call is made.
Missing/expired quotas are unavailable, never guessed as zero. **Generate with
Claude** and starting task/review sessions do make provider requests.

Hibernation stops Claude only after an explicit done event and the configured idle
period. Resume remains explicit. Codex shows process running/stopped; terminal text
is not used to guess whether an agent has finished. Notifications need OS support
and permissions. Keep-awake prevents idle suspension, not explicit sleep/lid closure.

References: [Codex App Server](https://learn.chatgpt.com/docs/app-server),
[Claude hooks](https://code.claude.com/docs/en/hooks),
[Claude status line](https://code.claude.com/docs/en/statusline).

## Develop and verify

Use Node.js 22, npm, Python 3 and a C++ build toolchain. Windows needs Visual Studio
Build Tools with the C++ workload; Linux needs make/g++ and X11 development packages.
`postinstall` rebuilds node-pty for the installed Electron version.

```sh
cd apps/desktop
npm ci
npm start
npm test
npm run test:pty
npm run test:ui
npm run test:app
npm run package
```

Headless Linux GUI checks run with `xvfb-run -a`. Build installers on their target
OS. CI builds x64 Windows NSIS and Linux AppImage/deb artifacts in `release/` and
runs unit, renderer, PTY and application-lifecycle checks using harmless shell
fixtures. Tests never send a model request or publish a branch/PR.

This remains a preview: target Windows desktop/provider validation, signing and
updates are release work. The Electron chrome follows the native app's design
(tab strip, breadcrumb session header, sidebar tree, sheets, palette, status bar
and the shared colour tokens), but Git, Specs & tasks and History still open as
sheets rather than embedded panels, Markdown is basic, and splits have two panes. Native macOS release scripts remain independent. See
[port validation](../../docs/port/status.md) for what was actually run.

## Storage and boundaries

Preview data is separate from the native Swift app:

- Windows: `%APPDATA%/Convoy Desktop Preview/`
- Linux: `${XDG_CONFIG_HOME:-~/.config}/Convoy Desktop Preview/`
- macOS development: `~/Library/Application Support/Convoy Desktop Preview/`

`workspace.json` is replaced atomically; corrupt/future schema files block startup
without overwriting them. Older preview schemas migrate additively to schema 3.
Do not point Swift and Electron at the same workspace file. Provider IDs and account
homes are preserved; imported conversations are saved but not automatically launched.
Output excerpts in `TerminalHistory/` are bounded plain text, not complete transcripts
or live screen snapshots. They can contain anything printed by the CLI.

The renderer cannot access Node, navigate away, or open remote windows. Main IPC
validates its sender and resolves paths from saved projects. Previews reject paths
outside their selected folder and bound file reads. Agents and explicitly approved
setup commands still run with your normal OS permissions.
