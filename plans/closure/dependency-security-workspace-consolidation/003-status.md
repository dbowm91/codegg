# Dependency Security and Workspace Consolidation M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/dependency-security-workspace-consolidation/003-optional-image-feature-graph-slimming.md`

Source subsystem roadmap:

- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md#m003--optional-image-feature-graph-slimming`

Repository baseline reviewed: `05e7b258` (M002 implementation; plan snapshot baseline `b3459640`)

Implementation commits or pull requests:

- `e9b72c56` — M003: slim optional image feature graph to supported formats

## 1. Executive finding

M003 is complete. The optional `image` graph is contracted to exactly
the formats CodeGG supports: `image` is now `default-features = false`
with only `png`, `jpeg`, `gif`, `webp` plus `bmp` (built-in decoder,
retained as a proven runtime extra — see §3), and `ratatui-image`
enables only the `crossterm` backend with `image-defaults`
(`image/default`) removed. PNG/JPEG/GIF/WebP (+BMP) decoding and TUI
render preparation are covered by 9 new focused tests and green;
malformed/unsupported inputs fail safely through the existing error
path. The optional graph shrank (`--features image` tree 1423 → 1305
lines; image feature tree 73 → 38 lines; 44 packages pruned from the
lock, all attributable to unused default formats + `rayon`
parallel-decoder closure); the default graph is byte-identical in
structure (1219 lines before/after, zero image references in the
default `codegg` package tree). Default builds and TUI layout behavior
are unchanged. Broad verification is green; no new medium-or-higher
finding introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `image` defaults disabled in CodeGG-owned declarations | `Cargo.toml`: `image = { version = "0.25", default-features = false, optional = true, features = ["png", "jpeg", "gif", "webp", "bmp"] }` | pass | `rayon` + all 15 `default-formats` no longer enabled by CodeGG |
| `ratatui-image` no longer re-enables broad image defaults | `Cargo.toml`: `ratatui-image = { version = "10", default-features = false, features = ["crossterm"], optional = true }`; `image-defaults` removed; after-tree shows `ratatui-image feature "crossterm"` only, no `image-defaults` node | pass | Upstream `ratatui-image`'s own minimal `image` dep (`default-features = false` + `png`) inspected in the published `10.0.8` manifest; CodeGG's direct `image` dep owns the remaining decoders — no retained default feature, so no justification needed beyond this paragraph |
| PNG/JPEG/GIF/WebP behavior covered and green | 9 tests in `src/tui/components/image.rs`: per-format 2×2 encode→`load_from_memory` roundtrips (png/jpeg/gif/webp/bmp), `ImageSource` render-preparation, mime allowlist, data-URI parsing, malformed/unsupported failure path | pass | `cargo test --features image --lib tui::components::image`: 9 passed; full `--features image --lib`: 4507 passed, 0 failed |
| Optional graph equivalent or smaller; removed families attributable | `--features image` full tree 1423 → 1305 lines; image feature tree 73 → 38 (only `bmp/gif/jpeg/png/webp`, all CodeGG-owned; no `default`/`default-formats`/`rayon`); lock `-44` packages, all image-default exclusives (avif/ravif stack incl. `rav1e`/`avif-serialize`/`arg_enum_proc_macro`/`equator`/`rgb`/`maybe-rayon`, `exr`+`half`/`lebe`, `tiff`, `qoi`, `zune-inflate`, loop9/y4m/av-scenechange/v_frame et al.); `ravif` no longer matches any locked package | pass | Before/after trees saved as closure evidence inputs (§4); built-in-only formats (dds/hdr/pnm/tga/farbfeld) vanish via feature flags with no lock entries — likewise attributable |
| Default feature behavior and default binary unchanged | Default `cargo tree --offline` 1219 lines before and after; default `codegg` package tree contains zero image references; `verify.sh quick` (default workspace check) green | pass | Optional-only delta by construction (`optional = true` + `image` feature gate untouched) |
| TUI rendering/layout unchanged | `tui_render` 99 passed; `tui` 165 passed; change is manifest + tests + comment only outside the image stack | pass | No layout/event/lifecycle edit |
| No new medium-or-higher finding | `cargo audit`: only pre-existing rustls RUSTSEC-2026-0285 (medium, unrelated, M001-accepted) + allowed warnings; 0 `lru`; no image-family advisory | pass | §8 |
| Broad verification green | fmt, workspace check (default + image + all-features), clippy all-features `-D warnings`, lib (default sweep via workspace + image-feature 4507), focused TUI suites, `verify.sh quick` | pass | §4; release-artifact note below |

## 3. Production implementation evidence

Landed changes (M003 implementation commit):

