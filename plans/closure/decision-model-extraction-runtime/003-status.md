# Decision-Model Extraction and Runtime Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/decision-model-extraction-runtime/003-local-artifact-backend-adoption.md`

Source subsystem roadmap:

- `plans/subsystems/decision-model-extraction-runtime-roadmap.md#m003--local-artifact-backend-adoption`

Repository baseline reviewed: `533be5941ac48334743de555cda96d8695cc6527`

Implementation commits:

- `da21d8f8` — add the opt-in SDM local Rank runtime backend and CodeGG adapter.
- `c549921d` — close M002 and publish the SDM dependency pin.

External runtime revision: `dbowm91/sdm@8139b064bdcf3212e8f6fd912e801a479b55751c`.

## 1. Executive finding

M003 is complete. CodeGG can load the pinned SDM Rank artifact through its
backend-neutral `DecisionEngine`, expose it through the existing `ToolAdvisor`
policy interface, and fall back to `NoopAdvisor`/deterministic discovery on
unsupported configuration, invalid artifacts, unsupported semantics, or a
backend error. The integration is opt-in through the
`tool-advisor-sdm-runtime` Cargo feature and `runtime_backend = "sdm_local_v1"`;
the default remains off. CodeGG still constructs and filters candidates and
owns all disclosure and actuation policy.

The shipped compatibility artifact is a runtime/contract smoke baseline, not a
qualified tool-selection model. Its benchmark is slower than the historical
legacy microbenchmark under the recorded workloads; no parity or promotion
claim is made from unlike workloads. Existing legacy runtime code remains for
the migration/retirement work owned by M005/M006.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pinned reusable Rank runtime is selectable through `DecisionEngine` | `src/decision_sdm.rs`; `src/tool_advisor/sdm_runtime_adapter.rs`; `advisor_from_config` | pass | Pin is exact and immutable; backend selection is explicit. |
| Default graph and advisor behavior remain model-free/off | `cargo tree --locked -e normal`; config defaults; `NoopAdvisor` paths | pass | No SDM crates occur in the default normal dependency tree. |
| Runtime feature excludes training stack | feature dependency tree and Cargo manifest | pass | Feature tree has `sdm-core` and `sdm-runtime`; it has no `sdm-training` or ML framework. |
| Artifact integrity and replacement are safe | `decision_sdm` tests | pass | Covers digest/manifest mismatch, tampering, and immutable in-flight snapshots. |
| Supported Rank requests preserve caller authority | `sdm_runtime_adapter` tests | pass | Unknown/expanded candidate output is rejected; only request candidates can be returned. |
| Unsupported semantics/deadline/errors fail explicitly | `decision_sdm` and adapter tests | pass | Deterministic fallback remains available. |
| M001 compatibility contract holds at pinned external revision | `scripts/check_sdm_compatibility.sh /tmp/sdm 8139b064bdcf3212e8f6fd912e801a479b55751c` | pass | Validated 12 frozen fixtures and executed 11 valid requests; the separate negative fixture remains negative. This validates contract and candidate identity, not model quality. |
| Resource report is available and regression classified | SDM `docs/benchmark-baseline.md` at the pinned revision | pass | Artifact 2,560 bytes; RSS 4,878,336 bytes; cold load 510 μs. Rank p95: 291 μs (16 candidates), 1.139 ms (64), 2.317 ms (128), 4.624 ms (256). The report records a latency regression against the non-comparable legacy microbenchmark; the baseline remains opt-in and unqualified. |
| Production authority and deterministic fallback remain CodeGG-owned | `src/tool_advisor/mod.rs`; request-preparation policy tests; causal-frontier implementation unchanged | pass | Backend receives an already-projected candidate list. No tool, permission, disclosure, or actuation authority moved to SDM. |

## 3. Production implementation evidence

The feature-gated `SdmDecisionEngine` loads a bounded artifact, validates its
manifest and expected digest, keeps an immutable runtime snapshot, and exposes
Rank through CodeGG's `DecisionEngine` contract. `SdmToolAdvisor` maps the
existing bounded advisor input to a Rank request and validates the response
against the submitted candidate identities. Explicit SDM configuration without
the feature, invalid artifact state, or backend errors degrade safely.

The agent-loop-facing policy remains the existing `ToolAdvisor` interface.
M005 owns migrating all learned request-preparation paths to one generic
DecisionEngine seam and removing production selection of legacy scorer types;
M006 owns deleting their retained training/runtime implementation. M003 does
not change causal-frontier behavior, historical qualification results, or
promotion eligibility.

## 4. Verification executed

### Commands run

