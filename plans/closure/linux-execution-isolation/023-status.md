# C023 — M005 Obtained Network Fact Contract — Status

Status: closed — the per-launch obtained network fact is reported, the C007
receipt states the boundary it ran under, and the audit record no longer
asserts a boundary nobody observed. M005 remains **active** on its remaining
gates, ADR-0044 remains **proposed**, and no host is admitted.

Plan:

- `plans/implementation/linux-execution-isolation/023-m005-obtained-network-fact-contract.md`

Baseline reviewed: `78359fd3cfc71b0319d323a879bfcfe0768a719c`

## 1. Executive finding

M005 obtains a private network namespace in reality — Bubblewrap runs the child
with `--unshare-net` inside a namespace with no host interfaces or routes, and
the hosted proof demonstrates the namespace is really there. **Nothing reported
it.** `NetworkEnforcement::Enforced` had no producer in the tree: the only
values ever constructed were `Unrestricted` and `Unavailable`.

Three surfaces therefore understated what the system obtained:

| Surface | Before | After |
|---|---|---|
| `SandboxEnforcement::for_constrained_enforced` | hard-coded `network: Unrestricted` | the launch's own obtained fact |
| `SandboxExecutionOutcome` | no network field at all | typed `network: NetworkEnforcement` |
| The C007 publication receipt | `network boundary: not_reported` | the boundary the staging actually obtained |

This was C014's first open item and the reason ADR-0044 clause 2's M005
precondition was vacuous: clause 2 requires an M005 `Enforced` claim, and the
only M005 claim that existed was a hoped-for one.

**What this closes.** The obtained fact is now reportable, per launch, and the
C007 receipt states it. What it does **not** do is admit a host, close M005, or
accept ADR-0044. A network boundary belongs to a launch, not to a profile, so
the per-launch report is the whole of the claim; the policy snapshot still
reports that it cannot know.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Verdict |
|---|---|---|
| A launch that requested the network namespace and observed its own setup completing reports `Enforced` with the mechanism named | `src/managed_process.rs::obtained_network_enforcement`; unit test `the_obtained_network_boundary_needs_both_the_receipt_and_the_request` | pass |
| A launch that did **not** request the namespace reports `Unrestricted`, even though its filesystem containment was obtained | unit test `a_launch_that_did_not_request_the_namespace_never_reports_it` — the anti-overclaim case | pass |
| A status stream without a `child-pid` is not a setup receipt and cannot support a claim | unit test `a_status_stream_without_a_child_is_not_a_setup_receipt`; `decode_bubblewrap_json_status` returns `Err` | pass |
| The receipt's boundary is read off the launch, never off the request | `ObtainedNetworkBoundary::from_launch`; unit test `the_receipt_boundary_is_read_off_the_launch` | pass |
| Every boundary has a producer | `Enforced` ← an obtained launch; `Unrestricted` ← a launch that obtained no namespace; `NotReported` ← a staging step that runs no contained child | pass |
| The policy snapshot's summary never contradicts its own network field | `SandboxEnforcementDto::summary_for` + `with_network`; unit test `the_enforcement_summary_never_contradicts_its_fields` | pass |
| The legacy Landlock route still reports truthfully | it reports `Unrestricted` — Landlock scopes the filesystem and has no network dimension | pass |
| The C007 publication suite still passes end to end | `cargo test -p codegg --locked --test git_index_publication` → 22 passed | pass |
| On a qualified host the hosted C007 proof reports the receipt stating `enforced` | hosted run `39356728579` (commit `862ddd14`, the delivered source): `test production_private_index_publisher_stages_and_publishes_under_containment ... ok` in the isolated C007 step and again in the 17-test M004/M005 suite, with the test asserting `ObtainedNetworkBoundary::Enforced { backend: "bubblewrap-netns" }` | pass |

## 3. Production implementation evidence

### 3.1 The observation, and why the request alone is not evidence

Bubblewrap writes a `child-pid` status record only after its namespace and
mount setup has completed and the sandbox child has been forked. A requested
namespace that cannot be created aborts the launch **before** that child
exists, so the status stream then carries no `child-pid` and the launch fails
closed.

