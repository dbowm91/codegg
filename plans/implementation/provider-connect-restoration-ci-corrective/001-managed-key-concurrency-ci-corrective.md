# Provider /connect Restoration CI Corrective C001 — Managed-Key Concurrent First-Write Closure

Status: active

Repository baseline: `5c4e2966`

Source roadmap: `plans/subsystems/provider-connect-restoration-ci-corrective-addendum.md`

Related historical closure:

- `plans/closure/provider-connect-restoration-corrective/001-status.md`

## 1. Objective

Determine and correct the hosted-only or concurrency-sensitive failure in
`codegg-config::encryption::tests::concurrent_first_writes_converge_on_one_key`
without weakening the managed master-key safety contract.

## 2. Required investigation

- Run the focused test repeatedly with the repository's standard single-thread
  test process and with the test's internal worker fan-out.
- Exercise the corresponding credential-store concurrent first-write test.
- Trace every `create_new`, write, sync, read-back, and validation ordering on
  the winner and loser paths.
- Determine whether the failure is a production race, a test-isolation issue,
  or a hosted-environment artifact; record evidence rather than assuming a
  retry is sufficient.

## 3. Invariants

- A loser of atomic key creation reads exactly the complete winner key.
- Partial, corrupt, symlinked, non-regular, or unsafe-permission key files fail
  closed and are never silently replaced.
- Explicit environment-key precedence remains unchanged.
- Key material never appears in logs, debug output, errors, or protocol data.
- The workspace suite must complete; CI cancellation or retry is not evidence
  of correctness.

## 4. Acceptance criteria

- Focused encryption and credential-store concurrency tests pass repeatedly.
- `scripts/verify.sh quick` passes.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passes.
- `cargo test --workspace --locked -- --test-threads=1` completes cleanly.
- Hosted `CI / verify` passes on the closure revision.

## 5. Closure requirement

Create a closure record under
`plans/closure/provider-connect-restoration-ci-corrective/001-status.md` with
the reproduction matrix, safety review, exact hosted run, and a registry
unblock audit. Do not rewrite the historical provider-connect M001 closure.
