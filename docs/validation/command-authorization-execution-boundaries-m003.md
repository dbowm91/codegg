# Command authorization and execution boundaries M003 validation

## Host and claim boundary

Native Linux qualification ran on `Linux deadpool 6.8.0-146-generic`
(`x86_64`, Rust `1.99.0`). The registered filesystem backend is Landlock;
the host reports ABI 4. This establishes filesystem effects for the tested
child processes only. Landlock does not restrict network, Unix IPC, host
process visibility, or pre-opened file descriptors. No outbound networking or
IPC denial is claimed. Native macOS and Windows testing was not run for this
closure. `cargo check -p codegg --lib --target x86_64-pc-windows-gnu --locked`
passed using the installed MinGW compiler.
The macOS target check failed in native C dependencies: the host compiler does
not support Apple target flags, and the Clang retry lacks an Apple SDK/sysroot
(`bits/libc-header-start.h` is unavailable). No Rust diagnostic in changed
CodeGG code was reported.

## Equivalent filesystem effects

`tests/sandbox_containment.rs::landlock_denies_equivalent_outside_write_from_shell_python_and_native_binary`
uses one `WorkspaceWrite` launch policy and one outside-workspace marker effect
for direct shell text, nested shell, a generated shell script, Python, a
compiled native executable, and a planted Git pre-commit hook. Each process
reports obtained enforcement and the outside marker remains absent. A separate
existing fixture verifies an allowed workspace write. This is a filesystem
effect test; classifier or prompt differences do not alter the kernel result.

The exact audit tests are:

- `tests/identity_m002_live_audit_hooks.rs::bash_shell_success_emits_one_structural_event`
  checks receipt source/id, policy revision, requested profile, route, digest,
  and FullHost filesystem/network metadata, with no command text in metadata.
- `tests/identity_m002_live_audit_hooks.rs::bash_constrained_audit_reports_obtained_landlock_and_unrestricted_network`
  checks an accepted `WorkspaceWrite` command reports the actual
  `enforced:landlock:abi=4` filesystem result and `unrestricted` network.

The constrained audit test requires the trusted sibling helper to be built
first: `cargo build -p codegg --bin codegg-sandbox-helper`, then copy the
trusted helper beside the integration-test executable with
`cp target/debug/codegg-sandbox-helper target/debug/deps/codegg-sandbox-helper`.
Without a trusted sibling the launch correctly fails closed before dispatch.

## Audit contract

The existing `command_execute` action carries bounded labels and structural
digests only: permission receipt/source/revision, requested sandbox profile,
safe route label, risk level, and obtained filesystem/network outcome where
the owning route has those facts. Raw command text, arguments, generated script
contents, process output, and secret-bearing paths are excluded. FullHost
active routes are marked FullHost; timed-out raw launches say the outcome was
not reported. Typed Git keeps its owning `git_operation` event to avoid a
duplicate command event. Completed raw-shell events also record process exit
code, output-truncation status, cleanup status, and helper identity.

## Limits and follow-up

This milestone does not provide an event for every nested syscall or detailed
output/cleanup internals beyond bounded status labels. Active-route provenance
reports the executor route and its FullHost ceiling; it is not syscall-level tracing.
The current Linux Landlock tier has unrestricted networking and IPC. No
native macOS tests were run here. These are explicit evidence gaps, not inferred
enforcement. Network/IPC denial and macOS native qualification remain gated on
their own backend/platform evidence.
