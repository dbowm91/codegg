from __future__ import annotations

import hashlib
import os
from pathlib import Path
import tempfile
import unittest

from dev.harness_lab.supervisor import SubjectSpec


class SubjectEnvironmentTests(unittest.TestCase):
    def test_lane_env_replaces_runtime_and_user_roots(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"candidate")
            binary.chmod(0o755)
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            original = dict(os.environ)
            os.environ.update({
                "CODEGG_DAEMON_HOME": "/shared/daemon",
                "CODEGG_DATA_HOME": "/shared/data",
                "CODEGG_TUI_CONFIG": "/shared/config.jsonc",
                "CODEGG_CORE_ENDPOINT": "unix:///shared.sock",
                "OPENAI_API_KEY": "must-not-inherit",
                "XDG_RUNTIME_DIR": "/shared/runtime",
            })
            try:
                env = spec.environment()
            finally:
                os.environ.clear()
                os.environ.update(original)
            self.assertEqual(env["CODEGG_DAEMON_HOME"], str(root / "lane" / "daemon"))
            self.assertEqual(env["CODEGG_DATA_HOME"], str(root / "lane" / "data"))
            self.assertEqual(env["HOME"], str(root / "lane" / "home"))
            self.assertEqual(env["XDG_RUNTIME_DIR"], str(root / "lane" / "runtime"))
            self.assertNotIn("CODEGG_CORE_ENDPOINT", env)
            self.assertNotIn("OPENAI_API_KEY", env)
            self.assertTrue((root / "lane" / "config").is_dir())

    def test_subject_hash_is_pinned_at_creation(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "codegg"
            binary.write_bytes(b"one")
            spec = SubjectSpec.create("a", binary, root, root / "lane", root)
            binary.write_bytes(b"two")
            self.assertNotEqual(spec.binary_sha256, hashlib.sha256(binary.read_bytes()).hexdigest())


if __name__ == "__main__":
    unittest.main()
