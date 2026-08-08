# Post-Audit Correctness, Simplification, and Footprint M004 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/post-audit-correctness-simplification/004-dependency-feature-slimming-and-upstream-review.md`

Source subsystem roadmap: `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Implementation commit: `b437f8eb` (`slim dependency feature defaults`)

## 1. Executive finding

M004 is complete and closed. CodeGG now disables unused default features for
`qrcode` and `comrak`, and narrows `rustpython-parser` to the parser features
used by the restricted Tool Program frontend. The changes preserve terminal QR
rendering, Markdown rendering, syntax highlighting, and Tool Program parsing.

No dependency replacement, binary split, supported-feature removal, MSRV
increase, automated audit/size infrastructure, or broad lockfile update was
introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Verify QR API usage before narrowing | `src/tui/components/dialogs/share.rs` uses only `QrCode::new`, width, and terminal-character rendering; focused share tests pass | Satisfied |
| Disable qrcode defaults | Root manifest uses `qrcode = { version = "0.14.1", default-features = false }`; post-change tree has only `qrcode -> codegg` | Satisfied |
| Evaluate Comrak defaults | Root manifest uses `comrak = { version = "0.35", default-features = false }`; Markdown/TUI focused tests pass; AST APIs compile | Satisfied and retained |
| Measure and safely narrow RustPython | `cargo bloat` reports `rustpython_parser` at 596.4 KiB of `.text` (0.9%) and `malachite_nz` at 200.8 KiB (0.3%); defaults were replaced by `all-nodes-with-ranges` and `malachite-bigint`; Tool Program tests pass | Satisfied |
| Reject speculative parser replacement | No handwritten parser or replacement dependency was added; the upstream parser-supersession concern is deferred | Satisfied |
| Review upstream maintenance, advisory, and MSRV risks | Manual review covered qrcode, Comrak, RustPython Parser, and RustSec's Comrak advisories; locked versions remain compatible with the repository's Rust 1.81+ requirement | Satisfied |
| Preserve behavior and topology | Focused tests, workspace check, release build, and quick verification pass; the single `codegg` binary remains the topology | Satisfied |

## 3. Production implementation evidence

The implementation changes are confined to dependency declarations, the lockfile,
and Tool Program architecture documentation:

- `qrcode` default `image`, `svg`, and `pic` features are no longer selected.
- `comrak` no longer selects its default CLI, shell-word parsing, terminal-size,
  `xdg`, `bon`, and default syntect feature surface. CodeGG's direct `syntect`
  dependency remains unchanged, so syntax highlighting remains available.
- `rustpython-parser` keeps `all-nodes-with-ranges` and explicitly retains
  `malachite-bigint`; its optional location/fold feature surface is not selected.
- The lockfile removes the qrcode-only `image` edge and unused Comrak auxiliary
  nodes. `image` remains in the overall graph through independent supported
  routes and was not claimed to be globally removed.
- Duplicate `fancy-regex` resolution was reduced from the 0.16/0.18 pair to the
  compatible 0.18 line as a consequence of the narrowed graph.

## 4. Verification and measurements

All measurements used the same host and release profile: `aarch64-apple-darwin`,
Rust `1.97.1 (8bab26f4f 2026-07-14)`, Cargo `1.97.1`, `--release`, locked
dependencies, default repository features, and isolated target directories.

Baseline fresh release build:

- `/tmp/codegg-m004-baseline-target/release/codegg`
- 54,463,680 bytes

Final fresh release build:

- `/tmp/codegg-m004-final-target/release/codegg`
- 54,430,576 bytes
- reduction: 33,104 bytes

Post-change feature and duplication checks:

- `cargo tree -e features --locked -i qrcode`: only the qrcode package remains
  under CodeGG; image/SVG/pic defaults are absent.
- `cargo tree -e features --locked -i comrak`: only the Comrak package remains
  under CodeGG; its default feature closure is absent.
- `cargo tree -e features --locked -i rustpython-parser`: only
  `all-nodes-with-ranges` and `malachite-bigint` are selected.
- `cargo tree -d --locked`: no former `fancy-regex` 0.16.2 duplicate remains;
  unrelated duplicate/image routes were retained where still required.

Diagnostic release contributor measurement:

- `cargo bloat --release --bin codegg --crates --target-dir /tmp/codegg-m004-bloat-target -n 40 --locked`
- `rustpython_parser`: 596.4 KiB `.text`, 0.9% of the report.
- `malachite_nz`: 200.8 KiB `.text`, 0.3%.
- `comrak`: 135.9 KiB `.text`, 0.2%.
- The report is diagnostic and explicitly approximate, as documented by
  cargo-bloat; it was not added to the repository or CI.

Verification commands and outcomes:

- `cargo test --lib tui::components::dialogs::share`: 3 passed.
- `cargo test --lib tui::components::messages`: 28 passed.
- `cargo test -p codegg-core tool_program`: 159 passed across the selected
  Tool Program suites.
- `cargo check -p codegg-core --offline`: passed.
- `cargo check --workspace --all-targets --locked`: passed.
- `cargo build --release --locked`: passed in isolated baseline and final target
  directories.
- `scripts/verify.sh quick`: passed, including formatting, generated-agent
  checks, static guards, core-boundary checks, execution-ownership checks, and
  workspace compilation.
- `git diff --check`: passed before the implementation commit.

No hosted verification result was required for M004; M008 owns the final broad
integration and hosted verification pass.

## 5. Invariant review

- QR terminal rendering remains available and tested.
- Markdown AST rendering remains available and tested.
- Syntax highlighting remains available through CodeGG's direct `syntect`
  dependency.
- Tool Program language acceptance and rejection behavior is covered by the
  existing parser suite and remains unchanged.
- Rust 1.81+ remains the repository requirement; no MSRV increase was made.
- Optional large dependencies remain optional, and the single-binary topology
  remains unchanged.
- No storage, protocol, migration, daemon, scheduler, plugin, LSP, or auth
  behavior was changed.

## 6. Failure and recovery

No runtime or persisted-state code changed. Cargo graph, focused parser/TUI
tests, workspace compilation, release compilation, and quick verification were
used as recovery gates; a failed candidate would have been reverted without a
migration. No failure requiring recovery occurred.

## 7. Migration and compatibility

No migration, configuration change, protocol change, or user action is needed.
The accepted changes are implementation-internal Cargo feature selections and
retain the existing dependency versions and supported feature behavior.

## 8. Security and upstream review

Manual review of the material touched dependencies found:

- `qrcode` 0.14.1 is the locked/current accepted line reviewed for this
  milestone. The terminal-only API does not require its image/SVG/pic defaults.
- Comrak 0.35.0 is retained rather than upgraded. The RustSec Comrak advisories
  reviewed are patched before this locked version; no advisory-driven update
  was necessary. The package's active upstream release line is noted for a
  future compatibility review, but a major/minor migration was out of scope.
- RustPython Parser 0.4.0 is the locked line. Its upstream repository notes
  that the parser is superseded by Ruff's parser; replacement is deferred
  because M004 does not justify a parser migration, and no safe drop-in
  replacement was evaluated here.
- No direct qrcode or rustpython-parser advisory requiring a change was found
  in the manual review. No cargo-audit dependency or recurring audit workflow
  was added.

The review references were the [qrcode documentation](https://docs.rs/crate/qrcode/latest),
[Comrak releases](https://github.com/kivikakk/comrak/releases),
[RustPython Parser upstream](https://github.com/RustPython/Parser), and the
[RustSec Comrak advisory index](https://rustsec.org/packages/comrak.html).

## 9. Documentation and operations

`architecture/tool_program_language.md` and `architecture/tool_programs.md`
now state the exact RustPython feature selection and the parse-only boundary.
No dependency bot, scheduled audit, binary-size gate, release automation, or
new CI lane was added.

## 10. Unresolved findings

No critical, high, or medium findings remain for M004. The following low-risk
follow-up is explicitly deferred:

- Evaluate a future RustPython-to-Ruff parser migration only as a separately
  scoped compatibility/performance effort, with language-parity evidence and
  an MSRV review. M004 does not register that replacement plan automatically.

## 11. Roadmap disposition and future-plan audit

M004 is closed. M005, M006, and M007 remain independently ready. M008 remains
blocked because it has hard dependencies on M004 through M007 and the other
three milestones are not yet closed. No future plan became newly unblocked from
this closure.

The independent supported-Linux Landlock evidence condition in the runtime
safety workstream remains conditional and does not depend on M004; it is not
changed here.

## 12. Registry and status updates

The following records are updated with this closure:

- this closure record is the canonical M004 status;
- the implementation plan is marked `implemented — see` this record;
- the subsystem roadmap marks M004 closed while retaining M005–M007 as ready
  and M008 as blocked;
- `plans/registry.md` records M004 in recently closed implementation plans,
  removes it from active closure work, and keeps M008 blocked on M005–M007.

