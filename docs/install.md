# Installing CodeGG

## Requirements

A source installation requires Rust 1.89 or newer and Git. CodeGG also needs network access to the selected model provider and a credential for that provider.

## Prebuilt installer

The installer supports Linux and macOS on x86_64 and aarch64. It installs the CodeGG executable, the `codegg-sandbox-helper`, and the `codegg-eggsearch` search sidecar in one user-writable directory. Rust and Cargo are not needed to run the installed bundle.

No GitHub release is currently published, so the installer has no bundle to download. The release listing was checked on 2026-11-23. When a release is available, the latest-release command is:

```sh
curl -fsSL https://raw.githubusercontent.com/dbowm91/codegg/main/install.sh | sh
```

The installer accepts these settings:

| Variable | Purpose |
|---|---|
| `CODEGG_VERSION` | Select a published release version. |
| `CODEGG_INSTALL_DIR` | Choose the installation directory. The default is `$HOME/.local/bin`. |

The installer verifies the release checksum before installing. It does not escalate privileges, edit shell profiles, start services, or compile from source. Piping a remote script to a shell executes it; download the script first if you want to inspect it.

Supported targets are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `aarch64-apple-darwin`. Other targets need a source build.

## From source

The `codegg` executable lives in the `codegg-cli` package, so install that package:

```bash
git clone https://github.com/dbowm91/codegg.git
cd codegg
cargo install --locked --path crates/codegg-cli
```

Installing the workspace root instead (`cargo install --locked --path .`) installs nothing, because the root package has no binary of its own.

The sandbox helper must sit next to the `codegg` executable. Build it and copy it into the same directory:

```bash
cargo build --locked --release -p codegg-sandbox-helper
cp target/release/codegg-sandbox-helper "$(dirname "$(command -v codegg)")"
```

Search uses the `codegg-eggsearch` sidecar next to the executable. An `eggsearch` executable on `PATH`, or an explicit `[search.eggsearch].command`, also work. CodeGG does not ship that sidecar in a source build; build it from its upstream project. Use `codegg doctor installation` to inspect the installed files and `codegg doctor search` to check search availability.

CodeGG is not published on crates.io, so `cargo install codegg` is not an alternative.

To run from a checkout:

```bash
cargo run -- --help
```

## Upgrading

`codegg upgrade` updates a managed prebuilt installation. It does not install releases for a source build. For source installations, update the checkout and run `cargo install --locked --path crates/codegg-cli` again.

## Shell completions

Print a completion script and redirect it to the appropriate location:

```bash
codegg completions zsh > ~/.zsh/completions/_codegg
```

The supported shell names are `bash`, `elvish`, `fish`, `powershell`, and `zsh`. The output directory must already exist when using `--output`.

## Troubleshooting

For installation diagnostics, run `codegg doctor installation`. For other startup, provider, permission, or daemon problems, see [docs/TROUBLESHOOTING.md](TROUBLESHOOTING.md).

## Source verification

Checked 2026-11-23 against `install.sh`, `Cargo.toml`, `src/install.rs`, `src/upgrade/mod.rs`, and `crates/codegg-cli/src/main.rs`. The GitHub Releases listing was empty when checked.
