# M003 — Daemon LSP Inspection and Lifecycle Command Parity (M007-F Part 1)

Status: blocked — ADR-0055 acceptance and lifecycle-control authorization selection (hard)

Primary class: capability.

Repository planning baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (`main`, 2026-10-27); before implementation inspect current branch HEAD, update file paths and record drift. Source roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`. Planning governance: `plans/003-planning-process.md`. Do not edit accepted ADRs or historical closure records.

Controlling proposed ADR: `plans/adrs/ADR-0055-daemon-lsp-command-and-runtime-control-surface.md`. Accepted ADRs: ADR-0008, ADR-0010–0012, ADR-0018, ADR-0052. Existing M007-F registration: desktop frontend roadmap; do not open a duplicate command catalog.

Current-head research: `plans/implementation/daemon-lsp-frontend-convergence/artifacts/ADR-0055-current-head-disposition-20261027.md`. This inventory does not accept ADR-0055 or resolve its authority decisions; retain the blocked status until the ADR owner records those decisions.

## Objective

Make low-frequency language-server status/health/capabilities/selection and authorized restart/stop/cache controls truly executable against the daemon's authoritative LSP instance from both TUI and GUI, with accurate availability publication. Split composite repair/review recipes to M004.

## Current evidence

`src/main.rs` installs `app.lsp_tool` in non-socket mode only. `src/core/daemon_commands.rs` publishes only `/lsp-status` and `/lsp-errors` as served; `architecture/lsp.md` records the other 24 as unserved. `crates/egglsp/src/service.rs` has supervision, health, generation and restart facilities. `src/core/daemon_lsp.rs` already serves read/completion/diagnostics; these MUST be reused instead of reimplementing LSP transport in the frontend.

## Ordered implementation

1. Generate a current-head inventory using `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check`; enumerate all current `/lsp-*` forms with request support, local-vs-socket behavior, security scope, daemon capability and exact disposition. Record counts from HEAD rather than freeze historical 26/2/24.
2. Implement *inspection first*: typed server list, selected root/server/config identity (authorized), capability matrix, health/indexing/restart generation, errors/status and cache status. Reuse existing `egglsp` structured data; size and redact stderr/errors, hide unauthorized workspace existence; distinguish cold, degraded, failed and unsupported. Only reuse `CommandExecute` for low-frequency human commands, never editor point reads.
3. After ADR capability choice, implement daemon-owned bounded lifecycle operations (restart, stop, possibly cache clear) with generation-safe sequencing, profile/checkout checks, restart conflict/supersession handling and managed-document replay. Fail closed rather than exposing process IDs or raw server args. Constrained Linux turn scopes must not be controlled by an unrelated UI.
4. Add only narrow DTOs/handlers to `codegg-protocol`, `codegg-core` authorization, daemon effect table and `codegg-client` as needed. Update catalogue `daemon_served` **only after** an end-to-end route and error path exists; present unavailable honestly otherwise. Rewrite TUI socket command arms to hit daemon through shared route; preserve standalone non-socket behavior until approved migration. Desktop palette calls same daemon forms, with no JS-side implementation.
5. Add non-vacuous guards for frontend LSP child launches under daemon-client mode and for falsely served unimplemented commands. Update `architecture/lsp.md`, `architecture/command.md`, authorization inventory, generated classification artifact and roadmap cross-references.

## Security, protocol, migrations

Additive typed protocol only, no storage migration expected. Review `file.read` disclosure separately from authority to restart/stop other clients' servers. Route every privileged control through existing authorization descriptors, audit per `CommandExecute` semantics, and avoid server-supplied `workspace/executeCommand`. No project root or server process authority chosen from untrusted argv. On old daemon, typed unsupported result; no local LSP fallback in socket mode.

## Tests / exit

`cargo test -p codegg --locked lsp`, `cargo test -p egglsp --locked`, `cargo test -p codegg-client --locked`, `cargo check -p codegg --locked --features server`, `python3 scripts/check_authorization_matrix.py`, classification-generator `--check`, desktop CI gate plus root `scripts/verify.sh` as supported. Two clients inspect same server; read-only principal cannot restart; restart while one read and one dirty document are active; probe old-generation diagnostics refusal, stale completion witness, constrained-scope isolation, disallowed path, server failure/restart and cold state. Use a fake LSP transcript plus optional real rust-analyzer evidence.

Closure `plans/closure/daemon-lsp-frontend-convergence/003-status.md` must provide per-form served/unserved matrix, verification results, mutation/authorization denial proofs, standalone-vs-socket comparison and explicit deferred M004 forms. STOP if ADR-0055 remains unaccepted or lifecycle capability undecided; never treat this plan as authority itself.