`obtained_network_enforcement(spec, child_pid)` therefore requires both halves:

- receipt observed **and** `--unshare-net` requested → `Enforced { backend:
  "bubblewrap-netns" }`;
- receipt observed **and** the namespace not requested → `Unrestricted`.

The request alone is a hope. The receipt alone does not say which namespaces
were created. The claim is made only when both are present.

### 3.2 The carrier

`SandboxExecutionOutcome::Enforced` gained a typed `network:
NetworkEnforcement`. It has exactly two construction sites — the legacy
Landlock route (`Unrestricted`, truthfully) and the Bubblewrap route (the
producer above). Every read site matched with `..`, so the outcome's shape did
not change for its consumers.

### 3.3 The snapshot contradiction

`SandboxEnforcementDto::for_profile_on_host` built its summary from the
filesystem field alone and hard-coded the network clause "network unrestricted
(no OS isolation)". The daemon then replaced the network field with
`Unavailable` and **appended** a note, producing one sentence that said both
"network unrestricted (no OS isolation)" and "OfflineRequired requested".

The summary is now derived from the facts it describes (`summary_for`), and
`with_network()` rebuilds it when the network fact changes. The summary for the
unchanged cases is byte-identical to before, so no frontend assertion moved.

### 3.4 The C007 receipt

`ObtainedNetworkBoundary` gained the reported case **with its producer**.
`GitIndexStaging::stage` returns the boundary it obtained, and the publisher
copies it into the receipt verbatim. The copy reads the launch's obtained fact,
so the receipt cannot claim a boundary the staging did not obtain.

The staging trait is the C007 trust boundary: the production implementation
runs the contained child and reports what the launch observed; the test
implementation reports `NotReported`, which is a *statement*, not an omission.

### 3.5 The audit record asserted a boundary nobody observed

The obtained fact was not only missing from the receipt — the audit record
**asserted** one. `BashTool::emit_command_audit` passed this for the audit's
network field:

```rust
ctx.execution_authorization.as_ref().map(|_| "unrestricted")
```

so every authorized command was recorded as `execution.network =
"unrestricted"`. That was wrong in both directions: it understated a launch
that obtained a network namespace, and it asserted a fact for the paths that
never launched anything at all — the scheduler-background submission, the
executor dispatch, and the two timeout events.

The audit record already had a separate `execution.network` metadata slot and
nothing filled it. It is now filled from the launch's own obtained fact:

- the completed dispatch records `enforced:<backend>` when the launch obtained
  one, `unrestricted` when it did not, and `unavailable` when it could not be
  determined. `unavailable` is deliberately **not** `unrestricted`: reporting
  "no isolation" for a boundary nobody observed would be a claim, and a claim
  in an audit record is worse than an absence;
- the paths with no observed launch record nothing — the field is omitted
  rather than filled with an assumption.

The hard-coded assertion is now structurally impossible: `emit_command_audit`
takes the network field as a parameter, so every call site states it
explicitly.

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --locked --test git_index_publication -- --test-threads=1
#   -> 22 passed; 0 failed
cargo nextest run --workspace --locked --profile ci --no-fail-fast
#   -> 14877 run: 14874 passed, 3 timed out (load-related LSP baseline, pass in isolation), 7 skipped
#      (the authoritative quiescent sweep; an earlier one failed 14 tests that do not
#       reproduce -- recorded in §5, not hidden)
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
scripts/verify.sh quick
#   -> "Quick verification passed."
```

### The workspace sweep

See §5.

## 5. The workspace sweep result

```text
cargo nextest run --workspace --locked --profile ci --no-fail-fast
Summary [ 552.455s] 14877 tests run: 14874 passed (1 slow), 3 timed out, 7 skipped
```

This is the **authoritative** sweep: it ran against a quiescent tree, with
nothing else touching the worktree or the shared repository while it executed.

The count moved from the previous sweep's 14,869 by exactly the **+8** tests this
corrective adds (4 in `src/managed_process.rs`, 2 in `src/git_publication/mod.rs`,
1 in `crates/codegg-protocol/src/core.rs`, and 1 in `src/tool/bash.rs`).

The three timeouts are **timeouts, not assertion failures**, and none is in a
file this corrective touches:

```text
TIMEOUT [ 122.974s] codegg tool::lsp::tests::security_context_call_expansion_scenarios_share_one_service
TIMEOUT [ 125.663s] codegg tool::lsp::tests::security_context_preset_scenarios_share_one_service
TIMEOUT [ 125.713s] codegg tool::lsp::tests::security_context_call_expansion_truncated_limit_field_present
```

Re-run in isolation, all three **pass**:

```text
cargo nextest run --workspace --locked --profile ci --no-fail-fast \
  -E 'test(security_context_call_expansion_scenarios_share_one_service) or test(security_context_call_expansion_truncated_limit_field_present) or test(security_context_preset_scenarios_share_one_service) or test(cancellation_and_overall_timeout_are_bounded)'
