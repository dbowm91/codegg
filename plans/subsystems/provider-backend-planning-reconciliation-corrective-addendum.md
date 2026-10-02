# Provider Backend — Planning and Documentation Reconciliation Corrective Addendum

Status: active

Repository baseline reviewed: `5005389b176545ae9df86d8d74441434aeddaa8b`

Predecessor implementation work:

- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md` — closed.
- `plans/subsystems/provider-backend-post-closure-corrective-addendum.md` — closed at C001–C003, implementation `7963db44`.
- `plans/closure/provider-backend-post-closure-corrective/001-status.md`
- `plans/closure/provider-backend-post-closure-corrective/002-status.md`
- `plans/closure/provider-backend-post-closure-corrective/003-status.md`

Cross-repository reference:

- EggPool provider-profile metadata M001 is closed at `plans/closure/provider-profile-metadata/001-status.md` in the sibling repository.
- Its execution-time first-party review established `https://api.together.ai/v1` as the current canonical Together prefix and `https://api.together.xyz/v1` as a retained legacy alias.
- CodeGG production behavior is not changed by this documentation-only corrective.

Canonical references:

- `plans/003-planning-process.md`
- `plans/registry.md`
- `architecture/provider.md`

No ADR is required. This work changes no runtime ownership, protocol, provider endpoint, credential, retry, model-discovery, storage, or public API behavior.

## 1. Corrective trigger

The provider backend implementation is technically closed and healthy, but current planning documents still contain stale lifecycle and source-truth statements:

1. `plans/subsystems/provider-backend-post-closure-corrective-addendum.md` is globally marked closed and its final status table correctly closes C001–C003, yet the individual milestone sections still say `Status: ready` and the dependency prose still says C001–C003 are independently ready.
2. The same addendum's historical C002 trigger text says CodeGG Together `.xyz` matched current first-party examples and EggPool had the inverse drift. EggPool's later first-party audit superseded that premise: `.ai` is canonical and `.xyz` is a legacy alias.
3. The closed C002 implementation plan repeats that superseded premise in its rationale, scope, and acceptance text even though its closure record correctly records the final evidence: CodeGG retained `.xyz` for compatibility while acknowledging canonical `.ai`.
4. `plans/registry.md` is materially correct but should link this reconciliation as the final current-authority cleanup so future readers do not treat stale historical plan prose as active source truth.

This is planning/documentation debt only. Historical closure records remain accepted and immutable.

## 2. Corrective milestone

### C001 — Provider backend current-authority planning/documentation reconciliation

Status: ready.

Implementation plan:

- `plans/implementation/provider-backend-planning-reconciliation/001-current-authority-status-and-provider-source-reconciliation.md`

Primary class: polish / documentation.

Dependencies:

- provider backend C001–C003 closed — satisfied;
- EggPool provider-profile metadata M001 closed — satisfied;
- no code/package/runtime dependency.

## 3. Invariants

The corrective MUST preserve:

- implementation commit `7963db44` and all provider backend closure records as historical evidence;
- current production Together endpoint behavior in CodeGG (`.xyz`) unless a separate runtime endpoint migration plan is approved;
- current canonical-source statement that Together `.ai` is the first-party canonical prefix and CodeGG `.xyz` is a legacy-compatible alias;
- OpenCode Go `/zen/go/v1` correction;
- native OpenAI URL correction;
- generic compatible probe split and shared bounded model parser;
- library-only `FallbackProvider`/`CircuitBreaker` disposition;
- the accepted CodeGG/EggPool ownership boundary.

No source code, Cargo manifests, lockfiles, tests, provider definitions, or runtime configuration may change.

## 4. Non-goals

- Migrating CodeGG Together from `.xyz` to `.ai`.
- Re-auditing every provider endpoint.
- Reopening provider wire M001–M003 or backend C001–C003.
- Editing historical closure evidence to make the original plan look prescient.
- Changing the EggPool repository.
- Adding planning lint infrastructure.
- Running live provider requests.

## 5. Required reconciliation

C001 must:

- change each stale C001/C002/C003 milestone-local `Status: ready` in the closed provider-backend addendum to its accepted closed disposition;
- change dependency/lifecycle prose that still says the three correctives are "ready" or may run in parallel into historical/closed wording;
- replace current-authority Together source statements in the addendum with the accepted result:
  - canonical first-party prefix: `https://api.together.ai/v1`;
  - CodeGG retained prefix: `https://api.together.xyz/v1`;
  - `.xyz` is a legacy-compatible alias retained to avoid an unrelated runtime migration;
- keep the original corrective trigger understandable by labeling superseded premises as historical rather than deleting the context;
- add a concise post-closure supersession note near the top of the closed C002 implementation plan pointing readers to `plans/closure/provider-backend-post-closure-corrective/002-status.md`;
- avoid rewriting the C002 plan line-by-line solely to erase the original assumption; the closure record remains the authoritative outcome;
- confirm `architecture/provider.md` already states canonical `.ai` vs retained legacy `.xyz`; only edit it if a contradictory statement remains;
- update `plans/registry.md` so this docs-only corrective is the explicit active/ready control point while the predecessor implementation workstream remains closed;
- on closure, remove/close this corrective row and leave the predecessor row closed.

## 6. Verification

Because the scope is documentation/planning only, implementation should verify:

```bash
git diff --check

grep -n "Status: ready" plans/subsystems/provider-backend-post-closure-corrective-addendum.md
grep -n "independently ready" plans/subsystems/provider-backend-post-closure-corrective-addendum.md
grep -n "api.together" plans/subsystems/provider-backend-post-closure-corrective-addendum.md
grep -n "api.together" plans/implementation/provider-backend-post-closure-corrective/002-provider-catalog-and-compatible-discovery-reconciliation.md
grep -n "api.together" architecture/provider.md
```

Acceptance inspection must also confirm all referenced closure/plan files exist and that `plans/registry.md` does not list the predecessor implementation workstream as active/ready.

No Cargo test/Clippy run is required if the diff contains only Markdown planning/documentation files. If implementation touches any Rust, Cargo, script, or config file, stop and register a new technical corrective rather than expanding this plan.

## 7. Acceptance criteria

C001 closes when:

- the provider backend addendum has one internally consistent closed lifecycle state;
- no milestone-local C001/C002/C003 text says `ready`;
- current-authority text no longer claims `.xyz` is Together's canonical current endpoint;
- CodeGG's deliberate retained `.xyz` behavior is documented as legacy-compatible, not silently migrated;
- the closed C002 implementation plan contains a prominent supersession pointer to its closure evidence;
- architecture docs, registry, addendum, implementation-plan status, and closure records do not contradict each other;
- historical closure records are unchanged;
- the diff is planning/documentation only and `git diff --check` passes.

## 8. Stop conditions

Stop and register separate work if reconciliation reveals:

- current production Together `.xyz` no longer works or is officially removed;
- a provider endpoint/runtime change is required;
- a closure record itself is materially false rather than merely superseded by later evidence;
- another provider correctness issue of medium-or-higher severity.

## 9. Closure evidence

Create:

- `plans/closure/provider-backend-planning-reconciliation/001-status.md`

The closure must list:

- files changed;
- before/after contradictory statements;
- explicit preservation of historical closure records;
- Together canonical-vs-retained-alias disposition;
- `git diff --check` and reference-resolution results;
- confirmation of zero production/source/Cargo/config diff;
- unblock audit (expected: nothing unblocked).
