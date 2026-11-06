#!/usr/bin/env python3
"""Validate the repository knowledge base.

Checks that AGENTS.md stays a short map, that required documents exist, that
every document is reachable from AGENTS.md and listed in its directory index,
that relative links and anchors resolve, that documents carry front matter with
an owner and a review date, and that ExecPlans keep their living sections.

Every finding ends with a `fix:` hint so an agent can remediate it directly.

Usage:
    python3 harness/check_docs.py             # fail on structural errors
    python3 harness/check_docs.py --strict    # also fail on unfilled template content

Requires Python 3.11+ and nothing outside the standard library.
"""

from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import re
import sys
import tomllib
import urllib.parse
from collections import deque
from dataclasses import dataclass
from pathlib import Path

MARKER = "TODO(harness)"
TEMPLATE_NAME = "TEMPLATE.md"
INDEX_NAMES = ("index.md", "README.md")

FENCE_RE = re.compile(r"^\s{0,3}(`{3,}|~{3,})")
INLINE_CODE_RE = re.compile(r"(`+).+?\1")
HEADING_RE = re.compile(r"^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$")
LINK_RE = re.compile(r"!?\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+[\"'][^\"']*[\"'])?\s*\)")
REF_DEF_RE = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s+.*)?$")
SCHEME_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")
UNCHECKED_RE = re.compile(r"^\s*[-*+]\s+\[ \]")


@dataclass(frozen=True, order=True)
class Finding:
    path: str
    line: int
    severity: str  # "error" or "warning"
    message: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line}: {self.severity}: {self.message}"


@dataclass
class Link:
    line: int
    target: str
    resolved: Path | None  # None for same-document anchors
    fragment: str


@dataclass
class Doc:
    path: Path
    rel: str
    lines: list[str]
    in_code: list[bool]
    prose: list[str]  # lines with code blocks blanked and inline code removed
    front_matter: dict[str, str] | None
    front_matter_end: int  # index of the first body line

    @property
    def is_template(self) -> bool:
        return self.path.name == TEMPLATE_NAME


def slugify(heading: str) -> str:
    """Approximate GitHub's heading anchor algorithm."""
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", heading)
    text = text.strip().lower()
    text = re.sub(r"[^\w\- ]", "", text)
    return text.replace(" ", "-")


def code_mask(lines: list[str]) -> list[bool]:
    mask, fence = [], ""
    for line in lines:
        m = FENCE_RE.match(line)
        if fence:
            mask.append(True)
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence):
                fence = ""
        elif m:
            mask.append(True)
            fence = m.group(1)
        else:
            mask.append(False)
    return mask


def parse_front_matter(lines: list[str]) -> tuple[dict[str, str] | None, int]:
    if not lines or lines[0].strip() != "---":
        return None, 0
    for i in range(1, len(lines)):
        if lines[i].strip() == "---":
            fields = {}
            for raw in lines[1:i]:
                key, sep, value = raw.partition(":")
                if sep and key.strip():
                    fields[key.strip()] = value.strip().strip("\"'")
            return fields, i + 1
    return None, 0


def load_doc(path: Path, root: Path) -> Doc:
    lines = path.read_text(encoding="utf-8").splitlines()
    front_matter, end = parse_front_matter(lines)
    mask = code_mask(lines)
    prose = [
        "" if (code or i < end) else INLINE_CODE_RE.sub("", line)
        for i, (line, code) in enumerate(zip(lines, mask))
    ]
    return Doc(path, path.relative_to(root).as_posix(), lines, mask, prose, front_matter, end)


def anchors(doc: Doc) -> set[str]:
    seen: dict[str, int] = {}
    result = set()
    for i, line in enumerate(doc.lines):
        if doc.in_code[i] or i < doc.front_matter_end:
            continue
        m = HEADING_RE.match(line)
        if not m:
            continue
        slug = slugify(m.group(2))
        count = seen.get(slug, 0)
        seen[slug] = count + 1
        result.add(slug if count == 0 else f"{slug}-{count}")
    return result


