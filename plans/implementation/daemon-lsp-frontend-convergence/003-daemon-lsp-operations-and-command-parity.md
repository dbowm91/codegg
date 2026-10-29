# M003 — Daemon LSP Inspection and Lifecycle Command Parity (M007-F Part 1)

Status: closed — 2026-10-28; ADR-0056 Option B accepted and the bounded M003 routes are qualified

Primary class: capability.

Repository planning baseline: `7de6f237c0c4ab2abff120f39b7ae98e7107bec3` (`main`, 2026-10-28). Source roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`. Planning governance: `plans/003-planning-process.md`. Do not edit accepted ADRs or historical closure records.

Controlling accepted ADR: `plans/adrs/ADR-0056-daemon-lsp-command-and-runtime-control-surface.md` (Option B, accepted 2026-10-28). Also governed by ADR-0008, ADR-0010–0012, ADR-0018 and ADR-0052. Existing M007-F registration: desktop frontend roadmap; do not open a duplicate command catalog.

Current-head research: `plans/implementation/daemon-lsp-frontend-convergence/artifacts/ADR-0056-current-head-disposition-20261028.md`. The production socket, stdio and HTTP daemon constructors already install the configured FullHost `LspService`; Linux constrained turns retain separate per-turn service scopes. Keep the inventory synchronized with the generator, and record implementation drift as routes land.

## Objective

Make low-frequency language-server status/health/capabilities/selection and authorized restart/stop/cache controls truly executable against the daemon's authoritative LSP instance from both TUI and GUI, with accurate availability publication. Split composite repair/review recipes to M004.

## Current evidence

Socket, stdio and HTTP constructors install the configured daemon-owned `LspService`; standalone TUI and its in-process core continue sharing one local instance. `/lsp-status`, `/lsp-servers`, `/lsp-capabilities`, and `/lsp-errors` use session-scoped `LspServerStatusGet`. `/lsp-errors` and `/lsp-capabilities` require a server selector resolved among clients under the session workspace. Restart and per-server stop use typed `LspControl` and independently require Maintainer+ `lsp.control`; stop retains the client/document owner for a later managed restart and never stops a sibling server. Cache status uses `file.read`; cache clear requires Maintainer+ `lsp.control`. Both operate only on the active session workspace. The semantic cache is owned by `LspService`, shared by exact cache-profile fingerprint, and its status/clear APIs filter by canonical workspace root. `/lsp-root` uses a `file.read`-authorized session-rooted request; it rejects traversal and symlinks without starting a server. The TUI issues served reads and controls asynchronously. The fake daemon fixture covers session-root status, two clients inspecting one shared server, omitted stderr, Contributor lifecycle/cache-clear denial, Contributor cache-status access, warm selection ranges, unsupported selection capability, cold reads without spawning, one-server stop with a warm sibling retained, cache controls, and root-diagnosis traversal plus symlink refusal. The M004 typed workflow route is implemented for eight bounded file-scoped forms. Preview-registry commands, diff/security review, doctor, and hunk repair remain explicitly unavailable in socket mode; no frontend fallback is used.

## Ordered implementation

1. Generate a current-head inventory using `plans/implementation/desktop-frontend-ide-foundation/artifacts/generate_m007d_command_classification.py --check`; enumerate all current `/lsp-*` forms with request support, local-vs-socket behavior, security scope, daemon capability and exact disposition. Record counts from HEAD rather than freeze historical 26/2/24.
2. Implement *inspection first*: typed server list, selected root/server/config identity (authorized), capability matrix, health/indexing/restart generation, errors/status and cache status. Reuse existing `egglsp` structured data; size and redact stderr/errors, hide unauthorized workspace existence; distinguish cold, degraded, failed and unsupported. Inspection and cache status return bounded data and no raw stderr. `CommandExecute` is used only for low-frequency human commands, never editor point reads.
3. Implement daemon-owned bounded lifecycle operations with generation-safe sequencing, profile/checkout checks, restart conflict/supersession handling and managed-document replay. Restart and per-key stop are implemented under `lsp.control`; semantic cache status/clear is daemon-owned, profile-fingerprinted and root-filtered. Controls fail closed rather than exposing process IDs or raw server args. Constrained Linux turn scopes are not controlled by an unrelated UI.
4. Add only narrow DTOs/handlers to `codegg-protocol`, `codegg-core` authorization, daemon effect table and `codegg-client` as needed. Update catalogue `daemon_served` **only after** an end-to-end route and error path exists; present unavailable honestly otherwise. Rewrite TUI socket command arms to hit daemon through shared route; preserve standalone non-socket behavior until approved migration. Desktop palette calls same daemon forms, with no JS-side implementation.
5. Add non-vacuous guards for frontend LSP child launches under daemon-client mode and for falsely served unimplemented commands. Update `architecture/lsp.md`, `architecture/command.md`, authorization inventory, generated classification artifact and roadmap cross-references.

## Security, protocol, migrations

Additive typed protocol only, no storage migration expected. Review `file.read` disclosure separately from authority to restart/stop other clients' servers. Route every privileged control through existing authorization descriptors, audit per `CommandExecute` semantics, and avoid server-supplied `workspace/executeCommand`. No project root or server process authority chosen from untrusted argv. On old daemon, typed unsupported result; no local LSP fallback in socket mode.

## Tests / exit

`cargo test -p codegg --locked lsp`, `cargo test -p egglsp --locked`, `cargo test -p codegg-client --locked`, `cargo check -p codegg --locked --features server`, `python3 scripts/check_authorization_matrix.py`, classification-generator `--check`, desktop CI gate plus root `scripts/verify.sh` as supported. Two clients inspect same server; read-only principal cannot restart; restart while one read and one dirty document are active; probe old-generation diagnostics refusal, stale completion witness, constrained-scope isolation, disallowed path, server failure/restart and cold state. Use a fake LSP transcript plus optional real rust-analyzer evidence.

Current host qualification: `cargo fmt --all`,
On 2026-10-28 the x86_64 macOS host ran the entire fake-LSP integration family
with the matching x86_64 linker wrapper: 71/71 passed. This includes shared
server inspection from two clients, Contributor lifecycle/cache-clear denial,
Maintainer stop isolation with a warm sibling, cache root isolation, session
status resolution, selection-range bounds, and root traversal/symlink refusal.
The desktop Tauri host `cargo check`, renderer typecheck, 18 focused editor
tests, and `apps/desktop/scripts/verify-desktop.sh host` pass (97 passed, 1
ignored; desktop formatting and Clippy `-D warnings` pass). The authorization
matrix, desktop authority guards, generated command inventory check, comment
budget, and `git diff --check` pass. The separate
root library checked-preview race test passes (1/1). No hosted CI or real
language-server qualification is claimed.

The closure record is `plans/closure/daemon-lsp-frontend-convergence/003-status.md`.
It provides the current per-form served/unserved matrix, verification results,
authorization-denial evidence, standalone/socket comparison and explicit M004
forms. ADR-0056 governs typed scope and authorization; it does not broaden
caller-supplied server/root authority.
