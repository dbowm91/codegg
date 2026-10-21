# Linux Execution Isolation C012 — Closure Status

Status: closed negative (safe CoW snapshots qualified; fragmented cold-preparation gate remains unmet)

Source implementation plan:

- `plans/implementation/linux-execution-isolation/020-m003-no-copy-object-view-architecture-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/linux-execution-isolation-roadmap.md`

Repository baseline reviewed: `f7935ad7` (reconciled with current `main`)

Implementation and evidence commits:

- `1eede757` — reject the unenforced daemon-lock/no-writer-lease design.
- `75ef0d04` — add the hosted Btrfs CoW and snapshot-isolation qualification.
- `b609c98c`, `f5064fea`, `3844dbb7` — correct and validate the workflow and exact-content fixtures.
- `54dce1bf` — try four-worker fanout copies; hosted results showed no material improvement.
- `c997586a` — remove the unproductive concurrency experiment; final-source Btrfs run passed.

## 1. Executive finding

C012 resolves the no-copy design question without admitting a live object view.
The daemon's `WorkspaceLockTable` is an in-process mutex, does not cover
external Git/editor/shell writers, and is not held by the trusted Git read
owner. It cannot provide the full-lifetime no-writer guarantee that a live
read-only mount would require. Ordinary worktrees therefore retain private
immutable snapshots.

