# Decision-Model Extraction and Runtime Milestone 006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/decision-model-extraction-runtime/006-legacy-training-runtime-retirement.md`

Source subsystem roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m006--legacy-trainingmodel-code-retirement-and-qualification`

Repository baseline reviewed: `b86f4caa` (M006 activation commit)

Implementation commits:

- `9b7c0064` — retire legacy advisor training/runtime implementation and dependencies
- Closure/registry commit follows this record.

## 1. Executive finding

M006 is complete. CodeGG no longer owns generic learned-model training or architecture-specific runtime implementation. The external public MIT repository [`dbowm91/sdm`](https://github.com/dbowm91/sdm), pinned at `8139b064bdcf3212e8f6fd912e801a479b55751c`, owns reusable training and local inference. CodeGG retains its bounded decision contract, adapters, candidate authority, deterministic fallback, causal-frontier policy, consent, and actuation. Default CodeGG remains model-free; the SDM backend is opt-in and unqualified for promotion.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Remove unowned generic training and model-specific implementation | Implementation commit `9b7c0064`; source/dependency inventory in section 3 | pass | Historical evidence remains in Git and frozen assets. |
| External repository owns reusable training/runtime | `dbowm91/sdm` at immutable revision `8139b064bdcf3212e8f6fd912e801a479b55751c`; compatibility runner passed | pass | CodeGG pins only runtime crates. |
| Remove obsolete features and ML dependencies | Cargo manifest/lock diff and feature graph inspection | pass | Removed Candle and advisor-training/encoder features; lock graph fell from 786 to 740 packages (46 packages, 568 lock lines). |
| Preserve off/local/System One and deterministic fallback behavior | 4 SDM runtime tests; decision adapter/backend tests; System One fake-server suite and M005 tests included in feature sweep | pass | SDM remains opt-in; artifact is not promotion-qualified. |
| Preserve causal-frontier behavior, consent, and historical traceability | Existing causal-frontier and capture/export tests in workspace and feature sweeps; no historical result assets changed | pass | Causal-frontier policy and test evidence remain CodeGG-owned. |
| Preserve supported builds and CLI/config behavior | CLI surface test, quick verification, Clippy, workspace nextest, supported-feature nextest | pass | Removed architecture-specific training/evaluation CLI commands. |

## 3. Production implementation evidence

The implementation removed the legacy contextual/sequence encoder and qualification stack, generic retrieval/late-interaction experiments, requalification code, in-repo training implementation, old SDM runtime adapter, unused qualification holdout generators, and their Candle/training-only dependency graph. `tool-advisor` is no longer a legacy model-stack feature; the external runtime is gated by `decision-runtime-sdm`.

The retained CodeGG-owned components are the decision contract and policy adapters, current operator status/inspect/data commands, `training_data.rs` capture/export/consent support, retrieval relevance used by current behavior, causal-frontier policy, compatibility fixtures, and historical assets/closure records. The old architecture spike document is explicitly historical. The prior order-invariance M005 successor is terminalized as superseded; future generic model work points to SDM.

Artifact inspection validates the pinned runtime artifact and reports its digest/capabilities without inference. Current `tool-advisor status --json` reports `off`/`noop`, no capabilities, and promotion disabled. Dependency inspection found no Candle packages in default or decision-runtime feature graphs; the latter contains `sdm-core` and `sdm-runtime`, not `sdm-training`.

## 4. Verification executed

### Commands run

```bash
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --locked -p codegg --features decision-runtime-sdm
cargo nextest run --workspace --locked --profile ci
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci
cargo test --locked --bin codegg --features decision-runtime-sdm cli_surface_tests::tool_advisor_help_contains_only_supported_operator_commands
scripts/check_sdm_compatibility.sh /tmp/sdm 8139b064bdcf3212e8f6fd912e801a479b55751c
```

The host's x86_64 build used linker flags for its installed SDK libraries. `scripts/verify.sh quick` passed on the final tree. Workspace Clippy passed. The SDM-feature check, five SDM/CLI tests, and external compatibility check passed. The compatibility runner validated 12 frozen fixtures and executed 11 runtime requests with unchanged artifact digest `sha256:b9484ce8b8afe2b345693e084468c89a628462e86faf1e9683c28ee53de6597e`.

The canonical workspace nextest sweep passed **12,282 tests, 5 skipped**. The supported feature sweep passed **9,426 tests, 5 skipped**. Its first run exposed a missing auth-disabled setup in one existing real transport test; that test returned HTTP 503 before its websocket handshake. The test now sets the same isolated test flag as the other transport helpers, the focused test passed, and the complete supported-feature sweep passed afterward. No `--all-features` sweep was run, consistent with repository instructions. Verification was local; no new hosted CI run was started. Binary size was not separately measured; dependency graph and lockfile deltas are recorded above.

## 5. Invariant review

No removed module had a supported production caller after the inventory. The default path has no model/runtime dependency. Learned decisions cannot expand CodeGG's authoritative candidate set or act without CodeGG policy. Deterministic fallback remains available. Causal-frontier modes, capture consent, and telemetry remain independent of the SDM runtime. Frozen results, receipts, and historical closure evidence were not rewritten.

## 6. Failure and recovery review

Missing, corrupt, mismatched, unsupported, or unavailable local artifacts remain bounded diagnostics/fallback cases through the opt-in backend and its tests. System One failures remain bounded and do not synthesize unsupported Rank decisions. Removing training commands and source introduces no persistence or user-data migration. Historical reproducibility remains available through Git history and immutable evidence rather than compiled legacy code.

## 7. Migration and compatibility review

No database or stored-session migration was needed. Obsolete training/encoder feature names and architecture-specific CLI commands were removed; supported CLI help and command-surface tests cover the remaining status, inspection, diagnostics, and data operations. The pinned external runtime interface remains versioned and opt-in. The compatibility fixtures pass at the pinned SDM revision. No backend is enabled by default and no promotion qualification was added.

## 8. Security review

Artifact digest validation, endpoint credential/TLS policy, candidate identifier validation, consent checks, and causal-frontier authority remain in their existing owners and are covered by focused tests and the verification sweeps. Status/inspect reports metadata without running inference. The retired model code granted no new authority; no network backend was enabled by this change.

## 9. Documentation and operations

Updated `architecture/tool-advisor.md`, `architecture/config.md`, and the historical framework spike document. Cargo features and CLI help now describe the supported operator surface. The obsolete qualification generators were removed while historical assets were retained. The order-invariance roadmap and tool-selection roadmap now record terminal/superseded planning state and direct future generic model work to SDM. The decision-model roadmap and registry are closed by this record.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Default/external artifact remains unqualified for promotion. | Learned ranking stays opt-in/observe-only; deterministic behavior remains available. | Any future qualification must be separately planned against the frozen compatibility contract. |
| low | Binary size was not separately measured. | Dependency count/lockfile reduction is evidenced, but no executable-size delta is claimed. | None for M006; measure only if a later release decision requires it. |

## 11. Roadmap disposition

M006 and the decision-model extraction/runtime subsystem are closed. M001–M006 are complete, the external runtime ownership boundary is established, and no registered successor in this line remains ready or blocked. Generic training/runtime changes belong in `dbowm91/sdm`; CodeGG changes must be justified as decision-contract, adapter, policy, or operator integration work.

## 12. Registry updates

- Mark the M006 plan, subsystem roadmap, and registry row closed and link this record.
- Mark the decision-model extraction/runtime roadmap closed in the registry overview and remove it from the active/ready work table.
- Preserve historical milestone closures M001–M005 unchanged.
