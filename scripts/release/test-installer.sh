#!/usr/bin/env bash
#
# test-installer.sh — focused offline tests for the M002 verified installer.
# No network, no real GitHub release, no publication. Uses fixture release
# directories built with the M001 helpers plus crafted malicious archives.
#
# Usage:
#   scripts/release/test-installer.sh
#
# The installer under test is <repo>/install.sh (POSIX sh). Unit tests source
# it with CODEGG_INSTALL_LIB_ONLY=1; integration tests execute it in a
# subprocess via `sh` with _CODEGG_INSTALLER_TEST_RELEASE_DIR pointing at a
# local fixture release directory.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
INSTALL_SH="$REPO_ROOT/install.sh"

PKG="$SCRIPT_DIR/package-binary.sh"
FIN="$SCRIPT_DIR/finalize-release.sh"

SH="$(command -v sh)"

PASS=0
FAIL=0

ok() {
    PASS=$((PASS + 1))
    printf 'ok %s: %s\n' "$PASS" "$1"
}

fail() {
    FAIL=$((FAIL + 1))
    printf 'FAIL: %s\n' "$1" >&2
}

# expect_pass <desc> <command...>: command must exit 0.
expect_pass() {
    local desc="$1"; shift
    if "$@" >"$RUN_LOG" 2>&1; then
        ok "$desc"
    else
        fail "$desc (expected success, got failure; see $RUN_LOG)"
        cat "$RUN_LOG" >&2 || true
    fi
}

# expect_fail <desc> <command...>: command must exit nonzero.
expect_fail() {
    local desc="$1"; shift
    if "$@" >"$RUN_LOG" 2>&1; then
        fail "$desc (expected failure, got success)"
    else
        ok "$desc"
    fi
}

# iu_out_eq <desc> <expected> <function> [args...]: sourced installer
# function must exit 0 and print exactly <expected>.
iu_out_eq() {
    local desc="$1" expected="$2"; shift 2
    if "$@" >"$GOT_LOG" 2>&1; then
        local got
        got="$(cat "$GOT_LOG")"
        if [ "$got" = "$expected" ]; then
            ok "$desc"
        else
            fail "$desc (expected [$expected], got [$got])"
        fi
    else
        fail "$desc (expected success, got failure; see $GOT_LOG)"
        cat "$GOT_LOG" >&2 || true
    fi
}

# iu_fail <desc> <function> [args...]: sourced installer function must fail.
iu_fail() {
    local desc="$1"; shift
    if "$@" >"$GOT_LOG" 2>&1; then
        fail "$desc (expected failure, got success)"
    else
        ok "$desc"
    fi
}

ROOT="$(mktemp -d "${TMPDIR:-/tmp}/codegg-installer-tests.XXXXXX")"
chmod 700 "$ROOT"
cleanup_root() {
    rm -rf -- "$ROOT"
}
trap cleanup_root EXIT HUP INT TERM

RUN_LOG="$ROOT/run.log"
GOT_LOG="$ROOT/got.log"

# --- source the installer for unit tests (no main, no side effects) -----------
# shellcheck disable=SC2034 # consumed by install.sh on sourcing
CODEGG_INSTALL_LIB_ONLY=1
# shellcheck disable=SC1090,SC1091 # sourced file is install.sh by construction
. "$INSTALL_SH"

# Host target for fixture integration runs (same uname mapping as installer).
HOST_OS="$(uname -s)"
HOST_MACH="$(uname -m)"
case "$HOST_OS/$HOST_MACH" in
    Linux/x86_64 | Linux/amd64) HOST_TARGET="x86_64-unknown-linux-gnu" ;;
    Linux/aarch64 | Linux/arm64) HOST_TARGET="aarch64-unknown-linux-gnu" ;;
    Darwin/x86_64) HOST_TARGET="x86_64-apple-darwin" ;;
    Darwin/arm64 | Darwin/aarch64) HOST_TARGET="aarch64-apple-darwin" ;;
    *) echo "test host $HOST_OS/$HOST_MACH is outside the installer matrix; fixture integration needs a supported host" >&2; exit 1 ;;
esac
HOST_ASSET="codegg-$HOST_TARGET.tar.gz"

FIXBIN_DIR="$ROOT/fixtures"
mkdir -p -- "$FIXBIN_DIR"

# Fixture "codegg": POSIX sh executable supporting --version.
make_fixture() {
    local path="$1" version="${2:-0.1.0}"
    cat >"$path" <<EOF
#!/bin/sh
if [ "\$1" = "--version" ]; then
    echo "codegg $version"
    exit 0
fi
echo "fixture codegg $version"
EOF
    chmod +x "$path"
}

# Sentinel "previously installed" binary with stable bytes.
make_sentinel() {
    local path="$1"
    cat >"$path" <<'EOF'
#!/bin/sh
echo "sentinel-installed-binary"
EOF
    chmod +x "$path"
}

