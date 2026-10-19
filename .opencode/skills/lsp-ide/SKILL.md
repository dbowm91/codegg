---
name: lsp-ide
description: Change CodeGG language-server or IDE integrations while preserving bounded operations and explicit workspace authority
version: 1.0.0
tags: [lsp, ide, editor]
---

# LSP and IDE integration

CodeGG's LSP implementation lives in `crates/egglsp`; `src/lsp/` is a thin
shim. The server manager owns executable lifecycle, initialization, and
capability-gated requests. Keep the TUI and other frontends as consumers of
typed CodeGG operations rather than adding direct language-server processes
or transport-specific parsing to a renderer.

Before changing behavior, read `architecture/lsp.md`,
`architecture/tui.md`, and `architecture/client.md`. Check the active LSP
capability and workspace-root contracts before adding an operation. Results
must remain bounded, and an unsupported server capability should produce the
existing typed unsupported result instead of an unqualified request.

For diagnostics, keep readiness separate from contents: `NotReady` is not an
empty clean set. Preserve the last complete view until a `Ready` pull arrives.
M013 gave the change publisher a **daemon-owned** driver
(`CoreDaemon::start_lsp_diagnostics_driver`), so push updates do arrive now. Do
not add implicit polling in a presentation-only change, and do not move the
cadence onto a frontend — a frontend-triggered poll would put the "may start a
server" decision on whichever client happens to be open.

Reads resolve **which** warm server answers in four steps: an explicit
`server_id`, then an extension match (only when exactly one warm client claims
it), then the single warm client, then an `Ambiguous` result naming the
candidates. `Ambiguous` is not a readiness state — retrying cannot make it go
away. Warm-only is structural, not a pre-gate check: every daemon read path
goes through a `*_for_warm_client` entry point, and any new operation must have
one, or it will resolve a client of its own and can start a server.

The native TUI editor uses F2 for diagnostics, F3 for hover, F4 for
definition, F5 for references, F6 for document symbols, and F7 for workspace
symbol search. These are existing `LspDiagnosticsGet`/`LspReadGet` operations;
bind results to the active editor identity and generation. M013 added the
Helix leaves on top: `space k` hover, `space s` document symbols, `space S`
workspace symbols, `space d` diagnostics, `space h` and `gr` references,
`gd` definition, `gi` implementation. A leaf that cannot run says so in the
status row rather than doing nothing. M015 added the two **preview** leaves:
`space r` previews renaming the symbol under the cursor — the new name is
collected on the `:` line, the same line the editor's other argument-taking
commands use — and `=` previews what formatting would change. Both show what
would change and **apply nothing**. `space a` (code action) stays unbound with
a visible reason, because no code-action *read* exists to preview.

**Do not say the write path is missing.** ADR-0008 is accepted and owns it:
`CoreRequest::LspPreviewApply` is authorized `via_session` + `file.modify` and
applies a **registered, still-current reviewable preview** by id, through the
change-review surface or the model-facing preview-apply tool. The editor's
preview leaves produce a *read result*, not such an artifact — no preview id,
no patches, no `register_preview_artifact` in the editor path — so they
cannot reach it. The gap is the **bridge**, not an ADR. When that bridge is
built, `space r` and `=` become the apply step; that transition is registered
in `CHANGELOG.md` now so it is a documented upgrade rather than a surprise.
Semantic tokens are only a presentation overlay for the exact controller
revision that was queried; malformed UTF-16 ranges, overlap, unsupported types,
or revision changes fall back to plain text.

For location DTOs, preserve display `path` separately from optional
`workspace_relative_path`. Only the daemon derives this locator after
canonical containment under the session-authorized workspace. Treat it as
untrusted input and open targets only through `DocumentOpen`; a missing value
means display-only. Never infer a path from the basename or trust an LSP URI.

Use the root package's `lsp-test-support` feature and its
`codegg-lsp-test-server` harness for deterministic integration scenarios.
The `egglsp` crate also builds a separate `egglsp-test-server` binary; they
are distinct targets. Real-server tests require separately installed servers
and are excluded from default workspace sweeps; follow the exact feature
guidance in `AGENTS.md`. Keep navigation and diagnostics read-only unless a
separately planned edit contract authorizes mutation.
