# Team Collaboration Corrective M006 — Multi-User Trajectory and Security Qualification

Status: blocked

Repository baseline: `4e12ecc192ba7e2192d3a62acce6e15fc1bb1153`

Source roadmap: `plans/subsystems/team-collaboration-corrective-addendum.md#M006`

Long-term requirements: `plans/000-long-term-specification.md#8`, `#14`, `#15`, `#21`, `#26`, `#27`, `#29`.

Applicable ADRs: ADR-0006 and ADR-0007. Primary class: invariant.

Hard dependencies: M001-M005 strict closure.

## 1. Objective

Qualify the complete team collaboration trajectory with several principals/devices and adversarial cross-project actions. This milestone should add harness/tests and only the smallest production corrections required by discovered defects.

## 2. Why this milestone is blocked

It is the final gate and requires all production semantics to be stable.

## 3. Current implementation evidence

Existing milestone suites test identity authorization, observation, collaboration, chat TUI, structured actions, Workspace dashboard, and task trajectories mostly within their subsystem boundaries. The corrective campaign changes cross-boundary policy, so a single integrated multi-user harness is required.

## 4. Invariants that must not regress

All invariants from M001-M005 are cumulative. In particular: no REST/Core authorization divergence, no chat-to-execution escalation, no implicit shared-turn control, no cross-project Workspace/chat routing, and immediate revocation at new request boundaries.

## 5. Scope

In: deterministic temporary catalog/daemon/server, multiple human principals and tokens, two projects, several roles, observer/chat/control/Workspace flows, reconnect/restart, revocation, contention, event isolation, secret-negative census, static guards.

Out: new feature design, performance benchmarking beyond boundedness checks, distributed multi-node execution, OIDC.

## 6. Required production changes

None expected. If qualification exposes a defect, fix only if it is a direct violation of an accepted M001-M005 invariant and record the production delta in closure. Larger architectural discoveries require a new corrective plan.

## 7. Ordered work packages

A. Build a reusable multi-principal fixture with LocalOwner, Owner, Contributor, Viewer, outsider, multiple device tokens, two projects/channels/sessions.
B. Exercise HTTP/Core parity and event isolation before collaboration flows.
C. Exercise chat policy matrix including Viewer grant, Contributor deny, restricted channel and structured-action negatives.
D. Exercise team administration and token/membership revocation while clients are connected.
E. Exercise shared-session observe/chat/control request/transfer/takeover, permission/question and reconnect/restart.
F. Exercise Workspace selection with ordinary composer plus side-panel chat across rapid project changes.
G. Run stress/race cases and negative secret/content census; document exact evidence.

## 8. Failure, cancellation, restart, contention semantics

Inject disconnects between request and response, duplicate idempotent requests, stale admin revisions, controller transfer vs turn completion, policy change vs chat send, membership revoke vs event delivery, and Workspace selection changes vs async completions. The expected outcome is convergence or typed failure with zero cross-project mutation/leak.

## 9. Compatibility and migration

Open a pre-corrective database fixture containing ordinary roles/channels/sessions and prove role-default chat compatibility plus additive migrations. LocalOwner solo mode must still work without team setup.

## 10. Required tests

At minimum one end-to-end scenario should model Alice Owner, Bob Contributor, Carol Viewer, and Mallory outsider across Project A/B. Carol receives chat only in one allowed scope; Bob is denied one restricted channel; Mallory learns no hidden identities. Alice/Bob transfer an active turn; non-controller mutation fails. Membership revocation terminates new chat/control/event access. Workspace rapid switching never cross-routes chat or prompt.

## 11. Required verification commands

- all focused M001-M005 test suites
- `cargo test --test identity_m003_daemon_authorization`
- `cargo test --test presence_m003_observation`
- `cargo test --test collaboration_m001_chat --test collaboration_m002_chat_tui --test collaboration_m003_chat_actions`
- new multi-user trajectory harness
- `python3 scripts/check_authorization_matrix.py`
- all relevant static authority/redaction guards
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

## 12. Documentation updates

Update the corrective roadmap status, architecture docs for any factual corrections, and create `plans/closure/team-collaboration-corrective/006-status.md` with the complete requirement-to-evidence matrix.

## 13. Acceptance criteria

The multi-principal harness demonstrates the intended observer+chat, team-space restriction, membership administration, explicit controller handoff, and Workspace selected-project chat behavior while all cross-project and privilege-escalation probes fail closed.

## 14. Stop conditions

Do not waive a high/medium authorization, privacy, controller, migration, or cross-project routing defect. Do not mark the campaign closed based only on compilation or subsystem unit tests.

## 15. Closure evidence required

Exact commit(s), migration versions, test commands/results, adversarial matrix, reconnect/restart/race outcomes, secret-negative census, compatibility result, and zero unresolved high/medium findings.

## 16. Handoff notes

This is the final closure gate. On acceptance, update the roadmap to closed, move registry rows from blocked/ready to recently closed, and audit downstream blocked work.