# Build a fixture release dir with all four targets at one version, using the
# real M001 packaging helpers (dogfoods the artifact contract).
make_release_dir() {
    local dir="$1" version="$2"
    local bin="$3"
    mkdir -p -- "$dir"
    local t
    for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin; do
        "$PKG" --target "$t" --binary "$bin" --out-dir "$dir" >/dev/null 2>&1
    done
    "$FIN" --dir "$dir" >/dev/null 2>&1
}

sha_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$1" | awk '{print $1}'
    else
        shasum -a 256 -- "$1" | awk '{print $1}'
    fi
}

# Fresh destination dir containing a sentinel binary; prints the dest path.
fresh_dest_with_sentinel() {
    local dest="$1"
    mkdir -p -- "$dest"
    make_sentinel "$dest/codegg"
    cp -- "$dest/codegg" "$dest/codegg.sentinel-copy"
    printf '%s\n' "$dest"
}

# Assert the sentinel at $1/codegg is byte-identical to its pre-run copy.
sentinel_intact() {
    local label="$1" dest="$2"
    if cmp -s -- "$dest/codegg" "$dest/codegg.sentinel-copy"; then
        ok "$label (sentinel byte-identical)"
    else
        fail "$label (sentinel was modified)"
    fi
}

# Run the installer as a subprocess (never inherits the unit-test source
# guard). Callers pass VAR= assignments via env. Always returns 0 so bare
# calls are safe under `set -e`; inspect INSTALLER_STATUS instead.
INSTALLER_STATUS=0
installer_run() {
    INSTALLER_STATUS=0
    env -u CODEGG_INSTALL_LIB_ONLY "$@" "$SH" "$INSTALL_SH" >"$RUN_LOG" 2>&1 || INSTALLER_STATUS=$?
    return 0
}

installer_expect_pass() {
    local desc="$1"; shift
    installer_run "$@"
    if [ "$INSTALLER_STATUS" -eq 0 ]; then
        ok "$desc"
    else
        fail "$desc (expected success, got failure; see $RUN_LOG)"
        cat "$RUN_LOG" >&2 || true
    fi
}

installer_expect_fail() {
    local desc="$1"; shift
    installer_run "$@"
    if [ "$INSTALLER_STATUS" -ne 0 ]; then
        ok "$desc"
    else
        fail "$desc (expected failure, got success)"
    fi
}

# Assert the last installer run log contains / lacks a fixed string.
run_log_contains() {
    local label="$1" needle="$2"
    if grep -qF -- "$needle" "$RUN_LOG"; then
        ok "$label"
    else
        fail "$label (missing [$needle]; see $RUN_LOG)"
        cat "$RUN_LOG" >&2 || true
    fi
}

run_log_lacks() {
    local label="$1" needle="$2"
    if grep -qF -- "$needle" "$RUN_LOG"; then
        fail "$label (unexpected [$needle] in output)"
        cat "$RUN_LOG" >&2 || true
    else
        ok "$label"
    fi
}

printf '== M002 installer tests (root=%s host=%s)\n' "$ROOT" "$HOST_TARGET"

# --- 1. OS/arch mapping ---------------------------------------------------------
iu_out_eq "map Linux x86_64" "x86_64-unknown-linux-gnu" codegg_installer_map_target Linux x86_64
iu_out_eq "map Linux amd64 spelling" "x86_64-unknown-linux-gnu" codegg_installer_map_target Linux amd64
iu_out_eq "map Linux aarch64" "aarch64-unknown-linux-gnu" codegg_installer_map_target Linux aarch64
iu_out_eq "map Linux arm64 spelling" "aarch64-unknown-linux-gnu" codegg_installer_map_target Linux arm64
iu_out_eq "map Darwin x86_64" "x86_64-apple-darwin" codegg_installer_map_target Darwin x86_64
iu_out_eq "map Darwin arm64" "aarch64-apple-darwin" codegg_installer_map_target Darwin arm64
iu_out_eq "map Darwin aarch64 spelling" "aarch64-apple-darwin" codegg_installer_map_target Darwin aarch64
iu_fail "reject FreeBSD" codegg_installer_map_target FreeBSD x86_64
iu_fail "reject unknown arch" codegg_installer_map_target Linux mips
iu_fail "reject Darwin i386" codegg_installer_map_target Darwin i386
iu_fail "reject empty OS/arch" codegg_installer_map_target "" ""
iu_fail "reject arch metachars" codegg_installer_map_target Linux 'x86_64; echo pwned'
iu_fail "reject OS metachars" codegg_installer_map_target 'Linux; echo pwned' x86_64
iu_out_eq "asset name gnu x86" "codegg-x86_64-unknown-linux-gnu.tar.gz" codegg_installer_asset_for_target x86_64-unknown-linux-gnu
iu_out_eq "asset name gnu arm" "codegg-aarch64-unknown-linux-gnu.tar.gz" codegg_installer_asset_for_target aarch64-unknown-linux-gnu
iu_out_eq "asset name mac x86" "codegg-x86_64-apple-darwin.tar.gz" codegg_installer_asset_for_target x86_64-apple-darwin
iu_out_eq "asset name mac arm" "codegg-aarch64-apple-darwin.tar.gz" codegg_installer_asset_for_target aarch64-apple-darwin
iu_fail "reject windows asset mapping" codegg_installer_asset_for_target x86_64-pc-windows-msvc
iu_fail "reject evil asset mapping" codegg_installer_asset_for_target '../evil'

