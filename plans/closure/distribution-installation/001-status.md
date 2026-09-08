# Distribution and Installation Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/distribution-installation/001-manual-prebuilt-artifact-contract.md`

Source subsystem roadmap:

- `plans/subsystems/distribution-installation-roadmap.md#7-milestones`

Repository baseline reviewed: `94ce12de213d71ad69c4450a700f470764aa108e`

Implementation commits:

- `94eb2754` — feat(release): add manual prebuilt artifact contract tooling (distribution M001)

## 1. Executive finding

M001 is complete. The stable manual prebuilt-artifact contract for the four
required Linux/macOS targets is implemented, tested, and documented.
Maintainer tooling packages one explicit already-built binary at a time into
a deterministic `codegg-<target>.tar.gz` archive, generates a sorted
two-column SHA-256 `checksums.txt` from exactly the files to be uploaded,
and validates the complete release set offline (manifest correctness,
completeness, unexpected-file rejection, archive payload inspection, and
native `codegg --version` smoke). `RELEASING.md` is the single artifact
naming source of truth; the stale `release/codegg-*` wildcard upload is
gone. No Rust runtime change was needed (`codegg --version` verified
working), and no GitHub Actions release automation was added. M002 has a
precise target/asset/checksum interface to implement against and is
unblocked by this closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Central allowlist of four required targets mapping to `codegg-<target>.tar.gz` | `scripts/release/lib-release.sh`: `CODEGG_REQUIRED_TARGETS`; `test-release-tools.sh` tests 1-2 assert mapping and required/optional distinction | pass |
| Archive member layout + executable permissions | `package-binary.sh` stages exactly `codegg` mode 755, verifies single-member list pre-rename; tests 7-8 assert payload/mode per target and no source/config/credential leakage | pass |
| Deterministic `checksums.txt` syntax (`sha256sum -c` compatible) | `finalize-release.sh` writes `<sha256>  <basename>` sorted LC_ALL=C, atomic replace; tests 24-28 assert stability across reruns and `sha256sum -c` acceptance | pass |
| Optional Windows treated explicitly, unknown names invalid | `CODEGG_OPTIONAL_TARGETS="x86_64-pc-windows-msvc"`; verifier accepts it only as an explicit entry; tests 55-61 cover windows-extra, windows-only strict-fail/relaxed-pass, unknown/traversal/absolute manifest rejection (tests 40-43) | pass |
| Safe packaging helper (explicit target/binary, no builds, atomic rename, refuse overwrite) | `package-binary.sh`; tests 9-19 (invalid/malicious/symlink/dir/non-exec inputs, overwrite policy, failure leaves no archive) and spaced-path quoting (tests 20-21) | pass |
| Checksum manifest binds exact upload set; validator fails on missing/extra/duplicate/tampered | `verify-release.sh` recompute + exact-basename lookup; tests 29-39 (complete set, tamper mismatch, missing file, incomplete strict-fail/relaxed-pass, duplicate) | pass |
| Archive payload validation + runnable-host version smoke | `verify-release.sh` single-`codegg` member, regular-file verbose check, symlink/traversal/absolute/device rejection, native-target `--version` vs `--expect-version`; tests 44-53 (crafted traversal/absolute/symlink/extra/missing fixtures) plus real-binary smoke below | pass |
| `RELEASING.md` build → package → validate → manual upload order, one naming source of truth | `RELEASING.md` Step 9 rewritten; wildcard upload removed; exact five-asset upload command; per-host `--allow-incomplete-target-set` documented as testing-only | pass |
| Focused shell/script tests with fixtures | `scripts/release/test-release-tools.sh`: 62/62 pass, offline, no network | pass |
| No release automation, manual cadence, one-binary topology preserved | `git status`/`git diff --stat` show no `.github/` or workflow changes; `RELEASING.md` states no Actions release job; archives hold one executable | pass |
| No Rust production change unless `--version` broken | `codegg --version` → `codegg 0.1.0` (verified via `cargo run --bin codegg`); zero `.rs` files touched | pass |

## 3. Production implementation evidence

New maintainer tooling under `scripts/release/` (all `bash`, `set -euo pipefail`, quoted, no `eval`):

- `lib-release.sh` — contract constants: 4 required targets, 1 optional
  Windows target, archive/checksum/member names, `sha256sum`/`shasum`
  selection, exact-match target/archive helpers, option-injection guard.
- `package-binary.sh` — `--target/--binary/--out-dir [--force]`. Strict
  allowlist, regular-file + non-symlink + non-empty + executable input
  checks, `mkdir -p` out-dir, refuse-overwrite default, private `mktemp -d`
  staging with trap cleanup, `cp` as `codegg` + `chmod 755`, deterministic
  gzip header best-effort (`tar -cf - | gzip -n`, fallback `tar -czf`),
  pre-rename member verification (exactly `codegg`, no absolute/traversal),
  atomic `mv` publish, prints target/path/sha256.
