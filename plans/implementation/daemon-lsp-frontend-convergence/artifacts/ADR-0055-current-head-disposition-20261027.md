# ADR-0055 Current-Head Command Disposition

Review baseline: requested branch `plan/daemon-lsp-frontend-convergence-20261027`, based on `origin/main` at `80171f45295bebb9ab0b521262c0b295acb93074`.

Purpose: provide the current source-backed disposition required by the proposed ADR before M003/M004 are made ready. This is research, not ADR acceptance or implementation authorization.

## Findings

- `crates/codegg-core/src/command_catalog.rs` defines 26 `/lsp-*` command identities. All are catalogued as daemon effects and name `command.execute`.
- `src/core/daemon_commands.rs::build_inner_request` currently has rows only for `lsp_status` and `lsp_errors`; `serves_form` therefore reports those two forms served.
- Both rows currently build the same `LspDiagnosticsGet` request. `/lsp-status` consequently does not return the advertised server health/status, and `/lsp-errors <server-key>` ignores its server key and does not return per-server errors. The served bit is true, but current behavior is not command parity.
- Native typed operations already exist for `LspReadGet`, `LspCompletionGet`, `LspDiagnosticsGet`, `LspDiagnosticsSubscribe`, and `LspPreviewApply`. `LspPreviewApply` is authorized as `via_session + file.modify` and is routed through the checked apply service. It is already used by the TUI change-review flow, but the `/lsp-preview-apply` slash arm still requires a frontend-local `LspTool` and refuses in socket mode.
- `LspService` owns the server processes in the daemon. Constrained Linux turns use a separate per-turn `M004LspServiceScope`; it is not a handle a project command may enumerate or control.
- `PreviewArtifactRegistry` is bounded and turn-local in `SessionToolContext`; preview artifacts are not durable and are dropped with their turn registry/service lifecycle. An operational `/lsp-preview*` surface therefore needs an explicit owner/lifetime binding or must remain unavailable when that registry is absent.
- The existing capability set has no LSP lifecycle-control capability. `command.execute` is `direct_project` and is granted to Contributor as well as Maintainer. It is too broad to silently authorize restart/stop/cache-clear of daemon-shared servers or state.
- The command execution path re-checks its inner typed request on that request's own authority and records a canonical `Run`; `command.execute` is not permission to bypass an inner operation's authorization.

## Form disposition

| Form | Current route/state | Proposed class under ADR option B |
|---|---|---|
| `/lsp-status` | Served, but currently returns diagnostics rather than server health | Replace with a bounded typed health/status read; do not keep the misleading mapping |
| `/lsp-errors <server-key>` | Served, but the argument is ignored and it returns the same diagnostics result as status | Bounded typed per-server health/error read; validate immutable server identity |
| `/lsp-servers` | No command route; local `LspTool` inspection exists | Bounded typed inspection read |
| `/lsp-capabilities <server-key>` | No command route; local inspection exists | Bounded typed inspection read, server key resolved daemon-side |
| `/lsp-root <path>` | No command route; local path/root diagnosis exists | Bounded typed read using workspace-relative path validation; never accept a frontend root |
| `/lsp-doctor <path>` | No command route; local doctor exists | Bounded typed diagnostic read, with root and server resolved by daemon |
| `/lsp-cache-status` | No command route; local cache summary exists | Bounded typed inspection read; expose only authorized scope and bounded counters |
| `/lsp-restart <server-key>` | No command route; local TUI can restart its own server | Explicitly authorized daemon lifecycle-control effect; server/root selected by daemon |
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

## Decisions needed to accept ADR-0055

1. The owner must accept Option B (or choose a different option) and confirm the M003/M004 split.
2. The owner must select authority for restart, stop, and cache-clear. Existing `command.execute` is Contributor-granted and cannot silently become authority over a shared daemon process. A new narrowly scoped capability such as `lsp.control` is the clearest semantic fit, but it requires an explicit role/grant decision; `project.configure` is an alternative only if the owner agrees that its meaning covers this operational effect. `file.read` must never authorize these controls.
3. The owner must define preview artifact ownership and applicability for human commands: session/turn binding, expiration, visibility across clients, clear scope, and behavior after turn teardown or daemon restart. Existing turn-local registries do not justify a daemon-global preview lookup.
4. The owner must confirm whether standalone TUI keeps its local `LspTool` command behavior during migration and what explicit deprecation would look like.
5. The owner must confirm whether `/lsp-status` and `/lsp-errors` keep their current names/arguments or receive a corrected contract; the present served mapping is not semantically accurate.

After those decisions, M003 needs typed bounded inspection/control DTOs, daemon-side server/root resolution, authorization descriptors and matrix/role updates if a new capability is selected, audit/run classification, compatibility negotiation, and end-to-end denial/isolation/restart tests. M004 then needs typed bounded workflow/preview requests that reuse the checked apply operation, plus cross-client preview ownership and stale-base tests. Neither milestone should add raw JSON-RPC forwarding, trust frontend paths/roots/PIDs/argv, or operate on `M004LspServiceScope` processes from a project-wide command.
