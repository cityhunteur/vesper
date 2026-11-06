#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Dict, List, Tuple

METHOD_PATH_RE = re.compile(r'\("([A-Z]+)",\s*"([^"]+)"\)\s*=>')

CORE_DOCS = [
    "AGENTS.md",
    "WORKFLOW.md",
    "docs/README.md",
    "ARCHITECTURE.md",
    "docs/QUALITY_SCORE.md",
    "docs/RELIABILITY.md",
    "docs/SECURITY.md",
    "docs/PLANS.md",
    "docs/process/harness-engineering.md",
]

HARNESS_WORKFLOWS = [
    ".github/workflows/harness-docs.yml",
    ".github/workflows/harness-structure.yml",
    ".github/workflows/harness-quality-score.yml",
    ".github/workflows/harness-garbage-collection.yml",
]

HARNESS_SCRIPTS = [
    "scripts/harness/check_docs.py",
    "scripts/harness/check_structure.py",
    "scripts/harness/compute_quality_score.py",
    "scripts/harness/janitor_scan.py",
    "scripts/harness/init_issue_run.sh",
    "scripts/harness/collect_verification_bundle.sh",
]

TEMPLATES = [
    ".symphony/templates/verification-manifest-template.yaml",
    ".symphony/templates/milestone-report-template.md",
    ".symphony/templates/council-rubric.md",
    ".symphony/templates/symphony-milestones-template.md",
]


def file_exists(root: Path, rel: str) -> bool:
    return (root / rel).exists()


def score_fraction(present: int, total: int, weight: int) -> Tuple[int, str]:
    value = round(weight * (present / max(total, 1)))
    return value, f"{present}/{total}"


def compute_route_coverage(root: Path) -> Tuple[int, int, List[str]]:
    api_path = root / "src/api.rs"
    golden_path = root / "testdata/contracts/firecracker-routes.golden.json"
    if not api_path.exists() or not golden_path.exists():
        return 0, 0, []
    golden = json.loads(golden_path.read_text(encoding="utf-8"))
    total = {(item["method"], item["path"]) for item in golden}
    api_text = api_path.read_text(encoding="utf-8")
    handled = set(METHOD_PATH_RE.findall(api_text))
    if (
        'if route.method == "PUT" && route.template == "/actions"' in api_text
        or 'if route_match.template.method == "PUT" && route_match.template.template == "/actions"' in api_text
    ):
        handled.add(("PUT", "/actions"))
    missing = sorted(f"{method} {path}" for method, path in total - handled)
    return len(handled), len(total), missing


def grade(score: int) -> str:
    if score >= 90:
        return "A"
    if score >= 80:
        return "B"
    if score >= 70:
        return "C"
    if score >= 60:
        return "D"
    return "F"


def main() -> int:
    parser = argparse.ArgumentParser(description="Compute a Vesper harness quality score.")
    parser.add_argument("--root", default=".", help="Repository root")
    parser.add_argument("--json-out", default="", help="Optional JSON output path")
    parser.add_argument("--markdown-out", default="", help="Optional markdown output path")
    args = parser.parse_args()

    root = Path(args.root).resolve()

    docs_present = sum(1 for rel in CORE_DOCS if file_exists(root, rel))
    docs_score, docs_ratio = score_fraction(docs_present, len(CORE_DOCS), 20)

    harness_present = sum(1 for rel in HARNESS_WORKFLOWS + HARNESS_SCRIPTS + TEMPLATES if file_exists(root, rel))
    harness_total = len(HARNESS_WORKFLOWS) + len(HARNESS_SCRIPTS) + len(TEMPLATES)
    harness_score, harness_ratio = score_fraction(harness_present, harness_total, 25)

    handled, total, missing = compute_route_coverage(root)
    parity_score = round(25 * (handled / max(total, 1))) if total else 0
    parity_ratio = f"{handled}/{total}"

    api_path = root / "src/api.rs"
    api_lines = api_path.read_text(encoding="utf-8").count("\n") + 1 if api_path.exists() else 0
    if api_lines == 0:
        arch_score = 0
    elif api_lines <= 1200:
        arch_score = 15
    elif api_lines <= 2500:
        arch_score = 11
    elif api_lines <= 3200:
        arch_score = 8
    else:
        arch_score = 4

    truthful_score = 15
    optimistic_workflows = []
    for rel in [
        ".github/workflows/eval-on-review.yml",
        ".github/workflows/promote-implemented-issues.yml",
    ]:
        if file_exists(root, rel):
            optimistic_workflows.append(rel)
    if optimistic_workflows:
        truthful_score -= 5

    total_score = docs_score + harness_score + parity_score + arch_score + truthful_score
    result = {
        "score": total_score,
        "grade": grade(total_score),
        "dimensions": {
            "knowledge_base": {"score": docs_score, "weight": 20, "ratio": docs_ratio},
            "harness_sensors": {"score": harness_score, "weight": 25, "ratio": harness_ratio},
            "parity_surface": {"score": parity_score, "weight": 25, "ratio": parity_ratio},
            "architecture": {"score": arch_score, "weight": 15, "api_lines": api_lines},
            "truthful_delivery": {
                "score": truthful_score,
                "weight": 15,
                "optimistic_workflows": optimistic_workflows,
            },
        },
        "missing_route_behaviors": missing,
    }

    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")

    if args.markdown_out:
        lines = [
            "# Harness Quality Score",
            "",
            f"- Score: **{result['score']} / 100**",
            f"- Grade: **{result['grade']}**",
            "",
            "## Dimensions",
            "",
            "| Dimension | Score | Weight | Signal |",
            "| --- | ---: | ---: | --- |",
            f"| Knowledge base | {docs_score} | 20 | {docs_ratio} core docs present |",
            f"| Harness sensors | {harness_score} | 25 | {harness_ratio} harness assets present |",
            f"| Parity surface | {parity_score} | 25 | {parity_ratio} explicit route behaviors |",
            f"| Architecture | {arch_score} | 15 | `src/api.rs` = {api_lines} lines |",
            f"| Truthful delivery | {truthful_score} | 15 | optimistic workflows: {len(optimistic_workflows)} |",
            "",
            "## Missing route behaviors",
        ]
        if missing:
            lines.extend([f"- {item}" for item in missing])
        else:
            lines.append("- none")
        out = Path(args.markdown_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(f"quality score: {result['score']}/100 ({result['grade']})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
