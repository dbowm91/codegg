# Developer Harness Lab M001 — Concurrent Daemon Subject Lanes

Status: active

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (`main`, reviewed 2026-10-18). Implementation must refresh against current branch head.

Source roadmap: `plans/subsystems/harness-lab-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#23-acp-boundary`, `#24-protocol-and-storage-requirements`, `#26-reliability-and-recovery`, `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.

Applicable ADRs: existing daemon/scheduler/session ownership remains authoritative; no new ADR planned. A proposal to change production singleton semantics stops for an ADR.

Primary class: infrastructure (with a demonstrable developer capability)

## 1. Objective

Implement a small, developer-only Python subject supervisor that launches **two or more simultaneously live CodeGG daemon instances**, binds **a separate ACP stdio client** to each correct daemon, proves binary + daemon generation identity, and shuts down only the resources it owns. Use a regular macOS/Linux development workstation; do not provision VMs, containers or network isolation. This milestone runs no paid model benchmark and adds no self-improvement agent.

## 2. Why ready

`CODEGG_DAEMON_HOME` already creates distinct lock, metadata, socket and log roots. `CODEGG_DAEMON_EXECUTABLE` can select a candidate binary in the existing autostart pathway; codegg's daemon commands can manage instances. ACP already calls the native daemon `TurnSubmit` seam. The remaining work is reliable process and **whole-state** isolation plus Python orchestration, not a new production daemon mode.

## 3. Current implementation evidence

- `src/core/instance.rs` uses `DaemonPaths::resolve()` and `ConnectOrStartOptions::for_default_paths()`; `DaemonInstanceGuard` owns the lock and metadata per resolved root. `crates/codegg-client/src/paths.rs` resolves `CODEGG_DAEMON_HOME`, `CODEGG_CORE_ENDPOINT`.
- `src/acp.rs::ensure_client` resolves default paths only. **Do not assume `CODEGG_CORE_ENDPOINT` targets the ACP child**: use lane-unique `CODEGG_DAEMON_HOME` and its default `core.sock` initially, or add an explicit endpoint argument only with native tests.
- `crates/codegg-core/src/storage/paths.rs` resolves catalog `CODEGG_DATA_HOME`; `crates/codegg-config/src/paths.rs` loads `CODEGG_TUI_CONFIG`, global/system and project config. `HOME` and `XDG_CONFIG_HOME` affect other files differently per OS; credential, MCP, state, worktree scratch, sessions, provider and artifact writers need an audited matrix.
- `architecture/core.md` and `docs/daemon.md` describe default singleton; this milestone may use multiple **isolated user-scoped runtime roots** without changing default user behavior.
- `tests/acp_stdio.rs` only checks initialize/shutdown protocol purity; no multi-instance binding test exists.

## 4. Invariants

1. Normal launch and default user daemon remain unchanged. Only the explicit development controller arranges several private instance roots.
2. Each lane's binary, lock, endpoint, daemon generation, catalog DB, runtime config, sessions, artifacts and mutable workspace paths are attributed; no accidental default daemon access and no silent shared mutable stores.
3. ACP stdout is JSON-RPC only, logs on stderr. Child processes never inherit another lane's `CODEGG_DAEMON_HOME`/`CODEGG_DATA_HOME` accidentally.
4. A stale `daemon.json`, PID reuse, socket alias or previously running daemon cannot impersonate the lane's expected process/binary. A live identity probe plus verified process ownership is required. Semver alone is not candidate identity.
5. Process kill/cleanup operates on supervised PID/process group and private root only. No broad `pkill codegg`, no deleting shared home folders.
6. Isolation of **mutable runtime state** is distinct from OS security isolation. Full-host tool calls are intentionally possible later; M001 makes no containment claim.

## 5. Scope

In: `dev/harness_lab/` (or an equivalent clearly dev-only directory); Python 3.11+ stdlib-first supervisor/CLI, minimal pinned optional dependency if needed for ACP schema; lane manifests, child reaping, readiness checks, temp roots and no-model fixtures; updated `architecture/acp.md`, `docs/daemon.md`/developer notes as appropriate.

Out: VM setup, network-policy backend, production daemon manager, new released CLI binary, ACP policy controls (M002), full benchmark schema (M003), OpenCode proposer (M004), Windows live qualification, permanent daemon/profile migration.

## 6. Required changes

### Runtime and process boundary

- Define a `SubjectSpec` and recorded `SubjectIdentity`: experiment/lane ID; absolute candidate binary path and SHA-256; expected Git revision; workspace root; per-lane daemon/home/data/config directories; requested endpoint; launch argv/env fingerprint (secret-safe); live daemon ID/generation/PID/socket; ACP PID; startup timestamp.
- Prefer each lane's private `CODEGG_DAEMON_HOME`, `CODEGG_DATA_HOME` (check whether expected prefix is app-owned), config root, and isolated project fixture. Audit `dirs::config_dir`/data/store, credential/provider, scratch, RunStore and session persistence writers before claiming complete isolation. For optional existing provider authentication, allow *explicit read-only configuration/credential reference*, never sharing mutable daemon catalog.
- Launch pinned candidate daemon with its own environment; use existing daemon `status`/native identity probes, and correlate process identity with the expected binary. Run ACP child with **the same** lane environment so its lazy attach targets the correct daemon. Do not trust `CODEGG_CORE_ENDPOINT` without a focused source-backed attachment test.
- Startup timeout and cancellation are deterministic. Capture bounded stderr/log tails; do not read arbitrary output into unbounded memory. On startup failure, diagnose then clean only this lane.
- Expose commands such as `python -m dev.harness_lab doctor` and `python -m dev.harness_lab smoke --subjects 2` (exact CLI naming may differ) with machine-readable JSON summary and nonzero exit on failed assertions.

### Storage and migrations

No production SQLite migrations; controller metadata may be a small JSON manifest with atomic writes. Persistent experiment output goes under an ignored configurable directory **outside candidate worktrees**. Check existing `.gitignore` conventions.

### Protocol and DTOs

No new native protocol method required unless there is no existing truthful identity probe. Do not add ACP session protocol features to solve test setup at this milestone. Prove normal `initialize`, `session/new`, `session/close`, `shutdown` through both ACP clients.

### Security and authorization

Do not weaken the existing daemon authorization or persisted rules. M001 is deliberately model-free so no unattended approval bypass is necessary. Credential references and env dumps must be redacted; only explicitly authorized paths may be written.

## 7. Work packages

**A — Storage/writer inventory.** Enumerate all resolved mutable paths and environment overrides on macOS/Linux. Produce a short table of supported lane-isolation settings and exceptions; add a negative test for any discovered shared DB/credential writer.

**B — Supervisor and binding contract.** Implement typed spec/identity, launch, probe, ACP start/initialize/session, cancellation, cleanup and bounded logs; pin binary hash and verify daemon attachment. Fail closed on identity disagreement.

**C — Concurrent two-instance smoke.** Start two daemons and two ACP clients at once, create the same-title session in distinct workspaces, prove independent records/generations, kill one daemon and show the other still operates; then clean up only launched children.

**D — Documentation/guards.** Document how a developer runs and stops the lane smoke; add an automated guard or integration assertion against accidental use of the default user daemon home or default DB. Test no background orphan remains.

## 8. Failure, restart and contention

Autostart races must converge within **one lane**, not across lanes. Existing same-lane daemons may only be reused if explicitly allowed and identities match; default is reject. A failed ACP client cannot mark its daemon complete; a dead daemon must cause the correct lane to fail. Controller interruption triggers bounded cancellation and process-group teardown; resumed execution must reject old generations before reusing artifacts. Use short socket roots on macOS/Linux to avoid AF_UNIX pathname length failures.

## 9. Compatibility and migration

No user-facing config or DB migration, no changed default daemon behavior. If code changes to `CODEGG_DAEMON_HOME`/data path resolution are unavoidable, maintain exact compatibility and prove standard single-instance lifecycle unchanged.

## 10. Tests

- Pure Python tests with fake child processes: delayed ready, wrong ID, stale metadata, reused PID, duplicate exit, hung ACP, invalid JSON, one lane kill versus two lane state.
- Real `pytest` or `unittest` subprocess smoke using built `codegg` and two private homes, distinct sockets and DBs; validate live daemon identity, ACP session binding, shutdown and retained default-user daemon invariance. No provider token required.
- Permission/path and cancellation assertions; executable digest mismatch should block binding, not warn.

## 11. Verification commands

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
# After implementation: developer-provided built candidate path and explicit output dir
python3 -m dev.harness_lab doctor
python3 -m dev.harness_lab smoke --subjects 2
cargo test -p codegg --test acp_stdio
cargo test -p codegg --lib acp
cargo fmt --all -- --check
./verify.sh quick
```

If the developer CLI differs, document the actual executed equivalents. Do not claim smoke or CI passing merely because the plan was written.

## 12. Acceptance and stop conditions

Accept only on **live** simultaneous two-daemon/two-ACP proof with different binary/generation-attested identities and distinct mutable stores, negative attachment tests, correct cleanup and zero unexpected default daemon mutations. Stop if a required state path cannot be isolated by supported development overrides, if an ACP instance can attach to the wrong lane, or if external controller cannot prove correct daemon identity. Create a corrective plan rather than allowing a soft assertion.

## 13. Closure artifact

`plans/closure/harness-lab/001-status.md` must include implementation SHA, OS/platform evidence, exact commands and results, state path inventory, two-daemon identity receipts, cross-lane negative results, orphan/cleanup evidence, limitations and independent review of no-production-regression.
