# Convoy for Linux

A native GTK4 rewrite of the Electron preview, for Linux only. The Electron
app in `apps/desktop` stays as the Windows implementation and is not touched by
this work. Plan and rationale: [`docs/plan-refactor/`](../../docs/plan-refactor/).

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
cargo test -p convoy-core      # 61 tests, no display needed
cargo clippy --all-targets -- -D warnings
cargo run --bin convoy
```

Debian/Ubuntu need `libgtk-4-dev libadwaita-1-dev libvte-2.91-gtk4-dev
libgtksourceview-5-dev`.

## Terminal probe

Before trusting VTE with the product's core, run a real agent in it:

```sh
cargo run --bin convoy-vte-probe                  # login shell
cargo run --bin convoy-vte-probe -- claude
cargo run --bin convoy-vte-probe -- codex --no-alt-screen
```

Check alt-screen entry and exit, colours, emoji and CJK width, mouse reporting,
bracketed paste and resize. The buttons exercise the three things the port
needs from a PTY beyond drawing: reading the buffer back, writing into the
child, and killing the process group.

## Storage

State lives in `${XDG_CONFIG_HOME:-~/.config}/Convoy Desktop Preview/`, the
same location and the same schema-3 `workspace.json` the Electron preview uses,
so this build can run on real data and the old build remains a working
fallback. Unknown fields are preserved on write.

**Do not run both builds at once against the same file.** Each writes the whole
document; the last writer wins.

## Status

- `convoy-core` — done: workspace, validation, migration, Git, files, previews,
  providers, launch, accounts, planning, history, telemetry hooks, worktree
  setup, transcripts, Codex usage, repository tools.
- `convoy-gtk` — skeleton and terminal probe. The window currently reports the
  storage path and workspace summary; the real UI is M2 onwards.

Tests never send a model request and never publish a branch or pull request.
