# Repository Audit Corrective Milestone C002 — Destructive-Delete Classification Coverage

Status: ready for handoff

Repository baseline: `d5be7acd05dc999888430b7ad23420d554359fec` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Source roadmap: `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c002--destructive-delete-classification-coverage`

Long-term requirements: `plans/000-long-term-specification.md`, `plans/002-long-term-roadmap.md`, `architecture/tool.md`, `architecture/authorization.md`

Applicable ADRs:

- **accepted** `plans/adrs/ADR-0045-command-syntax-and-execution-authority.md` — governing. Command syntax is **advisory**; authority is enforced independently. Built-in syntax-only terminal blockers are to be reclassified as risk/approval signals, and **not** replaced by "an equally bypassable command parser elsewhere".
- accepted ADR-0004 — the separation of approval from obtained sandbox enforcement.
- ADR-0028 is a **historical alias** archived by the identity migration; the shell-policy line under it is closed (`plans/subsystems/shell-command-policy-adr0028-post-closure-corrective-addendum.md`) and is **not reopened**.

Primary class: **invariant** — the advisory risk signal must not under-report a catastrophic delete. Authority is explicitly out of scope.

## 1. Objective

The advisory risk classification of a destructive command is truthful in both directions: it reports the spellings that actually execute as catastrophic, and it stops reporting a legitimate scoped delete as catastrophic. The signal remains advisory. No authority boundary moves.

## 2. Why this milestone is ready

It has no hard or interface dependency, and it owns one defect class in one ownership boundary: the destructive-command risk classifier and the human-shell built-in block pattern.

It is registered as ready because accepted ADR-0045 already decides the shape of the fix — the signal is advisory, the built-in terminal blockers are the target — so no new architectural decision is required to begin.

## 3. Current implementation evidence

### The classifier under-reports the spellings that execute

`src/tool/destructive.rs` holds `DESTRUCTIVE_BASH_PATTERNS`. The root-delete entry is:

```rust
(
    "rm -rf /",
    r"rm\s+(-[a-zA-Z]*[rR][fF]|-[a-zA-Z]*f[a-zA-Z]*r|-rf|-fr)\s+/\s*$",
),
```

Two properties of that regex produce the gap:

- `\s+/\s*$` requires the `/` to follow the flag group **immediately** and to be the **last** token. `rm -rf --no-preserve-root /` has `--no-preserve-root` between the flag and the `/`, and `rm -rf / --no-preserve-root` has the option after the `/`. Neither matches. Both are the spellings GNU coreutils actually executes as a root delete — the `--no-preserve-root` option is what makes `rm -rf /` effective, since coreutils otherwise refuses it.
- The `rm -rf /*` entry and the `rm -rf ~ or $HOME` entry have the same shape and the same limitation.

`src/tool/bash/policy.rs`'s `contains_catastrophic_delete` has the same gap by a different mechanism. It splits on the **literal** prefixes `["rm -rf /", "rm -fr /", "rm -rf ~", "rm -rf $HOME"]`:

```rust
for prefix in ["rm -rf /", "rm -fr /", "rm -rf ~", "rm -rf $HOME"] {
    let Some((_, suffix)) = command.split_once(prefix) else {
        continue;
    };
```

`split_once("rm -rf /")` on `"rm -rf --no-preserve-root /"` finds no occurrence, so the function returns `false` — the command is reported as **not catastrophic**. The literal form `rm -rf /`, which coreutils refuses, is reported as catastrophic.

The consequence is not a wrong label on a harmless command. It is that the **approval path is not escalated** for the spelling that would destroy the filesystem. Under ADR-0045 a classifier's failure to recognise an action cannot expand authority, so the safety boundary is not broken — but the signal the ApprovalRouter and the automatic reviewer rely on is silently wrong, and ADR-0045 decision 2 permits classifiers precisely so they can escalate.

### The human-shell path over-blocks a legitimate scoped delete

`src/shell/policy.rs` holds `BLOCK_RM_RF_ROOT`:

```rust
static BLOCK_RM_RF_ROOT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"rm\s+-[a-zA-Z]*r\s*-?[a-zA-Z]*f\s*-?[a-zA-Z]*\s+/").unwrap()
});
```

