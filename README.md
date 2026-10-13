# Convoy

**Website and download:** [peshbar.com/convoy](https://peshbar.com/convoy) · **Releases:** [GitHub Releases](https://github.com/iftech-team/convoy/releases) · Part of the [Peshbar](https://peshbar.com) family.

Convoy is a workspace for Claude Code and Codex sessions. Sessions and cross-agent reviews come first; Spec Driven Development is optional.

It is written in Rust on every platform:

- [`apps/linux/crates/convoy-core`](apps/linux/crates/convoy-core) holds the rules, storage, Git, providers and the task queue, with no toolkit dependency.
- [`apps/convoy`](apps/convoy/README.md) is the client for macOS, Windows and Linux, built with Tauri on top of the core.
- [`apps/linux`](apps/linux/README.md) also holds the native GTK client for Linux, which shares the same core.

See each client's README for setup, build instructions and current limitations.

Data from the earlier native macOS app (`~/Library/Application Support/Convoy`) is imported by `convoy_core::macos_import`; the client offers it on first launch and under Settings → Setup.

## Releasing

`scripts/release.sh <version> [notes.md]` tags the current `main` and publishes a GitHub release. That starts `.github/workflows/convoy-release.yml`, which builds every client with the release version and attaches them under names that never change: `Convoy.zip` and `Convoy-macos-universal.dmg` for macOS, `Convoy-windows-x64-setup.exe`, and the Linux AppImage and `.deb`. peshbar.com and `convoy-install.sh` download `releases/latest/download/Convoy.zip`, so they follow every release once its macOS job finishes.

The macOS app is ad-hoc signed unless the repository has the `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` and `APPLE_SIGNING_IDENTITY` secrets (a Developer ID Application certificate, base64-encoded `.p12`); with `APPLE_ID`, `APPLE_PASSWORD` (app-specific) and `APPLE_TEAM_ID` as well, it is also notarized.

## License

MIT. The Claude and OpenAI marks belong to their owners and are used to identify the agents.
