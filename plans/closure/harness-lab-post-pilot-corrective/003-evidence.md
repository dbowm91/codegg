# Harness Lab Post-Pilot Corrective 003 — Verification Evidence

Branch: `plans/harness-lab-post-pilot-correctives`
Baseline reviewed: `2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59`
Verified: 2026-10-21

## 1. The installed OpenCode CLI

```
$ opencode --version
1.19.3

$ opencode run --help
run opencode with a message

Positionals:
  message  message to send                                                     [array] [default: []]

Options:
  --format       format: default (formatted) or json (raw JSON events)
                                          [string] [choices: "default", "json"] [default: "default"]
  --dir          directory to run in, path on remote server if attaching                [string]
  --model        model to use in the format of provider/model                   [string]
  --agent        agent to use                                                           [string]
  --auto         auto-approve permissions that are not explicitly denied (dangerous!)
                                                                          [boolean] [default: false]
```

Every flag the harness adapter passes to the proposer — `run`, `--auto`,
`--format json`, `--dir` — is present in the installed version with an
unchanged meaning.

## 2. The version gate against the real binary

```
$ python3 -c "..."
pinned: 1.19.3
observed stdout: '1.19.3'
gate passes: True
```

Before this change the pin was `1.19.0`, so the gate refused the only
installed binary:

```
pinned: 1.19.0
observed stdout: '1.19.3'
gate passes: False
```

The refusal is now distinct and nameable. With a fake CLI reporting `1.18.9`:

```
OpenCode version preflight failed: expected 1.19.3, observed 1.18.9 from /path/to/opencode
```

Asserted by `test_an_unsupported_opencode_version_is_a_named_refusal`.

## 3. The bounded compile-correction mechanic

The mechanic was extracted into `_build_with_compile_correction` so its
bounding is directly testable. It is qualified against a **labelled fault
injection** — a fake build that fails once with a compiler-shaped error and a
fake proposer that returns a correction. This is the branch §5-C explicitly
allows for a mechanics qualification; it is not a real OpenCode-generated
compile correction and claims no production gain.

| Test | Assertion |
|---|---|
| `test_a_compile_failure_is_corrected_once_and_the_build_is_rerun` | The build is attempted twice; the correction is requested exactly once; the successful build is the recorded gate; the correction prompt identifies itself as a compile-correction retry. |
| `test_the_compile_correction_retry_is_bounded` | With two allowed retries, the build is attempted three times and the proposer twice, then the failure propagates instead of retrying forever. |
| `test_a_non_opencode_proposer_never_retries` | A manual proposer gets exactly one build attempt and no correction request. |
| `test_a_correction_that_drops_the_changelog_is_refused` | A correction that removes the required `CHANGELOG.harness.md` is refused. |

```
$ python3 -m unittest dev.harness_lab.tests.test_improve.CompileCorrectionMechanicsTests -v
test_a_compile_failure_is_corrected_once_and_the_build_is_rerun ... ok
test_a_correction_that_drops_the_changelog_is_refused ... ok
test_a_non_opencode_proposer_never_retries ... ok
test_the_compile_correction_retry_is_bounded ... ok

Ran 4 tests in 0.7s

OK
```

## 4. The full harness suite

```
$ python3 -m unittest discover -s dev/harness_lab/tests
Ran 232 tests in 28.659s

OK

$ python3 -m pyflakes dev/harness_lab/*.py dev/harness_lab/tests/*.py
(no findings)
```

## 5. The live run

### 5.1 The authorization and the routes

The operator approved four low-cost/free routes — `longcat-2.5-preview-free`,
`muse-spark-1.3-contributor`, `minimax-m3.1-flash-free` and `beam-free` — and
a controller-enforced budget policy was admitted before the first provider
request:

```json
{"token_limit": 4000000, "cost_limit_usd": 1.0, "cost_enforcement": "controller_enforced",
 "cost_observable": true, "max_provider_requests": 80, "max_wallclock_seconds": 5400}
```

