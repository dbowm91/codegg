# ADR-0056 Current-Head Command Disposition

Research baseline: `main` at `7de6f237c0c4ab2abff120f39b7ae98e7107bec3` (2026-10-28), after PR #208. The implementation snapshot below reflects the active M003/M004 branch and the generated current-head inventory.

Purpose: record the source-backed disposition under accepted ADR-0056 Option B. The bounded `lsp.control`, session/turn-bound preview, standalone TUI, and status/error defaults were selected on 2026-10-27 and formally accepted on 2026-10-28. This is a living implementation inventory; refresh it as routes land.

## Findings

- `crates/codegg-core/src/command_catalog.rs` defines 26 `/lsp-*` command identities. All are catalogued as daemon effects and name `command.execute`.
- `src/core/daemon_commands.rs::build_inner_request` has rows for status, server list, capabilities, errors, restart, stop, cache status/clear and the M004 recipes. The inspection forms build session-scoped `LspServerStatusGet`; the daemon resolves server keys among already-attached clients beneath that session's canonical workspace. Restart, stop and cache clear use `LspControl`, independently gated by `lsp.control`. Cache status is a separate `file.read` operation. Cache status/clear resolve the session root in the daemon and filter service-owned entries by that root.
- Production socket, stdio and HTTP constructors install the configured FullHost `LspService`; Linux per-turn `M004LspServiceScope` remains isolated.
- Native typed operations already exist for `LspReadGet`, `LspCompletionGet`, `LspDiagnosticsGet`, `LspDiagnosticsSubscribe`, `LspWorkflowExecute`, and `LspPreviewApply`. `LspPreviewApply` is authorized as `via_session + file.modify` and remains the only apply authority. TUI change review and the desktop editor preview use that checked route; `/lsp-preview-apply` as a registry command remains unavailable in socket mode.
- `LspService` owns the server processes in the daemon. Constrained Linux turns use a separate per-turn `M004LspServiceScope`; it is not a handle a project command may enumerate or control.
- `PreviewArtifactRegistry` is bounded and turn-local in `SessionToolContext`; preview artifacts are not durable and are dropped with their turn registry/service lifecycle. An operational `/lsp-preview*` surface therefore needs an explicit owner/lifetime binding or must remain unavailable when that registry is absent.
- `lsp.control` is a separate capability granted to Maintainer and Owner. Restart, per-server stop, and cache clear pass through it; cache status uses session-rooted `file.read`. `command.execute` alone remains insufficient.
- The command execution path re-checks its inner typed request on that request's own authority and records a canonical `Run`; `command.execute` is not permission to bypass an inner operation's authorization.

## Form disposition

