#!/usr/bin/env bash
#
# test-release-tools.sh — focused offline tests for the managed-runfile
# release bundle (self-contained-installation M001). No network, no real
# GitHub release, no cross-compilation. Uses fixture executables and local
# release dirs.
#
# Usage:
#   scripts/release/test-release-tools.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib-release.sh"

PKG="$SCRIPT_DIR/package-binary.sh"
FIN="$SCRIPT_DIR/finalize-release.sh"
VER="$SCRIPT_DIR/verify-release.sh"

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
    if "$@" >/tmp/codegg-release-test-out.log 2>&1; then
        ok "$desc"
    else
        fail "$desc (expected success, got failure; see /tmp/codegg-release-test-out.log)"
        cat /tmp/codegg-release-test-out.log >&2 || true
    fi
}

# expect_fail <desc> <command...>: command must exit nonzero.
expect_fail() {
    local desc="$1"; shift
    if "$@" >/tmp/codegg-release-test-out.log 2>&1; then
        fail "$desc (expected failure, got success)"
    else
        ok "$desc"
    fi
}

ROOT="$(mktemp -d "${TMPDIR:-/tmp}/codegg-release-tests.XXXXXX")"
chmod 700 "$ROOT"
cleanup_root() {
    rm -rf -- "$ROOT"
}
trap cleanup_root EXIT HUP INT TERM

FIXBIN_DIR="$ROOT/fixtures"
mkdir -p -- "$FIXBIN_DIR"

# Fixture "codegg": POSIX sh executable supporting --version.
make_fixture() {
    local path="$1"
    local version="${2:-0.1.0}"
    cat > "$path" <<EOF
#!/bin/sh
if [ "\$1" = "--version" ]; then
    echo "codegg $version"
    exit 0
fi
echo "fixture codegg $version"
EOF
    chmod +x "$path"
}

# Fixture sandbox helper: safe identity probe refuses a bare invocation
# (mirrors the real helper's --spec/--status-fd protocol refusal).
make_helper_fixture() {
    local path="$1"
    cat > "$path" <<'EOF'
#!/bin/sh
echo "sandbox helper protocol failure: missing --spec path" >&2
exit 125
EOF
    chmod +x "$path"
}

# Fixture eggsearch: reports the given upstream version via --version.
make_eggsearch_fixture() {
    local path="$1"
    local version="${2:-0.3.9}"
    cat > "$path" <<EOF
#!/bin/sh
if [ "\$1" = "--version" ]; then
    echo "eggsearch $version"
    exit 0
fi
echo "fixture eggsearch $version"
EOF
    chmod +x "$path"
}

printf '== managed-runfile release-tool tests (root=%s)\n' "$ROOT"

# --- 1. Target allowlist and filename mapping --------------------------------
if codegg_release_is_supported_target "aarch64-apple-darwin" \
    && codegg_release_is_supported_target "x86_64-unknown-linux-gnu" \
    && ! codegg_release_is_supported_target "x86_64-pc-windows-gnu" \
    && ! codegg_release_is_supported_target "" \
    && ! codegg_release_is_supported_target "aarch64-apple-darwin; rm -rf /" \
    && [ "$(codegg_release_archive_name "aarch64-apple-darwin")" = "codegg-aarch64-apple-darwin.tar.gz" ] \
    && [ "$(codegg_release_target_for_archive "codegg-x86_64-unknown-linux-gnu.tar.gz")" = "x86_64-unknown-linux-gnu" ]; then
    ok "target allowlist and filename mapping"
else
    fail "target allowlist and filename mapping"
fi

if codegg_release_is_required_target "x86_64-apple-darwin" \
    && ! codegg_release_is_required_target "x86_64-pc-windows-msvc"; then
    ok "required vs optional target distinction (windows optional)"
else
    fail "required vs optional target distinction (windows optional)"
fi

