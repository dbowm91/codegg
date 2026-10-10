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

Use the root package's `lsp-test-support` feature and its
`codegg-lsp-test-server` harness for deterministic integration scenarios.
The `egglsp` crate also builds a separate `egglsp-test-server` binary; they
are distinct targets. Real-server tests require separately installed servers
and are excluded from default workspace sweeps; follow the exact feature
guidance in `AGENTS.md`. Keep navigation and diagnostics read-only unless a
separately planned edit contract authorizes mutation.
