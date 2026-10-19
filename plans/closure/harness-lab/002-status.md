# Developer Harness Lab Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-lab/002-acp-unattended-execution-contract.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-roadmap.md#milestones`

Repository baseline reviewed: `7a1b9d9a1d5a28e7919e3d69851a3ae710b0afbf`

Implementation commits or pull requests:

- `d64da3d8951d743b6aae8a29f6fa3a390e6d4cb5` — ACP unattended policy and truthful terminal outcomes

## 1. Executive finding

M002 is closed. ACP can select the existing daemon-owned policy through the shared CLI policy parser, report requested and effective policy provenance, and preserve real failure and denial outcomes.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit unattended workspace-write and full-host policy | live local fixture ACP trials | pass | Both reached real tool calls without an Ask. |
| Explicit Bash denial remains effective | live local fixture denial trial | pass | Denial was returned by the tool execution path. |
| Requested/effective policy and revision receipt | `src/acp.rs` policy lease and receipt | pass | Uses daemon policy snapshot and compare-and-set update. |
| Native failed turn is not cancellation or success | ACP `TurnFailed` handling and focused tests | pass | Emits JSON-RPC error `-32010`; permission pending has a distinct `-32011`. |
| Session/workspace association and projection correlation | ACP session registration/subscription code and focused tests | pass | Correlation uses nested projection subscription/session/turn/cursor data. |
| Existing interactive ACP compatibility | `acp_stdio` integration test | pass | Default policy remains unchanged unless explicit policy flags are supplied. |

## 3. Production implementation evidence

`codegg acp` now accepts the shared flattened policy flags. Explicit yolo plus workspace-write or full-host requires a policy lease and explicit daemon root, performs a CAS update, verifies the effective snapshot and records requested/effective policy and revision. ACP session creation/loading registers the project and subscribes to its session events. Native turn failure and permission-pending conditions are reported distinctly.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --test acp_stdio
cargo test -p codegg --lib acp
cargo test -p codegg --bin codegg acp_policy_flags
python3 -m unittest discover -s dev/harness_lab/tests -v
scripts/verify.sh quick
```

### Results

`acp_stdio`: 1 passed. ACP library filter: 10 passed. CLI flag filter: 1 passed. Python suite: 12 passed. Quick verification passed locally. Live local fixture trials covered workspace-write, full-host, and explicit Bash denial; no external provider calls were made. No hosted CI run is claimed.

## 5. Invariant review

The daemon remains the policy authority. The adapter does not weaken explicit Deny, does not infer policy from a successful preflight, and does not turn native failure into cancellation. Existing clients retain their default effective policy unless they request the new explicit flags.

## 6. Failure and recovery review

Policy lease acquisition and snapshot verification fail closed on mismatch. Turn failure, user cancellation, and pending permission are distinct terminal/event outcomes. The adapter does not retry an ambiguous tool or turn outcome.

## 7. Migration and compatibility review

No database migration or wire schema change. Existing ACP clients keep the existing default behavior; additive CLI policy flags are parsed through the existing shared policy definitions.

## 8. Security review

Explicit unattended modes require an explicit daemon root and a runtime policy lease. Requested policy is not treated as effective until the daemon confirms its snapshot. Credentials are not included in receipts. Denial remains enforced by the broker/tool path.

## 9. Documentation and operations

Updated `architecture/acp.md`, `docs/cli.md`, and `docs/daemon.md`. Operator instructions explain explicit roots, policy selection, and terminal outcomes.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Live trials used a local fixture/provider and did not exercise a remote provider | Remote provider interoperability is outside this milestone's evidence | Keep provider-specific qualification in a future plan if needed. |

## 11. Roadmap disposition

Milestone closed. M001 and M002 are both closed; M003's hard dependency is discharged and M003 may proceed. M004 remains gated on M003 closure.

## 12. Registry updates

M002 is removed from active work and listed as closed. M003 moves from blocked to dependency-ready; M004 remains blocked on M003.
