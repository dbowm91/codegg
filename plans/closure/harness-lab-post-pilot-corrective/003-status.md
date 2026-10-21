# Harness Lab Post-Pilot Corrective 003 — Closure Status

Status: **conditionally closed — blocked on live-provider authorization**

This is the disposition §9 of the plan prescribes for exactly this case:

> If provider spend cannot be enforced, a real defect does not exist, or
> OpenCode access is missing, preserve a **conditional/blocked** disposition
> with precise evidence. A deterministic seeded-regression success closes
> *mechanical scoring* only, never the external live autonomous improvement
> claim.

Source implementation plan:

- `plans/implementation/harness-lab-post-pilot-corrective/003-live-proposer-correction-and-acceptance.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`

Repository baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`

## 1. Executive finding

The C001 and C002 dependency on C003 is discharged. What remains is not a
code dependency: it is a **live-provider authorization decision** that has not
been made, plus one input that does not yet exist — a genuine, currently
reproducible harness defect.

Two deterministic prerequisites were completed in this pass:

- The OpenCode CLI version pin now names the **actually installed** CLI, and
  the unsupported-version refusal is a distinct, nameable error rather than an
  opaque one.
- The bounded compile-correction mechanic is extracted and qualified against a
  **labelled fault injection**, so the retry bounding is a tested property
  rather than an untested code path.

The live acceptance claim itself is **not** made. No authenticated OpenCode
proposal was run, no real defect was found or planted, and no candidate was
accepted. §9 requires both a genuine fixed defect and an actual authenticated
proposer invocation for a strict closure, and neither is in evidence.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| §5-A: freeze a baseline SHA and a failing case with an explicit assertion and case hash | — | **Blocked**: no genuine currently reproducible defect has been identified. Planting one is prohibited by the plan. |
| §5-A: prove the failure in the base binary before any proposal | — | **Blocked** on the defect above. |
| §5-A: deterministic seeded historical regression, clearly labelled synthetic | The acceptance wiring is proven at the contract level in C002 (`test_correctness_fix_is_accepted_with_a_correctness_objective`, `test_fully_matched_candidate_is_not_accepted`); the full candidate → ledger → compare → qualify chain is not | **Partial** — closes mechanical scoring only |
| §5-B: pin the actual installed OpenCode CLI version and `run --auto --format json` semantics from the documented version | `SUPPORTED_OPENCODE_VERSION = "1.19.3"`; verified against the installed `opencode run --help` and a live `--version` read | **Met** (CLI surface); the JSON *event payload* shape still needs one authenticated invocation |
| §5-B: one authenticated bounded proposal in a dedicated worktree and private config | — | **Blocked**: needs an approved model route with C001-verified spend authority |
| §5-C: bounded real compile correction, separately counted and gated, with a compile/test re-run | `_build_with_compile_correction`; four labelled fault-injection tests | **Met** for the mechanic, **labelled** — not a real OpenCode-generated correction |
| §5-C: a genuine source candidate: fmt/clippy/focused tests, isolated build, original failing verifier, protected regressions, holdout, paired complete score | — | **Blocked** on a real candidate and a real defect |
| §5-D: kill a supervised process after ≥1 persisted scored trial, resume with identical immutable inputs, prove prior row reused | C002 proved the mechanism against real SIGTERM/SIGKILL subprocesses; the plan requires this *during an actual candidate qualification* | **Partial** — mechanism proven, live run not |
| §6: a missing OpenCode binary must leave manual proposer mode working | `run_opencode` raises `FileNotFoundError`; `manual_dry_run` is the manual path | **Met** |
| §6: an unsupported version must yield a distinct failed/blocked receipt | The version refusal now names the expected and observed versions | **Met** |
| §9: no merge/push of candidate code | No candidate worktree was created and nothing was merged | **Met** |

## 3. Implementation and safety evidence

### 3.1 The OpenCode version pin

`improve.py` pinned `SUPPORTED_OPENCODE_VERSION = "1.19.0"` and compared the
CLI's `--version` stdout for exact string equality. The installed CLI is
**1.19.3**, so the preflight refused the only binary on this machine and the
live proposer path was unreachable.

The pin is now `1.19.3`, verified against the documented CLI surface of the
installed version:

```
$ opencode --version
1.19.3

$ opencode run --help
run opencode with a message
Options:
  --format       format: default (formatted) or json (raw JSON events)
  --dir          directory to run in
  --model        model to use in the format of provider/model
  --agent        agent to use
  --auto         auto-approve permissions that are not explicitly denied (dangerous!)
