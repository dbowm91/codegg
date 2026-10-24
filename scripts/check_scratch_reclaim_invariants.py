#!/usr/bin/env python3
"""The managed-scratch reclaim path must keep its invariants (M008).

A scratch tree the daemon owns is reclaimed through exactly one routine, and
that routine decides on four facts: the tree is inside the canonical sessions
root, it is a real directory, it is owned by the daemon's user, and it carries
the daemon's owner record. A tree that holds content must present that record:
a record that is missing, malformed, or owned by another user is refused, and
nothing is removed.

The failure this guard exists for is **silent**. A reclaim failure that is
reported as a provisioning failure tells the operator the wrong thing about
what failed; a removal that skips the owner-record check removes a tree the
daemon cannot prove it owns; a report that claims a tree is reclaimable when
the validator would refuse it sends an operator to a command that cannot
succeed. None of those is a crash — they are wrong answers — so they need a
control that is run automatically.

The controls this guard enforces:

- **G1 — the error taxonomy is split.** `ScratchError` carries a
  provisioning variant and a reclaim variant, each naming its own operation,
  and it derives **no** blanket `From<std::io::Error>`. The blanket conversion
  is what let a reclaim failure wear the provisioning text; with it gone, a
  `?` on an `io::Result` does not compile, so every I/O failure is mapped
  explicitly.
- **G2 — the reclaim path maps every I/O failure explicitly.** No `?` may
  reach an `std::fs::`/`std::io::` call in the reclaim path, no arm may
  convert an I/O error with a bare `.into()`, and the provisioning variant
  may not be constructed anywhere in it.
- **G3 — one removal routine, under an emptiness proof.** The tree is removed
  in exactly one place, and that place consults the emptiness predicate
  first. The one exception the owner-record check allows is a tree that holds
  **no entries at all**: there is nothing left to dispose of. A tree that
  holds content is refused with `ownership_unverifiable` instead, and that
  refusal is produced by the shared rule alone.
- **G4 — the refusal taxonomy is mirrored, with the same labels.** The
  validator's label, the protocol's wire name, the daemon's DTO mapping and
  the CLI's label map must agree, so an operator sees one name for one rule.
- **G5 — the census derives its verdict from the validator's rule.** A report
  that restates the rule independently can disagree with the validator; the
  census must call the shared rule.

Only production code is checked: the unit tests at the foot of `src/scratch.rs`
plant and remove trees on purpose, and a control that flagged them would be
flagging the tests rather than the product.

Modes:
    python3 scripts/check_scratch_reclaim_invariants.py             # static guard
    python3 scripts/check_scratch_reclaim_invariants.py --self-test # non-vacuity

`--self-test` copies the sources it reads into a scratch directory, injects
each defect class in turn, and requires the guard to reject every one of them
while accepting the unmodified copy.
"""

from __future__ import annotations

import argparse
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

SCRATCH_RS = Path("src/scratch.rs")
PROTOCOL_RS = Path("crates/codegg-protocol/src/scratch.rs")
DAEMON_RS = Path("src/core/daemon_scratch.rs")
MAIN_RS = Path("src/main.rs")

# The reclaim path. Every one of these functions may report an I/O failure,
# and every one of them must say *reclaim*, not *provision*.
RECLAIM_FUNCTIONS = (
    # The entry points.
    "reclaim_session_scratch_dir",
    "reclaim_agent_run_scratch_dir",
    "reclaim_shared_scratch_dir",
    "reclaim_session_scratch_by_digest",
    "reclaim_session_scratch_for_operator",
    "existing_session_scratch_dir",
    # The internals.
    "reclaim_owned_resource_dir",
    "recover_reclaim_tombstones",
    "validate_and_remove_tombstone",
    "validate_reclaim_tree",
    "tree_is_empty",
)

# The one predicate that lets the reclaim path remove a tree whose owner
# record is missing.
EMPTINESS_PREDICATE = "tree_is_empty"

# The refusal that means "this tree cannot be proven owned".
OWNERSHIP_REFUSAL = "OwnershipUnverifiable"

# The removal routine. There is exactly one.
REMOVAL_CALL = "std::fs::remove_dir_all("

