import json
import os
from pathlib import Path
import subprocess
import sys


EXPECTED = {
    "a005-rust-dependency-inspection": ("auto", True),
    "a005-rust-isolated-build": ("isolated_new", True),
    "a005-python-read-only": ("auto", True),
    "a005-python-isolated-debug": ("isolated_new", True),
    "a005-spanish-isolated-edit": ("isolated_new", True),
    "a005-existing-unavailable-refusal": ("existing_managed_read_only", False),
}


def main():
    transcript_path = os.environ.get("CODEGG_AUTONOMY_TRANSCRIPT")
    if not transcript_path:
        return 1
    try:
        with open(transcript_path, encoding="utf-8") as source:
            transcript = json.load(source)
        case_id = transcript["case_id"]
        expected, accepted = EXPECTED[case_id]
        all_task_calls = [call for call in transcript["tool_calls"]
                          if call.get("tool") == "task"]
        task_calls = [call for call in all_task_calls if call.get("task_placement")]
    except (OSError, ValueError, KeyError, TypeError):
        return 1
    root_value = os.environ.get("CODEGG_AUTONOMY_CASE_ROOT")
    if root_value:
        root = Path(root_value)
        try:
            head = subprocess.run(["git", "-C", str(root), "log", "-1", "--format=%s"],
                                  check=True, capture_output=True, text=True).stdout.strip()
            dirty = subprocess.run(["git", "-C", str(root), "status", "--porcelain"],
                                   check=True, capture_output=True, text=True).stdout
            worktree_output = subprocess.run(
                ["git", "-C", str(root), "worktree", "list", "--porcelain"],
                check=True, capture_output=True, text=True).stdout
        except (OSError, subprocess.SubprocessError):
            return 1
        if head != "frozen placement fixture" or dirty:
            return 1
        worktrees = [line.removeprefix("worktree ") for line in worktree_output.splitlines()
                     if line.startswith("worktree ")]
        if case_id == "a005-existing-unavailable-refusal":
            if len(worktrees) != 1:
                return 1
        elif expected == "auto":
            if len(worktrees) != 1:
                return 1
        else:
            candidates = [Path(path) for path in worktrees if Path(path) != root]
            if not candidates:
                return 1
            if case_id == "a005-rust-isolated-build":
                passed = any(subprocess.run(
                    ["cargo", "test", "--offline", "--manifest-path", "fixture/Cargo.toml"],
                    cwd=path, capture_output=True, timeout=90, check=False).returncode == 0
                    for path in candidates)
                if not passed:
                    return 1
            else:
                passed = any(subprocess.run(
                    [sys.executable, "-m", "unittest", "discover", "-s", "fixture/tests"],
                    cwd=path, capture_output=True, timeout=30, check=False).returncode == 0
                    for path in candidates)
                if not passed:
                    return 1
    if case_id == "a005-existing-unavailable-refusal":
        if len(task_calls) != 1:
            return 1
        call = task_calls[0]
        request = call["task_placement"]
        return int(not (request.get("requested") == expected
                        and request.get("worktree_id_present") is True
                        and call.get("status") == "failed"
                        and not call.get("placement_accepted")))
    return int(not any(
        call["task_placement"].get("requested") == expected
        and call.get("placement_accepted") is True
        for call in task_calls
    ) or not any(call.get("task_run_status") == "completed"
                 for call in all_task_calls))


if __name__ == "__main__":
    sys.exit(main())
