# Dependency Security and Workspace Consolidation M006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/dependency-security-workspace-consolidation/006-rustls-advisory-remediation-and-planning-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md#m006--rustls-advisory-remediation-and-planning-reconciliation`

Repository baseline reviewed: `b8b08774d615985105c06d15d12d0f33b8ce9fd1` (M006 plan registration; plan snapshot baseline `24cf2327`)

Implementation commits or pull requests:

- (this change) — M006: targeted lock-only Rustls 0.23.41 → 0.23.45 remediation plus planning reconciliation

## 1. Executive finding

RUSTSEC-2026-0285 is remediated on the existing 0.23 family with the
smallest compatible change: a lock-only `cargo update -p
rustls@0.23.41 --precise 0.23.45`. The only package-set change is
`rustls 0.23.41 → 0.23.45` plus its causally required companion
`rustls-webpki 0.103.13 → 0.103.15` (Rustls itself requires the newer
WebPKI). No manifest changed, no direct CodeGG `rustls` dependency was
added, no audit ignore was added, and the Eggfetch HTTP/1 +
Rustls/WebPKI ownership (ring crypto, TLS 1.2 + 1.3, packaged
`webpki-roots`) is byte-for-byte unchanged in behavior.

`cargo audit` reports zero occurrences of RUSTSEC-2026-0285 and zero
vulnerability-severity findings of any kind. The remaining audit output
is the pre-existing informational warning set (unmaintained crates plus
one yanked `spin 0.9.8` notice), none of which was introduced by this
two-package change. Focused provider/streaming/Eggpool/update/EggLSP
regression suites and the broad workspace posture (fmt, Clippy
`-D warnings`, full workspace tests, `scripts/verify.sh quick`) are all
green.

Recommendation: closed. No medium-or-higher M006 finding remains open.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reproduce advisory + owner graph before editing (WP1) | `cargo tree -i rustls@0.23.41 --locked` → single resolved `rustls 0.23.41` owned via `eggfetch-core 0.1.4` (direct edge) + `hyper-rustls 0.27.9` + `tokio-rustls 0.26.4`; `cargo tree -i rustls --locked --all-features` identical (one version); feature tree: `ring`, `tls12`, `std`, `logging` (no aws-lc/fips); `rustls-webpki 0.103.13`; `webpki-roots 0.26.11` via eggfetch (with nested `1.0.8`); `cargo audit` showed RUSTSEC-2026-0285 (medium, 5.3, fix `>=0.23.45`) | pass | Advisory reproduced exactly at the plan's snapshot versions |
| Smallest patched update, lock-only preferred (WP2) | `cargo update -p rustls@0.23.41 --precise 0.23.45` succeeded with no owner change; `git diff Cargo.lock` package-set delta is exactly `rustls 0.23.41→0.23.45` + `rustls-webpki 0.103.13→0.103.15` (verified by set-diff of old vs new lock: REMOVED 2, ADDED 2, nothing else) | pass | No `cargo update` broad run; no manifest edit; no new Eggfetch release, fork, or family migration needed |
| Companion changes causally justified (WP2 rule 3) | `rustls-webpki` bump is required by the patched Rustls release line itself; it is the only companion change | pass | Crypto provider (`ring`) and trust store untouched |
| No direct root `rustls` dependency (invariant) | `rg rustls Cargo.toml crates/*/Cargo.toml` matches only `eggfetch-core` feature lines (`http1`, `tls-rustls`, `json`); `git diff` touches `Cargo.lock` only (8+/8-) | pass | Resolution stays transitive through Eggfetch |
| No RUSTSEC-2026-0285 ignore (invariant) | `git diff .cargo/audit.toml` empty; audit file retains only the separately justified `RUSTSEC-2023-0071` RSA/SQLx lock-union exception | pass |  |
| Focused consumer regression (WP3) | `cargo test -p codegg-providers --lib --locked` → 129 passed / 0 failed; `cargo test --lib core::eggpool` → 10/0; `cargo test --lib provider` → 28/0; `cargo test --test upgrade` → 11/0; `cargo test -p egglsp --all-features --locked` → all suites ok (1000+43+19+14+11+5+5+5+3+2 lib/integration/doc, 0 failed) | pass | Existing deterministic suites only; no new TLS framework, no public-network test |
| Advisory absent post-update (WP4) | `cargo tree -i rustls --locked` → `0.23.45` on the 0.23 line, same owner edges; `cargo audit` → 0 occurrences of RUSTSEC-2026-0285, 0 `Severity` lines (zero vulnerabilities) | pass |  |
| No new advisory introduced (WP4) | Post-update audit output contains only the pre-existing unmaintained/yanked warning set; package-set diff proves no unrelated package version changed, so none of those warnings could have been introduced here | pass | See §10 info findings |
| RSA ignore unchanged (WP4) | `.cargo/audit.toml` untouched; `cargo tree -i sqlx-mysql/rsa` reachability rationale from M001 still stands (no evidence reviewed invalidates it; out of M006 scope to re-litigate) | pass |  |
| Broad verification green (WP5) | `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` clean; `cargo test --workspace --locked -- --test-threads=1` → exit 0, 214 ok suites, 10621 passed / 0 failed; `scripts/verify.sh quick` → passed | pass | Capped resources per repo policy (`CARGO_BUILD_JOBS=1`, `--test-threads=1`) |
| Planning reconciliation (WP5) | Plan `ready` → `implemented`; this closure record created; registry control-point rows corrected to `M006 closed`; M005 left blocked; M001-M003 history untouched | pass | §12 |

