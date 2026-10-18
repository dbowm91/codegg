# Linux Trusted Git Cost Corrective C003 — gix versus Contained Git Read Feasibility

Status: ready — **comparison/test-only; no production dependency or read-backend promotion**
Primary class: infrastructure
Baseline reviewed: `planning/linux-sandbox-next-admission-and-adr-cleanup` `0b36c71f (post-rebase review baseline; the pre-rebase evidence commit was 004d770b8cd23ca33fb724dae95f97de563ba60d)` (2026-10-18).
Source roadmaps: `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`, `plans/subsystems/linux-execution-isolation-roadmap.md`.
Long-term `plans/000-long-term-specification.md` §§4,17,27,29; accepted ADR-0004/0016/0034/0035; proposed ADR-0036 (not adopted).
Target future closure: `plans/closure/linux-trusted-git-cost-corrective/003-status.md`.
Interface dependency: share C001 fixture definitions where ready; otherwise compare static safe fixture, no hard C001 dependency.

## Objective

Determine if a **minimal-feature `gix` read library or separate worker** offers sufficient latency, portability and structured-result correctness improvement over native Git inside the Linux sandbox to justify increased code/dependency maintenance. Make an explicit keep-CLI vs adopt-gix vs selective-hybrid decision based on measurements, not prior assumption.

## Current source and constraints

`crates/egggit` already provides structured `status`, `diff`, `log`, `blame`, `refs`, `worktree`, and parse/convert logic over pinned shell-free `GitEnvPolicy` subprocesses. `GitExecutionService` produces established `GitPayload` DTOs and currently falls back to raw Git for some structured errors. The cold object snapshot is a launch-layer overhead separate from native Git parsing performance. A new `gix` dependency has potential compilation and binary size cost; in-process repo parsing would expose daemon to untrusted data. Measure the *actual* alternatives.

## Invariants / non-goals

No default read backend changes, no full rewrite of Git mutation stack, no migration, no new public structured payload, no assumed trusted-repository status merely because UID ownership matches. No surprise network/promisor fetch, config-driven executable extension, implicit credential use or unapproved clone/submodule handling. Do not claim gix supports every feature of native Git. No dependency promotion to normal Codegg build in this pass.

## Ordered packages

1. Freeze a representative matrix using actual `GitPayload` shapes: status/untracked/submodule, staged/unstaged diffs, `log` and show, refs/HEAD, worktrees/common-dir, binary data, rename and weird path encodings; include packed/fragmented and SHA-1/SHA-256 when supported. Use pinned official Git as the semantic oracle and report deliberately unsupported cases.
2. Create an **isolated, opt-in comparison harness** (bench/example crate behind no-default feature or separate research source) that can use minimal `gix` feature sets, with explicit configuration trust filtering and no external config execution. Avoid adding gix to default `codegg` build or lockfile if a separate throwaway benchmark workspace suffices; if lockfile must change, keep compile evidence and revert unselected dependency before closure.
3. Compare three read routes under identical repo states: native Git with no snapshots, existing Codegg snapshot route, and gix (in-process benchmark as non-production and, if promising, a separate sandboxed worker). Preserve structured semantics; record exact Git/gix version and feature graph.
4. Run repeated cold/warm samples (>=100 warm / >=30 cold per packed and >=4k-loose-object >=100 MiB fragmented fixture where feasible); report p50/p95/max for end-to-end request, pure operation, sandbox startup, RSS, CPU, disk bytes, retained scratch bytes and tail impact with 1,4,8 concurrent callers. Include compile time (clean incremental comparison), binary size and dependency counts on ordinary x86-64 runner, **not an SBC startup/compilation benchmark**.
5. Security differential: malformed pack, reflog/config/include, alternates, replace refs, malicious Git executable settings, invalid UTF-8 paths, out-of-scope gitdir and unexpected object data. No untrusted repo parsing within production daemon; a candidate worker must not inherit host sockets/credentials. Identify paths where `gix` behavior intentionally differs from Git CLI.
6. Produce an evidence-backed recommendation: `native-contained-git`, `gix-worker-selected-reads`, or `defer-gix`. Include impact on existing failure classes (`RepositoryAlias`, `ChangedFiles` payload, structured fallback) and explicitly separate snapshot avoidance benefit from CLI-to-library benefit.

## Change scope / lifecycle

Research fixtures and optional test-only prototype under `research/linux-trusted-git-cost-corrective/` / dedicated benchmark crate or `crates/egggit` test gates; no production rewiring to `src/git_service.rs` by default. Any candidate process must use owned lifecycle and bounded output if given untrusted data. No config/DB/DTO migrations; cross-platform targets remain unchanged.

## Verification / evidence

`cargo fmt --all -- --check`; `cargo test --locked -p egggit`; selected `GitExecutionService` parity tests; `python3 scripts/check_execution_ownership.py`; `python3 scripts/check_adr_integrity.py`; `scripts/verify.sh quick`. Benchmark commands should be recorded verbatim, with fixture identity, controlled cache state, exact code SHA, host, compiler, versions and raw sample artifacts. CI must compile baseline with default features unchanged. No security conclusion from a benchmark alone.

## Closure and stop

A closure record at `plans/closure/linux-trusted-git-cost-corrective/003-status.md` must show comparative values, parity failures, dependency cost, tested vs untested gitoxide features and a concrete go/no-go. Stop if `gix` runs untrusted hooks/config with higher daemon authority, unexpected dependency inclusion widens production surface, or comparison misattributes old snapshot costs to native Git. C003 completion informs C005 but does not unblock C002 production/ADR or authorize gix deployment.
