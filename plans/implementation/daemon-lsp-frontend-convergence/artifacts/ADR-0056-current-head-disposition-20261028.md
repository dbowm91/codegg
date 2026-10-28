# ADR-0056 Current-Head Command Disposition

Review baseline: `main` at `7de6f237c0c4ab2abff120f39b7ae98e7107bec3` (2026-10-28), after PR #208.

Purpose: record the source-backed disposition under accepted ADR-0056 Option B. The bounded `lsp.control`, session/turn-bound preview, standalone TUI, and status/error defaults were selected on 2026-10-27 and formally accepted on 2026-10-28. This is a living implementation inventory; refresh it as routes land.

## Findings

- `crates/codegg-core/src/command_catalog.rs` defines 26 `/lsp-*` command identities. All are catalogued as daemon effects and name `command.execute`.
- `src/core/daemon_commands.rs::build_inner_request` has rows for `lsp_status`, `lsp_servers`, `lsp_capabilities`, `lsp_errors`, and `lsp_restart`. The four inspection forms build the session-scoped `LspServerStatusGet` request; `/lsp-errors` and `/lsp-capabilities` require a server key, which the daemon verifies against its already-attached clients for that session's canonical workspace. Restart builds `LspControl::Restart` and passes the independent `lsp.control` authorization gate.
- Production socket, stdio and HTTP constructors install the configured FullHost `LspService`; Linux per-turn `M004LspServiceScope` remains isolated.
- Native typed operations already exist for `LspReadGet`, `LspCompletionGet`, `LspDiagnosticsGet`, `LspDiagnosticsSubscribe`, and `LspPreviewApply`. `LspPreviewApply` is authorized as `via_session + file.modify` and is routed through the checked apply service. It is already used by the TUI change-review flow, but the `/lsp-preview-apply` slash arm still requires a frontend-local `LspTool` and refuses in socket mode.
- `LspService` owns the server processes in the daemon. Constrained Linux turns use a separate per-turn `M004LspServiceScope`; it is not a handle a project command may enumerate or control.
- `PreviewArtifactRegistry` is bounded and turn-local in `SessionToolContext`; preview artifacts are not durable and are dropped with their turn registry/service lifecycle. An operational `/lsp-preview*` surface therefore needs an explicit owner/lifetime binding or must remain unavailable when that registry is absent.
- `lsp.control` now exists as a separate capability granted to Maintainer and Owner. Lifecycle controls must pass their inner typed operation through it; `command.execute` alone remains insufficient.
- The command execution path re-checks its inner typed request on that request's own authority and records a canonical `Run`; `command.execute` is not permission to bypass an inner operation's authorization.

## Form disposition

| Form | Current route/state | Proposed class under ADR option B |
|---|---|---|
| `/lsp-status` | Session-scoped typed status read, pending runtime qualification | Retain with accurate daemon-owned attached-server status |
| `/lsp-errors <server-key>` | Session-scoped per-server failure read; server identity is daemon-validated | Retain; require immutable server identity |
| `/lsp-servers` | Session-scoped bounded status list, pending runtime qualification | Retain; list only clients attached beneath session root |
| `/lsp-capabilities <server-key>` | Session-scoped bounded supported-capability list, pending runtime qualification | Retain; server key resolved daemon-side |
| `/lsp-root <path>` | No command route; local path/root diagnosis exists | Bounded typed read using workspace-relative path validation; never accept a frontend root |
| `/lsp-doctor <path>` | No command route; local doctor exists | Bounded typed diagnostic read, with root and server resolved by daemon |
| `/lsp-cache-status` | No command route; local cache summary exists | Bounded typed inspection read; expose only authorized scope and bounded counters |
| `/lsp-restart <server-key>` | Typed `LspControl::Restart` command route; Maintainer+ `lsp.control`; TUI request is asynchronous | Keep as explicitly authorized daemon lifecycle-control effect; server/root selected by daemon |
| `/lsp-stop <server-key or --all>` | No command route; local TUI can stop its own server(s) | Explicitly authorized control effect; `--all` needs a defined authorized scope and cannot mean daemon-global by default |
| `/lsp-cache-clear <root or --all>` | No command route; local TUI can clear semantic cache | Explicitly authorized control effect; root must be daemon-resolved and scope-bounded |
| `/lsp-previews` | No command route; lists the local tool's bounded registry | Typed preview-registry read only if a daemon-owned registry is bound to the caller's session/turn; otherwise remain unserved |
| `/lsp-preview <id>` | No command route; local registry detail | Typed preview detail read under the same registry ownership/lifetime rule |
| `/lsp-preview-clear <id or --all>` | No command route; clears local registry entries | Scoped ephemeral-registry mutation; define whether `--all` means this caller's turn only |
| `/lsp-preview-refresh <id>` | No command route; refreshes local staleness | Scoped read/refresh against the exact artifact and its daemon-authorized workspace; no implicit apply |
| `/lsp-preview-apply <id>` | Existing typed `LspPreviewApply` is used by change review; slash arm is local-only | Wire the command as a thin caller of the existing checked apply service; do not add another apply path |
| `/lsp-context-diagnostics <path>` | No command route; local composite context operation | M004 bounded typed recipe/read; bind path and disclosure to session/workspace |
| `/lsp-repair-local <path[:line]>` | No command route; local composite recipe | M004 composite preview recipe; reviewable output only, apply through existing checked apply |
| `/lsp-repair-hunk <path> [hunk-id|range]` | No command route; local recipe; current TUI parsing passes the path and does not use the optional hunk selector | M004 recipe only after argument semantics are specified and validated |
| `/lsp-review-file <path>` | No command route; local composite recipe | M004 bounded review recipe |
| `/lsp-review-diff` | No command route; local composite recipe | M004 bounded review recipe over daemon-resolved diff/workspace scope |
| `/lsp-security-review [path|diff]` | No command route; local composite recipe | M004 security-context recipe with explicit source, path, and output bounds |
| `/lsp-impact <path:line:col>` | No command route; local composite recipe | M004 bounded hierarchy/impact recipe; validate UTF-16 position and path daemon-side |
| `/lsp-test-repair <test-file> [failure-text]` | No command route; local composite recipe | M004 preview recipe; bound and treat failure text as untrusted data, never as a command |
| `/lsp-interface <path[:symbol]>` | No command route; local composite recipe | M004 bounded interface-review recipe |
| `/lsp-cross-repair <primary> [related...]` | No command route; local composite recipe | M004 bounded multi-file context/preview recipe with per-path authorization |
| `/lsp-call-neighbors <path:line:col> [direction]` | No command route; local composite recipe | M004 bounded typed hierarchy read; validate direction, path, and position |

The four preview-registry forms above are listed separately from the recipes: their blocker is artifact ownership/lifetime, not a missing recipe DTO. The recipes should not be implemented by recreating a second LSP process in either frontend.

## Accepted owner decisions and remaining evidence

ADR-0056 Option B is accepted. M003 is active and still needs its remaining inspection/lifecycle routes, negative authorization proofs and complete socket/standalone comparisons before closure. M004 remains blocked on M003 closure and still needs workflow parity and checked-preview race evidence. Neither milestone may add raw JSON-RPC forwarding, trust frontend roots/PIDs/argv, or operate on `M004LspServiceScope` processes from a project-wide command.