Summary [ 106.536s] 4 tests run: 4 passed (3 slow), 14880 skipped
```

The three LSP tests each take ~102–107 s against a 120 s limit, so they time
out when the workstation is contended and pass when it is not — the same
load-related baseline recorded in the previous sweep, where two of the three
timed out. The `eggpool` timeout is the same class: it takes **0.273 s** in
isolation and 120 s under contention. None of the four is in a file this
corrective changes, and none is an assertion failure.

### An earlier sweep failed 14 tests that do not reproduce — recorded, not hidden

One earlier sweep on the same tree failed **14** tests that the authoritative
sweep passes:

```text
codegg tool::bash::tests::active_mode_routes_git_to_native_tool
codegg tool::bash::tests::route_mode_alias_uses_active_git_route_with_accepted_authority
codegg::command_routing_execution_ownership active_git_readonly_routes_to_git_caller_owned
codegg::command_routing_execution_ownership one_logical_execution_produces_exactly_one_record
codegg security::workflow::report::tests::security_review_*   (10 tests)
```

The git-routing four failed with a typed refusal
(`git execution policy outcome: Git backend unavailable for [RepositoryAlias]`,
the classification for an unrecognised Git subcommand) and the security-review
ten with `assertion failed: result.is_ok()`. **All fourteen share one
requirement: a working Git repository at the process cwd** — the routing tests
run `git status` against the repository, and the security-review command runs a
Git diff against `HEAD` in the same repository.

Reproduction attempts, each negative:

- in isolation, 4/4 pass — and across **six further loop iterations, 24/24**
  pass;
- under concurrent writes into the worktree, 4/4 pass;
- in the quiescent sweep, 0 failures.

The cause was **not established**. What is known: the repository is a *shared*
one — this worktree is a linked worktree of `/home/sugarwookie/projects/codegg`,
which has 17 linked worktrees and is operated on by other agents — so a
transient Git failure in the shared repository would surface exactly this way.
That is a hypothesis, not a finding, and it is not offered as one.

What *is* established: the path these tests exercise (command-intent
classification → Git service policy → the security-review workflow) is **not
modified** by this corrective, whose changed files are `src/managed_process.rs`,
`src/git_publication/**`, `src/git_mutations.rs` (the `Add` arm and the receipt
summary), `src/tool/bash.rs` (the audit label), `src/core/daemon_ops.rs`,
`crates/codegg-core/src/approval.rs`, and `crates/codegg-protocol/src/core.rs`.
The failure signature `BackendUnavailable[RepositoryAlias]` is also a
**documented historical failure mode** in this repository — C005's plan names
`git_execution_origin_matrix` row-3 `BackendUnavailable[RepositoryAlias]`
among the historical Git mutation failures to freeze. It is recorded here
rather than waived, and it is not reported as a pass.

### The narrow C007 suite

```text
cargo test -p codegg --locked --test git_index_publication -- --test-threads=1
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.58s
```

### The gate

```text
scripts/verify.sh quick
Quick verification passed.
```

### The hosted run at the branch's final source

Run `39356728579` (2026-10-20, push, commit `862ddd14`) is a **success** for
both qualification jobs, and it is the run that covers the delivered source.
Its log is the evidence, not the badge:

```text
process-ipc  C007 private-index publication under containment
  running 1 test
  test production_private_index_publisher_stages_and_publishes_under_containment ... ok
  test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 16 filtered out; finished in 0.70s

process-ipc  M004 managed-process host PID and IPC denial probes
  running 17 tests
  test managed_process_owner_applies_authenticated_offline_network_intent ... ok
  test production_private_index_publisher_stages_and_publishes_under_containment ... ok
  test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 625.48s

restricted-userns-fails-closed
  test required_process_tier_fails_closed_when_user_namespaces_are_restricted ... ok
  test m004_daemon_constrained_pty_fails_closed_when_user_namespaces_restricted ... ok
  test lsp::m004::tests::constrained_lsp_launch_fails_before_target_when_userns_are_restricted ... ok
```

**This is the obtained fact proven end to end.** The C007 test asserts that the
publication receipt states
`ObtainedNetworkBoundary::Enforced { backend: "bubblewrap-netns" }` — read off
the staging child's own launch. It passed on a qualified host, so the receipt
states a boundary the system really obtained, with the mechanism that obtained
it.

The non-vacuity check is repeated rather than carried over: the workflow
exports `CODEGG_RUN_M005_OFFLINE_NATIVE=1`, so the gate was open, and the
literal skip marker `skipped: set CODEGG_RUN_M005_OFFLINE_NATIVE=1` appears
**zero** times in the `process-ipc` log. The only `skipped:` lines belong to
two different gates — the restricted-userns probe and the C006 Bubblewrap
qualification. `process-ipc` took 34m25s of its 45-minute budget and
`restricted-userns-fails-closed` took 15m06s of its 35.

### The earlier hosted runs

Run `39351065569` (commit `d58ce82d`) was the first run against the obtained-
facts work and is also a **success** for both jobs, with the same C007 step
passing and the same `17 passed; 0 failed` suite.

Run `39349642026` (commit `b52e51df`) is recorded as **cancelled**, and it is
not read as a pass: the workflow sets `cancel-in-progress: true`, so pushing
the next commit cancelled the in-progress run, and the 17-test suite was cut
short. Its C007 step had already completed and passed — that is where the
receipt's `enforced` boundary was first observed on a qualified host — so the
run contributes its C007 step's result and nothing else.

## 6. What remains open

1. **M005 stays active.** The obtained network fact is reported now, but the
   tool-program child-job route is still unqualified (C014 open item 2) and
   there is still no machine-readable supported-host policy (C014 open item 3).
   No host is admitted by this corrective.
2. **ADR-0044 stays proposed.** This corrective removes one of the reasons
   clause 2 was vacuous — there is now an M005 `Enforced` claim that is
   actually obtained. It does **not** accept the ADR, and it does not touch
   the live-read consistency model that C006 reviewed.
3. **C010 no-admission and M003 blocked status are unchanged.**
4. **The hosted proof has run and passed.** Run `39356728579` (commit
   `862ddd14`) shows the C007 receipt stating the boundary its staging child
   obtained, on a qualified host (§4). The M005 skip marker is absent, so the
   test genuinely ran under the M005 gate rather than taking its own skip
   path.

## 7. Stop conditions

- If the hosted proof shows the receipt reporting anything other than
  `Enforced` on a qualified host, the producer's derivation is wrong and the
  claim is withdrawn rather than adjusted.
- If the obtained fact turns out to be derivable only from the request, the
  producer is removed rather than weakened.

## 8. Related records

- `plans/closure/linux-execution-isolation/022-status.md` — C014, whose open
  item 1 this closes.
- `plans/closure/linux-trusted-git-cost-corrective/007-status.md` — C007, the
  publication vertical whose receipt now states its boundary.
- `plans/closure/linux-trusted-git-cost-corrective/006-status.md` — C006, the
  ADR-0044 live-read review that found clause 2's precondition vacuous.
- `architecture/security.md` — the obtained-fact contract.
- `architecture/git.md` — the receipt's boundary contract.
