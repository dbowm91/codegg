# Linux Trusted Git Cost Corrective C004 — Private Index/Object Staging and Broker Promotion

Status: active — test-only private index/object staging prototype; **publication and production mutation remain blocked on independent ADR-0036 acceptance and broker safety proof**
Primary class: invariant
Baseline: `planning/linux-sandbox-next-admission-and-adr-cleanup` `0b36c71f (post-rebase review baseline; the pre-rebase evidence commit was 004d770b8cd23ca33fb724dae95f97de563ba60d)` (2026-10-18).
Source: `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md`, parent Linux execution isolation roadmap and Linux C002 contained-local-Git recovery plan `plans/implementation/linux-execution-isolation/010-m001-contained-local-git-recovery.md`.
Normative: long-term §§4,17,27–29 and terminology §8; accepted ADR-0002/0004/0016/0034/0035, **proposed ADR-0036**.
Target future closure: `plans/closure/linux-trusted-git-cost-corrective/004-status.md`.

## Objective

Prototype an authorized Git mutation transaction that reads existing repository objects **without copying the whole ODB**, stages a private index and new objects, and publishes only validated results through the existing daemon-owned Git service. Demonstrate external-writer conflict handling and crash safety before enabling the positive local mutation flows owned by Linux C002.

## Current evidence / problem

The current contained Git plan denies arbitrary `.git`/common-dir writes and treats clean/smudge filters, merge drivers, hooks and other configuration-triggered programs as needing contained execution. The private immutable object view is read-only and cannot satisfy routine object/index writes; C002 cannot be closed simply by allowing the entire protected `.git` as a writable mount. `WorkspaceLockTable` is process-local and cannot fence IDEs or external git. Git worktrees share objects/refs but have distinct per-worktree gitdir/index. Index, worktree and refs are **not one atomic resource**.

The native test-only prototype initializes an operation-owned gitdir, writes a broker-authored alternate to the selected repository's ordinary ODB, and runs one contained `git add` with the repository metadata protected by M004's read-only mount overlay. Exact-head hosted Linux run `39113624242` on `eadac9c86c6a79ee6af5ec6faed7412ea7c8b90e` passed the private-index/object assertions, confirmed the source-configured filter was not invoked, detected and rejected an index change made before publication, imported the verified blob into the source ODB, and atomically replaced the source index under a Git-compatible `index.lock`. Follow-up exact-head run `39115964190` on `7a5c79c9d23c4b7b423787c12c0a0533540a6f23` passed the full Linux M004 workflow. Its private staging test confirmed the source gitdir was protected and identity-captured and its gitdir/ODB device-inode identities stayed stable; the complete process/IPC, PTY/LSP, protected-mount/Git, and restricted-userns checks passed. A follow-up test-only broker model now persists an index publication intent before creating the Git lock, simulates interruption after the lock is synced but before rename, then checks gitdir device/inode identity, the expected source index, a no-follow regular lock file, and exact staged-index bytes before completing recovery. Local runtime qualification is unavailable because this host refuses `process-ipc-landlock-abi-6`; exact-head hosted rerun is pending. This remains a test-only broker model rather than integration through production Git service APIs. It still does not prove daemon restart/recovery, cancellation cleanup, path replacement race safety, arbitrary non-Git writer exclusion, ref/worktree multi-resource promotion, durable journal ownership, or recovery across all publication phases.

## Invariants and limits

- Agent CLI, Bash, Python, filters and hooks never acquire permission to modify original protected gitdir/common-dir directly; new objects/index live under Codegg-owned scoped scratch and an explicit `ContainedGitOperation` authorization.
- Trusted broker publisher never runs repo hooks/filters/unknown helpers, never reads arbitrary external paths or inherits ambient network/credential authority, and verifies operation ID, workspace/worktree/gitdir identity, scope, lease and result.
- Existing local Git objects may be observed concurrently changing; no consistency guarantee without separate generation validation. The broker must reject index/ref conflicts or promote only proven compatible effects; Codegg locks alone are insufficient.
- `DestructiveRepositoryChange` remains independent; no remote network, credential or arbitrary `GIT_ALTERNATE_OBJECT_DIRECTORIES` grants. No FullHost fallback, global filesystem locks, writable whole-gitdir bind, or fake multi-file atomicity.
- Any helper code executed by Git runs inside M004 and *actually obtained* M005 offline isolation before writing private objects.

