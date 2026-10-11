# Installing CodeGG

Three installation paths are available. The prebuilt installer is the
supported, self-contained contract. The source paths exist for development and
do not ship the full managed runfile bundle.

## Requirements

**For the prebuilt installer (Linux and macOS):** a POSIX shell, plus
`uname`, `curl`, `tar`, `mktemp`, `mkdir`, `chmod`, `mv`, `cp`, `rm`, `grep`,
`awk`, and either `sha256sum` or `shasum -a 256`. The script checks for these
before any download and fails cleanly if one is missing. No Rust/Cargo,
Python runtime, or separately installed sidecar is needed at runtime.

**For a source installation:** Rust `1.89` or newer and Git to fetch the
checkout.

**For everything:** credentials for at least one configured LLM provider, and
network access to that provider.

Git is needed only for Git-backed repository operations that use it, and
language servers only for language-server features. Neither is a prerequisite
for launching CodeGG and connecting a provider.

## There are no published releases yet

> Installer support is implemented, but no GitHub releases have been
> published for this repository, so the installer currently has nothing to
> download. Until the first release exists, use one of the source paths below.
> Once releases are published, the installer commands apply exactly as written.

This was checked against the GitHub API: the releases and tags listings both
return empty.

## Prebuilt installer

The installer at `install.sh` maps your host to a release target, downloads the
release asset over a fixed HTTPS origin, verifies its SHA-256 against the
release `checksums.txt` manifest, and atomically installs the managed runfile
bundle into one user-writable directory.

It installs these siblings side by side:

| File | Purpose |
|---|---|
| `codegg` | The application you invoke. |
| `codegg-sandbox-helper` | Unix sandbox helper. On Windows the packaged `.exe` currently reports unavailable; Windows constrained requests fail before launching a target. |
| `codegg-eggsearch` | Pinned web-search sidecar, version `0.3.9`. |
| `THIRD-PARTY-NOTICES.txt` | Only present when the release carries it. |

CodeGG resolves the two helpers relative to its own executable rather than
from `PATH`, so only the install directory itself needs to be on `PATH`.

The script never escalates privileges, never edits shell profiles, never
starts background services, and never installs Rust/Cargo. A checksum mismatch
or missing asset is a hard failure — there is no automatic fallback to
compiling from source.

### Supported hosts

| OS | Architecture | Release target |
|---|---|---|
| Linux | x86_64 / amd64 | `x86_64-unknown-linux-gnu` |
| Linux | aarch64 / arm64 | `aarch64-unknown-linux-gnu` |
| macOS | x86_64 | `x86_64-apple-darwin` |
| macOS | arm64 / aarch64 | `aarch64-apple-darwin` |

Any other host fails before any download and prints the source-install
alternatives.

### Commands

Latest release (default destination `~/.local/bin/codegg`):

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | sh
```

Pinned version — the value must match a published release tag:

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | CODEGG_VERSION=0.1.0 sh
```

Custom destination:

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | CODEGG_INSTALL_DIR="$HOME/bin" sh
```

Piping to a shell executes remote code. To read it first:

```bash
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh -o install.sh
sh install.sh
```

`CODEGG_VERSION` and `CODEGG_INSTALL_DIR` are the installer's only supported
configuration surface. There is deliberately no download-URL or mirror
override, because such a variable would turn the installer into arbitrary
remote-code execution by configuration.

After installation, put the destination directory on `PATH` if it is not
already discoverable. The installer prints that guidance when needed.

## From source

`cargo install` installs both `codegg` and `codegg-sandbox-helper` — they are
two `[[bin]]` targets with no required features, so Cargo installs both. It
does **not** produce a working managed bundle, because the third managed
sibling is missing:

- The pinned `codegg-eggsearch` sidecar is an upstream project
  (`https://github.com/eggstack/eggsearch`, tag `v0.3.9`) with no Cargo
  dependency in this repository. You must build it from the pinned tag and
  place it next to `codegg` as `codegg-eggsearch`, or point CodeGG at a binary
  you already have via a `[search.eggsearch].command` or `[mcp.eggsearch]`
  override.

```bash
git clone https://github.com/dbowm91/codegg.git
cd codegg
cargo install --locked --path .
```

CodeGG is not published on crates.io, so `cargo install codegg` does not
resolve. Publication remains a manual maintainer step.

Check what is missing at any time:

```bash
codegg doctor installation
```

That report names the resolved executable, the install directory it inferred,
and whether each managed sibling is present. A missing sidecar is reported
with the reinstall-or-override remedy.

## Running from a checkout

For iterating on CodeGG itself, run it in place rather than installing:

```bash
cargo run -- --help
```

`cargo run --` with no further arguments launches the interactive TUI and
blocks until you exit it.

Helper resolution follows the executable, so a `cargo build` (not
`cargo install`) layout leaves `codegg-sandbox-helper` in
`target/debug/` next to `codegg`. You still need to supply `codegg-eggsearch`
yourself.

### Cargo features

The default build includes the TUI and clipboard support. These features are
worth enabling:

| Feature | Effect |
|---|---|
| `server` | HTTP/WebSocket server and the remote attach client. Adds the `server` and `attach` subcommands. |
| `plugins` | WASM plugin runtime. |
| `image` | Terminal image support. |

```bash
cargo install --locked --path . --features server,plugins
```

Do not build with `--all-features`: it pulls in test-only feature sets that
are not part of the user-facing contract.

## Upgrading

`codegg upgrade` updates an existing managed prebuilt bundle as one verified
transaction on the supported Linux and macOS targets. It downloads
`checksums.txt` and the single target archive, verifies the archive checksum
before extraction, rejects unexpected archive members, checks the CodeGG,
eggsearch, and helper identities it staged, and only then commits the complete
runfile set — rolling back and preserving the prior bundle on any failure.

It does **not** fetch or execute `install.sh`; the fresh-install script
remains a separate bootstrap path. Windows, unsupported targets, and
installations outside the qualified sibling layout receive version-pinned
manual fresh-install guidance instead of an in-place replacement.

Nothing upgrades automatically. `autoupdate` in config is inert, and the
upgrade path runs only when you invoke it.

See `architecture/upgrade.md` for the ownership and failure contract, and
`RELEASING.md` for release provenance, eggsearch pinning, and the Windows
best-effort archive.

## Shell completions

```bash
codegg completions zsh > ~/.zsh/completions/_codegg
```

Accepted shells are `bash`, `elvish`, `fish`, `powershell`, and `zsh`. With no
`--output` the script goes to stdout, so redirect or pipe it where you want it.
Pass `--output <dir>` to write a file instead; that directory must already
exist, or the command fails with `Output directory does not exist`.

## Troubleshooting

For startup failures, permission problems, LSP and MCP issues, config that
does not load, and daemon connection errors, see `docs/TROUBLESHOOTING.md`.

For provider credentials and model selection, see `docs/providers.md`.
