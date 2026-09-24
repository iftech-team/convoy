# Convoy for Linux

A native GTK4 client for Linux. It shares `convoy-core` with the Tauri client
in [`apps/convoy`](../convoy/README.md), which covers Windows. Plan and rationale: [`docs/plan-refactor/`](../../docs/plan-refactor/).

## Layout

```
crates/convoy-core/   every rule, no toolkit dependency, fully tested
crates/convoy-gtk/    GTK4 + libadwaita + VTE front end and the binary
data/                 .desktop file, icons, AppStream metainfo
packaging/            PKGBUILD, debian/, rpm spec
```

`convoy-core` does not depend on `gtk`, `adw` or `vte4`. That boundary is the
point: logic cannot drift into signal handlers by accident, and the tests run
without a display.

## Build

```sh
sudo pacman -S rust gtk4 libadwaita vte4 gtksourceview5   # Arch
cargo test -p convoy-core                        # 79 tests, no display needed
xvfb-run -a cargo test -p convoy-gtk --test ui   # 70 window checks
xvfb-run -a cargo run --bin convoy-vte-selftest  # 12 terminal checks
./scripts/smoke.sh                               # starts and exits cleanly
cargo clippy --all-targets -- -D warnings
cargo run --bin convoy
```

Debian/Ubuntu need `libgtk-4-dev libadwaita-1-dev libvte-2.91-gtk4-dev
libgtksourceview-5-dev`.

## Terminal checks

Everything a machine can decide about VTE is decided by the selftest, which
launches the real agent CLIs through the same `session_spec` the app uses:

```sh
xvfb-run -a cargo run --bin convoy-vte-selftest
```

It covers output fidelity, all three input routes, bracketed paste, the
alternate screen, exit codes for both instant and normal exits, `killpg`
reaching the whole process tree, resize, and character width. It exits
non-zero if any check fails.

What a machine cannot judge — mouse reporting and redraw under load — is left
to the interactive probe:

```sh
cargo run --bin convoy-vte-probe                  # login shell
cargo run --bin convoy-vte-probe -- claude
cargo run --bin convoy-vte-probe -- codex --no-alt-screen
```

Read the buffer back with `text_format`, never `text_range_format`: agents draw
their interface with absolute cursor positioning, which a row range does not
return. That mistake would have produced empty review briefs.

## Storage

State lives in `${XDG_CONFIG_HOME:-~/.config}/Convoy Desktop Preview/`, the
same location and the same schema-3 `workspace.json` the Tauri client uses. The
directory keeps its older name so existing projects are not stranded. Unknown
fields are preserved on write.

**Do not run both builds at once against the same file.** Each writes the whole
document; the last writer wins.

## What it costs

Measured on this machine with `scripts/memory.sh`: one window, three session
tabs, no agent running.

| | This build |
|---|---|
| Processes | 1 |
| RSS | 167 MB |
| Unique (PSS) | 65 MB |
| Artifact | 3.6 MB binary |

The figures were taken under Xvfb, which has no GPU: GTK then falls back to a
software renderer that carries tens of megabytes of its own. With `GSK_RENDERER`
left at its default that same window measures 217 MB RSS and 115 MB PSS, and the
50 MB difference is the software stack, not the application. A real desktop
session pays neither.

## Status

- `convoy-core` — done: workspace, validation, migration, Git, files, previews,
  providers, launch, accounts, planning, history, telemetry hooks, worktree
  setup, transcripts, Codex usage, repository tools.
- `convoy-gtk` — the main window: project tree with groups, session tabs, VTE
  terminals with start and stop, the thirteen-entry session menu, worktrees,
  review handoff, quick commands, a split view, search, settings and toasts.
  Files & Changes covers the working tree, files, log and branches, with
  highlighted diffs, a side-by-side reader, staging, commits and remotes.
  Specs & tasks covers revisions, approval, Markdown export, task briefs and
  the queue. Hooks report each agent's state, which drives notifications,
  hibernation and the activity log; account profiles keep provider sign-ins
  separate. A command palette, an editable set of shortcuts shared with the
  Tauri client, project settings and a sidebar context menu complete the
  navigation. Packaging is M8.

`convoy-gtk` is a library with thin binaries on top, so the window can be built
and inspected by a test rather than only by a person.

Tests never send a model request and never publish a branch or pull request.
