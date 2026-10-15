# Search / Eggsearch Pair Compatibility C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/search-eggsearch-pair-compatibility-corrective/002-v041-local-only-offline-dispatch-requalification.md`

Source subsystem roadmap:

- `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md`

Repository baseline reviewed: `158f08f477f97ad6ef5121a697e0eb8d5a5b6426`

Implementation commits or pull requests:

- `9ed08996b8634ce9ea5c404cdca6f4d889c13856` — pin CodeGG to eggsearch v0.4.3, forward native repo-search provider/package selectors, add scoped local fixtures and compatibility coverage, and update installation/release/docs authorities.
- `aaa9d3e92379daab96035b6076e08a9ee766b25d` — correct package-resolution expectations to match v0.4.3's omitted false field and register the disabled-local negative-control child under the wrapper's canonical MCP server name.
- Final qualification candidate: `aaa9d3e92379daab96035b6076e08a9ee766b25d`.

## 1. Executive finding

CodeGG's v0.4.3 integration now proves the formerly unsafe Mode-Off local search path across native CodeGG wrappers and a real Eggsearch MCP subprocess. A private repository marker was returned with local provenance; telemetry named only `local_workspace` and reported no remote attempts. Explicit remote provider hints, package resolution, and advisory context did not reopen remote dispatch. The exact tagged child, ten-tool inventory, sanitized child environment, bounded negative controls, and process cleanup all passed. C002 strictly closes and discharges the local-only dispatch finding recorded by historical C001. It does not close or implement legacy-retirement M002.

The upstream source was verified at annotated tag `v0.4.3`, peeled commit `5e7b853e7eebf29c58f84959805c8774279d7ea4`. The offline release-profile binary used for the final pair test had SHA-256 `bfa7d59ee747b4838522ca930f59d3ff841f3923f760a3b0bcbd0eba6bb1dad4`. Cargo's release LTO was disabled for local qualification throughput; the source commit and locked dependency graph remained exact.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Exact immutable eggsearch identity and MCP surface | Offline `git archive` of v0.4.3 at `5e7b853e7eebf29c58f84959805c8774279d7ea4`; pair log SHA-256 above; `provider_status(probe=false)` and initialize/list-tools through the real child | pass | Exact version, ten tools, provider inventory, and mode `off`; no moving upstream branch was used. |
| Native Mode-Off local repository result | `pinned_eggsearch_process_pair_is_compatible_offline`; native `RepoSearchTool` over the shared production MCP session | pass | Returned the per-run marker and `LOCAL_ONLY_MARKER.md`, only `local_workspace`, local trust provenance, empty selected providers, zero remote attempts, and no out-of-scope/out-of-root sentinels. |
| Local no-match and truthful coverage | Same real-child pair test | pass | Empty local result retained retrieval summary; remote coverage was skipped and no provider was attempted. |
| Explicit provider hint cannot reopen remote routing | Same real-child pair test with `providers=["duckduckgo"]` under Mode Off; upstream immutable M001 zero-engine/resolver regression referenced by source plan | pass | Local query succeeded without remote provider IDs or attempts. |
| Local inclusion and disabled backend controls | Same real-child pair test, including a separate explicitly configured disabled-local child | pass | `include_local=false` and disabled local backend returned classified policy denials. |
| Package and security context | Same real-child pair test with crates.io/serde plus `include_security_context=true` | pass | Coordinate preserved; registry lookup warning says skipped by local-only policy; no verified resolution or security context; advisory skip warning preserved. |
| Existing native web/security policy responses | Native `WebSearchTool` and `SecuritySearchTool` through the same real child, compared with tagged upstream structured errors | pass | Exact policy denial, `isError`, and structured response preserved. |
| Child privacy and bounded lifecycle | Pair child-environment observation, malformed-config negative control, child PID teardown; MCP local lifecycle unit tests | pass | Explicit private `--config`, HOME/XDG paths and CWD; parent credential/proxy sentinels absent; malformed config rejected; hung discovery/tool calls timed out and children were reaped. No packet-level network containment is claimed. |
| Provider selector validation and schema compatibility | Native invalid/retired selector cases; real ten-tool inventory; mapping and fake-MCP suites | pass | Unknown, non-string, empty/automatic lists and retired `google_news` reject locally without MCP fallback; provider-list schema is exposed on native `repo_search`. |
| Exact pin ownership and release identity checks | Pin constants, upgrade/installer/release scripts and regression fixtures | pass | v0.4.1, v0.4.2, and spoofed 0.4.30 are rejected. No CodeGG release was built or published. |
| Structured fidelity under display truncation | Real local hit with 256-character model-facing cap | pass | Display provenance reports truncation while the structured value retains marker, retrieval summary, warnings, and telemetry. |

## 3. Production implementation evidence

