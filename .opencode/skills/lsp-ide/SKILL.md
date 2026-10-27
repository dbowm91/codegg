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

Route native TUI and desktop diagnostics/read/completion gestures through
`codegg-client::LspRequestCoordinator`. Its key must retain attachment,
project/workspace/session, document/path/server, generation, operation, and
position/argument identity. Keep frontend rendering and status vocabulary
separate; discard a response when the ticket is superseded or the active
attachment/revision changed. Do not hold a renderer or route lock across an
await.

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
commands use — and `=` previews what formatting would change. M016 gave both
previews a way to become an edit: a complete preview carries the material the
apply path consumes, and the chord opens the **change review** on it, where
`[a]pply` writes it and `Esc` rejects it. A preview that cannot be applied
(truncated, empty, too large) stays a preview and says so. M017 added the third
preview leaf, `space a`: it asks the server what code actions it would offer
at the cursor, **once**, and works out each action's own diff locally. A code
action is a *choice*, so the LSP panel is the picker — `j`/`k` move and `Enter`
opens the change review on the selected action. An action the server attached
a **command** to is never applied here, even one that also carries an edit:
the LSP specification applies an action's edit *and then* its command, and
applying half of an action is not applying it. The panel says which actions
cannot be applied and why; the reason is per action, because one action in a
list can be command-only while the one beside it is perfectly applicable.

**The write path is the daemon's, and it is ADR-0008's.** ADR-0008 is accepted
and owns it: `CoreRequest::LspPreviewApply` is authorized `via_session` +
`file.modify` and applies a digest-bound candidate through the change-review
surface or the model-facing preview-apply tool. The editor reaches it by
**carrying** the candidate — the daemon mints it into the read result, the
editor holds it, and the review's accept builds the request through
`build_apply_request`, the single place a `CoreRequest::LspPreviewApply` is
constructed (`scripts/check_tui_editor_apply_path.py` enforces that). Nothing
is registered: `apply_preview` recomputes the digest over the request's own
patches, so a candidate altered in transit fails the check. Do not describe
this as a missing ADR, and do not add a second apply path.
Semantic tokens are only a presentation overlay for the exact controller
revision that was queried; malformed UTF-16 ranges, overlap, unsupported types,
or revision changes fall back to plain text.

For location DTOs, preserve display `path` separately from optional
`workspace_relative_path`. Only the daemon derives this locator after
canonical containment under the session-authorized workspace. Treat it as
untrusted input and open targets only through `DocumentOpen`; a missing value
means display-only. Never infer a path from the basename or trust an LSP URI.

LSP servers are long-lived child processes in a daemon-owned service. The
service accepts one `LspProcessLauncher` at construction and routes initial
launches and restarts through it. FullHost turns retain the shared direct
launcher. On Linux, a constrained turn may receive only its own M004 service
scope, bound to the canonical workspace and daemon-resolved profile. The M004
launcher preserves supervised JSON-RPC pipes and retains the helper status
receipt through child exit. The turn owns launch, restart, and shutdown; it
cannot reuse a FullHost client. `lsp`, `lsp_read`, preview apply, and LSP
prompt-context collection stay unavailable if scope construction or launch
fails. Never accept profile selection from an LSP client payload.

Use the root package's `lsp-test-support` feature and its
`codegg-lsp-test-server` harness for deterministic integration scenarios.
The `egglsp` crate also builds a separate `egglsp-test-server` binary; they
are distinct targets. Real-server tests require separately installed servers
and are excluded from default workspace sweeps; follow the exact feature
guidance in `AGENTS.md`. Keep navigation and diagnostics read-only unless a
separately planned edit contract authorizes mutation.

## See Also

- `architecture/lsp.md` — authoritative LSP contract
- `architecture/ide.md` — the IDE-facing operations and their DTOs
- `architecture/tui.md` — the editor keymap and the change-review surface
- `architecture/client.md` — the frontend/client boundary the TUI consumes
- `architecture/agent.md` — tool dispatch for the model-facing preview-apply
  tool
- `architecture/security.md` — the daemon-owned sandbox and containment rules
  that bound an LSP server launch
- `.opencode/skills/tui/SKILL.md` — TUI command registration and async dispatch
- `.opencode/skills/tool-execution/SKILL.md` — the process-ownership rules that
  govern the LSP child processes
- `.opencode/skills/security-hardening/SKILL.md` — platform containment
  guarantees (Linux/macOS qualified, Windows backend absent)

## Source verification

Verified 2026-10-23 against `crates/egglsp/src/server.rs`,
`crates/egglsp/src/bin/`, `crates/egglsp-test-server/src/main.rs`,
`src/lsp/`, `src/core/daemon_lsp.rs`, `src/tui/commands/change_review.rs`,
`crates/codegg-protocol/src/core.rs`,
`scripts/check_tui_editor_apply_path.py`, `Cargo.toml`, and `AGENTS.md`.

Confirmed unchanged: `crates/egglsp` owning the implementation with
`src/lsp/` as a thin shim (the shim holds `mod.rs`, `mutation.rs`,
`semantic_context.rs`, the `hunk_nav*` modules, and `m004.rs` — no language
server implementation); `CoreDaemon::start_lsp_diagnostics_driver`
(`src/core/daemon_lsp.rs:1456`), called from `src/main.rs:4580,4860`;
`build_apply_request` (`src/tui/commands/change_review.rs:77`) as the single
place a `CoreRequest::LspPreviewApply` is built;
`CoreRequest::LspPreviewApply` (`crates/codegg-protocol/src/core.rs:3530`);
`scripts/check_tui_editor_apply_path.py` enforcing that single apply path;
and the two distinct test-server binaries — `codegg-lsp-test-server` is the
root package's `[[bin]]` behind `lsp-test-support`
(`Cargo.toml:106-109`, path `crates/egglsp-test-server/src/main.rs`), while
the `egglsp` package builds `egglsp-test-server` from `crates/egglsp/src/bin/`.

No claim in this skill was contradicted by the current tree. The only changes
are the addition of this note and of the missing `## See Also` section, which
the skill's own intro pointed at (`architecture/lsp.md`, `architecture/tui.md`,
`architecture/client.md`) but never listed under a `## See Also` heading.

Not verified: the F2–F7 key bindings and the `space k` / `space s` /
`space S` / `space d` / `space h` / `gr` / `gd` / `gi` / `space r` / `=` /
`space a` chords are asserted by this skill but were **not** re-checked
line-by-line against the TUI keymap in this pass — the TUI keymap lives in
`src/tui/` and is owned by the `tui` skill's audit. Treat the chord list as
authoritative only after reading the TUI editor's own keymap.