# --- 1b. Canonical runfile manifest ------------------------------------------
if [ "$(codegg_release_runfiles_for_target "x86_64-unknown-linux-gnu" | LC_ALL=C sort | tr '\n' ' ')" = "codegg codegg-eggsearch codegg-sandbox-helper " ] \
    && [ "$(codegg_release_runfiles_for_target "aarch64-apple-darwin" | LC_ALL=C sort | tr '\n' ' ')" = "codegg codegg-eggsearch codegg-sandbox-helper " ] \
    && [ "$(codegg_release_runfiles_for_target "x86_64-pc-windows-msvc" | LC_ALL=C sort | tr '\n' ' ')" = "codegg-eggsearch.exe codegg-sandbox-helper.exe codegg.exe " ] \
    && [ "$CODEGG_EGGSEARCH_PINNED_VERSION" = "0.3.9" ] \
    && [ "$CODEGG_EGGSEARCH_SIDECAR" = "codegg-eggsearch" ]; then
    ok "canonical per-target runfile manifest with pinned eggsearch 0.3.9"
else
    fail "canonical per-target runfile manifest with pinned eggsearch 0.3.9"
fi

if codegg_release_check_eggsearch_version_output "eggsearch 0.3.9" "0.3.9" 2>/dev/null \
    && ! codegg_release_check_eggsearch_version_output "eggsearch 0.3.8" "0.3.9" 2>/dev/null \
    && ! codegg_release_check_eggsearch_version_output "codegg 0.1.0" "0.3.9" 2>/dev/null; then
    ok "eggsearch identity/version check accepts pin, rejects drift"
else
    fail "eggsearch identity/version check accepts pin, rejects drift"
fi

# --- 2. Valid packaging for every required target ------------------------------
REL="$ROOT/rel"
mkdir -p -- "$REL"
make_fixture "$FIXBIN_DIR/codegg-fixture" "0.1.0"
make_helper_fixture "$FIXBIN_DIR/helper-fixture"
make_eggsearch_fixture "$FIXBIN_DIR/eggsearch-fixture" "0.3.9"
printf 'eggsearch 0.3.9 (%s tag %s)\nlicense: see upstream %s\n' "$CODEGG_EGGSEARCH_SOURCE" "$CODEGG_EGGSEARCH_UPSTREAM_TAG" "$CODEGG_EGGSEARCH_SOURCE" > "$FIXBIN_DIR/notice.txt"

for t in $CODEGG_REQUIRED_TARGETS; do
    expect_pass "package bundle fixture for $t" "$PKG" --target "$t" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
done

# Archive payload: exact managed runfiles, executable mode after extract.
EXTRACT_CHECK="$ROOT/extract-check"
mkdir -p -- "$EXTRACT_CHECK"
payload_ok=1
for t in $CODEGG_REQUIRED_TARGETS; do
    aname="$(codegg_release_archive_name "$t")"
    [ -f "$REL/$aname" ] || { payload_ok=0; break; }
    members="$(tar -tzf "$REL/$aname" | LC_ALL=C sort | tr '\n' ' ')"
    [ "$members" = "codegg codegg-eggsearch codegg-sandbox-helper " ] || { payload_ok=0; break; }
    rm -rf -- "$EXTRACT_CHECK/$t"
    mkdir -p -- "$EXTRACT_CHECK/$t"
    tar -xzf "$REL/$aname" -C "$EXTRACT_CHECK/$t"
    [ -f "$EXTRACT_CHECK/$t/codegg" ] && [ ! -L "$EXTRACT_CHECK/$t/codegg" ] && [ -x "$EXTRACT_CHECK/$t/codegg" ] || { payload_ok=0; break; }
    [ -f "$EXTRACT_CHECK/$t/codegg-sandbox-helper" ] && [ ! -L "$EXTRACT_CHECK/$t/codegg-sandbox-helper" ] && [ -x "$EXTRACT_CHECK/$t/codegg-sandbox-helper" ] || { payload_ok=0; break; }
    [ -f "$EXTRACT_CHECK/$t/codegg-eggsearch" ] && [ ! -L "$EXTRACT_CHECK/$t/codegg-eggsearch" ] && [ -x "$EXTRACT_CHECK/$t/codegg-eggsearch" ] || { payload_ok=0; break; }
    out="$("$EXTRACT_CHECK/$t/codegg" --version)"
    [ "$out" = "codegg 0.1.0" ] || { payload_ok=0; break; }
    egg_out="$("$EXTRACT_CHECK/$t/codegg-eggsearch" --version)"
    [ "$egg_out" = "eggsearch 0.3.9" ] || { payload_ok=0; break; }
    if "$EXTRACT_CHECK/$t/codegg-sandbox-helper" >/dev/null 2>&1; then payload_ok=0; break; fi
