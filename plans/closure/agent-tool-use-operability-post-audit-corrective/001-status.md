# Agent Tool-Use Operability Corrective C001 — Closure Status

Status: blocked
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/001-broker-accepted-authority-projection.md`
Implementation commit: `590c713ea377f50a741a69284060f3fdd483bf12` (`plans/agent-tool-use-operability-20261019`)
Recommendation: blocked pending real agent → broker → native Tool Program evidence.

## 1. Finding and disposition

The authority projection defect is repaired at the CodeGG trust boundaries. The plan cannot strictly close because its required production-path Tool Program acceptance trajectory and pre-job negative cases were not implemented or run. This is an evidence/implementation gap, not an external operational prerequisite, so the status remains blocked rather than conditionally closed.

## 2. Changes landed

- `WorkspacePathPolicy::authority_fingerprint` derives a stable identity from the effective canonical/read/write roots.
- Agent tool context now obtains the workspace ID and path-policy identity from the actual workspace service lease and carries the verified permission revision/receipt. Synthetic workspace and authority identifiers were removed.
- Broker retries preserve the authority projection, and the broker validates the workspace path-policy identity. The scheduler Tool Program executor forwards the content-addressed revision.
- AgentLoop integration fixtures now acquire real workspace registry/service leases.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Real authority values, no synthesized IDs | `authority_fingerprint` unit test; source path through `build_tool_execution_context` and `execution_context_for_attempt` | Implemented; focused tests pass |
| Broker receipt/path-policy projection | `tool::broker`, `tool::tool_program`, `agent::tool_batch` focused suites | Pass |
| Real agent → broker → native Tool Program `emit(str(1+1))`, foreground and background | No such normal-path trajectory added; deterministic scenario tests use a broker double | Missing — blocks closure |
| Missing/mismatched/revoked/expired authority rejected before job creation | Existing authority contract tests cover field/scope mismatches, but no normal-path no-job assertion for all required cases | Partial — blocks closure |
| Retry/cancellation/restart authority behavior | Existing Tool Program lifecycle/fault suites are unchanged; this correction did not add an exact-path regression | Partial |

## 4. Before/after trace

Before: the normal agent call produced no workspace-policy identity/revision in the per-attempt context, while Tool Program context validation required verified decision and policy fields. After: the real lease supplies a content-addressed path policy and the accepted permission snapshot/receipt supplies the authorization fields. The strict after-state result through a native Tool Program remains unproven because the required production-path trajectory is absent.

## 5. Verification

Passed on implementation commit:

- `cargo test --locked -p codegg --lib tool::broker` — 17 tests.
- `cargo test --locked -p codegg --lib tool::tool_program` — 18 tests.
- `cargo test --locked -p codegg --lib agent::tool_batch` — 27 tests.
- `cargo test --locked -p codegg --test tool_program_scenarios` — 13 tests (scenario harness; not production agent path).
- `cargo test --locked -p codegg --test agent_loop_harness` — 62 tests.
- `scripts/verify.sh quick` — passed, including locked workspace all-target check.

Not run: the C001-required foreground/background normal-path Tool Program integration and missing/revoked/expired zero-job matrix; live Eggpool and SBC benchmarks are not required by this plan.

## 6. Compatibility and migration

No storage migration, tool-name change, or public protocol change. The policy fingerprint is ephemeral metadata derived from the immutable effective policy. Existing tool contracts remain unchanged.

## 7. Security, failure, cancellation, and restart

The implementation fails closed when a required workspace identity/lease is missing, and it does not treat model input or audit correlation IDs as authority. Mismatch tests and existing lifecycle suites provide partial negative coverage. The required normal-path proof that rejection occurs before scheduler job creation, including revoked/expired receipts and cancellation/restart, is still missing.

## 8. Contention and observability

This change adds no scheduler or global admission mechanism. Broker attempts retain the same authorization projection; policy identity is content-addressed. No new high-cardinality or secret-bearing telemetry was added.

## 9. Limitations and severity

Severity: medium correctness/qualification gap. Production code now has a genuine authority source, but C001's defining success condition is not demonstrated through the normal Tool Program caller path. No security bypass was found; strict closure is withheld.

## 10. Unblock audit

C007 remains blocked. C001 is a hard prerequisite and did not strictly close; no downstream milestone became eligible. A focused normal-path AgentLoop → broker → scheduler Tool Program trajectory plus pre-job denial assertions is the next eligible work.
