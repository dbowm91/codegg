# C023 — M005 Obtained Network Fact Contract — Status

Status: implemented on branch; the per-launch obtained fact is reported and the
C007 receipt states the boundary it ran under. M005 remains **active** on its
remaining gates, ADR-0044 remains **proposed**, and no host is admitted.

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
| On a qualified host the hosted C007 proof reports the receipt stating `enforced` | `tests/linux_m004_process_ipc.rs::production_private_index_publisher_stages_and_publishes_under_containment` asserts `ObtainedNetworkBoundary::Enforced { backend: "bubblewrap-netns" }` | **pending the hosted run** |

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

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg --locked --test git_index_publication -- --test-threads=1
#   -> 22 passed; 0 failed
cargo nextest run --workspace --locked --profile ci --no-fail-fast
#   -> 14876 run: 14873 passed, 3 timed out (load-related LSP baseline, pass in isolation), 7 skipped
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
Summary [ 562.384s] 14876 tests run: 14873 passed (1 slow), 3 timed out, 7 skipped
```

The count moved from the previous sweep's 14,869 by exactly the **+7** tests this
corrective adds (4 in `src/managed_process.rs`, 2 in `src/git_publication/mod.rs`,
1 in `crates/codegg-protocol/src/core.rs`).

The three timeouts are all in `src/tool/lsp.rs` — a file this corrective does not
touch:

```text
TIMEOUT [ 120.273s] codegg tool::lsp::tests::security_context_call_expansion_scenarios_share_one_service
TIMEOUT [ 120.892s] codegg tool::lsp::tests::security_context_call_expansion_truncated_limit_field_present
TIMEOUT [ 120.889s] codegg tool::lsp::tests::security_context_preset_scenarios_share_one_service
```

Re-run in isolation, all three **pass**, and each takes ~95 s:

```text
cargo nextest run -p codegg --locked --profile ci --no-fail-fast \
  -E 'test(security_context_call_expansion_scenarios_share_one_service) or test(security_context_call_expansion_truncated_limit_field_present) or test(security_context_preset_scenarios_share_one_service)'
Summary [  96.109s] 3 tests run: 3 passed (3 slow), 11470 skipped
```

Each is roughly 95 s of work against a 120 s limit, so they time out when the
workstation is contended and pass when it is not. This is the same
load-related baseline recorded in the previous sweep — where two of these
three timed out — and it is **not** a regression: the module is untouched, and
the failures are timeouts, not assertions.

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
4. **The hosted C007 proof must re-run** on this source to show the receipt
   stating `enforced` end to end. The assertion is in
   `tests/linux_m004_process_ipc.rs` and runs in the `process-ipc` job's
   isolated C007 step.

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
