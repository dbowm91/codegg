# Developer Harness Lab Roadmap

Status: closed — M001–M005 closed

Long-term references:

- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related existing architecture: `architecture/acp.md`, `architecture/core.md`, `architecture/agent.md`, `architecture/run_store.md`, `architecture/projection.md`, `architecture/tool_broker.md`.

## 1. Purpose and ownership boundary

Build a **development-only, external Python controller** to exercise real CodeGG agent turns through the existing ACP/daemon boundary; record full-tool-use performance; and, after a reliable baseline, coordinate bounded autonomous diagnosis, candidate patches, Rust build/test, retesting and acceptance. Optimize the **agent-facing harness** (tool availability/usability, recovery, context, scheduling interaction, ACP behavior), not product breadth, UI polish, arbitrary Rust code quality, or model weights.

Python owns experiment scheduling, budgets, process supervision, task fixtures, result aggregation, candidate selection, provenance and stop/rollback. The CodeGG daemon continues to own sessions, turns, provider dispatch, scheduler, permissions, tools, audit and durable run events. ACP remains only a frontend adapter. Candidate versions do not control their own evaluation or controller.

Run on the user's trusted, separate **development workstation**. **Do not require a VM, container, network namespace, or a special isolation backend.** Unattended execution is a hard requirement: explicitly select `yolo` approval with either `full-host` (default for faithful full-capability measurement) or `workspace-write` (opt-in comparison). These are different profiles, not equivalent levels of full capability. An explicit deny or unavailable capability must remain effective and observable. Never implement blanket automatic permission responses by suppressing real denials.

## 2. Work classification

- **Invariant:** candidate-specific daemon/session/run correlation; no accidental attachment to a user's production daemon; no cross-candidate persistence or worktree mutation; no false claim of full tool authority; no false-pass terminal or benchmark result; controller-owned stop and acceptance.
- **Capability:** run two or more concurrent CodeGG daemon+ACP subjects, each with explicit binary identity, model, policy, task and workspace; replay and compare cases; optionally drive an OpenCode proposer.
- **Infrastructure:** Python supervisor, instance manifest, bounded event collector, SQLite/JSONL experiment ledger, fixture verifier adapters, candidate build manager.
- **Polish:** reports, dashboards and wider corpus formats only after the correctness loop is qualified.

## 3. Non-goals

- No standalone ACP-specific runtime, second daemon scheduler, broad production multi-daemon mode, release architecture, installed Python dependency, SaaS controller, default external telemetry, VM/container setup, unrestricted CI provider-backed tests, SBC startup/build benchmarks, arbitrary self-modifying controller, automatic push/merge, or modifications to long-term normative documents.
- No claim that separate daemon locks alone isolate all storage: provider/config/state/artifact paths must be enumerated and tested.
- No migration of OpenCode's execution internals into CodeGG; OpenCode is an **optional external stable proposer**. A manual/no-proposer workflow must work.

## 4. Current state and verified seams (repository main `f37840ef5fca36d44b35daec5da6b26cd049a3ee`, 2026-10-18)