# --- 2. Version grammar -----------------------------------------------------------
iu_out_eq "empty version means latest" "" codegg_installer_normalize_version ""
iu_out_eq "bare version passes through" "0.1.1" codegg_installer_normalize_version "0.1.1"
iu_out_eq "v-prefixed version normalized" "0.1.1" codegg_installer_normalize_version "v0.1.1"
iu_out_eq "prerelease suffix accepted" "1.2.3-rc.1" codegg_installer_normalize_version "1.2.3-rc.1"
iu_fail "reject lone v" codegg_installer_normalize_version "v"
iu_fail "reject leading dash" codegg_installer_normalize_version "-1.2.3"
iu_fail "reject traversal" codegg_installer_normalize_version "../1.2.3"
iu_fail "reject semicolon injection" codegg_installer_normalize_version '1.2.3;curl evil'
iu_fail "reject whitespace" codegg_installer_normalize_version '1.2.3 4'
iu_fail "reject command substitution" codegg_installer_normalize_version '1.2.3$(id)'
iu_fail "reject backticks" codegg_installer_normalize_version '1.2.3`id`'
iu_fail "reject slash" codegg_installer_normalize_version '1.2.3/4'
iu_fail "reject short version" codegg_installer_normalize_version '1.2'
iu_fail "reject branch name" codegg_installer_normalize_version 'latest'
iu_fail "reject URL" codegg_installer_normalize_version 'https://example.com/x'
iu_fail "reject colon" codegg_installer_normalize_version '1.2.3:4'
iu_fail "reject pipe" codegg_installer_normalize_version '1.2.3|sh'

# --- 3. URL construction (fixed origin, exact strings) -----------------------------
iu_out_eq "latest archive URL exact" \
    "https://github.com/dbowm91/codegg/releases/latest/download/codegg-x86_64-unknown-linux-gnu.tar.gz" \
    codegg_installer_download_url "codegg-x86_64-unknown-linux-gnu.tar.gz" ""
iu_out_eq "pinned archive URL exact" \
    "https://github.com/dbowm91/codegg/releases/download/v0.1.1/codegg-aarch64-apple-darwin.tar.gz" \
    codegg_installer_download_url "codegg-aarch64-apple-darwin.tar.gz" "0.1.1"
iu_out_eq "latest checksums URL exact" \
    "https://github.com/dbowm91/codegg/releases/latest/download/checksums.txt" \
    codegg_installer_download_url "checksums.txt" ""
iu_out_eq "pinned checksums URL exact" \
    "https://github.com/dbowm91/codegg/releases/download/v0.1.1/checksums.txt" \
    codegg_installer_download_url "checksums.txt" "0.1.1"
for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin; do
    iu_out_eq "latest URL for $t" \
        "https://github.com/dbowm91/codegg/releases/latest/download/codegg-$t.tar.gz" \
        codegg_installer_download_url "codegg-$t.tar.gz" ""
done
# Production origin cannot be redirected through environment overrides.
if CODEGG_DOWNLOAD_URL="http://127.0.0.1:9/evil" CODEGG_RELEASE_BASE_URL="http://127.0.0.1:9/evil" \
    CODEGG_ORIGIN="http://127.0.0.1:9/evil" CODEGG_MIRROR="http://127.0.0.1:9/evil" \
    codegg_installer_download_url "codegg-x86_64-unknown-linux-gnu.tar.gz" "" >"$GOT_LOG" 2>&1; then
    if [ "$(cat "$GOT_LOG")" = "https://github.com/dbowm91/codegg/releases/latest/download/codegg-x86_64-unknown-linux-gnu.tar.gz" ]; then
        ok "origin override env vars have no effect on URLs"
    else
        fail "origin override env vars have no effect on URLs (got [$(cat "$GOT_LOG")])"
    fi
else
    fail "origin override env vars have no effect on URLs (function failed)"
fi
iu_fail "reject filename with slash in URL builder" codegg_installer_download_url "../evil" ""
iu_fail "reject un-normalized version in URL builder" codegg_installer_download_url "codegg-x86_64-unknown-linux-gnu.tar.gz" "v0.1.1"
# The rejected override names may be NAMED in installer comments (documenting
# what must not be added) but must never appear in executable lines.
if grep -nE 'CODEGG_DOWNLOAD_URL|CODEGG_RELEASE_BASE_URL|CODEGG_MIRROR|CODEGG_ORIGIN' "$INSTALL_SH" \
    | grep -vE '^[0-9]+:[[:space:]]*#' >/dev/null 2>&1; then
    fail "installer has no origin-override variable (grep hit in code)"
    grep -nE 'CODEGG_DOWNLOAD_URL|CODEGG_RELEASE_BASE_URL|CODEGG_MIRROR|CODEGG_ORIGIN' "$INSTALL_SH" >&2 || true
