# Eggwork Remote Execution Corrective C001 — Lease Identity Coherence and Live-Node Qualification

Status: ready for handoff

Source corrective:

- `plans/subsystems/eggwork-fixed-target-remote-execution-post-closure-corrective-addendum.md`

Historical predecessor:

- `plans/implementation/eggwork-fixed-target-remote-execution/001-fixed-target-finite-job-executor.md`
- `plans/closure/eggwork-fixed-target-remote-execution/001-status.md`

CodeGG plan-authoring baseline:

- `338074062e3e8748ba83708ea3c2a7e33de12f74`

Eggwork dependency baseline:

- `128f808c62f176d414dd18a705773e45f5e2891a`

Primary class: invariant/corrective

## 1. Objective

Correct the durable remote-handle lease identity bug and add real local Eggwork mTLS qualification so restart/cancel/renew behavior is proven against Eggwork's actual fencing contract rather than only a scripted client.

This is a bounded post-closure corrective. Do not add new scheduling, placement, job kinds, transport routing, or operator policy.

## 2. Current defect evidence

At the baseline, `derive_handle(job, attempt_id)` creates two independent random lease values:

```rust
let lease_id = LeaseId::new(format!(
    "codegg-lease-{}",
    uuid::Uuid::new_v4().simple()
))?;

ExecutionHandle {
    execution_id: eggwork_id,
    generation,
    lease_id,
}

RemoteExecutionHandle {
    ...
    lease_id: uuid::Uuid::new_v4().simple().to_string(),
}
```

`to_eggwork_handle` later reconstructs an Eggwork `ExecutionHandle` from the persisted `RemoteExecutionHandle`.

Eggwork's server stores `lease_hash(wire.handle.lease_id)` at acceptance and compares cancel/renew request hashes against the stored token. Therefore the persisted handle is not equivalent to the accepted handle.

The current `ScriptedClient` tests do not validate lease identity and therefore permit the mismatch.

## 3. Work package A — Single-source lease identity

Refactor handle creation so one lease token is generated exactly once for a fresh CodeGG attempt.

Required shape:

1. derive deterministic execution id from CodeGG job id + attempt id as today;
2. use generation 1 as today unless the current Eggwork contract requires otherwise;
3. generate one fresh lease id;
4. construct the live `ExecutionHandle` from that lease id;
5. construct `RemoteExecutionHandle` by copying the exact execution id, generation, and lease id from the live handle.

Do not generate another random lease token when creating the durable record.

Prefer a conversion/helper that makes structural divergence difficult, for example:

- `RemoteExecutionHandle::from_eggwork(...)` in CodeGG-owned code; or
- a local helper that builds both representations from one canonical tuple.

Do not introduce Eggwork types into `codegg-core` merely to gain this conversion. `codegg-core` remains transport-independent.

## 4. Work package B — Pure regression coverage

Add focused tests around handle construction/reconstruction.

Required assertions:

- live.execution_id == durable.execution_id;
- live.generation == durable.generation;
- live.lease_id == durable.lease_id;
- `to_eggwork_handle(durable)` reconstructs the exact tuple;
- repeated reconstruction from the same durable value is stable;
- a new CodeGG attempt gets a distinct execution identity and lease;
- retransmission/reconciliation of the same attempt does not generate a new lease.

Add a regression specifically structured so the previous two-random-token implementation fails deterministically.

Do not rely on statistical UUID inequality/equality.

## 5. Work package C — Real local Eggwork fixture

Add a CodeGG integration fixture that exercises the production `NodeClientFactory` path against a real local Eggwork server.

Preferred shape:

- start `eggwork_server::NodeServer` in-process on `127.0.0.1:0`;
- use `eggwork_runner::LocalProcessRunner` as the node's canonical process owner;
- create a temporary state/database/blob/workspace root;
- construct a test CA, server identity, and client identity;
- configure Eggwork client authentication as required;
- map the client certificate to one test principal;
- authorize only the operations required by the fixture;
- write the CA/client cert/client key to temp files;
- construct CodeGG `ResolvedEggworkNode` / daemon config using those paths;
- let production `NodeClientFactory` build the client.

If the reviewed Eggwork public surface makes an in-process fixture impractical, a bounded subprocess `eggworkd` fixture is acceptable only if:

- it uses the same immutable Eggwork revision;
- lifecycle is deterministic;
- cleanup is guaranteed;
- it does not create a second production process owner in CodeGG;
- the reason for not using `NodeServer` directly is recorded in closure.

Test-only dependencies such as `eggwork-server`, `eggwork-runner`, and a certificate generator MAY be added under dev-dependencies, pinned to the exact same Eggwork revision. Do not widen production dependencies solely for the fixture.

## 6. Work package D — Real lease-fencing tests

The real-node fixture MUST prove Eggwork's actual contract.

At minimum:

### D1. Accept + renew

- submit one bounded remote command;
- renew using the exact accepted/persisted handle;
- assert renew succeeds before expiry.

### D2. Wrong lease rejection

- clone the execution id/generation but replace the lease token;
- call cancel or renew through the real client;
- assert Eggwork rejects it as invalid lease/forbidden;
- prove the test would have caught the original M001 mismatch.

Do not assert only on display strings if a typed client error/code is available.

### D3. Persist + reconstruct + control

- persist the durable CodeGG handle;
- reconstruct an `ExecutionHandle` using the same path used after restart;
- use the reconstructed handle against the real node;
- assert renew/cancel succeeds.

### D4. Restart reconciliation

Exercise the CodeGG executor reconciliation path rather than only the low-level client:

