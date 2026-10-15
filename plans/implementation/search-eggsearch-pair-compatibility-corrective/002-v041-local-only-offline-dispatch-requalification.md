# Search / Eggsearch Pair Compatibility C002 — v0.4.3 Local-Only Requalification

Status: **ready** (upstream immutable-release gate satisfied; CodeGG implementation and acceptance pending)
Class: post-closure corrective / development compatibility / safety
Source roadmap: `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md`
Predecessor C001: `plans/closure/search-eggsearch-pair-compatibility-corrective/001-status.md` (historically conditionally closed; do not rewrite).
Development adoption M001: `plans/closure/search-eggsearch-legacy-retirement/001-status.md` (historical pinned v0.4.1 evidence; do not rewrite).
Downstream legacy-retirement M002: `plans/implementation/search-eggsearch-legacy-retirement/002-builtin-search-backend-retirement.md` (**blocked** until this C002 strictly closes).
Controlling process: `plans/003-planning-process.md#7-corrective-passes`.

## 1. Objective and unblock evidence

Upgrade CodeGG's *development* eggsearch contract from `0.4.1` to the immutable corrected **`0.4.3`** and qualify **CodeGG native wrappers + one real pinned eggsearch MCP subprocess as a unit**, including a successful private local-only `repo_search(include_local=true)` trajectory under `search.mode="off"`. The purpose is to discharge C001's high finding, **not** to certify the CodeGG agent, daemon, TUI, installer or any CodeGG release.

