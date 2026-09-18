# Command Surface Reconciliation Corrective Addendum

Status: closed

M001 closed (`plans/closure/command-surface-reconciliation-corrective/001-status.md`;
implementation `6e5203b6`); M002 closed
(`plans/closure/command-surface-reconciliation-corrective/002-status.md`;
implementation `da7fab03`). Both exit conditions in §6 hold; no further
milestones are registered under this addendum.

Repository baseline reviewed: 5af6766a04d6326ba1e7bb3695b2f0422399e9ee

Related closed work:

- plans/implementation/tui-project-sessions/010-command-discovery-keybinding-convergence.md
- plans/closure/tui-project-sessions/010-status.md
- plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md
- architecture/command.md
- architecture/tui.md

## 1. Purpose and corrective trigger

Reconcile CodeGG's user-facing TUI slash-command and process CLI surfaces with the
capabilities that are actually executable today.

The audit found two distinct classes of drift:

1. the TUI command registry is canonical for discovery/validation, while execution is
   still selected by a second string-matching surface; and
2. the Clap surface has accumulated stale help, duplicated execution flags,
   development transport switches and an orphan plugin CLI implementation.

This corrective work is intentionally smaller than a command-system redesign. It
makes one authority executable, fixes proven unreachable/stale commands, and makes the
CLI tell the truth about the default build.

## 2. TUI findings

M010 correctly converged command discovery metadata and preserved one parser/router,
but explicitly left the existing dispatcher intact. Its tests proved metadata coverage
for registered commands, not bidirectional registry ↔ executable-action coverage.

At the current baseline, the audit identified dispatcher literals that are not
registered through the normal command registry and are therefore unreachable from
ordinary typed slash input:

- `/checkpoints`
- `/edit-reapply`
- `/edit-undo`
- `/history`
- `/model`
- `/tool-contracts`
- `/worktree`

Conversely, `/checkpoint` is registered and documented but no matching direct
execution branch was found during the audit. It must be traced to its intended current
domain before retention; do not invent new checkpoint semantics merely to satisfy a
coverage test.

The architecture documentation also contains built-in command counts that have become
stale as the registry evolved.

## 3. CLI findings

The current root CLI contains useful user modes mixed with compatibility/developer
transport controls.

Concrete defects/drift:

- verbosity help says `-v` is warning, `-vv` info and `-vvv` debug, while the
  implementation maps no flag → warn, `-v` → info, `-vv` → debug,
  `-vvv` → trace;
- README examples use `codegg doctor search`, while Clap currently requires
  `codegg doctor --subsystem search`;
- doctor help claims provider/storage diagnostics that are not represented by the
  current `DoctorSubsystem` enum;
- root after-help advertises feature-gated `server` / `attach` commands even when
  they are absent from the default build;
- execution-policy flags are duplicated between root one-shot execution and
  `exec`;
- launch/session flags lack a single explicit conflict/precedence model;
- deprecated/internal transport flags occupy the ordinary top-level help surface;
- `attach-daemon` is a top-level command even though daemon lifecycle is otherwise
  grouped;
- output naming differs between root and `exec`;
- `src/command/plugin.rs` defines a Clap plugin command implementation that is not
  wired into the root CLI/module authority.

## 4. Invariants

- Existing user scripts do not break without an explicit deprecation/compatibility
  path.
- Slash-command authorization, observer/read-only restrictions and permission checks
  remain downstream authorities; command metadata/action selection cannot grant
  capability.
- Dynamic project/plugin commands retain their existing precedence/collision policy.
- TUI command discovery performs no new I/O on the input hot path.
- CLI cleanup must not expose internal core transports or remote secret operations
  accidentally.
- Default-build help lists only commands actually compiled into that build.
- Documentation is generated/guarded from canonical metadata where practical rather
  than maintained as an independent numeric census.

## 5. Milestones

### M001 — Executable TUI command authority and reachability

Status: closed (`plans/closure/command-surface-reconciliation-corrective/001-status.md`; implementation `6e5203b6`)

Plan:
plans/implementation/command-surface-reconciliation-corrective/001-tui-command-action-convergence.md

Attach typed executable actions to the canonical command registry, route resolved
commands through that action, repair the proven reachability gaps and add exhaustive
coverage tests.

### M002 — CLI truthfulness and compatibility cleanup

Status: closed (`plans/closure/command-surface-reconciliation-corrective/002-status.md`; implementation `da7fab03`)

Plan:
plans/implementation/command-surface-reconciliation-corrective/002-cli-surface-cleanup.md

Fix objective help/parser mismatches, share duplicated argument structures, hide/group
internal transports, reconcile doctor behavior and remove or deliberately wire orphan
CLI code while retaining compatibility aliases for public behavior.

## 6. Exit conditions

- Every registered built-in slash command has a defined executable action or an
  explicitly non-executing template/process/dialog action.
- Every built-in executable action is reachable through one canonical registered name
  or alias.
- No second free-form string switch can silently add a slash command without registry
  coverage.
- The seven audit-discovered unreachable commands are reconciled and
  `/checkpoint` has an evidence-backed disposition.
- Help/docs command counts cannot silently drift from the registry.
- Default `codegg --help`, `codegg doctor --help` and relevant subcommand help are
  accurate for the compiled feature set.
- Compatibility tests cover retained public aliases/options.

## 7. Non-goals

- Renaming every existing top-level slash command into nested families.
- Replacing Clap.
- Rewriting dynamic command/plugin loading.
- Changing permission/sandbox semantics.
- Adding new provider-onboarding semantics; that belongs to the provider `/connect`
  corrective roadmap.
