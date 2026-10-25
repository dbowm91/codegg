# Scratch Cache Root Contract Corrective — Milestone C001 — A Platform-Aware Cache-Root Contract

Status: ready for handoff

Repository baseline: `6e8a6bb60` — `origin/main` with M007-A, M007-B and M007-C
landed.

Source roadmap:

- `plans/subsystems/scratch-cache-root-contract-corrective-roadmap.md`

Planning basis:

- The owner's direction of 2026-10-25: register a corrective milestone for the
  pre-existing macOS failure of
  `scheduler_jobs::scratch_cache_root_contract`, after the M007 desktop GUI
  parity line closes. The defect is **pre-existing**, proven on `origin/main`
  at `678728662`, and is untouched by that line.
- `plans/002-long-term-roadmap.md:899` — "Avoid accumulating incompatible local
  meanings in phase plans." A test that asserts one platform's convention
  while the product implements the platform-correct behaviour is exactly that.

Long-term requirements:

- `plans/000-long-term-specification.md` §4.5 "Locality by default" — a
  process's own cache is its own, and a test process's cache is the test's,
  never the developer's.
- The managed agent scratch foundation's contract: the per-user cache root is
  the allocator's, and the product refuses a root another user could write to.

Applicable architecture:

- `architecture/` — the managed agent scratch spaces' trust boundary and the
  per-user cache-root ownership.

Predecessor closure:

- None. The defect is self-contained: it needs no ADR, no product milestone and
  no other corrective.

Primary class: infrastructure

---

## 1. Objective

Make the cache-root contract **platform-aware**, and make it impossible for
the fixture to reach the developer's real cache root.

Two things are wrong today, and only one of them is a test failure:

1. **The expectation is Linux-only.** The fixture asserts `<home>/.cache` and
   `XDG_CACHE_HOME`. The allocator resolves `dirs::cache_dir()`
   (`src/scratch.rs:132`), which is `~/Library/Caches` on macOS and honours
   `XDG_CACHE_HOME` on Linux. The two failures are this mismatch.
2. **The fixture cannot redirect the allocator.** The redirect hook is
   `#[cfg(test)]` (`src/scratch.rs:128-131`, `:175`), so it does not exist in
   the library an integration test links. The fixture therefore resolves the
   **developer's real** cache root and writes a session scratch tree into it.

The second is the worse half, and it is the one a failure message does not
show. It is also what M008 §16 forbids: no test may run against the
developer's real scratch root.

## 2. What the corrective must do

### 2.1 A redirect seam that works across the crate boundary

The allocator needs a way to be pointed at a scratch tree that an
**integration** test can reach. Two shapes are acceptable, and the implementer
chooses:

- an **environment variable** the allocator honours — read at the same point
  `test_scratch_root()` is read today, and named so it cannot be confused with
  a product setting; or
- a **feature-gated** redirect compiled into the library for integration
  tests, so the seam exists only when the test suite is built.

A `#[cfg(test)]` hook cannot be reused: it is invisible to integration tests by
construction. Whatever is chosen must be **unreachable in a shipped build** —
if it is environment-driven, it must be gated so a product process ignores it,
and the gate must be provable.

### 2.2 A platform-aware expectation

The fixture's expectation must be derived from the platform's actual cache
resolution, not hard-coded to the XDG convention. The member must assert:

- the default resolution: the scratch root lives under the root the platform's
  cache resolution yields — `dirs::cache_dir()` joined with the allocator's
  own sub-path; and
- the explicit resolution: the scratch root lives under the root the caller
  supplies, **through the seam from §2.1**, on every platform.

The existing canonicalisation of the temporary directory stays: the allocator
refuses a path with a symlinked component, and a temporary directory's
ancestors can contain one (`/tmp` is a symlink on macOS).

### 2.3 The refusal the allocator already makes, preserved