```

`run`, `--auto`, `--format json`, `--dir`, `--model`, and `--agent` are all
present with unchanged meanings, and the live version gate now passes:

```
pinned: 1.19.3
observed stdout: '1.19.3'
gate passes: True
```

The pin certifies the **CLI surface** only. The JSON event payload shape that
`parse_opencode_events` consumes still needs one authenticated bounded
invocation to confirm, and that is part of the blocked live work.

The refusal is now a distinct, nameable failure rather than a bare
`"OpenCode version preflight failed"`:

```
OpenCode version preflight failed: expected 1.19.3, observed 1.18.9 from /path/to/opencode
```

This is asserted by `test_an_unsupported_opencode_version_is_a_named_refusal`,
which checks that the expected version, the observed version, and the
refusal's nature all appear. The plan requires an unsupported version to yield
a distinct receipt, not a transport fault.

### 3.2 The bounded compile-correction mechanic

The compile-correction loop was inline in `qualify_candidate`, so its bounding
was not directly testable. It is now `_build_with_compile_correction`, with
the build attempt and the proposer call injected:

- each retry is separately counted and gated;
- the build is re-run after every correction;
- the retry count is exhausted rather than unbounded;
- a non-OpenCode proposer never retries;
- a correction that removes the required changelog, invalidates its required
  sections, or leaves an unresolved placeholder is refused;
- the correction stays inside the already authorized path scope.

The four tests qualify this mechanic against a **labelled fault injection**:
a fake build that fails once with a compiler-shaped error, and a fake
proposer that returns a correction. This is the branch §5-C explicitly allows
for a mechanics qualification, and it is labelled as such — it is **not** a
real OpenCode-generated compile correction and claims no production gain.

The extraction preserved the original deadline semantics: the build and the
proposer call each recompute the remaining wallclock at call time, so a retry
cannot outlive the cumulative budget.

## 4. Verification executed

```
$ python3 -m unittest discover -s dev/harness_lab/tests
Ran 117 tests in 4.544s

OK

$ python3 -m unittest dev.harness_lab.tests.test_improve
Ran 26 tests in 2.840s

OK
```

New in this plan:

| Test | Assertion |
|---|---|
| `test_a_compile_failure_is_corrected_once_and_the_build_is_rerun` | The build is attempted twice and the correction is requested exactly once; the successful build is the recorded gate. |
| `test_the_compile_correction_retry_is_bounded` | With two allowed retries, the build is attempted three times and the proposer twice, then the failure propagates. |
| `test_a_non_opencode_proposer_never_retries` | A manual proposer gets one build attempt and no correction request. |
| `test_a_correction_that_drops_the_changelog_is_refused` | A correction that removes the required changelog is refused. |
| `test_an_unsupported_opencode_version_is_a_named_refusal` | The version refusal names the expected version, the observed version, and its nature. |

See `plans/closure/harness-lab-post-pilot-corrective/003-evidence.md`.

## 5. Remaining conditions

These are the precise conditions that block a strict C003 closure. Each is
stated so the next run can discharge it directly.

1. **An approved model route with C001-verified spend authority.** The plan
   requires it for every actual provider request. The C001 preflight is
   implemented and refuses a policy without an enforceable cap, so a budget
   policy must be supplied — with a stated `cost_enforcement`, or a
   retry-safe provider quota — before any live proposal. This needs an
   operator decision that has not been made.
2. **A genuine, currently reproducible harness defect.** §5-A forbids
   fabricating one and §9 requires reporting blocked if none exists. No such
   defect is in evidence, and none was planted.
3. **One authenticated bounded OpenCode proposal** in a dedicated linked
   worktree with a private config, which will also confirm the JSON event
   payload shape against the pinned version.
4. **A real candidate qualification run** with fmt/clippy/focused tests, an
   isolated build, the original failing verifier, protected regressions, a
   hidden holdout, and a complete paired score.
5. **The interruption proof during that actual run**, not only against the
   synthetic subprocesses C002 used.

## 6. Planning audit

- No plan file was edited to make this implementation look complete.
- No candidate worktree was created, no candidate code was merged, and
  nothing was pushed from a candidate.
- The fault-injected compile-correction qualification is labelled as a
  mechanics qualification, not as a real fix.
- No live provider call was made and no model route was exercised.
- The baseline expected outcomes were not changed to force a win.
- The harness-lab Python suite remains wired into `scripts/verify.sh quick`
  and the CI workflow.

## 7. Status of M004 and the C004 unblock

- **M004** stays **conditionally closed**. C003's mechanical scoring half is
  now evidenced, but M004's original live-acceptance condition — an actual
  authenticated OpenCode correction with an independently accepted fix — is
  not discharged.
- **C004** stays **blocked on C003** for its final autonomy claim. Its C001
  precondition is discharged, so what C004 still needs from C003 is the
  accepted-defect receipt and the approved usage budget for the Task 2
  provider pair.
