# Decision-Model Extraction and Runtime Milestone 005 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/decision-model-extraction-runtime/005-tool-advisor-decision-engine-migration.md`
Source subsystem roadmap: `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m005--tool-advisor-decisionengine-migration`
Repository baseline reviewed: `a1a02b5c` (implementation head)
Implementation commits: `a1a02b5c` — migrate live advisor ranking to `DecisionEngine`

## 1. Executive finding

Both live learned-ranking paths now use the shared asynchronous `DecisionEngine`. CodeGG owns candidate authority, deterministic shortlisting/fallback, actuation, and promotion policy. Backend capability/state is observable without inference. An unqualified local artifact cannot be promoted by configuration. M005 is closed. Fresh source/dependency inventory is recorded below and in the M006 handoff; M006 is ready.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Agent pre-turn disclosure uses the engine | `src/agent/request_preparation.rs` calls `rank_with_engine`; request preparation tests preserve candidate authority. |
| On-demand tool search uses the engine | `src/tool/tool_search.rs` calls `rank_with_engine`; candidate IDs are checked against the request. |
| One backend snapshot is shared | `src/tool/factory.rs` resolves the backend once and passes the shared engine to `AgentLoop` and `ToolSearchTool`. |
| Failure is bounded and deterministic | Adapter falls back on unsupported Rank, malformed/mismatched response, errors, timeout/deadline, or panic; focused adapter tests passed. |
| System One does not emulate Rank | Its backend advertises no Rank and does not issue repeated Choice requests. |
| Policy mode and backend status remain distinct | `codegg tool-advisor status --json` reports policy, capability, state and promotion qualification without inference. |
| Unqualified artifacts cannot be promoted | Backend resolution exposes `promotion_qualified`; all current backend resolutions are false, and `promote` resolves to `observe` with a warning. |
| Causal-frontier authority and capture consent remain independent | Existing causal-frontier and capture/export paths are unchanged by the migration. |

## 3. Production implementation evidence

The production live callers are the pre-turn path and tool-search path above. The previous synchronous `advisor_from_config` bridge is test-only; `project_preturn_promotions` and `ToolSearchTool::set_advisor` are also test-only. Remaining scorer calls in experiment, contextual, and order-invariance modules are offline research/qualification utilities, not live agent inference paths. M006 inventories and retires unowned legacy implementation without deleting historical evidence or local policy.

Fresh handoff inventory: the Candle references and dependencies are confined to root-package advisor encoder/experiment features and their source modules; no other workspace crate uses Candle. The generic SDM runtime feature graph includes `sdm-core` and `sdm-runtime`, not `sdm-training`; the default graph has no SDM packages. Historical result/receipt/preregistration assets are evidence and remain retained. `training_data.rs` has CLI capture/export consumers and requires an explicit retention decision. Causal-frontier files (`causal_frontier.rs`, `causal_active.rs`, `causal_observe.rs`) are a separate CodeGG policy owner and are out of M006 deletion scope. M001 frozen compatibility fixtures remain runnable against the public MIT repository pinned at `8139b064bdcf3212e8f6fd912e801a479b55751c`.

## 4. Verification executed

All results below are local on the implementation head unless specified.

- Focused adapter, resolver, promotion-gate, request-preparation, and SDM runtime feature tests passed (4 SDM feature tests).
- `cargo run --locked --bin codegg -- tool-advisor status --json` passed and reported policy `off`, backend `noop`, no capabilities, and `promotion_qualified: false`.
- `cargo check --workspace --all-targets --locked` passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- Supported feature Clippy passed: `cargo clippy --locked -p codegg --all-targets --features server,plugins,lsp-test-support -- -D warnings`.
- `cargo fmt --all -- --check` and `scripts/verify.sh quick` passed.
- Canonical workspace nextest passed: 12,306 passed, 7 skipped (`cargo nextest run --workspace --locked --profile ci`, with host SDK link flags).
- SDM compatibility runner at the pinned revision passed 12 fixture validations / 11 runtime requests; artifact digest remained `sha256:b9484ce8b8afe2b345693e084468c89a628462e86faf1e9683c28ee53de6597e` and remains unqualified.
- An initial direct `cargo test --locked --workspace` run had one unrelated checkpoint assertion failure (`test_append_checkpoint_update`, expected `Phase 1`); rerunning that test alone passed, and the subsequent complete canonical nextest sweep passed.
- The plan's all-features Clippy command was not run because repository instructions prohibit `--all-features` workspace sweeps (it includes real-server tests). The supported feature set and SDM feature-specific tests were run instead.

## 5. Invariant review

No model response can introduce or broaden the CodeGG-owned candidate set. Backend identity does not alter durable session/provider selection. Fallback remains deterministic. Ranking does not grant actuation or promotion authority. System One remains bounded to its supported subset. Causal-frontier modes and training-event consent are independent.

## 6. Failure and recovery review

Unsupported capability, backend error, invalid response, panic, and deadline all resolve through deterministic fallback. No repeated Choice loop is used to synthesize Rank. Status reporting is observational and does not load/infer from an artifact.

## 7. Migration and compatibility review

The `promote` policy now fails safely to `observe` unless the backend is qualified; current backend resolutions are deliberately unqualified. The pinned external runtime remains opt-in and the frozen compatibility contract passed. No stored session/provider identity migration was introduced.

## 8. Security review

Candidate identifier validation remains enforced at the adapter boundary. The migration does not change artifact hash verification, endpoint credential/TLS policy, capture consent, or authority checks. No new network path is enabled by default.

## 9. Documentation and operations

`architecture/config.md` and `architecture/tool-advisor.md` describe the live engine path, fallback, backend status and promotion qualification. The public SDM repository remains the owner for generic training/runtime implementation.

## 10. Unresolved findings

None for M005. The baseline SDM artifact remains unqualified and opt-in; qualification is not an M005 deliverable. M006 is a separate active cleanup milestone.

## 11. Roadmap disposition

M005 is closed. The inventory satisfies M006's precondition to begin reviewable retirement work. M006 is unblocked to ready; it must reassess each deletion against current callers and preserve frozen evidence, capture/export if supported, and causal-frontier policy.

## 12. Registry updates

Mark M005 closed with this record. Mark M006 ready for handoff and activate it in the subsequent status-transition commit, following the planning lifecycle.