_IO_CALL = re.compile(r"std::(?:fs|io)::(?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*\(")
_BARE_INTO = re.compile(r"Err\(\s*[A-Za-z_][A-Za-z0-9_]*\s*\.\s*into\(\s*\)\s*\)")
_VARIANT_DECL = re.compile(r"^[ \t]*([A-Z][A-Za-z0-9]*)\s*,\s*$", re.MULTILINE)
_TEST_MODULE = re.compile(
    r"#\s*\[\s*cfg\(test\)\s*\]\s*(?:#\s*\[[^\]]*\]\s*)*mod\s+tests\b"
)


# ── Rust source scanning ───────────────────────────────────────────────


def strip_rust(text: str) -> str:
    """Blank out comments and literals, preserving offsets and line breaks.

    Brace counting over raw Rust source misreads a `{` inside a format
    string as a block, and a `//` inside a string as a comment. The scanner
    blanks both, so what remains is code with the same shape and the same
    line numbering as the original. Every blanked region keeps its length,
    so an offset in the stripped text is the same offset in the original.
    """
    out = list(text)
    index = 0
    length = len(text)

    def blank(start: int, end: int) -> None:
        for position in range(start, min(end, length)):
            if out[position] != "\n":
                out[position] = " "

    while index < length:
        ch = text[index]
        # Raw string: r"..." / r#"..."#
        if ch == "r" and index + 1 < length and text[index + 1] in '"#':
            cursor = index + 1
            hashes = 0
            while cursor < length and text[cursor] == "#":
                hashes += 1
                cursor += 1
            if cursor < length and text[cursor] == '"':
                terminator = '"' + ("#" * hashes)
                end = text.find(terminator, cursor + 1)
                end = length if end == -1 else end + len(terminator)
                blank(index, end)
                index = end
                continue
        # Line comment.
        if ch == "/" and index + 1 < length and text[index + 1] == "/":
            end = text.find("\n", index)
            end = length if end == -1 else end
            blank(index, end)
            index = end
            continue
        # Block comment (nestable in Rust).
        if ch == "/" and index + 1 < length and text[index + 1] == "*":
            depth = 0
            cursor = index
            while cursor < length:
                if text.startswith("/*", cursor):
                    depth += 1
                    cursor += 2
                    continue
                if text.startswith("*/", cursor):
                    depth -= 1
                    cursor += 2
                    if depth == 0:
                        break
                    continue
                cursor += 1
            blank(index, cursor)
            index = cursor
            continue
        # Ordinary string.
        if ch == '"':
            cursor = index + 1
            while cursor < length:
                if text[cursor] == "\\":
                    cursor += 2
                    continue
                if text[cursor] == '"':
                    cursor += 1
                    break
                cursor += 1
            blank(index, cursor)
            index = cursor
            continue
        # Character literal. A lifetime ('a) has no closing quote, so it is
        # not a literal and is left alone.
        if ch == "'":
            match = re.match(r"'(\\.|[^\\'])'", text[index : index + 8])
            if match:
                blank(index, index + match.end())
                index += match.end()
                continue
        index += 1
    return "".join(out)