- `Cargo.toml` (root, image stack only):
  - `image`: added `default-features = false`; explicit features
    `["png", "jpeg", "gif", "webp", "bmp"]`.
  - `ratatui-image`: features `["crossterm", "image-defaults"]` →
    `["crossterm"]`.
  - Long-form comment records the M003 contract: why defaults are
    off, why each retained feature exists, why `image-defaults` was
    safe to drop (upstream minimal dep inspected, not guessed).
- `Cargo.lock`: -44 packages / -454 lines, all image-default-format
  exclusives (enumerated in §2). No version change to any retained
  package; no non-image family touched.
- `src/tui/components/image.rs`: added `#[cfg(test)] mod tests` (9
  tests, §2). No production logic changed — the mime allowlist,
  `parse_data_uri`, `load_from_data_uri`/`load_from_path`, and
  protocol selection are byte-identical outside the test module.
- `docs/dependency-maintenance.md`: image checkpoint appended to the
  feature-ownership list (defaults-off, retained set, TUI contract,
  no broader support advertised).

### Retained-`bmp` justification (the only `+1` beyond the plan's named four)

`is_supported_image_format` already accepted `image/bmp` before M003,
and `src/tool/read.rs` already maps `bmp` → `image/bmp`, so BMP is a
proven runtime format, not a new addition. The plan authorizes
explicitly enabling proven runtime features beyond the named four
("plus any proven runtime feature CodeGG requires"). `bmp` in `image`
0.25 is a built-in decoder (`bmp = []`, no extra dependency family),
so retaining it costs zero graph weight while avoiding a silent
user-visible removal (removing it would have been out-of-scope per
the plan's non-goals and would have tripped the stop condition).
The TUI contract is therefore documented as PNG/JPEG/GIF/WebP (+BMP
retained); no broader support is advertised, and no other default
format was retained.

### Deliberately absent

Image UI redesign; provider/image-generation API changes; `image`
crate replacement; SIMD/codec micro-optimization; default-binary size
claims or permanent size gates (verification policy forbids them);
publication or release automation.

## 4. Verification executed

### Before/after feature trees (same host/toolchain)

```bash
cargo tree --features image --locked -i image@0.25.10 -e features  # before: 73 lines (default/default-formats/rayon + 15 formats)
cargo tree --features image --locked                                # before: 1423 lines
cargo tree --locked                                                 # before (default): 1219 lines
# ... apply M003 ...
cargo tree --features image --offline -i image@0.25.10 -e features # after: 38 lines (bmp/gif/jpeg/png/webp only)
cargo tree --features image --offline                               # after: 1305 lines
cargo tree --offline                                                # after (default): 1219 lines
cargo tree --offline -p codegg                                      # default package tree: 0 image refs
cargo tree --offline -i ravif                                       # after: no match (pruned from lock)
git diff Cargo.lock | grep "^-name"                                 # 44 removed packages, all image-default exclusives
```

### Commands run

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --features image --locked
cargo test --features image --lib -- --test-threads=1
cargo test --features image --lib tui::components::image -- --test-threads=1
cargo test --test tui_render --locked -- --test-threads=1
cargo test --test tui --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
cargo audit
cargo tree --offline -i ravif
```

### Results

- `cargo fmt --all -- --check`: pass (one auto-format applied to the
  new test module, re-green).
- `cargo check --workspace --all-targets --features image --locked`:
  pass.
- `cargo test --features image --lib tui::components::image`: 9
  passed, 0 failed.
- `cargo test --features image --lib`: 4507 passed, 0 failed.
- `tui_render`: 99 passed; `tui`: 165 passed.
- `CARGO_BUILD_JOBS=1 cargo test --workspace --locked`: green (every
  `test result:` line `ok`, zero non-ok).
- `cargo check --workspace --all-targets --all-features --locked`:
  pass.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings`: pass.
- `scripts/verify.sh quick`: passed (fmt, agent schema,
  core-boundary, sandbox, execution-ownership, tui-authority guards,
  workspace check).
- `cargo audit`: 1 vulnerability (rustls RUSTSEC-2026-0285, medium,
  pre-existing per M001 §10) + allowed unmaintained/yanked warnings;
  0 `lru` findings; no image-family finding.
- Optional release artifact/bloat observation: not re-run. Rationale:
  the plan's acceptance is graph-based (equivalent-or-smaller +
  attributable removals), which the tree/lock evidence satisfies
  directly; byte deltas are descriptive-only per WP4 and the roadmap
  verification policy forbids turning artifact size into a gate; a
  second full `--release --features image` build pair was not run to
  avoid ~2× costly release builds for a descriptive number, and no
  size claim is made in this closure. Recorded here as unrun rather
  than invented, consistent with the planning anti-pattern rule
  against recording only successful evidence.