| Form | Current route/state | Proposed class under ADR option B |
|---|---|---|
| `/lsp-status` | Session-scoped typed status read; fake-LSP integration qualified | Retain with accurate daemon-owned attached-server status |
| `/lsp-errors <server-key>` | Session-scoped per-server failure read; server identity is daemon-validated | Retain; immutable server identity |
| `/lsp-servers` | Session-scoped bounded status list | Retain; list only clients attached beneath session root |
| `/lsp-capabilities <server-key>` | Session-scoped bounded capability list | Retain; server key resolved daemon-side |
| `/lsp-root <path>` | `LspWorkflowExecute::RootDiagnosis`; session-rooted and traversal/symlink checked | Retain; workspace-relative path only |
| `/lsp-doctor <path>` | No daemon command route | Remain unavailable; full doctor depends on local preview-registry and cache state not exposed by a safe bounded route |
| `/lsp-cache-status` | `LspControl::CacheStatus`; `file.read`; session-root-filtered bounded counters | Retain as a read-only inspection |
| `/lsp-restart <server-key>` | Typed `LspControl::Restart`; Maintainer+ `lsp.control`; asynchronous TUI request | Retain as explicitly authorized daemon lifecycle control; selector resolves under session root |
| `/lsp-stop <server-key>` | Typed `LspControl::Stop`; Maintainer+ `lsp.control`; one attached server only | Retain; no daemon-wide `--all` operation |
| `/lsp-cache-clear` | `LspControl::CacheClear`; session-root-filtered; Maintainer+ `lsp.control` | Retain; no caller-selected root or daemon-wide `--all` |
| `/lsp-previews` | No command route; lists the local tool's bounded registry | Typed preview-registry read only if a daemon-owned registry is bound to the caller's session/turn; otherwise remain unserved |
| `/lsp-preview <id>` | No command route; local registry detail | Typed preview detail read under the same registry ownership/lifetime rule |
| `/lsp-preview-clear <id or --all>` | No command route; clears local registry entries | Scoped ephemeral-registry mutation; define whether `--all` means this caller's turn only |
| `/lsp-preview-refresh <id>` | No command route; refreshes local staleness | Scoped read/refresh against the exact artifact and its daemon-authorized workspace; no implicit apply |
| `/lsp-preview-apply <id>` | Checked `LspPreviewApply` remains native and is consumed by TUI change review and desktop editor; registry slash arm is unavailable | Keep command unavailable until its ephemeral registry can be safely bound to authenticated session/turn lifetime |
| `/lsp-context-diagnostics <path>` | `LspWorkflowExecute::ContextDiagnostics`; bounded workspace-relative inputs/results | M004 served recipe; `file.read` and session root |
| `/lsp-repair-local <path[:line]>` | `LspWorkflowExecute::RepairLocal`; inert bounded summary, no mutation | M004 served recipe; any accepted change still uses checked apply |
| `/lsp-repair-hunk <path> [hunk-id|range]` | No command route; local recipe; current TUI parsing passes the path and does not use the optional hunk selector | M004 recipe only after argument semantics are specified and validated |
| `/lsp-review-file <path>` | `LspWorkflowExecute::ReviewFile`; bounded daemon-owned LSP evidence | M004 served recipe; `file.read` and session root |
| `/lsp-review-diff` | No command route; local composite recipe | M004 bounded review recipe over daemon-resolved diff/workspace scope |
| `/lsp-security-review [path|diff]` | No command route; local composite recipe | M004 security-context recipe with explicit source, path, and output bounds |
| `/lsp-impact <path:line:col>` | `LspWorkflowExecute::ImpactAnalysis`; bounded hierarchy recipe with daemon-validated position | M004 served recipe; `file.read` and session root |
| `/lsp-test-repair <test-file> [failure-text]` | `LspWorkflowExecute::TestFailureRepair`; failure text is bounded untrusted data | M004 served recipe; never executes server-supplied commands |
| `/lsp-interface <path[:symbol]>` | `LspWorkflowExecute::InterfaceBoundary`; bounded interface evidence | M004 served recipe; `file.read` and session root |
| `/lsp-cross-repair <primary> [related...]` | `LspWorkflowExecute::CrossFileRepair`; bounded per-path inputs | M004 served recipe; every path is checked under session root |
| `/lsp-call-neighbors <path:line:col> [direction]` | `LspWorkflowExecute::CallNeighborhood`; direction/path/position validated daemon-side | M004 served recipe; `file.read` and session root |

The four preview-registry forms above are listed separately from the recipes: their blocker is artifact ownership/lifetime, not a missing recipe DTO. The recipes should not be implemented by recreating a second LSP process in either frontend.

## Accepted owner decisions and remaining evidence

The generated inventory at `plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md` is the source for exact current counts. ADR-0056 Option B is accepted. The implementation now has typed routes for status, controls, root diagnosis, eight bounded semantic workflows, and checked format/rename previews in both editor surfaces. Remaining unserved command forms are preview-registry management, doctor, repair-hunk, diff review and security review; each lacks an equivalent session-bound bounded route or has ambiguous legacy argument semantics. Do not claim command parity for those forms. Neither milestone may add raw JSON-RPC forwarding, trust frontend roots/PIDs/argv, or operate on `M004LspServiceScope` processes from a project-wide command. The x86_64 host's complete fake-LSP family ran with a compatible linker wrapper; no hosted CI or real-server claim is made.
