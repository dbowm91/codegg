# Memory-to-Skill Promotion Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/memory-skill-promotion/005-publication-clippy-and-hosted-closure.md`

Source corrective roadmap/addendum:

- `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md`

Long-term requirements reviewed:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md#7-corrective-passes`

Accepted executable revision: `28a0cb41f80621726b9d8e0e4e5f93ee4d828970`

Implementation commits:

- `8c8febe` — fix(memory): close publication clippy findings
- `184fd07` — fix(verify): satisfy hosted review clippy ordering (separate DVR M008 follow-up)
- `28a0cb4` — fix(verify): align built-in agent test expectations (separate DVR M009 follow-up)

## Executive finding

M005 is complete. All six M002/M003-owned Workspace Clippy findings exposed by
the M004 exact-head hosted candidate were corrected with behavior-preserving
changes, and the final accepted candidate passed the normal hosted `CI /
verify` lane through Workspace Clippy and Workspace tests.

The M005 production implementation is limited to proposal/publication code:
argument bundles reduce Clippy argument counts, all proposal/publication
advisory lock opens explicitly use `.truncate(false)`, and one needless borrow
is removed. No proposal or publication schema, root, parser, precedence,
provenance, reconciliation, digest, refresh, or approval contract changed.

The hosted candidate also exposed two unrelated verification defects after the
M005 Clippy phase: ReviewTool module ordering and stale built-in-agent test
expectations. They were registered and closed as DVR M008 and M009 in separate
commits, without broadening M005's production scope.

## Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Correct all six hosted findings | `src/skills/promotion.rs` and `src/skills/publish.rs` use argument bundles, explicit non-truncating lock opens, and the corrected path join | pass |
| Preserve proposal/publication behavior | Existing proposal/publication tests plus the unchanged schemas, roots, parser, precedence, digest, and refresh paths | pass |
| Preserve lock-file contents before advisory ownership | `promotion_lock_contents_are_preserved` and `publication_lock_contents_are_preserved` regression tests pass; all synchronization opens use `.truncate(false)` | pass |
| Preserve core habit and memory behavior | `cargo test -p codegg-core habit --locked` — 5 passed; `cargo test -p codegg-core memory --locked` — 23 passed | pass |
| Preserve promotion/publication integration behavior | Arm64 isolated-target integration run — 24 tests passed across `habit_skill_promotion` and `skill_publication` | pass |
| Exact local workspace Clippy passes | `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed on the accepted candidate after separate M008/M009 corrections | pass |
| Quick verification passes | `scripts/verify.sh quick` — passed | pass |
| Exact-head hosted verification passes through Workspace tests | GitHub Actions run `33841790039`, rerun verify job `100930040217`, exact accepted revision `28a0cb41`, passed | pass |

## Production implementation evidence

`SkillPromotionStore::submit` now receives the public
`SkillProposalSubmission` request bundle. `publish_locked` receives a private
`PublishLockedInput` bundle. The bundles preserve field values, call ordering,
lock scope, ownership, error mapping, and persistence sequencing.

The promotion lock open and both publication lock opens explicitly retain
existing lock-file bytes with `.truncate(false)`. The marker-preservation tests
exercise the observable synchronization-file invariant. The reported needless
borrow in publication path construction was removed without changing the
derived path.

No lint suppression, CI weakening, toolchain pin, schema change, migration,
filesystem-root change, publication-authority change, or generic filesystem
redesign was introduced.

## Verification executed

Local results:

- `cargo fmt --all` and the formatting phase of `scripts/verify.sh quick` —
  passed.
- `git diff --check` — passed.
- `cargo test -p codegg-core habit --locked` — 5 passed.
- `cargo test -p codegg-core memory --locked` — 23 passed.
- The required default root integration command reached an environment-only
  x86 linker mismatch because `/opt/local/lib` supplied arm64 `liblzma` and
  `libiconv` to the x86 toolchain; it was not treated as evidence.
- The same integration tests with explicit arm64 Rust tooling, target
  `aarch64-apple-darwin`, and an isolated target directory — 24 passed.
- `cargo clippy -p codegg --lib --locked -- -D warnings` — passed for the
  M005 production surface.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed
  after the separately governed M008/M009 verification corrections.
- `scripts/verify.sh quick` — passed, including generated-agent schema,
  core-boundary, sandbox, execution-ownership, formatting, and workspace
  all-target checks.

Hosted exact-head evidence:

- Final run `33841790039` —
  [GitHub Actions run](https://github.com/dbowm91/codegg/actions/runs/33841790039).
- Accepted rerun verify job `100930040217` —
  [hosted verify job](https://github.com/dbowm91/codegg/actions/runs/33841790039/job/100930040217).
- The job passed formatting, all static guards, Workspace Clippy, Workspace
  tests, cache teardown, and completion on the exact accepted revision.

Historical hosted evidence is preserved: M004 candidate run `33836217483`
failed on the six findings M005 owns; M005 candidate run `33838526507` then
failed on the unrelated ReviewTool ordering finding; the next candidate
`33839695302` exposed stale agent assertions; and the final candidate's first
attempt exposed one independent Tool Programs M015 failpoint test that passed
on the accepted failed-job rerun. None of those historical failures were
hidden or repaired by changing M005 scope.

## Invariant, failure, recovery, contention, and compatibility review

The proposal path remains explicit user-triggered and model-inaccessible for
approval/publication. Publication remains host-owned, CodeGG-rooted, path-safe,
atomic, durable, collision-safe, and digest/revision-bound. Lock acquisition
still precedes load/mutate/save or publication/reconciliation work, and the
existing cleanup/error behavior is unchanged. Existing lock inodes are not
truncated before advisory ownership is acquired.

No persistence format, protocol, path root, parser rule, skill precedence,
asset-refresh behavior, migration, cancellation, restart, or compatibility
boundary changed. No sensitive data, permissions, authority, or logging path
was added.

## Unresolved findings

None within M005. The independent DVR M008 and M009 findings are separately
closed in their own records. The first-attempt Tool Programs M015 test result
was not reproduced on the accepted hosted rerun and remains outside M005.

## Roadmap and downstream disposition

M005 is strictly closed. Its implementation plan, corrective addendum, and
registry entry now point to this record and show the memory-to-skill subsystem
closed. M001–M004 closure records remain immutable historical evidence; this
record reconciles their later hosted findings without rewriting them.

The dependency audit found no registered future plan with M005 as a hard or
interface dependency. Therefore no future plan was unblocked or promoted by
this closure. The dependency-ready table remains empty. The unrelated
supported-Linux Landlock condition remains under its existing conditional
closure and is not affected by M005.
