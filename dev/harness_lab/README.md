# Developer Harness Lab

This Python package is development-only. It is not registered as a production
CodeGG tool, ACP method, or shipped Rust command.

## Local two-subject lifecycle smoke

Build the candidate and run the smoke from a clean CodeGG checkout:

```bash
cargo build --bin codegg --bin codegg-sandbox-helper
python3 -m dev.harness_lab doctor --binary target/debug/codegg
python3 -m dev.harness_lab smoke --binary target/debug/codegg --subjects 2
# Optional policy handshake checks, each against private lane daemons:
python3 -m dev.harness_lab smoke --binary target/debug/codegg --subjects 2 --unattended-profile workspace-write
python3 -m dev.harness_lab smoke --binary target/debug/codegg --subjects 2 --unattended-profile full-host
```

`CODEGG_HARNESS_BINARY` may replace `--binary`. The smoke starts two ACP
clients concurrently, creates sessions through each native daemon, checks the
live daemon ID against `daemon.json`, verifies distinct generations/PIDs and
the candidate executable path/hash, stops one lane through the verified
`daemon stop` path, and confirms the other lane can still create a session.
It does not submit a prompt, call a provider, or modify the checkout.
The optional `--unattended-profile` cases prove that both daemons accepted and
reported the requested yolo/profile snapshot before session admission. They
do not prove a model tool call avoided `session/request_permission`; that
gap is covered by the turn-level fixture below.

## Deterministic native tool-path fixture

```bash
python3 -m dev.harness_lab tool-trial --binary target/debug/codegg --profile workspace-write
python3 -m dev.harness_lab tool-trial --binary target/debug/codegg --profile full-host
python3 -m dev.harness_lab tool-trial --binary target/debug/codegg --profile workspace-write --deny-bash
```

This starts a loopback-only OpenAI-compatible fixture and sends a real ACP
prompt through the daemon's provider dispatch, tool broker and write permission
path. The accepted trials verify a completed ACP tool receipt and harmless
workspace file write under an Ask rule resolved by yolo. `--deny-bash` sends a
separate explicit Bash denial and proves it remains denied without a permission
prompt. Provider credentials inherited from the
caller are removed from every lane; the fixture uses a lane-local dummy key.
The fixture never contacts an upstream model service.

```bash
python3 -m dev.harness_lab evaluate --suite dev/harness_lab/fixtures/smoke.jsonl --ledger /tmp/codegg-ledger
```

The evaluator runs the versioned 13-case task manifest and writes versioned
immutable run manifests, sequenced redacted event rows, captured verifier
artifacts, and an append-only `index.sqlite3` query index cross-checked against
the JSONL index. Cases cover write/edit/read/grep, distinct repeated reads, projected
output recovery through `context_read`, shell execution/recovery and bounded
identical failures, both policy profiles, explicit denial, cancellation,
timeout, an intentionally failing verifier, and one headless `codegg exec` turn
that must write a file through the native tool broker. `report`, `replay`, and `compare` read
that ledger offline. The projection case forces a large native `read` result
through its artifact handle, then requires `context_read` to return a tail
marker omitted from the initial model-visible projection. Comparison pairs task IDs and repeat indexes only when
group, suite digest, and case digest match. A paired comparison judges each
trial against its **declared** `expected_terminal`, not against the literal
string `passed`: a protected negative control expects a failure terminal, so a
candidate that reproduces the declared control is not a regression against a
baseline that happened to record `passed` for it. A trial with no declared
expectation is refused rather than guessed. Unknown token, cost,
CPU, RSS, provider cost, and model-visible context metrics remain unavailable;
token counts and native turn identity are recorded only when CodeGG's ACP
terminal event carries provider-reported usage. Tool selection is captured from
the ACP tool name; argument values are not stored, only their bounded projection
digest. Repeated identical tool calls and tool calls following failures are
reported as trace-derived recovery indicators, not model-declared recovery.
The suite intentionally includes `assertion_failed`, `execution_failed`,
`cancelled`, and `timeout` terminals, so its process exit is nonzero when those
negative cases are observed; inspect each JSON trial's `expected_terminal_matched`
and the ledger's exact terminal before treating that as an unexpected failure.
The tool notifications that close a turn can arrive after the `session/prompt`
response, so the trial drains the notification stream before it counts tool
calls; otherwise the same turn could report a different number of tool calls
from run to run.

