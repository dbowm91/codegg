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
The current diagnostics change publisher has no production driver, so do not
promise ongoing push updates or add implicit polling in a presentation-only
change.

The native TUI editor uses F2 for diagnostics, F3 for hover, F4 for
definition, F5 for references, F6 for document symbols, and F7 for workspace
symbol search. These are existing `LspDiagnosticsGet`/`LspReadGet` operations;
bind results to the active editor identity and generation. Semantic tokens
are only a presentation overlay for the exact controller revision that was
queried; malformed UTF-16 ranges, overlap, unsupported types, or revision
changes fall back to plain text.

For location DTOs, preserve display `path` separately from optional
`workspace_relative_path`. Only the daemon derives this locator after
canonical containment under the session-authorized workspace. Treat it as
untrusted input and open targets only through `DocumentOpen`; a missing value
means display-only. Never infer a path from the basename or trust an LSP URI.

LSP servers are long-lived child processes in a daemon-shared service. The
current service is not keyed by sandbox profile and its launcher is not yet
integrated with M004 process/IPC containment. Constrained turns must not create
or reuse its FullHost clients: keep `lsp`, `lsp_read`, preview apply, and LSP
prompt-context collection unavailable for those turns. FullHost turns retain
the existing service path. Never accept profile selection from an LSP client
payload. Restore constrained LSP only after a daemon-owned M004 launcher and
per-profile process identity/lifecycle are qualified end to end.

Use the root package's `lsp-test-support` feature and its
`codegg-lsp-test-server` harness for deterministic integration scenarios.
The `egglsp` crate also builds a separate `egglsp-test-server` binary; they
are distinct targets. Real-server tests require separately installed servers
and are excluded from default workspace sweeps; follow the exact feature
guidance in `AGENTS.md`. Keep navigation and diagnostics read-only unless a
separately planned edit contract authorizes mutation.