- `src/acp.rs`: stdio ACP v1, `initialize`, session new/load/resume, prompt/cancel/close, daemon `TurnSubmit`, session projection tool notifications, one active prompt per connection. The ACP CLI has no explicit approval/sandbox flags. `TurnFailed` is currently reported with ACP `stopReason=cancelled`, losing failure identity.
- `src/core/instance.rs` + `crates/codegg-client/src/paths.rs`: `CODEGG_DAEMON_HOME` selects per-root lock/metadata/socket/log; `CODEGG_CORE_ENDPOINT` exists for endpoint resolution, but ACP currently attaches through `ConnectOrStartOptions::for_default_paths()` and does **not** directly use explicit endpoint resolution. Use **one private `CODEGG_DAEMON_HOME` per subject** first. Do not assume setting `CODEGG_CORE_ENDPOINT` alone makes ACP attach to it.
- `crates/codegg-core/src/storage/paths.rs`: `CODEGG_DATA_HOME` can affect catalog storage. `crates/codegg-config/src/paths.rs` resolves global/system/project configs and `CODEGG_TUI_CONFIG` as one layer, not a complete isolated-config guarantee. Other provider/credential/plugin paths may still refer to the user's home. Explicitly audit and isolate/readonly-share these; never silently run two candidate daemons against the same mutable DB.
- `src/main.rs`, `src/exec.rs`, `src/policy_surface.rs`: root/exec support `--approval-mode yolo` and `--sandbox full-host|workspace-write`; `exec` is a separate in-process path, not the primary benchmark subject.
- `architecture/run_store.md`, `architecture/projection.md`: existing structured run and projection evidence; ACP's public events alone do not provide full token/cost/retry/permission/recovery attribution.
- `src/agent/loop.rs`: execution limits exist but the external controller must own global budgets and independent wall-clock termination. Current ACP transport test is initialization/shutdown only.
- OpenCode: officially documents `opencode run --format json --auto`, `opencode acp`, and `opencode serve`/HTTP SDK. `--auto` approves Ask but honors explicit Deny. Pin supported CLI version and inspect event formats before relying on them.

Research references: https://docs.opencode.ai/docs/cli/ ; https://docs.opencode.ai/docs/permissions/ ; https://docs.opencode.ai/docs/acp/ ; https://docs.opencode.ai/docs/server/ ; https://agentclientprotocol.com/ ; https://arxiv.org/abs/2505.22954 ; https://arxiv.org/abs/2606.17546 ; https://github.com/harbor-framework/harbor .

## 5. Target architecture

```text
developer command
  -> dev/harness_lab Python controller (stable; owns budget + policy + ledger)
      -> subject lane A: pinned CodeGG binary A, private daemon root A,
         private mutable data/config, own ACP process/session/workspace
      -> subject lane B: pinned CodeGG binary B, private daemon root B,
         private mutable data/config, own ACP process/session/workspace
      -> verifier: independent task assertions, baseline/holdout comparisons
      -> optional OpenCode proposer: runs on candidate worktree only
          -> patch -> build -> qualification -> candidate trial -> accept/reject
```

All subjects run with the *actual* native tool runtime. Python's target transport is ACP v1, not a bespoke command server. The supervisor may use daemon status/core identity probes for binding verification; it must not parse stale metadata as proof of a live identity. Each subject has immutable experiment/candidate/build/daemon identifiers and a separate artifact root. The controller records the exact binary hash and Git revision, machine/resource limits, agent/model/provider configuration fingerprint, effective policy and test-case revision.

## 6. Dependency graph

```text
M001 multi-daemon process + state isolation ─┐
                                            ├── M003 measured ACP pilot + evaluator
M002 ACP headless policy + outcome contract ─┘                |
                                                              v
                                                M004 optional OpenCode proposer
                                                and bounded build/retest loop
```

M001 and M002 have **independent hard dependencies** on existing code and can be implemented in parallel, using the interface contract that a lane identifies the exact binary, private daemon root, requested/effective policy, and session/turn IDs. M003 has **hard** dependencies on M001/M002 closure. M004 has **hard** dependency on M003; OpenCode CLI experimentation before M004 is a **soft**, exploratory dependency.

## 7. Milestones

### M001 — Concurrent subject-lane supervisor

Class: infrastructure/capability. Plan: `plans/implementation/harness-lab/001-concurrent-daemon-subject-lanes.md`. **Closed** (`plans/closure/harness-lab/001-status.md`). Two live private daemon lanes passed identity, separation, cleanup and no-default-daemon-mutation checks.

### M002 — ACP unattended policy and truthful terminal semantics

Class: invariant/capability. Plan: `plans/implementation/harness-lab/002-acp-unattended-execution-contract.md`. **Closed** (`plans/closure/harness-lab/002-status.md`). Local native ACP trials proved explicit workspace-write/full-host operation, retained Bash denial and truthful terminal reporting.

### M003 — Baseline assessment, telemetry and reproducibility

Class: infrastructure/capability. Plan: `plans/implementation/harness-lab/003-trajectory-evaluation-and-ledger.md`. **Closed** (`plans/closure/harness-lab/003-status.md`). The 12-case native ACP suite, verifier receipts, append-only ledger, replay and paired comparison are implemented; no performance improvement is claimed.