else
    ok "installer has no origin-override variable"
fi
if grep -qF 'https://github.com/dbowm91/codegg' "$INSTALL_SH"; then
    ok "canonical origin is hard-coded"
else
    fail "canonical origin is hard-coded"
fi

# --- 4. Exact checksum manifest parsing ---------------------------------------------
UROOT="$ROOT/unitsum"
mkdir -p -- "$UROOT"
make_fixture "$FIXBIN_DIR/unit-fixture" "0.1.0"
mkdir -p -- "$UROOT/stage"
cp -- "$FIXBIN_DIR/unit-fixture" "$UROOT/stage/codegg"
tar -czf "$UROOT/unit.tar.gz" -C "$UROOT/stage" codegg
UHASH="$(sha_of "$UROOT/unit.tar.gz")"
printf '%s  %s\n' "$UHASH" "$HOST_ASSET" >"$UROOT/good.txt"
expect_pass "valid manifest verifies" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/good.txt" "$HOST_ASSET"
printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "$HOST_ASSET" >"$UROOT/wrong.txt"
expect_fail "wrong hash fails" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/wrong.txt" "$HOST_ASSET"
printf '%s  %s\n' "$UHASH" "codegg-aarch64-unknown-linux-gnu.tar.gz" >"$UROOT/other.txt"
if [ "$HOST_ASSET" != "codegg-aarch64-unknown-linux-gnu.tar.gz" ]; then
    expect_fail "manifest without expected entry fails" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/other.txt" "$HOST_ASSET"
else
    printf '%s  %s\n' "$UHASH" "codegg-x86_64-unknown-linux-gnu.tar.gz" >"$UROOT/other.txt"
    expect_fail "manifest without expected entry fails" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/other.txt" "$HOST_ASSET"
fi
cat "$UROOT/good.txt" "$UROOT/good.txt" >"$UROOT/dup.txt"
expect_fail "duplicate entry fails" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/dup.txt" "$HOST_ASSET"
printf '%s  %s\n' "$UHASH" "$HOST_ASSET.bak" >"$UROOT/super.txt"
expect_fail "superstring entry does not match" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/super.txt" "$HOST_ASSET"
cp "$UROOT/good.txt" "$UROOT/trav.txt"
printf '%s  %s\n' "$UHASH" "../evil.tar.gz" >>"$UROOT/trav.txt"
expect_fail "traversal entry fails closed" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/trav.txt" "$HOST_ASSET"
cp "$UROOT/good.txt" "$UROOT/abs.txt"
printf '%s  %s\n' "$UHASH" "/tmp/evil.tar.gz" >>"$UROOT/abs.txt"
expect_fail "absolute entry fails closed" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/abs.txt" "$HOST_ASSET"
cp "$UROOT/good.txt" "$UROOT/emptyline.txt"
printf '\n' >>"$UROOT/emptyline.txt"
expect_fail "empty line fails closed" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/emptyline.txt" "$HOST_ASSET"
printf 'not-a-manifest-line\n' >"$UROOT/malformed.txt"
expect_fail "malformed line fails" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/malformed.txt" "$HOST_ASSET"
printf '%s  %s\n' "$(printf '%s' "$UHASH" | tr 'a-f' 'A-F')" "$HOST_ASSET" >"$UROOT/upper.txt"
expect_fail "uppercase hash fails" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/upper.txt" "$HOST_ASSET"
printf '%s *%s\n' "$UHASH" "$HOST_ASSET" >"$UROOT/binarymode.txt"
expect_pass "binary-mode star separator accepted" codegg_installer_verify_checksum "$UROOT/unit.tar.gz" "$UROOT/binarymode.txt" "$HOST_ASSET"

# --- 5. Archive member validation ------------------------------------------------------
MROOT="$ROOT/members"
mkdir -p -- "$MROOT"
python3 - "$MROOT/trav.tar.gz" <<'PYEOF'
import io, sys, tarfile
with tarfile.open(sys.argv[1], "w:gz") as tf:
    data = b"evil\n"
    ti = tarfile.TarInfo(name="../evil")
    ti.size = len(data)
    ti.mode = 0o755
    tf.addfile(ti, io.BytesIO(data))
PYEOF
python3 - "$MROOT/abs.tar.gz" <<'PYEOF'
import io, sys, tarfile
with tarfile.open(sys.argv[1], "w:gz") as tf:
    data = b"evil\n"
    ti = tarfile.TarInfo(name="/tmp/evil")
    ti.size = len(data)
    ti.mode = 0o755
    tf.addfile(ti, io.BytesIO(data))
PYEOF
python3 - "$MROOT/sym.tar.gz" <<'PYEOF'
import sys, tarfile
with tarfile.open(sys.argv[1], "w:gz") as tf:
    ti = tarfile.TarInfo(name="codegg")
    ti.type = tarfile.SYMTYPE
    ti.linkname = "/etc/passwd"
    ti.mode = 0o777
    tf.addfile(ti)
