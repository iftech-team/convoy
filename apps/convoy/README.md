# Convoy for Windows and Linux

A Tauri client sharing one design and one codebase across Windows and Linux.
macOS keeps its native SwiftUI app — it is the design this one follows.

## Why this and not the GTK client

The design is a web design: segmented controls, a single violet accent, thin
borders, cards with soft shadows. GTK and libadwaita have their own visual
language and fight attempts to leave it, and even a won fight only reaches
"nearly". More to the point, GTK cannot cover Windows, and neither can VTE —
the terminal widget the GTK client was built around.

What made the swap cheap is the rule the GTK client was built under:
`convoy-core` has no toolkit dependency. It moves here **unchanged**, with all
its tests and every behavioural invariant. Only the front end is new.

| | Kept | Replaced |
|---|---|---|
| Rules, storage, Git, providers, queue | `convoy-core`, 7,981 lines with tests | — |
| Window, dialogs, terminal widget | — | `convoy-gtk`, 7,891 lines |

## What it does

Everything the app does. Projects and discovery, sessions with resume and
recovery, models and accounts; reviews and feedback to the builder; Git
worktrees with shared files and a setup command; Files & Changes with four
views, side-by-side diffs, staging, hunk discard, commits, branches, remotes
and pull requests; specifications with approval and export; tasks as a list and
as a board, with a queue and publish modes; transcript import; usage limits;
the agent monitor, hibernation, keep-awake, notifications, the activity log and
a command palette.

The rules behind all of it are `convoy-core`, shared with the GTK client and
byte-compatible with what the Electron and SwiftUI builds write.

## The terminal

`portable-pty` gives a real pty on Unix and ConPTY on Windows, and xterm.js
draws it — the same pairing VS Code uses, and the same terminal the Electron
preview already ran Claude Code in. Output is pushed to the web view as events:
an agent emits thousands of lines a second and a poll would either lag or spin.

The exit rules run in the thread that reaps the child, not in the web view. A
clean exit hands the task to review — never to done, which stays a decision
somebody makes — and a failure or a stop pauses the queue. None of that should
depend on a window being able to answer.

The open question is still **input latency on Linux**, where the web view is
WebKitGTK rather than the Chromium-based WebView2 or WKWebView the other two
platforms get. Nobody has sat and typed at a running agent for long enough to
say.

## Windows

The same binary, built on `windows-2022` in CI: formatting, lints, the terminal
tests through ConPTY, the shared core's own tests, and an NSIS installer as an
artifact. Four things differ underneath, and each has a reason to exist rather
than a `cfg!` for its own sake:

- The agent is found by walking `PATH` for `claude.exe`, then for the standard
  npm package to run under `node` — the resolution the Electron build used,
  with a test asserting the argv is byte-identical.
- There are no process groups, so stopping walks the tree with `taskkill /T`
  and then `/T /F`, which keeps the two stages of a stop rather than making
  both of them fatal.
- Keeping the machine awake owns a thread of its own, because the request
  belongs to the thread that made it and a command runs on a pooled one.
- Every child process is spawned with `CREATE_NO_WINDOW`. Convoy has no console
  of its own, and there are a great many git calls.

## What it costs

Release build, one window, three session tabs, no agent running, measured with
`scripts/memory.sh`.

| | Electron preview | GTK client | This |
|---|---|---|---|
| Processes | 7 | 1 | 3 |
| RSS | 745 MB | 167 MB | 417 MB |
| Unique (PSS) | ~400–500 MB (estimated) | 65 MB | 137 MB |
| Artifact | 122 MB AppImage | 3.6 MB binary | 8.9 MB binary |

**A browser engine is not free.** This costs roughly twice the GTK client in
resident memory, and that is the price of the design, stated plainly. It is
still well under Electron, and in three processes rather than seven, because
the web view is the system's rather than a bundled copy of Chromium.

Two caveats on the numbers. The GTK and Tauri figures come from the same Xvfb
setup, where there is no GPU and both fall back to software rendering; the
Electron figure was taken on a real desktop session and is not measured the
same way. And on Windows and macOS the web view is WebView2 and WKWebView,
which are not WebKitGTK — these numbers describe Linux only.

## Run it

```sh
npm install
npm run tauri dev
```

Needs `webkit2gtk-4.1`, `libsoup-3.0` and a Rust toolchain. It reads the real
`${XDG_CONFIG_HOME:-~/.config}/Convoy Desktop Preview/workspace.json`, the same
schema-3 file the Electron and GTK builds use. **Do not run two of them against
it at once** — each writes the whole document.

## Look at it without a display

```sh
./scripts/shot.sh /tmp/convoy.png
```

Starts Xvfb, seeds a throwaway workspace, runs the app and photographs it.
