# Command Surface Reconciliation M002 — CLI Surface Cleanup

Status: ready

Corrective roadmap:
plans/subsystems/command-surface-reconciliation-corrective-addendum.md

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

## 1. Objective

Make the `codegg` process CLI accurate, coherent and maintainable without breaking
existing public automation unnecessarily.

This is primarily a parser/help/ownership cleanup. It does not change provider,
sandbox or daemon semantics.

## 2. Fix objective parser/help defects first

### Verbosity

Align help with implementation or implementation with the conventional mapping. The
current behavior is:

- no `-v` → warn;
- `-v` → info;
- `-vv` → debug;
- `-vvv` (and above if capped) → trace.

Document/test exactly that mapping unless a deliberate behavior change is separately
justified.

### Doctor syntax and scope

Make the documented simple syntax work:

`codegg doctor [subsystem]`

Prefer an optional positional subsystem. Retain `--subsystem` as a hidden/deprecated
compatibility spelling for at least one release if it has been public.

Reconcile the enum with the help text. With the installation/provider corrective work,
use a bounded set such as:

- all
- search
- mcp
- lsp
- deterministic-tools
- providers/connections
- credentials
- installation

Only advertise diagnostics that have real implementations. Installation diagnostic
data is supplied by Self-Contained Installation M002; this plan owns CLI exposure.

### Feature-aware help

Root after-help/examples must not advertise `server` or remote `attach` when those
subcommands are not compiled. Use Clap's actual command metadata or feature-gated help
fragments rather than an unconditional prose block.

## 3. Share duplicated execution arguments

Extract shared Clap `Args` structures for execution policy where root one-shot mode
and `exec` expose the same concepts:

- approval mode;
- sandbox policy;
- yolo/unsafe override;
- model/agent where semantics are truly identical;
- cwd/output controls where appropriate.

The parser should have one validation function for mutually exclusive policy choices.

Do not force interactive-only flags into `exec` merely to share a struct.

## 4. Explicit launch-mode conflicts

Define Clap conflict/requirement groups for:

- continue latest session;
- explicit session;
- fork;
- no-session/new session as currently supported.

Remove order-dependent “last branch wins” behavior. Invalid combinations should fail
in argument parsing with concise help before runtime state is opened.

Add table-driven parser tests for valid/invalid combinations.

## 5. One-shot execution surface

Audit the overlap between root `-p/--run` and `codegg exec`.

Preserve compatibility, but choose one canonical implementation path. Prefer routing
both spellings through the same exec request/options rather than maintaining separate
behavior.

Normalize output naming around one typed format option (for example
`--format text|json`) while retaining existing `--output-format` /
`--json-output` spellings as compatibility aliases where currently public.

Do not remove a public one-shot shorthand in the same corrective commit that
introduces the canonical path unless the repository's deprecation policy explicitly
permits it.

## 6. Transport/developer surface

Ordinary help should focus on user operations. Reclassify:

- deprecated `--core-transport`;
- `--standalone`;
- `--stdio`;
- `--core-endpoint`;
- hidden `core-stdio`;
- `attach-daemon`.

Keep flags required by internal harnesses/compatibility, but hide deprecated/internal
ones from normal help and document them in architecture/developer docs.

Move daemon attachment under `codegg daemon attach` if the current daemon command
tree can own it cleanly; retain `attach-daemon` as a hidden compatibility alias.
Do not conflate it with feature-gated remote HTTP `attach`.

## 7. Orphan plugin CLI

`src/command/plugin.rs` defines `PluginCommand` and install/search/list behavior,
but it is not wired into the root CLI/module execution surface.

Do not leave a phantom CLI implementation.

At implementation baseline, inspect whether current TUI/plugin services expose a
stable shared command service suitable for headless use:

- if yes and the parser can call that shared authority without duplicating install
  policy, deliberately expose `codegg plugin ...` and add tests/docs;
- otherwise remove the orphan Clap module/file and stale CLI documentation. Do not wire
  dead/older marketplace code merely to preserve a file.

Record the chosen disposition in closure evidence.

## 8. Naming and help cleanup

Update root/about/help text to describe CodeGG as the Rust-native coding agent rather
than a “lightweight implementation of Codegg” if that stale wording remains.

Reconcile singular/plural names where aliases already exist, but avoid a mass breaking
rename. Prefer one canonical name plus hidden/documented alias.

Generate CLI examples from commands that exist in the default build or guard them with
the same feature condition.

## 9. Tests

Add parser/help golden or structured tests for:

- verbosity mapping;
- `doctor` positional and compatibility syntax;
- doctor advertised subsystem coverage;
- default-feature help excluding unavailable feature-gated commands;
- all-feature help including them;
- execution-policy conflicts;
- session-launch conflicts;
- root one-shot and `exec` sharing behavior;
- compatibility aliases for output/daemon attach;
- plugin CLI chosen disposition.

Tests should inspect Clap command metadata where possible instead of brittle terminal
width formatting.

## 10. Documentation

Update README, architecture/command.md, architecture/core.md and relevant daemon/server
docs. Separate end-user CLI from internal transport entry points.

Do not advertise commands that require an optional build feature without saying so.

## 11. Verification

- focused CLI parser tests
- `codegg --help` default-feature smoke
- `codegg doctor --help`
- `codegg exec --help`
- all-feature CLI help/parser smoke
- daemon command tests
- `cargo fmt --all -- --check`
- strict workspace Clippy
- `scripts/verify.sh quick`
- `git diff --check`

## 12. Acceptance

Close when the default CLI help is truthful for the compiled binary, documented
examples parse as written, duplicated execution policy has one validation authority,
invalid launch combinations fail at parse time, internal transports no longer clutter
the user surface, and the orphan plugin CLI has an explicit implemented-or-removed
disposition with tests.
