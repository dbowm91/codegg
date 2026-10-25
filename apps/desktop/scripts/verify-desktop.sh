#!/usr/bin/env bash
#
# verify-desktop.sh — The one local entry point for the desktop gate (M007-A).
#
# Usage:
#   apps/desktop/scripts/verify-desktop.sh             # every phase, in order
#   apps/desktop/scripts/verify-desktop.sh <phase>     # one phase
#
# Phases, in the order the hosted lane runs them:
#   boundary      the desktop production boundary guard
#   renderer      npm ci / typecheck / test / bindings:check / build
#   stage-daemon  stage the root daemon binary into the Tauri bundle
#   host          cargo test --lib / cargo fmt --check / cargo clippy -D warnings
#   e2e           the built-app visible-window trajectory (needs a display)
#
# The hosted qualification (.github/workflows/desktop-e2e.yml) calls this
# script, so the maintainer's machine and CI run the same checks and the two
# cannot drift. The script stops at the first failing command and returns its
# status.
#
# `stage-daemon` sits between the renderer checks and the host checks because
# the Tauri build script refuses to compile without the bundled daemon
# resource (`tauri.conf.json` bundles `binaries/codegg`); the host checks
# therefore cannot run before it.
#
# Environment:
#   CODEGG_DAEMON_EXECUTABLE
#     the daemon binary the desktop app runs. When unset the script uses
#     `<repo>/target/debug/codegg` and builds it if that is missing.
#   DISPLAY / WAYLAND_DISPLAY
#     an X11/Wayland session. The built-app trajectory needs a display; when
#     none is available the step is skipped and the reason is printed.

set -euo pipefail

# ── Resolve the tree from the script's own location ────────────────────────
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app_root="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$app_root/../.." && pwd)"

# The desktop host's own pinned toolchain
# (apps/desktop/src-tauri/rust-toolchain.toml pins 1.90.0 with rustfmt and
# clippy). The root workspace keeps its own toolchain; the daemon build below
# uses the ambient `cargo`, exactly as the hosted lane does.
toolchain="1.90.0"

daemon_bin=""
daemon_staged=0

# The lint command exists in two places on purpose: the hosted lane states it
# in the workflow (that is where the gate is documented, and where the gate
# guard reads it from), and this script runs it for the local gate. The two
# copies must still agree — a local gate that drifted from the hosted lane
# would be a different gate — so a mismatch fails loudly instead of running
# one set of checks silently.
check_host_lint_drift() {
  local workflow="$repo_root/.github/workflows/desktop-e2e.yml"
  [[ -f "$workflow" ]] || die "the hosted desktop workflow is missing: $workflow"
  local expected="rustup run $toolchain cargo clippy --manifest-path"
  expected+=" apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings"
  grep -qF -- "$expected" "$workflow" ||
    die "the hosted workflow no longer runs '$expected'; the local gate and the hosted lane would be different gates"
}