done
if [ "$payload_ok" -eq 1 ]; then
    ok "archive payload is exactly the managed runfile bundle per target"
else
    fail "archive payload is exactly the managed runfile bundle per target"
fi

# Notice member: fixed name only, staged when --notice is given.
NOTICEREL="$ROOT/rel-notice"
mkdir -p -- "$NOTICEREL"
expect_pass "package bundle with notice" "$PKG" --target "x86_64-unknown-linux-gnu" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --notice "$FIXBIN_DIR/notice.txt" --out-dir "$NOTICEREL"
if [ "$(tar -tzf "$NOTICEREL/codegg-x86_64-unknown-linux-gnu.tar.gz" | LC_ALL=C sort | tr '\n' ' ')" = "THIRD-PARTY-NOTICES.txt codegg codegg-eggsearch codegg-sandbox-helper " ]; then
    ok "notice member uses the fixed allowlisted name"
else
    fail "notice member uses the fixed allowlisted name"
fi

# Windows manifest uses explicit .exe names.
WINMANIFEST="$ROOT/winmanifest"
mkdir -p -- "$WINMANIFEST"
expect_pass "package windows bundle manifest" "$PKG" --target "x86_64-pc-windows-msvc" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$WINMANIFEST"
if [ "$(tar -tzf "$WINMANIFEST/codegg-x86_64-pc-windows-msvc.tar.gz" | LC_ALL=C sort | tr '\n' ' ')" = "codegg-eggsearch.exe codegg-sandbox-helper.exe codegg.exe " ]; then
    ok "windows bundle uses explicit .exe runfile names"
else
    fail "windows bundle uses explicit .exe runfile names"
fi

# No source/config/credential content leaks into archives.
if tar -tzf "$REL/$(codegg_release_archive_name "x86_64-unknown-linux-gnu")" | grep -Eq 'target/|\.git|config|credential|plans/|\.log'; then
    fail "archives must not include source/config/credential paths"
else
    ok "archives must not include source/config/credential paths"
fi

# --- 3. Negative packaging inputs ----------------------------------------------
expect_fail "reject unknown target" "$PKG" --target "mips-unknown-linux-gnu" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_fail "reject malicious target (metachars)" "$PKG" --target 'aarch64-apple-darwin; echo pwned' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_fail "reject malicious target (traversal)" "$PKG" --target '../etc/passwd' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_fail "reject option-like target" "$PKG" --target '-h' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_fail "reject missing binary" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$ROOT/does-not-exist" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_fail "reject missing helper" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$ROOT/does-not-exist" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_fail "reject missing eggsearch" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$ROOT/does-not-exist" --out-dir "$REL"
expect_fail "reject directory as binary" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"

ln -sf -- "$FIXBIN_DIR/codegg-fixture" "$ROOT/link-fixture"
expect_fail "reject symlink binary" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$ROOT/link-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
ln -sf -- "$FIXBIN_DIR/helper-fixture" "$ROOT/link-helper"
expect_fail "reject symlink helper" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$ROOT/link-helper" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
ln -sf -- "$FIXBIN_DIR/eggsearch-fixture" "$ROOT/link-egg"
expect_fail "reject symlink eggsearch" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$ROOT/link-egg" --out-dir "$REL"

printf 'not executable\n' > "$ROOT/plain-file"
expect_fail "reject non-executable binary" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$ROOT/plain-file" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"

make_eggsearch_fixture "$FIXBIN_DIR/eggsearch-wrong" "0.3.8"
expect_fail "reject wrong eggsearch version" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-wrong" --out-dir "$REL"
cat > "$FIXBIN_DIR/not-eggsearch" <<'EOF'
#!/bin/sh
if [ "$1" = "--version" ]; then
    echo "totally-different-tool 1.0.0"
    exit 0
