# C023 — M005 Obtained Network Fact Contract

Status: implemented on branch; the per-launch obtained fact is reported, the
C007 receipt states the boundary it ran under, and the audit record no longer
asserts a boundary nobody observed. M005 remains **active** on its remaining
gates, ADR-0044 remains **proposed**, and no host is admitted.

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Closure record:

- `plans/closure/linux-execution-isolation/023-status.md`

Repository baseline reviewed: `78359fd3cfc71b0319d323a879bfcfe0768a719c`

## 1. Problem

M005 obtains a private network namespace in reality: Bubblewrap is launched with
`--unshare-net` and the child runs inside a namespace with no host interfaces or
routes. The hosted proof (`managed_process_owner_applies_authenticated_offline_network_intent`)
demonstrates that the namespace is really there.

**Nothing reported it.** `NetworkEnforcement::Enforced` had no producer anywhere
in the tree — the only values ever constructed were `Unrestricted` and
`Unavailable`. Three surfaces therefore understated what the system obtained:

1. `SandboxEnforcement::for_constrained_enforced` hard-coded
   `network: NetworkEnforcement::Unrestricted`, so a constrained launch reported
   "no network isolation" even when it had obtained one.
2. `SandboxExecutionOutcome` carried no network field at all, so the caller that
   launched a child — the only party positioned to know what the child got —
   had no way to learn the boundary.
3. The C007 publication receipt stated `network boundary: not_reported`, which
   was true of the receipt and false of the system: the boundary existed and the
   system could not say so.

This is the gap C014's closure carried as its first open item, and it is the
reason ADR-0044 clause 2's M005 precondition was **vacuous**: clause 2 requires
an M005 `Enforced` claim, and the only M005 claim that existed was a hoped-for
one.

### 1.1 What this corrective is not

It is not a claim that the system now obtains network isolation everywhere. It
is the contract that makes an obtained fact *reportable* — and it reports the
fact **per launch**, because a network boundary belongs to a launch, not to a
profile.

The distinction matters: the legacy Landlock route has no network dimension at
all. Its truthful answer is `Unrestricted`, and it stays that way. The
corrective adds a producer for the case that is genuinely obtained; it does not
relabel the cases that are not.

## 2. Requirement

> The system reports the network boundary it obtained, truthfully, per launch.
> A boundary that was not obtained is never reported as obtained; a boundary
> that cannot be determined is reported as undetermined, not as absent.

## 3. Design

### 3.1 The observation, not the request

The producer is anchored on an **observation**, with the request as the second
half of a conjunction.

Bubblewrap writes a `child-pid` status record only after its namespace and
mount setup has completed and the sandbox child has been forked
(`decode_bubblewrap_json_status` → `BubblewrapObservedStatus::child_pid`). A
requested namespace that cannot be created aborts the launch **before** that
child exists, so the status stream then carries no `child-pid` and the launch
fails closed. The receipt therefore proves the setup *happened*.

`obtained_network_enforcement(spec, child_pid)` combines the two:

- receipt observed **and** `--unshare-net` requested → `Enforced { backend:
  "bubblewrap-netns" }`;
- receipt observed **and** the namespace not requested → `Unrestricted`.

Neither half is sufficient alone. The request alone is a hope; the receipt alone
does not say which namespaces were created. The claim is only made when both
are present.

### 3.2 The carrier

`SandboxExecutionOutcome::Enforced` gains a typed `network:
NetworkEnforcement`. It has exactly two construction sites:

- the legacy Landlock route reports `Unrestricted` — truthfully, because
  Landlock scopes the filesystem and has no network dimension;
- the Bubblewrap route reports what `obtained_network_enforcement` derived.

Every read site matched with `..`, so the field threads without changing the
shape of the outcome.

### 3.3 The policy snapshot is not a launch

`SandboxEnforcementDto` describes a *profile on a host*. It cannot report an
obtained per-launch boundary, and it does not pretend to: with an
`OfflineRequired` intent it reports `Unavailable { reason: "OfflineRequired is
verified per child launch, not by the policy snapshot" }`.

