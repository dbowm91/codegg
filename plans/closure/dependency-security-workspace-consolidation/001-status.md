# Dependency Security and Workspace Consolidation M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/dependency-security-workspace-consolidation/001-security-and-duplicate-graph-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md#m001--security-and-duplicate-dependency-graph-convergence`

Repository baseline reviewed: `cd122ac4` (plan snapshot baseline `b3459640`)

Implementation commits or pull requests:

- `3bd54ccd` — M001: converge Ratatui/LRU, DashMap, SQLx feature surface

## 1. Executive finding

M001 is complete. The three bounded changes landed with manifest-only edits
plus one focused compatibility fix: Ratatui moved off the stale `lru` train
onto a fixed supported line, CodeGG-owned DashMap converged to the
Eggfetch-compatible major, and SQLx feature ownership narrowed to the
source-demonstrated surface with the audit exception reconciled to current
evidence. No protocol, storage-schema, provider-behavior, or authorization
change was made. Broad local verification is green; the remaining advisory
finding is an out-of-scope medium in an unrelated family, documented below.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| LRU unsoundness off CodeGG's direct Ratatui path | `cargo tree -i lru@0.12.5` errors (no match); lock holds only `lru 0.18.2`; `cargo audit` lists no `lru` finding | pass | Fixed floor `>= 0.18.2` per RUSTSEC-2026-0253 confirmed in local advisory DB |
| Ratatui convergence preserves default + image TUI behavior | `cargo check -p codegg --features image`; `cargo test --test tui_render` (99 pass); `cargo test --test tui` (165 pass) | pass | No layout/event/lifecycle change |
| CodeGG-owned DashMap on one major; old major absent | `cargo tree -i dashmap@5.5.3` errors (no match); lock holds only `dashmap 6.2.1`; reverse tree shows codegg, core, providers, eggfetch-core | pass | No third-party owner of old major |
| SQLx feature ownership source-demonstrated, minimal | Census: only `sqlx::FromRow` derive used; no `query!`/`migrate!`/`Migrator`/`MigrateDatabase`; manifests now `runtime-tokio,sqlite,derive,chrono,json` (providers `runtime-tokio,sqlite` unchanged); workspace check + all-targets check green | pass | `macros`/`migrate` removed from root + core |
| `sqlx-mysql`/`sqlx-postgres`/`rsa` disposition | `cargo tree -i sqlx-mysql/rsa` empty in default and `--all-features` resolutions; lock union still lists them (feature-union, upstream SQLx wiring); documented, not hand-patched | pass | Upstream closure per plan WP4 |
| Audit ignore reconciliation | `.cargo/audit.toml` retains only RUSTSEC-2023-0071 with rewritten owner-path/feature evidence; no LRU ignores added | pass | `patched = []` confirmed; no fixed upgrade exists |
| No critical/high or unexplained memory-safety advisory | `cargo audit`: 1 vulnerability (rustls medium, out-of-scope, explained); 11 allowed warnings (unmaintained/yanked); 0 `lru` findings | pass | See §10 for rustls disposition |
| Broad local verification green | fmt, workspace check (default + all-targets + all-features), clippy all-features `-D warnings`, workspace tests, feature tests, `scripts/verify.sh quick` | pass | One pre-existing flake observed once, green on re-runs (§4) |
| Release-size observation (descriptive) | `cargo build --release --locked` 65M binary; `cargo bloat` top crates recorded | pass | Descriptive only; no gate added |

## 3. Production implementation evidence

Landed ownership changes (commit `3bd54ccd`):

- `Cargo.toml`: `ratatui 0.29` → `0.30` (`default-features = false`,
  `features = ["crossterm", "underline-color"]`, preserving the 0.29-default
  backend/underline surface without `all-widgets`/`layout-cache`/`macros`
  expansion); `crossterm 0.28` → `0.29` (same `event-stream` feature,
  converging the image-feature-required line into one crossterm major);
  `dashmap 5` → `6`; root `sqlx` features `macros,migrate` → `derive`.
- `crates/codegg-core/Cargo.toml`: `dashmap 5` → `6`; `sqlx`
  `macros,migrate` → `derive`.