class Source:
    """One Rust file, scanned.

    `code` is the comment- and literal-free view used for structure; `raw`
    is the original, used for the text inside string literals. Both have the
    same length, so an offset in one is the offset in the other.
    """

    def __init__(self, path: Path, root: Path) -> None:
        self.path = path
        self.rel = path.relative_to(root).as_posix()
        self.raw = path.read_text(encoding="utf-8")
        self.code = strip_rust(self.raw)
        # Production code only: the unit tests plant and remove trees on
        # purpose.
        match = _TEST_MODULE.search(self.code)
        self.production_end = match.start() if match else len(self.code)

    def line(self, offset: int) -> int:
        return self.raw.count("\n", 0, offset) + 1

    def body(self, name: str) -> tuple[int, int] | None:
        """The `(start, end)` offsets of `fn <name>(`'s body."""
        return _body_at(self.code, self._fn_offset(name))

    def _fn_offset(self, name: str) -> int | None:
        match = re.search(r"\bfn\s+" + re.escape(name) + r"\b", self.code)
        return match.start() if match else None

    def body_text(self, name: str) -> str | None:
        """The raw text of `fn <name>(`'s body."""
        span = self.body(name)
        if span is None:
            return None
        return self.raw[span[0] : span[1]]

    def enclosing(self, offset: int) -> str | None:
        """The name of the function whose body contains `offset`."""
        best: tuple[int, str] | None = None
        for match in re.finditer(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\b", self.code):
            span = _body_at(self.code, match.start())
            if span is None:
                continue
            if span[0] <= offset <= span[1]:
                if best is None or span[0] > best[0]:
                    best = (span[0], match.group(1))
        return best[1] if best else None


def _body_at(code: str, start: int | None) -> tuple[int, int] | None:
    """The `(start, end)` offsets of the body of the function at `start`."""
    if start is None:
        return None
    index = start
    depth = 0
    while index < len(code):
        if code[index] == "(":
            depth += 1
        elif code[index] == ")":
            depth -= 1
            if depth == 0:
                break
        index += 1
    while index < len(code) and code[index] != "{":
        index += 1
    if index >= len(code):
        return None
    body_start = index
    depth = 0
    while index < len(code):
        if code[index] == "{":
            depth += 1
        elif code[index] == "}":
            depth -= 1
            if depth == 0:
                return (body_start, index + 1)
        index += 1
    return None


# ── Labels ─────────────────────────────────────────────────────────────


def snake(name: str) -> str:
    """The `snake_case` wire name of a variant, as serde derives it."""
    out: list[str] = []
    for index, ch in enumerate(name):
        if ch.isupper() and index > 0 and (name[index - 1].islower() or name[index - 1].isdigit()):
            out.append("_")
        out.append(ch.lower())
    return "".join(out)


def validator_labels(scratch: Source) -> set[str]:
    """The labels `ReclaimRefusal::as_str` produces."""
    body = scratch.body_text("as_str")
    if body is None:
        return set()
    return set(re.findall(r'"([a-z0-9_]+)"', body))


def protocol_labels(protocol: Source) -> set[str]:
    """The wire names `ScratchReclaimRefusalDto` carries."""
    match = re.search(
        r"pub enum ScratchReclaimRefusalDto\s*\{(.*?)\n\}", protocol.raw, re.DOTALL
    )
    if match is None:
        return set()
    return {snake(variant) for variant in _VARIANT_DECL.findall(match.group(1))}


def daemon_labels(daemon: Source) -> set[str]:
    """The wire names the daemon's `refusal_to_dto` can produce."""
    body = daemon.body_text("refusal_to_dto")
    if body is None:
        return set()
    return {
        snake(dto)
        for dto in re.findall(r"ScratchReclaimRefusalDto::([A-Za-z0-9_]+)", body)
    }


def cli_labels(main: Source) -> set[str]:
    """The labels the CLI's `scratch_refusal_name` prints."""
    body = main.body_text("scratch_refusal_name")
    if body is None:
        return set()
    return set(re.findall(r'"([a-z0-9_]+)"', body))


# ── The controls ───────────────────────────────────────────────────────


def check_taxonomy(scratch: Source) -> list[str]:
    """G1 — the reclaim error taxonomy is split, and split explicitly."""
    violations: list[str] = []
    raw = scratch.raw[: scratch.production_end]
    provisioning = re.findall(r'#\[error\("([^"]*provision[^"]*)"\)\]', raw)
    reclaiming = re.findall(r'#\[error\("([^"]*reclaim[^"]*)"\)\]', raw)
    if not provisioning:
        violations.append(
            f"{scratch.rel}: the error taxonomy has no variant whose text names "
            "provisioning. A reclaim failure must be distinguishable from a "
            "provisioning failure."
        )
    if not reclaiming:
        violations.append(
            f"{scratch.rel}: the error taxonomy has no variant whose text names "
            "reclamation. A reclaim failure must never wear the provisioning "
            "text."
        )
    for text in reclaiming:
        if "provision" in text:
            violations.append(
                f"{scratch.rel}: the reclaim variant's text names provisioning "
                f"too: {text!r}. One variant, one operation."
            )
    for text in provisioning:
        if "reclaim" in text:
            violations.append(
                f"{scratch.rel}: the provisioning variant's text names "
                f"reclamation too: {text!r}. One variant, one operation."
            )
    # The blanket conversion is the defect this whole taxonomy exists to
    # prevent: it is what turns a reclaim's I/O failure into the provisioning
    # text without anybody choosing that.
    for match in re.finditer(r"#\[from(?:\(source\))?\]", raw):
        context = raw[match.start() : match.start() + 120]
        if "io::Error" not in context:
            continue
        violations.append(
            f"{scratch.rel}:{scratch.line(match.start())}: the error taxonomy "
            "derives a blanket `From<std::io::Error>`. Every I/O failure must "
            "be mapped explicitly, so a reclaim failure cannot become a "
            "provisioning failure by accident."
        )
    return violations


def check_explicit_io(scratch: Source) -> list[str]:
    """G2 — the reclaim path maps every I/O failure explicitly."""
    violations: list[str] = []
    for name in RECLAIM_FUNCTIONS:
        span = scratch.body(name)
        if span is None:
            violations.append(
                f"{scratch.rel}: the reclaim path no longer has `{name}`. The "
                "invariant cannot be checked against a function that is gone."
            )
            continue
        body = scratch.code[span[0] : span[1]]
        for match in _IO_CALL.finditer(body):
            index = match.end()
            depth = 1
            while index < len(body) and depth:
                if body[index] == "(":
                    depth += 1
                elif body[index] == ")":
                    depth -= 1
                index += 1
            rest = body[index:].lstrip()
            if rest.startswith("?"):
                offset = span[0] + index + (len(body[index:]) - len(rest))
                violations.append(
                    f"{scratch.rel}:{scratch.line(offset)}: `{name}` propagates "
                    "an I/O error with `?`. A reclaim failure must be mapped "
                    "explicitly onto the reclaim variant."
                )
        for match in _BARE_INTO.finditer(body):
            violations.append(
                f"{scratch.rel}:{scratch.line(span[0] + match.start())}: "
                f"`{name}` converts an I/O error with a bare `.into()`. The "
                "blanket conversion lands on the provisioning variant; the "
                "reclaim path must name its own."
            )
        if "ScratchError::Io" in body:
            violations.append(
                f"{scratch.rel}:{scratch.line(span[0] + body.find('ScratchError::Io'))}: "
                f"`{name}` names the provisioning variant. An error that "
                "escapes a reclaim must say that the failure was a reclaim."
            )
    return violations


def check_one_removal(scratch: Source) -> list[str]:
    """G3 — one removal routine, and only under an emptiness proof."""
    violations: list[str] = []
    production = scratch.code[: scratch.production_end]
    occurrences = [
        match.start()
        for match in re.finditer(re.escape(REMOVAL_CALL), production)
    ]
    if len(occurrences) != 1:
        violations.append(
            f"{scratch.rel}: the scratch tree is removed in {len(occurrences)} "
            "places. There is exactly one removal routine; a second one is a "
            "second set of checks."
        )
        return violations
    offset = occurrences[0]
    holder = scratch.enclosing(offset)
    if holder is None:
        violations.append(
            f"{scratch.rel}:{scratch.line(offset)}: the removal call is not "
            "inside a function this guard can read."
        )
        return violations
    span = scratch.body(holder)
    if span is None:
        violations.append(
            f"{scratch.rel}:{scratch.line(offset)}: the removal call sits in "
            f"`{holder}`, whose body this guard cannot read."
        )
        return violations
    body = scratch.code[span[0] : span[1]]
    removal_at = offset - span[0]
    predicate_at = body.find(EMPTINESS_PREDICATE + "(")
    if predicate_at == -1:
        violations.append(
            f"{scratch.rel}:{scratch.line(offset)}: `{holder}` removes a tree "
            f"without consulting `{EMPTINESS_PREDICATE}`. A tree whose owner "
            "record is missing may only be removed when it holds no entries "
            "at all; a tree that holds content cannot be proven owned."
        )
    elif predicate_at > removal_at:
        violations.append(
            f"{scratch.rel}:{scratch.line(offset)}: `{holder}` removes the tree "
            f"before it consults `{EMPTINESS_PREDICATE}`. The emptiness proof "
            "must come first."
        )
    return violations


def check_refusal_is_shared(scratch: Source) -> list[str]:
    """G3b — the ownership refusal is produced by the shared rule only."""
    violations: list[str] = []
    production = scratch.code[: scratch.production_end]
    constructions = [
        match.start()
        for match in re.finditer(r"ReclaimRefusal::" + OWNERSHIP_REFUSAL, production)
    ]
    if not constructions:
        violations.append(
            f"{scratch.rel}: nothing produces the `{OWNERSHIP_REFUSAL}` refusal. "
            "A tree that holds content and has no owner record must be refused "
            "with a name, not removed and not reported as a failure."
        )
        return violations
    producers = {scratch.enclosing(offset) for offset in constructions}
    if len(producers) != 1:
        named = ", ".join(sorted(str(p) for p in producers))
        violations.append(
            f"{scratch.rel}: the `{OWNERSHIP_REFUSAL}` refusal is produced by "
            f"{len(producers)} functions ({named}). The rule lives in one "
            "place, so the validator and the census cannot disagree about it."
        )
    return violations


def check_mirrored_labels(
    scratch: Source, protocol: Source, daemon: Source, main: Source
) -> list[str]:
    """G4 — the refusal taxonomy is mirrored, with the same labels."""
    violations: list[str] = []
    validator = validator_labels(scratch)
    if not validator:
        violations.append(
            f"{scratch.rel}: the refusal taxonomy exposes no labels, so there "
            "is nothing to mirror."
        )
        return violations
    for name, labels, owner in (
        ("the protocol DTO", protocol_labels(protocol), protocol),
        ("the daemon's DTO mapping", daemon_labels(daemon), daemon),
        ("the CLI's label map", cli_labels(main), main),
    ):
        if not labels:
            violations.append(
                f"{owner.rel}: {name} exposes no refusal labels. A refusal the "
                "operator cannot name is a refusal the operator cannot act on."
            )
            continue
        if validator != labels:
            violations.append(
                f"{owner.rel}: {name} and the validator disagree about the "
                f"refusal labels (validator: {sorted(validator)}; {name}: "
                f"{sorted(labels)}). One rule, one name."
            )
    return violations


def check_census_shares_the_rule(scratch: Source) -> list[str]:
    """G5 — the census derives its verdict from the validator's rule."""
    violations: list[str] = []
    production = scratch.code[: scratch.production_end]
    definitions = len(re.findall(r"\bfn\s+reclaim_verdict\b", production))
    if definitions != 1:
        violations.append(
            f"{scratch.rel}: the reclaim rule is defined {definitions} times. It "
            "is one rule: the validator applies it and the census reports it."
        )
    span = scratch.body("census_tree")
    if span is None:
        violations.append(f"{scratch.rel}: the census has no `census_tree` to check.")
        return violations
    body = scratch.code[span[0] : span[1]]
    if "reclaim_verdict(" not in body:
        violations.append(
            f"{scratch.rel}: the census does not consult the reclaim rule. A "
            "report that restates the rule independently can claim a tree is "
            "reclaimable when the validator would refuse it."
        )
    return violations


def check_repository(root: Path) -> list[str]:
    """Every control, against the sources under `root`."""
    sources = {
        "scratch": Source(root / SCRATCH_RS, root),
        "protocol": Source(root / PROTOCOL_RS, root),
        "daemon": Source(root / DAEMON_RS, root),
        "main": Source(root / MAIN_RS, root),
    }
    for name, path in (
        ("scratch", SCRATCH_RS),
        ("protocol", PROTOCOL_RS),
        ("daemon", DAEMON_RS),
        ("main", MAIN_RS),
    ):
        if not (root / path).is_file():
            return [f"{path}: the file this guard reads is missing."]

    violations: list[str] = []
    violations.extend(check_taxonomy(sources["scratch"]))
    violations.extend(check_explicit_io(sources["scratch"]))
    violations.extend(check_one_removal(sources["scratch"]))
    violations.extend(check_refusal_is_shared(sources["scratch"]))
    violations.extend(
        check_mirrored_labels(
            sources["scratch"], sources["protocol"], sources["daemon"], sources["main"]
        )
    )
    violations.extend(check_census_shares_the_rule(sources["scratch"]))
    return violations


# ── Self-test ──────────────────────────────────────────────────────────


def _copy_sources(root: Path, destination: Path) -> Path:
    for relative in (SCRATCH_RS, PROTOCOL_RS, DAEMON_RS, MAIN_RS):
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / relative, target)
    return destination


