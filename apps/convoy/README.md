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

## The terminal

`portable-pty` gives a real pty on Unix and ConPTY on Windows, and xterm.js
draws it — the same pairing VS Code uses. Output is pushed to the web view as events:
an agent emits thousands of lines a second and a poll would either lag or spin.

The open question this prototype exists to answer is **input latency on Linux**,
where the web view is WebKitGTK rather than the Chromium-based WebView2 or
WKWebView the other two platforms get. Measure before building further.

## What it costs

Release build, one window, three session tabs, no agent running, measured with
`scripts/memory.sh`.

| | GTK client | This |
|---|---|---|
| Processes | 1 | 3 |
| RSS | 167 MB | 417 MB |
| Unique (PSS) | 65 MB | 137 MB |
| Artifact | 3.6 MB binary | 8.9 MB binary |

**A browser engine is not free.** This costs roughly twice the GTK client in
resident memory, and that is the price of the design, stated plainly. It stays
modest because the web view is the system's rather than a bundled copy of
Chromium.

Two caveats on the numbers. Both figures come from the same Xvfb setup, where
there is no GPU and both fall back to software rendering. And on Windows and macOS the web view is WebView2 and WKWebView,
which are not WebKitGTK — these numbers describe Linux only.

## Tasks and Linear/Jira import

The Tasks tab lists a project's tasks and imports them from Linear or Jira
(**Linear & Jira…** in the tab or in Settings). Linear connects with a personal
API key; Jira with an email and API token (Cloud), a personal access token
(Data Center) or a username and password (Server basic auth). Either can use
the agent's own MCP server instead, in which case Convoy stores nothing and the
task brief tells the agent to fetch the issue with its MCP tools.

Choose one agent and model for the whole import or override them per issue,
then queue the tasks or run them at once. The same key from two different
sites is two issues; the same issue is never imported twice into a project.
Jira queries with symbolic operators (`project=PAY`) are sent as JQL; tick
**JQL** to send anything else verbatim.

Requests, parsing and import rules live in `convoy-core::integrations`.
Connections are saved in `integrations.json` beside `workspace.json`, created
owner-only (0600) — plain text, not an OS keychain.

## Run it

```sh
npm install
npm run tauri dev
```

Needs `webkit2gtk-4.1`, `libsoup-3.0` and a Rust toolchain. It reads the real
`${XDG_CONFIG_HOME:-~/.config}/Convoy Desktop Preview/workspace.json`, the same
schema-3 file the GTK build uses. **Do not run both against it at once** — each writes the whole document.

## Look at it without a display

```sh
./scripts/shot.sh /tmp/convoy.png
```

Starts Xvfb, seeds a throwaway workspace, runs the app and photographs it.