fi
echo "impostor"
EOF
chmod +x "$FIXBIN_DIR/not-eggsearch"
expect_fail "reject wrong eggsearch identity" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/not-eggsearch" --out-dir "$REL"
expect_fail "reject drifted --eggsearch-version flag" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --eggsearch-version "0.3.8" --out-dir "$REL"

expect_fail "reject option-like binary path" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary '-evil' --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_fail "reject option-like out-dir" "$PKG" --target 'x86_64-unknown-linux-gnu' --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir '-evil'

# Packaging failure must leave no final archive behind.
if [ -e "$REL/codegg-mips-unknown-linux-gnu.tar.gz" ]; then
    fail "failed packaging left no final archive"
else
    ok "failed packaging leaves no final archive"
fi

# --- 4. Output path quoting (spaces) --------------------------------------------
SPACED="$ROOT/dir with spaces/release out"
mkdir -p -- "$SPACED"
expect_pass "package with spaced out-dir" "$PKG" --target "x86_64-unknown-linux-gnu" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$SPACED" --force
if [ -f "$SPACED/codegg-x86_64-unknown-linux-gnu.tar.gz" ]; then
    ok "spaced out-dir archive exists"
else
    fail "spaced out-dir archive exists"
fi

# --- 5. Overwrite policy ---------------------------------------------------------
expect_fail "refuse overwrite without --force" "$PKG" --target "x86_64-unknown-linux-gnu" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL"
expect_pass "allow overwrite with --force" "$PKG" --target "x86_64-unknown-linux-gnu" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$REL" --force

# --- 6. Finalize determinism + sha256sum -c compat --------------------------------
expect_pass "finalize checksums" "$FIN" --dir "$REL"
if [ -f "$REL/checksums.txt" ]; then
    ok "checksums.txt created"
else
    fail "checksums.txt created"
fi
cp -- "$REL/checksums.txt" "$ROOT/checksums-first.txt"
expect_pass "finalize again (deterministic)" "$FIN" --dir "$REL"
if cmp -s -- "$ROOT/checksums-first.txt" "$REL/checksums.txt"; then
    ok "checksum manifest stable across repeated generation"
else
    fail "checksum manifest stable across repeated generation"
fi
# Sorted by basename (second column; hashes are content-derived).
if awk '{print $2}' "$REL/checksums.txt" | LC_ALL=C sort -c >/dev/null 2>&1; then
    ok "checksum manifest sorted"
else
    fail "checksum manifest sorted"
fi
if command -v sha256sum >/dev/null 2>&1; then
    if (cd "$REL" && sha256sum -c checksums.txt >/dev/null 2>&1); then
        ok "manifest compatible with sha256sum -c"
    else
        fail "manifest compatible with sha256sum -c"
    fi
else
    ok "manifest compatible with sha256sum -c (sha256sum absent, skipped)"
fi

# --- 7. Verify complete set -------------------------------------------------------
expect_pass "verify complete release set" "$VER" --dir "$REL"
expect_pass "verify with matching --expect-version" "$VER" --dir "$REL" --allow-incomplete-target-set --expect-version "0.1.0"
expect_pass "verify with v-prefixed --expect-version" "$VER" --dir "$REL" --allow-incomplete-target-set --expect-version "v0.1.0"
expect_fail "reject mismatched --expect-version" "$VER" --dir "$REL" --allow-incomplete-target-set --expect-version "9.9.9"
expect_fail "reject malformed --expect-version" "$VER" --dir "$REL" --allow-incomplete-target-set --expect-version "../9.9.9"
expect_pass "rerun verification is idempotent" "$VER" --dir "$REL"

