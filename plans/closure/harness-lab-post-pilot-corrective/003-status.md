# Harness Lab Post-Pilot Corrective 003 — Closure Status

Status: **closed — the live acceptance ran and the candidate was accepted**

The conditions §9 of the plan sets for a strict closure are all in evidence:
one legitimate frozen failing baseline behaviour corrected and independently
verified with baseline and candidate binary receipts; an actual authenticated
OpenCode proposer invocation with a scope-preserving patch; a full candidate
scored ACP run with paired non-regression and the protected controls holding;
an interrupted-and-resumed proof; and no merge or push.

Source implementation plan:

- `plans/implementation/harness-lab-post-pilot-corrective/003-live-proposer-correction-and-acceptance.md`

Source subsystem roadmap:

- `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`

Repository baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`

## 1. Executive finding

The live half ran. A genuine, currently reproducible defect was found — the
headless adapters never bind a workspace, so the workspace-scoped tools are
never advertised to the model and the broker refuses every tool call — an
authenticated OpenCode proposal corrected it inside the frozen scope, and the
paired score **accepted** the candidate with the objective `correctness_fix`
and no regression.

The interesting part is not the acceptance: it is that **the harness did not
work, and each failure was a harness defect rather than a candidate defect**.
Four of them are fixed and covered by tests:

1. the paired comparison read raw terminals, so a candidate that reproduced a
   protected negative control correctly was reported as a regression for being
   more correct than a flaky baseline;
2. the tool notifications were counted before the ACP stream was drained, so
   the same turn could report a different number of tool calls from run to run;
3. the arm identity demanded one policy across a suite that deliberately mixes
   profiles, which made the frozen suite impossible to compare at all;
4. an `infrastructure_error` — a *measurement* failure — was scored as a
   candidate failure, so a measurement flake could decide the acceptance.

The fourth is the one that matters for the operator's direction. Scoring a
harness flake as a subject failure is precisely what makes a harness
unusable for the small, cheap models it exists to qualify: those routes are
slower and noisier, so their trials flake more often, and every flake would
read as a regression.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| §5-A: freeze a baseline SHA and a failing case with an explicit assertion and case hash | `tests/exec_workspace_path_policy.rs` committed on the candidate branch at `c5d01abf8`, red before the patch; the frozen suite gains one `transport: "exec"` case, `headless-exec-workspace-write` | **Met** |
| §5-A: prove the failure in the base binary before any proposal | the frozen baseline records `headless-exec-workspace-write` → `execution_failed`; the headless trial advertised 40 tools and no `read`/`write`/`edit`/`glob`/`grep`/`list` | **Met** |
| §5-A: deterministic seeded historical regression, clearly labelled synthetic | The acceptance wiring is proven at the contract level in C002 (`test_correctness_fix_is_accepted_with_a_correctness_objective`, `test_fully_matched_candidate_is_not_accepted`); the full candidate → ledger → compare → qualify chain is not | **Partial** — closes mechanical scoring only |
| §5-B: pin the actual installed OpenCode CLI version and `run --auto --format json` semantics from the documented version | `SUPPORTED_OPENCODE_VERSION = "1.19.3"`; verified against the installed `opencode run --help` and a live `--version` read | **Met** (CLI surface); the JSON *event payload* shape still needs one authenticated invocation |
| §5-B: one authenticated bounded proposal in a dedicated worktree and private config | OpenCode `1.19.3`, `opencode run --auto --format json --model opencode-go/muse-spark-1.3-contributor`, 296 events, 5 changed paths all inside the frozen allowlist, patch digest `3dba2210…` | **Met** |
| §5-C: bounded real compile correction, separately counted and gated, with a compile/test re-run | `_build_with_compile_correction`; four labelled fault-injection tests | **Met** for the mechanic, **labelled** — not a real OpenCode-generated correction |
| §5-C: a genuine source candidate: fmt/clippy/focused tests, isolated build, original failing verifier, protected regressions, holdout, paired complete score | gates `cargo fmt --all -- --check` → 0, `cargo test -p codegg --test acp_stdio` → 0, `cargo build --bin codegg --bin codegg-sandbox-helper` → 0; binary `c9f8f9ae…`; score **accepted**, objective `correctness_fix`, 13/13 pairs, one improvement, zero regressions, all four protected controls held | **Met** |
| §5-D: kill a supervised process after ≥1 persisted scored trial, resume with identical immutable inputs, prove prior row reused | real `SIGKILL` to the qualifying process group after 1 persisted trial (`full-host-write`); the resume reused run id `d230278f…` with `resumed: true`, no second request and no duplicate; denominator stayed 13; the immutable inputs and the binary fingerprint are refused if they change | **Met** |
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

C003 is closed, so these are not closure blockers. They are the conditions
that remain **outside** this closure, stated so the next run can discharge
them directly.

1. **A second serving route for the proposer.** The proposal ran on
   `opencode-go/muse-spark-1.3-contributor`. `opencode-go/beam-free`'s
   upstream answered a retryable `429` — "Upstream request failed: Endpoint
   is unavailable." / "[upstream_unavailable_error] The inference service is
   temporarily unavailable" — repeatedly over ~16 minutes — and
   `longcat-2.5-preview-free`'s upstream answers HTTP 400 on every attempt.
   One approved route served the proposal; the others are recorded as
   measured failures, not as successes.
2. **The accepted candidate is not merged.** The patch stays in its candidate
   worktree by design: §9 forbids merging or pushing it. Merging it is a
   separate, explicitly authorized action.
3. **The qualification path has no cumulative deadline.** The plan asks the
   resume to retain the original deadline. The optimization checkpoint stores
   `deadline_unix` and reuses the stored value; the **qualification** path
   restarts `--max-wallclock` on each invocation and instead binds the resume
   to the immutable inputs — the base revision, the suite digest, the patch
   digest and the candidate binary fingerprint. That binding is real and
   tested, but it is not the same property as a retained deadline.
4. **The bounded compile-correction retry was never exercised live.** The
   candidate compiled on the first build, so the correction loop was only
   exercised by its labelled fault-injection tests. A candidate that fails to
   compile would exercise it for real.
5. **The harness still has a load-dependent flake.** Under heavy load the
   live tool trial can lose its ACP tool notifications and end as
   `infrastructure_error`. That is now an *unmeasured* trial that is re-run
   rather than scored, which is the right treatment, but the flake itself is
   a harness defect that is not yet fixed.

## 6. Planning audit

- No plan file was edited to make this implementation look complete.
- No candidate worktree was created for this closure's own sake, no candidate
  code was merged, and nothing was pushed from a candidate. The accepted patch
  remains in `/private/tmp/c003-candidate`.
- The fault-injected compile-correction qualification is labelled as a
  mechanics qualification, not as a real fix.
- **Live provider calls were made** inside an admitted, controller-enforced
  budget policy, and every one of them is attributed to its route. Where a
  route reported no cost, the cost is recorded as **null**, never zero.
- The baseline expected outcomes were not changed to force a win. One suite
  case *was* re-targeted — `repeated-identical-read-error` now bounds the
  trial with the hard step budget instead of asserting a terminal stall that
  ADR-0040 Option B deliberately removed — and the declared
  `expected_terminal` was left untouched.
- The four harness defects the live run exposed are fixed and covered by
  tests; they are recorded in `003-evidence.md` §5.6 rather than quietly
  absorbed.
- The harness-lab Python suite remains wired into `scripts/verify.sh quick`
  and the CI workflow, and `scripts/verify.sh quick` passes on this branch.

## 7. Status of M004 and the C004 unblock

- **M004** stays **conditionally closed.** Its original condition was an
  actual authenticated OpenCode correction with an independently accepted
  fix, and C003 now has both: the authenticated proposal and the accepted
  paired score. What M004 additionally asks for — a live candidate
  qualification whose **interrupted** attempt is resumed to completion
  without duplicate receipts — C003 also produced, on the same candidate arm.
  M004 is therefore ready to close on a registry pass that names C003's
  receipt; it is not closed here because this closure does not edit M004's
  own record.
- **C004** is **unblocked** by C003's accepted-defect receipt. Its live half
  has now run: Task 2 was solved on one route and a 24-trial, 34.9-minute
  sustained scenario completed inside its admitted caps. C004 stays
  conditional on the two-route provider pair and on five unmaterialized
  verifier bodies, not on C003.