- `crates/codegg-providers/Cargo.toml`: `dashmap 5` → `6` (sqlx already
  minimal, unchanged).
- `src/tui/components/dialogs/connect.rs`: `self.list_state.clone()` →
  `self.list_state` (`ListState` is `Copy` in Ratatui 0.30; clippy
  `clone_on_copy` fix, behavior-preserving).
- `.cargo/audit.toml`: RUSTSEC-2023-0071 comment rewritten with the exact
  lock-union owner path (`sqlx 0.8.6` → `sqlx-mysql 0.8.6` → `rsa 0.9.10`),
  enabled-feature evidence (`cargo tree -i` empty), lock-union rationale,
  and `patched = []` status. Ignore list unchanged in membership.
- `docs/dependency-maintenance.md`: sqlx checkpoint updated to
  `derive`-only, no `migrate`, with the lock-union/`cargo tree -i` note.
- `Cargo.lock`: attributable churn only — removed `ratatui 0.29`, `lru
  0.12.5`, `crossterm 0.28.1`, `dashmap 5.5.3` (+ their exclusive
  transitive deps); added `ratatui 0.30.2` family, `lru 0.18.2`,
  `crossterm 0.29.0`, `dashmap 6.2.1` (+ union-listed optional backend
  deps not enabled in supported resolutions).

Before/after reverse trees:

- `lru@0.12.5`: before `lru → ratatui 0.29 → codegg`; after: no match
  (removed from lock).
- `lru@0.18.x`: before `lru 0.18.0 → ratatui-core 0.1.2 → ratatui 0.30.2 →
  ratatui-image → codegg` (vulnerable); after: same path at `lru 0.18.2`
  (fixed), with root `ratatui 0.30.2 → codegg` on the same line.
- `dashmap@5.5.3`: before owned by codegg, codegg-core, codegg-providers;
  after: no match. `dashmap@6.2.1`: after owned by codegg, codegg-core,
  codegg-providers, and eggfetch-core (single major).
- `sqlx-mysql`/`rsa`: before and after `cargo tree -i` empty in every
  supported resolution; lock union lists `sqlx-mysql 0.8.6` / `rsa 0.9.10`
  in both states (unchanged upstream wiring).

Deliberately absent (out of scope, not attempted): TUI redesign, image
graph slimming (M003), workspace inheritance (M002), DashMap replacement,
SQLx replacement/migration-framework change, broad `cargo update`,
server/plugin/image semantics, new CI/advisory/size gates.

## 4. Verification executed

### Commands run

```bash
cargo tree -d --locked
cargo tree -i lru@0.12.5 --locked
cargo tree -i lru@0.18.0 --locked --all-features
cargo tree -i dashmap@5.5.3 --locked
cargo tree -i dashmap@6.2.1 --locked
cargo tree -i sqlx-mysql --locked
cargo tree -i rsa --locked
cargo tree -e features -p sqlx --locked
cargo audit
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --test tui_render --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --test tui --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test -p codegg --features server,plugins,lsp-test-support --locked -- --test-threads=1
scripts/verify.sh quick
cargo build --release --locked
cargo bloat --release --bin codegg --crates --locked -n 40
cargo tree -d --locked
```

### Results

- `cargo fmt --all -- --check`: pass.
- `cargo check --workspace --all-targets --locked`: pass (also green for
  `-p codegg --features image`).