usage() {
  cat <<EOF
Usage: apps/desktop/scripts/verify-desktop.sh [phase]

Phases (the hosted lane's order):
  boundary      the desktop production boundary guard
  renderer      npm ci / typecheck / test / bindings:check / build
  stage-daemon  stage the root daemon binary into the Tauri bundle
  host          cargo test --lib / cargo fmt --check / cargo clippy -D warnings
  e2e           the built-app visible-window trajectory (needs a display)

With no phase, every phase runs in the order above.
EOF
}

die() {
  echo "verify-desktop.sh: $*" >&2
  exit 1
}

step() {
  echo
  echo "==> $*"
}

skip() {
  echo "==> skipping $*"
}

# ── The daemon binary ──────────────────────────────────────────────────────
# The desktop app runs a daemon binary, and the Tauri build script requires
# the bundled resource to exist before the host compiles. The binary is this
# revision's, so the script builds it when it is missing rather than
# accepting a stale one.
resolve_daemon() {
  if [[ -n "${CODEGG_DAEMON_EXECUTABLE:-}" ]]; then
    [[ -x "$CODEGG_DAEMON_EXECUTABLE" ]] ||
      die "CODEGG_DAEMON_EXECUTABLE is not an executable file: $CODEGG_DAEMON_EXECUTABLE"
    daemon_bin="$CODEGG_DAEMON_EXECUTABLE"
    return
  fi
  local candidate="$repo_root/target/debug/codegg"
  if [[ -x "$candidate" ]]; then
    daemon_bin="$candidate"
    return
  fi
  step "daemon: building the root codegg binary (the whole root workspace; minutes on a cold target)"
  echo "+ (cd $repo_root && cargo build --locked --config 'profile.dev.strip=\"none\"')"
  (cd "$repo_root" && cargo build --locked --config 'profile.dev.strip="none"')
  [[ -x "$candidate" ]] || die "the root daemon build did not produce $candidate"
  daemon_bin="$candidate"
}

stage_daemon_resource() {
  resolve_daemon
  if ((daemon_staged)); then
    echo "+ daemon resource already staged"
    return
  fi
  step "stage-daemon: stage the daemon binary into the Tauri bundle"
  echo "+ CODEGG_DAEMON_EXECUTABLE=$daemon_bin bash $app_root/scripts/stage-daemon.sh"
  CODEGG_DAEMON_EXECUTABLE="$daemon_bin" bash "$app_root/scripts/stage-daemon.sh"
  daemon_staged=1
  export CODEGG_DAEMON_EXECUTABLE="$daemon_bin"
}

# ── Phases ─────────────────────────────────────────────────────────────────
phase_boundary() {
  step "boundary: the desktop production boundary guard"
  echo "+ (cd $repo_root && ./scripts/check-desktop-boundary.sh)"
  (cd "$repo_root" && ./scripts/check-desktop-boundary.sh)
}

phase_renderer() {
  step "renderer: npm ci, typecheck, test, bindings:check, build"
  (
    cd "$app_root"
    echo "+ npm ci"
    npm ci
    echo "+ npm run typecheck"
    npm run typecheck
    echo "+ npm test"
    npm test
    echo "+ npm run bindings:check"
    npm run bindings:check
    echo "+ npm run build"
    npm run build
  )
}

phase_stage_daemon() {
  stage_daemon_resource
}

phase_host() {
  step "host: cargo test --lib, cargo fmt --check, cargo clippy -D warnings"
  # The host compiles the Tauri build script, which refuses to run without
  # the bundled daemon resource.
  stage_daemon_resource
  check_host_lint_drift
  (
    cd "$repo_root"
    echo "+ rustup run $toolchain cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib"
    rustup run "$toolchain" cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib
    echo "+ rustup run $toolchain cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check"
    rustup run "$toolchain" cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
    echo "+ rustup run $toolchain cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings"
    rustup run "$toolchain" cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
  )
}

# The built-app trajectory needs a display: the qualification asserts on a
# visible window (Tauri + the embedded WebDriver provider).
#   native  a real session (X11/Wayland, or a macOS Aqua session)
#   xvfb    no session, but `xvfb-run` can provide a virtual one (Linux CI)
#   none    no display at all — the step is skipped, and says so
display_mode() {
  if [[ -n "${DISPLAY:-}" || -n "${WAYLAND_DISPLAY:-}" ]]; then
    echo "native"
    return
  fi
  if [[ "$(uname -s)" == "Darwin" ]] && [[ "$(launchctl managername 2>/dev/null || true)" == "Aqua" ]]; then
    echo "native"
    return
  fi
  if command -v xvfb-run >/dev/null 2>&1; then
    echo "xvfb"
    return
  fi
  echo "none"
}

phase_e2e() {
  step "e2e: the built-app visible-window trajectory"
  # The trajectory builds the app with the `desktop-e2e` feature, and the
  # Tauri build script refuses to run without the bundled daemon resource.
  stage_daemon_resource
  local mode
  mode="$(display_mode)"
  case "$mode" in
    native)
      echo "+ CODEGG_DAEMON_EXECUTABLE=$daemon_bin $app_root/e2e/run-e2e.sh"
      CODEGG_DAEMON_EXECUTABLE="$daemon_bin" bash "$app_root/e2e/run-e2e.sh"
      ;;
    xvfb)
      echo "+ xvfb-run -a $app_root/e2e/run-e2e.sh"
      CODEGG_DAEMON_EXECUTABLE="$daemon_bin" xvfb-run -a "$app_root/e2e/run-e2e.sh"
      ;;
    *)
      skip "built-app trajectory: no display available (DISPLAY/WAYLAND_DISPLAY unset, no macOS Aqua session, and no xvfb-run)"
      echo "   the hosted lane runs this step under xvfb; a local run without a display cannot assert on a visible window"
      return 0
      ;;
  esac
}

# ── Entry point ────────────────────────────────────────────────────────────
main() {
  case "${1:-}" in
    "")
      step "CodeGG desktop verification: every phase"
      phase_boundary
      phase_renderer
      phase_stage_daemon
      phase_host
      phase_e2e
      echo
      echo "==> desktop verification finished"
      ;;
    boundary) phase_boundary ;;
    renderer) phase_renderer ;;
    stage-daemon) phase_stage_daemon ;;
    host) phase_host ;;
    e2e) phase_e2e ;;
    help | -h | --help) usage ;;
    *)
      echo "verify-desktop.sh: unknown phase '$1'" >&2
      usage >&2
      exit 2
      ;;
  esac
}

main "$@"