PYEOF
python3 - "$MROOT/dev.tar.gz" <<'PYEOF'
import sys, tarfile
with tarfile.open(sys.argv[1], "w:gz") as tf:
    ti = tarfile.TarInfo(name="codegg")
    ti.type = tarfile.CHRTYPE
    ti.devmajor = 1
    ti.devminor = 5
    tf.addfile(ti)
PYEOF
mkdir -p -- "$MROOT/extra-stage" "$MROOT/missing-stage"
cp -- "$FIXBIN_DIR/unit-fixture" "$MROOT/extra-stage/codegg"
printf 'extra\n' >"$MROOT/extra-stage/extra.txt"
tar -czf "$MROOT/extra.tar.gz" -C "$MROOT/extra-stage" codegg extra.txt
printf 'nothing here\n' >"$MROOT/missing-stage/not-codegg"
tar -czf "$MROOT/nocodegg.tar.gz" -C "$MROOT/missing-stage" not-codegg
expect_pass "valid single-codegg archive passes" codegg_installer_check_archive_members "$UROOT/unit.tar.gz"
expect_fail "traversal member rejected" codegg_installer_check_archive_members "$MROOT/trav.tar.gz"
expect_fail "absolute member rejected" codegg_installer_check_archive_members "$MROOT/abs.tar.gz"
expect_fail "symlink member rejected" codegg_installer_check_archive_members "$MROOT/sym.tar.gz"
expect_fail "device member rejected" codegg_installer_check_archive_members "$MROOT/dev.tar.gz"
expect_fail "extra payload rejected" codegg_installer_check_archive_members "$MROOT/extra.tar.gz"
expect_fail "missing codegg rejected" codegg_installer_check_archive_members "$MROOT/nocodegg.tar.gz"

# --- 6. Version output validation ---------------------------------------------------------
expect_pass "matching version output accepted" codegg_installer_check_version_output "codegg 0.1.0" "0.1.0"
expect_fail "mismatched version rejected" codegg_installer_check_version_output "codegg 0.1.0" "9.9.9"
expect_pass "latest accepts any well-formed version" codegg_installer_check_version_output "codegg 0.1.0" ""
expect_fail "garbage version output rejected" codegg_installer_check_version_output "hello world" ""
expect_fail "empty version output rejected" codegg_installer_check_version_output "" ""

# --- 7. End-to-end fixture installs ------------------------------------------------------------
REL="$ROOT/rel"
make_fixture "$FIXBIN_DIR/codegg-0.1.0" "0.1.0"
make_release_dir "$REL" "0.1.0" "$FIXBIN_DIR/codegg-0.1.0"

# 7a. Valid install into a fresh directory.
DEST_OK="$ROOT/dest-ok"
installer_expect_pass "valid fixture installs" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$DEST_OK"
if [ -x "$DEST_OK/codegg" ] && [ "$("$DEST_OK/codegg" --version)" = "codegg 0.1.0" ]; then
    ok "installed binary is executable and reports version"
else
    fail "installed binary is executable and reports version"
fi
installer_run env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$DEST_OK"
run_log_contains "success prints checksum evidence" "checksum ok: $HOST_ASSET"
run_log_contains "success prints install destination" "installed: $DEST_OK/codegg"
run_log_contains "success prints PATH guidance" "not on PATH"
run_log_contains "success notes daemon semantics" "daemon"

# 7b. Sentinel replacement + idempotent rerun.
DEST_REPLACE="$ROOT/dest-replace"
fresh_dest_with_sentinel "$DEST_REPLACE" >/dev/null
installer_expect_pass "success replaces prior sentinel" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$DEST_REPLACE"
if [ "$("$DEST_REPLACE/codegg" --version)" = "codegg 0.1.0" ]; then
    ok "prior sentinel replaced with new binary"
else
    fail "prior sentinel replaced with new binary"
fi
installer_expect_pass "rerunning same version succeeds" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$DEST_REPLACE"

# 7c. Checksum mismatch leaves the sentinel intact.
TAMPER="$ROOT/tamper"
rm -rf -- "$TAMPER"
cp -r -- "$REL" "$TAMPER"
printf 'x' >>"$TAMPER/$HOST_ASSET"
DEST_TAMPER="$ROOT/dest-tamper"
fresh_dest_with_sentinel "$DEST_TAMPER" >/dev/null
installer_expect_fail "tampered archive fails" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$TAMPER" "CODEGG_INSTALL_DIR=$DEST_TAMPER"
installer_run env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$TAMPER" "CODEGG_INSTALL_DIR=$DEST_TAMPER"
run_log_contains "mismatch is reported" "checksum mismatch"
sentinel_intact "checksum mismatch" "$DEST_TAMPER"

# 7d. Missing manifest / missing archive (download failure) leaves sentinel intact.
NOMANIFEST="$ROOT/nomanifest"
rm -rf -- "$NOMANIFEST"
cp -r -- "$REL" "$NOMANIFEST"
rm -- "$NOMANIFEST/checksums.txt"
DEST_NOMANIFEST="$ROOT/dest-nomanifest"
fresh_dest_with_sentinel "$DEST_NOMANIFEST" >/dev/null
installer_expect_fail "missing manifest fails" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$NOMANIFEST" "CODEGG_INSTALL_DIR=$DEST_NOMANIFEST"
sentinel_intact "missing manifest" "$DEST_NOMANIFEST"

