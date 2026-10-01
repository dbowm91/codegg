# Stable-Toolchain Clippy-Drift Corrective C001 — `double_must_use` Suppression + `fetch_update` Deprecation

Status: ready for handoff

Repository baseline: `f3886890`

Source corrective:

- `plans/subsystems/toolchain-clippy-drift-corrective-addendum.md`

Triggering classification (owned by retrieval-signal closure corrective C001):

- Hosted run `36866669727` on `7b509faf` fails default-feature Workspace
  Clippy under stable 1.99.0 with `clippy::double_must_use` errors at 16
  `#[async_trait]` trait sites: `crates/egglsp/src/evidence_collector.rs:77`,
  `crates/codegg-providers/src/provider_core.rs:83`, and 14 further sites in
  `crates/codegg-core` (store/callback traits).
- Local 1.89.0 passes; the lint is new in Clippy 1.99.0.
- The flagged `#[must_use]` is injected by `async-trait` 0.1.89 macro
  expansion (`expand.rs:69`), not authored in repository source (zero
  `must_use` hits in either file; both files byte-identical between
  `7b509faf` and the C001 head).
- Retrieval-signal M002 (`eff890ec`) and M003 (`2df9a5f7`) touch only
  `src/tool_advisor/*` + advisor assets; neither touched the flagged crates.

Primary class: polish (toolchain-drift hygiene; no behavior change).

## 1. Objective

Suppress the redundant-`must_use` diagnostic at its two macro-invocation
sites so default-feature Workspace Clippy is green on current stable while
remaining green on the 1.89.0 floor.

## 2. Required production changes

Sixteen mechanical edits across three crates, each adding one
`#[allow(clippy::double_must_use)]` attribute plus a brief drift comment
directly above the affected trait definition:

1. `crates/egglsp/src/evidence_collector.rs` — `LspEvidenceProvider`.
2. `crates/codegg-providers/src/provider_core.rs` — `Provider`.
3. `crates/codegg-core`: `ConvergenceStore` (`agent_convergence.rs`),
   `AgentRunStore` (`agent_run.rs`), `AgentRunControlStore`
   (`agent_run_control.rs`), `AgentRunGroupStore` (`agent_run_group.rs`),
   `JobStore` (`jobs/mod.rs`), `ScheduleStore` + `OccurrenceMaterializer`
   (`jobs/schedule.rs`), `ProjectionArtifactRegistry`
   (`projection_replay/artifact_registry.rs`), `RunStore` (`run_store.rs`),
   `SessionSummaryProvider` (`session/models.rs`), `BrokerCallback`
   (`tool_program/interpreter.rs`), `WorkspaceStore` (`workspace.rs`),
   `WorktreeStore` + `WorktreeOwnerResolver` (`worktree_service.rs`).
4. Root `codegg` crate (19 traits): `TurnRuntime` (`agent/turn_runtime.rs`),
   `Hook` (`hooks/mod.rs`), `JobDispatcher` (`job_dispatcher.rs`),
   `ReviewerModelBackend` + `ReviewerInvestigator`
   (`permission/reviewer.rs`), `TypedHunkSourceContextTarget`
   (`security/lsp_executor.rs`), `SecurityContextExecutor` +
   `HunkSourceContextExecutor` (`security/workflow/context.rs`),
   `ContextArtifactStore` (`context/artifact.rs`), `CoreClient`
   (`core/mod.rs`), `ConnectionStore` (`core/provider_connections.rs`),
   `PluginRuntime` (`plugin/runtime/mod.rs`), `EggworkNodeClient`
   (`scheduler/eggwork.rs`), `JobProgressSink` + `JobExecutor`
   (`scheduler/executor.rs`), `SearchProvider` (`search/types.rs`),
   `RemoteTransport` (`tool_advisor/training_data.rs`), `Tool`
   (`tool/mod.rs`), `TtsEngine` (`tts/mod.rs`).
5. `src/util/metrics.rs` (`Gauge::dec`): the same 1.99 toolchain renamed
   `AtomicU64::fetch_update` to `try_update`. The repo MSRV floor is 1.89,
   where `try_update` does not exist, so the fix is a scoped
   `#[allow(deprecated)]` on the call with a revisit note — not the rename.

No logic, signature, schema, or contract change. No new dependency, feature,
test harness, or CI lane.

## 3. Why suppression (not refactor)

- The redundancy is inside `async-trait`-generated code; repository source
  cannot add a `must_use` message or remove the injected attribute.
- Removing `async_trait` would forfeit `dyn`-compatible traits (`Box<dyn
  Provider>` and mock providers) and require a transport-level refactor —
  disproportionate for a lint with zero behavioral content.
- A dependency bump of `async-trait` to dodge the lint is lockfile churn
  with re-verification cost and no contract benefit.
- The CI workflow intentionally floats on stable, so newly stabilized lints
  are expected; targeted suppression at the macro site is the established
  minimal response.

## 4. Required verification commands

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo +stable clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
scripts/verify.sh quick
git diff --check
```

Both Clippy invocations must be fully green (the 1.89 run proves the
suppression name does not trip `unknown_lints` under `-D warnings`).

## 5. Hosted CI

Push the fix commit and require ordinary `CI / verify`. Record run id,
commit SHA, Workspace Clippy result, and final conclusion in the closure
record.

## 6. Acceptance criteria

- Zero `double_must_use` errors on stable 1.99.0; no new warnings on either
  toolchain.
- `scripts/verify.sh quick` passes.
- Hosted `CI / verify` green on the fix head.
- Closure record at
  `plans/closure/toolchain-clippy-drift-corrective/001-status.md`.

## 7. Stop conditions

Stop and escalate rather than improvise if:

- the suppression does not silence the lint (macro-site `allow` interacts
  differently than measured);
- either toolchain reports new diagnostics beyond the 17 known errors;
- fixing Clippy would require behavior/contract changes.

## 8. Handoff notes

Pre-verified on this branch head: the exact attribute change yields exit 0
on both `cargo clippy` (1.89.0) and `cargo +stable clippy` (1.99.0) for the
affected crates. Full-workspace verification runs at implementation time.
