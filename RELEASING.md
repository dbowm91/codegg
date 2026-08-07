# Releasing CodeGG

This document describes the manual crates.io release procedure for the
CodeGG workspace. There is no automated release pipeline. All steps
are maintainer-operated and sequential.

## Scope and ownership

- crates.io publication is manual; version and cadence are maintainer decisions.
- GitHub Actions does not publish, create tags, create releases, or hold
  crates.io credentials.
- GitHub tags and binary releases are optional separate actions performed
  manually after crates.io publication.
- The maintainer who runs `cargo publish` becomes the publishing owner of
  that crate on the registry. crates.io ownership and crates.io name
  availability are distinct from local Cargo authentication.

## Three distinct preflight concepts

`RELEASING.md` separates three concerns that are easy to conflate:

1. **Cargo authentication.** Confirms that the local `cargo` invocation
   is authenticated to publish as you. This is set up with `cargo login`
   or a credential provider and has nothing to do with whether any
   crate exists yet.
2. **Crate-name existence / availability.** Asks whether a given name
   already exists on crates.io. A 404 ("does not exist") means the name
   is unused. A 200 response means another account has already taken
   the name; you cannot reuse it without the existing owner's
   cooperation. A 200 response to the JSON API also lists the current
   owner(s). On the *initial* release of this workspace, all ten names
   are expected to return 404.
3. **Existing-crate owner membership.** Asks whether the authenticated
   maintainer is in the owner list for a crate that *already* exists.
   `cargo owner --list <name>` answers this. The command is meaningless
   for a name that does not yet exist; it will return an error and
   nothing else.

Do not mix these three concepts. The original guide conflated them in
step 1; this corrected guide separates them so a maintainer can tell
why a preflight step succeeded or failed.

## Package graph

The workspace publishes 10 crates to crates.io in topological order.
The layer column comes from `cargo metadata` and the manifests at the
accepted M006 implementation head. The authoritative table is
`plans/closure/development-verification-release/006-package-inventory.md`.

| Layer | Package | Internal dependencies (normal/build) |
|---|---|---|
| 0 | `codegg-config` | — |
| 0 | `codegg-protocol` | — |
| 0 | `codegg-git` | — |
| 0 | `eggcontext` | — |
| 0 | `egggit` | — |
| 0 | `eggsentry` | — |
| 0 | `egglsp` | — |
| 1 | `codegg-providers` | `codegg-config` |
| 2 | `codegg-core` | `codegg-config`, `codegg-git`, `codegg-protocol`, `codegg-providers`, `egggit`, `egglsp`, `eggsentry` |
| 3 | `codegg` | `codegg-config`, `codegg-core`, `codegg-git`, `codegg-protocol`, `codegg-providers`, `eggcontext`, `egggit`, `egglsp`, `eggsentry` |

Packages within a layer can be published in any order. Layers must be
published bottom-up: layer 0 first, then 1, 2, 3.

All packages are tightly coupled at version `0.1.0` with exact version
requirements (`=0.1.0`). A version bump in any package requires a
coordinated bump across all packages that depend on it.

## Prerequisites

- Rust 1.81+ stable toolchain
- Clean git working tree on `main`
- `cargo login` completed (token stored in `~/.cargo/credentials.toml`)
  **or** a credential provider configured. Do not print or commit the token.
- crates.io account authorized to publish under the maintainer's name.
  For an *initial* release, ownership is established by the first
  successful `cargo publish`. For a *subsequent* release, the
  authenticated maintainer must already be in the crate's owner list.

## Release procedure

### Step 1 — Clean-tree and authentication preflight

```bash
git switch main
git pull --ff-only
git status --short
```

Confirm the working tree is clean. Then confirm your Cargo authentication:

```bash
cargo login --help   # documentation only; do not expose token
cargo whoami         # prints the crates.io username cargo will publish as
```

`cargo whoami` returning the expected username is the canonical
authentication preflight.

### Step 2 — Crate-name existence and ownership preflight

This step has two different shapes depending on whether the workspace
has been published before.

#### Initial release (this is the first time these names are being published)

For each of the ten package names, confirm the name is unused on
crates.io. A `404` response (or `"crate '<name>' does not exist"` from
the JSON API) means the name is available and the first successful
publisher will establish ownership.