NOARCHIVE="$ROOT/noarchive"
mkdir -p -- "$NOARCHIVE"
DEST_NOARCHIVE="$ROOT/dest-noarchive"
fresh_dest_with_sentinel "$DEST_NOARCHIVE" >/dev/null
installer_expect_fail "download failure fails" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$NOARCHIVE" "CODEGG_INSTALL_DIR=$DEST_NOARCHIVE"
sentinel_intact "download failure" "$DEST_NOARCHIVE"

# 7e. Duplicate manifest entry fails without touching the sentinel.
DUP="$ROOT/dup"
rm -rf -- "$DUP"
cp -r -- "$REL" "$DUP"
# Duplicate the selected host-asset entry (a duplicate of an unrelated asset
# must not affect this host; the installer selects its exact basename).
grep -F -- "$HOST_ASSET" "$DUP/checksums.txt" >>"$DUP/checksums.txt"
DEST_DUP="$ROOT/dest-dup"
fresh_dest_with_sentinel "$DEST_DUP" >/dev/null
installer_expect_fail "duplicate manifest entry fails" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$DUP" "CODEGG_INSTALL_DIR=$DEST_DUP"
sentinel_intact "duplicate manifest" "$DEST_DUP"

# 7f. Version-mismatch smoke fails without touching the sentinel.
DEST_VMISMATCH="$ROOT/dest-vmismatch"
fresh_dest_with_sentinel "$DEST_VMISMATCH" >/dev/null
installer_expect_fail "version mismatch smoke fails" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$DEST_VMISMATCH" "CODEGG_VERSION=9.9.9"
sentinel_intact "version mismatch smoke" "$DEST_VMISMATCH"
installer_expect_pass "pinned version matching fixture succeeds" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$ROOT/dest-pinned" "CODEGG_VERSION=0.1.0"
installer_expect_pass "v-prefixed pinned version succeeds" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$ROOT/dest-pinned-v" "CODEGG_VERSION=v0.1.0"

# 7g. Malicious payloads (hash re-finalized so the payload check must reject).
check_bad_payload() {
    local fixture="$1" label="$2"
    local work dest
    work="$ROOT/work-$(basename "$fixture" .tar.gz)"
    dest="$ROOT/dest-$(basename "$fixture" .tar.gz)"
    rm -rf -- "$work"
    mkdir -p -- "$work"
    cp -- "$fixture" "$work/$HOST_ASSET"
    "$FIN" --dir "$work" >/dev/null 2>&1
    fresh_dest_with_sentinel "$dest" >/dev/null
    installer_expect_fail "$label" \
        env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$work" "CODEGG_INSTALL_DIR=$dest"
    sentinel_intact "$label" "$dest"
}
check_bad_payload "$MROOT/trav.tar.gz" "traversal payload rejected end-to-end"
check_bad_payload "$MROOT/abs.tar.gz" "absolute payload rejected end-to-end"
check_bad_payload "$MROOT/sym.tar.gz" "symlink payload rejected end-to-end"
check_bad_payload "$MROOT/extra.tar.gz" "unexpected extra payload rejected end-to-end"
check_bad_payload "$MROOT/nocodegg.tar.gz" "missing codegg payload rejected end-to-end"

# 7h. Unsupported host fails before the download seam is invoked.
FAKEBIN="$ROOT/fakebin"
mkdir -p -- "$FAKEBIN"
cat >"$FAKEBIN/uname" <<'EOF'
#!/bin/sh
if [ "$1" = "-s" ]; then
    printf '%s\n' "${FAKE_UNAME_S:-Linux}"
else
    printf '%s\n' "${FAKE_UNAME_M:-x86_64}"
fi
EOF
chmod +x "$FAKEBIN/uname"
DEST_UNSUP="$ROOT/dest-unsup"
fresh_dest_with_sentinel "$DEST_UNSUP" >/dev/null
if PATH="$FAKEBIN:$PATH" FAKE_UNAME_S="FreeBSD" FAKE_UNAME_M="x86_64" \
    env -u CODEGG_INSTALL_LIB_ONLY \
    "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$ROOT/does-not-exist" "CODEGG_INSTALL_DIR=$DEST_UNSUP" \
    "$SH" "$INSTALL_SH" >"$RUN_LOG" 2>&1; then
    fail "unsupported OS fails (expected failure, got success)"
else
    ok "unsupported OS fails"
