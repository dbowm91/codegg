# Historical Dependency Security M005 — CodeGG Eggup Adoption Closure

Status: closed

Follow-up plan: `plans/implementation/dependency-security-workspace-consolidation/007-eggup-managed-runfile-adoption-follow-up.md`

Related Eggup plan: `plans/implementation/consumer-adoption/005-codegg-managed-runfile-bundle-adoption.md`

Historical M005 closure: `plans/closure/dependency-security-workspace-consolidation/005-status.md` (preserved unchanged).

CodeGG implementation: `23d84422c71bc1c251ba916a1a6c81e35c341fc9`.
Eggup API source: `66813b3b94de3a9b2f270e0000dc339ef6f0b478`.
Hosted CI: [run 35876819155](https://github.com/dbowm91/codegg/actions/runs/35876819155), passed.

## Result

CodeGG's supported Linux and macOS `codegg upgrade` path now replaces the
managed `codegg`, `codegg-sandbox-helper`, and `codegg-eggsearch` runfiles as
one Eggup transaction. CodeGG retains release/tag/version/target policy,
strict bundle extraction, eggsearch pinning, helper identity, and CLI policy.
Eggup provides bounded acquisition contracts and local transaction,
rollback, and recovery mechanics. Normal self-update does not fetch or execute
`install.sh` and does not invoke external `curl`.

The historical M005 blocker is resolved by an immutable-pinned external API;
the old blocked-state closure remains as historical evidence. No Eggup
producer-release or archive-generation responsibility was introduced.

## Ownership and trust evidence

- `eggup-core` and `eggup-acquisition` are both pinned to Eggup revision
  `66813b3b94de3a9b2f270e0000dc339ef6f0b478` in CodeGG `Cargo.toml` and
  `Cargo.lock`.
- `cargo tree -i eggup-core --locked` and
  `cargo tree -i eggup-acquisition --locked` each show only the direct CodeGG
  dependency. `cargo tree -d --locked` shows no duplicate instance of either
  Eggup crate.
- CodeGG's adapter uses its existing ordinary Eggfetch client builder and
  Rustls/WebPKI features. `eggup-eggfetch` is not enabled, so this adoption
  does not widen root-store or proxy policy or introduce another HTTP/TLS
  stack.
- CodeGG's target map is the four existing Linux/macOS x86_64/aarch64 targets.
  The exact expected archive basename and one exact `checksums.txt` match are
  required before extraction. The checksum establishes archive integrity,
  not publisher authenticity.
- Extraction permits only the three required top-level runfiles and the fixed
  optional third-party notice. Negative fixtures cover missing, extra,
  duplicate/case-colliding, nested, backslash, traversal, absolute, and
  symlink members. Extracted SHA-256 requirements flow through Eggup staging
  and locked pre-mutation revalidation.
- Candidate probes check CodeGG release identity/version, the independent
  pinned eggsearch identity/version, and the helper's safe refusal identity.
  Ownership requires the running CodeGG installation and recognized siblings;
  missing historical siblings may be created under `AbsentPolicy::AllowCreate`,
  while foreign/ambiguous members fail closed. Receipts preserve committed,
  rolled-back, and recovery-required distinctions.

## Verification

Local qualification was performed on macOS x86_64. The final source received
stable full verification before the last Rust 1.89 compatibility refinements;
the refinements were then covered by focused and full-repository checks below.

Passed:

```text
scripts/verify.sh full
  - workspace guards, formatting, check and Clippy passed
  - default workspace tests: 4,890 passed, 1 failed initially in a fixture
    with no registered 404 route; after fixing the fixture, the repeat passed
  - feature-enabled CodeGG tests: 4,941 passed
cargo test --locked upgrade::managed::tests -- --test-threads=1  # 9 passed
cargo test --test upgrade --locked -- --test-threads=1           # 10 passed
cargo clippy --workspace --all-targets --locked -- -D warnings   # passed
cargo +1.89.0 check --workspace --all-targets --locked           # passed
scripts/verify.sh quick                                           # passed after final changes
scripts/release/test-release-tools.sh                             # passed
scripts/release/test-installer.sh                                 # passed
scripts/release/test-clean-host.sh                                # 26 passed
cargo tree -i eggup-core --locked                                 # one direct consumer
cargo tree -i eggup-acquisition --locked                          # one direct consumer
cargo tree -d --locked                                            # reviewed
git diff --check                                                  # passed
```

The first Rust 1.89 workspace check exposed three pre-existing source
compatibility issues (`floor_char_boundary`, two `PathBuf`/`str` comparisons,
and a TUI module alias). Small compatibility fixes are included in the
implementation commit; the final Rust 1.89 check passes. Their behavior is
covered by the full stable suite and focused upgrade tests.

The hosted CodeGG CI workflow passed on 2026-09-23. It ran the repository's
Linux workflow with formatting, ownership/security guards, workspace Clippy,
and serialized workspace tests. Native macOS local qualification passed.
Hosted macOS and Windows runtime jobs were not available in this workflow;
Windows in-place replacement remains explicitly unsupported. Rust 1.89 local
check passed. No release-profile before/after binary-size measurement was
made; no release binary-size claim is made here.

## Disposition

Historical dependency-security M005 is satisfied by this adoption. CodeGG
M005 closes with no medium-or-higher generic updater issue identified. The
consumer adoption does not depend on post-commit service rollback; the
separately requested Eggup Verified Update Core M007 remains the next
sequential plan.