- `cargo check --workspace --all-targets --all-features --locked`: pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings`: pass after the one `ListState` compatibility fix (initial
  failure `clone_on_copy` at `src/tui/components/dialogs/connect.rs:127`,
  fixed, re-green).
- `cargo test --workspace --locked`: green — 214 `test result: ok` lines,
  zero non-ok, on the final sweep.
- Focused: `tui_render` 99 passed; `tui` 165 passed.
- Feature sweep `cargo test -p codegg --features
  server,plugins,lsp-test-support`: green, zero non-ok.
- `scripts/verify.sh quick`: passed (fmt, agent schema, core-boundary,
  sandbox, execution-ownership, tui-authority guards, workspace check).
- `cargo audit`: 1 vulnerability (rustls RUSTSEC-2026-0285, §10), 11
  allowed warnings (unmaintained + yanked `spin`), 0 `lru` findings
  (previously 3 unsound warnings across `lru 0.12.5`/`0.18.0`).
- `cargo build --release --locked`: 17m13s, `target/release/codegg` 65M
  (78.3M file, 45.0M `.text` per bloat). Top crates: codegg 10.0MiB, std
  5.3MiB, serde_core 3.9MiB, serde 3.0MiB, eggsact 2.8MiB, codegg_core
  2.5MiB. No before-measurement was taken at the pre-change baseline;
  recorded here as descriptive evidence only.
- Flake note: `provider_core::tests::shared_client_follows_redirects_and_enforces_ten_hop_bound`
  failed once mid-sweep (`WouldBlock`/`ConnectionReset` in its raw
  nonblocking-loopback fixture), then passed twice in isolation and passed
  in both full workspace sweeps. A pristine-baseline worktree passed the
  same test, confirming the fixture race predates M001; M001 touches no
  HTTP/hyper/socket path. Not a regression; no production change made for
  it. See §10.

## 5. Invariant review

- TUI rendering/input/focus/popup/terminal-restore/remote/local behavior:
  preserved. Manifest-only migration plus a Copy fix; `tui` + `tui_render`
  suites green; image-feature check green. No layout, event-semantics, or
  styling change.
- No protocol, storage-schema, migration-order, provider-behavior, or
  authorization change: confirmed by diff (manifests, one Clone fix, audit
  comment, docs, planning registry). No migration added.
- SQLite remains the only enabled SQLx driver: `sqlite` retained;
  `mysql`/`postgres` never enabled; reverse trees empty.
- `sqlx::FromRow` rows and handwritten migrations behave identically:
  compile + workspace/storage test evidence green with `derive`.
- DashMap concurrency semantics preserved: no architecture change, no
  Mutex/RwLock substitution; `remove_if` atomic check/remove paths
  (bus, project activation) compile unchanged against v6 and are covered
  by existing concurrency/presence/collaboration tests in the workspace
  sweep.
- No advisory silenced for green: LRU findings fixed by upgrade, not
  ignored; the sole retained ignore has a reachable lock-union path,
  `patched = []`, and a no-fixed-upgrade rationale.
- Lockfile churn attributable: §3 package accounting; no broad update.

## 6. Failure and recovery review

- TUI panic/error/terminal-restoration behavior unchanged
  (`src/tui/terminal.rs` untouched; guard semantics intact).
- DashMap update weakens no registry/session/presence/transport path
  (same APIs, same atomicity; full test sweep green).
- SQLx errors and migration failure paths unchanged (CodeGG-owned,
  handwritten; `migrate` feature removal affects only unused SQLx
  machinery).
- No persistent data migration introduced.
- The one observable dependency behavior change (ListState Copy) was
  classified as a no-op compatibility edit, not buried: surfaced by
  clippy, fixed minimally, re-verified.

## 7. Migration and compatibility review

- No schema migration; no config migration; no rollback beyond `git
  revert` of the manifest commit.
- Ratatui 0.30 requires Rust 1.88+ (crate `rust-version`); CodeGG MSRV is
  1.89 — compatible, no MSRV change.
- Crossterm 0.28 → 0.29 kept in the same surface because the selected
  Ratatui line defaults to a 0.29 backend and the optional image stack
  already required 0.29; this converges crossterm to one major instead of
  leaving divergent default/image backends.
- `ratatui` declaration uses `default-features = false` with
  `["crossterm", "underline-color"]` to preserve the 0.29-default surface
  without adopting `all-widgets`/`layout-cache`/`macros`.
- Lock-union additions (`ratatui-termina`, `ratatui-termwiz`, `termwiz`,
  `termina`, calendar-related transitive deps) are not enabled in any
  supported resolution (`cargo tree -p codegg` shows only `ratatui-core`,
  `ratatui-crossterm`, `ratatui-widgets`).

## 8. Security review

- LRU memory-safety exposure removed from supported builds: `lru 0.12.5`
  (RUSTSEC-2026-0002 + RUSTSEC-2026-0253) gone; `lru 0.18.0`
  (RUSTSEC-2026-0253) → `0.18.2` (patched).
- RSA/Marvin (`rsa 0.9.10` via `sqlx-mysql` lock union) remains
  reachable-to-audit but unreachable-to-build; exception retained with
  exact evidence and no fixed upgrade available. No new secret, network,
  sandbox, Landlock, authorization, SSRF, archive, plugin, or redaction
  behavior changed.
- New post-baseline advisory RUSTSEC-2026-0285 (`rustls 0.23.41`, medium,
  TLS 1.3 cross-encryption-level acceptance, fixed `>= 0.23.45`) is
  out-of-scope for M001 (broad-update exclusion) and is recorded in §10,
  not ignored.

## 9. Documentation and operations

- Updated: `docs/dependency-maintenance.md` (sqlx `derive`-only baseline +
  lock-union note); `.cargo/audit.toml` (rewritten RSA rationale);
  `plans/implementation/.../001-...md` (ready → active → implemented);
  `plans/registry.md` (§12); this closure record. No new CI lane, scanner,
  bot, size gate, or release automation added.
- Checked and intentionally left unchanged (no stale version pins found):
  `README.md`, `AGENTS.md`, `architecture/` docs (generic `DashMap`,
  `sqlx`, `ratatui` mentions carry no version pins), `.opencode/skills/`
  (generic `ratatui` mention in tui skill only).
- Preserved without rewriting: prior runtime-safety/HTTP/footprint closure
  records and the subsystem roadmap baseline narrative (historical
  evidence at `b3459640`).
- Operator diagnostics used as temporary evidence only: `cargo audit`,
  `cargo tree -d/-i`, `cargo tree -e features`, `cargo bloat`. None added
  as a permanent gate.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | RUSTSEC-2026-0285: `rustls 0.23.41` (TLS 1.3 handshake across encryption levels), fixed `>= 0.23.45`. Post-baseline advisory, unrelated family. | Bounded TLS-robustness issue in the HTTP stack; transcript still authenticated per advisory, not exploitable for handshake alteration by a network-position attacker. | Separate bounded maintenance (not M001 scope): targeted `rustls` upgrade with provider/streaming regression evidence. Do not bundle into M001. |
| low | Flaky `shared_client_follows_redirects_and_enforces_ten_hop_bound` loopback fixture (nonblocking accept + blocking read race; `WouldBlock`/`ConnectionReset`). Pre-existing, fails intermittently on main and M001 alike. | Test-only reliability noise; no production path affected. | Optional hardening pass: make the fixture's accepted streams blocking or retry `WouldBlock`; out of scope for M001, no production change warranted here. |
| info | `sqlx-mysql`/`sqlx-postgres`/`rsa` persist in the lockfile union despite empty reverse trees. | `cargo audit` will keep reporting RSA (suppressed with justification); no build/runtime exposure. | Upstream SQLx must drop the optional MySQL/RSA edge; revisit the ignore only then. No CodeGG patch/fork. |

No critical or high findings. No unexplained memory-safety advisory remains
in the supported graph.

## 11. Roadmap disposition

Milestone M001 closed. The M002 predecessor gate (M001 accepted closure) is
satisfied: M002 workspace dependency ownership normalization may proceed to
`ready` on its converged versions/features. M003/M004 remain blocked on
M002; M005 remains blocked on M002 plus the external generalized updater
interface.

## 12. Registry updates

- `plans/registry.md`: M001 implementation plan `active` → `closed`;
  subsystem current milestone `M001 active` → `M001 closed`; M002
  registered as dependency-ready (`ready`, predecessor M001 closed);
  Blocked-work M002 row cleared to reflect readiness; this closure added
  under Recently closed work with commit `3bd54ccd`.
- Subsystem roadmap: M001 status updated to closed; M002 unblocked note
  added (no baseline history rewritten).
- Implementation plan: Status `active` → `implemented` (closure record is
  the gate).
