# Search and Eggsearch Integration Milestone 003 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/search-eggsearch-integration/003-structured-contract-and-compatibility-closure.md`

Source subsystem roadmap: `plans/subsystems/search-eggsearch-integration-roadmap.md#milestone-003--structured-contract-consumption-and-compatibility-closure`

Repository baseline reviewed: `89dbac7`

Implementation commit: `89dbac7` — `feat(search): preserve structured eggsearch responses`

Accepted predecessor closures:

- `plans/closure/search-eggsearch-integration/001-status.md`
- `plans/closure/search-eggsearch-integration/002-status.md`

## 1. Executive finding

M003 is strictly closed. CodeGG now preserves the complete parsed eggsearch
response in `StructuredToolResult::value` while deriving bounded,
external-content-framed display text separately. All nine supported eggsearch
wrappers use the structured path, additive upstream fields are tolerated, and
the legacy string-returning MCP path remains compatible for unrelated callers.

The exact local compatibility baseline was eggsearch `0.3.6`, installed from
the crates.io package as `/tmp/codegg-eggsearch-036/bin/eggsearch`. The binary
reported `eggsearch 0.3.6`. The audited upstream release commit recorded by
the implementation plan is `4ccb374af00348bba75761f6bbd1e192d385a2b9`.

The real process smoke connected through CodeGG's normal MCP stdio bootstrap,
discovered the expected tool inventory, and reached the actual upstream
handlers for every wrapper. One representative `repo_fetch` request received
an upstream HTTP 404 for its sample locator; this is recorded as a provider/
sample failure, not a request-schema or MCP compatibility failure.

## 2. Requirement and evidence matrix

| Requirement | Evidence |
|---|---|
| Parse before display projection | `EggsearchCallResult` captures structured MCP content before `clamp_output`; display text and `value` are separate. |
| Populate all wrapper values | `src/search_backend/mod.rs` and the nine wrapper `execute_structured()` methods populate `StructuredToolResult::value` when current structured content is available. |
| Preserve model-facing compatibility | Existing string-returning dispatch and trust framing remain; structured wrappers retain bounded output and provenance. |
| Preserve metadata | Fake integration fixture verifies `stable_id`, warnings, trust markers, routing, next actions, repository locator, security/research metadata, and an additive unknown field. |
| Diagnose compatibility | Bootstrap records server version when available, bounded provider/capability summaries, malformed status, and required/recommended tool coverage. |
| Real current process | Local opt-in smoke uses eggsearch 0.3.6 through MCP stdio and invokes every supported wrapper. |
| Avoid CI expansion | No network-dependent CI lane, version matrix, scheduled compatibility job, source scanner, or release gate was added. |

## 3. Production implementation evidence

The generic MCP boundary gained an additive structured result method. It retains
top-level structured content and JSON content while preserving the old
`call_tool() -> String` contract. Local and remote clients also expose the MCP
server version when initialize metadata supplies one.

The eggsearch adapter has one bounded internal call result containing parsed
JSON, display output, and truncation state. The display path is clamped and
framed; the parsed value is not. Text-only legacy responses remain supported
as an explicit degraded compatibility mode with `value = None`.

The structured dispatch covers `websearch`, `webfetch`, `repo_search`,
`repo_fetch`, `repo_map`, `security_search`, `research_search`, `batch_fetch`,
and `evidence_bundle`. Existing backend selection, explicit builtin fallback,
timeouts, cancellation, and MCP service ownership remain authoritative.

Bootstrap installs the live service before provider-status inspection and
produces bounded provider and capability summaries without printing raw
provider JSON, credentials, or provider failure reasons.

## 4. Verification commands and results

Focused deterministic verification:

- `rtk cargo test --test search_backend_arg_mapping -- --test-threads=1` — 11 passed.
- `rtk cargo test --test search_backend_eggsearch -- --test-threads=1` — 9 passed.
- `rtk cargo test --test fake_eggsearch_mcp -- --test-threads=1` — 26 passed.
- `rtk cargo test --test eggsearch_real_compat --no-run` — passed.
- `rtk git diff --check` — passed.

Broad verification:

- `rtk scripts/verify.sh full` — passed. This included formatting, generated
  agent validation, core-boundary/sandbox/execution-ownership guards, locked
  workspace checking, Clippy with `-D warnings`, the capped single-threaded
  workspace test suite, doc tests, and the feature check.
- The workspace library phase reported 4,192 passed tests and no failures;
  all subsequent workspace integration and doc-test phases were also green.

Real-process compatibility evidence:

```text
rtk /tmp/codegg-eggsearch-036/bin/eggsearch --version
eggsearch 0.3.6

rtk env CODEGG_EGGSEARCH_BIN=/tmp/codegg-eggsearch-036/bin/eggsearch \
  cargo test --test eggsearch_real_compat -- --ignored --nocapture --test-threads=1
1 passed; 0 failed
```

The smoke discovered this exact inventory:

```text
["batch_fetch", "build_evidence_bundle", "provider_status", "repo_fetch",
 "repo_map", "repo_search", "research_search", "security_search",
 "web_fetch", "web_search"]
```

Per-wrapper disposition was: `websearch ok structured=true`, `webfetch ok
structured=true`, `repo_search ok structured=true`, `repo_fetch
provider_or_network_failure` with upstream HTTP 404 for the sample locator,
`repo_map ok structured=true`, `security_search ok structured=true`,
`research_search ok structured=true`, `batch_fetch ok structured=true`, and
`evidence_bundle ok structured=true`. Invalid parameters and unknown fields
were treated as incompatibility failures by the smoke; none occurred.

The actual eggsearch initialize response did not expose a `serverInfo.version`
value, so the smoke records the exact `--version` output instead. The CodeGG
diagnostic path surfaces a server version whenever the MCP server provides it.

## 5. Invariants reviewed

- M001 request-contract repairs and M002 single-owner external-search
  architecture remain intact and their focused tests are green.
- Eggsearch remains the default backend; builtin search remains explicit
  compatibility fallback; raw eggsearch MCP tools remain hidden by default.
- External output remains bounded and trust-framed.
- Structured values are retained internally as evidence/data, not instructions.
- `next_actions` are preserved but never auto-executed.
- No per-call eggsearch process spawn or background task was introduced.
- No unbounded structured value is injected into model context solely because
  it is retained in the current structured result.

## 6. Failure, recovery, and cancellation semantics

Malformed provider-status JSON produces a bounded actionable diagnostic and
does not dump raw JSON. Structured JSON is parsed deterministically; a
text-only legacy response is explicitly degraded rather than presented as a
current structured result. Display truncation changes only output and the
provenance truncation flag. Existing MCP timeout, cancellation, bootstrap,
restart, and shared-service behavior remains unchanged.

The real smoke's `repo_fetch` 404 is an upstream provider/sample-locator
failure after successful MCP discovery and request handling. It is not hidden,
and it does not weaken the compatibility result for the other wrappers.

## 7. Compatibility and migration

No durable storage migration was required. The MCP structured API is additive,
and legacy `call_tool()` and string-returning adapter methods remain available.
Unknown response fields are retained in the JSON value without requiring
CodeGG domain structs. Existing wrapper argument aliases and M001 request
translations continue through the same boundary.

Future eggsearch upgrades should run the local opt-in smoke against the exact
binary being adopted and record its version/tool inventory. A breaking request
or response contract requires a focused corrective plan; tool-name discovery
alone is not treated as compatibility proof.

## 8. Security and authorization

CodeGG's outer external-content framing remains applied even when upstream
trust markers or structured warnings identify injection or sanitization risks.
Provider diagnostics expose only bounded names/status/capability state and do
not print credentials, raw provider configuration, or raw failure payloads.
Upstream `next_actions` remain evidence metadata and cannot bypass CodeGG tool
policy or permissions.

## 9. Documentation and operations

The structured result contract and upgrade/smoke workflow are documented in:

- `architecture/search_backend.md`
- `architecture/tool.md`
- `architecture/mcp.md`
- `architecture/config.md`
- `README.md`

The real smoke is intentionally local and opt-in; it is not part of CI and
does not require a permanent network test lane.

## 10. Unresolved findings

No critical, high, or medium findings remain in the M003 scope.

Informational findings retained in the evidence:

- The sample `repo_fetch` in the real smoke returned upstream HTTP 404. This
  is classified as provider/sample data failure after successful contract
  reachability, not a CodeGG incompatibility.
- eggsearch 0.3.6 did not provide MCP initialize server-version metadata.
  The exact executable version is therefore recorded from `--version`, while
  CodeGG reports initialize version information when a future server supplies
  it.

## 11. Roadmap disposition

The search and eggsearch integration roadmap is closed. M001, M002, and M003
all have accepted closure records, and the roadmap's ownership, structured
contract, diagnostics, and local compatibility-smoke exit conditions are met.

## 12. Registry and dependency audit

`plans/registry.md` now marks the search subsystem closed, removes M003 from
dependency-ready work, and records this closure. The registry contains no
future plan that depends on Search M003, so no additional plan was moved to
ready. Development Verification and Release M006 remains blocked on its
independent prerequisites: strict Provider M007 and Tool Programs M019
closure records.