## 5. Invariant review

- `--features image` retains PNG/JPEG/GIF/WebP (+BMP) decoding: proven
  by roundtrip tests through the real `image` decoders at the resolved
  `0.25.10` versions, not by manifest inspection alone.
- TUI rendering/layout unchanged: manifest + tests + docs only; both
  TUI suites green; `ImageSource` preparation exercised headless.
- Default builds unaffected: optional-only delta; default tree
  identical; default package tree image-free.
- No custom decoder/encoder introduced: tests use the `image` crate's
  own encoders purely as fixture generators, never shipped.
- Security limits and input validation unchanged: production validation
  code untouched; malformed/unsupported inputs still fail through the
  existing `Err` path (tested, including truncated-header no-panic).

## 6. Failure and recovery review

- Image load failures (bad data URI, unsupported mime, corrupt bytes,
  truncated headers) return `Err` without panic: covered by the
  `malformed_and_unsupported_inputs_fail_safely` test.
- No storage, protocol, scheduler, daemon, or authorization path
  touched. No migration. Rollback is `git revert` of the M003 commit;
  re-enabling defaults would restore the old graph without data
  consequences.

## 7. Migration and compatibility review

- No schema, config, protocol, or data migration. No MSRV change
  (`image` 0.25 / `ratatui-image` 10 already MSRV-compatible per the
  M001 baseline). No public API change. File-path loads of
  now-unenabled formats (e.g. TIFF/AVIF via `image::open`) fail closed
  through the pre-existing `Err` path rather than silently
  mis-decoding; the data-URI gate already rejected those mimes before
  M003, so no advertised contract changed (BMP retained precisely to
  avoid changing one).

## 8. Security review

- Attack surface reduced: 44 default-format packages (including the
  AV1/EXR/TIFF/QOI decoder stacks and `rayon` parallel-decoder
  closure behind `image/default`) leave the optional graph; fewer
  decoders reachable from untrusted image bytes under the `image`
  feature.
- No advisory silenced; no audit-ignore touched. Fresh `cargo audit`
  shows no new medium-or-higher finding from this change (sole medium
  is the pre-existing, unrelated rustls item).
- No secret, network, sandbox, Landlock, authorization, SSRF,
  archive, plugin, or redaction behavior changed.

## 9. Documentation and operations

- Updated: `Cargo.toml` (contract comment at the image stack),
  `docs/dependency-maintenance.md` (image checkpoint),
  implementation plan status (`blocked` → `implemented` via the
  ready/active transitions in this push), this closure record. No new
  CI lane, scanner, bot, size gate, or release automation added.
- Checked and intentionally left unchanged: `README.md` ("terminal
  image support" — no format list to narrow), `architecture/`
  (generic `image` mention only), `.opencode/skills/` (no format
  pins), prior closure records (history preserved).
- Temporary diagnostics (`cargo tree`, lock diff) remain closure
  evidence only; none added as a permanent gate.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | RUSTSEC-2026-0285 (`rustls 0.23.41`, pre-existing, unrelated family) | Bounded TLS-robustness issue; transcript still authenticated. | Separate bounded maintenance; not M003 scope. |
| info | Optional release artifact/bloat observation not re-run (see §4) | No size claim made; graph evidence is the acceptance basis. | None in this workstream; future release prep may record a descriptive optional-artifact number without creating a gate. |

No critical or high findings. No unexplained memory-safety advisory in
the supported graph.

## 11. Roadmap disposition

Milestone M003 closed. M004 (reusable crate boundary qualification) was
unblocked by M002 and is unaffected by M003 — it remains `ready` and
may proceed independently (no M003 interface to consume; the image
stack stays a local single-consumer optional). M005 remains blocked
solely on the external generalized updater interface (its M002 hard
dependency is now satisfied). No previously registered plan was
unblocked by M003 closure beyond what M002 already unblocked; the
remaining unrelated conditional blockers (architecture-convergence
M009 compatible-host Clippy evidence, runtime-safety C002 Landlock
fixture evidence) are untouched by this workstream.

## 12. Registry updates

- `plans/registry.md`: M003 implementation plan `blocked` → `closed`
  (via ready/active/implemented transitions in this push); subsystem
  current milestone `M002 closed, M003 ready` → `M003 closed, M004
  ready`; Blocked-work M003 row cleared; M004 row retained as `ready`
  (predecessor M002 closed); M005 blocker narrowed to the external
  generalized updater interface only; this closure added under
  Recently closed work with the M003 implementation commit.
- Subsystem roadmap: M003 status updated to closed; M004 ready
  reaffirmed; M005 externally-blocked note retained (no baseline
  history rewritten).
- Implementation plan: Status `blocked` → `implemented` (closure
  record is the gate).