The fixture already makes the temporary cache root private (`0o700`) because
the allocator refuses a root another user could write to. That stays, and the
explicit-resolution member must keep asserting it.

## 3. Out of scope — explicit non-goals

- **No change to the allocator's resolution.** `dirs::cache_dir()` is correct
  on every platform; the product is not what is broken.
- **No new product configuration.** The seam is a test seam, not a setting a
  user can set.
- **No change to the scratch tree's layout, naming or ownership contract.**
- **No change to the managed agent scratch roadmap's scope.** This milestone
  does not reopen it.
- **No broad sweep.** The milestone touches one integration-test family and
  the allocator's root resolution; it runs the narrowest tests that cover
  them.

## 4. Required production changes

| File | Change |
|---|---|
| `src/scratch.rs` | the redirect seam, reachable from an integration test and provably unreachable in a shipped build |
| `tests/scheduler_jobs/scratch_cache_root_contract.inc` | a platform-aware expectation, driven through the seam, asserting that the fixture's root is under the supplied root and **not** under the developer's real cache root |

If the implementer finds the seam can live entirely in the test — for example
by making the assertion compare against a root the allocator can be shown to
use without a redirect — then `src/scratch.rs` does not change, and the
closure record must say why the developer's real root cannot be reached.

## 5. Required tests

1. Both existing members pass **on macOS and on Linux**, with the same
   expectations derived per platform rather than per platform-branch.
2. A new assertion, in the same member, that the resolved scratch root is
   **not** under the developer's real cache root. This is the assertion that
   would have caught the residue hazard.
3. The allocator's own unit tests still pass, and the `#[cfg(test)]` unit-test
   seam still works for the crate's own tests.

## 6. Required verification commands

```bash
cargo test --test scheduler_jobs --locked -- scratch_cache_root_contract
cargo test -p codegg --locked -- scratch
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

**Never** run the member against the developer's real scratch root. If the
seam is not in place, the milestone is not ready to run — that is a stop
condition, not something to work around.

## 7. Acceptance criteria

1. `scheduler_jobs::scratch_cache_root_contract` passes on macOS and Linux.
2. The fixture's expectation is derived from the platform's cache resolution,
   not hard-coded to one platform's convention.
3. The resolved scratch root is provably **not** under the developer's real
   cache root, and an assertion enforces it.
4. The redirect seam is unreachable in a shipped build, and that is provable
   rather than asserted.
5. The allocator's resolution is unchanged: `dirs::cache_dir()` on every
   platform.
6. No product capability, configuration or layout change lands.
7. `scripts/verify.sh quick` is green, and fmt and workspace clippy with
   `-D warnings` are clean.

## 8. Stop conditions

The implementer **stops and reports** rather than deciding any of these:

1. **Making the seam unreachable in a shipped build turns out to require a
   product-visible switch.** A user-settable environment variable that moves
   the scratch tree is a **product capability** with a trust consequence, not
   a test seam, and it is out of scope. Report it; a feature-gated seam is
   the in-scope answer.
2. **The platform's cache resolution cannot be observed from a test without a
   product change.** Report it. The alternative is to assert the contract the
   allocator actually implements, which the implementer must not invent.
3. **The defect turns out to be wider than the fixture** — for example if the
   allocator's own refusal logic is also platform-specific. Report the
   evidence; a second milestone may be needed.
4. **Any change to the scratch tree's ownership or naming contract would be
   required.** Report it.

## 9. Closure evidence required

1. The two members' real output on **macOS**, and the same members' real
   output on **Linux** (a CI lane or a colleague's run, named).
2. The new assertion that the resolved root is not under the developer's
   real cache root, with its output.
3. The seam's shape, quoted, and the evidence that a shipped build ignores it.
4. The commands in §6 with their real outcomes.
5. A statement that `dirs::cache_dir()` is still the allocator's resolution,
   quoted from the source.
6. The developer's real cache root inspected before and after the run, with no
   new session tree under it.
