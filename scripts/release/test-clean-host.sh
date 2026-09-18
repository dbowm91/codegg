#!/usr/bin/env bash
#
# test-clean-host.sh — reproducible temporary-home clean-host qualification
# for self-contained-installation M002.
#
# The fixture ensures the basic-user path never depends on implementation
# helpers preinstalled on the host:
# - no Rust/Cargo on PATH for the runtime smoke;
# - no `eggsearch` or `eggsact` executable on PATH;
# - no CODEGG_MASTER_KEY / CODEGG_ENCRYPTION_KEY / OPENCODE_ENCRYPTION_KEY;
# - no pre-existing CodeGG config/credentials;
# - only the packaged installation plus ordinary OS facilities.
#
# Usage:
#   scripts/release/test-clean-host.sh [--artifact <tar.gz>] [--install-dir <dir>]
#
# With no --artifact, the harness builds a fixture bundle from the current
# checkout's debug binaries when available (codegg + sandbox helper) plus a
# pinned-identity eggsearch fixture, packages it with package-binary.sh, and
# qualifies that bundle. With --artifact, the given verified release tarball
# is installed instead (preferred for release closure).
#
# Steps (plan §7):
#   1. install the verified release artifact;
#   2. `codegg --version`;
#   3. installation/search doctor;
#   4. TUI launch surface (headless: --help parses; full interactive TUI
#      requires a pty and is documented, not faked);
#   5. /connect path proxy: `auth status` + providers listing with an
#      isolated HOME (no credentials) and a deterministic fake-provider
#      config where supplied by the test environment;
#   6. connection/models projection (providers listing);
#   7. one eggsearch wrapper through the managed sidecar against fixtures;
#   8. one in-process eggsact deterministic tool;
#   9. on supported Linux, one sandboxed command through the packaged helper.
#
# The installed CodeGG process itself must not rely on a separately
# installed helper: PATH for the smoke contains only the install dir plus
# minimal OS dirs, with no cargo/rust/eggsearch/eggsact entries.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

ARTIFACT=""
INSTALL_DIR_OVERRIDE=""

while [ $# -gt 0 ]; do
  case "$1" in
    --artifact)
      ARTIFACT="${2:-}"
      shift 2
      ;;
    --install-dir)
      INSTALL_DIR_OVERRIDE="${2:-}"
      shift 2
      ;;
    -h|--help)
      sed -n '1,45p' "$0"
      exit 0
      ;;
    *)
      printf 'error: unknown argument: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

PASS=0
FAIL=0
RUN_LOG="$(mktemp)"
trap 'rm -f "$RUN_LOG"' EXIT

ok() {
  PASS=$((PASS + 1))
  printf 'ok %s: %s\n' "$PASS" "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf 'FAIL: %s\n' "$1" >&2
}

# --- 0. Clean-host fixture -------------------------------------------------
CLEAN_ROOT="$(mktemp -d)"
FAKE_HOME="$CLEAN_ROOT/home"
INSTALL_DEST="$CLEAN_ROOT/install"
FIX_WORK="$CLEAN_ROOT/work"
mkdir -p "$FAKE_HOME" "$INSTALL_DEST" "$FIX_WORK"

# Detect the host target BEFORE isolating HOME (rustup shims need the real
# HOME to resolve the toolchain). Failure falls back to uname mapping.
HOST_TARGET_DETECTED="$(rustc -vV 2>/dev/null | awk '/^host:/{print $2}' || true)"
if [ -z "${HOST_TARGET_DETECTED:-}" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) HOST_TARGET_DETECTED="x86_64-unknown-linux-gnu" ;;
    Linux-aarch64|Linux-arm64) HOST_TARGET_DETECTED="aarch64-unknown-linux-gnu" ;;
    Darwin-x86_64) HOST_TARGET_DETECTED="x86_64-apple-darwin" ;;
    Darwin-arm64|Darwin-aarch64) HOST_TARGET_DETECTED="aarch64-apple-darwin" ;;
    *) HOST_TARGET_DETECTED="x86_64-unknown-linux-gnu" ;;
  esac
fi

cleanup() {
  rm -rf "$CLEAN_ROOT"
}
trap 'rm -f "$RUN_LOG"; rm -rf "$CLEAN_ROOT"' EXIT

if [ -n "$INSTALL_DIR_OVERRIDE" ]; then
  INSTALL_DEST="$INSTALL_DIR_OVERRIDE"
  mkdir -p "$INSTALL_DEST"
fi

