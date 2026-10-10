# Repository initialization drafts

Repository initialization analyzes the explicitly selected project and
proposes a concise `AGENTS.md` section from local evidence. The analyzer is
read-only: it does not run build or test commands, contact a provider, access
global personal instructions, or write project files.

Evidence is limited to allowlisted README, language manifest, task, workflow,
and existing instruction files. Scans have entry-count, depth, per-file,
total-byte, and elapsed-time limits. Common dependency, generated, cache, and version-control
directories and symlinked evidence are skipped. Reports name the source file
and classify facts as observed, inferred, or unknown. A manifest or declared
script is evidence that a command exists, not that it succeeds.

For an existing root `AGENTS.md`, the candidate replaces only the section
between `<!-- codegg:init:start -->` and `<!-- codegg:init:end -->`. Human text
outside those markers is preserved. If the markers are absent, the proposal
is appended; unchanged evidence produces an idempotent no-op. The candidate
includes the digest of the exact existing file, or records that the file was
absent, so a later publishing surface can reject stale previews.

Run `/init` in the TUI to inspect the complete candidate and diff. Press `a`
to publish the root `AGENTS.md`; Esc cancels without writing. Publication is
bound to the selected project/workspace, requires a fresh one-use daemon token,
and fails if the original target changed or has unsaved editor changes. A
successful write refreshes project instructions for future turns; active turns
keep their existing snapshot. No provider connection is required. See
[`architecture/agent.md`](../architecture/agent.md#repository-initialization-drafts)
for the implementation contract.
