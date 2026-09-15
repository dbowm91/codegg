# Dependency Security and Workspace Consolidation M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/dependency-security-workspace-consolidation/002-workspace-dependency-ownership-normalization.md`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md#m002--workspace-dependency-ownership-normalization`

Repository baseline reviewed: `05e7b258` (plan snapshot baseline `b3459640`)

Implementation commits or pull requests:

- `05e7b258` — M002: normalize workspace dependency ownership

## 1. Executive finding

M002 is complete. Shared versions and default-feature policy now have one
workspace authority (`[workspace.package]` / `[workspace.dependencies]` in
the root manifest); member manifests use `.workspace = true` plus only
their local feature additions; single-consumer dependencies stayed local;
and `unsafe_code = deny` is inherited only by `codegg-core` via
`[workspace.lints]`. No behavior change, no dependency-major upgrade, no
global feature superset. The M002 commit itself is lockfile-stable; the
later M003 image-feature narrowing (separate milestone, separate lock
delta) is isolated to the optional image graph and leaves the default
resolution unchanged. Broad verification is green; no new advisory
finding is introduced by M002.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Repeated versions/default policies have one workspace authority where semantically correct | Root `[workspace.package]` (version/edition/rust-version/license/repository/homepage) + `[workspace.dependencies]` (tokio, serde, serde_json, thiserror, anyhow, tracing, chrono, sqlx, dashmap, dirs, regex, url, uuid, async-trait, futures-util/executor, tokio-util, tempfile, toml, sha2, base64, rand, aes-gcm, argon2, hmac, hex, similar, libc, once_cell, subtle, flate2, tar, walkdir, eggfetch-core, http, tokio-stream, parking_lot, proptest, internal `codegg-*`/`egg*` pairs); members converted to `.workspace = true` | pass | `git show 05e7b258 --stat`: root + 8 member manifests |
| Member manifests retain only actual feature additions and package-specific deps | Members use `dep = { workspace = true, features = [...] }`; e.g. `sqlx` baseline carries no union features (root/core add `runtime-tokio,sqlite,derive,chrono,json`, providers adds `runtime-tokio,sqlite`); `uuid` baseline is `v4` with serde local; `url` baseline has no serde, egglsp adds it; optionality preserved (`optional = true` stays in members) | pass | Source census per WP1 classes; single-consumer deps (clap, ratatui, crossterm, comrak, syntect, image stack, server/plugin optionals, rustpython-parser, notify) left local |
| No dependency feature family widens unintentionally | Baseline tables keep minimal features; `cargo tree -e features` for sensitive families shows no new enablement vs pre-M002 resolutions; `cargo tree -d` remaining duplicates are third-party owned (base64 0.22/0.23 via eggsact, md5 0.7/0.8, strum 0.26/0.28 via Ratatui) with evidence, not CodeGG-owned widening | pass | Workspace inheritance is additive by design; the baseline was kept minimal rather than a union — see §5 |
| Internal path/version declarations centralized where release semantics shared | All internal crates (`codegg-config/protocol/providers/core/git`, `egggit`, `egglsp`, `eggsentry`, `eggcontext`) centralized as `version = "=0.1.0", path = ...` | pass | Direction unchanged; `check-core-boundary.sh` green |
| Package metadata truthful; no cosmetic ownership change | `authors`/`description` left local; only genuinely shared `version/edition/rust-version/license/repository/homepage` centralized | pass | Per plan §3/WP1 |
| Existing crate-boundary guards green | `scripts/check-core-boundary.sh` pass (via `verify.sh quick`); internal dependency direction unchanged | pass | §4 |
| Lockfile unchanged or every change explained | M002 commit `05e7b258` contains no `Cargo.lock` change (stat shows 15 files, none is the lockfile) | pass | Later M003 lock pruning (-44 image-only packages) is a separate milestone delta, recorded in its own closure, not M002 churn |
| Broad local verification passes | §4 command set green | pass | One overlay note applies (M003 image delta landed after M002; default resolution proven unchanged — see §4) |

## 3. Production implementation evidence

Landed ownership changes (commit `05e7b258`, 15 files):

- `Cargo.toml`: added `[workspace.package]` + `[workspace.dependencies]`
  (minimal-feature baselines) + `[workspace.lints.rust] unsafe_code =
  "deny"` (narrowly scoped, see below); root package + deps converted to
  `*.workspace = true` with local feature additions only.
