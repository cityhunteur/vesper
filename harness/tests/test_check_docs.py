"""Tests for harness/check_docs.py. Run: python3 -m unittest discover -s harness/tests"""

from __future__ import annotations

import datetime as dt
import sys
import tempfile
import textwrap
import tomllib
import unittest
from pathlib import Path

HARNESS = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HARNESS))

import check_docs  # noqa: E402

TODAY = dt.date(2026, 10, 20)
FRONT = "---\nstatus: draft\nowner: team\nlast_reviewed: 2026-10-01\n---\n"

CONFIG = {
    "scope": ["AGENTS.md", "ARCHITECTURE.md", "docs/*.md"],
    "agents_md": {"path": "AGENTS.md", "target_lines": 5, "max_lines": 10},
    "required": {"files": ["AGENTS.md", "docs/README.md"]},
    "front_matter": {
        "required_keys": ["status", "owner", "last_reviewed"],
        "statuses": ["template", "draft", "verified"],
        "stale_after_days": 90,
        "exempt": ["AGENTS.md", "docs/exec-plans/active/*", "docs/exec-plans/completed/*"],
    },
    "exec_plans": {
        "dirs": ["docs/exec-plans/active", "docs/exec-plans/completed"],
        "completed_dir": "docs/exec-plans/completed",
        "required_sections": ["Purpose / Big Picture", "Progress", "Decision Log"],
    },
}


class CheckDocsTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.write("AGENTS.md", "# Agents\n\nSee [docs](docs/README.md).\n")
        self.write("docs/README.md", FRONT + "# Docs\n")

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def write(self, rel: str, text: str) -> None:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text), encoding="utf-8")

    def findings(self, strict: bool = False) -> list[check_docs.Finding]:
        return check_docs.check(self.root, CONFIG, strict=strict, today=TODAY)

    def messages(self, severity: str = "error", strict: bool = False) -> list[str]:
        return [f"{f.path}: {f.message}" for f in self.findings(strict) if f.severity == severity]

    def assertFinding(self, fragment: str, severity: str = "error", strict: bool = False) -> None:
        messages = self.messages(severity, strict)
        self.assertTrue(any(fragment in m for m in messages), f"{fragment!r} not in {messages}")

    def test_minimal_repository_is_clean(self) -> None:
        self.assertEqual(self.findings(), [])

    def test_missing_required_file(self) -> None:
        (self.root / "docs/README.md").unlink()
        self.assertFinding("docs/README.md: required document is missing")

    def test_agents_md_length(self) -> None:
        self.write("AGENTS.md", "[docs](docs/README.md)\n" + "line\n" * 6)
        self.assertFinding("exceeds the 5-line target", severity="warning")
        self.write("AGENTS.md", "[docs](docs/README.md)\n" + "line\n" * 12)
        self.assertFinding("exceeds the 10-line limit")

    def test_broken_link_and_anchor(self) -> None:
        self.write("docs/README.md", FRONT + "# Docs\n\n[x](missing.md) [y](#nope) [z](../AGENTS.md#agents)\n")
        self.assertFinding("broken link to missing.md")
        self.assertFinding("anchor #nope not found in this document")
        self.assertEqual(len(self.messages()), 2)

    def test_anchor_slugs_follow_github(self) -> None:
        self.write("docs/README.md", FRONT + "# Docs\n\n## Surprises & Discoveries\n\n## Notes\n\n## Notes\n\n"
                   "[a](#surprises--discoveries) [b](#notes-1)\n")
        self.assertEqual(self.messages(), [])

    def test_links_in_code_are_ignored(self) -> None:
        self.write("docs/README.md", FRONT + "# Docs\n\n`[x](missing.md)`\n\n```\n[y](missing.md)\n```\n")
        self.assertEqual(self.messages(), [])

    def test_orphan_and_index_coverage(self) -> None:
        self.write("docs/guide.md", FRONT + "# Guide\n")
        self.assertFinding("docs/guide.md: document is not reachable from AGENTS.md")
        self.assertFinding("docs/README.md: index does not link guide.md")

    def test_subdirectory_needs_index(self) -> None:
        self.write("docs/README.md", FRONT + "# Docs\n\n[specs](specs/)\n")
        self.write("docs/specs/a.md", FRONT + "# A\n")
        self.assertFinding("docs/specs: directory has no index.md")

    def test_subdirectory_linked_via_index(self) -> None:
        self.write("docs/README.md", FRONT + "# Docs\n\n[specs](specs/index.md)\n")
        self.write("docs/specs/index.md", FRONT + "# Specs\n\n[a](a.md)\n")
        self.write("docs/specs/a.md", FRONT + "# A\n")
        self.assertEqual(self.messages(), [])

    def test_front_matter(self) -> None:
        self.write("docs/README.md", "# Docs\n")
        self.assertFinding("missing front matter")
        self.write("docs/README.md", "---\nstatus: bogus\nlast_reviewed: 2026-13-01\n---\n# Docs\n")
        self.assertFinding("front matter lacks owner")
        self.assertFinding("unknown status 'bogus'")
        self.assertFinding("is not a YYYY-MM-DD date")

    def test_stale_review_warns(self) -> None:
        self.write("docs/README.md", FRONT.replace("2026-10-01", "2026-01-01") + "# Docs\n")
        self.assertFinding("last reviewed 292 days ago", severity="warning")

    def test_template_content_warns_then_fails_in_strict(self) -> None:
        self.write("docs/README.md", FRONT.replace("draft", "template") + "# Docs\n\n> TODO(harness): fill\n")
        self.assertFinding("unfilled template", severity="warning")
        self.assertEqual(self.messages(), [])
        self.assertFinding("unfilled TODO(harness) marker", strict=True)
        self.assertFinding("status is still `template`", strict=True)

    def test_template_files_are_exempt(self) -> None:
        self.write("docs/README.md", FRONT + "# Docs\n\n[t](TEMPLATE.md)\n")
        self.write("docs/TEMPLATE.md", "---\nstatus: draft\nlast_reviewed: YYYY-MM-DD\n---\nTODO(harness)\n")
        self.assertEqual(self.messages(strict=True), [])

    def test_exec_plan_sections_and_progress(self) -> None:
        self.write("docs/README.md", FRONT + "# Docs\n\n[plans](exec-plans/index.md)\n")
        self.write("docs/exec-plans/index.md", FRONT + "[a](active/index.md) [c](completed/index.md)\n")
        self.write("docs/exec-plans/active/index.md", "[p](plan.md)\n")
        self.write("docs/exec-plans/active/plan.md", "# P\n\n## Progress\n\n- [ ] todo\n")
        self.write("docs/exec-plans/completed/index.md", "[d](done.md)\n")
        self.write("docs/exec-plans/completed/done.md",
                   "# D\n\n## Purpose / Big Picture\n\n## Progress\n\n- [x] a\n- [ ] b\n\n## Decision Log\n")
        self.assertFinding("plan.md: ExecPlan lacks section '## Purpose / Big Picture'")
        self.assertFinding("done.md: completed ExecPlan has an unchecked Progress item")
        self.assertEqual(len(self.messages()), 3)  # plan.md lacks two sections; done.md has one unchecked item

    def test_every_finding_has_a_fix_hint(self) -> None:
        self.write("docs/README.md", "# Docs\n\n[x](missing.md)\n")
        self.write("docs/orphan.md", "# Orphan\n")
        findings = self.findings(strict=True)
        self.assertTrue(findings)
        self.assertTrue(all("(fix: " in f.message for f in findings))


class TemplateRepositoryTest(unittest.TestCase):
    """The template itself must pass the non-strict check with its shipped config."""

    def test_shipped_template_has_no_errors(self) -> None:
        root = HARNESS.parent
        config = tomllib.loads((HARNESS / "config.toml").read_text(encoding="utf-8"))
        errors = [str(f) for f in check_docs.check(root, config) if f.severity == "error"]
        self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()
