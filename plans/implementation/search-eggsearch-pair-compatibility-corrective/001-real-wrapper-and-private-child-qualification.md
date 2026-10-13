# Search / Eggsearch Pair Compatibility C001 — Real Wrapper Dispatch and Private Child Qualification

Status: ready
Class: post-closure verification corrective / pre-release development infrastructure
Planning baseline: CodeGG branch `agent/eggsearch-evidence-workflow-20261013`; M001 implementation `9dd8a1c89253630b1c4ca556f276042325f19836`; M001 closure `plans/closure/search-eggsearch-legacy-retirement/001-status.md`.
Source roadmap: `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md`.
Governance: `plans/003-planning-process.md#7-corrective-passes`.
Long-term relevant: `plans/000-long-term-specification.md` (search/MCP ownership, trust boundaries) and `plans/002-long-term-roadmap.md` (staged development).
Immutable upstream: eggsearch `v0.4.1` at `32769d684417567dfb30fc5adea1475cee7f0ed0`, particularly `src/main.rs`, `src/core/config.rs::default_config_path`, `src/mcp/tools/{web_search,repo_search,security_search,provider_status}.rs` and `docs/tool-matrix.md`.
Hard dependencies: original development M001 implemented/closed, satisfied. No codegg release/daemon/TUI readiness requirement.

## 1. Objective

Turn existing `scripts/test-eggsearch-pair.sh` + `tests/eggsearch_pair_compat.rs` into a **reliable offline one-command test of CodeGG native search wrappers and an actual pinned eggsearch MCP child as a single integration unit**. Repair test environment isolation so the *child*, rather than only the parent test process, uses a private, explicitly controlled configuration. Keep existing v0.4.1 pin/provider mapping and historical M001 closure; no product release qualification.

## 2. Reproduced review findings

1. The existing real subprocess test calls `provider_status` via `McpService::call_tool`, not `WebSearchTool::execute_structured`, `RepoSearchTool::execute_structured` or the equivalent `SearchRuntimeContext::dispatch_*`. The other wrappers' requests are covered by mock-MCP tests, not the tagged eggsearch process. No deterministic real-child assertion currently proves their wire acceptance or rejection.
2. The test calls `std::env::set_var("HOME", ...)` and `std::env::set_var("XDG_CONFIG_HOME", ...)`, but configures `EggsearchConfig.env = Some(HashMap::new())`. The real child is spawned by `src/mcp/local.rs` using `env_clear()`, reinjecting only PATH and `EggsearchConfig.env`; private paths never reach that child under current test code. On macOS eggsearch's default configuration may not use XDG. The child also inherits the test's cwd unless explicitly constrained.
3. Existing harness logs `network_requests=none` without forcing a private **eggsearch search-mode-off config** and proving it was loaded. The lack of direct network search calls so far avoids an observed leak, but adding wrappers without explicit offline policy would create an avoidable network risk.
4. Historical M001 closure called the pair gate passed; that statement is accurate for handshake/inventory/provider status, but the fuller real-wrapper and child-isolation evidence must appear in this new corrective's closure instead of retroactively modifying it.

## 3. Scope, explicit non-goals and architecture

In scope:
- `tests/eggsearch_pair_compat.rs` real process test;
- `scripts/test-eggsearch-pair.sh` one-command runner;
- small test-only fixtures under `tests/fixtures/eggsearch/pair/` if required;
- narrow changes to existing MCP test/launch seams only **if** needed to achieve verifiable process isolation and diagnostics;
- documentation of development command/evidence, and a new corrective closure record upon implementation.

No new MCP server implementation, provider runtime, generic sandbox/process architecture, changes to production search behavior unrelated to this test, real web search, new always-on CI lane, end-user installer/release testing, or application operability claim. Do not use network filters or credential stripping as proof of an OS-level sandbox: this is deterministic test-harness isolation, **not enforced network containment**.

