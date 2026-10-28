# M006 Current-Head LSP Process Identity Audit

Review baseline: branch `plan/daemon-lsp-frontend-convergence-20261027`,
HEAD `11e2f4a1801f6f867edbb8f72cf403fecb7595e0`.

This is the source audit and implementation baseline for M006. The maintainer
approved opt-in admission; concurrent process-start admission is implemented,
while active-process caps, retirement and benchmark evidence remain open.

## Existing ownership paths

| Path | Service owner and identity | Reuse / isolation result |
|---|---|---|
| Production socket/HTTP daemon | `CoreRuntimeDeps::lsp_service` is installed by socket, stdio and HTTP constructors via `configured_service`. | FullHost process ownership is daemon-scoped; optional startup admission config is shared by that service. |
| Explicit standalone/in-process TUI | `src/main.rs` constructs one `LspService` from the user's LSP config and passes that same `Arc` to `InprocCoreClient` and the local `LspTool`. | Reuse is local to that process; it is not shared with a separately running socket daemon. |
| FullHost `LspService` | `get_or_create_client` resolves a canonical project root and server id, then keys clients as `"{root}:{server_id}"`. Client-map publication and initialization-slot election are single-flight per key. The `LspConfig` and process launcher are immutable fields of the containing service. | Same canonical root/server within one service reuses one process. Different roots, including separate worktrees, remain separate. Different service instances/configs never alias. |
| Linux constrained-turn LSP | `M004LspServiceScope` canonicalizes the workspace root and constructs a new `LspService` with a profile-bound launcher for each scope. Scope shutdown drains its service and cancels the launcher. | Must remain separate from FullHost and from other turn scopes, even at the same path/server id. |

`crates/codegg-core::ProcessAdmissionService` is a different authority: it
refuses process launches into a checkout reserved by a shared agent-run group.
It is not a general LSP concurrency budget and must not be repurposed as one.

## Current bounds and gaps

- The initialization map coalesces concurrent starts only for the identical
  root/server key. Optional `lsp_process_admission` additionally bounds
  concurrent starts and total admitted starts across distinct roots.
- When the config section is absent, startup behavior remains unlimited as
  before. Queue saturation returns a typed request failure; cancelling a
  waiting start releases its slot. This bounds start bursts, not live process
  count. There is still no global/per-workspace active-process cap or idle
  retirement policy.
- `LspService::shutdown_all` is the service-lifetime cleanup path. Manual
  restart is generation-coordinated, but there is no per-key eviction API.
- Open documents are tracked, and client keys, operational states, generation
  and persisted launch descriptors are queryable inside the service. There is
  no bounded public process inventory that combines these facts or exposes
  resource counters. Do not expose process IDs or raw launch arguments to a
  frontend.
- The service key omits a config fingerprint because config/launcher are
  immutable within a service instance. Any future registry that reuses clients
  across service instances must include a non-secret compatibility/config
  fingerprint and execution profile in its identity.
- Canonical root detection falls back to the input path if `canonicalize`
  fails. Daemon callers must continue to supply the catalog's canonical
  workspace root; filesystem aliases must not become an alternate authority.

## Host and server availability

The qualification host is ARM64 macOS with 24 GiB memory; the shell reports
x86_64 because Rust 1.89 verification runs through Rosetta. Rust 1.89.0,
rust-analyzer 1.89.0 and pyright 1.1.411 are available. No LSP servers were
running during this audit.

## M006 implementation constraints

Keep start admission scoped to daemon-owned FullHost services; preserve
same-key single-flight; never share across distinct canonical roots, config
fingerprints, launchers or execution profiles; keep constrained scopes
turn-bounded; and never retire a process while a lease/request/document makes
it active. If present, `lsp_process_admission.max_concurrent_starts` and
`max_admitted_starts` default to 2 and 8. No idle policy is selected.
Benchmark evidence must record exact server versions,
root/worktree topology, process counts, startup/read latency, peak RSS/CPU and
whether indexing completed; the previous host's qualitative claims do not
substitute for this host's measurements.
