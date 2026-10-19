# Agent Tool-Use Operability Corrective C008 — Eggsearch 0.4.4 Follow-up

Status: closed
Source closure: `plans/closure/agent-tool-use-operability-post-audit-corrective/008-status.md`
CodeGG baseline: `388643bfca38f39bfb6e7cca75a702aa24b3340a` (`main`)
Eggsearch final revision: `13fcac0b7ac0125d38e4e9775ebe1d62028fda72` (`main`, merge of PR #9, tag `v0.4.4`)

## Purpose

The original C008 closure qualified CodeGG against an earlier Eggsearch 0.4.4
commit. Eggsearch C008 was forward-ported to current Eggsearch `main`, local
filesystem reads were hardened, the package was requalified and released, and
CodeGG was requalified against the resulting immutable Eggsearch revision.
The original closure remains historical evidence and is not rewritten.

## Eggsearch qualification and release

- Eggsearch PR #9 merged at `13fcac0b7ac0125d38e4e9775ebe1d62028fda72`.
- The clean-commit `make release-check` passed at that exact revision,
  including all-feature tests, documentation, optimized release build, and
  crates.io publish dry-run.
- Eggsearch hosted PR checks passed on head `d8fe7fc32cd8425dab8b69c2ee2c129d7ce6ee47`:
  CI, release drift, Python native qualification, all seven egress target
  checks, and Windows portability.
- Exact-SHA Eggpack candidate qualification run `39290716548` passed all seven
  builds, qualifications, consumer validations, required gate, and aggregate
  for `13fcac0b7ac0125d38e4e9775ebe1d62028fda72`.
- The `v0.4.4` tag resolves to that exact merge commit and crates.io accepted
  `eggsearch 0.4.4`.
- Eggpack release producer run `39295251643` and `Release provenance` run
  `39301025264` both passed. The published release contains exactly 19 assets:
  seven executables, seven checksum sidecars, two reviewed installers, two
  exact-version installers, and `release-manifest.json`.
- The downloaded manifest names release `v0.4.4`, the exact source commit, and
  all seven targets. All seven executable SHA-256 sidecars passed `sha256sum
  -c`; independent `gh attestation verify` passed for all seven executables
  and all four installers before the draft was published.

## CodeGG paired qualification

`scripts/test-eggsearch-pair.sh` now pins `EXPECTED_VERSION="0.4.4"` and
`EXPECTED_COMMIT="13fcac0b7ac0125d38e4e9775ebe1d62028fda72"`. The run built the
Eggsearch executable from that exact `git archive` with Cargo offline and
reported SHA-256
`24698b565312a40d17ca8ac5d2049e362c3da3116bb59089c2cde97638ea992b`.

Passed against that binary:

- Provider contract (29), bootstrap (21), research adapter (18),
  search-backend integration (9), provider mapping (18), fake MCP (28), and
  MCP child lifecycle (5) tests.
- Real managed-child compatibility and privacy test, including local-only
  mode-off behavior, scoped no-match, disabled local retrieval, structured
  result fidelity, cleared inherited credentials/proxy settings, and child
  teardown.
- Real AgentLoop A/B/A accepted-workspace isolation test.
- `scripts/verify.sh quick`.

No live provider calls or real-model claims were made. The original C008
authority boundary and limitations remain as recorded in the source closure.

## Registry disposition

C008 remains closed; no downstream plan status changes. C002 and C007 remain
strictly closed as recorded in `plans/registry.md`. This follow-up adds release
and exact-revision qualification evidence only.