## 3. Production implementation evidence

Landed change (single file, lock-only):

- `Cargo.lock` (8 insertions, 8 deletions):
  - `rustls 0.23.41 → 0.23.45` (checksum + version; same dependency list).
  - `rustls-webpki 0.103.13 → 0.103.15` (checksum + version; same dependency list).
  - Three dep-edge re-pointings with no version change at either end,
    all among versions already present in the lock: `errno`/`rustix`/
    `tempfile` edge `windows-sys 0.52.0 → 0.60.2`, and `tempfile` edge
    `getrandom 0.4.3 → 0.3.4`. Both endpoints of every re-pointed edge
    remain locked (`windows-sys` still ships 0.48.0/0.52.0/0.60.2/0.61.2;
    `getrandom` still ships 0.2.17/0.3.4/0.4.3 with `uuid` still on
    0.4.3). This is resolver normalization under the refreshed index,
    not unrelated package churn: no package was added, removed, or
    bumped beyond the two Rustls-line entries, and hand-reverting the
    edges would only be rewritten by the next non-`--locked` resolve.

No manifest (`Cargo.toml`), source, config, or audit-config file
changed. Before/after reverse trees for default and `--all-features`
resolutions show the identical owner shape (`eggfetch-core 0.1.4` →
`codegg`, `codegg-providers` (+ `codegg-core`), `egglsp`;
`hyper-rustls 0.27.9`; `tokio-rustls 0.26.4`) with only the version
number advanced.

## 4. Verification executed

Local verification (all on the patched lockfile, `--locked` unless the
command itself performs resolution):

- `cargo tree -i rustls@0.23.41 --locked` / `-i rustls` / `-e features
  -i rustls` / `-i rustls-webpki` / `-i webpki-roots@1.0.8` /
  `-i webpki-roots@0.26.11` — before-state census (§2 row 1).
- `cargo tree -i rustls --locked --all-features` — single-version
  confirmation in the maximal feature resolution.
- `cargo update -p rustls@0.23.41 --precise 0.23.45` — the remediation.
- `cargo tree -i rustls --locked`, `cargo tree -e features -i rustls
  --locked`, `cargo audit` — after-state reconciliation (§2 rows 7–8).
- `cargo tree -d --locked` — no new CodeGG-attributable duplicate
  introduced (representative head: `base64`/`plist`/`sqlx-core` edges
  unchanged in shape).
- `cargo test -p codegg-providers --lib --locked -- --test-threads=1` —
  129 passed, 0 failed (covers provider client construction, redirects,
  model discovery, SSE/streaming, cancellation).
