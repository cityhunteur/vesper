#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re

# An early bypass before the dispatch match, as PUT /actions and PATCH /vm have (TD-003, TD-022).
BYPASS_RE = re.compile(r'route_match\.template\.method == "([A-Z]+)" && route_match\.template\.template == "([^"]+)"')
from pathlib import Path
from typing import Dict, List, Set, Tuple

LOCAL_REF_RE = re.compile(r"(?:crate|vesper)::([a-z_]+)")
METHOD_PATH_RE = re.compile(r'\("([A-Z]+)",\s*"([^"]+)"\)\s*=>')

RULES: Dict[str, Set[str]] = {
    "src/baseline.rs": set(),
    "src/baseline/upstream.rs": set(),
    "src/baseline/upstream/tests.rs": set(),
    "src/buildinfo.rs": set(),
    "src/compat.rs": set(),
    "src/platform.rs": set(),
    "src/spec.rs": set(),
    "src/api.rs": {"api_matrix", "baseline", "buildinfo", "compat", "platform", "spec"},
    "src/api_matrix.rs": {"baseline", "spec"},
    "src/api_behavior_matrix.rs": {"api_matrix", "baseline", "spec"},
    "src/api_behavior_matrix/validate.rs": {"api_matrix", "baseline", "spec"},
    "src/api_replay.rs": {"api_behavior_matrix"},
    "src/api_replay/tests.rs": set(),
    "src/api/actions.rs": set(),
    "src/api/balloon.rs": set(),
    "src/api/boot_source.rs": set(),
    "src/api/errors.rs": set(),
    "src/api/logger_metrics.rs": set(),
    "src/api/machine_config.rs": set(),
    "src/api/mmds.rs": set(),
    "src/api/pmem.rs": set(),
    "src/api/vm_config.rs": set(),
    "src/api/tests/mod.rs": set(),
    "src/api/tests/envelopes.rs": set(),
    "src/api/tests/lifecycle.rs": set(),
    "src/api/tests/properties.rs": set(),
    "src/api/tests/balloon.rs": set(),
    "src/api/tests/balloon_reporting.rs": set(),
    "src/api/tests/memory_hotplug.rs": set(),
    "src/api/tests/mmds.rs": set(),
    "src/api/tests/device_config.rs": set(),
    "src/api/tests/vsock.rs": set(),
    "src/api/tests/drives.rs": set(),
    "src/api/tests/pmem.rs": set(),
    "src/api/tests/network.rs": set(),
    "src/api/tests/snapshot.rs": set(),
    "src/control.rs": set(),
    "src/control/suites.rs": set(),
    "src/control/suites/bundle.rs": set(),
    "src/control/suites/exec.rs": set(),
    "src/control/suites/restart.rs": set(),
    "src/control/suites/launchapi.rs": set(),
    "src/control/launchapi.rs": set(),
    "src/control/suites/vesperd.rs": set(),
    "src/control/suites/workload.rs": set(),
    "src/control/suites/shim.rs": set(),
    "src/control/suites/rootfs.rs": set(),
    "src/control/suites/snapshot.rs": set(),
    "src/control/suites/snapshot_bounds.rs": {"control"},
    "src/control/suites/replay.rs": set(),
    "src/control/suites/egress.rs": set(),
    "src/control/suites/bounds.rs": set(),
    "src/control/suites/sharenet.rs": set(),
    "src/control/stage.rs": set(),
    "src/proxy.rs": {"buildinfo", "platform"},
    "src/proxy/relay.rs": set(),
    "src/ports.rs": set(),
    "src/ports/udp.rs": set(),
    "src/systemvm.rs": {"buildinfo", "platform"},
    "src/systemvm/boot_check.rs": set(),
    # The two below name their parent as `crate::systemvm` in their tests.
    "src/systemvm/bridge.rs": {"systemvm"},
    "src/systemvm/data_disk.rs": {"systemvm"},
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


GROUP_RE = re.compile(r"(?:crate|vesper)::\{")
SUPER_RE = re.compile(r"\b((?:super::)+)(\{|[a-z_][a-z0-9_]*)")
IDENT_RE = re.compile(r"[a-z_][a-z0-9_]*")


def group_items(text: str, start: int) -> List[str]:
    """The first identifier of each top-level item of the `{...}` group that opens at text[start] (TD-018)."""
    depth, item, items = 0, "", []
    for ch in text[start:]:
        if ch == "{":
            depth += 1
            if depth == 1:
                continue
        elif ch == "}":
            depth -= 1
            if depth == 0:
                items.append(item)
                break
        if depth == 1 and ch == ",":
            items.append(item)
            item = ""
        elif depth >= 1:
            item += ch
    out = []
    for raw in items:
        m = IDENT_RE.match(raw.strip())
        if m and m.group(0) not in ("self", "super", "crate"):
            out.append(m.group(0))
    return out


def module_path(rel: str) -> List[str]:
    """`src/api/tests/mod.rs` -> ["api", "tests"]; `src/api.rs` -> ["api"]; `src/lib.rs` -> []."""
    parts = rel[len("src/"):-len(".rs")].split("/")
    if parts[-1] in ("mod", "lib"):
        parts = parts[:-1]
    return parts


INLINE_MOD_RE = re.compile(r"^\s*(?:pub(?:\([a-z]+\))? )?mod [a-z_][a-z0-9_]* \{", re.M)


def inline_depth(text: str, pos: int) -> int:
    """How many inline `mod name { ... }` blocks are open at pos (a `super::` there climbs out of them first)."""
    depth = 0
    for m in INLINE_MOD_RE.finditer(text, 0, pos):
        opened, i = 0, m.end() - 1
        while i < pos:
            if text[i] == "{":
                opened += 1
            elif text[i] == "}":
                opened -= 1
                if opened == 0:
                    break
            i += 1
        if opened > 0:
            depth += 1
    return depth


def local_refs(text: str, rel: str = "") -> Set[str]:
    refs = set(LOCAL_REF_RE.findall(text))
    for m in GROUP_RE.finditer(text):
        refs.update(group_items(text, m.end() - 1))
    if rel:
        mod = module_path(rel)
        for m in SUPER_RE.finditer(text):
            up = m.group(1).count("super::")
            if len(mod) + inline_depth(text, m.start()) - up != 0:
                continue  # stays inside this file's own top-level module
            if m.group(2) == "{":
                refs.update(group_items(text, m.end() - 1))
            else:
                refs.add(m.group(2))
    return refs


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

    # Every library file has a RULES row; a new one is an error until it gets one (TD-018). Binaries may use any
    # library layer (ARCHITECTURE.md, layer 4), so src/bin/ is not listed.
    for path in sorted((root / "src").rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        if not rel.startswith("src/bin/") and rel not in RULES:
            errors.append(f"{rel} has no RULES row (add one with its allowed local references)")

    for rel, allowed in RULES.items():
        path = root / rel
        if not path.exists():
            warnings.append(f"expected structural file missing: {rel}")
            continue

        text = read_text(path)
        refs = local_refs(text, rel)
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
            handled.update(BYPASS_RE.findall(api_text))
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