```
$ python3 -m dev.harness_lab budget-check --budget /tmp/codegg-proposer-budget2.json
{"status": "admitted", "enforced_caps": ["cost_limit_usd", "max_provider_requests",
 "max_wallclock_seconds", "token_limit"], "missing_source": null}
```

The first proposal attempt, on `opencode-go/beam-free`, **failed mid-run** with
a retryable upstream error: `APIError "Upstream request failed: Endpoint is
unavailable."`, `statusCode 429`, `isRetryable: true`, after the model had
already edited two files. That is an upstream condition and it is recorded as
measured; the partial patch was discarded and the worktree was reset.
`opencode-go/muse-spark-1.3-contributor` then served the proposal.

### 5.2 The genuine defect

The headless adapters — `codegg exec` (the CI entry point) and `codegg run`
(the one-shot path) — build their agent loop in-process and never bind it to a
workspace. Two consequences follow from that one omission:

1. `src/exec.rs` and `src/main.rs` build the tool registry with
   `ToolRegistry::with_config_and_search_runtime(&config, search_runtime)`,
   which leaves `ToolRegistryOptions::workspace_root` as `None`. Every
   workspace-scoped tool (`read`, `write`, `edit`, `glob`, `grep`, `list`) is
   then registered as an `UnrootedTool`, and an unrooted tool is **not
   advertised to the model at all**. A headless run offers the model no way to
   read or write a file.
2. The same paths never install a workspace path policy on the agent loop, so
   the tool broker refuses every tool call with
   `permission denied: verified tool authority is missing workspace path
   policy`.

The measured proof is in the frozen baseline: the headless trial's advertised
tool list contains **40 tools and no `read`, `write`, `edit`, `glob`, `grep` or
`list`**. The daemon and ACP paths are unaffected — `src/agent/agent_loop_factory.rs`
binds the root and installs the single-root policy.

The failing case was frozen before any proposal: `tests/exec_workspace_path_policy.rs`
on the candidate branch (`c5d01abf8`), red with the exact expected message.

### 5.3 The authenticated proposal

| Item | Value |
|---|---|
| CLI | OpenCode `1.19.3` (the pin matches the installed binary) |
| Invocation | `opencode run --auto --format json --dir <worktree> --model opencode-go/muse-spark-1.3-contributor` |
| Events | 296, with the `step_start` / `text` / `tool_use` / `step_finish` shape confirmed against a live run |
| Changed paths | `CHANGELOG.harness.md`, `src/agent/agent_loop_factory.rs`, `src/exec.rs`, `src/main.rs`, `src/tool/mod.rs` — all inside the frozen allowlist |
| Patch digest | `3dba2210e779236d110341f6c956c840738b524b79eba55480f2bca3190d21af` |
| Gates | `cargo fmt --all -- --check` → 0, `cargo test -p codegg --test acp_stdio` → 0, `cargo build --bin codegg --bin codegg-sandbox-helper` → 0 |
| Candidate binary | `c9f8f9ae41633072566be8e692b9194591284d8caf75844eba3f268aa12b2668` |

The scope check ran before the candidate was admitted: every changed path is
inside the allowlist, and the candidate's `CHANGELOG.harness.md` carries the
four required sections with no placeholder.

### 5.4 The interrupted qualification (§5-D)

The qualified process was **really killed** with `SIGKILL` to its whole process
group, after at least one persisted scored trial:

```json
{"event": "cut_point_reached", "persisted_trials": 1, "task_ids": ["full-host-write"]}
{"event": "killed", "signal": "SIGKILL", "pid": 61333}
```

| Property | Evidence |
|---|---|
| The prior row is reused, with no second request and no duplicate | `full-host-write` kept run id `d230278f-5241-45c2-81e9-46fd1aaec9d0` across the kill; the resumed evaluation reported it as `resumed: true` and never re-ran it |
| The exact task denominator is unchanged | 13 expected pairs, 13 paired, `unpaired_left` and `unpaired_right` both empty |
| The immutable inputs are refused if they change | the resume requires the same base revision `e10724b98…`, suite digest `2b564ab0…`, patch digest and candidate binary fingerprint; a changed patch or a changed binary is refused |
| A stale build is fenced | the resumed run rebuilt the candidate and compared the binary fingerprint against the recorded `c9f8f9ae…` |
| The original deadline is retained | the **optimization** checkpoint stores `deadline_unix` and reuses the stored value on resume rather than recomputing it; the **qualification** path has no cumulative deadline field — its resume is bound to the immutable inputs above, and `--max-wallclock` is per invocation. This is stated rather than claimed as the same property. |