The pattern ends with a bare `/` and no anchor, so it matches the `/` of **any** absolute path. `rm -rf /tmp/build` matches, and is hard-blocked with a catastrophic-delete reason. That is a legitimate, scoped, in-workspace operation, and ADR-0045's decision drivers name "obstruction loops, workaround pressure and false security claims" as the problems to avoid.

The same argument does **not** apply to a **user-configured** deny. ADR-0045 decision 3 and its compatibility section require that explicit operator and admin text filters be preserved, with their best-effort-text-match semantics labelled. The built-in default and the user's own rule are different things, and this milestone must not blur them.

### What the policy already decided

ADR-0045, accepted, is unambiguous about the shape of the correct outcome:

- decision 2 — syntax is advisory; classifiers may annotate risk, explain proposed actions, or escalate to the ApprovalRouter; their match alone is not evidence that the effect is impossible under another spelling;
- decision 3 — hard rejection is reserved for valid enforceable cases, and an explicit operator-selected command-text deny remains a **local UX/workflow prohibition** with conspicuous `best_effort_text_match` semantics, not a statement that equivalent behaviour is forbidden; "Preserve explicit denies rather than silently deleting user rules";
- decision 4 — remove built-in syntax-only terminal command blockers and reclassify them as risk/approval signals after a documented compatibility pass; "do not substitute an equally bypassable command parser elsewhere";
- decision 10 — staged migration: map the syntax-only denials, convert built-in default catastrophic denials to advisory escalation with explicit FullHost caveats, preserve user-configured text filters, and run Linux/macOS/Windows non-regressions.

The shell-policy line closed under ADR-0045 with `src/tool/bash/policy.rs::default_blocked_commands()` returning an **empty** set and `find_risk_pattern` / `find_ask_pattern` classifying risk as advisory. The remaining built-in catastrophic patterns are the residue of that migration.

## 4. Invariants that must not regress

- **Authority is unchanged.** No request gains or loses an allowed effect because of this milestone. ADR-0045 decision 1 holds: authority derives from the daemon-authorized principal, the project/workspace/worktree, the agent hierarchy, the approval mode, the requested ceilings and the obtained executor capabilities — never from shell spelling.
- **The risk signal stays advisory.** A pattern match annotates risk and may escalate. It must not become a terminal rejection, and it must not be presented to the user as a guarantee.
- **User-configured denies are preserved.** `bash_deny_patterns`, `bash_allow_patterns`, `bash_ask_patterns` and explicit deny receipts keep their current behaviour and their best-effort-text-match labelling.
- **The approval ordering is preserved.** Higher-priority explicit operator grants and denies still win over a risk hint, and a risk hint is still evaluated before an auto-approval rule may allow a command on the strength of a benign-looking prefix.
- **No equivalent parser is introduced.** The fix must not add a second command parser that is bypassable in a different way. Where a spelling cannot be recognised, the honest outcome is "not recognised", not "recognised by a new rule".
- **FullHost semantics are not weakened.** For an unsandboxed FullHost session, approval cannot provide technical protection, and that caveat continues to be stated.
- **Linux, macOS and Windows non-regressions pass.**

## 5. Scope

**In scope**

- The destructive-command risk patterns in `src/tool/destructive.rs` and the catastrophic-delete detection in `src/tool/bash/policy.rs`, so that the effective root-delete spellings are reported as catastrophic.
- The built-in human-shell block pattern in `src/shell/policy.rs`, so that a scoped absolute path is not hard-blocked as a root delete.
- A fixture set of destructive and benign spellings, asserted against the classification's verdict.
- A guard that prevents the classifier from regressing to a shape that misses the effective spellings.

**Out of scope**

- Any change to what the ApprovalRouter does with a risk signal, to the auto-approval rules, or to any authority ceiling.
- Any change to `default_blocked_commands()`, which is intentionally empty.
- Any change to user-configured `bash_deny_patterns` / `bash_allow_patterns` / `bash_ask_patterns`, or to explicit deny receipts.
- The remaining ADR-0045 migration of other built-in syntax-only terminal blockers. This milestone repairs one defect class; the broader migration is a separate, larger pass.
- Replacing the classifier with a shell parser, an AST, or an allowlist of safe binaries.
- The "the classifier is not a security guarantee" caveat, which ADR-0045 already establishes and which this milestone must not weaken.