Its summary, however, was built from the filesystem field alone and hard-coded
the network clause "network unrestricted (no OS isolation)". The daemon then
replaced the network field and **appended** a note, producing a summary that
read

```text
workspace_write requested; filesystem enforced (landlock); network unrestricted (no OS isolation); OfflineRequired requested (obtained enforcement is reported per launch)
```

— two contradictory statements about the same boundary in one sentence. The
summary is now derived from the facts it describes (`summary_for`), and
`with_network()` rebuilds it when the network fact changes, so the summary and
the field cannot disagree. The snapshot's summary for the unchanged cases is
byte-identical to before.

### 3.4 The C007 receipt

`ObtainedNetworkBoundary` gains the reported case **with its producer**:

| Variant | Producer |
|---|---|
| `Enforced { backend }` | a launch that observed its own setup completing with the network namespace requested |
| `Unrestricted` | a launch that obtained no network namespace |
| `NotReported` | a staging step that runs no contained child, so there is no launch to read |

`GitIndexStaging::stage` now returns the boundary it obtained, and the
publisher copies it into the receipt verbatim. The copy is
`ObtainedNetworkBoundary::from_launch(&result.sandbox)`, which reads the
launch's obtained fact — never the request — so the receipt cannot claim a
boundary the staging did not obtain.

The staging trait is the C007 trust boundary (§9.3 of the C007 plan): the
production implementation runs the contained child and reports what the launch
observed; a test implementation reports `NotReported`, which is a *statement*,
not an omission.

## 4. Required proof

1. A launch that requested the network namespace and observed its own setup
   completing reports `Enforced` with the mechanism named.
2. A launch that did **not** request the namespace reports `Unrestricted`, even
   though its filesystem containment was obtained — the anti-overclaim case.
3. A status stream without a `child-pid` is not a setup receipt and cannot
   support a claim.
4. The receipt's boundary is read off the launch: an enforced launch yields
   `Enforced`, an unrestricted launch yields `Unrestricted`, and a launch that
   never ran yields `NotReported`.
5. The policy snapshot's summary never contradicts its own network field.
6. The C007 publication suite still passes end to end.
7. On a qualified host, the hosted C007 proof reports the receipt stating
   `enforced` — the obtained fact, observed end to end.

## 5. Non-goals

- No admission change: C010 no-admission and M003 blocked status are unchanged.
- No new Linux host is admitted, and no supported-host policy is declared —
  that remains C014's open item 3.
- The tool-program child-job route remains unqualified (C014's open item 2).
- The ADR-0044 live-read model is untouched; ADR-0044 stays **proposed**. This
  corrective removes one of the reasons its clause 2 was vacuous; it does not
  accept it.
- No M005 milestone closure. The milestone stays active on its remaining gates.

## 6. Files

| File | Change |
|---|---|
| `crates/codegg-core/src/approval.rs` | `NetworkEnforcement` gains `as_str`/`backend`; the "no backend produces this" claim is replaced by the implemented contract |
| `src/managed_process.rs` | `SandboxExecutionOutcome::Enforced` gains `network`; the Bubblewrap status decoder returns the observed facts; `obtained_network_enforcement` is the producer |
| `crates/codegg-protocol/src/core.rs` | the enforcement summary is derived from its fields; `with_network` keeps them coherent |
| `src/core/daemon_ops.rs` | the `OfflineRequired` snapshot uses `with_network` instead of mutating the field and appending to the summary |
| `src/git_publication/mod.rs` | `ObtainedNetworkBoundary` gains the reported case, its producers, and `from_launch` |
| `src/git_publication/staging.rs` | the staging trait returns the boundary it obtained |
| `src/git_publication/publisher.rs` | the receipt carries the staging's boundary verbatim |
| `src/git_mutations.rs` | the receipt summary names the mechanism that obtained the boundary |
| `tests/git_index_publication.rs` | the test staging reports `NotReported` |
| `tests/linux_m004_process_ipc.rs` | the hosted proof asserts the receipt states the boundary it obtained |
| `architecture/security.md` | the obtained-fact contract is recorded |
| `architecture/git.md` | the receipt's boundary contract is updated |