# --- 7b. Missing sidecars fail verification ---------------------------------------
MISSBASE="$ROOT/missbase"
rm -rf -- "$MISSBASE"
mkdir -p -- "$MISSBASE"
cp -- "$REL/$(codegg_release_archive_name "x86_64-unknown-linux-gnu")" "$MISSBASE/good.tar.gz"
# Archive with only codegg (historical single-binary layout).
stage_single="$ROOT/stage-single"
rm -rf -- "$stage_single"
mkdir -p -- "$stage_single"
cp -- "$FIXBIN_DIR/codegg-fixture" "$stage_single/codegg"
tar -czf "$MISSBASE/single.tar.gz" -C "$stage_single" codegg
single_work="$ROOT/work-single"
rm -rf -- "$single_work"
mkdir -p -- "$single_work"
cp -- "$MISSBASE/single.tar.gz" "$single_work/codegg-x86_64-unknown-linux-gnu.tar.gz"
cp -- "$REL/checksums.txt" "$single_work/checksums.txt"
"$FIN" --dir "$single_work" >/dev/null 2>&1
expect_fail "historical single-codegg archive rejected (missing sidecars)" "$VER" --dir "$single_work" --allow-incomplete-target-set --skip-version-smoke
# Archive missing only the helper.
stage_nohelper="$ROOT/stage-nohelper"
rm -rf -- "$stage_nohelper"
mkdir -p -- "$stage_nohelper"
cp -- "$FIXBIN_DIR/codegg-fixture" "$stage_nohelper/codegg"
cp -- "$FIXBIN_DIR/eggsearch-fixture" "$stage_nohelper/codegg-eggsearch"
tar -czf "$MISSBASE/nohelper.tar.gz" -C "$stage_nohelper" codegg codegg-eggsearch
nohelper_work="$ROOT/work-nohelper"
rm -rf -- "$nohelper_work"
mkdir -p -- "$nohelper_work"
cp -- "$MISSBASE/nohelper.tar.gz" "$nohelper_work/codegg-x86_64-unknown-linux-gnu.tar.gz"
cp -- "$REL/checksums.txt" "$nohelper_work/checksums.txt"
"$FIN" --dir "$nohelper_work" >/dev/null 2>&1
expect_fail "archive missing helper rejected" "$VER" --dir "$nohelper_work" --allow-incomplete-target-set --skip-version-smoke
# Archive missing only eggsearch.
stage_noegg="$ROOT/stage-noegg"
rm -rf -- "$stage_noegg"
mkdir -p -- "$stage_noegg"
cp -- "$FIXBIN_DIR/codegg-fixture" "$stage_noegg/codegg"
cp -- "$FIXBIN_DIR/helper-fixture" "$stage_noegg/codegg-sandbox-helper"
tar -czf "$MISSBASE/noegg.tar.gz" -C "$stage_noegg" codegg codegg-sandbox-helper
noegg_work="$ROOT/work-noegg"
rm -rf -- "$noegg_work"
mkdir -p -- "$noegg_work"
cp -- "$MISSBASE/noegg.tar.gz" "$noegg_work/codegg-x86_64-unknown-linux-gnu.tar.gz"
cp -- "$REL/checksums.txt" "$noegg_work/checksums.txt"
"$FIN" --dir "$noegg_work" >/dev/null 2>&1
expect_fail "archive missing eggsearch rejected" "$VER" --dir "$noegg_work" --allow-incomplete-target-set --skip-version-smoke
# Archive with wrong eggsearch version (native target so the version smoke runs).
detect_native_target() {
    if command -v rustc >/dev/null 2>&1; then
        host_line="$(rustc -vV 2>/dev/null | grep '^host:' | awk '{print $2}' || true)"
        if codegg_release_is_supported_target "${host_line:-}" >/dev/null 2>&1; then
            printf '%s\n' "$host_line"
            return 0
        fi
    fi
    os="$(uname -s 2>/dev/null || printf 'unknown')"
    mach="$(uname -m 2>/dev/null || printf 'unknown')"
    case "$os/$mach" in
        Linux/x86_64|Linux/amd64) printf '%s\n' "x86_64-unknown-linux-gnu" ;;
        Linux/aarch64|Linux/arm64) printf '%s\n' "aarch64-unknown-linux-gnu" ;;
        Darwin/x86_64) printf '%s\n' "x86_64-apple-darwin" ;;
        Darwin/arm64|Darwin/aarch64) printf '%s\n' "aarch64-apple-darwin" ;;
        *) return 1 ;;
    esac
}
NATIVE_T=""
if NATIVE_T="$(detect_native_target)"; then
    :