fi
run_log_contains "unsupported host message" "unsupported OS/architecture"
run_log_contains "unsupported host prints alternatives" "cargo install"
run_log_lacks "unsupported host downloads nothing" "downloading"
run_log_lacks "unsupported host skips fixture seam" "test fixture missing"
sentinel_intact "unsupported host" "$DEST_UNSUP"
if PATH="$FAKEBIN:$PATH" FAKE_UNAME_S="Linux" FAKE_UNAME_M="mips" \
    env -u CODEGG_INSTALL_LIB_ONLY \
    "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$ROOT/does-not-exist" "CODEGG_INSTALL_DIR=$DEST_UNSUP" \
    "$SH" "$INSTALL_SH" >"$RUN_LOG" 2>&1; then
    fail "unsupported arch fails (expected failure, got success)"
else
    ok "unsupported arch fails"
fi
run_log_contains "unsupported arch message" "unsupported OS/architecture"

# 7i. Bad destinations fail without side effects.
FILEDEST="$ROOT/filedest"
printf 'i am a file\n' >"$FILEDEST"
installer_expect_fail "file-as-install-dir fails" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$FILEDEST"
if [ "$(cat "$FILEDEST")" = "i am a file" ]; then
    ok "file-as-install-dir left alone"
else
    fail "file-as-install-dir left alone"
fi
if [ "$(id -u)" -eq 0 ]; then
    ok "unwritable install dir fails (skipped: running as root bypasses permission bits)"
else
    NOWRITE="$ROOT/nowrite"
    mkdir -p -- "$NOWRITE"
    chmod 555 "$NOWRITE"
    installer_expect_fail "unwritable install dir fails" \
        env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$NOWRITE"
    chmod 755 "$NOWRITE"
fi
installer_expect_fail "option-like install dir rejected" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=-evil"

# 7j. Install-dir quoting: spaces and shell metacharacters are path data.
SPACED="$ROOT/dir with spaces/bin"
installer_expect_pass "spaced install dir succeeds" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$SPACED"
if [ "$("$SPACED/codegg" --version)" = "codegg 0.1.0" ]; then
    ok "spaced install dir binary works"
else
    fail "spaced install dir binary works"
fi
METADIR="$ROOT/we;ird \$dollar (paren)/bin"
installer_expect_pass "metachar install dir treated as data" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$METADIR"
if [ -x "$METADIR/codegg" ] && [ "$("$METADIR/codegg" --version)" = "codegg 0.1.0" ]; then
    ok "metachar install dir binary works"
else
    fail "metachar install dir binary works"
fi

# 7k. Checksum failure prevents execution of the untrusted payload (canary).
CANARY="$ROOT/canary-touched"
rm -f -- "$CANARY"
cat >"$FIXBIN_DIR/canary-fixture" <<EOF
#!/bin/sh
if [ "\$1" = "--version" ]; then
    touch "$CANARY"
    echo "codegg 0.1.0"
    exit 0
fi
echo "canary fixture"
EOF
chmod +x "$FIXBIN_DIR/canary-fixture"
CANREL="$ROOT/canrel"
rm -rf -- "$CANREL"
mkdir -p -- "$CANREL"
"$PKG" --target "$HOST_TARGET" --binary "$FIXBIN_DIR/canary-fixture" --out-dir "$CANREL" >/dev/null 2>&1
"$FIN" --dir "$CANREL" >/dev/null 2>&1
printf 'x' >>"$CANREL/$HOST_ASSET"
DEST_CANARY="$ROOT/dest-canary"
fresh_dest_with_sentinel "$DEST_CANARY" >/dev/null
installer_expect_fail "tampered executable payload fails" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$CANREL" "CODEGG_INSTALL_DIR=$DEST_CANARY"
if [ -e "$CANARY" ]; then
    fail "checksum failure prevents payload execution (canary touched)"
else
    ok "checksum failure prevents payload execution (canary untouched)"
fi
sentinel_intact "tampered executable payload" "$DEST_CANARY"

# 7l. Default HOME layout works and user config is untouched.
FAKEHOME="$ROOT/fakehome"
mkdir -p -- "$FAKEHOME/.config/codegg"
printf '{"sentinel":"credentials"}\n' >"$FAKEHOME/.config/codegg/credentials.json"
cp -- "$FAKEHOME/.config/codegg/credentials.json" "$ROOT/credentials-copy.json"
installer_expect_pass "default HOME install succeeds" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "HOME=$FAKEHOME"
if [ "$("$FAKEHOME/.local/bin/codegg" --version)" = "codegg 0.1.0" ]; then
    ok "default HOME binary works"
else
    fail "default HOME binary works"
fi
if cmp -s -- "$FAKEHOME/.config/codegg/credentials.json" "$ROOT/credentials-copy.json"; then
    ok "existing user config untouched"
else
    fail "existing user config untouched"
fi

# 7m. CLI surface: --help ok, unknown args rejected, version injection rejected.
if env -u CODEGG_INSTALL_LIB_ONLY "$SH" "$INSTALL_SH" --help >"$RUN_LOG" 2>&1; then
    ok "installer --help succeeds"
else
    fail "installer --help succeeds"
    cat "$RUN_LOG" >&2 || true
fi
if grep -qF "CODEGG_VERSION" "$RUN_LOG"; then
    ok "help documents CODEGG_VERSION"
else
    fail "help documents CODEGG_VERSION"
