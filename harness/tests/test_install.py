"""Tests for harness/install.py. Run: python3 -m unittest discover -s harness/tests"""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

HARNESS = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HARNESS))

import install  # noqa: E402


class InstallTest(unittest.TestCase):
    def test_copies_missing_files_and_never_overwrites(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp)
            (target / "AGENTS.md").write_text("existing instructions\n", encoding="utf-8")

            copied, skipped = install.install(target)

            self.assertEqual(skipped, [Path("AGENTS.md")])
            self.assertEqual((target / "AGENTS.md").read_text(encoding="utf-8"), "existing instructions\n")
            self.assertIn(Path("docs/README.md"), copied)
            self.assertTrue((target / "harness/check_docs.py").is_file())
            self.assertTrue((target / ".github/workflows/docs.yml").is_file())
            self.assertFalse(any("__pycache__" in p.parts for p in copied))

    def test_dry_run_writes_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            copied, _ = install.install(Path(tmp), dry_run=True)
            self.assertTrue(copied)
            self.assertEqual(list(Path(tmp).iterdir()), [])


if __name__ == "__main__":
    unittest.main()
