# M003 — Daemon LSP Inspection and Lifecycle Command Parity: Closure

Status: **closed on branch**
Closed on: 2026-10-28
Branch: `plan/daemon-lsp-m003-m004-20261028`
Decision: ADR-0056 Option B accepted 2026-10-28

## Implementation

The implementation is present in the branch worktree based on `dacf8d3c3`.
The shared daemon `LspService` now serves bounded session-rooted status,
capability, error, root-diagnosis, lifecycle, cache, and composite workflow
requests. TUI socket commands use typed daemon routes; standalone TUI retains
its in-process behavior. Cache ownership and profile fingerprints remain in
`LspService`, and root-filtered status/clear cannot cross workspaces. A missing
`LspControl` and `LspServerStatusGet` session-scope mapping was found by the
team-principal integration tests and corrected before closure.

## Requirement and command-form matrix

The generated current-head inventory is
`plans/implementation/desktop-frontend-ide-foundation/artifacts/m007-d-command-classification.md`.
It records 161 commands, 167 argument forms, 129 daemon-effect forms, and the
authoritative source/route for each form. The `/lsp-*` disposition is:

| Form | Route | Authority / outcome |
|---|---|---|
| `/lsp-status`, `/lsp-servers`, `/lsp-capabilities`, `/lsp-errors` | `LspServerStatusGet` | Session-rooted `file.read`; server selectors resolve only among warm clients; bounded/redacted status. |
| `/lsp-root` | `LspWorkflowExecute::RootDiagnosis` | Session-rooted `file.read`; traversal and symlink escapes refused before server start. |
| `/lsp-restart`, `/lsp-stop` | `LspControl` | Maintainer+ `lsp.control`; selected server only; manual stop retains the managed owner for later restart. |
| `/lsp-cache-status` | `LspControl::CacheStatus` | Session-rooted `file.read`; canonical root and profile-fingerprint scoped. |
| `/lsp-cache-clear` | `LspControl::CacheClear` | Maintainer+ `lsp.control`; clears only the active session workspace. |
| `/lsp-context-diagnostics` | Existing bounded diagnostics/context route | Session/project authorization; bounded evidence. |
| `/lsp-repair-local`, `/lsp-review-file`, `/lsp-impact`, `/lsp-test-repair`, `/lsp-interface`, `/lsp-cross-repair`, `/lsp-call-neighbors` | `LspWorkflowExecute` | Eight bounded session-rooted recipes total; file paths are relative and validated by the daemon. |
| `/lsp-previews`, `/lsp-preview`, `/lsp-preview-clear`, `/lsp-preview-refresh`, `/lsp-preview-apply` | Not served in socket mode | Session/turn preview-registry contract is not part of ADR-0056 Option B; no local fallback is advertised as daemon execution. |
| `/lsp-doctor`, `/lsp-repair-hunk`, `/lsp-review-diff`, `/lsp-security-review` | Not served in socket mode | No safe bounded canonical mapping was approved; reasons are recorded in the generated inventory and M004 plan. |

`/lsp-errors` and `/lsp-capabilities` retain their names but require a selected
server; `/lsp-status` reports the session's server summary. The classification
generator `--check` passes, and availability was not published before a typed
route, authorization descriptor and error path existed.

## Verification

| Qualification | Result |
|---|---|
| `cargo test -p codegg --features server,lsp-test-support --test lsp_test_support` (x86_64 macOS, compatible linker wrapper) | 71 passed, 0 failed. Includes two-client server inspection, contributor denial, Maintainer stop isolation, cache scoping, status scope, root traversal/symlink refusal, and M004 workflow coverage. |
| `cargo test -p egglsp --lib cache::tests::cache_root_stats_and_clear_do_not_cross_workspace_boundaries` | 1 passed. |
| `cargo test -p egglsp --locked` | 1,038 passed across 5 suites. |
| `python3 scripts/check_authorization_matrix.py` | Passed. |
| M007-D command classification generator `--check` | Passed; 161/167/129 inventory matches source. |
| `python3 scripts/check_comment_budget.py` | Passed. |
| Desktop panel and editor authority guards | Passed. |
| Desktop Tauri host `cargo check` | Passed. |
| `CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/tmp/codegg-lsp-test-linker/clang apps/desktop/scripts/verify-desktop.sh host` | Passed: 97 tests, 1 ignored (requires an external daemon fixture), desktop `cargo fmt --check`, and Clippy `-D warnings`. |
| Desktop renderer typecheck and focused `EditorSurface` suite | Passed; 18/18 tests. |
| `git diff --check` | Passed. |

The host is x86_64 macOS with arm64 MacPorts libraries also installed. Runtime
tests used the worktree's x86_64 linker wrapper to select compatible
dependencies. No hosted exact-head CI or real language-server qualification is
claimed.

## Authorization, compatibility, and limitations

`lsp.control` is limited to restart, stop and cache clear. Cache status and
semantic reads use `file.read`. Server/root scope is derived from the session
row; caller-supplied paths do not set authority. No process IDs, server argv,
raw stderr or raw JSON-RPC are exposed. The constrained Linux turn service is
not controlled by the FullHost UI route. Old clients receive typed unsupported
behavior; socket mode does not fall back to frontend-owned server launches.
Standalone TUI behavior remains available.

M003 is closed. M004 is closed separately in
`plans/closure/daemon-lsp-frontend-convergence/004-status.md`. M005 and M006
are ready as separate implementation plans under their now-accepted decisions.

## Closure audit

Source plan: `plans/implementation/daemon-lsp-frontend-convergence/003-daemon-lsp-operations-and-command-parity.md`.
Source roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`.
Baseline reviewed: `dacf8d3c3` on `plan/daemon-lsp-m003-m004-20261028`; the
implementation and closure are currently the branch worktree changes.

### Invariants and failure recovery

- Session-derived workspace and server scope is enforced in daemon handlers;
  unauthorized lifecycle/cache mutation fails through `lsp.control` before
  effect. Root traversal and symlink escape tests prove no server is started.
- Warm-only reads do not start a server. Per-server stop retains the selected
  managed owner for restart and leaves a warm sibling attached. Cache status
  and clear remain canonical-root and profile-fingerprint scoped.
- Status/control DTOs remain bounded and redact raw stderr, process IDs and
  server arguments. Additive protocol variants preserve typed unsupported
  behavior for older peers; no storage migration is introduced.
- Fake-LSP tests cover read-only denial, selected-server isolation, cold reads,
  root rejection, cache isolation and shared-client operation. Hosted exact-head
  CI and real-server qualification were not run and are not represented as
  passed evidence.

### Documentation, unresolved findings and registry

Updated references include `architecture/lsp.md`,
`architecture/authorization.md`, the M003/M004 plans, the current generated
command inventory, the convergence roadmap and `plans/registry.md`.
Unresolved product findings: none for M003's bounded route scope. External
qualification still outstanding: hosted exact-head CI and installed real LSP
servers. These do not change the verified fake-server contract.

Registry disposition: M003 and M004 are closed; M005 and M006 are ready under
their accepted decisions. M007-F is closed by these two milestones. No other
registered dependent plan was blocked solely on M003 after M004 closure.
