#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import List, Tuple

LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
METHOD_PATH_RE = re.compile(r'\("([A-Z]+)",\s*"([^"]+)"\)\s*=>')


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def broken_doc_links(root: Path) -> List[str]:
    issues: List[str] = []
    markdown_files = [p for p in root.glob("docs/**/*.md")] + [
        root / "AGENTS.md",
        root / "ARCHITECTURE.md",
        root / "WORKFLOW.md",
    ]
    for path in markdown_files:
        if not path.exists():
            continue
        content = read_text(path)
        if "/Volumes/" in content:
            issues.append(f"warning: {path.relative_to(root).as_posix()} contains an absolute filesystem path")
        for link in LINK_RE.findall(content):
            if "://" in link or link.startswith("#") or link.startswith("mailto:"):
                continue
            target = link.split("#", 1)[0]
            if not target:
                continue
            if target.startswith("/"):
                candidate = root / target.lstrip("/")
            else:
                candidate = (path.parent / target).resolve()
            if not candidate.exists():
                issues.append(f"error: broken link {link!r} in {path.relative_to(root).as_posix()}")
    return issues


def missing_route_behaviors(root: Path) -> List[str]:
    api = root / "src/api.rs"
    golden = root / "testdata/contracts/firecracker-routes.golden.json"
    if not api.exists() or not golden.exists():
        return []
    golden_routes = json.loads(golden.read_text(encoding="utf-8"))
    total = {(item["method"], item["path"]) for item in golden_routes}
    api_text = api.read_text(encoding="utf-8")
    handled = set(METHOD_PATH_RE.findall(api_text))
    if 'if route.method == "PUT" && route.template == "/actions"' in api_text:
        handled.add(("PUT", "/actions"))
    return [f"warning: missing explicit route behavior for {method} {path}" for method, path in sorted(total - handled)]


def oversized_files(root: Path) -> List[str]:
    issues: List[str] = []
    budgets = {
        "src/api.rs": 2500,
    }
    for path in sorted(root.glob("src/*.rs")):
        limit = budgets.get(path.relative_to(root).as_posix(), 1200)
        lines = path.read_text(encoding="utf-8").count("\n") + 1
        if lines > limit:
            severity = "warning" if path.name == "api.rs" else "error"
            issues.append(f"{severity}: {path.relative_to(root).as_posix()} is {lines} lines (budget {limit})")
    return issues


def optimistic_automation(root: Path) -> List[str]:
    issues: List[str] = []
    for rel in [
        ".github/workflows/eval-on-review.yml",
        ".github/workflows/promote-implemented-issues.yml",
    ]:
        if (root / rel).exists():
            issues.append(f"warning: optimistic workflow still present: {rel}")
    return issues


def classify(messages: List[str]) -> Tuple[List[str], List[str]]:
    errors = [m for m in messages if m.startswith("error:")]
    warnings = [m for m in messages if m.startswith("warning:")]
    return errors, warnings


def main() -> int:
    parser = argparse.ArgumentParser(description="Run janitor scans for Vesper harness drift.")
    parser.add_argument("--root", default=".", help="Repository root")
    parser.add_argument("--fail-on", default="errors", choices=["never", "errors", "warnings"])
    parser.add_argument("--json-out", default="", help="Optional JSON output path")
    parser.add_argument("--markdown-out", default="", help="Optional markdown output path")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    messages = (
        broken_doc_links(root)
        + missing_route_behaviors(root)
        + oversized_files(root)
        + optimistic_automation(root)
    )
    errors, warnings = classify(messages)

    report = {
        "status": "pass" if not errors else "fail",
        "fail_on": args.fail_on,
        "errors": errors,
        "warnings": warnings,
    }

    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    if args.markdown_out:
        lines = [
            "# Harness Janitor Scan",
            "",
            f"- Errors: {len(errors)}",
            f"- Warnings: {len(warnings)}",
            "",
            "## Errors",
        ]
        lines.extend([f"- {item}" for item in errors] or ["- none"])
        lines.extend(["", "## Warnings"])
        lines.extend([f"- {item}" for item in warnings] or ["- none"])
        out = Path(args.markdown_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text("\n".join(lines) + "\n", encoding="utf-8")

    for item in errors:
        print(item)
    for item in warnings:
        print(item)

    if args.fail_on == "never":
        return 0
    if args.fail_on == "warnings" and (errors or warnings):
        return 1
    if args.fail_on == "errors" and errors:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
