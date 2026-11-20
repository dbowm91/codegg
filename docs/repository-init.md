# Repository initialization

`/init` previews an evidence-based draft for the selected project's root
`AGENTS.md`. The analyzer is read-only: it does not run discovered commands,
contact a provider, read global personal instructions, or write project files.

The scan reads only allowlisted project metadata and is bounded by file-count,
depth, file-size, total-size, and time limits. It skips symlinked evidence and
common generated, cache, dependency, and version-control directories. The
preview identifies each source and labels its facts observed, inferred, or
unknown. A declared script is evidence that a command exists, not that it
works.

For an existing root `AGENTS.md`, the draft replaces only the section between
`<!-- codegg:init:start -->` and `<!-- codegg:init:end -->`, preserving text
outside those markers. If the markers are absent, it appends the managed
section. Repeated evidence that would make no change produces a no-op. The
daemon binds publication to the selected project and workspace, and refuses a
stale preview if the target changes or the editor has unsaved changes.

In the TUI, run `/init` to inspect the candidate and diff. Press `a` to publish
the root `AGENTS.md`; press `Esc` to cancel. Publication uses a fresh, one-use
daemon token. After a successful write, project instructions are refreshed for
future turns; active turns keep the instruction snapshot they already loaded.
No provider connection is required. See `architecture/agent.md#repository-initialization-drafts`
for the implementation contract.

## Source verification

Previously checked 2026-10-23 against the source tree. Rechecked 2026-11-15
against `src/agent/bootstrap.rs`, `src/tui/commands/project_init.rs`,
`src/tui/components/dialogs/info.rs`, `src/tui/app/input.rs`,
`src/core/daemon_project_init.rs`, and `src/core/project_init.rs`.

- The analyzer's allowlist, bounded read-only scan, evidence labels, marker
  behavior, and target digest are in `src/agent/bootstrap.rs`.
- `/init`, the `a` publish action, and `Esc` cancellation are implemented in
  `src/tui/command.rs`, `src/tui/components/dialogs/info.rs`, and
  `src/tui/app/input.rs`.
- Publication consumes a client-, project-, and workspace-bound one-use token;
  it checks the target digest, selected workspace root, and unsaved editor state
  before writing in `src/core/daemon_project_init.rs` and
  `src/core/project_init.rs`.
- Successful publication triggers project-scoped instruction refresh through
  `src/tui/commands/project_init.rs`.
