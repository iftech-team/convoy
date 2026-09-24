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
draws it — the same pairing VS Code uses, and the same terminal the Electron
preview already ran Claude Code in. Output is pushed to the web view as events:
an agent emits thousands of lines a second and a poll would either lag or spin.

The open question this prototype exists to answer is **input latency on Linux**,
where the web view is WebKitGTK rather than the Chromium-based WebView2 or
WKWebView the other two platforms get. Measure before building further.

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