- `crates/codegg-config`, `codegg-core`, `codegg-git`,
  `codegg-protocol`, `codegg-providers`, `eggcontext`, `egggit`,
  `egglsp`, `eggsentry` manifests: converted to workspace inheritance
  per dependency family; package-specific features/optionality retained
  locally.
- `crates/codegg-core/src/lib.rs`: removed `#![deny(unsafe_code)]`
  source attribute because the crate now inherits the identical rule
  from `[workspace.lints]` (`[lints] workspace = true` in the core
  manifest). Enforcement is preserved via manifest lints, not weakened:
  the rule text is byte-identical (`unsafe_code = "deny"`), the root
  package deliberately stays outside package-wide inheritance because
  `src/bin/codegg-sandbox-helper.rs` contains reviewed `unsafe`, and
  the root library keeps its own `#![deny(unsafe_code)]` in
  `src/lib.rs`. Other crates keep explicit local allows on reviewed
  test helpers. `cargo check` + `clippy -D warnings` + core-boundary
  guard all green.
- `AGENTS.md`, `architecture/native_crates.md`,
  `docs/dependency-maintenance.md`: workspace-ownership policy
  documented (authoritative owner, minimal-baseline rule,
  single-consumer-local rule, lint-scope rule).
- `plans/implementation/.../002-...md`: `ready` → `implemented`.
- `Cargo.lock`: unchanged in the M002 commit (no lock churn from
  replacing repeated declarations with inheritance — the ideal
  manifest-only diff).

Deliberately absent (out of scope, not attempted): dependency-major
upgrades, root feature redesign, crate-boundary flattening, code moves
for manifest length, single-consumer hoisting, lint-policy redesign,
build scripts/manifest generators/bots/scanners, image-graph slimming
(M003), crate publication (M004), updater work (M005).

## 4. Verification executed

### Commands run (on the M002+M003 tree; M003 delta isolated per below)

```bash
cargo metadata --locked --format-version 1
cargo tree -d --locked
cargo tree -e features --locked
cargo check --workspace --all-targets --locked
cargo check --workspace --all-targets --features image --locked
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
cargo test --features image --lib -- --test-threads=1
cargo test --test tui_render --locked -- --test-threads=1
cargo test --test tui --locked -- --test-threads=1
scripts/check-core-boundary.sh
scripts/verify.sh quick
cargo fmt --all -- --check
cargo audit
```

### Results

- `cargo metadata --locked`: ok.
- `cargo tree -d --locked`: remaining duplicates third-party owned
  only (base64, md5, strum families noted above); no CodeGG-owned
  duplicate major introduced by centralization.
- `cargo check --workspace --all-targets --locked` (via `verify.sh
  quick`): pass.
- `cargo check --workspace --all-targets --features image --locked`:
  pass (53.67s).
- `cargo check --workspace --all-targets --all-features --locked`:
  pass (56.01s).
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings`: pass.
- `CARGO_BUILD_JOBS=1 cargo test --workspace --locked
  -- --test-threads=1`: green — every `test result:` line `ok`, zero
  non-ok across the full sweep.
- `cargo test --features image --lib`: 4507 passed, 0 failed
  (includes the 9 new M003 image-format tests; the M002-relevant
  remainder is unchanged behavior).
- `tui_render` 99 passed; `tui` 165 passed.
- `scripts/verify.sh quick`: passed (fmt, agent schema,
  core-boundary, sandbox, execution-ownership, tui-authority guards,
  workspace check).
- `cargo fmt --all -- --check`: pass.
- `cargo audit`: 1 vulnerability (rustls RUSTSEC-2026-0285, medium,
  pre-existing per M001 §10, unrelated family, out-of-scope broad
  update exclusion) + allowed unmaintained/yanked warnings; 0 `lru`
  findings; no new finding introduced by workspace inheritance.

### M003-overlay isolation note

M003's optional-image narrowing landed in the working tree after
`05e7b258` and before this closure was written. Its delta is proven
isolated from M002's claims: default `cargo tree --offline` is 1219
lines (identical count to the pre-M003 default); the default `codegg`
package tree contains zero image references; `ravif` no longer matches
any locked package; the lock delta (-44 packages) enumerates only
image-default-format families (avif/ravif stack, exr, tiff, qoi and
their exclusive transitive deps — full list in the M003 closure).
No M002-owned family (tokio, sqlx, eggfetch, dashmap, ratatui default)
changed versions or widened features due to M003. M002's
lockfile-stable conclusion therefore stands at its commit; M003's
intentional pruning is owned by M003.

## 5. Invariant review

- Resolved production features did not widen from inheritance: the
  workspace baseline owns versions/default policy with minimal
  features; union-sized baselines were explicitly avoided (sqlx and
  eggfetch-core baselines carry no union features; tokio/futures
  baselines carry no union; uuid baseline is `v4`; serde stays local).
- Package-specific features/optionality preserved: verified by
  manifest diff (features only added in members; `optional = true`
  never moved into the baseline).
- Internal dependency direction unchanged: core-boundary guard green.
- MSRV, public APIs, package names, version constraints unchanged: no
  version bump in the M002 commit; `rust-version = "1.89"` centralized
  truthfully (all members already shared it).
- `unsafe_code` enforcement preserved and scoped: only `codegg-core`
  inherits the deny (the one crate that already enforced it with no
  deliberate unsafe); root and test-helper exceptions documented and
  minimal.

## 6. Failure and recovery review

- No runtime, storage, protocol, scheduler, daemon, or authorization
  path touched: manifest + docs + one lint-attribute move only. No
  failure-mode change; no migration; rollback is `git revert` of the
  manifest commit.
- The `#![deny]` → `[lints]` move cannot silently permit unsafe: any
  future `unsafe` block in `codegg-core` fails `cargo check`/`clippy`
  identically (deny is deny regardless of spelling); the guard suite
  and both all-targets checks prove the current tree has no such
  block outside the documented exceptions.