### M004 — Optional OpenCode proposer and bounded improvement cycle

Class: capability. Plan: `plans/implementation/harness-lab/004-opencode-proposer-and-candidate-gates.md`. **Closed** (`plans/closure/harness-lab/004-status.md`). Candidate-only proposer, qualification gates, and bounded compile-correction retries are implemented, and every evidence condition is in the record: an accepted correctness-fix trace, a rejected trace (a well-formed candidate that changed no measured outcome), an interrupted qualification resumed without duplicate receipts, and a live compile-correction trace whose correction was authored by an authenticated proposer from the compiler's own diagnostic. No automatic merge/push.

### M005 — Multi-iteration controller and agent pilot

Class: capability. Plan: `plans/implementation/harness-lab/005-optimization-controller-and-agent-pilot.md`. **Closed** (`plans/closure/harness-lab/005-status.md`). Implements a bounded outer assess/propose/build/retest scheduler, restartable immutable run checkpoints, candidate partial-trial validation, and ACP usage/tool-selection telemetry. The paired pilot ran on the frozen Task 2 fixture with one binary, one policy and several allowed routes, each passing the fixture's independent verifier; the receipts are in `plans/closure/harness-lab/evidence/pilot-pair-ranges-20261022.json`. The usage-accounting condition is discharged by the controller-enforced budget, and cost stays `null` with an explicit reason where no route reports it. No automatic merge/push.

## 8. Cross-cutting requirements

### Storage and migration

All experiment state stays in `dev/harness_lab/` and a user-selected **ignored external experiment output root**, not normal daemon/run schemas. SQLite plus append-only JSONL/atomic manifests is sufficient. No production DB migration. Ensure per-lane daemon, catalog, session, scratch, caches, and logs cannot silently share mutable storage; redact provider secrets. Explicit optional shared **read-only** credentials with documented provenance may be allowed.

### Protocol and compatibility

Keep ACP v1 compatible with existing clients and v1 `stopReason` enumeration. If needed, add backward-compatible optional namespaced metadata or a controller-only native side channel. Never emit unrecognized stop reasons as standard ACP. A feature/capability handshake must distinguish unsupported from disabled and from authorized-but-failed.

### Security and authorization

The trusted dev workstation is the trust boundary. `full-host` grants the subject the OS user's rights; workspace-write is weaker and subject to actual platform enforcement, not a guarantee for every tool backend. Unattended operation does not imply eliminating explicit denies, principal checks, broker admission, or audit. Do not accidentally select full-host on ordinary codegg launches. No VM requirement.

### Concurrency, cancellation and recovery

The controller owns external timeouts and signals cancellation, waits a bounded grace interval, then terminates only processes it created and positively identifies. Never kill a same-name PID or clean another instance's root. A crash/timeout is recorded as a failed trial, not silently retried into a pass. Crash-safe resumability must reject stale daemon generations and orphaned trial leases. Concurrent build admission is bounded; no shared mutable Cargo target across incompatible candidates.

### Observability

Record public ACP frames and authorized structured host evidence using a fixed schema; no chain-of-thought or credential leakage. Correlate experiment -> candidate -> subject -> daemon generation -> session -> turn -> tool/run. Separate raw tool outputs (bounded, redacted) from model-visible projections and use content digests for provenance. Record missing metrics as unavailable, not zero.

## 9. Verification strategy

