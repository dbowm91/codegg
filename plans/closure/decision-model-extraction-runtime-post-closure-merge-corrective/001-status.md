# Decision-Model Extraction and Runtime Post-Closure Merge Qualification C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/decision-model-extraction-runtime-post-closure-merge-corrective/001-final-hosted-ci-and-merge-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/decision-model-extraction-runtime-post-closure-merge-corrective-addendum.md`
- Predecessor: `plans/subsystems/decision-model-extraction-runtime-roadmap.md` (M001-M006 closed)

Repository baseline reviewed: `291a361d402c7062a713278e8e5ef3a840cee884`

Implementation commits or pull requests:

- `9bfc07a0b87482b6a145de9965cf421f0c6e2b07` — rebased decision-runtime implementation, lint correction, and hosted qualification branch head.
- PR [#104](https://github.com/dbowm91/codegg/pull/104) — `codex/decision-model-extraction-runtime` to `main`.
- Final qualified base: `13a57c251f1fb0816d723f750048bed385de7e09`.

## 1. Executive finding

C001 is complete. The implementation branch was reconciled with current `main`, its ownership and dependency boundaries were verified, the exact pinned SDM compatibility suite passed, local correctness checks passed, and the required hosted workflows passed. The qualified implementation head is `9bfc07a0b87482b6a145de9965cf421f0c6e2b07`; the base used was `13a57c251f1fb0816d723f750048bed385de7e09`. **Disposition: closed — merge recommended.**

The PR remains unmerged by this closure; C001 grants the merge recommendation and records qualification evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reconcile with current `main` | Rebase from `533be594` to `13a57c251f1f`; final implementation head `9bfc07a0` | pass | One semantic conflict resolved in `src/server/ws.rs`; retained newer mainline bounded snapshot-bundle behavior. |
| Preserve ownership and dependency boundary | Cargo feature trees, source/manifest searches, exact SDM pin inspection | pass | Default graph is free of SDM/Candle; opt-in graph includes pinned `sdm-core` and `sdm-runtime`, not `sdm-training`. |
| Local correctness | Formatting, workspace Clippy, quick verification, workspace and supported-feature Nextest, focused projection transport tests | pass | See §4 for exact scope and counts, including earlier broad workspace evidence. |
| Reproduce pinned SDM compatibility | `scripts/check_sdm_compatibility.sh /tmp/sdm 8139b064bdcf3212e8f6fd912e801a479b55751c` | pass | 12 frozen fixtures; 11 valid requests executed; artifact digest recorded in §7. |
| Hosted CI on implementation head | GitHub `CI / verify`, run `37419090787` | pass | Exact head `9bfc07a0b87482b6a145de9965cf421f0c6e2b07`. |
| Other triggered hosted workflow | Desktop E2E, run `37419090740`, attempt 2 | pass | Attempt 1 hit a 10-second timeout in an unchanged projection watcher test; same-SHA rerun passed host checks and built-app WebDriver trajectory. |
| No unresolved medium/high finding | Review of final diff, conflict resolution, dependency graph, local and hosted results | pass | No open correctness or security findings. |

## 3. Production implementation evidence

The implementation establishes the ADR-0013 ownership boundary: CodeGG owns the bounded decision request/response contract, adapters, candidate authority, deterministic fallback, promotion/disclosure policy, causal-frontier behavior, capture/export consent, and operator configuration. Generic model training and reusable artifact runtime are owned by the external MIT repository `dbowm91/sdm`, pinned by CodeGG at `8139b064bdcf3212e8f6fd912e801a479b55751c` behind the opt-in `decision-runtime-sdm` feature.

Default operation remains off and model-free. The SDM smoke artifact remains unqualified for promotion; System One Rank remains explicitly unsupported; no implicit model download or network activation is introduced. C001 made no production architecture or decision-policy change. The sole narrow source correction after initial hosted CI was the explanatory `#[must_use]` annotation on the async `DecisionEngine::decide` future, required by the hosted Rust/Clippy version.

## 4. Verification executed

### Commands run

```bash
git fetch origin
git rebase origin/main
git diff --check origin/main...HEAD
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run --workspace --locked --profile ci
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci
cargo check --locked -p codegg --features decision-runtime-sdm
scripts/check_sdm_compatibility.sh /tmp/sdm 8139b064bdcf3212e8f6fd912e801a479b55751c
scripts/verify.sh quick
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci --test projection_transport_real
cargo clippy -p codegg-core --all-targets --locked -- -D warnings
cargo tree --locked -e normal
cargo tree --locked --features decision-runtime-sdm -e normal
rg -n 'candle|tool-advisor-training|tool-advisor-encoder' Cargo.toml Cargo.lock src crates
rg -n 'sdm-training' Cargo.toml Cargo.lock
cargo run --locked --bin codegg -- tool-advisor status --json
```

### Results

| Check | Result |
|---|---|
| Workspace fmt | Passed. |
| Workspace Clippy | Passed before the final narrow annotation; targeted `codegg-core` Clippy and `scripts/verify.sh quick` passed after it. Hosted `CI / verify` passed on the final implementation head. |
| Workspace Nextest | 12,282 passed / 5 skipped on the pre-rebase implementation; repeat full sweep passed after an isolated transient test failure. |
| Supported-feature Nextest, exact rebased implementation source | 9,427 passed / 5 skipped in 1,144.075 seconds. |
| Focused conflict-surface suite | `projection_transport_real`: 58 passed. |
| SDM feature check | Passed. |
| SDM frozen compatibility | 12 fixtures validated; 11 runtime requests executed; passed. |
| Quick verification | Passed after final source annotation. |
| CLI default status | `policy_mode: off`, backend `noop`, state `Off`, no capabilities, `promotion_qualified: false`. |
| Diff whitespace check | Passed. |

An earlier supported-feature sweep on the same implementation lineage completed at 9,426 passed / 5 skipped. The exact post-rebase rerun above is the final supported-feature result. The default workspace sweep count is retained from the prior local run; final-head hosted workspace verification also passed.

## 5. Invariant review

- **Default-off/no-network:** the CLI reports `Off` with the noop backend, no capabilities, and no promotion qualification. No default SDM dependency is present.
- **Opt-in SDM:** runtime is behind `decision-runtime-sdm` and locked to the exact compatible revision.
- **Training ownership:** neither default nor opt-in runtime dependency graph includes `sdm-training`; Candle and retired advisor training/encoder manifests and implementation are absent from active runtime paths.
- **Candidate authority:** decision adapters operate only over CodeGG-provided bounded candidate surfaces; hosted CI and core decision tests pass.
- **Promotion:** the baseline smoke artifact remains unqualified; status command reports `promotion_qualified: false`.
- **System One:** remains opt-in; Rank does not synthesize from Choice and is explicitly unsupported.
- **Causal frontier and consent:** remain CodeGG-owned and independent of backend policy; their focused regression and consent/export coverage passed in the final hosted workspace suite.
- **No implicit downloads:** no runtime path or dependency change adds model acquisition.
- **Historical evidence:** M001-M006 closure records and assets were not rewritten.

## 6. Failure and recovery review

The rebase conflict occurred in `src/server/ws.rs`. Resolution retained current `main`'s `fold_session_snapshot_bundle(snapshot, &descriptor)` behavior, including bounded-session handling and rejection of LSP diagnostics in the TUI snapshot envelope. Focused `projection_transport_real` tests passed 58/58; the complete supported-feature run passed 9,427/9,427 executed tests.

Hosted Desktop E2E attempt 1 failed only at `projection::tests::watcher_coalesces_rapid_diagnostics_to_latest` with a 10-second request timeout. The test file had no diff against current `main`, and the failure showed transient timing behavior. Per the plan's same-SHA rerun policy, failed jobs were rerun without a source change; attempt 2 passed the host checks and built-app trajectory. Root hosted CI run `37419090787` passed. A prior root CI attempt on an earlier head exposed `clippy::double_must_use`; it was corrected with an explicit must-use message and a complete fresh hosted run then passed.

No applicable persistence, lease, or migration behavior was changed by C001.

## 7. Migration and compatibility review

No schema, protocol version, or stored artifact migration was introduced by C001. The SDM pin remains `8139b064bdcf3212e8f6fd912e801a479b55751c`; the checkout used for compatibility was clean at that exact revision. Compatibility passed against the frozen 12-fixture contract, with 11 runtime executions and artifact digest `sha256:b9484ce8b8afe2b345693e084468c89a628462e86faf1e9683c28ee53de6597e`. No fixture was changed to obtain the pass.

## 8. Security review

C001 did not alter authorization, secret handling, path validation, tool authority, artifact loading, or network policy. Runtime remains opt-in; candidate authority stays CodeGG-bounded; promotion remains unavailable to the unqualified smoke artifact; generic training remains outside the runtime dependency graph. Hosted guards and the workspace suite passed.

## 9. Documentation and operations

This closure closes C001 in the implementation plan, corrective addendum, and registry. The predecessor decision-runtime roadmap remains closed at M001-M006. The external SDM pin and compatibility command are recorded above; no architecture or operator contract needed revision.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | Desktop E2E's first same-SHA attempt had a 10-second timeout in an unchanged diagnostic watcher test. | Runner timing caused one transient red attempt; the retry passed the same test and full triggered workflow. | None; retain both attempt outcomes in this closure record. |

No medium or high findings remain.

## 11. Roadmap disposition

**C001 closed — merge recommended.** M001-M006 remain closed. PR #104 is qualified for merge against base `13a57c251f1fb0816d723f750048bed385de7e09`, with hosted CI green on exact implementation head `9bfc07a0b87482b6a145de9965cf421f0c6e2b07`. This closure is a recommendation; no merge was performed.

## 12. Registry updates

- `plans/registry.md`: mark the C001 corrective addendum and dependency-ready implementation plan closed; link this closure record and hosted evidence.
- Corrective addendum: status closed; C001 closed with merge recommended.
- Implementation plan: status closed; link this closure record.
- Predecessor roadmap: remains closed; M001-M006 are unchanged.