## 6. Required production changes

### Core

- The risk classification must recognise the effective root-delete spellings. The implementer must decide the shape. Two options are legitimate and both are acceptable:
  - recognise the **option form** explicitly — the `--no-preserve-root` option, and the option before or after the target — rather than trying to enumerate every flag permutation; or
  - recognise the **intent** — a recursive force removal whose target resolves to the filesystem root — by matching the target position rather than the flag position.
  The second is preferable if it can be done without a parser, because it does not enumerate spellings. The first is acceptable if it is a bounded extension of the existing shape.
- What is **not** acceptable is leaving the pattern as-is and adding a second, differently-bypassed matcher beside it.

### Storage

None.

### Protocol

None. The risk signal's surface — the escalation to the ApprovalRouter and the automatic reviewer — is unchanged.

### Runtime

None.

### Frontend

- The human-shell block message for a scoped absolute path must change from a catastrophic-delete refusal to the path that ADR-0045's decision 3 describes: either the command proceeds under the normal authority rules, or it is refused for a **real** enforceable reason (a denied path, a missing containment ceiling, an explicit operator rule).
- The reason text shown to the user must not claim a guarantee the classifier cannot provide.

### Security

- This is the security-relevant milestone, and its security effect is **truthfulness of an advisory signal**, not a new denial. The closure must state the before-and-after classification for every fixture, and must state plainly that the change does not and cannot make command syntax a security boundary.

### Docs

- `architecture/tool.md` must record the rule that the destructive-command classification is an advisory risk signal whose job is to escalate, and that the effective root-delete spellings are recognised.
- Any user-facing note about the human-shell block must move from "catastrophic delete blocked" to the advisory framing, per ADR-0045's compatibility section requirement for operator-facing notes.

## 7. Ordered work packages

### WP1 — Map the built-in syntax-only denials that remain

Intent: satisfy ADR-0045 decision 10's first step, and establish exactly which built-in patterns are in scope for this milestone and which belong to the broader migration.

Required changes: inventory the built-in destructive patterns in `src/tool/destructive.rs`, the catastrophic-delete detection in `src/tool/bash/policy.rs`, the built-in human-shell block patterns in `src/shell/policy.rs`, and any remaining heuristic critical-command denials. For each, record: what it matches, what it is used for (advisory risk / advisory ask / terminal deny), and whether it is in scope here.

Acceptance evidence: the inventory is recorded in the plan's own notes or the closure record, and the in-scope set is explicit before any code changes.

### WP2 — Repair the advisory classifier

Intent: make the risk signal report the effective root-delete spellings.

Required changes: as §6 describes. The change must be expressed so that a reviewer can see which spellings are now recognised and which are deliberately not.

Acceptance evidence: the fixture set from WP3 asserts the before-and-after verdict for each spelling.

### WP3 — Build the fixture set

Intent: pin the classification's behaviour in both directions.

Required changes: a fixture set containing, at minimum:

| Spelling | Expected classification | Why |
|---|---|---|
| `rm -rf /` | catastrophic | the form coreutils refuses; retained as an explicit signal |
| `rm -rf --no-preserve-root /` | catastrophic | the form coreutils executes |
| `rm -rf / --no-preserve-root` | catastrophic | option after the target |
| `rm -fr /` | catastrophic | flag order variant |
| `rm -rf /*` | catastrophic | glob form |
| `rm -rf ~` | catastrophic | home |
| `rm -rf $HOME` | catastrophic | home |
| `rm -rf /tmp/build` | **not** catastrophic | a legitimate scoped delete |
| `rm -rf ./build` | **not** catastrophic | relative path |
| `rm -rf /home/user/project/cache` | **not** catastrophic | scoped absolute path |
| `rm -rf "$TARGET"` | not recognised | a variable target cannot be resolved; not recognised is the honest verdict |