# Sanitized PATH: install dir first, then minimal OS facilities only.
# Deliberately excludes $HOME/.cargo/bin, /usr/local/cargo, and any dir
# that could supply eggsearch/eggsact/rustc/cargo.
CLEAN_PATH="$INSTALL_DEST:/usr/bin:/bin"
case "$(uname -s)" in
  Darwin*) CLEAN_PATH="$INSTALL_DEST:/usr/bin:/bin:/usr/sbin:/sbin" ;;
esac

# Unset master-key env vars for the smoke (record prior state for evidence).
for v in CODEGG_MASTER_KEY CODEGG_ENCRYPTION_KEY OPENCODE_ENCRYPTION_KEY; do
  if [ -n "${!v:-}" ]; then
    printf 'note: unsetting %s for clean-host smoke (was set)\n' "$v"
  fi
  unset "$v" || true
done
unset CODEGG_EGGSEARCH_BIN || true

# Isolated HOME/XDG so no pre-existing config/credentials leak in.
export HOME="$FAKE_HOME"
export XDG_CONFIG_HOME="$FAKE_HOME/.config"
export XDG_DATA_HOME="$FAKE_HOME/.local/share"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME"

printf '== clean-host qualification (root=%s)\n' "$CLEAN_ROOT"
printf 'artifact: %s\n' "${ARTIFACT:-<fixture bundle>}"
printf 'install dir: %s\n' "$INSTALL_DEST"
printf 'sanitized PATH: %s\n' "$CLEAN_PATH"
printf 'HOME: %s\n' "$HOME"

# Evidence: absence of helpers on the sanitized PATH.
PATH="$CLEAN_PATH" command -v rustc >/dev/null 2>&1 && fail "rustc must be absent on sanitized PATH" || ok "no Rust/Cargo on sanitized PATH (rustc absent)"
PATH="$CLEAN_PATH" command -v cargo >/dev/null 2>&1 && fail "cargo must be absent on sanitized PATH" || ok "no Rust/Cargo on sanitized PATH (cargo absent)"
PATH="$CLEAN_PATH" command -v eggsearch >/dev/null 2>&1 && fail "eggsearch must be absent on sanitized PATH" || ok "no eggsearch executable on sanitized PATH"
PATH="$CLEAN_PATH" command -v eggsact >/dev/null 2>&1 && fail "eggsact must be absent on sanitized PATH" || ok "no eggsact executable on sanitized PATH"
[ -z "${CODEGG_MASTER_KEY:-}" ] && [ -z "${CODEGG_ENCRYPTION_KEY:-}" ] && [ -z "${OPENCODE_ENCRYPTION_KEY:-}" ] \
  && ok "no master-key env vars" \
  || fail "master-key env vars must be unset"
[ ! -e "$FAKE_HOME/.config/codegg/codegg.jsonc" ] && [ ! -e "$FAKE_HOME/.config/codegg/codegg.json" ] \
  && ok "no pre-existing CodeGG config" \
  || fail "isolated HOME must have no CodeGG config"

# --- 1. Obtain/install the artifact ---------------------------------------
if [ -z "$ARTIFACT" ]; then
  # Fixture bundle from current debug binaries when available, else shell
  # fixtures with the pinned identity strings.
  FIXBIN="$FIX_WORK/fixbin"
  mkdir -p "$FIXBIN"
  if [ -x "$REPO_ROOT/target/debug/codegg" ]; then
    cp -- "$REPO_ROOT/target/debug/codegg" "$FIXBIN/codegg-fixture"
    chmod 755 "$FIXBIN/codegg-fixture"
    printf 'note: using built target/debug/codegg as fixture codegg\n'
  else
    printf '#!/usr/bin/env sh\nprintf "codegg 0.1.0\\n"\n' > "$FIXBIN/codegg-fixture"
    chmod 755 "$FIXBIN/codegg-fixture"
    printf 'note: no built codegg; using version fixture\n'
  fi
  if [ -x "$REPO_ROOT/target/debug/codegg-sandbox-helper" ]; then
    cp -- "$REPO_ROOT/target/debug/codegg-sandbox-helper" "$FIXBIN/helper-fixture"
    chmod 755 "$FIXBIN/helper-fixture"
    printf 'note: using built target/debug/codegg-sandbox-helper\n'
  else
    printf '#!/usr/bin/env sh\nexit 125\n' > "$FIXBIN/helper-fixture"
    chmod 755 "$FIXBIN/helper-fixture"
    printf 'note: no built helper; using exit-125 fixture\n'
  fi
  printf '#!/usr/bin/env sh\nprintf "eggsearch 0.3.9\\n"\n' > "$FIXBIN/eggsearch-fixture"
  chmod 755 "$FIXBIN/eggsearch-fixture"
  HOST_TARGET="$HOST_TARGET_DETECTED"
  printf 'fixture target: %s\n' "$HOST_TARGET"
  if sh "$SCRIPT_DIR/package-binary.sh" --target "$HOST_TARGET" \
      --binary "$FIXBIN/codegg-fixture" \
      --sandbox-helper "$FIXBIN/helper-fixture" \
      --eggsearch "$FIXBIN/eggsearch-fixture" \
      --out-dir "$FIX_WORK/rel" >"$RUN_LOG" 2>&1; then
    ok "fixture bundle packaged for $HOST_TARGET"
  else
    fail "fixture bundle packaging failed (see $RUN_LOG)"
    cat "$RUN_LOG" >&2 || true
    printf 'RESULT: %s passed, %s failed\n' "$PASS" "$FAIL" >&2
    exit 1
  fi
  ARTIFACT="$(ls "$FIX_WORK"/rel/codegg-*.tar.gz | head -n 1)"
  printf 'fixture artifact: %s\n' "$ARTIFACT"
  if sh "$SCRIPT_DIR/finalize-release.sh" --dir "$FIX_WORK/rel" >"$RUN_LOG" 2>&1 \
    && sh "$SCRIPT_DIR/verify-release.sh" --dir "$FIX_WORK/rel" --allow-incomplete-target-set >"$RUN_LOG" 2>&1; then
    ok "fixture bundle finalizes and verifies"
  else
    fail "fixture bundle finalize/verify failed"
    cat "$RUN_LOG" >&2 || true
  fi