```bash
python3 -m dev.harness_lab improve --proposer manual --max-iterations 1 --dry-run
```

A real OpenCode proposal
requires an explicitly supplied dedicated linked worktree, prompt, allowed path
prefixes, and uses the version-qualified `opencode run --auto --format json`
adapter. Candidate edits remain in that worktree; accepted state still requires
independent build, ACP, and paired-ledger gates.

After a baseline suite is present in the ledger, the local candidate gate is:

```bash
python3 -m dev.harness_lab candidate-worktree \
  --repository . --branch harness/candidate-001 \
  --base-revision <frozen-source-revision> \
  --destination /tmp/codegg-candidate-001

python3 -m dev.harness_lab qualify \
  --worktree /tmp/codegg-candidate-001 \
  --suite dev/harness_lab/fixtures/smoke.jsonl \
  --ledger /path/to/frozen-experiment-ledger \
  --baseline baseline --candidate candidate-001 \
  --target-dir /tmp/codegg-candidate-target-001 \
  --allow-prefix src/ --allow-prefix crates/ --proposer manual
```

The worktree command creates a new local branch at the exact requested commit
outside the source checkout. The candidate must include `CHANGELOG.harness.md` with Root cause, Contracts,
Validation, and Accepted risks sections. The gate checks the linked non-main
worktree and scoped diff, runs formatting, focused ACP tests and a separately
targeted candidate build, evaluates the frozen suite against private ACP lanes,
and verifies the original baseline manifests and index prefix are unchanged.
The result is a qualification receipt; it does not switch branches or accept,
merge, or push the candidate. Manual mode and the deterministic fixture require
no OpenCode credentials or paid model call.
To invoke the optional proposer from this command, set `--proposer opencode`
and supply a bounded `--prompt`; the CLI version is pinned to 1.19.3. `--model
<provider>/<model>` pins the proposal to one approved route, and the receipt
attributes the proposal to that exact route; without it the CLI's own unpinned
default is used and the receipt says so. `--budget <policy.json>` declares the
spend authority for a live proposal and `--live-provider` declares that the run
reaches a live provider; the policy is admitted before the first provider
request and enforced during the run. A bounded
compile-correction retry can be enabled with `--max-candidate-retries N`; each
retry consumes a separate proposal attempt, is checkpointed, and reruns the
candidate gates. This option requires the OpenCode proposer. A
checkpoint binds the candidate patch, baseline and suite. With `--resume`, an
interrupted build/evaluation can continue only after those immutable inputs and
the recorded candidate binary match. Partial ledger evaluations continue only
the missing task/repeat pairs after validating every existing row against the
frozen suite. The qualification runs the controller's own harness rather than
any copy of it that happens to sit in the candidate tree: the measuring
instrument is the one the controller owns, so a candidate cannot measure itself.

For a bounded outer assess/propose/build/retest loop, use `optimize` after a
baseline ledger is complete:

```bash
python3 -m dev.harness_lab optimize \
  --repository . \
  --suite dev/harness_lab/fixtures/smoke.jsonl \
  --ledger /path/to/frozen-experiment-ledger \
  --baseline baseline \
  --candidate-root /tmp/codegg-optimization-candidates \
  --target-root /tmp/codegg-optimization-targets \
  --checkpoint /tmp/codegg-optimization.json \
  --allow-prefix src/ --allow-prefix crates/ \
  --max-iterations 3 --max-candidate-retries 1 \
  --max-wallclock 3600 --timeout 900 \
  --prompt 'Assess the frozen baseline failures and propose a scoped correction.'
```

The controller creates a dedicated candidate branch/worktree for each
iteration, keeps all iterations pinned to the baseline revision and suite, and
stops at the first candidate that completes every expected outcome without a
regression and improves at least one expected correctness result. It never
accepts an incomplete or unpaired run. Each event is hash-linked to an
append-only JSONL log and mirrored by an atomic pointer. Restart with identical
arguments plus `--resume`; the original wallclock deadline is retained. An
ambiguous interrupted proposal is preserved and refused for manual inspection.
Accepted code remains in its candidate worktree and is never merged or pushed.