def headings(doc: Doc, level: int) -> dict[str, int]:
    """Map heading text at the given level to its 0-based line index."""
    result = {}
    for i, line in enumerate(doc.lines):
        if doc.in_code[i]:
            continue
        m = HEADING_RE.match(line)
        if m and len(m.group(1)) == level:
            result.setdefault(m.group(2).strip(), i)
    return result


def has_glob(part: str) -> bool:
    return any(c in part for c in "*?[")


def discover(root: Path, patterns: list[str]) -> list[Path]:
    found: set[Path] = set()
    for pattern in patterns:
        parts = pattern.split("/")
        if not any(has_glob(p) for p in parts):
            candidate = root / pattern
            if candidate.is_file():
                found.add(candidate)
            continue
        base = root.joinpath(*[p for p in parts[: next(i for i, p in enumerate(parts) if has_glob(p))]])
        if not base.is_dir():
            continue
        for path in base.rglob("*.md"):
            if path.is_file() and fnmatch.fnmatch(path.relative_to(root).as_posix(), pattern):
                found.add(path)
    return sorted(found)


class Checker:
    def __init__(self, root: Path, config: dict, strict: bool, today: dt.date):
        self.root = root.resolve()
        self.config = config
        self.strict = strict
        self.today = today
        self.findings: list[Finding] = []
        paths = discover(self.root, config.get("scope", []))
        self.docs = {p: load_doc(p, self.root) for p in paths}
        self.links: dict[Path, list[Link]] = {}
        self._anchor_cache: dict[Path, set[str]] = {}

    def report(self, rel: str, line: int, severity: str, message: str, fix: str) -> None:
        self.findings.append(Finding(rel, line, severity, f"{message} (fix: {fix})"))

    def run(self) -> list[Finding]:
        self.check_required()
        self.check_agents_md()
        for doc in self.docs.values():
            self.links[doc.path] = self.check_links(doc)
            self.check_front_matter(doc)
            self.check_template_content(doc)
        self.check_reachability()
        self.check_indexes()
        self.check_exec_plans()
        return sorted(set(self.findings))

    def check_required(self) -> None:
        for rel in self.config.get("required", {}).get("files", []):
            if not (self.root / rel).is_file():
                self.report(rel, 0, "error", "required document is missing",
                            f"create {rel}; see harness/README.md for what it must contain")

    def check_agents_md(self) -> None:
        cfg = self.config.get("agents_md", {})
        path = self.root / cfg.get("path", "AGENTS.md")
        doc = self.docs.get(path)
        if doc is None:
            return
        count, target, limit = len(doc.lines), cfg.get("target_lines", 100), cfg.get("max_lines", 150)
        if count > limit:
            self.report(doc.rel, count, "error", f"{count} lines exceeds the {limit}-line limit",
                        "move detail into a focused doc under docs/ and link to it")
        elif count > target:
            self.report(doc.rel, count, "warning", f"{count} lines exceeds the {target}-line target",
                        "AGENTS.md is a map; move detail into docs/")

    def check_links(self, doc: Doc) -> list[Link]:
        links = []
        for i, line in enumerate(doc.prose):
            targets = [m.group(1) for m in LINK_RE.finditer(line)]
            ref = REF_DEF_RE.match(line)
            if ref:
                targets.append(ref.group(1))
            for target in targets:
                link = self.resolve(doc, i + 1, target)
                if link is not None:
                    links.append(link)
        return links

    def resolve(self, doc: Doc, line: int, target: str) -> Link | None:
        if SCHEME_RE.match(target) or target.startswith("//"):
            return None
        raw_path, _, fragment = target.partition("#")
        raw_path = urllib.parse.unquote(raw_path)
        if not raw_path:
            if fragment and fragment not in self.anchors_of(doc.path):
                self.report(doc.rel, line, "error", f"anchor #{fragment} not found in this document",
                            "link to an existing heading or rename the heading")
            return Link(line, target, None, fragment)
        base = self.root if raw_path.startswith("/") else doc.path.parent
        resolved = (base / raw_path.lstrip("/")).resolve()
        if not resolved.exists():
            self.report(doc.rel, line, "error", f"broken link to {raw_path}",
                        "point the link at an existing path, or remove it if the target was deleted")
            return Link(line, target, resolved, fragment)
        if fragment and resolved.is_file() and resolved.suffix == ".md":
            if fragment not in self.anchors_of(resolved):
                self.report(doc.rel, line, "error", f"anchor #{fragment} not found in {raw_path}",
                            "link to an existing heading in the target document")
        return Link(line, target, resolved, fragment)

    def anchors_of(self, path: Path) -> set[str]:
        if path not in self._anchor_cache:
            doc = self.docs.get(path) or load_doc(path, self.root)
            self._anchor_cache[path] = anchors(doc)
        return self._anchor_cache[path]

    def doc_for(self, target: Path) -> Path | None:
        if target in self.docs:
            return target
        if target.is_dir():
            for name in INDEX_NAMES:
                if target / name in self.docs:
                    return target / name
        return None

    def check_reachability(self) -> None:
        start = self.root / self.config.get("agents_md", {}).get("path", "AGENTS.md")
        if start not in self.docs:
            return
        seen, queue = {start}, deque([start])
        while queue:
            for link in self.links.get(queue.popleft(), []):
                nxt = self.doc_for(link.resolved) if link.resolved else None
                if nxt and nxt not in seen:
                    seen.add(nxt)
                    queue.append(nxt)
        for path, doc in self.docs.items():
            if path not in seen:
                self.report(doc.rel, 0, "error", "document is not reachable from AGENTS.md",
                            "link it from its directory index, or delete it if obsolete")

    def check_indexes(self) -> None:
        docs_root = self.root / "docs"
        dirs = set()
        for path in self.docs:
            parent = path.parent
            while parent == docs_root or docs_root in parent.parents:
                dirs.add(parent)
                parent = parent.parent
        for directory in sorted(dirs):
            index = next((directory / n for n in INDEX_NAMES if directory / n in self.docs), None)
            rel_dir = directory.relative_to(self.root).as_posix()
            if index is None:
                self.report(rel_dir, 0, "error", "directory has no index.md",
                            f"add {rel_dir}/index.md that lists every document in the directory")
                continue
            linked = {link.resolved for link in self.links.get(index, []) if link.resolved}
            children = [p for p in self.docs if p.parent == directory and p != index]
            children += sorted({d for d in dirs if d.parent == directory})
            for child in sorted(children):
                covered = child in linked or (child.is_dir() and self.doc_for(child) in linked)
                if not covered:
                    name = child.name + ("/" if child.is_dir() else "")
                    self.report(self.docs[index].rel, 0, "error", f"index does not link {name}",
                                f"add a row or bullet for {name} with a one-line description")

    def check_front_matter(self, doc: Doc) -> None:
        cfg = self.config.get("front_matter", {})
        if doc.is_template or any(fnmatch.fnmatch(doc.rel, pattern) for pattern in cfg.get("exempt", [])):
            return
        fm = doc.front_matter
        if fm is None:
            self.report(doc.rel, 1, "error", "missing front matter",
                        "start the file with ---, status, owner, last_reviewed, ---")
            return
        for key in cfg.get("required_keys", []):
            if not fm.get(key):
                self.report(doc.rel, 1, "error", f"front matter lacks {key}", f"add `{key}:` to the front matter")
        status = fm.get("status")
        statuses = cfg.get("statuses", [])
        if status and statuses and status not in statuses:
            self.report(doc.rel, 1, "error", f"unknown status {status!r}", f"use one of: {', '.join(statuses)}")
        reviewed = fm.get("last_reviewed")
        if not reviewed:
            return
        try:
            date = dt.date.fromisoformat(reviewed)
        except ValueError:
            self.report(doc.rel, 1, "error", f"last_reviewed {reviewed!r} is not a YYYY-MM-DD date",
                        "use an ISO date such as 2026-01-31")
            return
        age = (self.today - date).days
        if age < 0:
            self.report(doc.rel, 1, "error", f"last_reviewed {reviewed} is in the future", "use the real review date")
        elif age > cfg.get("stale_after_days", 90) and status not in ("deprecated", "superseded", "rejected"):
            self.report(doc.rel, 1, "warning", f"last reviewed {age} days ago",
                        "re-verify the document against the code, then update last_reviewed")

    def check_template_content(self, doc: Doc) -> None:
        if doc.is_template:
            return
        markers = [i + 1 for i, line in enumerate(doc.lines[: doc.front_matter_end]) if MARKER in line]
        markers += [i + 1 for i, line in enumerate(doc.prose) if MARKER in line]
        is_template_status = (doc.front_matter or {}).get("status") == "template"
        if not markers and not is_template_status:
            return
        if self.strict:
            for line in markers:
                self.report(doc.rel, line, "error", f"unfilled {MARKER} marker",
                            "replace it with repository-specific content, or state the unknown explicitly")
            if is_template_status:
                self.report(doc.rel, 1, "error", "status is still `template`",
                            "set status to draft or verified once the content is filled in")
            return
        parts = []
        if is_template_status:
            parts.append("status is `template`")
        if markers:
            parts.append(f"{len(markers)} {MARKER} marker(s)")
        self.report(doc.rel, markers[0] if markers else 1, "warning", "unfilled template: " + ", ".join(parts),
                    "fill it in; harness/prompts/bootstrap.md describes how")

    def check_exec_plans(self) -> None:
        cfg = self.config.get("exec_plans", {})
        completed = self.root / cfg.get("completed_dir", "")
        required = cfg.get("required_sections", [])
        for rel_dir in cfg.get("dirs", []):
            directory = self.root / rel_dir
            for path, doc in self.docs.items():
                if path.parent != directory or path.name in INDEX_NAMES:
                    continue
                sections = headings(doc, 2)
                for name in required:
                    if name not in sections:
                        self.report(doc.rel, 0, "error", f"ExecPlan lacks section '## {name}'",
                                    "add it; docs/PLANS.md has the skeleton")
                if directory == completed and "Progress" in sections:
                    self.check_progress_done(doc, sections["Progress"])

    def check_progress_done(self, doc: Doc, start: int) -> None:
        for i in range(start + 1, len(doc.lines)):
            if not doc.in_code[i] and HEADING_RE.match(doc.lines[i]):
                break
            if UNCHECKED_RE.match(doc.lines[i]):
                self.report(doc.rel, i + 1, "error", "completed ExecPlan has an unchecked Progress item",
                            "finish the item, or move the plan back to docs/exec-plans/active/")


def check(root: Path, config: dict, strict: bool = False, today: dt.date | None = None) -> list[Finding]:
    return Checker(root, config, strict, today or dt.date.today()).run()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent,
                        help="repository root (default: parent of harness/)")
    parser.add_argument("--config", type=Path, help="config file (default: <root>/harness/config.toml)")
    parser.add_argument("--strict", action="store_true", help="fail on unfilled template content")
    args = parser.parse_args(argv)

    config_path = args.config or args.root / "harness" / "config.toml"
    try:
        config = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        print(f"check_docs: config not found: {config_path}", file=sys.stderr)
        return 2

    findings = check(args.root, config, strict=args.strict)
    for finding in findings:
        print(finding)
    errors = sum(f.severity == "error" for f in findings)
    warnings = len(findings) - errors
    print(f"check_docs: {errors} error(s), {warnings} warning(s)", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
