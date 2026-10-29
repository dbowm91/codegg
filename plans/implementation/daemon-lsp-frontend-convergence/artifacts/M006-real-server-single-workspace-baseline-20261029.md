# M006 Real-Server Single-Workspace Baseline — 2026-10-29

Status: partial qualification evidence; not an M006 closure

## Environment

- Host session reported `x86_64` by `uname -m`, macOS.
- rust-analyzer: `1.89.0 (29483883 2025-08-04)`.
- The compatibility suite's `basedpyright` profile selected `pyright-langserver`; its `--version` probe returned no version, so the report labels the version `unknown`. The matching installed Pyright CLI reports `1.1.411`.
- The opt-in compatibility suite ran serially against one temporary workspace per server.
- RSS and CPU were sampled from the language-server process with `ps` every 50 ms while the smoke test ran. CPU is the sampled process CPU percentage, not a sustained average.

## Results

| Server | Smoke result | Initialize | Readiness | Peak RSS | Peak sampled CPU | Samples |
|---|---|---:|---:|---:|---:|---:|
| rust-analyzer 1.89.0 | pass after one bounded retry | 28 ms | 2,807 ms | 834.0 MB | 157.8% | 85 |
| Pyright profile (CLI 1.1.411) | pass | 411 ms | 920 ms | 155.7 MB | 116.3% | 13 |

The rust-analyzer first references request returned LSP `ContentModified`
(-32801). A single retry after 250 ms returned two references and the smoke
test passed. Persistent errors still fail the test. The compatibility report
now records the request as exercised and successful after retry.

## Harness correction

The real-server smoke test had been reporting core document-symbol,
definition, references and hover checks in its human-readable `checks` list
but omitting those request outcomes from `operation_support`. The fallback
matrix marked each as unexercised/optional, so `assert_required_checks` could
pass despite a failing references check. Core operations now emit typed
request-site records, and the rust-analyzer run proved that an unretried
`ContentModified` fails the qualification. This change makes the smoke suite's
machine-readable matrix authoritative for those core operations as intended.

## Limits and next evidence

These observations cover one client and one worktree per language server on a
single host. They do not establish memory budgets, admission thresholds,
multi-worktree scaling, sustained CPU use, server-retirement safety, or
cross-platform behavior. M006 still requires the 1/2/4 client and 1/3
worktree matrix, warm/cold comparisons, process counts, and restart/retirement
evidence. The reports were generated under `crates/egglsp/target` and are
machine-local build artifacts; the exact results above are the durable record.

## Native ARM64 requalification

On 2026-10-29, reran the opt-in smoke tests with Rust 1.89.0 on the host's
native ARM64 toolchain (Apple M4 Pro, macOS 26.6.2; the default shell still
reports x86_64 under Rosetta). `rust_analyzer_smoke` passed against
rust-analyzer 1.89.0 in 4.94 seconds; `basedpyright_smoke` passed against the
Pyright profile / 1.1.411 in 2.36 seconds. These confirm operation
interoperability on the native host after the request-outcome harness fix.
They do not add a client/worktree scale point or a new resource sample, so the
matrix and M006 closure gates remain open.