Preserve a single daemon-owned `McpService` session across wrapper requests. All fake-MCP and real-process tests must use CodeGG's production adapters (`SearchRuntimeContext` / native tool implementations), not duplicate request translation in test helper code.

## 4. Work packages

### WP1 — Freeze the actual offline upstream contract before changing the test

Examine eggsearch `v0.4.1` at immutable tag for:
- `eggsearch --config <absolute private file> mcp stdio` CLI syntax and supported config `[search] mode = "off"`; identify any upstream local-only repo behavior if a repo request is tested;
- tool-specific `tools/list` input schemas, MCP `isError` vs transport errors, `response_detail` and request validation ordering;
- provider_status fields that prove the child loaded a deliberately chosen config, **or** another deterministic read-only way to verify the child's private config was actually applied. Do not assume a field/path exists; when no direct echo is available, use a distinct observable policy/config fingerprint in a controlled test and a negative control with the config withheld;
- CodeGG `McpService` call error representation and wrapper `execute_structured` propagation, including the distinction between `Ok(result)` where the upstream tool signaled `isError` and a Rust-side validation error.

Freeze minimal tagged-compatible fixtures and expected error classifications. Separate a **contract assertion** (accepted upstream argument form, recognizable policy rejection, preserved structured error) from a **provider-success assertion** (impossible to guarantee offline without a local upstream fixture provider).

### WP2 — Establish *actual child* isolation and observable proof

In `tests/eggsearch_pair_compat.rs`, construct the test's private HOME, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME and work/config roots. Generate a tagged-compatible eggsearch config in the private root explicitly setting **`search.mode="off"`** and harmless provider settings. Pass the **absolute config file path** in `EggsearchConfig.args` using eggsearch's global `--config` flag, or an equivalent tagged-supported explicit config argument; this is important on macOS. Pass HOME/XDG (and platform equivalents such as APPDATA/USERPROFILE where applicable) through `EggsearchConfig.env` so that `McpLocalClient` injects them after `env_clear()`. Never rely on parent `set_var` to propagate to the child.

Avoid mutating global process environment or taking a serial-global test lock as a workaround. The test should be independent across parallel test executions and private directories. A test that proves env propagation may use an isolated, test-owned echo probe of the **existing** MCP child launcher or an observable eggsearch configuration lookup; do not instrument production logging with secrets and do not add an agent-visible diagnostic. Assert a distinguishable read-only server outcome or explicit configuration-path fingerprint. Test failure if the private file is absent/mispointed, if env injection is removed, or if the command receives a poisoned HOME/config path. Any environment-dependent signal must be portable or platform-conditional with equivalent coverage.

Sanitize credential-bearing env variables, proxy/URL variables and state surfaces used by the child. Existing launcher uses `env_clear` and reinjects PATH; verify no provider token or inherited proxy becomes visible. The test must not **assume** the OS network is blocked; use upstream `Mode::Off` plus no live provider requests, bounded timeouts and no configured credentials as the deterministic offline contract. Do not claim packet-level isolation unless an actual OS/network sandbox test is introduced separately.

### WP3 — Drive native wrappers over real MCP and assert the transport boundary