- `src/install.rs`, `src/upgrade/managed.rs`, bootstrap diagnostics, release metadata, pair runner, and associated fixtures now share exact eggsearch v0.4.3 version and immutable source commit ownership.
- The native `repo_search` contract advertises explicit provider IDs. The adapter translates provider aliases and forwards ecosystem/package/version/version requirement/namespace/security-context inputs to the v0.4.3 MCP tool.
- The paired test creates a private Git repository and configured local root, uses the production MCP child client and shared `SearchRuntimeContext`, and drives native CodeGG wrappers rather than replacing them with direct upstream calls.
- User and architecture docs now describe v0.4.3, native `repo_search` selectors, and the Mode-Off local-only telemetry contract. No legacy backend implementation was deleted in C002.

## 4. Verification executed

### Commands run

The shell was started under native arm64 because the login shell otherwise ran under Rosetta with an existing mixed-architecture Cargo target. `CARGO_TARGET_DIR` isolated this verification from other workspace builds.

```bash
# Build from the immutable upstream tag, offline. This produced the binary
# retained for the final candidate run; --release LTO was disabled for speed.
arch -arm64 /bin/zsh -c 'export PATH=/Users/davidbowman/.rustup/toolchains/1.89.0-aarch64-apple-darwin/bin:$PATH CARGO_TARGET_DIR=/tmp/codegg-c002-arm64-target CODEGG_EGGSEARCH_SOURCE=/Users/davidbowman/projects/eggsearch CODEGG_EGGSEARCH_KEEP_TEMP=1 CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_LTO=false; /bin/bash scripts/test-eggsearch-pair.sh'

# Final pair gate on the implementation candidate, using that exact tagged
# source binary by its recorded SHA-256.
arch -arm64 /bin/zsh -c 'export PATH=/Users/davidbowman/.rustup/toolchains/1.89.0-aarch64-apple-darwin/bin:$PATH CARGO_TARGET_DIR=/tmp/codegg-c002-arm64-target CODEGG_EGGSEARCH_BIN=/var/folders/2j/dlwhrpps66scv9bw8f7vdfg40000gq/T/codegg-eggsearch-pair.xd5lwQ/eggsearch-target/release/eggsearch CODEGG_EGGSEARCH_TAG=v0.4.3 CARGO_BUILD_JOBS=2; /bin/bash scripts/test-eggsearch-pair.sh'

rtk proxy /bin/bash scripts/release/test-release-tools.sh
rtk proxy /bin/bash scripts/release/test-installer.sh
arch -arm64 /bin/zsh -c 'PATH=/Users/davidbowman/.rustup/toolchains/1.89.0-aarch64-apple-darwin/bin:$PATH /bin/bash scripts/release/test-clean-host.sh'
rtk git diff --check
```

### Results

- Immutable v0.4.3 source build succeeded offline; final binary SHA-256 was `bfa7d59ee747b4838522ca930f59d3ff841f3923f760a3b0bcbd0eba6bb1dad4`.
- Final `scripts/test-eggsearch-pair.sh`: pass on candidate `aaa9d3e92379daab96035b6076e08a9ee766b25d`. Focused counts: provider adapter 29, bootstrap 22, research adapter 18, backend 9, argument mapping 18, fake MCP 30, child lifecycle 5, real pair 1; all passed. `scripts/verify.sh quick` passed all guards and `cargo check --workspace --all-targets --locked`.
- `scripts/release/test-release-tools.sh`: 79 passed, 0 failed.
- `scripts/release/test-installer.sh`: 165 passed, 0 failed.
- `scripts/release/test-clean-host.sh`: 26 passed, 0 failed using the actual arm64 CodeGG debug binary and v0.4.3 sidecar. An initial fallback-fixture invocation had 21 passes and 5 doctor-output failures because no built CodeGG binary was available; after staging the actual arm64 binary and rerunning natively, all 26 checks passed.
- During the first post-implementation pair attempt, formatting stopped the script before tests. Subsequent real-child attempts exposed a false-field expectation and a noncanonical server name in the disabled-local negative control. Both test issues were corrected in `aaa9d3e`; the final complete pair gate above passed.
- Linux and Windows executions were unavailable and were not inferred from the macOS result. The final qualification was on macOS 26.6.2, arm64, Rust 1.89.0. No live provider credentials or provider requests were used.

## 5. Invariant review

1. The local marker and file identity came from the private test Git repository. `providers_queried` contained only `local_workspace`; routing telemetry selected no remote provider; subqueries reported no remote attempts.
2. The upstream v0.4.3 zero-engine and package-resolver call regression remains the direct upstream no-execution evidence. CodeGG's real-process response independently confirms no remote routing. Neither result claims OS network containment.
3. Explicit DuckDuckGo selection under Mode Off remained local-only. `include_local=false` and a disabled local backend produced policy denials, not automatic internet search.
4. Provider alias mapping, retired provider rejection, exact ten-tool inventory, `provider_status`, native web/security denials, `isError` handling, structured values, and trust provenance passed the focused and paired suites.
5. The shared bootstrap MCP session was reused for the primary native calls. The child received a private explicit config and sanitized environment; tests used no process-global `set_var`, live credentials, or per-tool subprocess.
6. Historical C001 and M001 closure records were not edited. C002 did not delete the legacy backend or claim CodeGG release readiness.

