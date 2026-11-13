#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Dict, List, Set, Tuple

LOCAL_REF_RE = re.compile(r"(?:crate|vesper)::([a-z_]+)")
METHOD_PATH_RE = re.compile(r'\("([A-Z]+)",\s*"([^"]+)"\)\s*=>')

RULES: Dict[str, Set[str]] = {
    "src/baseline.rs": set(),
    "src/buildinfo.rs": set(),
    "src/compat.rs": set(),
    "src/platform.rs": set(),
    "src/spec.rs": set(),
    "src/api.rs": {"api_matrix", "baseline", "buildinfo", "compat", "platform", "spec"},
    "src/api_matrix.rs": {"api", "baseline", "spec"},
    "src/api_behavior_matrix.rs": {"api", "api_matrix", "baseline", "spec"},
    "src/api_replay.rs": {"api_behavior_matrix"},
    "src/api/actions.rs": set(),
    "src/api/boot_source.rs": set(),
    "src/api/logger_metrics.rs": set(),
    "src/api/machine_config.rs": set(),
    "src/control.rs": set(),
    "src/control/suites.rs": set(),
    "src/control/suites/bundle.rs": set(),
    "src/control/suites/exec.rs": set(),
    "src/control/suites/restart.rs": set(),
    "src/control/suites/launchapi.rs": set(),
    "src/control/launchapi.rs": set(),
    "src/control/suites/vesperd.rs": set(),
    "src/control/suites/snapshot.rs": set(),
    "src/control/suites/replay.rs": set(),
    "src/control/suites/egress.rs": set(),
    "src/control/stage.rs": set(),
    "src/proxy.rs": {"buildinfo", "platform"},
    "src/proxy/relay.rs": set(),
    "src/ports.rs": set(),
    "src/systemvm.rs": {"buildinfo", "platform"},
    "src/systemvm/image_lock.rs": set(),
    "src/systemvm/packages.rs": set(),
    "src/lib.rs": set(),
}

WARN_BUDGETS = {
    "src/api.rs": 2500,
}
ERROR_BUDGETS = {
    "src/api.rs": 3200,
}
DEFAULT_WARN_BUDGET = 800
DEFAULT_ERROR_BUDGET = 1200


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def line_count(path: Path) -> int:
    return read_text(path).count("\n") + 1


def local_refs(text: str) -> Set[str]:
    return set(LOCAL_REF_RE.findall(text))


def main() -> int:
    parser = argparse.ArgumentParser(description="Check Vesper structure rules and file budgets.")
    parser.add_argument("--root", default=".", help="Repository root")
    parser.add_argument("--json-out", default="", help="Optional JSON output path")
    parser.add_argument("--markdown-out", default="", help="Optional markdown output path")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    errors: List[str] = []
    warnings: List[str] = []
    line_sizes: Dict[str, int] = {}

    for rel, allowed in RULES.items():
        path = root / rel
        if not path.exists():
            warnings.append(f"expected structural file missing: {rel}")
            continue

        text = read_text(path)
        refs = local_refs(text)
        disallowed = sorted(ref for ref in refs if ref not in allowed)
        if rel == "src/lib.rs":
            # lib.rs is just module declarations in the current repo
            disallowed = []
        if disallowed:
            errors.append(f"{rel} has disallowed local references: {', '.join(disallowed)}")

        lines = line_count(path)
        line_sizes[rel] = lines
        warn_budget = WARN_BUDGETS.get(rel, DEFAULT_WARN_BUDGET)
        error_budget = ERROR_BUDGETS.get(rel, DEFAULT_ERROR_BUDGET)
        if lines > error_budget:
            errors.append(f"{rel} is {lines} lines (error budget {error_budget})")
        elif lines > warn_budget:
            warnings.append(f"{rel} is {lines} lines (warn budget {warn_budget})")

    # Report on explicit route handling density to keep the hotspot visible
    api_path = root / "src/api.rs"
    golden_path = root / "testdata/contracts/firecracker-routes.golden.json"
    if api_path.exists() and golden_path.exists():
        try:
            golden = json.loads(golden_path.read_text(encoding="utf-8"))
            total = {(item["method"], item["path"]) for item in golden}
            handled = set(METHOD_PATH_RE.findall(api_path.read_text(encoding="utf-8")))
            api_text = api_path.read_text(encoding="utf-8")
            if (
                'if route.method == "PUT" && route.template == "/actions"' in api_text
                or 'if route_match.template.method == "PUT" && route_match.template.template == "/actions"' in api_text
            ):
                handled.add(("PUT", "/actions"))
            coverage = len(handled) / max(len(total), 1)
            if coverage < 1.0:
                warnings.append(
                    f"explicit route behavior coverage is {len(handled)}/{len(total)} ({coverage:.0%}); missing routes remain"
                )
        except Exception as exc:  # pragma: no cover - defensive
            warnings.append(f"could not compute route coverage summary: {exc}")

    report = {
        "root": str(root),
        "status": "pass" if not errors else "fail",
        "errors": errors,
        "warnings": warnings,
        "line_sizes": line_sizes,
    }

    if args.json_out:
        out = Path(args.json_out)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    if args.markdown_out:
        lines = [
            "# Harness Structure Report",
            "",
            f"- Status: **{report['status']}**",
            f"- Errors: {len(errors)}",
            f"- Warnings: {len(warnings)}",
            "",
            "## Errors",
        ]
        lines.extend([f"- {item}" for item in errors] or ["- none"])
        lines.extend(["", "## Warnings"])
        lines.extend([f"- {item}" for item in warnings] or ["- none"])
        lines.extend(["", "## Line sizes"])
        for rel, count in sorted(line_sizes.items()):
            lines.append(f"- `{rel}`: {count} lines")
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
