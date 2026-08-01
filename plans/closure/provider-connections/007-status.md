# Provider Connections Milestone 007 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/provider-connections/007-independent-closure-ratification-and-governance-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md`

Repository baseline reviewed: `04f4bb28c4d4236d066fc2d3f80ad41b4858738d`

Review-state commit: `ebd7c11b3117ca7e5fd976bd6658093691bea4ff`

Implementation and evidence lineage:

- `139c832c986106f31304d845860a66b17ba17099` — M006 executable correction;
  one provider test assertion now uses `crate::storage::STORAGE_LAYOUT_VERSION`.
- `f701925ccc3089d4bdc160367886a530ec1f1ffb` — M006 implementation-authored
  closure/evidence record; retained as historical provisional evidence.
- `8eddda26c417043c1ce0a9112df98beff2edeba1` — provider branch reconciliation.
- `7d8657e60aad85f677144b1bd0e7fb5d2929faa3` — merge to `main`.

## 1. Executive finding

The independent review confirms that M006 corrected a stale test assertion,
not a production migration defect. Provider storage is historically introduced
at v24, provider lifecycle additions continue through v33, and repository-wide
migrations v34 and v35 advance the shared terminal layout to
`STORAGE_LAYOUT_VERSION = 35`. The global dispatcher is idempotent after it
reaches v35, and no provider/storage executable semantics changed after the
M006 correction.

M007 is conditionally closed because the fresh hosted verify run for the
accepted review-state tree (`30681164263`, job `91318309629`) failed before
tests at Workspace Clippy. The failure is seven unrelated `dead_code` errors
in `crates/codegg-core/build.rs` model-profile parsing fields. M007 must not
modify that production surface. Strict M007 closure requires a later hosted
verify run on an accepted revision with that workspace gate green; Provider
Connections and DVR therefore remain non-strictly closed.

The review pass was performed by an independent Codex reviewer/pass distinct
from the agent/pass that authored M006 implementation commit `139c832c` and
M006 closure commit `f701925c`. Shared repository credentials do not change
the distinct review-pass attribution.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reviewer independence | Review-pass declaration and lineage inspection | pass | M007 reviewer did not author M006 implementation or provisional closure |
| Exact M006 executable diff | `git show 139c832c` | pass | One test assertion; no production migration/provider change |
| Merge integrity and drift | `git diff 139c832c..HEAD` plus provider/storage identity check | pass | Provider/storage executable tree is identical; later drift is unrelated agent/model-profile code |
| Canonical terminal contract | `storage/mod.rs`, `session/schema.rs` | pass | Constant is 35; dispatcher applies v1–v35; provider history remains v24/v33 |
| Migration idempotency/provider CRUD | Focused provider test | pass | Migration twice, CRUD, stale revision rejection, tombstone/purge all pass |
| `codegg-core` regression | `cargo test -p codegg-core --locked -- --test-threads=1` | pass | 481 passed |
| Formatting and diff hygiene | `cargo fmt --all -- --check`; `git diff --check` | pass | Zero errors |
| Quick verification | `scripts/verify.sh quick` | pass | Exit 0 on `04f4bb28` |
| Historical hosted M006 evidence | Run `30603541350`, job `91071065732` | pass | Exact M006 executable SHA `139c832c`; all required steps succeeded |
| Current accepted hosted verify | Run `30681164263`, job `91318309629` | partial | Checkout and guards passed; Workspace Clippy failed before tests on unrelated `build.rs` dead-code errors |
| Planning ownership | This record, registry, and roadmap reconciliation | pass | M006 remains historical; M007 owns the independent disposition |

## 3. Production implementation evidence

M007 introduced no production executable changes. The M006 executable
correction remains the canonical test-only assertion change. The current
provider/storage tree is executable-identical to the correction revision;
later accepted-head executable drift is outside Provider Connections and was
not altered by this review.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-core provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe -- --test-threads=1
cargo test -p codegg-core --locked -- --test-threads=1
cargo fmt --all -- --check
git diff --check
scripts/verify.sh quick
```

### Results

- Focused provider test: pass, 1 passed.
- `codegg-core`: pass, 481 passed.
- Formatting and diff checks: pass.
- Quick verification: pass.
- Hosted M006 run `30603541350` / job `91071065732`: pass on exact
  `139c832c`.
- Hosted accepted-review run `30681164263` / job `91318309629`: conditional
  evidence only; Clippy failed with seven unrelated `dead_code` errors in
  `crates/codegg-core/build.rs`, and workspace tests were not run.

## 5. Invariant review

- `STORAGE_LAYOUT_VERSION` remains the canonical repository-wide terminal
  contract at 35.
- Provider migration history remains historically accurate; no migration was
  added, removed, renumbered, reordered, or rewritten.
- Provider metadata remains credential-free and secret references remain opaque.
- Endpoint, TLS, scope, lifecycle, health, selection, rotation, and revision
  semantics were not changed.
- Stale revisions remain rejected by the focused regression test.
- No test was ignored, filtered, deleted, weakened, or converted to an expected
  failure.

## 6. Failure and recovery review

The second global migration invocation is guarded by the recorded terminal
version and is a no-op at v35. The focused test exercises provider CRUD and
stale-revision behavior after that second invocation. No retry, cancellation,
restart, contention, or recovery behavior was changed by M006/M007.

## 7. Migration and compatibility review

The global dispatcher reaches v35 from supported lower versions, with provider
storage retained at its historical migration points and later repository-wide
migrations applied in order. Existing provider rows and compatibility paths
remain covered by the passing codegg-core suite. The unresolved hosted issue is
workspace lint evidence, not migration or compatibility behavior.

## 8. Security review

No credential, endpoint, secret-reference, scope, lifecycle, authorization,
logging, or path-handling code changed. No new secret exposure or privilege
boundary was introduced.

## 9. Documentation and operations

The M007 plan, provider roadmap/addenda, registry, and this closure record now
identify M006 as historical implementation-authored evidence and M007 as the
independent conditional authority. The fresh hosted failure is preserved with
its exact run, job, command, and owning unrelated file. No CI topology,
resource, or release behavior was changed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Hosted `verify` run `30681164263` fails Workspace Clippy on seven unrelated `dead_code` diagnostics in `crates/codegg-core/build.rs` | Prevents strict current-head verification and keeps Provider/DVR closure non-strict | Resolve in the owning agent/model-profile work, then run hosted `verify` on an accepted descendant; do not modify it in M007 |

No Provider Connections migration, provider, security, or compatibility finding
remains at high or medium severity.

## 11. Roadmap disposition

M007 is conditionally closed. Provider M006 is accepted as implemented and its
self-authored closure record remains historical evidence. Strict Provider
Connections closure requires the named hosted workspace-gate evidence. Tool
Programs M019 remains independently ready. DVR M006 remains blocked until both
Provider M007 and Tool Programs M019 have strict closure records.

## 12. Registry updates

- Provider Connections remains `closing` with M007 conditionally closed.
- M007 is removed from dependency-ready handoffs.
- Tool Programs M019 remains dependency-ready and unchanged.
- DVR M006 remains blocked on strict Provider M007 and Tool Programs M019.
- `plans/closure/provider-connections/006-status.md` remains available as
  historical implementation-authored evidence.