else
    NATIVE_T="x86_64-unknown-linux-gnu"
fi
NATIVE_ARCHIVE="$(codegg_release_archive_name "$NATIVE_T")"
stage_wrongegg="$ROOT/stage-wrongegg"
rm -rf -- "$stage_wrongegg"
mkdir -p -- "$stage_wrongegg"
cp -- "$FIXBIN_DIR/codegg-fixture" "$stage_wrongegg/codegg"
cp -- "$FIXBIN_DIR/helper-fixture" "$stage_wrongegg/codegg-sandbox-helper"
cp -- "$FIXBIN_DIR/eggsearch-wrong" "$stage_wrongegg/codegg-eggsearch"
tar -czf "$MISSBASE/wrongegg.tar.gz" -C "$stage_wrongegg" codegg codegg-sandbox-helper codegg-eggsearch
wrongegg_work="$ROOT/work-wrongegg"
rm -rf -- "$wrongegg_work"
mkdir -p -- "$wrongegg_work"
cp -- "$MISSBASE/wrongegg.tar.gz" "$wrongegg_work/$NATIVE_ARCHIVE"
"$FIN" --dir "$wrongegg_work" >/dev/null 2>&1
expect_fail "archive with wrong eggsearch version rejected" "$VER" --dir "$wrongegg_work" --allow-incomplete-target-set

# --- 8. Tamper / missing / duplicate ----------------------------------------------
TAMPER="$ROOT/tamper"
rm -rf -- "$TAMPER"
cp -r -- "$REL" "$TAMPER"
printf 'x' >> "$TAMPER/$(codegg_release_archive_name "x86_64-unknown-linux-gnu")"
expect_fail "tampered archive fails verification" "$VER" --dir "$TAMPER"

MISSING="$ROOT/missing"
rm -rf -- "$MISSING"
cp -r -- "$REL" "$MISSING"
rm -- "$MISSING/$(codegg_release_archive_name "aarch64-unknown-linux-gnu")"
# Manifest still lists it -> missing file failure.
expect_fail "missing archive file fails verification" "$VER" --dir "$MISSING"

# Omit one target then re-finalize: completeness failure (strict), relaxed pass.
MISSING2="$ROOT/missing2"
rm -rf -- "$MISSING2"
mkdir -p -- "$MISSING2"
i=0
for t in $CODEGG_REQUIRED_TARGETS; do
    i=$((i + 1))
    if [ "$i" -eq 4 ]; then continue; fi
    "$PKG" --target "$t" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$MISSING2" >/dev/null 2>&1
done
"$FIN" --dir "$MISSING2" >/dev/null 2>&1
expect_fail "incomplete set fails strict verification" "$VER" --dir "$MISSING2"
expect_pass "incomplete set passes with explicit relaxation" "$VER" --dir "$MISSING2" --allow-incomplete-target-set

DUP="$ROOT/dup"
rm -rf -- "$DUP"
cp -r -- "$REL" "$DUP"
tail -n 1 -- "$DUP/checksums.txt" >> "$DUP/checksums.txt"
expect_fail "duplicate manifest entry fails" "$VER" --dir "$DUP"

# --- 9. Unknown artifacts -----------------------------------------------------------
UNKNOWN="$ROOT/unknown"
rm -rf -- "$UNKNOWN"
cp -r -- "$REL" "$UNKNOWN"
printf 'evil\n' > "$UNKNOWN/codegg-evil-os.tar.gz"
expect_fail "unexpected file in release dir fails" "$VER" --dir "$UNKNOWN"

UNKNOWN2="$ROOT/unknown2"
rm -rf -- "$UNKNOWN2"
cp -r -- "$REL" "$UNKNOWN2"
sum="$(codegg_release_sha256_file "$UNKNOWN2/$(codegg_release_archive_name "x86_64-unknown-linux-gnu")")"
printf '%s  %s\n' "$sum" "codegg-evil-os.tar.gz" >> "$UNKNOWN2/checksums.txt"
expect_fail "unknown manifest entry fails" "$VER" --dir "$UNKNOWN2"