```bash
for name in codegg codegg-core codegg-config codegg-protocol \
            codegg-providers codegg-git eggsentry eggcontext \
            egggit egglsp; do
    code=$(curl -s -o /dev/null -w "%{http_code}" \
        -A "codegg-release/1.0" \
        "https://crates.io/api/v1/crates/$name")
    echo "$name: HTTP $code"
done
```

- All ten return `404`: proceed to Step 3.
- Any name returns `200`: a third party has already taken that name.
  **Stop.** Treat this as a maintainer decision blocker — do not rename
  automatically, do not use a different spelling. The maintainer must
  decide whether to negotiate release under the existing crate, change
  the package name in the workspace (which is a separate multi-crate
  rename), or abandon publication.

Do **not** run `cargo owner --list` for these names during the initial
release. The command will fail because the names do not yet exist, and
that failure is expected — it is not an authentication or ownership
problem.

#### Subsequent release (the names already exist on crates.io)

For each of the ten package names, confirm the authenticated
maintainer is in the owner list **before** doing any irreversible
publication:

```bash
for name in codegg codegg-core codegg-config codegg-protocol \
            codegg-providers codegg-git eggsentry eggcontext \
            egggit egglsp; do
    cargo owner --list "$name"
done
```

- All ten list your username: proceed to Step 3.
- Any name does not list your username: **stop.** Do not publish. The
  maintainer must be added to the owner list (by an existing owner) or
  release authority must be transferred before publication.

### Step 3 — Version and dependency preparation

For a subsequent release, update versions in all workspace crates and
update the path-plus-version dependency requirements. All tightly
coupled packages must change versions together. See
`plans/closure/development-verification-release/006-package-inventory.md`
for the exact internal-dependency table that must move together.

For an initial release at `0.1.0`, no version preparation is needed.

Confirm the working tree remains clean:

```bash
git status --short
```

### Step 4 — Verify

```bash
scripts/verify.sh full
```

This must exit 0 before proceeding. Do not duplicate a divergent
command list; the script is the single source of truth.

### Step 5 — Package inspection

Inspect each package in topological order to confirm the packaged
contents are correct:

```bash
cargo package -p codegg-config --list
cargo package -p codegg-protocol --list
cargo package -p codegg-git --list
cargo package -p eggcontext --list
cargo package -p egggit --list
cargo package -p eggsentry --list
cargo package -p egglsp --list
cargo package -p codegg-providers --list
cargo package -p codegg-core --list
cargo package -p codegg --list
```

Inspect for:

- missing source or migration files
- missing README/license
- generated files required for compilation
- accidental large fixtures, target artifacts, local databases, logs, secrets,
  or planning evidence
- path dependencies that normalize without a registry version

The root `codegg` package currently includes development and planning
files because the root `Cargo.toml` does not declare an explicit
`[package] exclude`. Until a future corrective plan adds that exclude,
either add it before publishing `codegg` or stop at layer 2 for the
initial release. Do not silently publish the root crate with planning
content attached.

### Step 6 — Dry-run in topological order

Leaf crates (layer 0) must pass before dependents (layers 1, 2, 3):

```bash
# Layer 0 (leaves) — these must pass
cargo publish --dry-run -p codegg-config
cargo publish --dry-run -p codegg-protocol
cargo publish --dry-run -p codegg-git
cargo publish --dry-run -p eggcontext
cargo publish --dry-run -p egggit
cargo publish --dry-run -p eggsentry
cargo publish --dry-run -p egglsp

# Layer 1 — blocked until layer 0 is published
cargo publish --dry-run -p codegg-providers

# Layer 2 — blocked until layer 0 and layer 1 are published
cargo publish --dry-run -p codegg-core

# Layer 3 — blocked until layers 0, 1, 2 are published
cargo publish --dry-run -p codegg
```

Interpretation:

- **Leaf crate dry-run exits 0**: publication is ready.
- **Dependent crate dry-run exits non-zero** with
  `no matching package named '<dep>' found / location searched: crates.io index`:
  expected — the dependency has not been published yet. Publish the
  dependency first, then re-run. Record this as
  `blocked until dependency publication`, **not** as a failure.