## Ordered packages

1. Define a typed `ContainedGitOperation` input and narrow `PublishGitMutation` output through existing `GitMutationExecutor`/`GitExecutionService` and worktree leases. Make `index`, object OID additions, refs and working-tree effects individually explicit in a transaction manifest. Tie to canonical common-dir, linked-worktree gitdir, selected index identity, expected HEAD/refs/sequence, allowed path set, origin principal and obtained sandbox receipt.
2. Build isolated operation-owned scratch: private `GIT_INDEX_FILE` initialized from an admitted baseline; private `GIT_OBJECT_DIRECTORY`; read existing ODB through carefully scoped `GIT_ALTERNATE_OBJECT_DIRECTORIES` or a separately verified read-only mount. Never allow repository-defined `objects/info/alternates` to transitively expose unapproved host paths; test Git recursive alternate traversal explicitly. Treat replace refs, grafts, promisor and shallow state as untrusted inputs, not automatic authority.
3. Implement **test-only** staging for bounded `git add` with clean/smudge filters disabled by default (or executed only under obtained confinement). Verify staged index path changes, blob/tree OID/content hashes and allowed paths. Reject malformed outputs, partial clones/fetches, unexpected worktree writes and capacity violations. Produce an atomic private output manifest and keep secrets out of it.
4. Prototype broker-side publication through existing Git primitive APIs/locks: import verified new objects without overwriting mutable originals; revalidate index/refs and transaction start assumptions; use Git-compatible index lock/rename and `update-ref` old-OID checks where appropriate. Treat index and refs as separately committed phases with explicit journal/rollback/reconciliation; do not claim a single atomic transaction across filesystem, index and refs.
5. Adversarial tests: externally mutate index/refs/HEAD during staging and between validate/publish, nested/link worktree pointer swaps, malicious alternates/hardlinks/symlinks, path collisions, pack GC/repack, concurrent agents, hostile filters/config hooks, Git object hash collision/corruption fixture, process termination at each promotion phase. Allowed outcomes: authorized committed state or typed conflict/incomplete-with-safe-recovery; never silent overwrite, secret/IPC access or corrupt published state.
6. Bound memory, output, tmp space and operation lifetime. On cancellation/crash, retain only broker-owned recovery manifest when needed and safely garbage-collect orphan private objects; on daemon restart reconcile owned attempts without assuming external writers cooperated.
7. Publish a rigorous option study for staging `checkout`/merge/rebase effects that span the working tree (commit stager alone is not parity). Defer complex operations until C005 and Linux C002 independent approval.

## Expected source changes / storage / compatibility

Scoped prototype in `src/git_mutations.rs`, `src/git_service.rs`, `crates/egggit/src/process.rs`, Codegg worktree/lease and `ManagedProcessService` integration, and test-only daemon broker publisher. No new standalone Git daemon or raw shell spawn. A durable recovery journal or DTO addition requires separate storage/versioning review before enabling production. No Windows/macOS behavior changes in test-only proof; retain existing immutable snapshot path.

## Verification

`cargo fmt --all -- --check`; `cargo test --locked -p egggit`; `cargo test --locked --test git_mutations_integration -- --test-threads=1`; `cargo test --locked --test git_execution_origin_matrix -- --test-threads=1`; new transaction crash/restart/concurrency native tests; `python3 scripts/check_execution_ownership.py`; `python3 scripts/check_git_forbidden_patterns.py`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_adr_integrity.py`; `scripts/verify.sh quick`, exact-revision hosted native Linux M004/M005 positive and refusal cases. Include revalidation against external Git and all known linked-worktree/common-dir cases.

## Closure / stop / unblock

Future C004 closure records actual host/ABI, commit SHA, manifest schema, operation-stage fault injection, publication proof and unresolved transactional edge cases. Stop if broad `.git` writes or uncontained publisher-run helpers are necessary; if no reliable external-writer conflict check exists; if staging can overwrite concurrent index/ref edits; if partial publication has no safe recovery semantics. A test-only C004 success does not close Linux C002, admit M003, or authorize C003 transport.