The existing per-file `FICLONE` path was qualified on a disposable loopback
Btrfs filesystem. It preserved snapshot contents after a source mutation and
reduced fragmented cold preparation substantially relative to the ext4
byte-copy fallback, but the final fragmented p95 was 1,109.391 ms against
C010's 1,000 ms limit. The production-admission decision remains negative:
C010 is unchanged, no Linux host is admitted for M003, Landlock remains
canonical, and no Git mutation or remote route is enabled.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Determine whether a daemon-owned no-writer lease can exclude source mutation | `WorkspaceLockTable` ownership in `crates/codegg-core/src/workspace_services.rs`; `GitExecutionService` and source-mutation probe | pass (negative design decision) | The lock is process-local and external writers do not participate. No ordinary-worktree lease can be asserted. |
| Qualify independent CoW object snapshots | Hosted [`38951802613`](https://github.com/dbowm91/codegg/actions/runs/38951802613), exact final source `c997586a`; `CODEGG_REQUIRE_OBJECT_VIEW_FICLONE=1` | pass | Btrfs FICLONE succeeded; source mutation did not alter the snapshot payload. |
| Compare exact-content packed and fragmented object stores | Same hosted run; 44,671 object IDs in each fixture, checked by sorted `cat-file --batch-all-objects` output | pass | Packed: one pack, no loose objects. Fragmented: 44,671 loose files, 358.09 MiB. |
| Meet C010 cold preparation budget | Same hosted run, 30 cold-advised samples per shape | fail | Fragmented p95 was 1,109.391 ms, narrowly over the 1,000 ms limit; max 1,202.083 ms stayed below the 3 s per-sample limit. |
| Avoid weakening trusted Git source-mutation guarantees | Existing immutable-view test; Btrfs-specific successful-FICLONE assertion and mutation | pass | No live mount, mutable hardlink, or host-object fallback was enabled. |
| Test whether bounded parallel copies materially improve cold time | Hosted [`38951277191`](https://github.com/dbowm91/codegg/actions/runs/38951277191), experimental source `54dce1bf` | fail (no material gain) | Four workers yielded 1,890.487 ms p95 versus 1,920.899 ms on the earlier serial run; change was reverted. |

## 3. Production implementation evidence

No production execution route or authorization policy changed. The existing
private object snapshot remains the correctness boundary and continues to try
Linux `FICLONE` per object file before falling back to an independent byte
copy. Test-only counters report successful reflinks and fallbacks; they are
compiled only under `cfg(test)`. The hosted workflow builds packed and
fragmented fixtures on an 8 GiB loopback Btrfs filesystem and runs the
existing cold snapshot/status harness against CodeGG's actual copier.

The final hosted runner was Ubuntu 24.04, kernel `6.17.0-1022-azure`, Git
`2.55.0`, and btrfs-progs `6.6.3`. Packed preparation median/p95/max was
0.191/0.279/0.313 ms, with three successful reflinks per sample and no
fallbacks. Fragmented preparation median/p95/max was
969.317/1,109.391/1,202.083 ms, with 44,671 successful reflinks per sample
and no fallbacks. Protected `git status` p95 was 10.100 ms against a 10.096 ms
baseline for packed, and 8.857 ms against a 7.972 ms baseline for fragmented.
These are one hosted runner and a loopback CoW image, not production host
qualification.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test --locked -p egggit trusted_git -- --test-threads=1
python3 scripts/check_adr_integrity.py
git diff --check
```

The hosted workflow additionally ran the Btrfs source-mutation test and 30
cold-advised sample pairs for each fixture shape.

### Results

- Local trusted-Git suite: 20 passed, 0 failed.
- ADR integrity guard passed; active ADR identifiers are unique and paths resolve.
- Final-source hosted Btrfs workflow `38951802613` passed all steps, including
  exact object-set comparison, required FICLONE success, mutation isolation,
  and both 30-sample shape measurements.
- Earlier same-workflow runs `38950666067` and `38951277191` passed the
  CoW and source-mutation checks; the latter tested and rejected the
  four-worker performance experiment.
- The local ext4 focused immutable-view test passed; ext4 does not support the
  CoW path and therefore exercises fallback semantics only.

## 5. Invariant review

- Trusted Git continues to consume a daemon-owned private object snapshot.
- A concurrent source mutation cannot change the bytes in a snapshot already
  admitted for a child; Btrfs FICLONE isolation was exercised directly.
- No hard links to mutable source object files were introduced.
- Repository alternates, external object directories, and untrusted metadata
  remain outside the private view.
- A failed reflink continues to use the existing byte-copy fallback; no setup
  failure falls back to host Git object discovery.
- C010 no-admission, Landlock defaults, and Git mutation/remote-route gates are
  unchanged.

## 6. Failure and recovery review

The CoW workflow fails if Btrfs cannot be created or mounted, fixtures do not
have identical object IDs, a required clone fails on Btrfs, the mutation test
reads changed source bytes, or a benchmark command fails. At runtime, a
filesystem rejecting FICLONE retains the existing safe independent byte-copy
fallback. The private snapshot remains command/cache-owned and is removed
according to its existing lease lifecycle after child completion, timeout,
cancellation, or cache eviction.

## 7. Migration and compatibility review

There is no public API, protocol, schema, persisted preference, or configuration
change. The CoW workflow is Linux-only and does not alter Windows/macOS shared
code. The four-worker copy experiment was reverted; the production copier has
no new concurrency behavior.

## 8. Security review

The no-writer lease was rejected because it cannot exclude external writers.
Readonly child mounts alone do not freeze the host object store. A live object
view remains prohibited. Btrfs snapshots are independent file inodes with
copy-on-write data semantics; source mutation isolation was tested with a
required successful ioctl. The loopback workflow does not establish admission
on physical Btrfs/XFS hosts or supportable distro/kernel combinations.

## 9. Documentation and operations

- The implementation plan records the lease decision, hosted filesystem
  details, exact fixture shapes, clone/fallback counts, distributions, and
  remaining C010 threshold failure.
- The subsystem roadmap and planning registry identify C012 as closed
  negative and retain M003's no-admission disposition.
- `.github/workflows/linux-git-object-cow.yml` provides a repeatable hosted
  CoW qualification.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | Fragmented Btrfs snapshot preparation p95 is 1,109.391 ms, above C010's 1,000 ms threshold. | Cold or invalidated views miss the agreed admission budget. | Keep M003 unadmitted. Any further optimization must repeat exact-content cold/warm qualification and source-mutation tests. |
| medium | The result is from one Ubuntu hosted runner using a loopback Btrfs filesystem. | Physical-device performance and supported-host coverage are unknown. | Treat as CoW path qualification only; do not generalize to host admission. |
| medium | M004 production PTY/LSP owners and M005 production offline-network owners remain incomplete. | The broader Linux isolation capability is not available through every production execution owner. | Continue M004 PTY/LSP ownership, then M005 execution-owner and offline-network qualification. |

## 11. Roadmap disposition

C012 closes as a bounded negative design/performance decision. Private immutable
snapshots remain required; the daemon-lock lease and live no-copy view are
rejected. The Btrfs per-file CoW path is correct and much faster than ext4
byte-copy fallback for the measured fixture, but misses the agreed fragmented
cold p95 limit and does not qualify a production host. M003 remains blocked by
C010. M004 stays active and is the next owner-integration priority; M005
remains gated on M004.

## 12. Registry updates and unblock audit

- Mark C012 closed negative in the Linux roadmap and planning registry and
  link this record.
- Keep C010's no-admission decision and M003 production block unchanged.
- M004 was already active; C012 unblocks no new implementation plan. Continue
  with PTY and LSP process ownership, then M005 production offline-network
  routing. Do not advance C002/C003 Git recovery or M003 admission from this
  result.
