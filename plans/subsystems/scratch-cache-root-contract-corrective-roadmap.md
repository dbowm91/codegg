# Scratch Cache Root Contract Corrective — Subsystem Roadmap

Status: proposed (opened 2026-10-25 by the owner's direction)

## 1. What this line is

A **corrective** line for one defect: the integration test
`scheduler_jobs::scratch_cache_root_contract` asserts a **Linux-only** cache-root
contract, so it fails on macOS, and — because the allocator's test redirect is
unreachable across the crate boundary — it resolves the **developer's real**
cache root and writes a session scratch tree into it.

**The product is right and the test is wrong.** The allocator resolves its
cache root with `dirs::cache_dir()` (`src/scratch.rs:132`), which is the
platform-correct resolution. The test hard-codes the XDG convention
(`<home>/.cache`, and `XDG_CACHE_HOME` as the override), which Linux honours
and macOS does not. Fixing the *product* to make the test pass would be
regressing a correct allocator to satisfy a wrong expectation.

This line therefore corrects the **test and the test seam**, and closes the
residue hazard with them.

## 2. The defect, stated exactly

`tests/scheduler_jobs/scratch_cache_root_contract.inc` holds two members:

| Member | Asserts | On macOS |
|---|---|---|
| `the_default_cache_root_is_the_home_directory_cache` (`:60`) | without `XDG_CACHE_HOME`, the scratch root lives under `<home>/.cache` | `dirs::cache_dir()` returns `~/Library/Caches`, so the root is elsewhere → **fails** |
| `an_explicit_cache_home_is_the_root_the_allocator_uses` (`:83`) | with `XDG_CACHE_HOME` set, the scratch root lives under that directory | macOS ignores `XDG_CACHE_HOME`, so the root is the developer's **real** `~/Library/Caches` → **fails**, and it has written there |

Both failures were proven **pre-existing on `origin/main`** at `678728662`;
nothing in the M007 desktop GUI parity line touches them.

### The residue hazard, which is the worse half

The allocator's redirect hook is `#[cfg(test)]`:

- `src/scratch.rs:128-131` — `#[cfg(test)] if let Some(root) = test_scratch_root() { return Ok(root); }`
- `src/scratch.rs:175` — `pub(crate) struct TestScratchRoot` is `#[cfg(test)]`

`#[cfg(test)]` compiles **only** for unit tests inside the `codegg` crate. An
integration test under `tests/` links the library as an ordinary dependency,
so `TestScratchRoot` does not exist there and `test_scratch_root()` is
unreachable. The fixture therefore cannot redirect the allocator, and the
allocator resolves the **developer's own** cache root.

That is the behaviour M008 §16 forbids: no test, census or sweep may run
against the developer's real scratch root. It is also how a **machine's**
cache accumulates abandoned session trees.

## 3. Milestone decomposition

| Milestone | Scope | Dependency | Status |
|---|---|---|---|
| C001 | The cache-root contract is platform-aware, the fixture never reaches the developer's real cache root, and the redirect seam works across the crate boundary | none — the defect is self-contained | **ready for handoff** — `plans/implementation/scratch-cache-root-contract-corrective/001-platform-aware-cache-root-contract.md` |

## 4. Closure boundary

This line's closure evidence is C001's closure record at
`plans/closure/scratch-cache-root-contract-corrective/001-status.md`. Per
`plans/003-planning-process.md:249` no part of it may be marked complete
solely because its infrastructure landed — here there is no infrastructure
milestone, and C001 **is** the work.

## 5. What this line must not do

- **No change to the allocator's resolution.** `dirs::cache_dir()` is the
  platform-correct answer. The corrective is not a licence to change it.
- **No new product capability.** The scratch allocator's behaviour is not in
  scope; only the *test* and the *test seam* are.
- **No change to the managed agent scratch roadmap's product scope.** This
  line is a corrective and does not reopen
  `plans/subsystems/managed-agent-scratch-roadmap.md`'s milestones.
