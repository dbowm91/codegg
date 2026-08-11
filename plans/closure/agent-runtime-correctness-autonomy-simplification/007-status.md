# Agent Runtime Correctness, Autonomy, and Simplification M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/007-measured-binary-footprint-and-upstream-dependency-review.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md#7-ordered-milestones`

Repository baseline reviewed: `b7deca3e` (`Close M006 prompt policy consolidation`)

Implementation commits or pull requests:

- `deb07a2` — Wasmtime feature contraction, scoped dependency closure, and Wasmtime security patch update

## 1. Executive finding

M007 is closed. Fresh measurements identified one worthwhile, behavior-preserving
plugin-runtime optimization. Wasmtime defaults were replaced with the narrow
`runtime`, `cranelift`, and `std` feature set used by CodeGG, and the lock was
updated from Wasmtime `36.0.12` to `36.0.13` for the current 36.x security fix.
No supported feature, binary topology, CI gate, or release automation was removed
or added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Fresh default release baseline | `cargo build --release --locked`; `stat` | pass | `aarch64-apple-darwin`, Rust/Cargo `1.97.1`; baseline `54,437,200` bytes |
| Fresh crate bloat profile | `cargo bloat --release --bin codegg --crates --locked -n 50` | pass | `34.7 MiB` `.text`; top CodeGG-owned crates recorded below |
| Plugin-runtime baseline and final size | plugin-enabled `cargo bloat` runs | pass | `75,795,560` → `72,594,280` bytes; `40.9` → `39.0 MiB` `.text` |
| Duplicate and feature-tree review | `cargo tree -d --locked`; `cargo tree -e features --locked` | pass | Remaining duplicate families are retained where independently required; no blanket trimming |
| Current Wasmtime security review | [Wasmtime 36.0.12 release](https://github.com/bytecodealliance/wasmtime/releases/tag/v36.0.12), [RustSec package advisories](https://rustsec.org/packages/wasmtime.html), [RUSTSEC-2026-0222](https://rustsec.org/advisories/RUSTSEC-2026-0222.html) | pass | `36.0.13` is the fixed 36.x version for the newly listed type-index advisory |
| Feature candidate preserves plugin behavior | `cargo check --features plugins --all-targets --locked`; focused plugin tests | pass | 365 focused tests passed; fuel, timeout, module/linker ABI and policy code unchanged |
| Routine verification remains bounded | `scripts/verify.sh quick` | pass | No new guard, workflow, audit job, size gate, or release automation |

## 3. Production implementation evidence

`Cargo.toml` now declares the optional Wasmtime dependency with
`default-features = false` and `features = ["runtime", "cranelift", "std"]`.
This retains the engine/module/linker/store/value APIs and Cranelift execution
used by `src/plugin/runtime/wasm.rs`, while removing unused Wasmtime defaults such
as component model, async, cache, WAT, profiling, GC, pooling, coredump, and
debugging extensions from plugin builds.

`Cargo.lock` contains the scoped Wasmtime 36.0.13 dependency closure and removes
the now-unreachable default-feature closure. The lock diff is explainable by this
feature contraction plus the targeted Wasmtime patch update; no broad dependency
update was performed.

`architecture/plugin.md` documents the retained feature contract and the fact
that existing sandbox and resource limits remain the behavioral boundary.

Candidate measurements:

| Candidate | Disposition | Measurement / delta | Rationale |
|---|---|---:|---|
| Wasmtime default features | accepted change | plugin binary `75,795,560` → `72,594,280` bytes; −3,201,280 (−4.22%); `.text` `40.9` → `39.0 MiB` | Narrow feature set compiles and focused tests pass; no plugin ABI or policy loss |
| Wasmtime patch update 36.0.12 → 36.0.13 | accepted change | Scoped lock update | Required current 36.x security fix; no downgrade accepted |
| Desktop notifications / `notify-rust` | rejected / no change | `notify-rust` is a narrow 4.18.0 surface; no measured replacement | Retain cross-platform user-visible notifications; custom platform code increases maintenance risk |
| Syntect / Comrak | rejected / no change | Comrak is `136.0 KiB`; existing default narrowing is already applied | Further change lacks meaningful evidence and risks rendering behavior |
| `tar` / `flate2` archive support | rejected / no change | Shared by plugin install and EggLSP; no safe isolated reduction measured | Preserve install/download behavior; no custom archive parser |
| release `opt-level`/panic/topology changes | rejected / no change | Existing profile already uses LTO, strip, and one codegen unit | Avoid responsiveness regressions, panic-unwind changes, or binary split |

Default bloat top contributors remained materially the same because Wasmtime is
optional and absent from the default graph: `codegg` 7.8 MiB, `std` 3.7 MiB,
`serde_core` 3.0 MiB, `eggsact` 2.6 MiB, `serde` 2.2 MiB, `codegg_core` 1.6 MiB,
and `egglsp` 1.3 MiB. RustPython remained `596.4 KiB` and Comrak `136.0 KiB`.

Plugin bloat after the accepted change was led by `codegg` 7.8 MiB, `std` 4.2 MiB,
`serde_core` 3.1 MiB, `eggsact` 2.6 MiB, `serde` 2.2 MiB,
`cranelift_codegen` 1.5 MiB, `codegg_core` 1.6 MiB, `codegg_protocol` 1.5 MiB,
and `egglsp` 1.3 MiB. Wasmtime-specific contributors fell to `wasmparser`
479.7 KiB, `wasmtime_internal_cranelift` 373.0 KiB, `wasmtime` 254.3 KiB,
and `wasmtime_environ` 107.9 KiB.

## 4. Verification executed

All results below are local measurements on the same host/toolchain and are not
CI or hosted verification claims.

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
target: aarch64-apple-darwin

cargo build --release --locked
passed; final default artifact: 54,430,624 bytes

cargo bloat --release --bin codegg --crates --locked -n 50
passed; default profile: 34.7 MiB .text, 62.3 MiB unstripped analysis file

cargo bloat --release --features plugins --bin codegg --crates --locked -n 50
passed for Wasmtime baseline and minimized-feature candidate

cargo check --features plugins --locked --all-targets
passed; 52 crates compiled

cargo test --features plugins --lib plugin -- --test-threads=1
365 passed, 3816 filtered out

scripts/verify.sh quick
passed: formatting, generated-agent checks, core boundary, sandbox contract,
execution ownership, and workspace all-targets check
```

## 5. Invariant review

- Supported notifications, syntax/Markdown rendering, archive/install support,
  plugins, server, image, LSP, research, and other documented capabilities were retained.
- Default TLS and SQLite ownership were unchanged.
- Wasmtime fuel, timeout, module-size, memory-policy, ABI fallback, and plugin
  policy paths were unchanged; only unused dependency features were removed.
- Measurements use one host/toolchain and the same release profile. No binary
  size was made a CI gate.
- Lockfile churn is limited to the Wasmtime closure/security update and the
  unreachable feature closure.

## 6. Failure and recovery review

No scheduler, process ownership, persistence, cancellation, restart, or recovery
authority changed. Plugin timeout, fuel exhaustion, module compilation failure,
modern-to-legacy ABI fallback, and policy errors continue through existing paths.
Feature reduction does not create a new execution branch or unbounded resource path.

## 7. Migration and compatibility review

No storage, protocol, configuration, or user migration is required. The
`plugins` feature remains opt-in and the plugin ABI is unchanged. The default
release remains a single binary.

## 8. Security review

Wasmtime `36.0.12` was below the fixed `36.0.13` threshold for RUSTSEC-2026-0222.
The lock now uses `36.0.13`; the current RustSec package page was checked for
other applicable 36.x advisories, and no security downgrade was accepted.
The latest Wasmtime 45 line was not adopted because its release notes raise MSRV,
outside this repository's declared compatibility target. Existing plugin
sandbox/resource controls remain in place.

## 9. Documentation and operations

- Updated `architecture/plugin.md` with the retained Wasmtime feature contract.
- Recorded manual `cargo bloat`, `cargo tree`, release-size, and upstream review
  evidence here; no generated reports were committed.
- Added no binary-size/dependency guard, scheduled audit, dependency bot, CI
  workflow, release automation, or fixed cadence.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| critical | none | none | none |
| high | none | none | none |
| medium | none | none | none |
| low | Wasmtime 36.x remains an older maintained-line choice rather than latest major | Future MSRV/compatibility review may be needed | Revisit only with explicit evidence; not a M007 blocker |

## 11. Roadmap disposition

M007 is closed. M008 remains ready and independently executable. M009 is not
unblocked: M008 still has to close, while M001-M007 closure records are now
present. No other registered future plan had a blocker resolved by this milestone.

## 12. Registry updates

- Marked M007 `implemented` in its implementation plan and `closed` in this record.
- Marked M007 closed in the subsystem roadmap and moved it to recently closed plans.
- Removed M007 from dependency-ready plans.
- Updated M009's blocker from M007+M008 to M008 only; M009 remains blocked.
- Left M008 `ready`; no future plan became ready as a result of M007 alone.
