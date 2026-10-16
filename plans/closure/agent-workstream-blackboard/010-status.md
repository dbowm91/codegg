# Agent Workstream Blackboard M010 — Strict Closure

Status: closed — deterministic isolated Rust/Python coordination qualification

Source plan: `plans/implementation/agent-workstream-blackboard/010-isolated-rust-python-trajectory.md`
Implementation commits: `469802dd`, `70405abd`
Exact qualified source head: `3d395876db3d88bad692596c15f892b0494a165c`
Hosted CI: [`38850114895`](https://github.com/dbowm91/codegg/actions/runs/38850114895) — success
Hosted Desktop E2E: [`38850114846`](https://github.com/dbowm91/codegg/actions/runs/38850114846) — success

## 1. Result

M010 closes for deterministic functional evidence that two run-owned task claims
can coordinate work on separate physical Git worktrees, run distinct Rust and
Python checks, see both authorized group claims, and integrate sequentially
into a clean parent checkout. The worktree roots remain isolated before
integration. The claim and workspace mechanics work without shared-checkout
execution; ADR-0002 isolation remains effective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Two distinct run identities hold separate task claims in one group | `tests/workstream_claim_m003.rs::isolated_rust_and_python_workers_claim_build_test_and_integrate_independent_changes` | pass |
| Workers receive distinct physical roots and edits do not cross before integration | Two local Git linked worktrees; assertions check the Python worker file is absent from the Rust worktree and the Rust worker function is absent from the Python/base source | pass |
| Language-specific verification succeeds | `cargo test --quiet` in the Rust worktree; `python3 -m unittest discover -s tests` in the Python worktree with bytecode disabled | pass |
| Authorized peer can observe both task claims | `WorkstreamClaimStore::list_group` returns both claim IDs to a group member | pass |
| Parent integration is explicit and leaves no dirty state | Separate Rust and Python commits, sequential `git merge --no-ff`, final empty `git status --porcelain` | pass |
| Existing M004 peer fence remains qualified independently | M009 strict closure, including `peer_evidence_arriving_during_provider_stream_fences_returned_tool_batch` | pass; no M004 code changed here |

## 3. Verification

- `cargo fmt --all -- --check` — passed.
- Hosted CI run `38850114895` — passed formatting, workspace Clippy, all
  guards, and the complete nextest workspace suite: **13,961 passed, 0 failed,
  6 skipped**. The trajectory passed at test 10,667.
- Hosted CI run `38850114895` also ran `replay_p95_within_budget` in the
  CPU-exclusive group; it passed at test 6,366.
- Hosted Desktop E2E run `38850114846` — passed.

The test-owned Git fixture is not a qualification of production
`WorktreeService` allocation. It proves the Git worktree isolation mechanics
and claim/integration composition allowed by M010's test-fixture policy.

## 4. Limitations and unresolved findings

- Worker actions are deterministic scripted file changes, not provider-backed
  autonomous AgentLoop sessions. This is functional capability evidence only;
  it makes no claim that claims or peer observations improve model success,
  latency, cost, or throughput.
- M004 peer observation and pre-mutation fencing is covered by the separate
  strict M009 evidence; M010 does not replay that event inside this Git fixture.
- Production-managed worktree allocation and shared-writer containment remain
  outside M010. Shared placement is still unavailable.
- No high- or medium-severity finding was introduced or remains open for this
  milestone. Prior failed CI attempts were corrected: the Python checkout's
  base Rust file is expected to exist, and the isolation assertion now checks
  only for the Rust worker's change.

## 5. Dependency and unblock audit

- M010's only prerequisites, closed M003 claims and strict M004 peer
  observation, are satisfied.
- No additional plan is unblocked by this functional fixture. M006 isolated
  session/worktree UX and M007 per-workspace build admission remain eligible
  independently of M005.
- M008 remains blocked on M006 and M007. Any shared-checkout arm remains
  separately gated on M005 writer containment.
- M005 remains active for design/qualification; this closure grants no shared
  checkout authority.