Retain existing real bootstrap, exact-version, ten-tool and provider inventory checks. Extend real-subprocess test using the **same returned `SearchRuntimeContext` and MCP server handle**:
- At least `WebSearchTool::with_search_runtime(runtime.clone())` (or `dispatch_web_search_structured`) and one distinct specialized `RepoSearchTool`, `SecuritySearchTool` or `ResearchSearchTool` with known syntactically valid native parameters.
- Through CodeGG's production adapter, perform calls against the real eggsearch MCP process in enforced offline mode and assert either a **known tagged policy-denial response** or a documented offline fixture result (if safely implementable). Validate the error class and originating **tool name**, not merely `Err(_)` or arbitrary text containing `invalid params`.
- Prove the call crossed the real child boundary rather than failing before MCP dispatch: compare against an isolated direct MCP call using **the same translated upstream args** on the same connection, or add a narrowly scoped test-only MCP transport observer that records outgoing tool IDs/arguments while retaining real child response. Avoid embedding a second translator in tests. A local argument error is an expected negative control, **not** evidence of successful wire dispatch.
- Examine `McpToolCallResult.structured`, `is_error` and CodeGG's `StructuredSearchResult.success/value/output` framing as applicable. Do not declare all upstream errors compatible. Unexpected schema/type/unknown-tool errors must fail, even if network is disabled.
- Exercise at least two **negative** native validation calls (`google_news`, unknown provider/non-string provider) proving local rejection with no MCP invocation; test valid `auto` and mapped `github -> github_repositories` at the translation/real schema seam without requiring providers to be live. Preserve raw `mcp__eggsearch__*` non-disclosure to models.
- Verify repeated calls share a single MCP connection, and that dropping/tearing down the test context actually closes or kills the spawned process.

If upstream rejects **all** useful wrapper requests before it parses their arguments under Mode::Off, do **not** misreport this as deep wire-contract verification. Use the real process for actual receipt/error handling and the exact tagged JSON schema plus fake-MCP trace for argument validation; record which dimensions are only indirectly qualified. When practical, use a deterministic **local-only** upstream capability that executes without a network call to get one positive native wrapper result; it may not read outside the private workspace. Avoid adding a fake upstream server masquerading as the real child.

### WP4 — Failure, teardown and isolation guards

Extend one-command harness, staying offline:
- malformed/missing binary, wrong-version `0.4.10`, incompatible tools, invalid or unreadable config, env-injection loss, sidecar crash, hanging handshake/call and unexpected `isError` each fail nonzero with stage labels;
- ensure subprocess shutdown is bounded even on assertion failure/panic and partial bootstrap; `kill_on_drop`/RAII plus an explicit shutdown path; no zombie/orphan server and no persisted test credentials, config or temporary home;
- demonstrate at least one negative-control run that **would pass the current existing handshake-only test** but fails the strengthened wrapper/isolation gate. Prefer a deliberately malformed wrapper arg or private config denial test rather than mutating production provider logic.
- stage-specific output categories: `version_identity`, `mcp_initialize`, `tool_schema`, `provider_status`, `wrapper_request`, `upstream_policy`, `structured_fidelity`, `child_environment`, `teardown`. Do not count intentional Mode::Off as a remote outage.
- caller may choose an explicit prebuilt binary or immutable v0.4.1 source. `CARGO_NET_OFFLINE=true` for Cargo build/test; never silently fetch HEAD or a CodeGG release archive. Use the *same script* after legacy backend retirement M002.

### WP5 — Contract regression and reporting

Retain the fake-MCP suite alongside real-process test and state the coverage boundary explicitly. Document exactly how many native tools were exercised against the **real child**, how many were checked only by fake-MCP and upstream schemas, what negative tests ran, what process/env evidence proves isolation, and what was intentionally not verified. Do not disguise upstream tool `isError` as successful search or claim live provider functionality.

## 5. Concrete tests and acceptance matrix

| Trajectory | Required assertion |
|---|---|
| Correct v0.4.1 local child | Initialize passes, exact version, 10 tools, full provider inventory, shared MCP process and clean shutdown |
| Isolated private config | Child was passed explicit private `--config`, HOME/XDG/platform dirs via child env; observable server behavior confirms `Mode::Off` |
| Parent has conflicting HOME/config or secret token | Child uses private config and cannot see inherited sensitive env; no process-global test-env mutation |
| Native `websearch` valid query | Request crosses real MCP path, upstream offline response correctly classified; argument-schema failure is **not** accepted |
| Native repo/security/research valid request | Second distinct wrapper crosses same real session with recognized tagged schema/policy response |
| Unknown / `google_news` / non-string provider | Rejected before MCP dispatch; no automatic routing or server-side side effects |
| Supported `auto`, `github` | Provider omitted on auto; repository-provider identity translated exactly; no live provider success required |
| Real MCP response vs fake-MCP fixture | Structured content/error/trust framing consistent, output remains bounded, receipts distinguish unknown/error from complete |
| Wrong executable version, missing config, schema break | Nonzero with stage-specific failure, not classified as provider outage |
| Timeout, interrupted test, partial spawn | Bounded process death and cleanup, no orphan; deterministic negative case |
| Original handshake-only gate passes but wrapper mismatch is injected | Strengthened test fails and identifies real wrapper/schema boundary |
| Success on both Linux and macOS | Portable private config/env behavior and offline output; Windows Cargo test retains a compatible invocation path where applicable |

