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

The evaluator runs the versioned 12-case task manifest and writes versioned
immutable run manifests, sequenced redacted event rows, captured verifier
artifacts, and an append-only `index.sqlite3` query index cross-checked against
the JSONL index. Cases cover write/edit/read/grep, distinct repeated reads, projected
output recovery through `context_read`, shell execution/recovery and bounded
identical failures, both policy profiles, explicit denial, cancellation,
timeout, and an intentionally failing verifier. `report`, `replay`, and `compare` read
that ledger offline. The projection case forces a large native `read` result
through its artifact handle, then requires `context_read` to return a tail
marker omitted from the initial model-visible projection. Comparison pairs task IDs and repeat indexes only when
group, suite digest, and case digest match. Unknown token, cost,
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
and supply a bounded `--prompt`; the CLI version is pinned to 1.19.0. A bounded
compile-correction retry can be enabled with `--max-candidate-retries N`; each
retry consumes a separate proposal attempt, is checkpointed, and reruns the
candidate gates. This option requires the OpenCode proposer. A
checkpoint binds the candidate patch, baseline and suite. With `--resume`, an
interrupted build/evaluation can continue only after those immutable inputs and
the recorded candidate binary match. Partial ledger evaluations continue only
the missing task/repeat pairs after validating every existing row against the
frozen suite.

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

An optional `--output-dir` must be empty and outside the candidate worktree.
Without it, the supervisor creates an OS temporary experiment directory and
removes it after cleanup. The output is a bounded JSON receipt on stdout;
ACP stderr tails are capped at 64 KiB per child.

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