- `finalize-release.sh` — `--dir`. Hashes every supported archive present,
  deterministic basename sort, builds the manifest fully in a temp file then
  atomically renames; failure never leaves a partial `checksums.txt`.
  Completeness is deliberately left to the verifier.
- `verify-release.sh` — `--dir [--allow-incomplete-target-set]
  [--expect-version X.Y.Z] [--skip-version-smoke]`. Strict manifest grammar
  (64-char lowercase hex, two-column, bare supported basenames, no
  duplicates), exact-basename hash recompute, present-on-disk vs listed
  cross-check both directions, required-set completeness (relaxation is
  explicit and named), unexpected-file rejection (only supported archives +
  `checksums.txt` allowed, including dot-temp leftovers), per-archive
  payload inspection (single `codegg` regular file; absolute/traversal/
  symlink/device/unexpected rejected), native-target extraction + `codegg
  --version` smoke compared to `--expect-version` when given, non-native
  targets skipped with a build-host-smoke note. Idempotent re-runs.
- `test-release-tools.sh` — 62 offline assertions covering every section 10
  test class of the plan (allowlist, packaging, permissions, negative
  inputs, quoting, determinism, tamper, completeness, duplicates, unknown/
  traversal payloads, stale-temp fail-closed, Windows optionality, empty
  dir).

Final contract:

```text
release/
  codegg-x86_64-unknown-linux-gnu.tar.gz
  codegg-aarch64-unknown-linux-gnu.tar.gz
  codegg-x86_64-apple-darwin.tar.gz
  codegg-aarch64-apple-darwin.tar.gz
  checksums.txt
```

`.gitignore` gains `!scripts/release/` exceptions (build-output `release/`
dirs stay ignored). No `.rs`, workflow, CI, or config change.

## 4. Verification executed

### Commands run

