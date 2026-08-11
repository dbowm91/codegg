# Agent Runtime Correctness, Autonomy, and Simplification M009 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/009-integration-documentation-and-closure.md`

Source subsystem roadmap: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`

Repository baseline: `5449aa2f` (M008 closure)

## 1. Outcome

M009 integration work is complete and formally closed. Hosted `CI / verify`
run `31515706555` passed on final candidate `c5154701`. The final pass reconciled active
documentation, corrected the stale project-catalog guard, repaired the broker
principal binding exposed by the integration harness, and updated stale
harness expectations to the bounded M005 recovery contract. No new product
architecture, protocol, storage migration, CI lane, or release automation was
introduced.

## 2. Implementation commits and predecessor disposition

| Milestone | Implementation / closure evidence | Status |
|---|---|---|
| M001 | `fb972426`, `001-status.md` | closed |
| M002 | `86f8f43`, `002-status.md` | closed |
| M003 | `8c2638db`, `003-status.md` | closed |
| M004 | `493fd596`, `004-status.md` | closed |
| M005 | `ddb495af`, `005-status.md` | closed |
| M006 | `4cd004db`, `006-status.md` | closed |
| M007 | `deb07a2a`, `007-status.md` | closed |
| M008 | `66326ad8`, `008-status.md` | closed |

The M009 implementation/closure commits are `7d57a34b` (integration,
documentation, and initial closure record) and `c5154701` (explicit workspace
roots in the remaining subagent integration fixtures). Hosted evidence is
`31515706555` / job `93860194586`.

## 3. Requirement-to-evidence matrix

| Roadmap exit condition | Evidence | Result |
|---|---|---|
| Unknown/raw MCP tools cannot bypass `Ask` | M001 authority tests; `cargo test -p codegg --test permission`; harness authority path | pass |
| External provenance is truthful | M001 receipt/provenance tests; `architecture/permission.md` and `architecture/tool.md` | pass |
| MCP identity/schema changes invalidate cache | `mcp_surface_revision_detects_equal_count_schema_changes`; cache docs | pass |
| Generic prose is not executable | M002 parser tests; `text_tool_parser` 4 passed; provider docs now describe only bounded profiles | pass |
| Fragile-model repair remains explicit/bounded | `hermes_xml`, `invoke_json`, and `raw_json_envelope` adapter contract and fixtures | pass |
| AgentLoop/snapshot workspace binding is explicit | M003 workspace tests; daemon-CWD guard; harness subagent workspace-root fixture | pass |
| Current-turn heuristics use current input | M004 `current_turn_prompt_uses_latest_user_message` | pass |
| Goal accounting does not recharge history | M004 `accounting_deltas_are_distinct_from_cumulative_limits` | pass |
| One terminal `AgentFinished` owner | M004 terminal tests; full harness lifecycle tests | pass |
| Recovery is one bounded state machine | M005 recovery tests; harness malformed-output and no-bootstrap expectations | pass |
| Startup prompt/control has one authority | M006 prompt compiler tests and fingerprint assertions | pass |
| Supported features retained during footprint work | M007 plugin feature compile/tests and retained-feature review | pass |
| Final release footprint recorded | M007 same-host measurements: default `54,430,624` bytes vs baseline `54,437,200`; plugin `72,594,280` vs `75,795,560` | pass |
| Wasmtime security disposition current for supported line | M007 lock at `36.0.13`, fixed for RUSTSEC-2026-0222; latest major deferred for MSRV compatibility | pass |
| Routine CI remains one bounded job | `.github/workflows/ci.yml`, `scripts/verify.sh`, M008 guard disposition | pass |
| Active docs match final behavior | M009 documentation reconciliation below | pass |
| Quick and broad verification pass | quick, Clippy, focused harness, `cargo test --test subagent` (22 passed), and hosted `31515706555` | pass |

## 4. Cross-milestone integration evidence

The final authority/workspace/autonomy integration surface passed:

- `cargo test -p codegg-providers text_tool_parser -- --nocapture`: 4 passed.
- `cargo test -p codegg --lib agent::r#loop::tests -- --nocapture`: 40 passed.
- `cargo test --test agent_loop_harness -- --test-threads=1`: 40 passed.
- The harness now proves native authority/provenance dispatch, permission
  allow/deny behavior, subagent denied-tool filtering with explicit workspace
  root, current terminal lifecycle, malformed structured-tool handling, and
  no synthetic bootstrap for strong-model final answers.