The candidate gates run in a fixed order: format, **build**, focused test. The
build gate precedes the focused test gate on purpose. The focused test gate
compiles the same crate, so a test-first order surfaces a compile failure as a
*test* failure and the bounded compile-correction mechanic — whose whole
purpose is to correct a compile failure — is never reached. When the build gate
fails and the proposer is OpenCode, the compiler's own diagnostic is handed to
the proposer, its correction is scope-checked and re-gated, and the retry count
is bounded by `--max-candidate-retries`. The order is named in
`CANDIDATE_GATE_SEQUENCE` and asserted by a test, so it cannot silently
regress.

An optional `--output-dir` must be empty and outside the candidate worktree.
Without it, the supervisor creates an OS temporary experiment directory and
removes it after cleanup. The output is a bounded JSON receipt on stdout;
ACP stderr tails are capped at 64 KiB per child.

## The score contract

`dev/harness_lab/scoring.py` holds the versioned contract that decides whether a
measured candidate is an acceptable improvement. It is deliberately standalone
and pure — no I/O, no clock, no randomness, no network — and it lives under a
path candidate patches may not touch, so a candidate cannot edit the contract
that judges it.

An acceptance rests on a **substantive** improvement: a criterion that declares
`improvement_accepts` must have improved, and no blocking regression floor may
have regressed. In the default contract that criterion is
`correctness.pass_rate` alone, so a candidate is accepted for a **verified
correctness improvement** — a non-control task that failed at baseline now
meets its expected terminal.

Everything else is measured and reported, but cannot *be* the acceptance:

| Criterion | Measured | Carries an acceptance |
|---|---|---|
| `correctness.pass_rate` | yes | **yes** |
| `reliability.success_rate` | yes | no |
| `efficiency.duration_ms` (advisory) | yes | no |
| `control.no_regression` (protected) | yes | never — it is a guard |

So a candidate that merely got marginally faster is **rejected**
(`no_substantive_criterion_improved`), and so is one that removed a single
avoidable tool failure while leaving the task failing. Both movements are
still reported in the receipt's reasons, so nothing is hidden — they simply do
not buy an acceptance. A contract that genuinely runs a reliability or latency
campaign declares that intent by setting `improvement_accepts` on the
criterion it optimizes.

Two rules keep the metrics honest:

- A **protected negative control** that regresses rejects the candidate
  outright, and it never contributes to an improvement, an objective, or the
  weighted aggregate.
- The **reliability rate's denominator excludes** every case whose declared
  `expected_terminal` is itself an avoidable failure. The
  `repeated-identical-read-error` control must fail; counting its expected
  failure as an avoidable failure would let a candidate that stopped the
  control from failing buy a reliability improvement.

A missing measurement is never read as zero: an unmeasured criterion yields
`insufficient_evidence` with no score. The contract's version moves when its
decision semantics change, so an older contract is refused rather than
reinterpreted.

## Live pilot and sustained scenarios

`pilot` runs one live ACP turn against a real provider route and records the
C001 failure/usage provenance for it:

```bash
python3 -m dev.harness_lab pilot \
  --binary target/debug/codegg \
  --task worklog --model opencode_go/muse-spark-1.3-contributor \
  --budget /path/to/budget.json --live-provider
```

The pilot's `--model` is a **CodeGG** provider route (`opencode_go/…`), because
the turn runs through CodeGG's own provider dispatch. It is not the same
namespace as the OpenCode CLI route the proposer uses (`opencode-go/…`): the
CLI has its own provider set, and a model id that is routable by one is not
necessarily routable by the other.

The pilot runs against a private lane daemon, so the lane's daemon is stopped
after every trial; a failure to stop it is recorded in the receipt as
`daemon_stop_error` rather than swallowed. A lane whose socket path would
exceed the platform's `sun_path` limit is refused: the lane root lives under
`~/.codegg-harness-lanes`, not under the system temporary directory.