TRAV_MANIFEST="$ROOT/travmanifest"
rm -rf -- "$TRAV_MANIFEST"
cp -r -- "$REL" "$TRAV_MANIFEST"
printf '0000000000000000000000000000000000000000000000000000000000000000  ../evil.tar.gz\n' >> "$TRAV_MANIFEST/checksums.txt"
expect_fail "traversal manifest entry fails" "$VER" --dir "$TRAV_MANIFEST"

ABS_MANIFEST="$ROOT/absmanifest"
rm -rf -- "$ABS_MANIFEST"
cp -r -- "$REL" "$ABS_MANIFEST"
printf '0000000000000000000000000000000000000000000000000000000000000000  /tmp/evil.tar.gz\n' >> "$ABS_MANIFEST/checksums.txt"
expect_fail "absolute manifest entry fails" "$VER" --dir "$ABS_MANIFEST"

# --- 10. Malicious archive payloads --------------------------------------------------
BADBASE="$ROOT/badpayload"
rm -rf -- "$BADBASE"
mkdir -p -- "$BADBASE"

# Traversal member.
mkdir -p -- "$ROOT/stage-trav-outer/inner"
printf 'evil\n' > "$ROOT/stage-trav-outer/payload"
(cd "$ROOT/stage-trav-outer/inner" && tar -czf "$BADBASE/trav.tar.gz" --exclude='./*' -C .. ../payload 2>/dev/null || tar -czf "$BADBASE/trav.tar.gz" -C "$ROOT/stage-trav-outer/inner" ../../stage-trav-outer/payload 2>/dev/null || true)
if [ -f "$BADBASE/trav.tar.gz" ] && tar -tzf "$BADBASE/trav.tar.gz" 2>/dev/null | grep -q '\.\.'; then
    ok "crafted traversal fixture contains .."
else
    # Fallback: Python is allowed in tests for fixture crafting only.
    python3 - "$BADBASE/trav.tar.gz" <<'PYEOF'
import io, sys, tarfile
dest = sys.argv[1]
with tarfile.open(dest, "w:gz") as tf:
    data = b"evil\n"
    ti = tarfile.TarInfo(name="../evil")
    ti.size = len(data)
    ti.mode = 0o755
    tf.addfile(ti, io.BytesIO(data))
PYEOF
    if tar -tzf "$BADBASE/trav.tar.gz" 2>/dev/null | grep -q '\.\.'; then
        ok "crafted traversal fixture contains .. (python)"
    else
        fail "could not craft traversal fixture"
    fi
fi

# Absolute member.
python3 - "$BADBASE/abs.tar.gz" <<'PYEOF'
import io, sys, tarfile
dest = sys.argv[1]
with tarfile.open(dest, "w:gz") as tf:
    data = b"evil\n"
    ti = tarfile.TarInfo(name="/tmp/evil")
    ti.size = len(data)
    ti.mode = 0o755
    tf.addfile(ti, io.BytesIO(data))
PYEOF
ok "crafted absolute fixture"

# Symlink member named codegg.
python3 - "$BADBASE/sym.tar.gz" <<'PYEOF'
import io, sys, tarfile
dest = sys.argv[1]
with tarfile.open(dest, "w:gz") as tf:
    ti = tarfile.TarInfo(name="codegg")
    ti.type = tarfile.SYMTYPE
    ti.linkname = "/etc/passwd"
    ti.mode = 0o777
    tf.addfile(ti)
PYEOF
ok "crafted symlink fixture"

# Wrong payload: extra file alongside the bundle.
stage2="$ROOT/stage-extra"
rm -rf -- "$stage2"
mkdir -p -- "$stage2"
cp -- "$FIXBIN_DIR/codegg-fixture" "$stage2/codegg"
cp -- "$FIXBIN_DIR/helper-fixture" "$stage2/codegg-sandbox-helper"
cp -- "$FIXBIN_DIR/eggsearch-fixture" "$stage2/codegg-eggsearch"
printf 'extra\n' > "$stage2/extra.txt"
tar -czf "$BADBASE/extra.tar.gz" -C "$stage2" codegg codegg-sandbox-helper codegg-eggsearch extra.txt
ok "crafted extra-file fixture"