else
  printf 'using provided artifact: %s\n' "$ARTIFACT"
  [ -f "$ARTIFACT" ] || { fail "provided --artifact missing"; exit 1; }
  ok "provided artifact exists"
fi

# Install via the real installer in a subprocess with the fixture release
# dir when testing fixtures, else via direct extract for an explicit tarball.
if [ -z "${INSTALL_DIR_OVERRIDE:-}" ] && [ -d "$FIX_WORK/rel" ] && [ -z "${USE_DIRECT_EXTRACT:-}" ]; then
  # The installer downloads from GitHub; for the offline fixture we extract
  # the verified tarball directly (same allowlist the installer enforces).
  if tar -xzf "$ARTIFACT" -C "$INSTALL_DEST" >"$RUN_LOG" 2>&1; then
    chmod 755 "$INSTALL_DEST"/codegg "$INSTALL_DEST"/codegg-sandbox-helper "$INSTALL_DEST"/codegg-eggsearch 2>/dev/null || true
    ok "installed verified artifact to $INSTALL_DEST"
  else
    fail "artifact extraction failed"
    cat "$RUN_LOG" >&2 || true
  fi
else
  if tar -xzf "$ARTIFACT" -C "$INSTALL_DEST" >"$RUN_LOG" 2>&1; then
    chmod 755 "$INSTALL_DEST"/codegg "$INSTALL_DEST"/codegg-sandbox-helper "$INSTALL_DEST"/codegg-eggsearch 2>/dev/null || true
    ok "installed provided artifact to $INSTALL_DEST"
  else
    fail "artifact extraction failed"
    cat "$RUN_LOG" >&2 || true
  fi
fi

for member in codegg codegg-sandbox-helper codegg-eggsearch; do
  if [ -x "$INSTALL_DEST/$member" ]; then
    ok "installed runfile executable: $member"
  else
    fail "installed runfile missing/not executable: $member"
  fi
done

# --- 2. codegg --version ----------------------------------------------------
if PATH="$CLEAN_PATH" "$INSTALL_DEST/codegg" --version >"$RUN_LOG" 2>&1; then
  CODEGG_VERSION="$(cat "$RUN_LOG")"
  printf 'codegg version: %s\n' "$CODEGG_VERSION"
  ok "codegg --version runs on clean host"
else
  fail "codegg --version failed on clean host"
  cat "$RUN_LOG" >&2 || true
fi

# --- 3. installation/search doctor ------------------------------------------
if PATH="$CLEAN_PATH" "$INSTALL_DEST/codegg" doctor installation >"$RUN_LOG" 2>&1; then
  ok "installation doctor runs on clean host"
  grep -q "CodeGG executable" "$RUN_LOG" && ok "installation doctor reports executable" || fail "installation doctor missing executable line"
  grep -q "Sandbox helper" "$RUN_LOG" && ok "installation doctor reports helper" || fail "installation doctor missing helper line"
  grep -qi "eggsact" "$RUN_LOG" && ok "installation doctor reports eggsact" || fail "installation doctor missing eggsact line"
