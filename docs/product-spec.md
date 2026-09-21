# SpecDesk: sessions first

## User workflow

The user works from a parent folder containing related repositories, e.g. `olucha-cargo`. An agent launched there can work across those repositories. They need to manage many Claude Code and Codex sessions and pass work between a builder and reviewer without juggling terminal windows. Spec Driven Development is an optional supporting feature, never a prerequisite for sessions or reviews.

## Required behavior

- Import a directory's recognizable projects and preserve nested groups.
- Make every group selectable as an actual session working directory.
- Default to Sessions; show New session prominently.
- Run installed agents inside embedded terminals using their own login/approval settings.
- Retain terminals while switching sessions or closing the window; distinguish quitting.
- Persist names, IDs, notes, review associations and recent output for recovery.
- Start a reviewer with an editable brief, then route inspected feedback back to the builder.
- Allow specs, plans, tasks and acceptance criteria when useful, without gating sessions.
- No mandatory app subscription, Full Disk Access, Accessibility or Automation permission.

## Implemented in 0.2

Nested folder discovery, reusable imports, real group scope, native PTYs via SwiftTerm, session list/search/archive, recent output persistence, explicit resume/picker behavior, reviewer launch, editable feedback insertion, optional specs. Earlier app-container data is preserved on migration. Embedded terminals run with standard user permissions per the user's explicit preference; the app no longer enables Apple App Sandbox.

## Remaining work

- Provider-structured session discovery and exact Codex IDs while running.
- Automatic bounded review/fix loops and structured findings.
- Immutable revision tracking and optional per-task worktrees.
- Attention status based on provider events rather than process state.
- Full searchable conversation import and robust crash recovery.
- Version-controlled Markdown specs with external-edit conflict handling.

## Verification boundaries

A running process is not a successful task. Recent terminal output is not authoritative review evidence. The reviewer should inspect actual code. Until revision pinning is implemented, avoid overlapping builder edits with reviews. Provider history must exist before resume can work. No automatic merge or publication.
