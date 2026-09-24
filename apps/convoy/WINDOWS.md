# Windows development and verification

The Tauri client uses WebView2 and ConPTY. The shared core resolves native
`claude.exe` / `codex.exe`, or their standard npm packages through `node.exe`.
Agent prompts are passed as arguments, without a PowerShell or `.cmd` wrapper.

Install Node.js 22, the stable Rust MSVC toolchain, Visual Studio C++ Build
Tools, WebView2, and Git. Install and sign in to Claude Code or Codex. npm CLI
installs also need `node.exe` on PATH. Restart Convoy after changing PATH.
Project setup commands run in PowerShell on Windows and Bash on Linux.
Copying shared folders containing symlinks requires Windows Developer Mode or
permission to create symlinks; failures are reported rather than followed or
silently copied as ordinary files.

From `apps/convoy`, in PowerShell:

```powershell
npm ci
npm run tauri dev
```

To build an installer:

```powershell
npm run tauri -- build --bundles nsis
```

The installer is written under `src-tauri/target/release/bundle/nsis`.
The Windows job in the `Convoy client` workflow compiles the core and its test targets, tests
native/npm executable resolution, runs ConPTY input/output and workspace/review
lifecycle tests, checks the folder/review UI in Chromium with mocked IPC, and
builds an NSIS installer. It does not publish a release.

Existing workspace data stays in
`%APPDATA%\Convoy Desktop Preview\workspace.json`. Keep that directory even
though the old client has been removed. Linux continues using
`${XDG_CONFIG_HOME:-~/.config}/Convoy Desktop Preview/workspace.json`.

The folder button opens a native folder picker. A builder’s session menu offers **Start review**, which creates a linked
session with the other agent; start it to run the review. **Send feedback to
builder** lets you paste selected findings
into the running builder, without pressing Enter for you.

A successful cross-compilation of the core is not a Windows runtime test.
Before release, require a green Windows workflow and manually check installed
CLI launch/resume, stop, review feedback, task imports, and reopening an existing
workspace on Windows. Real account authentication is not covered by CI.

To run the UI regression check locally:

```powershell
npx playwright install chromium
npm run test:ui
```

This test starts its own Vite server on port 1421 and uses an in-memory backend
fixture. It never opens your saved workspace or launches an agent.
