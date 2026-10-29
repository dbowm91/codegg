# M006 — Daemon LSP Process Budgets, Worktree Identity and Retirement

Status: active — the opt-in daemon-wide live-child count budget is implemented and lifecycle-qualified locally; exact-head hosted qualification and closure record remain. Bounded start-admission defaults were accepted 2026-10-28; automatic idle retirement is not required.

Primary class: infrastructure/polish.

Repository planning baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (`main`, 2026-10-27). Reinspect live branch before implementation. Roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`; governance: `plans/003-planning-process.md`; long-term specification §§4,13,24–25.

Relevant accepted ADRs: ADR-0010–0012, ADR-0021; Linux M004 process-scope boundary under `src/lsp/m004.rs`; shared checkout/worktree policy and writer fencing are separate authorities. ADR-0056 concerns operational commands, **not** permission to merge restricted and host processes.

## Objective

Bound redundant LSP indexing and resource consumption when TUI, desktop and agent sessions share a checkout, while retaining correct isolation for distinct worktrees and sandbox profiles. Implement safe process admission/retirement with measured benefit and no unbounded memory/CPU spikes under many Rust worktrees.

## Evidence and policy gate

`crates/egglsp/src/service.rs` keys clients as canonical root/server ID and coalesces starts per key; `src/lsp/m004.rs` creates per-turn scoped LSP services. Cross-worktree process reuse is *not* established. `crates/codegg-core/src/process_admission.rs` names `LanguageServer` as a process kind; `src/tool/process_capability.rs` marks `lsp` and `lsp_read` persistent. Production socket/stdio/HTTP constructors now inject a daemon-owned `LspService`; same-root reuse is scoped to that service instance, whose config and launcher are immutable. The full current-head audit is `plans/implementation/daemon-lsp-frontend-convergence/artifacts/M006-process-identity-audit-20261027.md`.

The maintainer accepted these policy boundaries on 2026-10-28: keep present concurrency unchanged by default; add an opt-in bounded admission budget; never retire or evict an attached editor server; include canonical worktree root, server/config fingerprint and execution profile in compatibility identity; and expose only bounded, redacted operational counts. Admission limits and queue saturation must produce typed, cancellable outcomes. Any idle retirement must prove there are no attachments, in-flight requests or open documents before stopping a process. No shared LSP process may cross worktree roots or execution profiles.

The 2026-10-29 corrective qualification resolves the remaining closure policy:
a **total-live-server count budget is required** for strict M006 closure because
the existing start permits end at launch and the sustained mixed workload
retained six rust-analyzer servers at up to 3.6 GiB aggregate descendant RSS.
The count budget is opt-in and daemon-owned, shared by FullHost and constrained
service scopes, retained until child exit is reaped, and refuses promptly with
a typed/cancellable saturation outcome. No portable RSS threshold is inferred
from the host-specific samples. **Automatic idle retirement is not required**:
budget saturation refuses new starts rather than evicting attached editors;
servers remain until owning-scope teardown or authorized stop. The complete
method, repeated measurements and implementation gate are recorded in
`artifacts/M006-sustained-frontend-agent-workload-20261029.md`.

## Work packages

1. Audit every LSP spawn/restart/cancel path for FullHost daemon service, model-turn LSP, Linux constrained scope, standalone TUI legacy path and each platform; map which actually shares process identity and which must remain isolated. Audit profile, canonical worktree/root, server id, argv/env/config, trust and compatibility fingerprint. A new key must be stable and not leak secrets, and callers must never supply arbitrary root/profile.
2. Add telemetry-free process lifecycle counters/measurements (active, starting, ready, failed, generation, queued, indexing, memory/CPU evidence when OS offers it). No raw source text/secret stderr in durable projections.
3. Introduce bounded *daemon-owned* start admission: concurrency cap, deterministic queue/coalescing per compatible key, optional workspace/global active limit, strict cancellation/timeout, no stalled warm-only editor read. Never kill an attached server without a defined user-visible `NotReady` transition and snapshot/replay semantics. Prefer explicit finite idle timeout and LRU policy only after acceptance. Constrained scope lifetime remains turn-bounded.
4. Use the existing managed process-launch helper and lifecycle cleanup, respecting the Linux sandbox helper and platform-specific admission. Keep a separate worktree as a separate semantic root even when Cargo build output is shared. Do not use `CARGO_TARGET_DIR` tricks or Rust dependency caches as a reason to reuse one language server across incompatible roots.
5. Test process admission under parallel agent substreams, two editors, shared checkout and separate worktrees, resource saturation, restart storms, cancelled startup, server crash and daemon shutdown. Surface bounded operator statuses through M003 if available; do not make M003 a hard correctness dependency on process accounting.
6. Add a daemon-wide lifetime permit for every LSP child process across FullHost and constrained service scopes. Hold it from launch admission through successful exit reap; do not release it at initialize or when a startup semaphore is dropped. Saturation refuses promptly and never evicts an attached editor process. Automatic idle retirement is outside this milestone.

## Verification

Run `cargo test -p egglsp --locked`, root `cargo test -p codegg --locked lsp`, `cargo check -p codegg --locked --features server`, `python3 scripts/check_authorization_matrix.py`, relevant managed-process/daemon-workspace static guards and root `scripts/verify.sh`. Native Linux contained LSP exchange must prove host-proc denial; macOS behavior must be qualified and Windows constrained LSP remains explicitly disabled while its filesystem-enforcement backend is absent. Benchmark 1, 2 and 4 simultaneous editors/agents, 1 vs 3 worktrees, warm and cold starts, Rust and Python fixtures; record p50/p95 startup/read latency, number of child processes, peak RAM/CPU and disk-impact evidence with exact machine/model/server versions. Include mixed TUI/desktop daemon reads and separate agent service scopes with managed edits and a sustained analysis window. No invented target SLO or false host-generalized throughput claims.

Current host qualification (2026-10-29): the fake-LSP integration family ran
on this x86_64-reported macOS session with a compatible linker wrapper
(71/71 passed), as did the full `egglsp` package suite (1,038 tests). Native
ARM64 macOS real-server evidence is recorded in
`artifacts/M006-real-server-single-workspace-baseline-20261029.md` and
`artifacts/M006-real-server-multiworktree-resource-matrix-20261029.md`.
The latter covers same-root 1/2/4 service acquisitions and three isolated
worktrees for rust-analyzer 1.89.0 and Pyright 1.1.411, including readiness,
warm symbol reads, process counts, peak sampled descendant RSS/CPU and normal
service teardown. It demonstrates per-root reuse/isolation and start-admission
behavior on this host; it does not establish a total live-process or memory
budget. The acquisition timings are single observations, not p50/p95.
The M006 update reran `cargo test -p egglsp --locked` with matching ARM64
Rust 1.89 `rustc` and `rustdoc`: all 1,038 unit and integration tests passed,
and the doctest target passed (zero doctests). The final
`scripts/verify.sh quick` passed, including formatting, generated assets, all
guards, 392 Python harness tests (one skipped), and workspace all-target
`cargo check`.
On that same implementation head, hosted CI passed (run 40912531183), Linux
M004 process/IPC and restricted-userns qualification passed (run 40912541817),
and Desktop E2E passed (run 40912552091). The Linux lane exercised the
daemon-owned constrained LSP duplex/procfs path and refusal under the
restricted user-namespace policy; run receipts are linked from the multiworktree
resource artifact.

The corrective pass adds
`rust_analyzer_sustained_tui_desktop_agent_worktrees`, run twice against
rust-analyzer 1.89 on this native ARM64 host. It uses two concurrent
frontend-shaped reads sharing one daemon service per root, separate agent
service scopes per root, three worktrees, managed document changes, a 60-second
active window, a 15-second idle-retention window, RSS/CPU sampling, workspace
disk accounting, and process-teardown assertions. Evidence and dispersion are
in `artifacts/M006-sustained-frontend-agent-workload-20261029.md`. This
qualifies the service/process shape; it does not launch the TUI, desktop
renderer, or agent runtime as separate application processes. The focused
stdio supervisor coverage includes
`real_server_restart_discards_an_inflight_old_generation_response`, where the
old child is replaced while its request is outstanding, the pending request
fails closed, and the replacement answers. An additional same-process delayed
response race remains unqualified; it is not claimed by this corrective pass.

Admission semantics are intentionally narrower than total process budgeting:
`max_concurrent_starts` bounds launches inside the launcher and
`max_admitted_starts` bounds starts concurrently admitted to that launcher.
Both permits are released when launch returns, so ready servers are not counted
against either bound. With no `lsp_process_admission` config, existing direct
launch behavior remains. Do not describe these settings as an active-server,
RAM, CPU or disk cap.

The implementation adds the opt-in
`lsp_process_admission.max_live_servers: Option<usize>` setting (default
`null`; zero rejected). One daemon-owned semaphore is retained by FullHost
dependencies and forwarded into each Linux constrained turn scope. The budget
launcher reserves before invoking a child launcher, keeps ownership across
caller cancellation, transfers the permit with the child, and returns capacity
only after successful wait/reap. Failed reaping quarantines the permit. A
replacement generation requires its own permit while the old generation is
still alive; saturation returns `LiveProcessBudgetExhausted` immediately and
does not stop or evict an attached server. The root error maps this refusal to
HTTP 429. With no configured `max_live_servers`, existing unbounded live-child
behavior remains.

Local lifecycle coverage now includes concurrent FullHost/constrained-scope
launcher competition at a one-child limit, immediate typed refusal, launch
cancellation before a child is returned, cancellation after a child starts,
crash exit/reap, overlapping restart generations, and daemon shutdown/reap.
The focused `egglsp` lifecycle unit group passed (6/6); production stdio tests
for after-spawn cancellation and shutdown passed (2/2). These tests establish
count ownership and cleanup behavior, not a portable RSS/CPU bound. Exact-head
hosted CI, Linux M004 process/IPC and restricted-userns qualification, Desktop
E2E, and the root quick gate remain required before M006 can be marked closed.

The sustained workload provides two host-local observations but does not
establish a universal RAM/CPU limit, separate native TUI/desktop/agent
application-process scale, or Windows constrained-LSP execution. Windows
remains explicitly unavailable because no filesystem-enforcement backend is
registered. Automatic idle retirement is a recorded non-requirement for this
milestone; services retain idle processes until scope teardown or authorized
stop. M005's bounded structural-selection slice is closed separately; broader
Helix M014 syntax-node text objects are outside M006 and do not block its
closure.

## Non-goals / closure

No globally shared LSP process across distinct roots or sandbox profiles; no new language-server binary download policy, code execution from frontend, scheduler changes, build-cache reconfiguration, persistence schema or untrusted config overrides. Close only with approved admission policy, invariant/negative tests, benchmark evidence and no introduced cold-read stall. Closure `plans/closure/daemon-lsp-frontend-convergence/006-status.md` must contain exact configuration semantics, tests, platform matrix and known unqualified modes. If resource tracking would require a new host-agent service, defer to a separate roadmap instead of expanding this milestone.