- Python unit/contract tests with fake ACP responders and fake daemon processes: silent hang, malformed JSON, duplicated terminal, delayed/stale response, permission ask, provider error, incorrect daemon generation, repeated cancellation, reattachment and interrupted resume.
- Real native two-daemon/two-ACP end-to-end on supported Unix workstation with explicit per-lane directory/DB/session isolation, distinct binary/generation proof and one no-model tool/lifecycle smoke each.
- Focused Rust ACP tests (`cargo test -p codegg --lib acp`, `cargo test -p codegg --test acp_stdio`) and policy/daemon tests; `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `./verify.sh quick` when applicable.
- Optional, manual/provider-backed suites never mandatory for ordinary CI; label all live-model outcomes as such. Pair repeated model trials across versions and report actual denominators, errors and costs.
- Negative proof: kill one daemon during a prompt and verify the other continues; confirm controller remains alive and does not attach to default production daemon.

## 10. Risks and decisions

No new ADR is necessary solely to use **already-supported per-root daemon isolation** from a developer process; if an implementation proposes changing the ordinary per-user singleton guarantee or daemon storage contract, stop and raise an ADR instead. Do not infer daemon isolation from socket uniqueness alone. ACP permission parity must reuse daemon authority; a new ACP-only bypass would require redesign, not acceptance. Unknown current OpenCode version/ACP SDK API behavior requires a version-pinned smoke fixture, not a guessed interface.

## 11. Completion definition

Two independent CodeGG versions can run headlessly and concurrently via ACP on the trusted workstation, with auditable no-Ask policy, correct fail/cancel status, strictly separate mutable runtime state, replayable multi-tool trajectories, controller-owned budgets, and an optional OpenCode-driven edit/build/test/compare/revert iteration. Documentation must distinguish deterministic, provider-backed and unrun evidence. No milestone closes without its own closure record.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure | Blocker |
|---|---|---|---|---|
| M001 | closed | `implementation/harness-lab/001-concurrent-daemon-subject-lanes.md` | `closure/harness-lab/001-status.md` | — |
| M002 | closed | `implementation/harness-lab/002-acp-unattended-execution-contract.md` | `closure/harness-lab/002-status.md` | — |
| M003 | closed | `implementation/harness-lab/003-trajectory-evaluation-and-ledger.md` | `closure/harness-lab/003-status.md` | — |
| M004 | closed | `implementation/harness-lab/004-opencode-proposer-and-candidate-gates.md` | `closure/harness-lab/004-status.md` | — |
| M005 | closed | `implementation/harness-lab/005-optimization-controller-and-agent-pilot.md` | `closure/harness-lab/005-status.md` | — |

## 13. Post-pilot corrective successor (2026-10-21)

The original M001–M005 closure records remain strictly closed. The developer-only reliability, scored improvement, real OpenCode acceptance and sustained pilot gaps were registered as **C001–C004** in `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md` with independently executable plans in `plans/implementation/harness-lab-post-pilot-corrective/`.

C001 (failed-turn evidence / enforceable budget admission) and C002 (objective scoring / end-to-end interruption recovery) are **closed**, with independent closure records in `plans/closure/harness-lab-post-pilot-corrective/001-status.md` and `002-status.md`. C003 (real proposer and accepted defect) is **closed**: its live half ran, an authenticated OpenCode proposer corrected a genuine reproducible defect, and the paired score accepted the candidate with the objective `correctness_fix` and no regression (`plans/closure/harness-lab-post-pilot-corrective/003-status.md`). C004 (Task 2 and sustained tool-rich pilot) is **conditionally closed**: its offline preparation is delivered (a versioned opt-in benchmark taxonomy with a strict loader, a verifier-isolation guard, and **all six supported cases materialized** with fixture and verifier bodies that are each proven to fail unsolved, pass solved and refuse an edited frozen file), its live half ran (Task 2 solved, a 24-trial sustained scenario inside its declared caps, and the provider pair completed on the frozen fixture with three allowed routes), and the paired baseline/candidate comparison over the taxonomy **ran live** and returned **`rejected`** — correctness is 1.0 on both sides and reliability 1.0 on both sides, because both binaries solved all five task cases and the taxonomy exercises the ACP path where C003's headless-adapter fix does not apply. The taxonomy's protected stall-stop control was **never exercised** by a live trial and is recorded as demonstrating nothing. The prior pilot receipt is immutable and was not reinterpreted as full success. No VM/container requirement, no permission prompts for explicitly authorized test subjects, and no automatic Git merge/push are introduced.

This is a successor line, not a retroactive change to the M004/M005 closure evidence. Only independently qualified corrective closure artifacts may advance those conditions.
