# Repository Audit Corrective Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/repository-audit-corrective/002-destructive-delete-classification-coverage.md`

Source subsystem roadmap:

- `plans/subsystems/repository-audit-corrective-roadmap.md`

Repository baseline reviewed: `8590c17c5` (branch `fix/c002-audit-corrective`, worktree
`/Users/davidbowman/projects/codegg/.worktrees/c002`; the audit's baseline was `d5be7acd0`).

Implementation commits or pull requests:

- `fix(tool): make the destructive-delete risk signal recognise the effective spellings` — the
  canonical root-delete recogniser, the narrowed human-shell block, the fixture member, the
  coverage guard, the operator note and this closure record.

Governing decision: **accepted ADR-0045** —
`plans/adrs/ADR-0045-command-syntax-and-execution-authority.md`. Command syntax is
**advisory**: a classifier annotates risk and escalates to the ApprovalRouter; it never
guarantees an outcome, never grants authority, and never substitutes a bypassable parser for
a removed one.

## 1. Executive finding

The milestone's capability boundary is complete. The built-in destructive-command
classification now **tells the truth in both directions**: it recognises the spellings GNU
coreutils actually executes against the filesystem root, and the built-in human-shell block
no longer hard-blocks a legitimate scoped absolute path.

One recogniser, `destructive::classify_root_delete`, is now the single owner of the
"recursive force removal of the filesystem root" classification. The tool-side classifier
(`destructive_match`), the bash-policy seam (`src/tool/bash/policy.rs`) and the human-shell
block (`src/shell/policy.rs`) all route through it, so they can no longer disagree about the
same spelling — which is precisely the defect shape ADR-0045 forbids ("do not substitute an
equally bypassable command parser elsewhere").

The signal stayed advisory. No authority ceiling, no ApprovalRouter ordering, no
user-configured rule, no stored receipt and no sandbox boundary changed. The built-in
default blocked-command set remains empty.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The effective root-delete spellings are classified as catastrophic | `tests/security_permission/destructive_delete_classification.inc::root_delete_spellings_are_classified_as_root_targets`; `destructive::classify_root_delete` | pass | `rm -rf --no-preserve-root /` and `rm -rf / --no-preserve-root` are `RootTarget(FilesystemRoot)` |
| The effective spellings reach the approval path | `…::an_effective_root_delete_reaches_the_approval_path` (`PermissionChecker::check_with_args` → `PermissionResult::Ask`) | pass | The escalation is at the permission layer, not a text denial |
| A scoped absolute path is not hard-blocked as a root delete | `…::human_shell_does_not_block_a_scoped_absolute_path`; `…::scoped_removals_do_not_raise_the_advisory_risk_signal` | pass | `rm -rf /tmp/build` and `rm -rf /home/user/project/cache` are `Allow` in the human shell |
| `rm -rf /` remains classified as catastrophic | `…::root_delete_spellings_are_classified_as_root_targets`; `…::human_shell_blocks_the_filesystem_root_spellings` | pass | Unchanged verdict |
| A user-configured deny still matches | `…::a_configured_deny_still_denies_before_the_risk_signal` | pass | `bash_deny_patterns: ["rm -rf /tmp/build"]` → `PermissionResult::Deny` |
| `default_blocked_commands()` is still empty | `src/tool/bash/policy.rs::default_blocked_commands_is_empty_by_design` (pre-existing lib test, untouched) | pass | The set was not "restored" |
| The fixture set is not vacuous | The baseline run below: **10 of 11 tests fail** against the baseline implementation | pass | Captured verbatim in §4.1; the one green test is the plan's *migration invariant* (a configured deny must keep matching), which is green before and after by design |
| The classifier does not pretend to resolve a variable target | `…::a_variable_target_is_reported_as_unresolved` → `RootDeleteVerdict::UnresolvedTarget` | pass | `rm -rf "$TARGET"` |
| A benign prefix does not suppress the signal | `…::a_benign_prefix_does_not_suppress_the_root_delete_signal` | pass | `echo ok && rm -rf --no-preserve-root /` |
| The fixtures never spawn a command | `scripts/check_shell_policy_test_safety.py` (the fixture member is in its audited list) | pass | No `.execute(`, no `Command::new`, no `TerminalTool` |
| The human-shell refusal makes no guarantee claim | `…::human_shell_root_delete_reason_makes_no_guarantee_claim`; the reason text | pass | It says "best-effort text match, not a security boundary" and no longer says "catastrophic" |
| One canonical recogniser | `scripts/check_destructive_risk_signal_coverage.py` (`single-recogniser` rule) | pass | All three owners call `classify_root_delete` |
| The guard runs in `verify.sh quick` and in CI | `scripts/verify.sh` `run_guards()`; `.github/workflows/ci.yml` | pass | Both the guard and its `--self-test` are wired |
| The guard's `--self-test` proves non-vacuity | `python3 scripts/check_destructive_risk_signal_coverage.py --self-test` | pass | Three injected regressions rejected |
| The operator note states that syntax is not a security boundary | `docs/security-semantics.md` § "The destructive-command risk signal (operator note)" | pass | Plain statement, plus the FullHost/YOLO caveat |
| `architecture/tool.md` records the advisory nature and the recognised spellings | `architecture/tool.md` § "Destructive-command classification (advisory)" | pass | Includes the verdict table |
| The WP1 inventory of the remaining built-in syntax-only denials | §3.3 below | pass | In-scope and out-of-scope sets are explicit |
| No user-configured rule, receipt, ordering or ceiling changed | The diff (§7) | pass | Only the three classification files changed |
| `scripts/verify.sh quick` (workspace sweep + clippy) | Not run — the shared-machine discipline for this workstream forbids the workspace sweep and `verify.sh full` | **not run** | Named condition, not a green result; see §4 and §10 |
| Linux and Windows platforms | Not exercised | **not run** | macOS only; named condition in §4 |

## 3. Production implementation evidence

### 3.1 The canonical recogniser

`src/tool/destructive.rs` gains `classify_root_delete(command) -> RootDeleteVerdict`, with
three verdict shapes:

- `RootTarget(RootDeleteTarget)` — `FilesystemRoot` (`/`), `FilesystemRootGlob` (`/*`) or
  `HomeDirectory` (`~`, `$HOME`);
- `UnresolvedTarget` — a recursive force removal whose target the text does not resolve;
- `NotRootDelete` — not a recursive force removal of a root target.

The recogniser matches the **intent** — a recursive force removal whose target resolves to
the filesystem root — by looking at the **target position**, not the flag position. It reads
one command's option and target positions: the `rm` command word, the option words that
carry recursive/force, `--` as the end-of-options marker, and the target words. It is not a
shell parser and it does not evaluate expansions, redirections or control flow.

The three root-delete entries of `DESTRUCTIVE_BASH_PATTERNS` (the flag-anchored regexes) are
**removed**, not supplemented. The remaining entries of that table are unchanged.

### 3.2 The three owners, and what each now does

| Owner | Before | After |
|---|---|---|
| `src/tool/destructive.rs::destructive_match` | flag-anchored regexes; the two effective spellings were **not** matched | `classify_root_delete` first, then the unchanged remaining patterns. The display names (`rm -rf /`, `rm -rf /*`, `rm -rf ~ or $HOME`) are preserved |
| `src/tool/bash/policy.rs::contains_catastrophic_delete` | `split_once("rm -rf /")` on a literal prefix, plus a root-name suffix list that flagged `/home/…` | delegates to `classify_root_delete`. The `"rm -rf /"` entry is removed from `RISK_SIGNAL_PATTERNS` so there is exactly one matcher |
| `src/shell/policy.rs::check_block_patterns` | `BLOCK_RM_RF_ROOT` / `BLOCK_RM_FR_ROOT`, which ended in a bare `/` with no anchor and so hard-blocked every absolute path | delegates to `classify_root_delete`, and blocks only `FilesystemRoot` / `FilesystemRootGlob`. The home directory keeps its pre-existing **warning** |

The human-shell refusal text moved from the "catastrophic delete" framing to ADR-0045
decision 3's framing:

> a recursive removal of the filesystem root is refused by this built-in command-text rule
> (best-effort text match, not a security boundary)

It makes no guarantee claim, and it names the rule's own limitation.

### 3.3 WP1 — the built-in syntax-only denials that remain

Every entry below is a built-in heuristic that reads command **text**. None of them is
OS containment.

**`src/tool/destructive.rs` — `DESTRUCTIVE_BASH_PATTERNS` (advisory risk → Ask)**

| Pattern | Matches | Use | In scope here |
|---|---|---|---|
| `rm -rf /`, `rm -rf /*`, `rm -rf ~ or $HOME` | the flag-anchored root/home forms | advisory risk | **yes — removed and replaced by the canonical recogniser** |
| `mkfs on /dev` | `mkfs(\.\w+)?\s+/dev/` | advisory risk | no |
| `dd from /dev` | `\bdd\s+[^|;&]*\bif=/dev/(zero|urandom|random)` | advisory risk | no |
| `fork bomb (:(){:...|:})` | the canonical fork bomb | advisory risk | no |
| `system shutdown or reboot` | `shutdown`, `reboot`, `halt`, `poweroff`, `init 0`, `telinit 0`, `systemctl …` | advisory risk | no |
| `write to /dev/sd or /dev/nvme` | `> /dev/sd…` | advisory risk | no |
| `partition tool (fdisk/parted/mklabel)` | `fdisk`, `parted`, `sfdisk`, `sgdisk`, `mklabel` | advisory risk | no |
| `curl … \| sh or wget … \| sh` | download piped into a shell | advisory risk | no |

**`src/tool/bash/policy.rs`**

| Built-in | Matches | Use | In scope here |
|---|---|---|---|
| `default_blocked_commands()` | **nothing** — the set is empty by design | terminal deny | **no — unchanged, and deliberately not "restored"** |
| `RISK_SIGNAL_PATTERNS` (fork bomb, `\|\| rm -rf`, mkfs, dd, partition, umount, init, shutdown, `kill -9 -1`) | their named shapes | advisory risk → Ask | no, except the `rm -rf /` entry, which is **removed** in favour of the canonical recogniser |
| `ASK_PATTERNS` | substitution, pipe-to-shell, device redirects, `eval`, `exec`, `source`, inline interpreters, `base64 -d`, `chmod`/`chown` to a system path, `sudo` … | advisory ask | no |
| `check_shell_security` | the **explicit** blocked list and the allowlist only | terminal deny | no — unchanged |

**`src/shell/policy.rs` — the human `!cmd` path**

| Built-in | Matches | Use | In scope here |
|---|---|---|---|
| `BLOCK_RM_RF_ROOT`, `BLOCK_RM_FR_ROOT` | `rm <flags> /` — any absolute path | terminal deny | **yes — removed and replaced by the canonical recogniser** |
| `BLOCK_MKFS_DOT`, `BLOCK_MKFS_SPACE` | `mkfs` | terminal deny | no — belongs to the broader ADR-0045 migration |
| `BLOCK_DD_ZERO`, `BLOCK_DD_DEV` | `dd` from a device node | terminal deny | no |
| `BLOCK_FORK_BOMB` | the fork bomb | terminal deny | no |
| `BLOCK_SHUTDOWN`, `BLOCK_REBOOT`, `BLOCK_POWEROFF`, `BLOCK_HALT` | the power commands | terminal deny | no |
| `WARN_RM_RF_DOT`, `WARN_RM_FR_DOT` | `rm -rf .` (current directory) | advisory warn | no |
| `WARN_RM_RF_HOME`, `WARN_RM_FR_HOME` | `rm -rf ~` / `$HOME` | advisory warn | no — the home removal stays a warning, not a block |

**Other heuristic critical-command surfaces**

| Surface | Matches | Use | In scope here |
|---|---|---|---|
| `src/permission/mod.rs` (`check_with_args`) | the destructive/ask signals | advisory escalation → Ask | no — the ordering is unchanged; it now sees the corrected verdict |
| `crates/eggsentry/src/command.rs::CRITICAL_RM_RE` | `rm <flags> (/\s*$\|~/\|/\*\s*$\|\.\s*$\|\*$)` | severity label for the `security` tool and the agent's security hint | **no — see §10.** It is a separate crate's severity classifier, it does **not** have the scoped-absolute-path false positive, and it under-reports the two effective spellings in the same way the repaired matcher did |
| `src/tool/risk.rs::classify_bash_risk` | the tool-risk surface | advisory risk label | no |

### 3.4 The fixture set — before and after

`tests/security_permission/destructive_delete_classification.inc` (a new member of the
`security_permission` family). Every command below is inert classifier data; **nothing is
spawned**.

| Spelling | Baseline verdict | After | Note |
|---|---|---|---|
| `rm -rf /` | catastrophic | `RootTarget(FilesystemRoot)` | unchanged |
| `rm -rf --no-preserve-root /` | **not** catastrophic | `RootTarget(FilesystemRoot)` | the form coreutils executes — the defect |
| `rm -rf / --no-preserve-root` | **not** catastrophic | `RootTarget(FilesystemRoot)` | option after the target — the defect |
| `rm -fr /` | catastrophic | `RootTarget(FilesystemRoot)` | flag order variant |
| `rm -rf /*` | catastrophic | `RootTarget(FilesystemRootGlob)` | root glob |
| `rm -rf ~` | catastrophic | `RootTarget(HomeDirectory)` | home |
| `rm -rf $HOME` | catastrophic | `RootTarget(HomeDirectory)` | home |
| `rm -rf /tmp/build` | **catastrophic** (the bash seam flagged the `/` prefix and the shell block refused it) | `NotRootDelete`, human shell `Allow` | the scoped-absolute-path defect |
| `rm -rf ./build` | not catastrophic | `NotRootDelete` | unchanged |
| `rm -rf /home/user/project/cache` | **catastrophic** (the bash seam's root-name suffix list) | `NotRootDelete` | the scoped-absolute-path defect |
| `rm -rf ~/docs` | **catastrophic** (the baseline's `rm … (~|$HOME)` pattern had no end anchor, so it flagged every path starting with `~`) | `NotRootDelete` | a removal *inside* the home directory, not of it |
| `rm -rf "$TARGET"` | not recognised (but not *distinguishable* from "scoped") | `UnresolvedTarget` | the classifier now says "not recognised" explicitly |

The three rows the plan requires to move — the two effective spellings and the scoped
absolute path — all move. `rm -rf ~/docs` is the milestone's own addition: it is a scoped
removal the **baseline classifier** mis-flagged, so the fixture fails on the baseline for
the classifier's own verdict and not only through the two seams.

### 3.5 The guard

`scripts/check_destructive_risk_signal_coverage.py`, wired into `scripts/verify.sh`'s
`run_guards()` and into `.github/workflows/ci.yml`. It uses `scripts/rust_test_scan.py` for
every Rust parse (literal and test-function extraction), never a regex scan of Rust. Its
rules:

- `fixture-coverage` — a member of the `security_permission` family covers every fixture
  row, is bound in the family module, and asserts the verdict through
  `classify_root_delete` and `evaluate_command` rather than through a pattern name;
- `single-recogniser` — the canonical recogniser exists and all three owners call it;
- `effective-spelling` — no built-in pattern may recognise the bare `rm -rf /` while
  missing an effective spelling (the flag-anchored shape is rejected);
- `scoped-absolute-path` — no built-in pattern in the human-shell or bash policy may match
  `rm -rf /tmp/build` or `rm -rf /home/user/project/cache`;
- `no-guarantee-claim` — the human-shell refusal names itself a best-effort text rule and
  does not say "catastrophic".

`--self-test` copies the tree, injects three regressions (a human-shell pattern matching a
scoped path; an uncovered fixture set; the flag-anchored root pattern) and proves each is
rejected, while the unmodified tree is accepted.

## 4. Verification executed

### Commands run

```bash
# The fixture member (new).
CARGO_BUILD_JOBS=3 cargo test --test security_permission -- destructive_delete_classification

# The affected library test modules.
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib -- destructive tool::bash::policy shell::policy

# The new guard and its non-vacuity proof.
python3 scripts/check_destructive_risk_signal_coverage.py
python3 scripts/check_destructive_risk_signal_coverage.py --self-test

# The shell-policy test-safety guard, which now audits the fixture member.
python3 scripts/check_shell_policy_test_safety.py
python3 scripts/check_shell_policy_test_safety.py --self-test

# The Rust-aware test inventory (ground truth).
python3 scripts/rust_test_scan.py --validate

# The manifest guard, after the ownership manifest changed.
python3 scripts/check_test_ownership.py
python3 scripts/check_test_ownership.py --self-test

# Formatting and the workspace check.
cargo fmt
scripts/verify.sh quick
```

### Results

- The fixture member: **11 passed, 0 failed** after the change
  (`cargo test --test security_permission -- destructive_delete_classification`).
  The scoped-row set carries one row beyond the plan's (`rm -rf ~/docs`) precisely so that
  every test which pins the *classification* fails on the baseline; see §4.1.- The affected library test modules: **128 passed, 0 failed**
  (`cargo test -p codegg --lib -- destructive tool::bash::policy shell::policy`); the
  human-shell policy module alone: **29 passed, 0 failed**. The pre-existing bash-policy
  assertions that pin the advisory contract (`echo "$(rm -rf /)"`, `echo \`rm -rf /\``,
  `cat > f << EOF\n$(rm -rf /)\nEOF`, `exec rm -rf /`, `grep -rn '$(rm -rf /)' .`) all
  still hold.
- The new guard: `check_destructive_risk_signal_coverage: ok`; `--self-test` reports
  "self-test ok (coverage + scoped-path + effective-spelling regressions)".
- `check_shell_policy_test_safety.py`: `ok`, and its `--self-test` passes with the fixture
  member in the audited list.
- `check_test_assertion_quality.py` and `check_test_duplication.py`: "all checks pass".
- `rust_test_scan.py --validate`: clean (tests 5154/5154, src 7553 attributes = 7552
  functions + 1 template, crates 3774/3774; residual 0).
- `check_test_ownership.py`: "the test-ownership manifest matches the tree and the
  workspace"; its `--self-test` passes.
- `check_test_target_references.py`: "all checks pass" after the *Verified counts* rows in
  `architecture/overview.md` were re-measured with `python3 scripts/measure_overview_counts.py`
  (integration test files 327 → **328**, guard scripts 80 → **81**, CI guard scripts
  40 → **41**).
- `cargo fmt --check`: clean.
- `scripts/verify.sh quick`: **"Quick verification passed."** (exit 0).

### 4.1 The baseline demonstration (the fixture set is not vacuous)

The baseline has no verdict API, so the fixture cannot compile against it directly. The
demonstration therefore restored the baseline production files
(`git checkout HEAD~2 -- src/tool/destructive.rs src/tool/bash/policy.rs src/shell/policy.rs`)
and appended a **temporary, uncommitted shim** to the baseline `src/tool/destructive.rs` that
exposes the verdict API by reusing the baseline's **own** `DESTRUCTIVE_BASH_REGEXES`
verbatim. The classification behaviour under test was therefore the baseline's, unmodified;
the shim was discarded afterwards (`git checkout HEAD -- <production files>`).

Both commits were already on the branch before the revert, and the diff was also saved to
`/tmp/c002-baseline-revert.patch`, so the capture is recoverable from the commit alone.

Result: **1 passed, 10 failed**. The captured output (trimmed to the assertion messages):

```
---- destructive_delete_classification::root_delete_spellings_are_classified_as_root_targets stdout ----
assertion `left == right` failed: the effective root-delete spelling "rm -rf --no-preserve-root /" must be classified as a root target
  left: NotRootDelete
 right: RootTarget(FilesystemRoot)

---- destructive_delete_classification::effective_root_delete_spellings_raise_the_advisory_risk_signal stdout ----
the effective root-delete spelling "rm -rf --no-preserve-root /" must raise the advisory risk signal

---- destructive_delete_classification::scoped_removals_are_not_classified_as_root_targets stdout ----
assertion `left == right` failed: the scoped removal "rm -rf ~/docs" must not be classified as a root target
  left: RootTarget(HomeDirectory)
 right: NotRootDelete

---- destructive_delete_classification::scoped_removals_do_not_raise_the_advisory_risk_signal stdout ----
the scoped removal "rm -rf /home/user/project/cache" must not raise the advisory risk signal

---- destructive_delete_classification::human_shell_does_not_block_a_scoped_absolute_path stdout ----
the scoped removal "rm -rf /tmp/build" must not be hard-blocked as a root delete, got: Block { reason: "rm -rf / is catastrophic" }

---- destructive_delete_classification::human_shell_blocks_the_filesystem_root_spellings stdout ----
the filesystem-root spelling "rm -rf --no-preserve-root /" must still be refused by the human-shell block, got: Allow

---- destructive_delete_classification::human_shell_root_delete_reason_makes_no_guarantee_claim stdout ----
expected the human-shell block to refuse the root delete

---- destructive_delete_classification::a_variable_target_is_reported_as_unresolved stdout ----
assertion `left == right` failed
  left: NotRootDelete
 right: UnresolvedTarget

---- destructive_delete_classification::a_benign_prefix_does_not_suppress_the_root_delete_signal stdout ----
assertion `left == right` failed
  left: NotRootDelete
 right: RootTarget(FilesystemRoot)

---- destructive_delete_classification::an_effective_root_delete_reaches_the_approval_path stdout ----
the effective root-delete spelling must escalate to the approval path, got: Allow

test result: FAILED. 1 passed; 10 failed; 0 ignored; 0 measured; 325 filtered out
```

The two rows the plan names — `rm -rf --no-preserve-root /` (and its option-after-target
sibling, which fails the same assertion) and the scoped absolute path
`rm -rf /home/user/project/cache` / `rm -rf /tmp/build` — all fail on the baseline.

**The one test that is green on the baseline is a migration invariant, not a defect pin:**
`a_configured_deny_still_denies_before_the_risk_signal` asserts that a user-configured
`bash_deny_patterns` entry still denies. That property is true before and after the change —
it is the plan's required migration test ("a test that a user-configured deny still matches
a configured pattern after the change, proving the user's rules were not touched"), and a
test that only held *after* the change would assert that the user's rules were broken at the
baseline, which is not the defect this milestone repairs. Every test that pins the
classification itself fails on the baseline.

### 4.2 Platform statement

**macOS (darwin 25.6.0, arm64) is the only platform exercised.** Every command above ran on
macOS. **Linux and Windows were not exercised** — this is a named condition, not a green
result. The change is platform-independent (pure string classification; no syscall, no
filesystem, no OS-specific code), and the fixture member is not `#[cfg]`-gated, so it
compiles and runs identically on every platform; but no Linux or Windows run was performed
from this worktree.

`scripts/verify.sh full` (the workspace sweep and `cargo clippy --workspace --all-targets
--locked -- -D warnings`) was **not run**: the shared-machine discipline for this workstream
forbids the workspace sweep, `cargo test --workspace` and `verify.sh full` while three
sibling milestones run in parallel. `scripts/verify.sh scoped HEAD~1` was **not run** either,
for the same reason: because the root package depends on all eleven other crates, the
scoped selection for a root-package source change widens to the root package's full test
surface, which is the sweep the discipline forbids. Its two halves were run separately and
narrowly instead: `scripts/verify.sh quick` (every guard, the generated-asset checks,
rustfmt and the workspace `cargo check`) and the affected test targets only
(`security_permission::destructive_delete_classification` and the `tool::destructive`,
`tool::bash::policy` and `shell::policy` library modules). The plan records that the macOS
lint set is already red at the baseline for reasons C005 owns; this milestone added no lint
finding, and the macOS lint set was not re-measured here. That is a named condition, not a
green result.

## 5. Invariant review

| Invariant (from the plan) | Evidence | Holds |
|---|---|---|
| The signal stays advisory; it is not a guarantee and not a terminal rule | The classifier only returns a verdict; `destructive_match` still feeds an `Ask`; `default_blocked_commands()` is unchanged and empty; the human-shell refusal now says it is not a security boundary | yes |
| No authority ceiling, ApprovalRouter ordering or auto-approval rule changed | The diff touches only `src/tool/destructive.rs`, `src/tool/bash/policy.rs`, `src/shell/policy.rs` (classification + message) — `src/permission/mod.rs` is untouched | yes |
| No user-configured `bash_deny_patterns` / `bash_allow_patterns` / `bash_ask_patterns`, no explicit deny receipt, no stored decision changed | The diff contains no change under `src/config/`, `src/permission/`'s storage, or the receipt types; `…::a_configured_deny_still_denies_before_the_risk_signal` proves the configured deny still matches | yes |
| `default_blocked_commands()` stays empty | The function is untouched; its pre-existing lib test `default_blocked_commands_is_empty_by_design` still passes | yes |
| The classifier never pretends to resolve a variable target | `RootDeleteVerdict::UnresolvedTarget`; `…::a_variable_target_is_reported_as_unresolved` | yes |
| The fixtures assert the classification and never spawn | The fixture member is in `check_shell_policy_test_safety.py`'s audited list and contains no dispatch call | yes |
| The historical `tests/tool_execution.rs` hard-denial cases are not reintroduced | No such file or case was added; the existing `CATASTROPHIC_ADVISORY_FIXTURES` in `tests/tool_programs/tool_execution.inc` is untouched | yes |
| No second, differently-bypassed command parser | One recogniser, three call sites; the guard's `single-recogniser` rule enforces it | yes |
| The recogniser stays cheap and adds no cache, lock or new long-running work | A single regex scan plus a linear word scan per command; no allocation beyond the command's words, no cache, no lock | yes |

## 6. Failure and recovery review

- **Failure.** A spelling the classifier does not recognise is reported as *not recognised*,
  never as "safe" and never as "root". A classification *error* therefore never expands
  authority: ADR-0045 decision 2 keeps the authority independent of the classifier, and an
  unrecognised spelling reaches the normal approval path, which is decided by the
  permission layer, the approval mode and the sandbox — not by the text.
  **Interpretation recorded:** plan §8's "a classification failure must be an escalation"
  is read as *the classifier must never report an unrecognised spelling as proven-safe*,
  which the three-way verdict guarantees. It is **not** read as *every unrecognised
  recursive force removal must escalate*, because that would newly escalate the ordinary
  script shape `rm -rf "$VAR"` — a new risk signal for a pattern that is not a root
  delete, outside this milestone's scope and against its "no new authority" constraint.
- **Cancellation, restart, contention.** Unchanged. The classifier is a pure function on
  the request path; no durable state, no cache, no lock held across the escalation.
- **Malformed input.** A command that is not a shell command, an empty command, or a
  command with no `rm` word is classified `NotRootDelete` without error.

## 7. Migration and compatibility review

- **Schema / protocol / storage:** none. No persisted enum is added, so no reader/writer
  versioning is needed.
- **Configuration:** unchanged. No config key was added, removed or reinterpreted.
- **Behavioural change (operator-visible).** In the human `!cmd` path, a scoped
  absolute-path delete is no longer hard-blocked as a root delete. The filesystem-root
  removal is still refused there, with a message that now says it is a best-effort
  command-text rule and not a security boundary. ADR-0045's compatibility section requires
  a documented migration, targeted regression fixtures and an operator-facing note: the
  fixtures are §3.4, the note is
  `docs/security-semantics.md` § "The destructive-command risk signal (operator note)".
- **Rollback.** The change is a pure function of three source files plus one test member
  and one guard; reverting the commit restores the baseline exactly. No data migration
  means no rollback hazard.
- **Legacy paths.** `destructive_bash_patterns()` still returns the pattern table; the
  three root-delete entries are no longer in it (they are not regexes any more), and its
  doc comment says so. The function has no in-tree consumer.

## 8. Security review

- **Authorization.** Unchanged. The repair changes what the advisory signal *says*, not
  who may run a command. The escalation of the effective spellings now happens at the
  permission layer, which is where the authority decision already lived.
- **Truthfulness, not a new boundary.** The security effect of this milestone is that the
  advisory signal stops misreporting. It does not and cannot make command syntax a
  security boundary: the recogniser reads one command's text, cannot see a script, a
  wrapper, another tool, a variable that expands to the root, or a different spelling.
  Under `full_host`, and under `yolo` (which auto-approves escalations), nothing here
  stops a root removal the text does not name. The operator note says so plainly.
- **Path validation / privilege boundaries.** Untouched.
- **Test safety.** The fixture member holds inert command text and asserts verdicts; it is
  audited by `scripts/check_shell_policy_test_safety.py`, which rejects a file that
  combines a destructive literal with a dispatch call.
- **Denial-of-service bounds.** The recogniser's work is linear in the command's length
  (`MAX_COMMAND_LENGTH` is 100 000) with no backtracking search: one regex scan for the
  `rm` word and a single word pass per invocation.

## 9. Documentation and operations

- `architecture/tool.md` — § "Destructive-command classification (advisory)": the advisory
  nature, the single recogniser, the verdict table, and the three load-bearing properties.
- `docs/security-semantics.md` — § "The destructive-command risk signal (operator note)":
  the operator-facing note, including the human-`!cmd` behaviour change, the "no configured
  rule changed" statement, and the "command syntax is not a security boundary" statement
  with the FullHost/YOLO caveat.
- `docs/test-ownership.toml` — the `security_permission` group now claims
  `src/tool/destructive.rs`, `src/tool/bash/policy.rs` and `src/shell/policy.rs`, so a
  change to any of them selects the family that owns the fixtures.
- Static guards: `scripts/check_destructive_risk_signal_coverage.py` (new, with
  `--self-test`); `scripts/check_shell_policy_test_safety.py` (the fixture member added to
  its audited list).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `crates/eggsentry/src/command.rs::CRITICAL_RM_RE` is a **separate** root-delete recogniser in another crate. It is root-anchored, so it does **not** have the scoped-absolute-path false positive, but it under-reports the two effective spellings (`rm -rf --no-preserve-root /` is not labelled `Critical`). It feeds the `security` tool's severity label and the agent's advisory security hint (`src/agent/tool_batch.rs`). | The authoritative escalation is fixed — the permission layer now escalates the effective spellings — so no dangerous spelling reaches execution unescalated. The residual is a *lower severity label* on one advisory surface, and the `security` tool's own output for the effective spellings. | Out of this milestone's named scope (the plan scopes three files in the root package). The broader ADR-0045 migration pass, or a follow-up milestone, must make the eggsentry classifier route through the same canonical recogniser. **This is the one place where the built-in signal still under-reports the effective spellings.** |
| low | The recogniser does not resolve a target that appears after another operand (`rm -rf /tmp/build --no-preserve-root /`), because the target position it matches is the first non-option word of the invocation. | Such a spelling is not classified as a root delete. | A deliberate limitation of the "first target position" rule; recorded rather than silently widened. Widening it would require recognising an arbitrary operand list, which is the parser shape ADR-0045 rejected. |
| low | `sh -c "rm -rf /"` is not classified: the `rm` word sits inside a quoted string, and the recogniser deliberately treats a quoted word as data. | A quoted wrapper is not recognised. | A deliberate trade-off: the alternative (treating any quoted `rm` as a command) produces false positives on `echo "rm -rf /"`. The baseline behaved the same way. |
| low | `scripts/verify.sh full` (workspace sweep + clippy) and `scripts/verify.sh scoped HEAD~1` were not performed; the Linux and Windows runs were not performed. | The macOS lint set and the cross-platform behaviour are unmeasured from this worktree. | CI's bounded `verify` job covers the sweep on Linux; the macOS lint asymmetry is C005's. Named as a condition, not claimed as green. `quick` plus the narrow affected test targets were run instead. |

## 11. Roadmap disposition

**Milestone closed.** The plan's §13 acceptance criteria are met (see §2), with the
platform and full-sweep limits of §4.2 named as conditions rather than results. No
corrective implementation plan is required for C002 itself. C005's dependency on C002 is
discharged; C005 remains blocked on C001, C003 and C004.

## 12. Registry updates

- `plans/registry.md` — the "Repository audit corrective" workstream row: the C002
  milestone row moves from **ready for handoff** to **closed**, with the closure record
  path. The "Blocked work" section is audited: C005's dependency line now records that
  C002 is closed, and its remaining blockers (C001, C003, C004) are unchanged.
- `plans/subsystems/repository-audit-corrective-roadmap.md` — the C002 milestone status
  moves to closed.
