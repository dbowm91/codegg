# M006 — Daemon LSP Process Budgets, Worktree Identity and Retirement

Status: ready — maintainer accepted bounded-admission policy defaults on 2026-10-28; implementation qualification remains

Primary class: infrastructure/polish.

Repository planning baseline: `80171f45295bebb9ab0b521262c0b295acb93074` (`main`, 2026-10-27). Reinspect live branch before implementation. Roadmap: `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md`; governance: `plans/003-planning-process.md`; long-term specification §§4,13,24–25.

Relevant accepted ADRs: ADR-0010–0012, ADR-0021; Linux M004 process-scope boundary under `src/lsp/m004.rs`; shared checkout/worktree policy and writer fencing are separate authorities. ADR-0056 concerns operational commands, **not** permission to merge restricted and host processes.

## Objective

Bound redundant LSP indexing and resource consumption when TUI, desktop and agent sessions share a checkout, while retaining correct isolation for distinct worktrees and sandbox profiles. Implement safe process admission/retirement with measured benefit and no unbounded memory/CPU spikes under many Rust worktrees.

## Evidence and policy gate

`crates/egglsp/src/service.rs` keys clients as canonical root/server ID and coalesces starts per key; `src/lsp/m004.rs` creates per-turn scoped LSP services. Cross-worktree process reuse is *not* established. `crates/codegg-core/src/process_admission.rs` names `LanguageServer` as a process kind; `src/tool/process_capability.rs` marks `lsp` and `lsp_read` persistent. Production socket/stdio/HTTP constructors now inject a daemon-owned `LspService`; same-root reuse is scoped to that service instance, whose config and launcher are immutable. The full current-head audit is `plans/implementation/daemon-lsp-frontend-convergence/artifacts/M006-process-identity-audit-20261027.md`.

The maintainer accepted these policy boundaries on 2026-10-28: keep present concurrency unchanged by default; add an opt-in bounded admission budget; never retire or evict an attached editor server; include canonical worktree root, server/config fingerprint and execution profile in compatibility identity; and expose only bounded, redacted operational counts. Admission limits and queue saturation must produce typed, cancellable outcomes. Any idle retirement must prove there are no attachments, in-flight requests or open documents before stopping a process. No shared LSP process may cross worktree roots or execution profiles.

## Work packages

1. Audit every LSP spawn/restart/cancel path for FullHost daemon service, model-turn LSP, Linux constrained scope, standalone TUI legacy path and each platform; map which actually shares process identity and which must remain isolated. Audit profile, canonical worktree/root, server id, argv/env/config, trust and compatibility fingerprint. A new key must be stable and not leak secrets, and callers must never supply arbitrary root/profile.
2. Add telemetry-free process lifecycle counters/measurements (active, starting, ready, failed, generation, queued, indexing, memory/CPU evidence when OS offers it). No raw source text/secret stderr in durable projections.
3. Introduce bounded *daemon-owned* start admission: concurrency cap, deterministic queue/coalescing per compatible key, optional workspace/global active limit, strict cancellation/timeout, no stalled warm-only editor read. Never kill an attached server without a defined user-visible `NotReady` transition and snapshot/replay semantics. Prefer explicit finite idle timeout and LRU policy only after acceptance. Constrained scope lifetime remains turn-bounded.
4. Use the existing managed process-launch helper and lifecycle cleanup, respecting the Linux sandbox helper and platform-specific admission. Keep a separate worktree as a separate semantic root even when Cargo build output is shared. Do not use `CARGO_TARGET_DIR` tricks or Rust dependency caches as a reason to reuse one language server across incompatible roots.
5. Test process admission under parallel agent substreams, two editors, shared checkout and separate worktrees, resource saturation, restart storms, cancelled startup, server crash and daemon shutdown. Surface bounded operator statuses through M003 if available; do not make M003 a hard correctness dependency on process accounting.

## Verification

Run `cargo test -p egglsp --locked`, root `cargo test -p codegg --locked lsp`, `cargo check -p codegg --locked --features server`, `python3 scripts/check_authorization_matrix.py`, relevant managed-process/daemon-workspace static guards and root `scripts/verify.sh`. Native Linux contained LSP exchange must prove host-proc denial; macOS/Windows behavior must be separately qualified or explicitly disabled. Benchmark 1, 2 and 4 simultaneous editors/agents, 1 vs 3 worktrees, warm and cold starts, Rust and Python fixtures; record p50/p95 startup/read latency, number of child processes, peak RAM/CPU and disk-impact evidence with exact machine/model/server versions. No invented target SLO or false host-generalized throughput claims.

Current host qualification: the fake-LSP integration family ran on this
x86_64 macOS host with a compatible linker wrapper (71/71 passed), as did the
full `egglsp` package suite (1,038 tests). These are source correctness
checks, not process-admission qualification. No multi-editor/worktree
benchmark or process-count evidence has been recorded; M006 remains ready for
implementation and active-resource measurement under the accepted defaults.

## Non-goals / closure

No globally shared LSP process across distinct roots or sandbox profiles; no new language-server binary download policy, code execution from frontend, scheduler changes, build-cache reconfiguration, persistence schema or untrusted config overrides. Close only with approved admission policy, invariant/negative tests, benchmark evidence and no introduced cold-read stall. Closure `plans/closure/daemon-lsp-frontend-convergence/006-status.md` contains exact configuration semantics, tests, platform matrix and known unqualified modes. If resource tracking would require a new host-agent service, defer to a separate roadmap instead of expanding this milestone.