`sustained` runs a repeated scenario over the same tasks and reports the
repeatability, the failure attribution and the cleanup evidence:

```bash
python3 -m dev.harness_lab sustained \
  --binary target/debug/codegg --trials 24 \
  --model opencode_go/muse-spark-1.3-contributor \
  --budget /path/to/budget.json --live-provider
```

`sustained` can inject a `cancel` or `abrupt_acp_drop` fault into a trial so
the interruption handling is exercised rather than assumed. The cleanup
evidence is **observed**, never asserted: the receipt reports how many lane
daemons survived the scenario, and a non-zero survivor count is a reported
fact, not a failure to hide.

## The autonomy taxonomy

`dev/harness_lab/fixtures/agent_autonomy/cases.jsonl` freezes the taxonomy: one
line per case, with the prompt the agent receives, the tools it may use, the
verifier's command and working directory, and the budget and abort criteria.
The loader refuses an unknown family, a malformed case, or a case whose
verifier scope overlaps the tree the agent may edit.

Each supported case has a body under `dev/autonomy_fixtures/<case_id>/`:

```
fixture/      the agent's editable tree — the task's files
verifier/     the independent verifier — never inside fixture/
```

The verifier re-derives the contract instead of trusting the fixture's own
tests, and it is the **only** thing that decides whether a case was solved. A
case body is not trusted on its own: `dev/harness_lab/tests/test_autonomy_cases.py`
proves, by execution, that every shipped verifier fails on the unsolved case,
passes on the repaired case, and refuses an edited frozen file.

Running a case live:

```bash
python3 -m dev.harness_lab autonomy \
  --binary target/debug/codegg \
  --model opencode_go/muse-spark-1.3-contributor \
  --budget /path/to/budget.json --live-provider
```

Before a live H006 run, build the complete runfile pair into the same target
directory with the same pinned Rust toolchain:

```bash
cargo build --offline --locked --bin codegg --bin codegg-sandbox-helper
```

Then require `codegg doctor installation` to report the sandbox helper as present.
The Python `dev.harness_lab doctor --binary` command checks only the executable
path and digest; it does not check managed sidecars. The H006 autonomy runner
also performs the native installation preflight before reserving a provider
request.

The runner copies the case's `fixture/` into a private lane workspace and
keeps the verifier **outside** that workspace, so the agent under test can
neither read the hidden contract nor edit the thing that judges it. It records
a transcript of what the agent actually did — the tool sequence, each call's
argument **digest**, and each call's terminal status — and hands that
transcript to the case's own verifier. The harness never projects raw tool
arguments, so a verifier can tell "the same call again" from "a different
call" without reading the call's payload.

A case counts as solved only when the turn reached a terminal **and** the
verifier confirmed it. A case the verifier never ran is not a solved case.

The verifier runs in the host environment, not the subject's isolated lane
environment: the lane deliberately overrides `HOME`, `CARGO_HOME` and the
config roots so the subject under test cannot see the developer's registry or
credential store, but a rustup shim with an overridden `HOME` resolves no
default toolchain, and every Rust case would fail for a reason that has
nothing to do with the agent.

Comparing a baseline binary with a candidate binary over the taxonomy:

```bash
python3 -m dev.harness_lab autonomy-compare \
  --baseline-binary /path/to/baseline \
  --candidate-binary /path/to/candidate \
  --model opencode_go/muse-spark-1.3-contributor \
  --budget /path/to/budget.json --live-provider
```

Both sides run the same cases in their own lanes and are judged by the shared
score contract. The protected negative control is scored separately: its
expected outcome is that the task is **not** solved, so a control that starts
being credited as solved is a safety regression and rejects the candidate
whatever else moved.

### A005 placement qualification

The frozen A005 workload lives in
`dev/harness_lab/fixtures/a005_placement/cases.jsonl`. It covers dependency-heavy
Rust inspection, isolated Rust edit/build, Python inspection and edit/debug, a
Spanish request with the same isolated-edit intent, and refusal for an exact
managed-worktree ID that is unavailable in the lane. Its verifier reads only
allowlisted ACP fields: requested placement enum, whether an exact ID was
provided, accepted placement rationale, and terminal child-run status. It
never receives the ID or raw task arguments.

