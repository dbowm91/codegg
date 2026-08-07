# Runtime Safety, Resource Control, and Footprint Milestone 005 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/005-dependency-feature-and-namespace-normalization.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `4d540ce315c9ef2a1c07544cd42df0efc43708e1`

Implementation commits:

- `b11bd25` — normalize dependency features and memory namespaces
- `31007071234` — hosted CI run; all verification steps passed, but the
  post-cache step failed twice with runner disk exhaustion

## 1. Executive finding

M005 is production-complete and conditionally closed. Workspace dependency
ownership is explicit, avoidable umbrella dependencies are removed, text-only
clipboard support remains available, and project memory writes now use a full
domain-separated SHA-256 namespace. Legacy MD5 namespaces are migrated
idempotently when the local store is available and remain readable through the
remote TUI compatibility fallback.

Strict hosted closure is conditional only because both executions of the
repository's hosted `verify` job failed after all checks and tests passed, when
the `rust-cache` post-step exhausted the runner filesystem while writing a
diagnostic/cache log. This is external operational evidence, not a production
or test failure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit reqwest/TLS ownership | Root, core, providers, and egglsp manifests; post-change feature tree | pass | Core no longer reactivates reqwest defaults; CodeGG reqwest consumers use rustls explicitly where needed. |
| No unintended native TLS in default graph | `cargo tree -e features -i native-tls` reports no package; reqwest tree has no `default-tls`/`default` path | pass | Native TLS is not CodeGG-owned in the default graph. |
| Exact SQLx features | SQLx defaults disabled in root/core/providers; macros/migrate/chrono/json retained only where source requires them | pass | SQLite/runtime features compile across affected crates. |
| Text-only clipboard | arboard `default-features = false`; no-default TUI import fixed through clipboard facade | pass | CodeGG's separate image feature remains independent. |
| Futures narrowing | `futures-util` plus `futures-executor` replace umbrella imports; sink feature retained where required | pass | All affected workspace targets compile. |
| Grep narrowing | `grep-regex` and `grep-searcher` are used directly; umbrella `grep` and unused `grep-matcher` removed | pass | M004 search behavior is unchanged. |
| SHA-256 namespace writes | `project_namespace()` uses `codegg-memory-namespace-v1\0` plus the full SHA-256 digest | pass | No new namespace uses MD5 or a truncated digest. |
| Legacy memory compatibility | `migrate_project_namespace()` rewrites and saves current records before removing the old directory; remote list fallback reads legacy namespaces | pass | Unit test proves migration is idempotent. |
| No user-visible feature removal | Default, no-default, server, plugins, image, provider, memory, and tool checks | pass | Clipboard, TLS, SQLite, optional features, and search remain available. |

## 3. Production implementation evidence

- Workspace manifests now explicitly own reqwest, SQLx, arboard, futures, and
  grep features. `Cargo.lock` removes the obsolete umbrella packages and
  retains only the legacy MD5 compatibility dependency in `codegg-core`.
- All `futures::` imports were narrowed to `futures_util::`; executor call
  sites use `futures_executor::`.
- `src/tool/grep.rs` imports `RegexMatcher` from `grep-regex` directly.
- `codegg_core::memory` owns current and legacy project namespace derivation,
  migration, and tests. TUI callers pass stable project identity rather than
  pre-hashed text.
- Local TUI memory access migrates legacy records before use. Remote summary
  requests query the current namespace and fall back to the legacy namespace
  for already-running daemons and the compatibility window.
- `architecture/memory.md` and `architecture/util.md` document the namespace
  migration and text-only clipboard feature ownership.

## 4. Verification executed

Local:

```text
cargo fmt --all -- --check                                      pass
cargo check --workspace --all-targets --locked                  pass
cargo check --workspace --all-targets --no-default-features     pass
cargo check --workspace --all-targets --features server         pass
cargo check --workspace --all-targets --features plugins        pass
cargo check --workspace --all-targets --features image           pass
cargo test -p codegg-core memory --lib -- --test-threads=1       17 passed
cargo test --test memory -- --test-threads=1                      6 passed
cargo test --test tool_execution -- --test-threads=1            54 passed
cargo test -p codegg-providers --lib -- --test-threads=1         99 passed
scripts/check-core-boundary.sh                                   pass
scripts/check_execution_ownership.py                             pass
scripts/verify.sh quick                                           pass
```