1. begin a remote execution that remains live long enough for reconciliation;
2. persist its real handle;
3. create a fresh `EggworkExecutor` instance as a daemon-restart analogue;
4. execute/reconcile the same CodeGG attempt;
5. assert no second remote execution is submitted;
6. assert the exact persisted handle can cancel the live remote execution;
7. assert the CodeGG attempt returns the intended conservative `Interrupted` disposition;
8. observe the node until the execution reaches terminal cancellation/interruption, bounded by a test deadline.

This test is the primary closure gate.

### D5. Terminal reconciliation

- complete an execution;
- create a fresh executor instance;
- reconcile the same persisted handle;
- assert terminal state maps back without a second submit.

## 7. Work package E — Existing scripted seam hardening

Keep `ScriptedClient` tests; they remain useful for fault injection and scheduler policy.

Enhance the scripted seam where practical so it can optionally enforce lease-token identity. At minimum, the persisted-handle tests should compare the submitted lease against the durable lease.

Do not replace deterministic scripted fault tests with slower real-node tests.

## 8. Work package F — Planning and documentation reconciliation

After implementation:

- update `architecture/jobs.md` and/or scheduler remote-execution documentation with the exact lease-fencing contract;
- keep `plans/closure/eggwork-fixed-target-remote-execution/001-status.md` immutable;
- create a new corrective closure record;
- update the subsystem roadmap so M001 is considered current/qualified only through C001;
- unblock M002 only if C001 closes with real-node evidence;
- update `plans/registry.md` accordingly.

## 9. Security and secret-handling constraints

Real-node test setup must not weaken credential handling:

- private keys live only in temporary files with restrictive permissions where supported;
- test credentials are generated/ephemeral or clearly non-production fixtures;
- no private key bytes appear in panic/debug/progress output;
- endpoint remains HTTPS;
- client-auth remains required;
- no principal identity is asserted in request payloads.

## 10. Failure, cancellation, restart, and contention semantics

The corrective does not change intended policy:

- a persisted handle identifies the exact accepted Eggwork execution and lease;
- a live pre-restart execution is cancelled under that same handle and the CodeGG attempt becomes interrupted;
- a terminal pre-restart execution is observed and mapped to its terminal completion;
- inability to observe/control the exact execution remains a typed interruption/failure, never a local fallback;
- a later retry creates a new CodeGG attempt and therefore a new execution id + lease;
- scheduler resource permits remain held through current attempt convergence.

If real-node testing shows these semantics cannot be implemented with the current Eggwork public API, stop and record the exact upstream gap instead of weakening fencing.

## 11. Required tests

Focused:

- handle tuple equality/reconstruction;
- persisted lease round-trip through in-memory JobStore;
- persisted lease round-trip through SQLite JobStore;
- duplicate same-attempt submit reuses exact lease;
- new attempt creates distinct lease.

Real node/mTLS:

- production `NodeClientFactory` connection;
- capabilities/status;
- workspace/blob upload;
- execute bounded argv;
- valid renew;
- invalid-lease reject;
- valid cancel;
- terminal observe;
- restart live reconciliation;
- restart terminal reconciliation;
- no second accepted execution on reconnect;
- local-workspace isolation remains true.

Existing:

- all `tests/eggwork_remote_execution.rs` scripted cases;
- target-routing static guard;
- scheduler bypass guard;
- core boundary guard.

## 12. Required verification

Follow current CodeGG verification policy/`AGENTS.md`. At minimum run the accepted equivalents of:

```bash
cargo test --test eggwork_remote_execution --locked
cargo test --test eggwork_remote_execution_live --locked
python3 scripts/check_eggwork_target_routing.py
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
scripts/verify.sh full
git diff --check
```

If the repository's current `verify.sh full` deliberately excludes environment-dependent tests, make the local Eggwork fixture self-contained enough to run in ordinary Linux CI or register a clearly bounded dedicated test command.

Do not claim hosted CI evidence unless an actual run is observed.

## 13. Acceptance criteria

1. Live and durable handles contain the same lease token.
2. Reconstructed handles are byte/semantic equivalent for id, generation, and lease.
3. The old two-random-token implementation is covered by a failing regression.
4. Production NodeClient communicates with a real local Eggwork server using required mTLS.
5. Valid persisted-handle renew/cancel succeeds on the real server.
6. Deliberately wrong lease is rejected by the real server.
7. Restart reconciliation creates no duplicate remote execution.
8. Live restart reconciliation actually terminates or reaches bounded terminal state on the real node.
9. Terminal restart reconciliation returns the existing result without resubmit.
10. Scheduler/target/no-fallback/workspace invariants remain green.
11. No production dependency widening is introduced solely for tests.
12. No unresolved medium-or-higher finding remains.

## 14. Stop conditions

Stop and report rather than widen scope if:

- Eggwork's public server/client APIs cannot support a deterministic local mTLS fixture;
- real-node qualification reveals a separate Eggwork protocol correctness bug;
- fixing the lease tuple requires changing Eggwork's fencing semantics;
- CodeGG would need to embed credentials in durable job state;
- a second scheduler or automatic node fallback is required;
- the fixture requires weakening client authentication;
- unrelated M002 capability/placement work becomes necessary;
- remote Test/AgentRun/ToolProgram support becomes entangled with the corrective.

## 15. Closure evidence

Create:

- `plans/closure/eggwork-fixed-target-remote-execution-corrective/001-status.md`

It must record:

- implementation SHA(s);
- exact Eggwork revision;
- before/after lease tuple evidence;
- regression demonstrating the historical defect;
- real-node fixture topology and TLS identities;
- valid-renew/invalid-lease/valid-cancel evidence;
- live and terminal restart-reconciliation evidence;
- duplicate-submission count/evidence;
- dependency changes and why they are test-only;
- focused + full verification;
- hosted CI evidence if available;
- unresolved findings by severity;
- final M002 disposition.