fi
installer_expect_fail "unknown argument rejected" \
    env -- "$SH" "$INSTALL_SH" "--frobnicate"
installer_expect_fail "version injection rejected" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$ROOT/dest-inject" \
    "CODEGG_VERSION=0.1.1;curl http://127.0.0.1:9/evil"
installer_expect_fail "version traversal rejected" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$ROOT/dest-inject2" \
    "CODEGG_VERSION=../../etc"

# 7n. Failed runs leave no staging files; a clean retry succeeds.
STAGING_LEFTOVER=0
for d in "$ROOT"/dest-*; do
    if [ -d "$d" ]; then
        for entry in "$d"/.codegg-install.*; do
            [ -e "$entry" ] || continue
            STAGING_LEFTOVER=1
        done
    fi
done
if [ "$STAGING_LEFTOVER" -eq 0 ]; then
    ok "no staging leftovers in destinations"
else
    fail "no staging leftovers in destinations"
fi
installer_expect_pass "clean retry after failures succeeds" \
    env "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$DEST_TAMPER"
if [ "$("$DEST_TAMPER/codegg" --version)" = "codegg 0.1.0" ]; then
    ok "retry replaced the intact sentinel"
else
    fail "retry replaced the intact sentinel"
fi

# 7o. Missing-tool failure is clean (no destination mutation).
DEST_NOTOOLS="$ROOT/dest-notools"
fresh_dest_with_sentinel "$DEST_NOTOOLS" >/dev/null
if env -u CODEGG_INSTALL_LIB_ONLY PATH=/nonexistent \
    "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$DEST_NOTOOLS" \
    "$SH" "$INSTALL_SH" >"$RUN_LOG" 2>&1; then
    fail "missing tools fail (expected failure, got success)"
else
    ok "missing tools fail"
fi
run_log_contains "missing tools reported" "missing required command"
sentinel_intact "missing tools" "$DEST_NOTOOLS"

# 7p. Portability: success path also works under bash (and dash when present).
if env -u CODEGG_INSTALL_LIB_ONLY \
    "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$ROOT/dest-bash" \
    bash "$INSTALL_SH" >"$RUN_LOG" 2>&1; then
    ok "success path works under bash"
else
    fail "success path works under bash; see $RUN_LOG"
    cat "$RUN_LOG" >&2 || true
fi
if command -v dash >/dev/null 2>&1; then
    if env -u CODEGG_INSTALL_LIB_ONLY \
        "_CODEGG_INSTALLER_TEST_RELEASE_DIR=$REL" "CODEGG_INSTALL_DIR=$ROOT/dest-dash" \
        dash "$INSTALL_SH" >"$RUN_LOG" 2>&1; then
        ok "success path works under dash"
    else
        fail "success path works under dash; see $RUN_LOG"
        cat "$RUN_LOG" >&2 || true
    fi
else
    ok "success path works under dash (dash absent, skipped)"
fi

# --- 8. Static prohibitions --------------------------------------------------------------
if grep -ni 'sudo' "$INSTALL_SH" | grep -v '^$' >/dev/null 2>&1; then
    fail "installer never mentions sudo"
    grep -ni 'sudo' "$INSTALL_SH" >&2 || true
else
    ok "installer never mentions sudo"
fi
if grep -nw 'eval' "$INSTALL_SH" >/dev/null 2>&1; then
    fail "installer never uses eval"
else
    ok "installer never uses eval"
fi
if grep -n '>>' "$INSTALL_SH" >/dev/null 2>&1; then
    fail "installer never appends to files (no profile edits)"
else
    ok "installer never appends to files (no profile edits)"
fi
if grep -nE '\.[Bb]ashrc|\.zshrc|bash_profile|\.profile|launchd|launchctl|systemd|\.config/fish|LoginItems' "$INSTALL_SH" >/dev/null 2>&1; then
    fail "installer references no shell profile or service paths"
else
    ok "installer references no shell profile or service paths"
fi
if grep -nE '(\$\(|`)[^)]*(cargo|rustup)' "$INSTALL_SH" >/dev/null 2>&1; then
    fail "installer never executes cargo/rustup"
else
    ok "installer never executes cargo/rustup"
fi

# --- 9. Syntax / shellcheck ------------------------------------------------------------------
if "$SH" -n "$INSTALL_SH" 2>"$RUN_LOG"; then
    ok "sh -n syntax check passes"
else
    fail "sh -n syntax check passes"
    cat "$RUN_LOG" >&2 || true
fi
if command -v shellcheck >/dev/null 2>&1; then
    if shellcheck -S warning "$INSTALL_SH" >"$RUN_LOG" 2>&1; then
        ok "shellcheck (warning level) passes"
    else
        fail "shellcheck (warning level) passes; see $RUN_LOG"
        cat "$RUN_LOG" >&2 || true
    fi
else
    ok "shellcheck (warning level) passes (shellcheck absent, skipped)"
fi

printf '\n== results: %s passed, %s failed\n' "$PASS" "$FAIL"
if [ "$FAIL" -ne 0 ]; then
    exit 1
fi