The last row is important: the classifier must not pretend to resolve a variable.

Acceptance evidence: every row is asserted; the set is committed as a test member, not as prose.

### WP4 — Repair the human-shell built-in block

Intent: stop obstructing a legitimate scoped delete.

Required changes: narrow the built-in pattern so that it matches the filesystem-root target and not any absolute path, or replace it with the same recognition used in WP2. The change must preserve a user-configured deny untouched.

Acceptance evidence: `rm -rf /tmp/build` is not hard-blocked as a root delete; `rm -rf --no-preserve-root /` still is; a user-configured deny still matches.

### WP5 — Add the guard

Intent: prevent the classifier from regressing to a shape that misses the effective spellings.

Required changes: a guard that fails when the destructive-classification fixture set is not covered by a test member, and that fails when the human-shell built-in pattern matches a scoped absolute path. It must use `scripts/rust_test_scan.py` for any Rust parsing, provide `--self-test` that injects a regression and proves the guard rejects it, and be wired into `scripts/verify.sh quick` and CI.

Acceptance evidence: the guard runs in `verify.sh quick`; its `--self-test` passes; it fails on an injected regression.

### WP6 — Record the operator-facing note

Intent: satisfy ADR-0045's compatibility requirement.

Required changes: an operator-facing note that the built-in destructive-command classification is advisory, that the risk signal now recognises the effective root-delete spellings, and that no user-configured rule was changed.

Acceptance evidence: the note exists in the user-facing documentation and is linked from the closure record.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** A classification failure must be an escalation, not a silent pass. If the classifier cannot recognise a spelling, the honest outcome is that it is not recognised — and the risk signal must not be reported to the user as a guarantee.
- **Cancellation.** No new long-running work is introduced. The ApprovalRouter's existing cancellation behaviour is unchanged.
- **Restart.** No durable state changes, so restart is unaffected.
- **Contention.** The classifier runs on the request path and must stay cheap. The implementer must not introduce a cache whose invalidation can race, and must not introduce a lock held across the escalation.

## 9. Compatibility and migration

- Stored user and admin `bash_deny_patterns`, `bash_allow_patterns`, `bash_ask_patterns`, tool toggles and explicit deny receipts are **unchanged**, with their best-effort-text-match labelling intact.
- The built-in default set is unchanged in size: `default_blocked_commands()` remains empty.
- The **behavioural change is operator-visible**: a scoped absolute-path delete in the human `!cmd` path is no longer hard-blocked. ADR-0045's compatibility section requires "a documented behavior migration, targeted regression fixtures and operator-facing notes for FullHost/YOLO" for changes to built-in default catastrophic denials. WP3's fixtures are the targeted regression fixtures and WP6's note is the operator-facing note.
- No persisted enum is added, so no reader/writer versioning is needed.

## 10. Required tests

**Focused**

- The WP3 fixture set, asserted against the classification's verdict rather than against a pattern.
- A test that the human-shell path no longer hard-blocks `rm -rf /tmp/build`, and still hard-blocks the effective root-delete spellings.
- A test that a user-configured deny still matches a configured pattern after the change, proving the user's rules were not touched.

**Integration**

- A test that the effective root-delete spelling reaches the **approval** path and is escalated, rather than being silently admitted. This is the behaviour that actually matters, and it is the one that distinguishes a truthful classifier from a merely broader pattern.

**Security**

- The fixture set is the security-relevant evidence. It must include a case that proves the classifier does **not** claim to resolve a variable target, and a case that proves a benign prefix does not suppress the risk signal.

**Migration**

- A test that the built-in default block set is still empty, and that no user rule was removed.

**Placement.** The `security_permission` family is the right home — it already carries the approval, sandbox-containment and policy-wiring members. A new member is `tests/security_permission/<name>.inc`, bound in `tests/security_permission/mod.rs` with `#[path = "<name>.inc"] pub mod <name>;`. The family target is already declared in the root `Cargo.toml`; a new family would need that declaration and is not expected here.

**Vacuity.** The fixture set must be shown to fail against the baseline implementation — at minimum the two effective-spelling rows and the scoped-absolute-path row. The closure records that result.

