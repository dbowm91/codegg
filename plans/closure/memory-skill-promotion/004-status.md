# Memory-to-Skill Promotion M004 — Closure Status

Status: corrective pass required

Source implementation plan:

- `plans/implementation/memory-skill-promotion/004-habit-lock-open-options-and-hosted-closure-corrective-pass.md`

Source subsystem addendum:

- `plans/subsystems/memory-skill-promotion-hosted-verification-corrective-addendum.md`

Repository baseline reviewed: `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197`

Implementation commit:

- `7ef387aa0302efa3106b1d14ee166fd93e921cb9` — fix(memory): make habit lock opens non-truncating

## 1. Executive finding

M004's bounded production correction is complete. Both M001-owned habit-store
advisory-lock opens now explicitly preserve existing lock-file contents with
`.truncate(false)`, while lock paths, flock lifecycle, persistence, and
atomic JSON writes remain unchanged.

Strict M004 closure is not accepted because the exact hosted candidate reached
Workspace Clippy but failed on six older M002/M003 publication/proposal
findings. Per M004's explicit stop condition, those findings were not folded
into this narrow corrective pass. M005 is registered and ready to own them.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Both ambiguous habit lock opens state non-truncating behavior | `open_lock_file` in `crates/codegg-core/src/memory/habit.rs` is used by `observe` and `transition` and calls `.truncate(false)` | pass |
| No lint suppression, CI weakening, or toolchain downgrade | Production diff adds only the helper/call-site correction; CI workflow unchanged; no lint allowances added | pass |
| Habit persistence and concurrent writers remain green | `cargo test -p codegg-core habit --locked` — 5 passed, including 8-writer concurrency test | pass |
| Text-memory compatibility remains green | `cargo test -p codegg-core memory --locked` — 23 passed | pass |
| Promotion/publication compatibility remains green | arm64 isolated-target `cargo test --test habit_skill_promotion --test skill_publication --locked` — 16 + 6 passed | pass |
| Exact local workspace Clippy passes | Matching arm64 Clippy reached the workspace but reported six pre-existing M002/M003 findings | not met; outside M004 scope and assigned to M005 |
| Quick verification passes | `scripts/verify.sh quick` — passed | pass |
| Exact-head hosted CI reaches and passes Workspace tests | Run `33836217483`, job `100909174354`, head `7ef387aa` failed Workspace Clippy; Workspace tests did not run | not met; blocked by M005 scope |

## 3. Production implementation evidence

`HabitStore::observe` and `HabitStore::transition` now call one private
`open_lock_file` helper. The helper creates the synchronization inode when
absent, opens an existing inode without truncation, and returns the same
`File` used by the existing `flock_lock`/`flock_unlock` lifecycle.

No habit JSON serialization, namespace/path derivation, lock path, advisory
ownership boundary, save path, or file format changed. No proposal,
publication, asset-refresh, protocol, storage migration, or user-approval
behavior changed.

## 4. Verification executed

Local results:

- `cargo fmt --all -- --check` — passed.
- `git diff --check` — passed.
- `cargo test -p codegg-core habit --locked` — 5 passed.
- `cargo test -p codegg-core memory --locked` — 23 passed.
- `cargo test --test habit_skill_promotion --test skill_publication --locked` — default x86 linker failed because `/opt/local` exposes arm64 `liblzma`/`libiconv` to an x86 toolchain; this environment-only attempt was not treated as evidence.
- The same root command with explicit arm64 Rust binaries, `--target aarch64-apple-darwin`, and an isolated target directory — 16 promotion tests and 6 publication tests passed. The run emitted the existing `__eh_frame` linker warning only.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` with matching arm64 Cargo/rustc/clippy and isolated target — failed on six M002/M003 findings listed below; the two M004 `habit.rs` warnings were absent.
- `scripts/verify.sh quick` — passed, including formatting, generated-agent, core-boundary, sandbox, execution-ownership, and workspace all-target checking.

Hosted exact-head evidence:

- Historical predecessor: run `33813852632`, job `100841494152`, head
  `4ea4eaa000ecf65b0e70ed7278cf071a57cf2197`; failed Workspace Clippy on the
  two M001-owned `habit.rs` lock opens before Workspace tests.
- M004 candidate: run `33836217483`, job `100909174354`, head
  `7ef387aa0302efa3106b1d14ee166fd93e921cb9`; formatting and all static
  guards passed, then Workspace Clippy failed before Workspace tests.

## 5. Invariant review

The M004-owned invariants pass review: lock files remain synchronization-only;
existing lock inodes are not truncated before flock acquisition; mutation
still reads complete current state while holding the project lock; complete
bounded JSON replacement still uses the existing temp-file durability and
atomic rename path; no privacy, approval, publication, or protocol boundary
changed.

## 6. Failure, recovery, and contention review

The helper preserves the prior open, lock, operation, unlock, and close order.
Open and flock errors remain I/O errors. The existing closure path still
attempts unlock after load, mutation, or save failure. The existing eight-way
concurrent observer test passed, confirming serialized complete-file updates.

## 7. Migration and compatibility review

No migration is required. Habit JSON files and lock paths remain byte- and
path-compatible. No protocol, TUI command, skill root, asset refresh, or
configuration behavior changed.

## 8. Security review

No sensitive data, logging, permissions, authority, or path-construction logic
was added. The existing bounded structural habit-evidence and explicit
user-approval boundaries remain intact. The hosted failure contains only source
locations and lint diagnostics; no habit payload was included.

## 9. Documentation and operations

The implementation plan and corrective addendum now record M004 as implemented
and identify M005 as the separate publication/proposal Clippy follow-up. The
historical M001, M002, and M003 closure records were not rewritten. No
architecture update or new CI lane is required.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| low / operational | `src/skills/promotion.rs:417` — `too_many_arguments` | Pre-existing M002 finding; M005 |
| low / operational | `src/skills/promotion.rs:625` — ambiguous promotion lock open | Pre-existing M002 finding; M005 |
| low / operational | `src/skills/publish.rs:97` and `:138` — ambiguous publication lock opens | Pre-existing M003 findings; M005 |
| low / operational | `src/skills/publish.rs:210` — `too_many_arguments` | Pre-existing M003 finding; M005 |
| low / operational | `src/skills/publish.rs:422` — needless borrow | Pre-existing M003 finding; M005 |

No critical, high, or medium M004 production finding remains. The unresolved
findings block the repository's exact hosted Clippy gate but are outside the
bounded M004 ownership defined by its plan.

## 11. Roadmap disposition

M004 production implementation is complete, but the milestone is formally
`corrective pass required`, not strictly closed. M005 owns the six discovered
M002/M003 publication/proposal findings and the final exact-head hosted
closure. The memory-to-skill subsystem remains active until M005's closure
record is accepted.

## 12. Registry updates

- M004 implementation plan moved from `active` to `implemented`.
- The subsystem addendum records M004 as implemented and M005 as ready.
- `plans/registry.md` records the subsystem as active with M005 ready and M004
  corrective-pass-required; M005 is listed under dependency-ready plans.
- Blocked-work audit: no registered future plan listed M004 as a hard or
  interface dependency, so no existing future plan was unblocked or had its
  status changed.
- M005 was registered immediately as the separately owned corrective follow-up;
  it is ready because M004's production correction is landed and M005 does not
  depend on strict M004 closure.
