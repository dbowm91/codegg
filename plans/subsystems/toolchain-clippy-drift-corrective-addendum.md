# Stable-Toolchain Clippy-Drift Corrective Addendum

Status: active

Repository planning baseline: `f3886890`

Controlling process:

- `plans/003-planning-process.md#7-corrective-passes`

Triggering evidence:

- Hosted run `36866669727` on `7b509faf` — failed at Workspace Clippy
  (`cargo clippy --workspace --all-targets --locked -- -D warnings`).
- Classified by retrieval-signal closure corrective C001 as unrelated to the
  retrieval-signal line (see
  `plans/closure/tool-selection-advisor-retrieval-signal-closure-corrective/001-status.md`):
  the failure is stable-toolchain drift (local 1.89.0 passes; hosted 1.99.0
  fails on the new `clippy::double_must_use` lint), and every flagged site is
  an `#[async_trait]` trait definition in `crates/egglsp`,
  `crates/codegg-providers`, or `crates/codegg-core` — crates untouched by
  retrieval-signal M002 (`eff890ec`) / M003 (`2df9a5f7`).

## 1. Purpose

Restore repository-clean default-feature Workspace Clippy under current stable
Rust without changing behavior, scores, or contracts: the `#[must_use]`
attributes flagged by the new lint are injected by the `async-trait` 0.1.89
macro expansion (`expand.rs`), not authored in repository source, and the
desugared `Pin<Box<dyn Future>>` return is already `#[must_use]`. The lint is
therefore a true positive about redundancy with zero behavioral content, and
the minimal coherent fix is a targeted suppression at the two affected trait
definitions.

## 2. Corrective scope

One milestone:

- `plans/implementation/toolchain-clippy-drift-corrective/001-async-trait-double-must-use-suppression.md`

Status: ready.

C001 must:

- add `#[allow(clippy::double_must_use)]` with a drift-explaining comment to
  all 35 affected `#[async_trait]` trait definitions: `LspEvidenceProvider`
  (`crates/egglsp/src/evidence_collector.rs`), `Provider`
  (`crates/codegg-providers/src/provider_core.rs`), 14 store/callback
  traits in `crates/codegg-core` (`agent_convergence`, `agent_run`,
  `agent_run_control`, `agent_run_group`, `jobs`, `jobs::schedule` x2,
  `projection_replay::artifact_registry`, `run_store`, `session::models`,
  `tool_program::interpreter`, `workspace`, `worktree_service` x2), and 19
  traits in the root `codegg` crate (`agent::turn_runtime`, `hooks`,
  `job_dispatcher`, `permission::reviewer` x2, `security::lsp_executor`,
  `security::workflow::context` x2, `context::artifact`, `core`,
  `core::provider_connections`, `plugin::runtime`, `scheduler::eggwork`,
  `scheduler::executor` x2, `search::types`, `tool_advisor::training_data`,
  `tool`, `tts`);
- scope a `#[allow(deprecated)]` on `Gauge::dec` (`src/util/metrics.rs`):
  stable 1.99 renamed `AtomicU64::fetch_update` to `try_update`, which does
  not exist on the 1.89 MSRV floor, so the old name stays with a revisit
  note rather than a rename;
- verify the suppression silences 1.99.0 and is inert on the 1.89.0 floor
  (no `unknown_lints` breakage under `-D warnings`);
- change no logic, scores, schemas, or public contracts;
- obtain green hosted `CI / verify` on the fix head.

## 3. Invariants

This corrective MUST NOT:

- change retrieval scoring, representation, fusion, K, gates, labels, or
  candidate universes;
- retrain or fine-tune any model or freeze any advisor artifact;
- alter `ResolvedToolSurface` authority or advisor default-off/local-only behavior;
- widen into a toolchain pin, dependency bump, or CI-command policy change;
- rewrite any historical closure record.

## 4. Completion definition

C001 closes only when:

- all hosted `double_must_use` errors are resolved with no new diagnostics;
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passes on
  both the 1.89.0 floor and current stable (1.99.0);
- `scripts/verify.sh quick` passes;
- hosted `CI / verify` is green on the fix head;
- additive closure record is committed.