### 5.5 The paired score

| Criterion | Baseline | Candidate |
|---|---|---|
| `correctness.pass_rate` | 0.889 | **1.000** |
| `reliability.success_rate` | 0.846 | **0.923** |
| `control.no_regression` | 1.000 | 1.000 |
| `efficiency.duration_ms` (advisory) | 1225 | 1811 |

**Decision: `accepted`, objective `correctness_fix`.** Evidence pairs: 13.
Protected controls: `cancel-after-read`, `repeated-identical-read-error`,
`timeout-after-read`, `verifier-negative` — all held, none violated.

- **One improvement:** `headless-exec-workspace-write`, `execution_failed` →
  `passed`. The candidate fixed the defect.
- **No regressions.** The four protected controls kept their declared
  terminals.
- **One advisory regression:** the median trial duration rose from 1225 ms to
  1811 ms. The efficiency criterion is `regression_blocks=false`, so it is
  reported, not hidden.

### 5.6 Four harness defects the live run exposed

The live qualification did not work the first time, and each failure was a
harness defect, not a candidate defect. All four are fixed and covered by
tests.

1. **The paired comparison read raw terminals.** A protected negative control
   expects a *failure* terminal, so a baseline that happened to record
   `passed` did not satisfy the control while a candidate that recorded the
   declared failure did — and the candidate was reported as a regression for
   being more correct. The comparison is now expectation-relative, and a
   candidate that breaks a control the baseline satisfied is still a
   regression.
2. **The tool notifications were counted before the stream was drained.** The
   ACP server can deliver the last tool notifications *after* the
   `session/prompt` response, so the same turn could report a different
   number of tool calls from run to run — and the recorded terminal followed
   from that count. The stream is now drained before the counts are taken.
3. **The arm identity demanded one policy across a suite that mixes
   profiles.** The frozen suite runs one case under the full-host policy and
   the rest under the workspace-write policy, and the headless adapter records
   no ACP policy handshake at all. Requiring a uniform policy across an arm
   made the frozen suite **impossible to compare at all**. The arm-wide
   invariants are now the suite digest, the binary fingerprint and the
   revision; the case-scoped fields are compared between the two arms for the
   same case.
4. **A measurement failure was scored as a candidate failure.** An
   `infrastructure_error` — the harness, the daemon or the transport failed
   before the trial produced an observation — says nothing about the
   candidate. It is now an **unmeasured** trial: the pair is re-run and the
   measured row is the one that counts. Scoring it as a zero would have let a
   measurement flake decide the acceptance, which is exactly the wrong
   behaviour for the small, cheap models this harness exists to qualify.

Two more defects were found and fixed on the way: the live pilot leaked lane
daemons (31 survivors after 24 trials), and the macOS lane socket path
exceeded the platform's `sun_path` limit.

## 6. What was NOT run, and why

- **No two-route proposal.** `opencode-go/beam-free`'s upstream answered a
  retryable `429` mid-proposal, and `longcat-2.5-preview-free`'s upstream
  answers HTTP 400 "Endpoint is unavailable." on every attempt. One approved
  route served the proposal; the others are recorded as measured failures, not
  as successes.
- **No compile-correction retry was needed.** The candidate compiled on the
  first build, so the bounded correction loop was exercised by its labelled
  fault-injection tests only, as C002 recorded.
- **No merge and no push of the candidate.** The accepted code stays in the
  candidate worktree.

## 6. Workspace guards

`scripts/verify.sh quick` exits 0 — see `001-evidence.md` §5 for the guard
list and the `cargo check --workspace --all-targets --locked` result.
`cargo fmt --check --all` is clean and `cargo clippy -p codegg --all-targets
-- -D warnings` finishes with no warning.