def _inject(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if old not in text:
        raise SystemExit(f"self-test could not find the anchor text in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def _self_test() -> int:
    """Prove the guard rejects each defect class it is meant to catch."""
    failures: list[str] = []
    with tempfile.TemporaryDirectory(prefix="codegg-scratch-reclaim-") as raw:
        copy = _copy_sources(ROOT, Path(raw))
        if check_repository(copy):
            failures.append("the unmodified copy must pass the guard")
            _report(failures)
            return 1

        cases: list[tuple[str, Path, str, str]] = [
            (
                "a blanket io conversion in the error taxonomy",
                SCRATCH_RS,
                "Io(#[source] std::io::Error)",
                "Io(#[from] std::io::Error)",
            ),
            (
                "a reclaim path that propagates an I/O error with `?`",
                SCRATCH_RS,
                "let metadata = std::fs::symlink_metadata(tombstone).map_err(reclaim_io)?;",
                "let metadata = std::fs::symlink_metadata(tombstone)?;",
            ),
            (
                "a reclaim path that converts an I/O error with `.into()`",
                SCRATCH_RS,
                "Err(error) => return Err(reclaim_io(error)),",
                "Err(error) => return Err(error.into()),",
            ),
            (
                "a reclaim path that names the provisioning variant",
                SCRATCH_RS,
                "ScratchError::ReclaimIo(error))",
                "ScratchError::Io(error))",
            ),
            (
                "a second removal routine",
                SCRATCH_RS,
                "    std::fs::remove_dir_all(tombstone).map_err(reclaim_io)?;",
                "    std::fs::remove_dir_all(tombstone).map_err(reclaim_io)?;\n"
                "    std::fs::remove_dir_all(tombstone).map_err(reclaim_io)?;",
            ),
            (
                "a removal that does not consult the emptiness predicate",
                SCRATCH_RS,
                "            let empty = tree_is_empty(tombstone, owner)?;",
                "            let empty = true;",
            ),
            (
                "a refusal label the protocol does not carry",
                PROTOCOL_RS,
                "    OwnershipUnverifiable,",
                "    OwnershipUnverified,",
            ),
            (
                "a census that restates the rule instead of sharing it",
                SCRATCH_RS,
                "    let verdict = reclaim_verdict(owner_record, usage.refusal, entry_count);",
                "    let verdict = match usage.refusal {\n"
                "        None => ReclaimVerdict::Acceptable,\n"
                "        Some(reason) => ReclaimVerdict::Refused(reason),\n"
                "    };",
            ),
        ]

        for label, relative, old, new in cases:
            with tempfile.TemporaryDirectory(
                prefix="codegg-scratch-reclaim-case-"
            ) as raw_case:
                case_copy = _copy_sources(ROOT, Path(raw_case))
                _inject(case_copy / relative, old, new)
                violations = check_repository(case_copy)
                if not violations:
                    failures.append(f"the guard did not reject {label}")
                else:
                    print(f"  rejected {label}: {violations[0]}")

    if failures:
        _report(failures)
        return 1
    print("scratch reclaim invariants guard self-test passed")
    return 0


def _report(failures: list[str]) -> None:
    for failure in failures:
        print(f"ERROR: {failure}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="prove the guard rejects each defect class it is meant to catch",
    )
    args = parser.parse_args()
    if args.self_test:
        return _self_test()
    violations = check_repository(ROOT)
    if violations:
        for violation in violations:
            print(f"ERROR: {violation}", file=sys.stderr)
        print(f"{len(violations)} scratch reclaim invariant violation(s)", file=sys.stderr)
        return 1
    print("scratch reclaim invariants guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