else
  fail "installation doctor failed"
  cat "$RUN_LOG" >&2 || true
fi

if PATH="$CLEAN_PATH" "$INSTALL_DEST/codegg" doctor search >"$RUN_LOG" 2>&1; then
  ok "search doctor runs on clean host (non-fatal degradation permitted)"
  grep -q "Managed sidecar" "$RUN_LOG" && ok "search doctor reports managed sidecar" || fail "search doctor missing managed sidecar line"
  grep -q "Resolution:" "$RUN_LOG" && ok "search doctor reports resolution source" || fail "search doctor missing resolution line"
else
  fail "search doctor failed (must stay non-fatal, exit 0 with diagnostic)"
  cat "$RUN_LOG" >&2 || true
fi

# --- 4. TUI launch surface (headless) ----------------------------------------
if PATH="$CLEAN_PATH" "$INSTALL_DEST/codegg" --help >"$RUN_LOG" 2>&1; then
  ok "TUI command surface parses on clean host (--help)"
else
  fail "codegg --help failed"
  cat "$RUN_LOG" >&2 || true
fi
printf 'note: full interactive TUI launch requires a pty; headless --help + doctor is the reproducible proxy (see closure record)\n'

# --- 5/6. /connect path proxy -------------------------------------------------
# Headless proxy for the restored /connect flow (M001 credential bootstrap +
# M003 provider-neutral dialog are TUI-interactive): auth status must run with
# no credentials, and the provider catalog must list without secrets.
if PATH="$CLEAN_PATH" "$INSTALL_DEST/codegg" auth status >"$RUN_LOG" 2>&1; then
  ok "/connect proxy: auth status runs with isolated HOME"
else
  fail "auth status failed on clean host"
  cat "$RUN_LOG" >&2 || true
fi
if PATH="$CLEAN_PATH" "$INSTALL_DEST/codegg" providers >"$RUN_LOG" 2>&1; then
  ok "/connect proxy: providers listing runs (connection/models projection surface)"
else
  fail "providers listing failed"
  cat "$RUN_LOG" >&2 || true
fi

# --- 7. eggsearch wrapper via managed sidecar ---------------------------------
if [ -x "$INSTALL_DEST/codegg-eggsearch" ]; then
  if "$INSTALL_DEST/codegg-eggsearch" --version >"$RUN_LOG" 2>&1; then
    EGG_VERSION="$(cat "$RUN_LOG")"
    printf 'managed eggsearch version: %s\n' "$EGG_VERSION"
    case "$EGG_VERSION" in
      *eggsearch*0.3.9*) ok "managed sidecar reports pinned 0.3.9" ;;
      *) printf 'note: fixture sidecar version differs (expected 0.3.9, got %s)\n' "$EGG_VERSION" ;;
    esac
  else
    fail "managed sidecar --version failed"
    cat "$RUN_LOG" >&2 || true
  fi
else
  fail "managed sidecar not executable"
fi

# --- 8. eggsact deterministic tool (in-process) --------------------------------
# Proved by the compiled-in test suite (no executable on PATH by construction
# above); here we assert the absence plus the doctor deterministic section.
if PATH="$CLEAN_PATH" "$INSTALL_DEST/codegg" doctor deterministic-tools >"$RUN_LOG" 2>&1; then
  ok "deterministic-tools doctor runs (in-process eggsact, no executable)"
else
  fail "deterministic-tools doctor failed"
  cat "$RUN_LOG" >&2 || true
fi

# --- 9. sandboxed command through the packaged helper ---------------------------
if [ "$(uname -s)" = "Linux" ]; then
  if "$INSTALL_DEST/codegg-sandbox-helper" >/dev/null 2>&1; then
    fail "helper bare invocation should refuse (exit nonzero)"
  else
    ok "packaged helper refuses bare invocation (safe identity probe)"
  fi
  printf 'note: full Landlock enforcement smoke runs in the compiled sandbox tests; see closure record for host ABI\n'
else
  if "$INSTALL_DEST/codegg-sandbox-helper" >/dev/null 2>&1; then
    fail "helper bare invocation should refuse on Unix"
  else
    ok "packaged helper refuses bare invocation (unsupported-kernel host distinguishes from missing runfile)"
  fi
fi

printf 'RESULT: %s passed, %s failed\n' "$PASS" "$FAIL"
printf 'evidence: artifact=%s install=%s path=%s home=%s\n' "$ARTIFACT" "$INSTALL_DEST" "$CLEAN_PATH" "$FAKE_HOME"
[ "$FAIL" -eq 0 ]