The A005 baseline and candidate use the same current binary and the same
placement authority. The baseline arm hides the strategic placement guidance
from the task schema and retains the compatible A002 contract; the candidate
arm receives the full conservative advisor. This isolates the advisor effect
without comparing against a binary that lacks A002 or allowance enforcement.
Each receipt names whether the advisor was enabled. Run the preregistered
three-repetition comparison with:

```bash
python3 -m dev.harness_lab autonomy-compare \
  --manifest dev/harness_lab/fixtures/a005_placement \
  --baseline-binary target/debug/codegg \
  --candidate-binary target/debug/codegg \
  --model opencode_go/muse-spark-1.3-contributor \
  --repetitions 3 --seed 20261028 \
  --budget /path/to/a005-budget.json \
  --lane-root /path/to/private/a005-lanes \
  --output /path/to/a005-comparison.json --live-provider
```

One shared guard covers all 36 randomized trials and counts requests at the
provider boundary. The Go-reported allowance is accumulated across both arms;
missing allowance observation refuses any next request. Receipts include
sampled process-tree CPU and peak RSS plus lane disk growth. The scheduler does
not currently publish a per-build queue-wait value through ACP, so that field
remains explicitly unavailable rather than inferred from task submission time.

A protected control only means something when the situation it is about really
happened. A control that was **never exercised** — the agent never put the
harness in that situation — is excluded from the control rate rather than
counted as a control that failed to hold, and when **no** control was
exercised the rate is **unmeasured**, not zero: "we did not see it happen" is
not "it went wrong". The comparison names the controls that never ran.

### Inducing a failure the agent cannot walk out of

A case may declare a **fault**. A fault is a property of the lane's
**environment**, never of the prompt: the harness applies it to the lane's own
copy of the fixture before the prompt is submitted, so the agent meets a
deterministic failure at the tool boundary while the instruction never names
it. A control that taught the model to loop would measure the model, not the
recovery ladder.

```json
{"tool": "read", "path": "fixture/report.txt", "mode": "unreadable",
 "min_consecutive_failures": 2, "max_consecutive_failures": 3}
```

- `mode` is `unreadable` (the target keeps its path and loses its read
  permission) or `missing` (the target is moved out of the workspace).
- `min_consecutive_failures` is what the control needs to observe for the
  fault to have been exercised.
- `max_consecutive_failures` is the **bound**: when that many consecutive
  identical failures have been observed, the runner stops the trial, so a
  faulted case is bounded even when the agent never gives up.

Every rule in the seam is a **refusal**, not a repair:

- only `read` is accepted — the seam is the filesystem, and a tool the
  filesystem cannot fail deterministically has no seam here;
- a declared fault is applied or the trial is refused. It is never ignored
  and never softened;
- the fault lives inside the lane's private workspace and nowhere else, and
  it is restored when the trial ends;
- a fault the agent undoes mid-trial (`chmod`) is put back in force.

The receipt records the fault, what the transcript shows about it, and
whether the declared bound stopped the trial.

### How a failed tool is observed

The product publishes a **failed** tool as a *successful* call whose output
carries the error: the broker wraps the tool's error in an error-marked value,
so the ACP wire reports `status: "completed"` for a call that in fact failed,
and the projection's `ToolFailed` variant has no producer. The ACP status
alone therefore cannot tell a failed tool from a successful one, and a fault
would be invisible to the harness.

The harness reads the product's error envelope out of the reported output and
carries the answer as a boolean, so the reported output itself never reaches a
receipt. The coupling to the product is **guarded**: `tests/test_autonomy_faults.py`
asserts that every envelope the harness recognises is still the product's
envelope, so a change to the product's error reporting fails the harness suite
instead of quietly leaving every control unexercised.

Two different counts are reported, and they mean different things:

- `observed_failures` — how many times the faulted tool failed at all. A tool
  that failed once and was not repeated produced **one** observed failure.
- `longest_consecutive_run` — the longest run of *identical* failures with
  nothing in between. This is the measurement: a repeat is a repeat only when
  the identical failing call followed itself. A success between two failures
  ends the run.