# Wrong payload: bundle with an impostor member instead of codegg.
stage3="$ROOT/stage-missing"
rm -rf -- "$stage3"
mkdir -p -- "$stage3"
cp -- "$FIXBIN_DIR/helper-fixture" "$stage3/codegg-sandbox-helper"
cp -- "$FIXBIN_DIR/eggsearch-fixture" "$stage3/codegg-eggsearch"
printf 'nothing here\n' > "$stage3/not-codegg"
tar -czf "$BADBASE/nocodegg.tar.gz" -C "$stage3" not-codegg codegg-sandbox-helper codegg-eggsearch
ok "crafted missing-codegg fixture"

check_bad_payload() {
    local fixture="$1"
    local label="$2"
    local work="$ROOT/work-$(basename "$fixture" .tar.gz)"
    rm -rf -- "$work"
    mkdir -p -- "$work"
    cp -- "$fixture" "$work/codegg-x86_64-unknown-linux-gnu.tar.gz"
    cp -- "$REL/checksums.txt" "$work/checksums.txt"
    # Re-finalize so the checksum matches the malicious bytes; the payload
    # check (not the hash) must be what rejects it.
    "$FIN" --dir "$work" >/dev/null 2>&1
    expect_fail "$label" "$VER" --dir "$work" --allow-incomplete-target-set --skip-version-smoke
}

check_bad_payload "$BADBASE/trav.tar.gz" "traversal archive member rejected"
check_bad_payload "$BADBASE/abs.tar.gz" "absolute archive member rejected"
check_bad_payload "$BADBASE/sym.tar.gz" "symlink archive member rejected"
check_bad_payload "$BADBASE/extra.tar.gz" "unexpected extra payload rejected"
check_bad_payload "$BADBASE/nocodegg.tar.gz" "missing codegg payload rejected"

# --- 11. Stale temp output does not verify ------------------------------------------
STALE="$ROOT/stale"
rm -rf -- "$STALE"
cp -r -- "$REL" "$STALE"
touch -- "$STALE/.codegg-x86_64-unknown-linux-gnu.tmp.STALE"
expect_fail "stale temp file fails closed" "$VER" --dir "$STALE"

# --- 12. Optional Windows treatment ---------------------------------------------------
WIN="$ROOT/win"
rm -rf -- "$WIN"
cp -r -- "$REL" "$WIN"
expect_pass "package optional windows target" "$PKG" --target "x86_64-pc-windows-msvc" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$WIN"
expect_pass "finalize with optional windows" "$FIN" --dir "$WIN"
expect_pass "verify complete set with optional windows extra" "$VER" --dir "$WIN" --skip-version-smoke
if grep -q "codegg-x86_64-pc-windows-msvc.tar.gz" -- "$WIN/checksums.txt"; then
    ok "windows artifact explicitly listed (not silently folded)"
else
    fail "windows artifact explicitly listed (not silently folded)"
fi

WINONLY="$ROOT/winonly"
rm -rf -- "$WINONLY"
mkdir -p -- "$WINONLY"
"$PKG" --target "x86_64-pc-windows-msvc" --binary "$FIXBIN_DIR/codegg-fixture" --sandbox-helper "$FIXBIN_DIR/helper-fixture" --eggsearch "$FIXBIN_DIR/eggsearch-fixture" --out-dir "$WINONLY" >/dev/null 2>&1
"$FIN" --dir "$WINONLY" >/dev/null 2>&1
expect_fail "windows-only set fails strict completeness" "$VER" --dir "$WINONLY" --skip-version-smoke
expect_pass "windows-only set passes relaxed mode" "$VER" --dir "$WINONLY" --allow-incomplete-target-set --skip-version-smoke

# --- 13. Empty finalize ---------------------------------------------------------------
EMPTY="$ROOT/empty"
rm -rf -- "$EMPTY"
mkdir -p -- "$EMPTY"
expect_fail "finalize empty dir fails" "$FIN" --dir "$EMPTY"

printf '\n== results: %s passed, %s failed\n' "$PASS" "$FAIL"
if [ "$FAIL" -ne 0 ]; then
    exit 1
fi
