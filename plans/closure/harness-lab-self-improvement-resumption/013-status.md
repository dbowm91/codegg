# Harness Lab Self-Improvement S013 — Live Diagnostic Campaign

Status: **in progress**. User authorization allows up to 20 Muse Spark 1.3
attempts through CodeGG's existing `opencode_go` route. One live attempt has
completed; the next attempt will use the two narrow observability/preflight
correctives recorded below. S013 is not a stable-live qualification.

## Authorization and bounds

Authorization provenance: user instruction in this conversation on
2026-10-31. Exact scope is recorded in
[`evidence/s013-live-authorization-20261031.json`](evidence/s013-live-authorization-20261031.json):

- Route: `opencode_go/muse-spark-1.3-contributor` only.
- At most 20 attempts; run and inspect each separately.
- Per attempt: 60 provider requests, 3600 seconds, `workspace-write`,
  10 GiB minimum free disk, and 300 seconds without observed activity.
- Cancel on operator request, any cap, disk-floor breach, uncertain request or
  event accounting, or unexpected private-verifier access.
- USD cost may be absent; record it as unknown, never as zero.

The no-provider admission refusal caused by the nonexistent lane ancestor is
recorded as a zero-request setup refusal and does not consume a live-model
attempt. It exposed and led to the disk-probe correction described below.

## Attempt 1

Exact sanitized receipt: [`evidence/s013-live-attempt-01.json`](evidence/s013-live-attempt-01.json).
The run used the frozen live manifest
`dev/harness_lab/fixtures/agent_autonomy_s013_live/cases.jsonl`, the native
ARM64 CodeGG binary, and source revision `e89ae1e2e602e894f2504ccce999fe924e6feec4`.
The receipt separately records the Python working-tree patch digest present
during this run.

| Measure | Result |
|---|---|
| Agent terminal | `completed` |
| Task outcome | `failed`; independent verifier exited 1 (5 tests, 1 failure and 1 error) |
| Active horizon | 606,167 ms; not qualified for 30 minutes |
| Provider admissions | 32 of 60 |
| USD allowance reported | $0.019777 |
| ACP observation | 43 starts, 43 terminals, zero event gaps |
| Activity stall | Not triggered |
| Boundary helper/report | Not created; probe outcomes are incomplete |
| Delegated work at teardown | Two private-lane subagent runs and jobs still recorded `running` |

The agent made 43 tool calls, attempted both requested task delegations, and
completed several test submissions and edits. It did not complete the Rust
workspace, create the required helper/probe report, or show terminal status for
both delegated runs. This is a task/model outcome, not sufficient evidence for
a CodeGG task-execution defect. The lane database showed two nonterminal
delegated runs after the parent turn ended and the harness stopped its private
daemon. Whether that is an expected durable-run state or a teardown/observation
defect remains unresolved; attempt 2 must require the agent to wait for each
exact child ID and must record the terminal states.

The causal trace recorded valid `task` tool events as `unknown` because its
bounded tool allowlist omitted `task`. The main receipt still identified the
tool, but the causal diagnostic could not distinguish delegation. A narrow
allowlist fix and regression were added.

## Correctives before attempt 2

1. Disk preflight now walks to the nearest existing ancestor before calling
   `disk_usage`; this permits a fresh nested lane root while retaining fail
   closed behavior if the filesystem probe itself fails. A focused regression
   covers a not-yet-created lane.
2. The causal trace now retains the allowlisted `task` tool name. A focused
   regression confirms task events stay attributable without persisting
   arguments or output.

Both focused tests passed:

```text
test_disk_preflight_uses_nearest_existing_ancestor_for_new_lane ... ok
test_task_tool_is_preserved_for_delegation_diagnostics ... ok
```

## Remaining work

Continue with separately receipted, one-at-a-time live runs up to the
authorization limit. Prioritize valid tool and boundary observations, complete
subagent lifecycle evidence, the actual dependent engineering task, and
independent verification. Independently reproduce any suspected harness issue
before a corrective; do not infer one from task failure, incomplete probes, or
an unobserved child status. Keep task outcome and horizon qualification
separate. Do not claim `stable_live_evidenced` unless the distinct-campaign
requirements in S014 are later met.
