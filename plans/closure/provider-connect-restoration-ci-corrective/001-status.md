# Provider /connect Restoration CI Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/provider-connect-restoration-ci-corrective/001-managed-key-concurrency-ci-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/provider-connect-restoration-ci-corrective-addendum.md`

Repository baseline and closure revision: `c9c37cca`

Implementation commit:

- `c9c37cca` — fix: publish managed keys atomically

Hosted evidence:

- [CI / verify run 35483642396](https://github.com/dbowm91/codegg/actions/runs/35483642396)

## 1. Executive finding

The hosted-only `CorruptKeyFile` was a production publication race in managed
key initialization. The first writer created the visible `master.key` path
before its complete payload had been written. A concurrent loser could
therefore read an empty or partial file and fail closed instead of converging
on the winner's key.

The fix writes the generated key to a private same-directory sibling, applies
the existing private-file policy, flushes the complete payload with `sync_all`,
and publishes it with non-overwriting `hard_link`. Readers can see only a
complete published file. An `AlreadyExists` publication loser reads and
validates the winner; corrupt, partial, symlinked, non-regular, or unsafe files
still fail closed. Key material is not logged or included in errors.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Managed-key concurrent first write converges | Focused `codegg-config` race test; 12 parallel process runs | pass |
| Credential-store first write remains convergent | Focused `codegg-providers` concurrent first-put test | pass |
| Provider-auth regression coverage | `cargo test -p codegg-providers --lib --locked -- --test-threads=1` | pass — 159 tests |
| Config encryption regression coverage | `cargo test -p codegg-config --lib --locked -- --test-threads=1` | pass — 83 tests |
| Retry test is deterministic | Focused ambiguous raw-shell test; 20 parallel process runs | pass |
| Migration fixture corrections remain green | Continuation checkpoint: 11; WorkPlan foundation: 10 | pass |
| Canonical quick verification | `scripts/verify.sh quick` | pass |
| Workspace Clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| Full local workspace suite | `cargo test --workspace --locked -- --test-threads=1` | pass — 11,540 passed, 3 ignored |
| Hosted canonical CI | [run 35483642396](https://github.com/dbowm91/codegg/actions/runs/35483642396) | pass — guards, formatting, Clippy, workspace tests, and cleanup all green |

## 3. Safety and compatibility review

- The read-side validation and fail-closed behavior are preserved.
- Publication is same-directory and non-overwriting, so concurrent writers
  converge on exactly one complete key without replacing an existing winner.
- Existing explicit environment-key precedence and credential-store behavior
  are unchanged.
- No schema, protocol, migration, or provider catalog changes were made.
- No secret or key payload appears in logs, diagnostics, or closure evidence.

## 4. Unresolved findings

None in this corrective. The hosted `CorruptKeyFile` race is corrected and the
workspace suite completed successfully on the closure revision.

## 5. Registry unblock audit

This corrective was a verification and safety repair for the already-closed
provider-connect M001 capability; it does not create a new provider milestone.
No registered future plan was blocked solely on this corrective. Identity /
audit M001 remains ready, while its M002 and M003 live-hook work remains
blocked on M001 and M004 remains blocked on those hooks. No other roadmap
status changes are required.

## 6. Closure disposition

C001 is formally closed. The historical provider-connect M001 closure remains
unchanged; this record closes the later hosted-CI corrective that hardened its
managed-key first-write contract.