```bash
scripts/check_sdm_compatibility.sh /tmp/sdm \
  8139b064bdcf3212e8f6fd912e801a479b55751c

cargo tree --locked -e normal
cargo tree --locked --features tool-advisor-sdm-runtime -e normal

RUSTFLAGS='-C link-arg=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib/libz.tbd -C link-arg=/usr/local/opt/libiconv/lib/libiconv.dylib -C link-arg=/usr/local/opt/xz/lib/liblzma.dylib' \
  cargo test --locked -p codegg --features tool-advisor-sdm-runtime \
  --lib sdm -- --nocapture

RUSTFLAGS='-C link-arg=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib/libz.tbd -C link-arg=/usr/local/opt/libiconv/lib/libiconv.dylib -C link-arg=/usr/local/opt/xz/lib/liblzma.dylib' \
  cargo nextest run --locked -p codegg \
  --features server,plugins,lsp-test-support \
  --test projection_transport_real --profile ci \
  -E 'test(real_core_rollback_invariants_on_writer_closed) | test(real_core_rollback_harness_asserts_unrelated_client_continuity)'

RUSTFLAGS='-C link-arg=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/lib/libz.tbd -C link-arg=/usr/local/opt/libiconv/lib/libiconv.dylib -C link-arg=/usr/local/opt/xz/lib/liblzma.dylib' \
  scripts/verify.sh quick

git diff --check
```

### Results

- Compatibility checker: passed, 12 fixtures validated / 11 valid requests executed.
- Default workspace nextest phase of `scripts/verify.sh full`: 12,301 passed, 7 skipped.
- Feature-enabled full phase: 7,209 passed before two existing WebSocket test-harness races stopped the run; 2,234 tests were not run. One fixture had not explicitly disabled auth, and one asserted cleanup before the async cleanup pass completed. Both test defects were corrected without changing production WebSocket behavior; the two affected tests then passed together under the feature set. The entire feature-enabled suite was not rerun.
- Focused SDM runtime tests: 7 passed.
- `projection_transport_real` regression tests: 2 passed after the harness corrections.
- `scripts/verify.sh quick`: passed, including formatting, generated-agent check, boundary/security guards, and workspace all-target check.
- `git diff --check`: passed.
- The full verifier's feature-enabled run is recorded as partial, not green. Its only observed failures were the two corrected fixtures above; the remaining feature-only suite was not rerun because the default workspace suite and focused affected-target evidence already cover M003's changed surfaces.

The host required explicit linker search flags for the system `z`, `iconv`,
and `lzma` libraries. Cargo emitted architecture linker warnings for unrelated
MacPorts arm64 dylibs while producing the x86_64 test artifacts; these did not
prevent the targeted suites from linking and passing.

## 5. Invariant review

- Advisor default remains off; no model artifact or external endpoint is
  required for normal operation.
- The adapter receives only CodeGG's supplied candidates and rejects response
  identity expansion. Tool permissions, schemas, and arguments stay outside
  the runtime contract.
- Invalid/mismatched artifacts and unavailable/unsupported decisions take the
  deterministic fallback path.
- Runtime state is immutable for an in-flight turn; replacing the artifact
  file does not mutate a loaded engine snapshot.
- Causal-frontier modes and frozen historical labels/results were not changed.
- Model evaluation status remains unqualified; no historical negative model
  gained a positive disposition or promotion authority.

## 6. Failure and recovery review

Artifact load, manifest, digest, capability, response, and deadline failures
are explicit and degrade to the no-op/deterministic path. Runtime execution
uses a bounded deadline and does not add retries. Snapshot replacement is
atomic from the caller's perspective because each loaded engine owns an
immutable runtime instance. There is no new durable state or migration.

## 7. Migration and compatibility review

No database or protocol migration was introduced. The optional backend
configuration is additive. Existing model-specific configuration continues
through the legacy compatibility path for the migration window; M005 owns its
production routing removal. The SDM artifact and fixture formats are versioned
independently of CodeGG config. Default dependency resolution stays SDM-free.

## 8. Security review

Artifact path and digest are operator-configured and validated before runtime
use. The backend receives bounded projected state and candidate descriptors,
not filesystem, tool, permission, session-store, or credential handles.
Diagnostics report backend state without dumping raw decision state. The
external repository pin is public and immutable.

## 9. Documentation and operations

- `architecture/tool-advisor.md` documents backend config, artifact lifecycle,
  fallback behavior, and resource report.
- `architecture/tool-advisor-framework-spike.md` records M003 adoption and the
  remaining M005/M006 boundary.
- The roadmap and registry record M003 closed and M005 ready.
- The dependency graph remains default-off and runtime-only when explicitly
  enabled.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The full feature-enabled 9,445-test suite was interrupted after 7,211 executions; the two observed harness failures pass in a focused rerun. | No M003 changed production behavior failed; broad feature-only coverage is incomplete. | Preserve the exact partial/full-target evidence above. M005's required verification continues to cover the full workspace and affected request-preparation paths. |
| informational | The compatibility Rank artifact is not a qualified advisor model and is slower than the recorded non-comparable legacy microbenchmark. | It must not be enabled by default or promoted based on this closure. | Keep the backend opt-in; qualification remains governed by existing CodeGG evidence gates. |

## 11. Roadmap disposition

M003 is positively closed and M005 may proceed. M004 is closed and its
DecisionEngine contract is stable. M006 remains blocked on M005 closure and
its fresh source/dependency reachability inventory.

## 12. Registry updates

- Mark M003 closed in the subsystem roadmap and registry.
- Move M005 from blocked to ready and preserve M006 as blocked.
- Keep the subsystem roadmap active because M005 and M006 remain.
