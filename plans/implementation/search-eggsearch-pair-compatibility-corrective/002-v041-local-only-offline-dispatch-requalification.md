# Search / Eggsearch Pair Compatibility C002 — Mode-Off Local Repository Dispatch Correction

Status: blocked
Class: external dependency correctness / compatibility qualification
Source roadmap: `plans/subsystems/search-eggsearch-pair-compatibility-post-closure-corrective-addendum.md`
Predecessor: C001 closure `plans/closure/search-eggsearch-pair-compatibility-corrective/001-status.md` (conditionally closed)
Original contract: `plans/implementation/search-eggsearch-legacy-retirement/001-eggsearch-parity-adoption-and-provider-contract.md`
Affected downstream gate: legacy-retirement M002 `plans/implementation/search-eggsearch-legacy-retirement/002-builtin-search-backend-retirement.md`
Upstream repository: `eggstack/eggsearch`; affected published baseline v0.4.1, commit `32769d684417567dfb30fc5adea1475cee7f0ed0`.

## 1. Objective

Obtain a corrected immutable eggsearch release and prove that `repo_search(include_local=true)` with `search.mode="off"` stays entirely on the private local path and never invokes a remote provider adapter. Then rerun the CodeGG paired compatibility gate against that exact tag and decide whether strict C001 closure and destructive legacy-retirement M002 are eligible.

## 2. Why blocked

During C001 exploration, a private local fixture and Mode-Off configuration were used with eggsearch v0.4.1. The local-only repository route nevertheless initiated remote DuckDuckGo provider dispatch, producing provider telemetry/results/timeouts. The request was uncredentialed and is not part of the final C001 harness, but it is direct evidence that v0.4.1 cannot currently be treated as an offline local repository fixture. This CodeGG worktree does not own the upstream implementation or release process; no upstream edit or release can be made here. Do not repeat a live provider dispatch attempt to investigate it.

## 3. Required upstream correction evidence

The upstream owner must publish a new immutable tag whose exact source commit contains a narrow regression suite proving all of the following without external calls:

- A local-only repository query under Mode Off returns only data from the private test workspace.
- A fake/counting remote-provider adapter records zero calls for the local-only request, including fallback and error paths.
- A non-local query under Mode Off is rejected by policy before any provider adapter is entered.
- The test fails against v0.4.1 behavior (or equivalently demonstrates the formerly failing branch) and passes on the new source.
- The source commit, release tag, build inputs, and binary digest are immutable and reproducible.

No credentials, live providers, or network-dependent success are acceptable as evidence. Do not use a timeout or empty result as proof of zero adapter dispatch.

## 4. CodeGG requalification after blocker clears

Once the upstream tag exists:

1. Review the exact upstream diff and tagged schema/config changes; update only the pinned compatibility inputs needed by CodeGG.
2. Extend the private real-child gate with a `repo_search(include_local=true)` trajectory against a fixture inside its private workspace. Assert returned file identity/content is fixture-bound and no remote adapter invocation is possible by the upstream regression evidence.
3. Retain the real `websearch` and `security_search` Mode-Off policy-denial assertions, environment observer, poisoned-config negative control, invalid-provider local rejections, and bounded process teardown.
4. Run the entire `scripts/test-eggsearch-pair.sh` gate from immutable upstream source in Cargo offline mode and `scripts/verify.sh quick` on the same CodeGG revision.
5. Write a new factual closure update or successor corrective record that identifies the exact upstream tag/commit/digest and resolves the C001 high finding. Do not rewrite historical M001 or C001 evidence.

## 5. Scope and non-goals

This plan is blocked pending external upstream correction. It does not authorize provider calls, upstream repository writes, release publication, CodeGG backend retirement, or claiming an OS-level network sandbox. It does not change CodeGG production search behavior by itself.

## 6. Stop conditions and acceptance

Stop if the new tag cannot prove zero provider-adapter dispatch through a deterministic local counter, if local results escape the private fixture, or if the correction requires a CodeGG trust/ownership change beyond this compatibility contract. Report the evidence and create a separately reviewed plan rather than weakening this requirement.

C002 can close only with: immutable corrected upstream source/tag; upstream zero-dispatch regression evidence; CodeGG's exact pinned subprocess passing local repo plus existing wrapper trajectories; clean environment and process teardown evidence; pair harness and quick verification passing; and explicit registry audit. On closure, strict C001 may be recorded as discharged by the successor evidence and legacy M002 may move to `ready` only if its other dependencies are satisfied.