`exercised` is keyed on the consecutive run, so a control the agent never
reached is **never exercised** and is excluded from the control rate. The
fault watcher's bound is keyed on the total, because the watcher's job is to
bound a faulted trial even when the agent interleaves other calls.

## Unmeasured trials, bounded re-runs and cumulative budgets

A trial that produced **no observation** says nothing about the candidate.
Two cases produce none: the harness, the daemon or the transport failed
before the trial could produce an observation (`infrastructure_error`), and
the event stream reported a **gap** — the daemon's event log is a bounded
broadcast, so a connection that falls behind it loses events, and the product
now says so (`codegg_event_gap`).

An unmeasured trial is **not** a candidate failure. It is excluded from the
reliability denominator and the pair is re-run — under a bound:

- `--max-measurement-retries` caps how many times one pair may be re-run
  after a measurement failure, **across every resume**. The count comes from
  the ledger's own rows, so it survives a resume.
- the cap is also enforced against the provider-request budget: every attempt
  is admitted through the spend guard, which refuses before the ceiling is
  crossed.
- the receipt reports, per pair, the number of attempts, the number of
  measurement failures and the terminal of the measured row.
- a pair that exhausts the cap is recorded as **unmeasured and unresolved**:
  never as a pass, never as a zero, and never as a silently dropped trial.

The wallclock a qualification may consume is **cumulative**: it is measured
from the start of the first attempt, across every resume. The consumed figure
is derived from recorded evidence — the candidate checkpoint's own
accumulated elapsed time, plus the measured durations of the trials the
checkpoint does not cover — never from a value the caller asserts. A
checkpoint that carries **no** elapsed-time accounting is a **refusal**, not a
fresh budget: handing a resumed qualification a new full-length budget is
exactly the unboundedness the cumulative deadline exists to remove.

The `sustained` command has an optional `--scenario-wallclock`: the scenario
stops before it crosses the cumulative limit and caps each trial's timeout by
what is left.

The comparison reports the **measured values** next to the verdict, so a
reader can see what moved and what did not. A verdict without its numbers is
an assertion.

Re-deciding a recorded pair after a score-contract correction does not re-run
the provider:

```bash
python3 -m dev.harness_lab autonomy-rescore \
  --baseline-runs recorded-baseline.json --candidate-runs recorded-candidate.json
```

The recorded trials are the evidence; the decision is a derivation from them,
so a derivation that changes when the contract changes is expected, while a
*trial* that changes is not. The re-score never runs anything.

## Budget admission before a live trial

A trial that reaches a live provider must present a budget policy whose hard
cap the controller verified. The gate is fail-closed and runs before any
provider request, daemon spawn, or subprocess launch:

```bash
python3 -m dev.harness_lab budget-check --budget /path/to/budget.json
python3 -m dev.harness_lab tool-trial --binary target/debug/codegg \
  --profile workspace-write --live-provider --budget /path/to/budget.json
```

`budget-check` is provider-free: it performs no I/O, makes no request, and
spawns nothing. It prints the admission decision and exits `0` when admitted
and `2` when refused.

A budget policy is a JSON object with at least one enforceable hard cap:

| Key | Meaning |
|---|---|
| `token_limit` | Maximum provider tokens for the trial. |
| `cost_limit_usd` | Maximum spend. Requires `cost_enforcement`. |
| `cost_enforcement` | `controller_enforced` or `provider_enforced`. Never inferred. |
| `cost_observable` | Whether the route reports a cost at all. A `controller_enforced` cost cap requires this to be `true`. |
| `max_provider_requests` | Maximum provider requests. `1` is valid. |
| `max_wallclock_seconds` | Maximum wall-clock seconds. |
| `provider_quota` | A provider-authoritative ceiling: `source`, `enforceable`, `retry_safe`, `scope`. |

Refusal reports `status: "budget_unverifiable"` and a `missing_source` naming
what could not be verified: `token_limit`, `cost_limit_usd`,
`max_provider_requests`, `max_wallclock_seconds`, `provider_quota`, or
`provider_quota_retry_safety`.

Three rules are strict on purpose:

- **A cost cap with no stated enforcement is refused.** A guessed or locally
  estimated price is never treated as an enforceable spend limit, and a
  requested-but-unenforceable cap refuses the trial rather than being silently
  ignored.
- **A controller-enforced cost cap needs an observable cost.** The controller
  can only hold a dollar ceiling it can measure, so `cost_observable` must be
  `true`. A route that reports no cost cannot be enforced against one. For
  OpenCode Go, the live route uses provider-reported `opencode.allowance` and
  labels the receipt source `opencode_go_allowance`; this is quota value, not
  cash charged.
- **A provider quota must be retry-safe.** A ceiling that resets on a provider
  retry does not bound the trial, so it is refused with
  `provider_quota_retry_safety`.

### What "enforceable" means at run time

The gate and the run agree. `max_provider_requests` is admitted before a trial
and enforced at the OpenCode Go provider boundary before every request. The
controller reserves each trial and reconciles it with the provider-boundary
receipt, so a failed or cancelled trial still counts. OpenCode Go allowance is
observed after each response; once the configured remaining allowance is
reached, the provider refuses another request. A single in-flight response
can cross the allowance ceiling before its usage arrives; no later request is
admitted. The aggregate is reported under `budget.cost_usd` with
`cost_source: "opencode_go_allowance"`.

## Failed-turn evidence

A native turn failure reaches the client as the ACP JSON-RPC error `-32010`.
Its `data` payload carries the redacted failure class and the usage
provenance:

| Field | Meaning |
|---|---|
| `codeggFailureCategory` | One allowlisted category, or `native_failure_unspecified`. |
| `codeggUsage` | Token counts observed before the failure, or `null`. |
| `codeggUsageReason` | `reported`, `not_observed`, `not_provided`, `lost_at_projection`, or `unspecified`. |
| `codeggUsage.providerAllowanceUsd` | OpenCode Go's provider-reported quota allowance for the run, distinct from cash charged. |

`codeggUsage` is `null` whenever no count was available, and the reason says
which of the distinguishable facts it was. A missing measurement is never
recorded as zero usage: the ledger stores a named `usage_unavailable` reason
instead.

The payload never carries the provider's error text. The category is derived
from the typed native error and the usage aggregate covers every provider
response in the failed run.

## Mutable path inventory

Each lane receives unique values for:

| State | Lane setting/path |
|---|---|
| Daemon lock, metadata, socket and log | `CODEGG_DAEMON_HOME=<lane>/daemon` |
| SQLite catalog and session/provider state | `CODEGG_DATA_HOME=<lane>/data`, resolved below `data/codegg` |
| Global config, agents and file credentials | `HOME`, `XDG_CONFIG_HOME`, `CODEGG_TUI_CONFIG` |
| XDG runtime/data/cache/state | `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME` |
| Tool scratch and Cargo cache | `TMPDIR`/`TMP`/`TEMP`, `CARGO_HOME` |
| Workspace-local state | private temporary workspace fixture |
| Child output | lane-local bounded ACP stderr file |
| Taxonomy fixture (the agent's editable tree) | `<lane>/case/fixture` |
| Taxonomy verifier and transcript | `<lane>/private/verifier`, `<lane>/private/TRANSCRIPT.json` |

The lane's daemon control socket lives at `<lane>/daemon/core.sock`, and the
platform's `sun_path` limit is short. The runner refuses a lane whose socket
path would not fit, with the reason, rather than letting the daemon fail with
a message that names the socket instead of the cause.

The smoke makes no provider call, so no credentials are needed. It does not
copy a developer's keychain or provider credentials into the lane. `RUSTUP_HOME`
and the executable runtime itself may be read from the installed toolchain;
the smoke does not write them. `CODEGG_CORE_ENDPOINT` is removed so ACP uses
the endpoint resolved from its unique daemon root. Linux and macOS are the
qualified host targets; other platforms fail before starting children.

This is state separation on a trusted workstation, not OS security isolation.
Full-host execution remains the OS user's authority. Do not run untrusted
candidate code in this harness. The controller owns only the subprocesses and
temporary roots it creates, and it refuses to infer daemon identity from PID
or stale metadata alone.