- `cargo test --lib core::eggpool --locked` — 10 passed, 0 failed
  (bounded request/body/error behavior incl. deterministic TLS/port
  matrix test).
- `cargo test --lib provider --locked` — 28 passed, 0 failed.
- `cargo test --test upgrade --locked` — 11 passed, 0 failed (Eggfetch
  update-check client behavior).
- `cargo test -p egglsp --all-features --locked -- --test-threads=1` —
  all suites ok, 0 failed (download redirect/body/archive-security and
  transport behavior).
- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings` — clean.
- `cargo test --workspace --locked -- --test-threads=1` — exit 0; 214
  `test result: ok` suites; 10621 passed, 0 failed.
- `scripts/verify.sh quick` — passed (fmt, agent schema,
  core-boundary, sandbox, execution-ownership, tui-authority guards,
  workspace check).

No hosted CI lane was added and none is required; no public network is
touched by the committed test suite (`cargo audit`/index refresh is
local diagnostic tooling, not a test). No one-shot local HTTPS smoke
was needed: there is no existing deterministic TLS fixture, and the
patch-level bump is covered by compile-every-consumer (workspace check
inside `verify.sh quick`) plus the deterministic HTTP/streaming/
static-routing suites above, exactly as the plan allows.

## 5. Invariant review

- Eggfetch remains the sole HTTP transport owner with the approved
  profile: dependents still declare `eggfetch-core` with `http1` (+
  `tls-rustls`, `json` where previously owned); feature tree confirms
  `ring` + `tls12`, no `tls-native-roots`, no HTTP/2/3, no proxy/cookie
  expansion.
- Deterministic packaged WebPKI roots preserved: `webpki-roots`
  entries absent from the lock diff; no `tls-native-roots` string in any
  manifest or the lock.
- Provider request/stream timeout, redirect, idle-pool, SSE,
  cancellation, and error-redaction semantics: covered by the green
  provider/Eggpool suites; no source changed.
- WebFetch/research/MCP validated-destination and no-second-DNS
  behavior: no source changed; static-routing suites green via the
  workspace run.
- EggLSP download redirect/body/archive-security: `egglsp
  --all-features` green; no source changed.
- Rust 1.89 MSRV: no manifest or toolchain change; only `Cargo.lock`
  versions moved within long-stable 0.23/0.103 lines.
- Check-only/fail-closed upgrade posture (M005 boundary): untouched;
  `tests/upgrade.rs` 11/11 green post-patch.
- No new CI lane, scanner, bot, size gate, or release automation.

## 6. Failure and recovery review

- TLS validation failures remain failures: certificate/hostname
  validation code was not touched (no source change at all); the patch
  only corrects cross-encryption-level acceptance of TLS 1.3 handshake
  messages per the advisory.
- Cancellation/timeout semantics remain owner-controlled above Rustls
  (Eggfetch + provider layers untouched).
- Provider/MCP/EggLSP errors remain sanitized and transport-neutral
  (no source change; suites asserting redaction/classification green).
- No data/config/protocol migration; rollback is a `Cargo.lock` revert
  (`git revert` of this change).
- No observable application-contract change was detected: the full
  workspace suite (10621 tests) passes without modification.

## 7. Migration and compatibility review

- No public API, CLI, config, storage-layout, or protocol change.
- Supported builds resolve exactly one Rustls version (`0.23.45`); no
  split-version graph.
- `rustls-webpki 0.103.15` is the patch-line companion required by
  `rustls 0.23.45`; no downstream API impact (no CodeGG crate depends
  on either directly).
- All platforms share the same lockfile resolution; no
  platform-specific carve-out.

## 8. Security review

- RUSTSEC-2026-0285 (medium, TLS 1.3 handshake across encryption-level
  boundaries): resolved — supported graph no longer contains an
  affected Rustls; handshake transcript was and remains authenticated.
- `cargo audit` post-update: zero vulnerability findings (no `Severity`
  lines). Remaining output is the pre-existing informational warning
  set only; see §10.
- No advisory silenced: no ignore added; existing `RUSTSEC-2023-0071`
  RSA/SQLx lock-union exception unchanged with its reachability
  rationale intact.
- Attack surface unchanged in shape: same Eggfetch transport, same
  trust store, same crypto provider, same protocol features.
- No secret, SSRF/pinning, archive-traversal, plugin-sandbox,
  Landlock, authorization, or redaction behavior changed (no source
  change; guards in `verify.sh quick` pass).

## 9. Documentation and operations

- Updated: `Cargo.lock` (the remediation itself); this closure record;
  `plans/registry.md` (M006 closed, stale control points corrected);
  subsystem roadmap M006 status; implementation plan status.
- Checked and intentionally left unchanged: M001-M003 closure records
  (accurate post-baseline history — rewriting them is prohibited by
  the plan); `docs/dependency-maintenance.md` and architecture docs
  (no version-specific Rustls text found outside historical records);
  `.cargo/audit.toml`; all manifests; no new operator documentation
  needed (no behavior change).
- No new CI lane, scanner, bot, size gate, or release automation.
  Temporary diagnostics (`cargo tree`, `cargo audit`, lock set-diff)
  remain closure evidence only.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| info | `cargo audit` still reports pre-existing unmaintained-crate warnings (`bincode 1.3.3`, `instant`, `paste`, `unic-*`, `yaml-rust`) | Informational only; no vulnerability severity attached; none touched by this change (package-set diff proves it) | None in M006; any future action is separate bounded work, not an M006 follow-up |
| info | `cargo audit` reports yanked `spin 0.9.8` warning | Informational warning, not an advisory; version unchanged by this change, so pre-existing | None in M006 |
| low | Resolver-normalized dep edges (`errno`/`rustix`/`tempfile` → already-locked `windows-sys 0.60.2` / `getrandom 0.3.4`) appear in the lock diff alongside the patch | Cosmetic lockfile lines; both endpoints already locked; no version added/removed/bumped beyond the Rustls line | None; documented here so a future reader does not misattribute them |

No critical, high, or medium findings. No new medium-or-higher
workstream finding introduced by the remediation.

## 11. Roadmap disposition

Milestone M006 is closed. The dependency/security portion of the
subsystem roadmap now satisfies its completion criteria except for the
explicitly allowed blocked item: M001-M004 have accepted closure
records, M006 has this accepted closure evidence, current advisory
review shows no unresolved critical/high or unexplained fixable medium
finding, every remaining audit ignore is reachable and justified, and
broad local verification is green.

Dependency audit for unblocking (registry Blocked work + subsystem
dependency graphs):

- Dependency security/workspace M005 remains independently blocked on
  the generalized external updater interface
  (`plans/closure/dependency-security-workspace-consolidation/005-status.md`).
  M006 neither satisfies nor alters that interface dependency, and per
  the M006 plan it must not reintroduce automatic self-update
  machinery as a side effect — none was introduced.
- No registered implementation plan lists dependency-security M006 as a
  hard or interface dependency, so closing M006 unblocks nothing and
  no plan moves to `ready` in this commit.
- Unrelated conditional blockers (architecture-convergence M009
  compatible-host Clippy evidence, runtime-safety C002 Landlock fixture
  evidence) are untouched by this workstream.
- HTTP client consolidation M001-M003 remain closed; the published
  `eggfetch-core 0.1.4` surface is reused unchanged.

Per the roadmap's completion criteria, the roadmap itself remains
active solely for the named external updater-interface dependency
(M005).

## 12. Registry updates

- `plans/registry.md`: Active-subsystem row now reads `M006 closed;
  M005 blocked`; Dependency-ready M006 row `ready` → `closed` with a
  pointer to this closure; Current-execution-order gate 1 rewritten
  from "current handoff" to closed disposition; Closure-control-point
  row corrected from `M006 ready; M005 blocked` to `M006 closed; M005
  blocked` (the stale control point this milestone owned); Recently
  closed work gains the M006 row with its implementation commit.
- Subsystem roadmap: header status and M006 section now `closed` with a
  pointer to this closure; M005 blocked note retained.
- Implementation plan: Status `ready` → `implemented` (lock change
  landed; this closure record is the gate and records `closed`).