## 6. Focused verification commands

Use the exact file/test names as implemented; correct documented names if necessary and report deviations:

```bash
cargo fmt --all -- --check
cargo test -p codegg --test search_backend_arg_mapping -- --test-threads=1
cargo test -p codegg --test fake_eggsearch_mcp -- --test-threads=1
cargo test -p codegg --lib search_backend::eggsearch -- --test-threads=1
cargo test -p codegg --lib search_backend::bootstrap -- --test-threads=1
cargo test -p codegg --lib research::sources::eggsearch -- --test-threads=1
CODEGG_EGGSEARCH_SOURCE=/path/to/eggsearch-with-v0.4.1-tag scripts/test-eggsearch-pair.sh
scripts/verify.sh quick
git diff --check
```

Retain one single-run no-credentials/no-network entrypoint as primary evidence; optional `tests/eggsearch_real_compat.rs` live provider smoke does **not** satisfy the offline gate. Hosted CI, all-features, full daemon/TUI and release/archive validation are **not required** for this corrective; if run, record separately.

## 7. Security / compatibility / concurrency / migration

No persistent config or storage migration. No change to stable tool names/schemas unless necessary to correct a demonstrated upstream incompatibility; in that case stop and propose a separate narrow production corrective. CodeGG authorization, SSRF, model tool exposure and trust framing remain unchanged. Test env must be scrubbed; private paths recorded, secret values never logged. Concurrent tests must not mutate global env. Cancellation and process teardown are bounded. A passing compatibility pair is **not** an OS-level network sandbox and not release readiness.

## 8. Stop conditions and exit criteria

**Strict close only when:** frozen v0.4.1 contract and observable private-child config validated; *two distinct native wrappers* actually call the **real child** with recognized args and unambiguous offline outcomes; negative invalid-provider and broken-schema tests demonstrate failure; structured error/value and output framing remain truthful; teardown and wrong-version/config failures are bounded; existing fake suites and quick verification pass. Give truthful direct vs indirect coverage in the closure record.

**Stop** if Mode::Off short-circuits all wrapper requests such that acceptance cannot distinguish a valid upstream argument schema from a malformed one. Either add a bounded non-network positive upstream path that proves schema acceptance, or explicitly revise the requirement through a separate corrective/ADR; do not mark "fully paired" by assuming all policy failures imply accepted schemas.

Close to `plans/closure/search-eggsearch-pair-compatibility-corrective/001-status.md` with:
- exact CodeGG SHA, binary/tag/commit/digest and Linux/macOS environment;
- source/tag schema evidence; real-child captured request names, outcome categories and exact test fixture identifiers;
- test private config and child env propagation evidence, injected negative control and cleanup proof;
- requirement-to-test matrix for real-child vs fake-MCP vs tagged-source checks;
- commands executed, passes/failures/skips/unsupported/blocked, optional live checks explicitly separated;
- security, concurrency, storage/restart/teardown checks and severity-ranked residual risks;
- strict/conditional/not-closed disposition and **explicit recommendation whether legacy M002 may begin**.

Historical M001 closure remains unchanged. Do not claim this corrective implemented merely because its handoff plan is registered.
