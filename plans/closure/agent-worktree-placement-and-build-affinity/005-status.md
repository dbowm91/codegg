# Agent Worktree Placement and Build Affinity A005 — Closure Status

Status: conditionally closed (deterministic implementation); live-model qualification remains open

Source implementation plan: `plans/implementation/agent-worktree-placement-and-build-affinity/005-conservative-placement-advisor.md`
Source subsystem roadmap: `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md#milestones-and-boundaries`
Repository baseline reviewed: `493e5ee111d133eb1cb99debddcc466122c7f7f9`
Implementation commits:
- `0ac17d38c` — conservative task placement guidance, exact existing-checkout targeting, and accepted-child rationale.
- `271fd8d56` — read-only placement ownership guard hardening.

Conditionally closed: 2026-10-28

Primary class: capability / polish

## 1. Executive finding

The deterministic placement advisor is implemented and locally verified. Its guidance preserves `auto` absent user direction, recommends isolated placement for independent edits, permits existing managed placement only for explicitly directed inspection with an exact ID, and keeps serialized sharing within its supported profile. Accepted child results report a deterministic rationale. Placement advice grants no path, write, process, quota, cache, or shared-target authority.

A005 is **not fully qualified**. The required paired real-model evaluation across Rust, Python, multilingual, and refusal cases has not run. The operator authorized up to 240 provider requests, 60 minutes, and $10 on 2026-10-28. The configured OpenCode Go route's Harness Lab receipts have no provider cost field, and the selected route has no verified provider-enforced $10 cap. The budget gate therefore refuses before sending requests; no live requests have been sent. The roadmap also keeps A005 blocked until A002 reaches strict closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Conservative task-tool placement rules | Task schema description and guidance in `docs/tools.md` | pass | Rules preserve explicit intent and typed authorization boundaries. |
| Accepted child reports selected mode and rationale | Deterministic placement rationale in task response | pass | No absolute path or cross-workspace data is disclosed. |
| Placement does not enable build/cache authority | Guidance and regression test | pass | Repository quota remains configuration-owned; no cache or shared target behavior is added. |
| Existing checkout is exact-ID read-only and fail-closed | A002 implementation and `check_existing_worktree_read_only.py` | pass for local contract | Strict A002 qualification remains separately open. |
| Paired real-model comparison | Not run | blocked | Request/time/spend ceilings are authorized (240 / 60 minutes / $10), but the available route cannot enforce the spend ceiling and A002 strict closure is still pending. |

## 3. Implementation evidence

The task schema describes the four placement choices and their authority limits. The accepted spawn response includes the selected placement and rationale. Missing durable scheduler authority refuses explicit placement. The task parser and rationale preserve per-child exact `WorktreeId` selection and do not offer an implicit fallback. Documentation in `architecture/agent.md` and `docs/tools.md` describes the same constraints.

## 4. Verification executed

- `cargo test --target aarch64-apple-darwin -p codegg --lib placement_rationale_preserves_conservative_authority_boundaries --locked`: **1 passed**.
- `python3 scripts/check_existing_worktree_read_only.py --self-test` and normal guard: passed.
- `scripts/verify.sh quick`: passed on commit `271fd8d56`; all quick guards and workspace all-target check passed.
- `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Formatting, test assertion quality, test duplication, Rust test scanner, and whitespace checks passed.

## 5. Invariant review

The advisor explains already authorized placement intent and cannot select a filesystem path, authorize a worktree ID, increase scheduler quota, run processes in read-only placement, or enable a compiler cache/shared Cargo target. `auto` remains the default. Existing managed inspection remains subject to A002's exclusive lease, owner checks, clean managed checkout requirement, and read-only allowlist.

## 6. Unresolved findings

- **Medium:** execute the preregistered paired multi-language and dependency-heavy Rust evaluation after A002 strict closure and when an authorized route provides an enforceable $10 spend ceiling (or a verified free CodeGG route). The request and time ceilings are already authorized. Record all successes, verifier results, costs, resource measurements, collisions, refusals, and fallbacks. No live-model success or performance claim is made here.

## 7. Roadmap disposition

The deterministic implementation slice is conditionally closed. The A005 roadmap milestone remains **blocked** pending A002 strict closure and the live-model evaluation. No automatic placement, cache, shared-target, or permission expansion is proposed.

## 8. Registry updates

- Record the deterministic implementation as conditionally closed.
- Keep the roadmap milestone blocked until A002 strict closure and the paired evaluation is completed under the authorized 240-request / 60-minute / $10 limits; the remaining prerequisite is an enforceable spend gate for a supported CodeGG route.
