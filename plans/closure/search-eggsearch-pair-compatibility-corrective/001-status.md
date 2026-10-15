# Search / Eggsearch Pair Compatibility C001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/search-eggsearch-pair-compatibility-corrective/001-real-wrapper-and-private-child-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md`

Repository baseline reviewed: `a9a67614d85def93f6664415c26d0912f1dd7874` (implementation verification revision)

Implementation commits or pull requests:

- `b1427c2` — real native-wrapper pairing, child environment observation, MCP timeout lifecycle coverage, runner hardening, and architecture contract correction.

## 1. Executive finding

C001 implemented and passed a deterministic Linux qualification for two distinct CodeGG native wrappers against the same real pinned eggsearch v0.4.1 MCP child. The child used an explicit private Mode-Off config, received private HOME/XDG directories and a private cwd, and did not receive the parent test's credential/proxy sentinels. Both wrapper calls reached the real child and preserved tagged `isError` policy-denial JSON without reporting search success. The strengthened gate includes a poisoned-config negative control that passes handshake and ten-tool inventory but fails the required config gate.

C001 is **conditionally closed**, not strictly closed. An exploratory request made during qualification used `repo_search(include_local=true)` with a private fixture and Mode Off. The pinned v0.4.1 process nevertheless initiated remote DuckDuckGo provider dispatch and produced provider telemetry/results/timeouts. That finding invalidates use of this route as an offline fixture and blocks legacy backend retirement M002 until a corrected immutable upstream tag is independently qualified. The final harness makes no provider search calls; the earlier exploratory dispatch is disclosed here and is not characterized as network containment.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Exact pinned process identity and ten-tool contract | Final `scripts/test-eggsearch-pair.sh`; exact version v0.4.1 and full inventory assertions | pass | Immutable source commit `32769d684417567dfb30fc5adea1475cee7f0ed0`; binary SHA-256 `a89d81eb9ebd01ac4ab618c40f732046024075af287a668773ac3a3fd80ffc5d`. |
| Provider contract and private Mode-Off config | Real `provider_status(probe=false)` response reports mode `off`, generic search false, and mapped provider inventory | pass | Config supplied through explicit absolute `--config`. |
| Native web wrapper reaches real child | `WebSearchTool::execute_structured` plus direct same-session `web_search` comparison | pass | Tagged Mode-Off `isError`; structured JSON preserved and `success=false`. |
| Distinct specialist wrapper reaches real child | `SecuritySearchTool::execute_structured` plus direct same-session `security_search` comparison | pass | Tagged Mode-Off `isError`; structured JSON preserved and `success=false`. |
| Invalid provider inputs fail locally | `google_news`, unsupported provider, and non-string provider trajectories on connected runtime | pass | Local validation errors are observed before MCP transport in production adapter path; no transport observer counts these individual calls. |
| Private child env/config and inherited sentinel isolation | Unix test-only launcher observation records cwd, HOME, XDG paths, explicit config argument, configured sentinel, and absence of parent secret/proxy/sentinel | pass on Linux | Windows and macOS were not run; the observer is Unix-only. No OS network sandbox is claimed. |
| Stronger gate catches config mismatch after handshake | Separate poisoned `mode="live"` child passes handshake and ten-tool inventory and fails the Mode-Off config assertion | pass | This is the injected negative control. |
| Shared process and bounded teardown | Both wrappers retain same MCP handle; explicit shutdown has a five-second bound and Unix PID disappearance assertion; RAII drop guard covers panic cleanup | pass on Linux | Tests added for bounded local MCP discovery and call timeouts. |
| Fake-MCP contract regression and repository quick verification | Pair harness fake-MCP suites and `scripts/verify.sh quick` | pass | Coverage boundary remains explicit; fake-MCP does not substitute for the two real-child denial trajectories. |
| Local-only repository request remains offline in v0.4.1 | Exploratory `repo_search(include_local=true)` with a private fixture | **fail / high finding** | It initiated remote DuckDuckGo provider dispatch under Mode Off. Do not repeat against v0.4.1. Owned by blocked C002. |
| Wrong-version, unreadable-config, incompatible-schema and OS matrix | Not run as isolated mutated harness cases; current script checks exact version and source tag identity | not run | Not represented as passing evidence. Windows/macOS and hosted CI were unavailable. |

## 3. Production implementation evidence

The production `McpLocalClient` test seam gained Unix-only child environment observation for the real-process test and bounded timeout/reaping regression cases for discovery and tool calls. No shipped search routing, provider adapter behavior, MCP model exposure, protocol, or storage migration changed. The real pair test now constructs private config/state/workspace roots, passes absolute `--config` and child env through the production env-clearing launcher, and validates two wrappers against one shared MCP connection. The runner labels stages, enforces immutable source tag identity, uses Cargo offline mode, and supports an explicit supplied binary. `architecture/mcp.md` documents the pair contract and the upstream local-only routing issue.