Hosted:

- PR #72 run `31007071234` was rerun after the first cache-disk failure.
- On the rerun, formatting, workspace check, Clippy, and workspace tests all
  passed. The job was marked failed only by
  `rust-cache`'s post-step: `No space left on device` while writing a runner
  diagnostic page.
- No hosted test or compile failure was reported. A SQLite-lock message was
  emitted by an existing contention test, whose test result was still `ok`.

## 5. Invariant review

- HTTPS consumers retain explicit rustls roots and no default/native TLS path
  is introduced by `codegg-core`.
- SQLx macros, migrations, SQLite, chrono, and JSON integrations remain
  enabled where source imports prove they are required.
- Clipboard text APIs remain available under the default `arboard` feature;
  arboard image-data is not enabled accidentally.
- Async stream/sink/executor behavior is preserved by the corresponding
  narrow futures crates.
- Grep matcher/searcher behavior and M004's worker implementation are not
  changed by this dependency-only import adjustment.
- The namespace uses a domain separator and full 256-bit digest, while old
  durable files remain readable and migration does not repeat or merge the
  same record twice.

## 6. Failure and recovery review

Namespace migration saves the rewritten current namespace before deleting the
legacy directory. If saving fails, the legacy directory is retained and the
error is returned. Repeated migration calls are no-ops after the namespace is
rewritten. Remote TUI fallback handles a daemon that loaded legacy records
before another process completed migration. No scheduler, protocol, process,
or concurrency authority changed.

## 7. Migration and compatibility review

There is no database schema or protocol migration. Existing memory files under
`project/<md5>` are accepted for the compatibility window and are rewritten to
`project/<domain-separated-sha256>` by local stores. The direct MD5 dependency
is intentionally scoped to this legacy-read/migration helper; new writes do
not use it. Existing HTTPS, SQLite, clipboard, grep, and optional feature
contracts remain compatible.

## 8. Security review

No credentials, endpoints, trust roots, or authorization boundaries changed.
The namespace digest is not a security identity and is not exposed as a new
protocol contract. Namespace paths continue through the existing safe path
validation. The migration removes only the exact validated legacy project
directory after a successful save. No critical, high, or medium finding
remains.

## 9. Documentation and operations

- Updated `architecture/memory.md` with the new namespace and migration
  contract.
- Updated `architecture/util.md` with arboard feature ownership.
- Feature-tree evidence was inspected locally and is summarized here rather
  than committed as a generated report.
- Hosted verification remains operationally reproducible once a runner with
  sufficient free disk is available; the implementation does not require a
  new CI lane or cache configuration.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low / operational | Hosted `verify` post-cache step cannot write its diagnostic/cache log because the GitHub runner is out of disk | Strict hosted-pass evidence is unavailable even though all verification steps passed | Re-run PR #72 on a runner with sufficient disk, or accept the existing all-steps-passed log as hosted evidence |

## 11. Roadmap disposition

M005 is conditionally closed: production implementation and all available
local/hosted correctness evidence are complete, with only the named hosted
runner disk condition outstanding. M006 is dependency-ready because M005's
manifest ownership and interfaces are stable and M006's dependency is soft.
M007 remains blocked: it requires strict M002, M003, M005, and M006 dispositions
and uses M004 as a soft measurement input. M003 and M008 retain their existing
blockers.

## 12. Registry updates

- M005 moved from closing review to recently closed as `conditionally closed`.
- M006 moved from blocked to dependency-ready `ready` in the same status
  change; its implementation plan status was updated accordingly.
- M003 remains blocked on strict M001/M002 evidence.
- M007 remains blocked on M002, M003, strict M005, and M006.
- M008 remains blocked on M001–M007 accepted dispositions.
- No corrective implementation plan is required: the only unresolved item is
  an external runner-disk condition, and no production correctness finding
  remains.