The former external blocker is **satisfied**, as of 2026-10-13:
- Upstream published version: `eggsearch v0.4.3`, crate `0.4.3`; [immutable release](https://github.com/eggstack/eggsearch/releases/tag/v0.4.3).
- Immutable annotated tag `v0.4.3` peels to **`5e7b853e7eebf29c58f84959805c8774279d7ea4`**. The released binary/manifest is bound to this SHA; its release has 19 assets, seven target binaries.
- Upstream M001 implementation `cc0c9c7b27e0706a3bab5f5d7cb137e64e9eaa3d` is in the tagged source. Its counted regression verified `RepoRemoteExecution::Denied`, zero configured engine calls and independently injected package-resolver calls, local fixture identity and unchanged live provider routing. Evidence: `eggstack/eggsearch/plans/closure/mode-off-repository-locality-corrective/001-status.md`.
- Exact-SHA qualification run [38476463235](https://github.com/eggstack/eggsearch/actions/runs/38476463235), binary producer [38480449328](https://github.com/eggstack/eggsearch/actions/runs/38480449328) and provenance [38483437078](https://github.com/eggstack/eggsearch/actions/runs/38483437078) succeeded. Upstream historical M002/M003 release closures are present on `agent/mode-off-closure-20261014`; eggsearch's main branch historically had stale blocked statuses, reconciled separately from this plan.
- CodeGG's own branch still pins `0.4.1` and does **not** have the C002 real subprocess/local-fixture closure. Upstream publication alone does not close C002 or authorize legacy-backend deletion.

## 2. Root cause and safety invariants

C001's exploratory test with v0.4.1 observed `repo_search(include_local=true)` dispatching DuckDuckGo even though Mode Off and a private local backend were configured. v0.4.1 encoded "zero remote providers" as `providers=[]`, which the upstream adapter treated as "all configured providers"; independent package registry HTTP was also possible. v0.4.3 introduces an explicit internal remote-denial policy and skips independent package resolution. Do **not** use v0.4.1/v0.4.2 for the formerly unsafe fixture.

**Invariants:**
1. The requested local file's identity/contents and provenance are from CodeGG's private test workspace; `providers_queried` does **not** contain DuckDuckGo, generic remote IDs, or remote attempts, and structured warnings/coverage do not imply a successful internet search.
2. The upstream zero-call regression is the direct proof of no remote provider/resolver execution. The CodeGG real-process test must additionally prove local data and no remote routing in its received response. This is **not an OS-enforced network sandbox**; no packet-level containment claim.
3. Explicit external providers under Mode Off cannot reopen remote dispatch. `include_local=false` and missing/disabled local backend do not become automatic provider selection.
4. All earlier CodeGG wrapper/provider mapping/structured retrieval tests remain passing; `github -> github_repositories`, retired `google_news` rejection, no silent fallback, ten stable MCP tool names, provider_status inventory, `isError` handling and trust framing remain intact.
5. Preserve a single daemon-owned `SearchRuntimeContext`/MCP session, validated child private `--config` and sanitized env, correct cancellation/timeout/teardown, no global `set_var`, no per-tool subprocess, no live credentials/provider calls.
6. Preserve historical C001/M001 closure records and any unrelated feature work. Do not remove the legacy builtin backend or claim CodeGG release readiness in this corrective.

## 3. Ordered work packages

**WP1 — Freeze upstream contract and pin ownership.** Verify from `v0.4.3` tagged source/manifest and `tools/list` that the ten-tool surface, provider IDs, input schemas, release version and required local config format remain as used by CodeGG. Record SHA/digest, old vs new config/schema delta, pin owners. Update all CodeGG pinned version and tagged-source references in `src/install.rs`, `src/upgrade/managed.rs`, `scripts/release/lib-release.sh`, `scripts/test-eggsearch-pair.sh`, `tests/eggsearch_pair_compat.rs`, related tests/docs and diagnostic text **together**. Preserve static future bundle-version identity but do not build/publish a CodeGG release. The harness must reject v0.4.1, v0.4.2 and spoofed `0.4.30`; never resolve moving upstream `main`.

**WP2 — Enhance the existing single-command paired harness.** Extend `tests/eggsearch_pair_compat.rs`, called by `scripts/test-eggsearch-pair.sh`, to create and configure a private local Git/repository fixture (marker file and deterministically discoverable local root) inside its isolated HOME/workspace. Pass the exact `v0.4.3` binary using the production MCP client, an explicit private config with `search.mode="off"` **and a properly enabled/scoped local backend**, and a sanitized child environment. Reuse the *same* real child/context from previous `websearch` and `security_search` policy-denial checks. Drive native CodeGG `repo_search` wrapper/structured dispatch, not an exclusively direct upstream `call_tool` substitute.

**WP3 — Deterministic positive and negative assertions.** Verify the returned repo/card/snippet references the exact private marker file and no out-of-root content; the local group records local provenance and no remote provider telemetry. Include a no-match scoped-local query; `include_local=false` policy denial; explicit remote provider hint under Mode Off (still no dispatch); missing/disabled local backend; package-coordinate/security-context selection (no unqualified registry resolution); invalid provider validation (must reject before MCP); malformed private config/wrong binary and bounded crash/teardown negative controls. Do not turn a generic `isError` or timeout into a pass. Distinguish MCP transport, native validation, policy deny, source/provenance and actual retrieval incompleteness.

**WP4 — Whole-suite compatibility and alternate environments.** Execute the paired real process and fake-MCP fixtures under one offline script; any Cargo fetch of dependencies must happen before the offline test phase. Run normal focused unit/integration tests and `scripts/verify.sh quick`. Record Linux and macOS results separately if available; Windows Rust test entrypoint remains usable and its actual execution status must be disclosed, not invented. Check that full `StructuredSearchResult.value` retains retrieval dimensions/warnings even if bounded model output is truncated. Preserve consistent consumer logs with exact CodeGG and upstream version/build digest.

**WP5 — Strict closure and gate reconciliation.** Write `plans/closure/search-eggsearch-pair-compatibility-corrective/002-status.md` recording candidate SHA, upstream immutable tag/SHA, digest/source of real binary, per-case receipt/remote-attempt/provenance evidence, exact command outcomes, timeout/cleanup and security/migration checks, unrun tests and residual severities. Reconcile registry and both owning roadmaps. Only **strict successful C002 closure** may mark legacy-retirement M002 `ready`; do not automatically delete the builtin provider or rewrite C001 historical closure.

## 4. Acceptance matrix (required)

| Trajectory | Required evidence |
|---|---|
| Exact `v0.4.3` binary from immutable SHA | Exact version + verified digest/provenance where available, 10 tools, provider_status, successful handshake and bounded shutdown |
| Private Mode-Off local fixture | A native CodeGG `repo_search` call crosses real child and returns only matching fixture file(s) with local provenance and no remote provider IDs |
| Explicit remote hint + `include_local=true` | Local-only or explicit policy error; no remote dispatch/attempts or provider fallback; upstream zero-call regression linked |
| `include_local=false` or disabled backend | Policy/unavailable, not complete internet no-match |
| Local query with no matches | Scoped local absence and truthful skipped remote coverage; no remote providers |
| Package-coordinate / security hints | No outbound registry, advisory or forge path; reference upstream counted tests and CodeGG local receipt |
| Native `websearch`, `security_search` Mode Off | Expected specific policy errors (not arbitrary `isError`), same real child |
| Wrong-version/malformed-config/child-exit/hang | Fail nonzero with stage classification and clean teardown |
| Bad explicit provider / obsolete `google_news` | Rejected without automatic routing or network |
| Prior fake-MCP and research evidence fixtures | Structured/receipt behavior unchanged; no spurious `Complete` on unavailable coverage |

**Stop** if private local results cannot be demonstrated with zero remote routing, the tagged binary's input/output contract differs materially from CodeGG assumptions, or any test demonstrates remote provider execution under Mode Off. Preserve the legacy backend gate and create a narrowly scoped follow-up corrective rather than weakening the assertion.

## 5. Verification

```bash
# SOURCE_ROOT must contain immutable annotated tag v0.4.3.
CODEGG_EGGSEARCH_SOURCE=/absolute/path/to/eggsearch-checkout scripts/test-eggsearch-pair.sh
# Or an explicitly verified release binary:
CODEGG_EGGSEARCH_BIN=/absolute/path/to/eggsearch scripts/test-eggsearch-pair.sh

cargo fmt --all -- --check
cargo test -p codegg --test eggsearch_pair_compat -- --test-threads=1
cargo test -p codegg --test search_backend_arg_mapping -- --test-threads=1
cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
cargo test -p codegg --test search_backend_eggsearch -- --test-threads=1
scripts/verify.sh quick
git diff --check
```

If the existing harness uses different feature gates or target names, follow actual Cargo targets and report the deviation. Explicit network/live-provider calls are **not** required or authorized for qualification.

## 6. Dependencies and closure

- **Ready**: the upstream blocker is satisfied by immutable `eggsearch v0.4.3`; CodeGG must still implement the pin/test corrections and collect its **own** paired-process evidence.
- **Blocked**: legacy-retirement M002 until strict C002 closure; C001 remains historically conditional, with its finding discharged only by successor C002 evidence.
- **Independent**: evidence-workflows M002–M004; no new blocking dependency from this corrective.
- **Deferred**: full CodeGG agent operation, release packaging/publishing and production installer qualification.

Only the C002 closure file may assert implementation completion. This implementation plan and registry update are planning-only.