## 4. Verification executed

### Commands run

```bash
CODEGG_EGGSEARCH_BIN=/tmp/codegg-eggsearch-pair.YhvR8P/target2/release/eggsearch rtk proxy scripts/test-eggsearch-pair.sh
```

The script ran formatting, provider contract tests (29), bootstrap tests (22), research adapter tests (18), search backend tests (9), provider mapping tests (18), fake MCP tests (30), local MCP lifecycle tests (5), the real subprocess qualification, and `scripts/verify.sh quick`.

### Results

- Pair harness: pass on Linux; real-process assertions passed for identity, ten tools, provider inventory, two wrapper policy denials, structured fidelity, private child environment, poisoned-config negative control, invalid provider inputs, and teardown. Final gate reported `live_provider_calls=0`.
- `scripts/verify.sh quick`: pass on the same implementation revision.
- Real-child wrappers: 2; upstream provider searches in the final harness: 0.
- Separate exploratory local-repository probe: failed the intended offline/local-only property by initiating remote provider dispatch. This is a high finding, not a successful fixture trajectory.
- No hosted CI, macOS, Windows, full verification, or release/archive validation was run.

## 5. Invariant review

- Native CodeGG wrappers remain the only model-facing search surface; raw MCP tools remain hidden by exposure policy: pass.
- Wrapper responses preserve upstream structured policy errors and do not turn `isError` into successful search: pass for the two qualified wrappers.
- Search runtime retains one bootstrap-owned MCP session across wrapper requests: pass.
- The test does not claim packet-level or OS-enforced network containment: pass; documentation states this limit.
- v0.4.1 `repo_search(include_local=true)` is not safe as an offline fixture based on the observed defect: fail; C002 owns correction/requalification.

## 6. Failure and recovery review

The real-child test uses bounded MCP startup/call paths, explicit bounded shutdown, and a Unix test-only RAII kill guard for assertion unwinds. Added local MCP tests cover hung discovery and hung tool-call timeout/reaping. The final test confirms the child PID is gone and private test data is removed after success. Malformed executable, deliberate wrong-version binary, unreadable config, and schema-mutation scenarios were not individually executed. The upstream exploratory dispatch demonstrates that Mode Off must not be assumed to suppress provider adapters for every tool family.

## 7. Migration and compatibility review

No storage/config migration or public protocol change. The production search contract remains pinned to v0.4.1 for this branch. Legacy backend retirement M002 is still blocked; this closure does not authorize deletion of the duplicate backend. A future immutable upstream tag and CodeGG pin change must be reviewed under C002 before retirement.

## 8. Security review

The test uses no provider credentials and sanitizes child environment through the existing `env_clear()` launcher. Parent-only secret and proxy sentinels were observed absent in the child; private config and child paths were observed on Linux. This is harness isolation, not a network sandbox. The exploratory repository trajectory initiated remote dispatch despite the local-only/Mode-Off intent; no credentials were provided, and the trajectory was stopped rather than repeated. C002 requires a deterministic upstream zero-provider-adapter regression before the route can be used again.

## 9. Documentation and operations

- Updated `architecture/mcp.md` with actual wrapper qualification and the v0.4.1 local repository routing limitation.
- Updated the paired compatibility roadmap and implementation plan status.
- Added blocked C002 as the owner for upstream correction and exact-tag requalification.
- Primary command remains `scripts/test-eggsearch-pair.sh` with either immutable source checkout or explicit prebuilt binary; Cargo is run offline.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Eggsearch v0.4.1 `repo_search(include_local=true)` can dispatch remote DuckDuckGo providers while configured Mode Off | Local-only repository search cannot be treated as offline; destructive legacy retirement could remove a fallback before this contract is understood | Blocked C002: corrected immutable upstream tag with fake/counting adapter zero-call tests, then CodeGG paired requalification. Do not retest this route against v0.4.1. |
| medium | Cross-platform child environment evidence and several negative startup/schema cases were not run | No macOS/Windows portability or malformed-install matrix claim | Run those cases on supported platforms as available; they are not a basis to claim strict closure now. |

## 11. Roadmap disposition

C001 is conditionally closed: implementation and its bounded Linux qualification are complete, but the named upstream correctness finding prevents strict closure. C002 is blocked pending an external immutable eggsearch release with deterministic evidence that Mode-Off local-only repository search enters no remote provider adapter. Legacy-retirement M002 may **not** begin destructive removal. No user-facing product release or full CodeGG operability claim is made.

## 12. Registry updates

- Mark C001 conditionally closed and register C002 as blocked in the paired compatibility roadmap and registry.
- Keep legacy-retirement M002 blocked; its new precise blocker is corrected immutable upstream local-only behavior plus C002 paired requalification.
- No other registered plan is unblocked. Evidence-workflow M002–M004 statuses and M005 dependencies are unchanged.
