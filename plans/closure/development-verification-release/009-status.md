# Development Verification and Release Milestone 009 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/development-verification-release/009-builtin-agent-test-contract-alignment.md`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md#milestone-009--built-in-agent-test-contract-alignment`

Accepted executable revision: `28a0cb41f80621726b9d8e0e4e5f93ee4d828970`

Implementation commit:

- `28a0cb41` — fix(verify): align built-in agent test expectations

## Executive finding

DVR M009 is complete. The eight stale unit-test expectations exposed after
hosted Workspace tests resumed were aligned with the checked-in built-in-agent
assets: ten built-ins, including the hidden prompt-bearing `verifier` agent.
Only test expectations changed. Built-in definitions, prompts, permissions,
generated assets, registry resolution, runtime behavior, Clippy policy, and CI
configuration were not changed.

The first hosted attempt on this revision exposed one independent Tool Programs
M015 daemon-failpoint test failure. The failed job was rerun under the same
hosted run and passed; M009 does not claim ownership of that unrelated test.

## Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Align all stale built-in-agent counts and names | `src/agent/mod.rs` and `src/agent/registry.rs` expectations now match ten checked-in built-ins, including `verifier` | pass |
| Preserve the hidden prompt-bearing-agent contract | The hidden-agent test allows the documented `compaction` and `verifier` exceptions only | pass |
| Avoid production asset/runtime changes | Diff is limited to stale test assertions and planning evidence | pass |
| Focused agent tests pass | `cargo test --target aarch64-apple-darwin -p codegg agent --locked` — 425 passed, 7,066 filtered | pass |
| Workspace Clippy passes | `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed | pass |
| Quick verification passes | `scripts/verify.sh quick` — passed | pass |
| Exact-head hosted verification passes through Workspace tests | GitHub Actions run `33841790039`, rerun job `100930040217`, exact head `28a0cb41`, passed Workspace Clippy and Workspace tests | pass |

## Verification executed

Local results:

- `cargo fmt --all` — passed; the quick verification formatting check passed.
- `git diff --check` — passed.
- `cargo test --target aarch64-apple-darwin -p codegg agent --locked` with
  matching arm64 Rust tooling and an isolated target directory — 425 passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `scripts/verify.sh quick` — passed, including generated-agent schema,
  boundary, sandbox, execution-ownership, formatting, and workspace checks.

Hosted exact-head evidence:

- Run `33841790039` —
  [GitHub Actions run](https://github.com/dbowm91/codegg/actions/runs/33841790039).
- Rerun verify job `100930040217` —
  [hosted verify job](https://github.com/dbowm91/codegg/actions/runs/33841790039/job/100930040217).
- The rerun passed formatting, all static guards, Workspace Clippy, Workspace
  tests, cache teardown, and job completion on the exact accepted revision.

The initial job `100925401116` failed only at the independent
`recursive_descendants_and_capacity_converge_after_cancel_crash` Tool Programs
M015 test. Its failed-job rerun passed, so no M009 code was broadened to that
subsystem.

## Invariant, failure, and compatibility review

The change does not alter agent lookup, ordering, prompt loading, permission
resolution, hidden-agent visibility, generated assets, or runtime execution.
The updated assertions make the tests reflect those existing contracts. No
failure, cancellation, restart, persistence, protocol, storage, or migration
semantics changed. No sensitive data or authority boundary was introduced.

## Unresolved findings

None within M009. The independent Tool Programs M015 test failure observed on
the first hosted attempt is not owned by this milestone and was not reproduced
by the accepted rerun.

## Roadmap and downstream disposition

M009 is strictly closed. The development-verification roadmap/addendum and
registry now identify M009 as closed and link this record. M008 remains active
as the separately owned ReviewTool ordering correction; M009's completion
leaves that work eligible to complete its own closure evidence, but does not
change its status or absorb its scope.

An audit of registered dependencies found no other future plan blocked on M009,
so no unrelated future plan was unblocked or promoted. M005 memory-to-skill
closure remains separately governed and is not rewritten by this record.