## 7. Migration and compatibility review

- No schema, config, protocol, or data migration. No MSRV change. No
  public API change. Downstream consumers of member crates see
  identical versions/features; the only observable manifest fact is
  single ownership of shared policy.

## 8. Security review

- No advisory silenced; no audit-ignore touched (M001's RSA
  reconciliation stands). No network, TLS, SSRF, sandbox, Landlock,
  secret, archive, or plugin behavior changed. The fresh `cargo audit`
  shows no new medium-or-higher finding attributable to M002.

## 9. Documentation and operations

- Updated: `AGENTS.md` (workspace-ownership paragraph),
  `architecture/native_crates.md` (Workspace Dependency Ownership
  section), `docs/dependency-maintenance.md` (Workspace ownership
  (M002) section), implementation plan status, this closure record.
  No new CI lane, scanner, bot, size gate, or release automation
  added — per the roadmap verification policy, `cargo tree -d`,
  feature trees, and audit output remain temporary closure evidence.
- Checked and left unchanged: historical M001/HTTP/footprint closure
  records (no rewriting of predecessor evidence).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | RUSTSEC-2026-0285 (`rustls 0.23.41`, pre-existing, unrelated family) | Bounded TLS-robustness issue; transcript still authenticated; not exploitable for handshake alteration by a network-position attacker (per advisory). | Separate bounded maintenance: targeted `rustls` upgrade with provider/streaming regression evidence. Not M002 scope (broad-update exclusion). |
| info | Remaining `cargo tree -d` duplicates are third-party owned (base64, md5, strum majors) | No CodeGG-owned duplication; network/TLS/storage behavior unchanged. | Revisit only with new measured or security evidence; no action in this workstream. |

No critical or high findings. No unexplained memory-safety advisory in
the supported graph.

## 11. Roadmap disposition

Milestone M002 closed. The M003/M004 predecessor gate (M002 accepted
closure) is satisfied: both may proceed independently — M003 owns
optional image-feature slimming; M004 owns reusable-package
qualification without automatic publication. M005 remains blocked on
the external generalized updater interface (its M002 hard dependency
is now satisfied; the interface dependency is not).

## 12. Registry updates

- `plans/registry.md`: M002 implementation plan `ready` → `closed`;
  subsystem current milestone `M001 closed, M002 ready` → `M002
  closed, M003 ready`; M003 and M004 registered as dependency-ready
  (`ready`, predecessor M002 closed); Blocked-work M002 row cleared;
  M005 blocker narrowed to the external interface only; this closure
  added under Recently closed work with commit `05e7b258`.
- Subsystem roadmap: M002 status updated to closed; M003/M004 unblocked
  notes added (no baseline history rewritten).
- Implementation plans: M002 `implemented` (landed; closure record is
  the gate); M003/M004 `blocked` → `ready`.
