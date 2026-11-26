# Sandbox Containment Audit Corrective C005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/sandbox-containment-audit-corrective/005-sandbox-guard-and-qualification-lane-integrity.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c005--sandbox-guard-and-qualification-lane-integrity`

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8` (`feat-auto-20261109-f8abad0d` HEAD) on a macOS 26.6.2 host (Darwin 25.6.0, arm64).

Implementation commits or pull requests:

- **None. The implementation is uncommitted working-tree work**, shared with sibling agents
  (C001, C002, C004, C006) on `feat-auto-20261109-f8abad0d`. No commit, stage, or push
  identifies this milestone.

## 1. Executive finding

The guard that protects the silent containment defects now runs, can prove it is not vacuous,
and is invoked by both lanes. `scripts/check_sandbox_policy_wiring.py` was invoked by nothing
and exited 1 on the baseline tree for a **stale pattern**; the pattern is corrected to the
production form (`sandbox_config_for_durable_payload`), a `--self-test` with **nine** injected
defect classes exists and is asserted against `EXPECTED_INJECTIONS = 9`, and both sandbox
guards run with their self-tests in `scripts/verify.sh`'s `run_guards()` (`:128-146`) and in
CI (`.github/workflows/ci.yml:105-120`).

Nothing was silently deleted to reach green: `errors.append` sites went **44 → 46**, so the
guard enforces more than it did, not less.

The macOS qualification lane now names files that exist (`tests/security_permission/**`
replacing the nonexistent `tests/sandbox_containment.rs` in both the `push` and
`pull_request` filters), runs the `sandbox_policy_wiring` and `host_admission_manifest`
members it previously omitted, and sets `CODEGG_SANDBOX_QUALIFICATION: required` so it cannot
go green from a skip. The lane itself was **not executed** — no hosted run was performed, and
this record says so rather than implying one.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 `check_sandbox_policy_wiring.py` passes on the baseline tree | `python3 scripts/check_sandbox_policy_wiring.py` → `sandbox-policy-wiring guard passed` (0.190 s) | Pass | Three `note:` lines name the accepted unrestricted-default sites and why |
| §13.2 Reverting the call site it protects makes it fail | Self-test case "the scheduler rebuilds containment without the durable-payload tier request" was rejected in the 9/9 run. The stale premise was confirmed by reading: HEAD's script demands the literal `sandbox_config_for_profile` in `executors.rs`, while HEAD's `executors.rs` contains **0** occurrences of it and **1** of `sandbox_config_for_durable_payload` | Pass (mechanism) | The baseline script was not executed against the baseline tree: `ROOT` is derived from the script's own location, so a faithful run needs a mirrored tree. The false-positive is established from the code it reads, not restated as an observed exit code |
| §13.3 `--self-test` exists, passes, and fails when an injected defect is removed | `--self-test` → `sandbox-policy-wiring self-test passed: 9 injected defects were each rejected`; `EXPECTED_INJECTIONS = 9` and the count is asserted in `self_test()` | Pass | Deleting one case fails the self-test by construction (`len(cases) != EXPECTED_INJECTIONS`). Not demonstrated by actually deleting a case — that requires editing `scripts/`, outside this record's write scope |
| §13.4 `check_sandbox_contract.py --self-test` is invoked by `quick` and by CI | `scripts/verify.sh:136-146` runs both guards and both self-tests; `.github/workflows/ci.yml:111-120` names the same four steps | Pass | Guard-then-self-test order in both lanes |
| §13.5 The macOS lane's path filters name files that exist and trigger on a change to the containment `.inc` | Both filter blocks now list `tests/security_permission/**` (the nonexistent path is gone); the path exists, and `tests/security_permission/sandbox_containment.inc` is modified in this very tree, so the pattern matches a real change | Pass (local filter evaluation) | **Not a hosted run** — see §10 |
| §13.6 The macOS lane sets C004's strict-mode marker and runs the sandbox members | `shared-checkout-macos-qualification.yml:123-136` sets `CODEGG_SANDBOX_QUALIFICATION: required`; a new step runs `sandbox_policy_wiring` and `host_admission_manifest`; both members exist in `tests/security_permission/mod.rs:34-35`, `:67-68` | Pass (wiring) | Both members pass locally: 23 and 7 tests |
| §13.7 `git diff --stat` shows no file under `src/**` changed | This milestone touches `scripts/check_sandbox_policy_wiring.py`, `scripts/verify.sh`, `.github/workflows/ci.yml`, `.github/workflows/shared-checkout-macos-qualification.yml` | Pass for this milestone's own contribution | The tree-wide `src/` diff belongs to C001/C002/C006 and unrelated TUI work sharing the uncommitted tree |

## 3. Production implementation evidence

**No production change**, as the plan requires. The change set is four files: one guard script,
the quick lane, and two workflows.

**Guard correction.** The `executors.rs` expectation now requires
`sandbox_config_for_durable_payload(` and explicitly rejects a regression to
`sandbox_config_for_profile(` with a message naming the M004 tier reason
(`scripts/check_sandbox_policy_wiring.py:385-406`). The comment above it records the accepted
production form and why the durable form is the stronger one, so the next refactor does not
resurrect the stale pattern.

**Self-test.** `self_test()` (from line 654) mutates a file per case, runs the same checks the
guard runs, and requires each to be rejected. The nine reported cases cover: factory
`ToolRegistryOptions` without the profile; `with_options` not mapping the profile into
`BashTool` containment; the child registry handed the parent profile instead of the narrowed
one; the managed process owner growing a degraded uncontained launch; bash continuing instead
of refusing an unavailable backend; the Seatbelt backend dropping out of the registry; the
Seatbelt profile emitting credential denies before the writable re-allows; a hardlinked denied
file launched instead of refused; the scheduler rebuilding containment without the
durable-payload tier request.

**Check count.** `errors.append` occurrences: 44 at HEAD, 46 now. Two were added, none removed.

**Lane.** Path filters repaired in both triggers, the two omitted members added as a new step,
and the strict-mode marker set on the containment step's environment.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_sandbox_contract.py            # 0.103 s
python3 scripts/check_sandbox_contract.py --self-test # 0.033 s
python3 scripts/check_sandbox_policy_wiring.py        # 0.190 s
python3 scripts/check_sandbox_policy_wiring.py --self-test # 0.863 s
bash -n scripts/verify.sh
python3 -c "import yaml; [yaml.safe_load(open(p)) for p in ['.github/workflows/ci.yml','.github/workflows/shared-checkout-macos-qualification.yml']]"
cargo test --locked --test security_permission -- sandbox_policy_wiring --test-threads=1
cargo test --locked --test security_permission -- host_admission_manifest --test-threads=1
scripts/verify.sh quick
```

### Results

- `check_sandbox_contract.py` — passed. `--self-test` — passed.
- `check_sandbox_policy_wiring.py` — passed. `--self-test` — passed with 9 rejections, printed
  one `ok: rejected …` line per injected defect.
- `bash -n scripts/verify.sh` — clean. Both workflow files parse as YAML.
- `sandbox_policy_wiring` member — **23 passed**, 0 failed. `host_admission_manifest` member —
  **7 passed**, 0 failed. Both are the lane's new steps, and both run on macOS.
- Measured local runtime of the four guard invocations on this host: **~1.2 s in total**
  (0.103 + 0.033 + 0.190 + 0.863). This is a local measurement, not a CI claim.
- **`scripts/verify.sh quick` — failed at its first step**, `cargo fmt --check --all`, on a
  formatting diff in `src/main.rs:5426` (`temp_measure_catalog_open`, a sibling agent's test).
  `cargo fmt --check --all` lists exactly that one file. The lane therefore never reached
  `run_guards()`, so the four sandbox invocations were not observed *inside* the lane in this
  tree; they were run directly and all four pass, and `run_guards()` names them.
- `python3 scripts/check_comment_budget.py` (part of `run_guards()`, run directly here) also
  fails in this tree on 21 entries, none under the sandbox surface — so the guard phase would
  remain red even with `src/main.rs` formatted, for reasons outside this milestone.
- **No hosted macOS run was performed.** Only the local filter evaluation and the local member
  runs above exist.

## 5. Invariant review

- **No guard is silenced to obtain green.** The check count rose 44 → 46 and the corrected
  pattern is stricter (it rejects `sandbox_config_for_profile(` explicitly rather than merely
  failing to find the durable form).
- **A wired guard carries an invoked `--self-test`.** Both do, in `run_guards()` and in CI.
- **`quick` and CI keep their subset relationship.** Both sandbox guards and both self-tests
  are in *both* lanes; nothing was added to `quick` alone. The pre-existing difference set
  (14 guards `quick` runs and CI omits) is unchanged by this milestone.
- **The macOS lane is not disabled or weakened.** It gained a step, gained the strict marker,
  and its filter now matches a directory that exists.
- **No production source change.** Confirmed by the change set in §3.

## 6. Failure and recovery review

Not applicable: the guards are stateless scripts and the lane is a single CI job. The
self-test writes into a `tempfile.TemporaryDirectory` per run (`prefix=
"codegg-sandbox-wiring-"`), so a failed case leaves no residue in the tree.

## 7. Migration and compatibility review

No production, configuration, protocol, or stored-data change. The only cost is runtime:
`quick` and CI each gained four short Python invocations, measured at ~1.2 s locally. No
environment variable, path or endpoint changed for operators.

## 8. Security review

The guard set is the security-relevant artifact here. It now mechanically rejects: a dropped
sandbox-profile thread from the factory through `ToolRegistryOptions`, `SessionToolContext`,
`turn_runtime` and `SubAgentRequest`; a widened child ceiling; a degraded uncontained launch;
bash or Python continuing when containment is unavailable; an unregistered backend; a Seatbelt
credential deny emitted before a writable re-allow; a hardlinked deny that is launched instead
of refused; and the scheduler rebuilding containment without the durable-payload tier request.
Each is proven observable by the self-test, so a green guard means the property was checked,
not that the script exited 0 vacuously.

## 9. Documentation and operations

- `scripts/check_sandbox_policy_wiring.py` — the script is the documentation: a comment per
  corrected check names the accepted production form and why
  (`sandbox_config_for_durable_payload` requests the M004 tier while the config is still being
  built, so path identities are captured).
- `scripts/verify.sh:128-146` — a block comment naming the seven defect classes the pair
  protects and why both run with their self-tests.
- `.github/workflows/ci.yml:105-120` — the matching four steps.
- `.github/workflows/shared-checkout-macos-qualification.yml` — the lane comments for the
  marker and the added members, plus the step-summary lines that publish each log.
- `plans/registry.md` does not enumerate per-guard coverage; the work package E record is
  therefore "every check was fixed and wired, and the audit found no deliberate exception",
  recorded here and in the script comments.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | No hosted run of the macOS lane | The path-filter trigger, the added members, and the strict marker are verified by reading the workflow, by local YAML parse, by a local filter evaluation, and by local member runs — not by a hosted run. A lane-level failure (runner image, helper build, seatbelt availability) is unobserved | Dispatch the lane (`workflow_dispatch`) on this branch and record the run URL; a green run also substantiates AC5 and AC6 end to end |
| Low | `scripts/verify.sh quick` is red in this tree on `src/main.rs` formatting, before the guard phase | The four sandbox invocations were not observed *inside* the lane; a future reader could mistake this record for "quick passed" | Format `src/main.rs` (not a C005 file) and re-run `scripts/verify.sh quick` |
| Low | `check_comment_budget.py` is red on 21 entries in this tree, none under the sandbox surface | The guard phase would still fail after the formatting fix, for reasons outside this milestone | Resolve in the owning changes (TUI/config/protocol comment budgets); the sandbox files are within their raised ceilings |
| Low | The "delete one injected defect and watch the self-test fail" demonstration was not executed | `EXPECTED_INJECTIONS` counting is asserted in code but not observed failing; a typo in the count would be caught only by review | Delete one case from `self_test()`'s case list, run `--self-test`, observe the count failure, restore |
| Low | The baseline script's exit 1 was established by reading, not by running the baseline script against the baseline tree | The stale-pattern premise is proven from the two readings (script demands a literal the file does not contain), which is stronger than the plan's narrative, but it is not an observed exit code | Optional: check out HEAD into a scratch worktree and run `python3 scripts/check_sandbox_policy_wiring.py` there |

No high-severity finding: every acceptance criterion is met, and the outstanding items are
either demonstration hygiene or hosted-lane corroboration that the plan explicitly declines to
claim without a run.

## 11. Roadmap disposition

**Milestone closed.** C004's strict-mode marker is consumed as designed: the lane sets
`CODEGG_SANDBOX_QUALIFICATION: required`, the fixture defines the contract, and neither side
renamed it. The guard set now runs in both lanes with invoked self-tests.

`scripts/verify.sh quick` could not complete in this tree — it aborts at `cargo fmt --check
--all` on `src/main.rs`, which is not a file this milestone owns and not a file this record
may format. The acceptance evidence for work package C is therefore the four invocations run
directly (all pass, §4) plus the `run_guards()` and CI wiring that names the same four, not a
green lane. The lane's hosted green remains an operational follow-up recorded in §10, not a
condition on the milestone: the plan scopes re-qualifying the lane *out* and forbids claiming
a hosted run that did not happen.

## 12. Registry updates

- Roadmap §12 status table: C005 → `closed`, closure record
  `plans/closure/sandbox-containment-audit-corrective/005-status.md`.
- `plans/registry.md`: the subsystem row's "no work started" state is inaccurate; C005's plan
  row moves from `ready` to `closed`. If the subsystem row enumerates which guards run where,
  add the two sandbox guards to the automatic set — they now run in both `quick` and CI, which
  is the definition that set uses.
- Unblock audit: C005 had one dependency (C004's marker) and it is satisfied; no milestone
  waits on C005.