The initial broad run exposed the same principal mismatch and three stale
M005/M003 harness assumptions. Those were corrected narrowly; the focused
harness then passed without weakening production authority checks.

## 5. Verification evidence

Passed locally on the final working tree:

```text
scripts/verify.sh quick
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

The quick tier passed formatting, generated-agent validation, core boundary,
sandbox contract, execution ownership, and locked workspace all-target
checking. The full workspace test run before the final harness correction
reported 4,173 unit tests passing and 10 stale harness failures; the corrected
harness passed all 40 tests. Hosted run `31515706555` passed the existing
`.github/workflows/ci.yml` `verify` job, including the locked full workspace
test command.

## 6. Static-guard sweep

The surviving M008 authority/security/workspace guard sweep passed, including
daemon CWD, project-agent PWD inference, discovery, scheduler bypass,
execution ownership, git policy, identity paths, TUI authority, tool broker,
provider-connection lifecycle/tombstone compatibility, projection disclosure
and transport, and WebSocket bounds. The project-catalog guard initially
reported its own stale expectation of layout version 33; it now checks the
authoritative version 35 and passes.

## 7. Documentation reconciliation

Updated active documentation/evidence:

- `architecture/provider.md`: removed the stale generic parser API and fenced/
  `invoke(...)` descriptions; documented the explicit bounded repair profiles.
- `architecture/agent.md`: documented digest-based MCP cache identity and
  removed the stale count-only limitation.
- `architecture/core.md`: clarified that `AgentLoop` owns `AgentFinished` and
  daemon completion/error events are distinct.
- `scripts/check_project_catalog_invariants.py`: aligned the active guard with
  storage layout version 35.
- `tests/agent_loop_harness.rs`: aligned integration fixtures with explicit
  workspace authority and bounded recovery/no-bootstrap behavior.

The reviewed permission, goal, cache-aware-context, tool, testing, and agent
sections otherwise match the M001-M008 closure records. No deferred idea is
presented as a current requirement. Routine CI remains one bounded `verify`
job; release remains manual.

## 8. Security and compatibility review

No permission bypass, workspace escape, authority broadening, protocol change,
storage migration, or supported-feature removal was found. The principal fix
uses the grant issuer's principal consistently; decision identity is no longer
mistaken for principal identity. Textual repair remains adapter-owned and
normal permission/broker checks remain mandatory.

## 9. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| critical | none | no action |
| high | none | no action |
| medium | none after the M009 principal, guard, and fixture corrections | no corrective pass |
| low | M001 retains a bounded digest rather than a service-owned monotonic MCP revision | deferred; no correctness gap |
| low | M007 remains on maintained Wasmtime 36.x rather than latest major | deferred until MSRV/compatibility evidence justifies change |
| info | daemon/TUI split, dependency replacement, new CI lanes, release automation, and other roadmap deferred ideas | intentionally unregistered |

## 10. Recommendation

Closed strictly after the hosted `CI / verify` workflow passed on the final
candidate. No corrective plan is required; the workstream is complete.

## 11. Registry and roadmap updates

The final closure update:

- mark M001-M009 `closed` in the subsystem roadmap;
- mark the subsystem `closed` in `plans/registry.md`;
- remove M009 from dependency-ready and active closure sections;
- retain the latest M009 entry under recently closed work;
- audit blocked work and record that no registered future plan is blocked on
  this subsystem, so no downstream plan becomes `ready`.

No future registered plan currently lists M009 as a dependency. The blocked
work audit found no corrective pass or dependency-ready downstream plan to
unblock or register; all other blocked/conditional work remains independently
scoped.
