#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import List, Tuple

REQUIRED_FILES = [
    "AGENTS.md",
    "WORKFLOW.md",
    "ARCHITECTURE.md",
    "docs/README.md",
    "docs/PLANS.md",
    "docs/QUALITY_SCORE.md",
    "docs/RELIABILITY.md",
    "docs/SECURITY.md",
    "docs/DEVELOPMENT.md",
    "docs/TESTING.md",
    "docs/OBSERVABILITY.md",
    "docs/design-docs/index.md",
    "docs/design-docs/core-beliefs.md",
    "docs/design-docs/adr/index.md",
    "docs/exec-plans/index.md",
    "docs/exec-plans/active/index.md",
    "docs/exec-plans/completed/index.md",
    "docs/exec-plans/tech-debt-tracker.md",
    "docs/product-specs/index.md",
    "docs/generated/README.md",
    "docs/process/index.md",
    "docs/process/harness-engineering.md",
    "docs/progress/symphony-milestones.md",
    "docs/references/index.md",
    "docs/specs/index.md",
]

MARKDOWN_GLOBS = [
    "AGENTS.md",
    "WORKFLOW.md",
    "ARCHITECTURE.md",
    "docs/**/*.md",
]

LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def resolve_link(base: Path, target: str, root: Path) -> Tuple[bool, str]:
    target = target.strip()
    if not target or target.startswith("#"):
        return True, ""
    if "://" in target or target.startswith("mailto:"):
        return True, ""
    if target.startswith("/"):
        # repo-absolute paths are acceptable; absolute filesystem paths are not
        abs_repo = root / target.lstrip("/")
        if abs_repo.exists():
            return True, ""
        return False, f"broken repo-absolute link {target!r} from {base.as_posix()}"
    resolved = (base.parent / target).resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError:
        return False, f"link escapes repo root: {target!r} from {base.as_posix()}"
    if "#" in target:
        path_only = target.split("#", 1)[0]
        if not path_only:
            return True, ""
        resolved = (base.parent / path_only).resolve()
    if resolved.exists():
        return True, ""
    return False, f"broken link {target!r} from {base.as_posix()}"


def find_markdown_files(root: Path) -> List[Path]:
    files: List[Path] = []
    for glob in MARKDOWN_GLOBS:
        files.extend(root.glob(glob))
    # Deduplicate and keep files only
    unique = []
    seen = set()
    for path in files:
        if path.is_file():
            key = path.resolve()
            if key not in seen:
                seen.add(key)
                unique.append(path)
    return sorted(unique)


def main() -> int:
    parser = argparse.ArgumentParser(description="Check Vesper harness docs layout and links.")
    parser.add_argument("--root", default=".", help="Repository root")
    parser.add_argument("--json-out", default="", help="Optional JSON output path")
    parser.add_argument("--markdown-out", default="", help="Optional markdown output path")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    errors: List[str] = []
    warnings: List[str] = []

    for rel in REQUIRED_FILES:
        if not (root / rel).exists():
            errors.append(f"missing required file: {rel}")

    agents_path = root / "AGENTS.md"
    if agents_path.exists():
        agents_lines = read_text(agents_path).splitlines()
        if len(agents_lines) > 160:
            warnings.append(f"AGENTS.md is {len(agents_lines)} lines; keep it short and map-like")
        if "docs/README.md" not in read_text(agents_path):
            errors.append("AGENTS.md does not link to docs/README.md")

    index_path = root / "docs/README.md"
    if index_path.exists():
        index_text = read_text(index_path)
        for needle in [
            "ARCHITECTURE.md",
            "QUALITY_SCORE.md",
            "RELIABILITY.md",
            "SECURITY.md",
            "PLANS.md",
        ]:
            if needle not in index_text:
                errors.append(f"docs/README.md does not reference {needle}")

    for path in find_markdown_files(root):
        content = read_text(path)
        if "/Volumes/" in content:
            warnings.append(f"{path.relative_to(root).as_posix()} contains an absolute filesystem path")
        for link in LINK_RE.findall(content):
            ok, message = resolve_link(path, link, root)
            if not ok:
                errors.append(message)

    report = {
        "root": str(root),
        "status": "pass" if not errors else "fail",
        "errors": errors,
        "warnings": warnings,
        "checked_files": [p.relative_to(root).as_posix() for p in find_markdown_files(root)],
    }

    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    if args.markdown_out:
        lines = [
            "# Harness Docs Report",
            "",
            f"- Status: **{report['status']}**",
            f"- Errors: {len(errors)}",
            f"- Warnings: {len(warnings)}",
            "",
            "## Errors",
        ]
        if errors:
            lines.extend([f"- {item}" for item in errors])
        else:
            lines.append("- none")
        lines.extend(["", "## Warnings"])
        if warnings:
            lines.extend([f"- {item}" for item in warnings])
        else:
            lines.append("- none")
        lines.append("")
        out = Path(args.markdown_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text("\n".join(lines) + "\n", encoding="utf-8")

    for item in errors:
        print(f"ERROR: {item}")
    for item in warnings:
        print(f"WARNING: {item}")

    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