### Step 7 — Irreversible publication

**Do not execute these commands until you have completed steps 1–6 and
recorded the decisions.**

Publication must follow the exact topological order. After each leaf
publication, verify the registry can resolve that exact name/version
before publishing a dependent that references it.

#### Layer 0 publication

```bash
cargo publish -p codegg-config
cargo publish -p codegg-protocol
cargo publish -p codegg-git
cargo publish -p eggcontext
cargo publish -p egggit
cargo publish -p eggsentry
cargo publish -p egglsp
```

After each `cargo publish`, verify registry availability for the package
you just published — **not** for a package that depends on it. The
checks below query the exact names that were just published:

```bash
cargo search codegg-config  --limit 1
cargo search codegg-protocol --limit 1
cargo search codegg-git     --limit 1
cargo search eggcontext     --limit 1
cargo search egggit         --limit 1
cargo search eggsentry      --limit 1
cargo search egglsp         --limit 1
```

`cargo search` queries the live registry index. crates.io index
propagation typically completes within a few seconds. A bounded retry
loop is acceptable — for example, up to 10 attempts spaced 5 seconds
apart, then stop and report. Do not run an unbounded tight loop:

```bash
# Bounded retry helper (run for each just-published package)
name="codegg-config"
for _ in $(seq 1 10); do
    if cargo search "$name" --limit 1 2>/dev/null | grep -q "$name"; then
        echo "$name propagated"
        break
    fi
    sleep 5
done
```

The same `cargo search <just-published-name>` check is the correct
shape for every layer below. Do not query a dependent package before it
is published as proof that its dependency propagated.

#### Layer 1 publication

```bash
cargo publish -p codegg-providers
cargo search codegg-providers --limit 1
```

#### Layer 2 publication

```bash
cargo publish -p codegg-core
cargo search codegg-core --limit 1
```

#### Layer 3 publication

```bash
cargo publish -p codegg
cargo search codegg --limit 1
```

### Step 8 — Partial failure and immutability

- **Successful versions cannot be overwritten.** An immutable published
  version is never replaced or retried as mutable state.
- **Fix and bump**: if a published version is defective, prepare a new
  version (patch bump) and publish that.
- **Yanking is not deletion**: `cargo yank --version <name> <version>`
  removes the version from the default install resolution but the
  tarball remains accessible to existing `Cargo.lock` files.
- **Do not blindly rerun the same version**: it will fail with "version
  already exists".
- **Record which packages were successfully published** before continuing.
  If the process is interrupted, resume from the next unpublished package.
- **Index propagation failures are not publish failures.** If
  `cargo search` does not return the just-published package within the
  bounded retry window, that is a registry-side delay. Do not re-run
  `cargo publish` for the same version. The dependent's dry-run will
  succeed once the registry index catches up; if it does not within a
  reasonable wait, treat that as a stop condition and report.

### Step 9 — Tags and optional GitHub binary release

After crates.io publication, optionally create a Git tag and binary
release. These are manual and separate — they do not trigger automation:

```bash
git tag -a v<VERSION> -m "Release v<VERSION>"
git push origin v<VERSION>
```

To create a GitHub Release with pre-built binaries:

```bash
gh release create v<VERSION> \
  --title "Release v<VERSION>" \
  --generate-notes \
  release/codegg-* \
  release/checksums.txt
```

Build targets (one binary per target, hosted release remains optional):

The current release artifact is the single `codegg` executable for each target;
there are no separately packaged daemon and TUI binaries. This matches the
measured no-split topology decision and preserves the user-scoped singleton
daemon discovery contract.

```bash
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
cargo build --release --target x86_64-pc-windows-msvc
```

### Step 10 — Installation verification

After an actual crates.io release, verify end-user installation:

```bash
cargo install codegg --version <VERSION>
```

Source/development installation remains:

```bash
cargo install --path .
```

## Concurrent releases

Only one maintainer should execute the release sequence at a time.
Confirm no parallel release is underway before starting step 7. If a
concurrent release is detected mid-sequence, stop after the current
`cargo publish` returns, record which packages were published, and
coordinate with the other maintainer before resuming.