## 11. Required verification commands

```bash
# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# The affected family.
cargo test --test security_permission

# The new guard, including its non-vacuity proof.
python3 scripts/check_destructive_risk_signal_coverage.py --self-test

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Full workspace, before the closure record is written.
bash scripts/verify.sh full
```

`scripts/verify.sh full` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`. On macOS this currently fails for reasons unrelated to this milestone (C005 owns that asymmetry); the implementer must record the failure set before and after and state that the change added and removed no lint finding. Linux must stay green.

**Non-regression on all three platforms** is an ADR-0045 decision 10 requirement. The implementer must state which platforms were actually exercised and which were not; a platform that was not exercised is a named condition, not a green result.

## 12. Documentation updates

- `architecture/tool.md`: the advisory nature of the destructive-command classification, and the recognised spellings.
- User-facing documentation: the operator note required by ADR-0045's compatibility section, covering the change in the human `!cmd` path's behaviour and the FullHost caveat.
- `docs/`: no other change.

## 13. Acceptance criteria

Externally observable:

1. `rm -rf --no-preserve-root /` and `rm -rf / --no-preserve-root` are classified as catastrophic and reach the approval path.
2. `rm -rf /tmp/build` is no longer hard-blocked as a root delete by the built-in human-shell pattern, and is not classified as catastrophic.
3. `rm -rf /` remains classified as catastrophic.
4. A user-configured deny still matches, and the stored user rule set is unchanged.
5. `default_blocked_commands()` is still empty.
6. The operator note states plainly that command syntax is not a security boundary.
7. The new guard runs in `verify.sh quick` and its `--self-test` passes.

## 14. Stop conditions

The implementer must report rather than improvise if any of the following holds.

- Making the signal truthful would require making it **terminal**. Accepted ADR-0045 reserves hard rejection for enforceable cases and forbids substituting a bypassable parser for a removed one. Record an ADR instead.
- The only way to recognise the effective spellings is a new command parser, an AST, or an allowlist of safe binaries. Stop: that is the route ADR-0045 rejected.
- The fix would require changing a user-configured deny, an explicit deny receipt, the ApprovalRouter's ordering, or any authority ceiling. Stop: none of those are in scope, and ADR-0045 decision 3 and decision 5 forbid it.
- The `rm -rf /` form must be distinguished from the effective form by a rule that would misclassify a scoped path. Stop and record the conflict.
- A fixture cannot be asserted without spawning a real command. Stop: the fixtures must assert the **classification**, never execute the command.

## 15. Closure evidence required

- The WP3 fixture results, with the before-and-after verdict for every row, and the demonstration that the fixture set fails against the baseline.
- The WP1 inventory, with the in-scope and out-of-scope built-in patterns named.
- The operator note, and its location.
- A statement that no user-configured rule, stored receipt, approval ordering or authority ceiling changed, verified against the diff.
- The guard's `--self-test` output and its presence in `verify.sh quick`.
- The `verify.sh scoped` and `verify.sh full` results, labelled local versus CI, with the macOS lint set stated before and after.
- The platforms actually exercised, with any platform not exercised named as a condition.
- The registry update, and the unblock audit of the registry's Blocked work section.

## 16. Handoff notes

- **Read accepted ADR-0045 before writing any code.** It decides the shape of this milestone. The intuitive fix — "make the regex match the dangerous spellings" — is incomplete on its own, because ADR-0045 requires the signal to stay advisory and forbids replacing a bypassable matcher with another one.
- The two halves of this milestone pull in opposite directions and both are wanted: the classifier must catch **more** of the genuinely dangerous spellings, and the built-in human-shell block must catch **fewer** of the harmless ones. A change that only does one of the two does not close it.
- `default_blocked_commands()` is intentionally empty. Do not "restore" it.
- The historical `tests/tool_execution.rs` cases that asserted a hard denial of catastrophic command text were removed as unsafe to execute. Do not reintroduce them; the fixture set asserts the classification, and must not spawn.
- This milestone closes no other milestone. C005 depends on it, because C005 re-enables the verification surface that should have caught this class of misreport.