```bash
scripts/release/test-release-tools.sh
# native real-binary smoke (rustc host aarch64-apple-darwin)
scripts/release/package-binary.sh --target aarch64-apple-darwin --binary target/debug/codegg --out-dir /tmp/codegg-release-smoke
scripts/release/finalize-release.sh --dir /tmp/codegg-release-smoke
scripts/release/verify-release.sh --dir /tmp/codegg-release-smoke --allow-incomplete-target-set --expect-version 0.1.0
# synthetic complete-set + negatives (fixture 0.1.0 binaries)
scripts/release/verify-release.sh --dir /tmp/m001-evidence/rel
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

### Results

- `test-release-tools.sh`: **62 passed, 0 failed** (local, macOS,
  offline). Covers all four required targets plus the optional Windows
  target, determinism, `sha256sum -c` compatibility, tamper/missing/
  duplicate/unknown/traversal/absolute/symlink/extra-payload negatives,
  stale-temp fail-closed, and spaced-path quoting.
- Native real-binary smoke: packaged `target/debug/codegg`
  (297 MB debug build) as `aarch64-apple-darwin`, finalized, verified with
  `--expect-version 0.1.0` → `version smoke:
  codegg-aarch64-apple-darwin.tar.gz -> codegg 0.1.0`, `release
  verification passed`. (Host `uname -m` reports `x86_64` under emulation;
  native detection uses the `rustc` host `aarch64-apple-darwin` first, and
  the packaged binary executed successfully.)
- Synthetic complete set (4 fixtures): strict verification passed —
  `completeness ok: all four required targets present`, all payloads ok,
  native fixture smoke `codegg 0.1.0`.
- Negatives: one-byte tamper → `checksum mismatch`; removed archive →
  `manifest lists missing file` (both exit nonzero; also covered in-test).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, agent schema, core boundary,
  sandbox, execution ownership, workspace check).

Local-only truth: no hosted CI run was used; none is required by this plan
(release smoke is maintainer evidence at publication time).

## 5. Invariant review

- Release/version/cadence decisions remain manual: `RELEASING.md` keeps
  maintainer-chosen version/tag/upload; no automation added.
- No GitHub Actions build/upload/publish: `git diff --stat` touches only
  `scripts/release/`, `.gitignore`, `RELEASING.md`, planning docs.
- One `codegg` executable per target: archives contain exactly `codegg`;
  docs restate the no-split topology.
- Stable required names; tag carries version: filenames have no version;
  documented explicitly with the no-embed-version rationale.
- Every required archive appears exactly once in `checksums.txt`: enforced
  by duplicate rejection + present-vs-listed cross-checks.
- SHA-256 from exact upload files: finalize hashes the final archives
  in place; verifier recomputes from the same paths.
- No accidental content: staging holds only the copied binary; member
  verification + unexpected-file rejection tested.
- Explicit validated target label: `--target` allowlist, no host guessing.
- Local/offline validation: no network calls in any script; tests hermetic.
- Windows optional: never required; tested both directions.

## 6. Failure and recovery review

- Packaging failure leaves no final archive (temp output + rename; tested
  across all negative inputs; existing archives untouched without `--force`).
- Checksum generation never leaves a partial manifest (temp + rename after
  all hashes succeed; empty dir fails before touching the manifest).
- Validation is read-only except private temp extraction with trap cleanup;
  re-runs are idempotent (tested).
- Concurrent packaging unsupported by design: default refuse-overwrite plus
  documented serialization; unique `mktemp` names prevent partial files.
- Stale temp files fail closed as unexpected directory entries (tested).

## 7. Migration and compatibility review

No user runtime migration. Historical GitHub releases without M001 assets
are explicitly not repackaged and must not be advertised as
installer-compatible. Crates.io ownership/procedure unchanged (`RELEASING.md`
Steps 1-8 untouched). Release output dir (`release/`) remains git-ignored;
only `scripts/release/` tooling is tracked via the new `.gitignore`
exceptions.

## 8. Security review

Untrusted shell arguments are quoted; option-like `--target/--binary/
--out-dir/--expect-version` values rejected; target allowlist is exact
equality (no patterns); no `eval`; private `mktemp -d` (mode 700) with
`EXIT/HUP/INT/TERM` traps; symlink inputs (binary, manifest, archives)
rejected; archives built only from a controlled staging dir; member names
checked for absolute paths and `..`; checksum parsing treats the manifest
as data with exact-basename match (never substring/regex); unexpected
directory entries fail closed; downloaded-code execution is out of scope
(no network). Malicious target/version/path/member fixtures all rejected
in tests 9-12, 17-18, 32-33, 40-43, 49-53.

## 9. Documentation and operations

- `RELEASING.md` Step 9: canonical contract, manual build → package →
  validate → upload order, exact upload command (no wildcard), per-host
  relaxation marked testing-only, offline test entry, historical-release
  note, M002 consumer pointer.
- No `architecture/` change warranted (maintainer tooling only, no runtime
  ownership change). No static guard added per plan (explicitly not needed;
  verifier is maintainer-invoked).
- Operator commands: `scripts/release/package-binary.sh`,
  `finalize-release.sh`, `verify-release.sh`, `test-release-tools.sh`
  (all `--help` documented).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Real (non-fixture) binaries were executed only for `aarch64-apple-darwin`; the other three targets are validated structurally via fixtures, with native build-host smoke prescribed in `RELEASING.md`. | No impact on the contract itself; first real four-target publication still needs per-host smoke. | Maintainer runs the documented per-host `--version` + package + verify steps at the first real release (M002 smoke consumes it). |
| low | `verify-release.sh` unexpected-file rule is strict (fails on any stray entry, e.g. `.DS_Store` or leftover temp files). | Maintainers must use a clean release dir; avoids silent stray uploads. | None; documented `ls -la release` inspection step. |

No critical, high, or medium findings.

## 11. Roadmap disposition

M001 is strictly closed. The subsystem roadmap stays `active` for M002.
M002 (`plans/implementation/distribution-installation/002-installer-and-end-user-installation.md`)
moves `blocked` → `ready`: its sole hard dependency (M001 stable
target/asset/archive/checksum contract) is now satisfied, with the exact
mapping table (`Linux x86_64 → x86_64-unknown-linux-gnu`, `Linux
aarch64 → aarch64-unknown-linux-gnu`, `Darwin x86_64 → x86_64-apple-darwin`,
`Darwin arm64 → aarch64-apple-darwin`), asset names
(`codegg-<target>.tar.gz` + `checksums.txt`), and manifest syntax
(`<sha256>  <basename>`, sorted) fixed above. No corrective pass required.

## 12. Registry updates

- `plans/registry.md`: Distribution subsystem row `M001 ready` → `M001
  closed, M002 ready`; remove the M001 row from dependency-ready plans and
  add the M002 row as ready; remove the M002 row from blocked work; record
  this closure under recently completed control points.
- `plans/subsystems/distribution-installation-roadmap.md`: M001 `ready` →
  `closed` with closure link; M002 `blocked on M001` → `ready`; milestone
  table updated; roadmap remains `active`.
- `plans/implementation/distribution-installation/001-manual-prebuilt-artifact-contract.md`:
  status → implemented/closed with closure link.
- `plans/implementation/distribution-installation/002-installer-and-end-user-installation.md`:
  status `blocked` → `ready for handoff` (hard dependency closed).
