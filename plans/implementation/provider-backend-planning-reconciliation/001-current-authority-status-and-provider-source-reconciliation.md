# Provider Backend Planning Reconciliation C001 — Current-Authority Status and Provider-Source Reconciliation

Status: ready for handoff

Repository baseline: `5005389b176545ae9df86d8d74441434aeddaa8b`

Source corrective addendum:

- `plans/subsystems/provider-backend-planning-reconciliation-corrective-addendum.md`

Primary class: polish / documentation.

## 1. Objective

Reconcile the closed provider-backend workstream's current planning/documentation surfaces so they accurately reflect accepted closure and later first-party provider evidence, without changing production code or rewriting historical closure records.

## 2. Why this is needed

The predecessor workstream is closed, but three stale documentation patterns remain:

- milestone-local C001/C002/C003 sections still say `Status: ready`;
- lifecycle prose still says the three closed milestones are independently ready;
- pre-implementation Together wording still presents CodeGG `.xyz` as current/canonical first-party truth even though accepted closure and EggPool's subsequent first-party audit establish `.ai` as canonical and `.xyz` as a legacy alias.

The implementation and closure records already contain the correct technical disposition. This plan only brings current-authority planning text into agreement.

## 3. Scope

In scope:

- `plans/subsystems/provider-backend-post-closure-corrective-addendum.md`;
- `plans/implementation/provider-backend-post-closure-corrective/002-provider-catalog-and-compatible-discovery-reconciliation.md`;
- `architecture/provider.md` only if needed to eliminate contradiction;
- `plans/registry.md`;
- new closure record for this docs-only corrective.

Out of scope:

- Rust/source changes;
- CodeGG Together endpoint migration;
- provider requalification;
- changes to closure records 001–003;
- EggPool changes.

## 4. Required edits

### A. Closed lifecycle truth

In the predecessor addendum:

- C001 -> `closed`;
- C002 -> `closed`;
- C003 -> `closed (retain library-only)`;
- replace "C001–C003 are independently ready and may be implemented in parallel" with closure-state wording;
- preserve the final status table and implementation commit references.

### B. Together source-truth reconciliation

In current-authority prose, state:

- current canonical first-party Together prefix: `https://api.together.ai/v1`;
- CodeGG currently retains `https://api.together.xyz/v1` as a legacy-compatible alias;
- no runtime endpoint change is authorized by this plan;
- the original C002 assumption that EggPool `.ai` was stale was superseded by execution-time first-party review.

Do not delete historical reasoning. Mark it superseded and point to the accepted C002 closure record.

### C. Closed C002 implementation-plan annotation

Add a prominent post-closure reconciliation note immediately after the status/header area of the C002 implementation plan. It must explain that endpoint-source assumptions in the body reflect the planning-time snapshot and that closure §3 is authoritative for the executed result.

Do not rewrite the full historical plan body.

### D. Registry reconciliation

Register this corrective as the only active provider-backend documentation control point while leaving:

- provider wire-kernel consolidation closed;
- provider backend C001–C003 closed;
- no downstream runtime milestone unblocked.

On C001 closure, mark this corrective closed and record it in Recently closed/current gate text according to repository convention.

## 5. Verification

Required:

```bash
git diff --check
```

Plus explicit source inspection proving:

- zero `Status: ready` remains for C001–C003 in the predecessor addendum;
- current-authority prose does not describe Together `.xyz` as canonical;
- C002 implementation plan has the supersession note;
- `architecture/provider.md` and the reconciliation text agree;
- historical closure files have zero diff;
- no non-Markdown file changed.

## 6. Acceptance criteria

- planning lifecycle is internally consistent;
- source-truth language matches accepted closure evidence;
- historical intent remains visible but is clearly superseded where appropriate;
- production behavior is untouched;
- registry accurately represents the docs-only corrective;
- closure evidence is recorded.

## 7. Stop conditions

Stop if the work requires any code/config/runtime change or if new evidence shows the retained Together alias is no longer operationally acceptable. That becomes a separate technical corrective.

## 8. Handoff note

This plan is intentionally small. Do not use it to "clean up" unrelated provider plans or normalize every historical statement in the repository.