## 6. Failure and recovery review

- Duplicate delivery/idempotency, durable state mutation, partial persistence, and stale leases do not apply: these are read-only search calls and no durable state is written.
- Cancellation and hung-child cleanup are exercised by `mcp::local::tests::hung_discovery_times_out_and_child_is_reaped` and `hung_tool_call_times_out_and_child_is_reaped`; the pair confirms shutdown completes and the shared child PID is no longer alive.
- Malformed private config failed the bounded MCP handshake and completed bounded cleanup. Wrong sidecar identity and version spoofing are rejected by release identity tests and the pair runner's exact version check.
- Local validation errors for unsupported, non-string, empty/automatic, and retired provider IDs occur before MCP invocation. No invalid selector falls back to automatic routing.
- The private fixture, config, and HOME/XDG paths are removed after a successful run; a kill guard reaps the primary child if an assertion unwinds. Retained qualification artifacts contain the exact upstream source/binary only; the per-test private workspace was removed.
- There is no durable event or artifact stream in this path; output and request sizes remain bounded by existing tool and MCP limits.

## 7. Migration and compatibility review

No schema or storage migration was needed. Managed install and upgrade identity checks, static future bundle manifest version, source documentation, and test fixtures now pin 0.4.3 at the immutable source SHA. Existing configuration continues to require the upstream `[local]` enabled/roots contract; Mode-Off defaults remain safe. Rollback to v0.4.1/v0.4.2 is explicitly rejected by exact version checks because those builds do not satisfy the local-only contract. The legacy builtin backend remains present pending M002.

## 8. Security review

The native wrapper validates provider selectors before transport and passes the explicit validated list to the upstream child. The real child is given a private config path, private HOME/XDG paths, and private working directory. Parent API-key, proxy, and sentinel variables were absent from the child. Private-root and out-of-root sentinels prove scope boundaries; returned local content is marked untrusted. Registry and advisory resolution warnings show those paths were skipped under local-only policy. Test output does not log secrets. This is application-level dispatch qualification, not packet-level network sandboxing.

## 9. Documentation and operations

- Updated `README.md`, `RELEASING.md`, `docs/install.md`, and `docs/tools.md` for the v0.4.3 immutable sidecar contract and native repo-search controls.
- Updated `architecture/mcp.md`, `architecture/search_backend.md`, and `architecture/upgrade.md` with the source pin and Mode-Off behavior.
- Updated release/install pin owners and exact identity regression fixtures. The one-command runner remains `scripts/test-eggsearch-pair.sh`; it performs a locked offline source build when given `CODEGG_EGGSEARCH_SOURCE`, then focused tests, the real pair, and `verify.sh quick`.
- Release-tool, installer, clean-host, boundary, agent-schema, and quick verification checks passed as recorded above.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Linux and Windows native executions were not available in this environment. | Cross-platform execution evidence is not claimed; the source plan requires disclosure and execution where available. | Run platform-specific qualification if those hosts become available; no blocker to this local C002 strict closure. |
| low | No packet-level network containment is part of the pair gate. | Correctness rests on upstream no-call regressions plus observed CodeGG routing/telemetry, as scoped by the plan. | None for C002; do not describe this result as a network sandbox. |

No known high- or medium-severity finding remains in C002 scope.

## 11. Roadmap disposition

Milestone C002 is strictly closed. Its evidence discharges C001's historical local-only dispatch finding without changing the C001 record. Legacy-retirement M002 is eligible to move from blocked to **ready** because its sole outstanding hard dependency was strict C002 closure. M002's backend deletion, its own tests, and its own closure remain outstanding. Evidence-workflow M005 remains blocked on completion of M002 and M004; C002 does not satisfy either M005 dependency.

The pair-compatibility corrective roadmap is closed. The broader search/eggsearch legacy-retirement roadmap remains active with M002 ready. No other registered plan names C002 as a hard dependency. No release gate was created.

## 12. Registry updates

- `plans/implementation/search-eggsearch-pair-compatibility-corrective/002-v041-local-only-offline-dispatch-requalification.md`: mark implemented and link this strict closure.
- `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md`: mark corrective stream closed, C002 closed, and state that M002 is ready; keep C001 historical conditional unchanged.
- `plans/subsystems/search-eggsearch-legacy-retirement-corrective-addendum.md` and M002 plan: change M002 from blocked to ready and identify C002's strict closure as the satisfied prerequisite.
- `plans/registry.md`: move C002 from active to recently closed, list legacy-retirement M002 as ready, remove its blocked row, and retain evidence-workflow M005 as blocked on M002+M004.
- `plans/closure/search-eggsearch-pair-compatibility-corrective/001-status.md` and `plans/closure/search-eggsearch-legacy-retirement/001-status.md` remain unchanged historical records.
