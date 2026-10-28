# M003 — Daemon LSP Inspection and Lifecycle Command Parity (M007-F Part 1)

Status: blocked — proposed ADR-0056 command-control decision pending

Primary class: capability.

Repository planning baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (`main`, 2026-10-27); before implementation inspect current branch HEAD, update file paths and record drift. Source roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`. Planning governance: `plans/003-planning-process.md`. Do not edit accepted ADRs or historical closure records.

Controlling proposed ADR: `plans/adrs/ADR-0056-daemon-lsp-command-and-runtime-control-surface.md`. Accepted ADRs: ADR-0008, ADR-0010–0012, ADR-0018, ADR-0052. Existing M007-F registration: desktop frontend roadmap; do not open a duplicate command catalog. Do not implement M003 until ADR-0056 receives its final decision.

Current-head research: `plans/implementation/daemon-lsp-frontend-convergence/artifacts/ADR-0056-current-head-disposition-20261027.md`. Update its baseline and route table from the generated M007-D inventory as implementation proceeds. A critical current-head gap is recorded there: the production socket/HTTP daemon constructors do not install `CoreRuntimeDeps::lsp_service`, so authorized native LSP reads cannot reach a daemon-owned service yet.

## Objective

Make low-frequency language-server status/health/capabilities/selection and authorized restart/stop/cache controls truly executable against the daemon's authoritative LSP instance from both TUI and GUI, with accurate availability publication. Split composite repair/review recipes to M004.

## Current evidence

Socket, stdio and HTTP constructors now install the configured daemon-owned `LspService`; standalone TUI and its in-process core continue sharing one local instance. `/lsp-status`, `/lsp-servers`, `/lsp-capabilities`, and `/lsp-errors` now use the session-scoped typed `LspServerStatusGet` route. `/lsp-errors` and `/lsp-capabilities` require a server selector that the daemon resolves among clients under the session workspace. `/lsp-restart` uses typed `LspControl`, independently authorized by Maintainer+ `lsp.control`. The TUI issues these reads and restart asynchronously. The fake daemon fixture now covers session-root status, omitted stderr, Contributor restart denial, warm selection ranges, unsupported selection capability, and cold selection reads without starting a process; the changed root integration target compiles. Standalone `/lsp-stop <key>` now refuses without stopping other servers, with a test asserting the service lifecycle remains Running; daemon stop and cache controls remain unserved until generation-safe per-key lifecycle and daemon-owned cache semantics exist. The remaining local composite forms still need explicit dispositions and parity work; do not label them served.

## Ordered implementation

1. Generate a current-head inventory using `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check`; enumerate all current `/lsp-*` forms with request support, local-vs-socket behavior, security scope, daemon capability and exact disposition. Record counts from HEAD rather than freeze historical 26/2/24.
2. Implement *inspection first*: typed server list, selected root/server/config identity (authorized), capability matrix, health/indexing/restart generation, errors/status and cache status. Reuse existing `egglsp` structured data; size and redact stderr/errors, hide unauthorized workspace existence; distinguish cold, degraded, failed and unsupported. The first four inspection forms are implemented with bounded server counts and no raw stderr; runtime and redaction qualification remains open. Only reuse `CommandExecute` for low-frequency human commands, never editor point reads.
3. Implement daemon-owned bounded lifecycle operations with generation-safe sequencing, profile/checkout checks, restart conflict/supersession handling and managed-document replay. Restart is implemented under `lsp.control`; stop and cache clear remain unserved until per-key stop and daemon-owned cache semantics can be proved safe. Fail closed rather than exposing process IDs or raw server args. Constrained Linux turn scopes must not be controlled by an unrelated UI.
4. Add only narrow DTOs/handlers to `codegg-protocol`, `codegg-core` authorization, daemon effect table and `codegg-client` as needed. Update catalogue `daemon_served` **only after** an end-to-end route and error path exists; present unavailable honestly otherwise. Rewrite TUI socket command arms to hit daemon through shared route; preserve standalone non-socket behavior until approved migration. Desktop palette calls same daemon forms, with no JS-side implementation.
5. Add non-vacuous guards for frontend LSP child launches under daemon-client mode and for falsely served unimplemented commands. Update `architecture/lsp.md`, `architecture/command.md`, authorization inventory, generated classification artifact and roadmap cross-references.

## Security, protocol, migrations

Additive typed protocol only, no storage migration expected. Review `file.read` disclosure separately from authority to restart/stop other clients' servers. Route every privileged control through existing authorization descriptors, audit per `CommandExecute` semantics, and avoid server-supplied `workspace/executeCommand`. No project root or server process authority chosen from untrusted argv. On old daemon, typed unsupported result; no local LSP fallback in socket mode.

## Tests / exit

`cargo test -p codegg --locked lsp`, `cargo test -p egglsp --locked`, `cargo test -p codegg-client --locked`, `cargo check -p codegg --locked --features server`, `python3 scripts/check_authorization_matrix.py`, classification-generator `--check`, desktop CI gate plus root `scripts/verify.sh` as supported. Two clients inspect same server; read-only principal cannot restart; restart while one read and one dirty document are active; probe old-generation diagnostics refusal, stale completion witness, constrained-scope isolation, disallowed path, server failure/restart and cold state. Use a fake LSP transcript plus optional real rust-analyzer evidence.

Current host qualification: `scripts/verify.sh quick` passes, including the
workspace `cargo check --all-targets --locked`; the test target compiles with
`cargo check -p codegg --locked --features server,lsp-test-support --test
lsp_test_support`. `cargo test -p egglsp --locked` cannot link on this host: the
selected x86_64 Rust target resolves arm64 MacPorts `liblzma`/`libiconv`, and
the arm64 Rust 1.89 attempt also fails to link the host's `zstd` library. Do
not treat compiled test code as executed runtime evidence; rerun the focused
suites on a host whose Rust target and native compression libraries agree.

Closure `plans/closure/daemon-lsp-frontend-convergence/003-status.md` must provide per-form served/unserved matrix, verification results, mutation/authorization denial proofs, standalone-vs-socket comparison and explicit deferred M004 forms. Once accepted, ADR-0056 governs typed scope and authorization; never use it to broaden caller-supplied server/root authority.
