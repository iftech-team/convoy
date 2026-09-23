# Port and native review — September 23, 2026

Reference: native macOS 0.5.8 (`2732d28`). Its Files & Changes source changes were
applied to this worktree while retaining the Electron implementation. No merge,
commit, push, release, model request, or real task publication was performed.

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

- Native macOS: 51-test suite passed; Git panel and terminal snapshot fixtures
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

## Local Linux artifact

Built `apps/desktop/release/linux-validation/Convoy Preview-0.1.0-arm64.AppImage`
(122 MB, ignored build output). Extraction and the unpacked telemetry/PTY payload
were verified using Debian Bookworm. The minimal Node image lacked the runtime's
`libz.so`; extraction succeeded in the full Debian image. Validate runtime library
availability and FUSE support on the intended distribution before shipping.
This is ARM64, not the Windows/Linux x64 CI artifact.

SHA-256: `6f500c765537d697f7896e627ec85d64d1834098b951465ec861212b5dfc55bb`.
