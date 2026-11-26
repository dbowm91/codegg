#!/usr/bin/env python3
"""Emit GitHub annotations for failed nextest JUnit testcases.

Does not select tests. A missing report is a failure: an unnamed red run
is the defect this script exists to make visible.
"""

from __future__ import annotations

import argparse
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path

MAX_ANNOTATIONS = 10
MAX_MESSAGE = 2000
FAILURE_TAGS = {"failure", "error", "flakyfailure"}


def local_name(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def workflow_escape(text: str, *, property_value: bool) -> str:
    text = text.replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
    if property_value:
        text = text.replace(":", "%3A").replace(",", "%2C")
    return text


def failed_cases(path: Path) -> list[tuple[str, str]]:
    try:
        root = ET.parse(path).getroot()
    except ET.ParseError as exc:
        raise SystemExit(f"nextest report is not XML: {path}: {exc}") from exc
    found: list[tuple[str, str]] = []
    for case in root.iter():
        if local_name(case.tag) != "testcase":
            continue
        bad = [
            child
            for child in list(case)
            if local_name(child.tag).lower() in FAILURE_TAGS
        ]
        if not bad:
            continue
        name = case.attrib.get("name") or "<unnamed>"
        classname = case.attrib.get("classname") or ""
        parts: list[str] = []
        for child in bad:
            message = child.attrib.get("message") or ""
            body = (child.text or "").strip()
            if message:
                parts.append(message)
            if body and body not in message:
                parts.append(body)
        if not parts:
            for child in list(case):
                if local_name(child.tag) in {"system-out", "system-err"} and (
                    child.text or ""
                ).strip():
                    parts.append((child.text or "").strip())
        title = name if not classname else f"{classname} {name}"
        found.append((title, "\n".join(parts) or "failed"))
    return found


def annotation_lines(found: list[tuple[str, str]]) -> list[str]:
    lines: list[str] = []
    for title, message in found[:MAX_ANNOTATIONS]:
        clipped = message[:MAX_MESSAGE]
        lines.append(
            "::error title="
            + workflow_escape(title, property_value=True)
            + "::"
            + workflow_escape(clipped, property_value=False)
        )
    if len(found) > MAX_ANNOTATIONS:
        extra = len(found) - MAX_ANNOTATIONS
        lines.append(
            "::warning title=nextest failures truncated::"
            + str(extra)
            + " more failed testcases are in the JUnit report"
        )
    for title, _message in found:
        lines.append("failed testcase: " + title)
    return lines


def evaluate(path: Path, expect_failure: bool) -> tuple[list[str], int]:
    if not path.is_file():
        return ["nextest report is missing: " + str(path)], 1
    found = failed_cases(path)
    lines = annotation_lines(found)
    if expect_failure and not found:
        lines.append("nextest report has no failed testcase: " + str(path))
        return lines, 1
    if found and not expect_failure:
        lines.append(
            "nextest report has failed testcases but this run was not marked failed"
        )
        return lines, 1
    return lines, 0


def self_test() -> int:
    xml = """<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="codegg" tests="3" failures="2">
    <testcase classname="codegg" name="passes"></testcase>
    <testcase classname="codegg" name="writer_permit::drops_quarantine">
      <failure message="assertion failed">expected quarantine</failure>
    </testcase>
    <testcase classname="codegg" name="flaky_case">
      <flakyFailure message="flaked">once</flakyFailure>
    </testcase>
  </testsuite>
</testsuites>
"""
    with tempfile.TemporaryDirectory() as tmp:
        report = Path(tmp) / "junit.xml"
        report.write_text(xml, encoding="utf-8")
        lines, code = evaluate(report, True)
        text = "\n".join(lines)
        if (
            code != 0
            or "writer_permit::drops_quarantine" not in text
            or "flaky_case" not in text
        ):
            print(
                "self-test: failed to name the failing testcase",
                lines,
                file=sys.stderr,
            )
            return 1
        if "::error" not in text or "title=codegg passes" in text:
            print("self-test: passing testcase was annotated", lines, file=sys.stderr)
            return 1
        if "::error title=codegg writer_permit%3A%3Adrops_quarantine::" not in text:
            print("self-test: failure title was not escaped", lines, file=sys.stderr)
            return 1
        clean = Path(tmp) / "clean.xml"
        clean.write_text("<testsuites></testsuites>", encoding="utf-8")
        _lines, clean_code = evaluate(clean, True)
        if clean_code == 0:
            print("self-test: empty failure report was accepted", file=sys.stderr)
            return 1
        _lines, green_code = evaluate(clean, False)
        if green_code != 0:
            print("self-test: clean report failed a green run", file=sys.stderr)
            return 1
        missing_lines, missing_code = evaluate(Path(tmp) / "absent.xml", True)
        if missing_code == 0 or not missing_lines:
            print("self-test: missing report was accepted", file=sys.stderr)
            return 1
    print("annotate_nextest_junit --self-test: pass")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--expect-failure", action="store_true")
    parser.add_argument("report", nargs="?", type=Path)
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if args.report is None:
        print("annotate_nextest_junit: report path is required", file=sys.stderr)
        return 1
    lines, code = evaluate(args.report, args.expect_failure)
    for line in lines:
        print(line)
    return code


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
