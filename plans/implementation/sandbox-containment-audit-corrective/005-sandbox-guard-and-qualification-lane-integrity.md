# Sandbox Containment Audit Corrective Milestone C005 — Sandbox Guard and Qualification-Lane Integrity

Status: closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c005--sandbox-guard-and-qualification-lane-integrity`

Long-term requirements:

- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: infrastructure

## 1. Objective

Make the guards that protect silent containment defects actually run, prove they
can fail, and make the macOS qualification lane trigger on the file it is supposed
to protect and refuse to go green from a skip.

## 2. Why this milestone is ready

No hard dependency. Every change is in `scripts/` and `.github/workflows/`. The
guards already exist and already encode the right properties; one has a stale
pattern and the other is simply not invoked with its self-test.

The one interface it consumes — the strict-mode marker that turns a skipped
containment test into a failure — is defined by **C004**. This milestone sets it in
the workflow and does not define it.

## 3. Current implementation evidence

**`check_sandbox_policy_wiring.py` is invoked by nothing.**
A search for its name across `scripts/` and `.github/` returns only its own
docstring. It is 580 lines and enforces real properties: profile threading through
`ToolRegistryOptions`/`SessionToolContext`/`turn_runtime`/`SubAgentRequest`, child
ceiling narrowing, no degraded uncontained launch, both backends registered, deny
carving, Seatbelt deny-after-reallow ordering, hardlinked-deny refusal, helper
dispatch through the registry, and the existence of the end-to-end containment
file.

**It currently fails, on a stale pattern.**
Running it on the tree at the baseline exits 1 with
`src/scheduler/executors.rs: missing shell executor rebuilds the accepted containment`.
The check looks for the literal `sandbox_config_for_profile` in
`executors.rs`; the code calls
`crate::security::sandbox::sandbox_config_for_durable_payload(...)`, which calls
`sandbox_config_for_profile` internally and is the *better* form — its own comment
explains that the M004 tier must be requested while the config is still being
built, or the spec has no captured path identity and the launch fails closed. The
property the guard intends to check **holds**; the pattern is stale. That false
positive is the most likely reason it was never wired.

**It has no `--self-test`.**
Passing `--self-test` runs the normal check (the flag is ignored). Under the
repository's automatic-guard doctrine — "a guard that runs only when somebody
remembers it protects nothing" — a guard with neither wiring nor a self-test
protects nothing in two ways at once.

**`check_sandbox_contract.py` is wired but its self-test is not.**
`scripts/verify.sh` runs it inside `run_guards()` and CI runs it as a step. Its
`--self-test` exists and passes, but neither file invokes it, so its non-vacuity is
unverified in every automated lane.

**The macOS lane names a file that does not exist.**
`.github/workflows/shared-checkout-macos-qualification.yml` lists
`tests/sandbox_containment.rs` in its path filters at two places. That file does
not exist; the real member is `tests/security_permission/sandbox_containment.inc`,
declared in `tests/security_permission/mod.rs`. An edit confined to the `.inc`
therefore does not trigger the lane. The lane also runs only the
`sandbox_containment` member, so `sandbox_policy_wiring` and
`host_admission_manifest` never run on hosted macOS.

**The lane has no strict mode.**
Its `/usr/bin/sandbox-exec` presence check is a separate shell step. The tests
themselves still return early when the capability probe fails, so the lane's
green does not imply the tests observed anything.

## 4. Invariants that must not regress

- No guard is silenced to obtain green. A failing check is either a stale pattern
  (fix the pattern) or a real defect (report it).
- A guard that is wired carries an invoked `--self-test`.
- `quick` and CI stay a subset relationship as the repository defines it: nothing
  this milestone adds to `quick` is absent from CI unless recorded with the reason.
- The macOS lane is not disabled, weakened, or made non-blocking to get green.
- No production source changes.

## 5. Scope

### In scope

- `scripts/check_sandbox_policy_wiring.py` — stale patterns and `--self-test`.
- `scripts/verify.sh` — invoke both sandbox guards' self-tests.
- `.github/workflows/ci.yml` — the same.
- `.github/workflows/shared-checkout-macos-qualification.yml` — path filters,
  member list, strict-mode marker.
- The registry's guard documentation if it names these guards.

### Explicitly out of scope

- Any new CI lane or job.
- Re-qualifying the macOS lane if it is currently red; if it is, record it.
- Changing the containment tests themselves (C004).
- Changing the production code any guard inspects.
- The other 84 guards in `quick`.

## 6. Required production changes

None. This milestone changes guard scripts and workflow configuration only.
`src/**` must be untouched, which is an acceptance criterion.

### Documentation and static guards

The guard scripts are the documentation. Each fixed check gets a comment naming
the production form it accepts and why, so the next refactor does not resurrect
the stale pattern.

## 7. Ordered work packages

### Work package A — Fix the stale patterns

Intent: the guard must fail on a real regression and pass on the correct
implementation.

Required changes: replace the `executors.rs` expectation with one that accepts the
durable-payload helper — for example match
`sandbox_config_for_(durable_payload|profile)` and, where the distinction matters,
assert that the durable form is used. Then run the guard to completion and treat
**every** remaining failure as a decision: a stale pattern is fixed, a real defect
is reported and does not get a passing pattern. Do not delete a check to reach
green; if a check no longer describes the intended property, record why in the
script and in the milestone evidence.

Acceptance evidence: the guard passes on the baseline tree, and a deliberately
reverted call site (restore a bare `BashTool::default()` or drop the profile
threading) makes it fail.

### Work package B — Give the guard a `--self-test`

Intent: a guard must be able to prove it is not vacuous.

Required changes: add a `--self-test` mode that injects at least one defect per
enforced class and asserts the guard rejects each. Model it on
`check_sandbox_contract.py`'s existing self-test. Cover at minimum: a dropped
profile thread, a widened child ceiling, a degraded uncontained launch, a missing
backend registration, a Seatbelt deny emitted before a re-allow, and a hardlinked
deny that is not refused.

Acceptance evidence: the self-test passes; deleting one injected defect makes it
fail.

### Work package C — Invoke both self-tests in `quick` and CI

Intent: non-vacuity must be checked by a lane, not by a human.

Required changes: add `check_sandbox_contract.py --self-test` and
`check_sandbox_policy_wiring.py` plus its `--self-test` to `run_guards()` in
`scripts/verify.sh`, following the existing guard-then-self-test pattern, and add
the same steps to `.github/workflows/ci.yml`. Keep `quick` and CI in the
established subset relationship; if any addition is intentionally `quick`-only,
record the reason where the repository records such differences.

Acceptance evidence: `scripts/verify.sh quick` runs all four invocations and
passes; the CI step names match.

### Work package D — Repair the macOS lane

Intent: the lane must trigger on the files it protects and must not pass from a
skip.

Required changes: replace `tests/sandbox_containment.rs` in both path-filter
entries with the real paths — `tests/security_permission/**` and, if the lane
should also react to policy changes, `src/security/sandbox/**`. Add the
`sandbox_policy_wiring` and `host_admission_manifest` members to the lane's test
selection so the lane covers the sandbox surface it exists for. Set **C004's
strict-mode marker** in the step environment.

Acceptance evidence: an edit confined to `tests/security_permission/
sandbox_containment.inc` triggers the lane (demonstrate with a throwaway branch or
the workflow's own filter evaluation); with the marker set, a host without a
backend fails the member.

### Work package E — Record any deliberately unwired guard

Intent: an unwired guard must be a recorded decision, never an omission.

Required changes: if work package A finds a check whose property genuinely no
longer applies, record it — in the script and in the registry's verification
policy if that is where the repository records guard coverage — rather than
deleting it silently. If every check is fixed and wired, say so and record that
the audit found no deliberate exception.

Acceptance evidence: the record exists and matches the script.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Guards are stateless scripts; the lane is a single CI job.

## 9. Compatibility and migration

No production, configuration, or protocol change. Adding guard steps lengthens
`quick` and CI by the runtime of four short Python scripts. If that is material,
the reason is measured and recorded rather than assumed — do not add a wall-clock
claim that was not measured.

## 10. Required tests

### Focused unit tests

- `check_sandbox_policy_wiring.py --self-test` covers six injected defect classes.
- `check_sandbox_contract.py --self-test` continues to pass.

### Integration tests

- `scripts/verify.sh quick` runs all four invocations and passes.
- The CI workflow's steps match `quick`'s for these guards.

### Restart and recovery tests

Not applicable.

### Contention and cancellation tests

Not applicable.

### Security and negative tests

- Each self-test injection is proved to fail the guard when the guard's check is
  removed.

### Migration and compatibility tests

- The macOS lane triggers on a change to the containment `.inc`.

## 11. Required verification commands

```bash
# the guards, focused
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_sandbox_policy_wiring.py --self-test

# the lane wiring
bash -n scripts/verify.sh
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml'))"
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/shared-checkout-macos-qualification.yml'))"

# the canonical sanity lane
scripts/verify.sh quick

# formatting
cargo fmt
```

Do not claim the macOS lane ran unless it ran. A local filter evaluation is not a
hosted run; label it as such.

## 12. Documentation updates

- `scripts/check_sandbox_policy_wiring.py` — a comment per fixed check naming the
  accepted production form.
- The registry's verification policy, if it enumerates guard coverage.
- The roadmap §12 status table and `plans/registry.md` on closure.

## 13. Acceptance criteria

1. `check_sandbox_policy_wiring.py` passes on the baseline tree.
2. Reverting the call site it protects makes it fail.
3. `check_sandbox_policy_wiring.py --self-test` exists, passes, and fails when an
   injected defect is removed.
4. `check_sandbox_contract.py --self-test` is invoked by `quick` and by CI.
5. The macOS lane's path filters name files that exist and trigger on a change to
   the containment `.inc`.
6. The macOS lane sets C004's strict-mode marker and runs the sandbox members.
7. `git diff --stat` shows no file under `src/**` changed.

## 14. Stop conditions

Stop and report rather than improvise when:

- a guard failure turns out to be a **real** defect rather than a stale pattern —
  report it and do not add a passing pattern;
- fixing the lane's path filter reveals the lane has been red, in which case record
  the failure and its cause rather than disabling the lane;
- C004's strict-mode marker is not available or has a different name;
- wiring a guard into `quick` would break the repository's `quick`-superset-of-CI
  relationship without a recorded reason;
- the work would require a new CI lane.

## 15. Closure evidence required

- The guard's before/after output on the baseline tree.
- The revert demonstration for each fixed check.
- Both self-tests, with the injected-defect-removal demonstration.
- The `verify.sh quick` result.
- The lane's path-filter change and the evidence it triggers; the hosted run
  identified by URL if one was executed, or an explicit statement that only the
  local filter evaluation was performed.
- The `git diff --stat` showing no `src/**` change.
- The roadmap §12 and registry updates, including the unblock audit.

## 16. Handoff notes

- The `executors.rs` expectation is the **only** known stale pattern, but the guard
  has 580 lines that have never run in a lane. Expect more. Treat each as a
  decision, and prefer fixing the pattern over weakening the check.
- `sandbox_config_for_durable_payload` is the correct production form for the
  scheduler path: it requests the M004 tier while the config is still being built,
  which is what makes the protected mount view capture path identities. Do not
  "fix" the code to satisfy the old pattern.
- The macOS lane's `/usr/bin/sandbox-exec` presence step is not a substitute for
  strict mode. Keep it, but the marker is what makes the tests fail.
- `verify.sh` has a documented `quick`-runs-more-than-CI relationship. If you add
  a guard to `quick` only, record it in the same place that relationship is
  documented.
- This plan and **C004** meet at the strict-mode marker. C004 owns the name and the
  test-side behaviour; this plan only sets it in the workflow.
