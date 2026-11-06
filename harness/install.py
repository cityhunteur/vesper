#!/usr/bin/env python3
"""Install the harness knowledge-base template into another repository.

Copies the template files that are missing from the target and never overwrites
existing ones. Files that already exist are reported so an agent can reconcile
them (see harness/prompts/bootstrap.md).

Usage:
    python3 harness/install.py /path/to/repo [--dry-run]
"""

from __future__ import annotations

import argparse
import shutil
import sys
from pathlib import Path

TEMPLATE_ROOT = Path(__file__).resolve().parent.parent

# Template payload, relative to the template root. Directories are copied recursively.
PAYLOAD = [
    "AGENTS.md",
    "CLAUDE.md",
    "ARCHITECTURE.md",
    "docs",
    "harness",
    ".github/workflows/docs.yml",
]

IGNORED = {"__pycache__", ".DS_Store"}


def payload_files() -> list[Path]:
    files = []
    for entry in PAYLOAD:
        path = TEMPLATE_ROOT / entry
        if path.is_dir():
            files += [p for p in sorted(path.rglob("*")) if p.is_file() and not IGNORED & set(p.parts)]
        elif path.is_file():
            files.append(path)
    return [p.relative_to(TEMPLATE_ROOT) for p in files]


def install(target: Path, dry_run: bool = False) -> tuple[list[Path], list[Path]]:
    copied, skipped = [], []
    for rel in payload_files():
        dest = target / rel
        if dest.exists():
            skipped.append(rel)
            continue
        copied.append(rel)
        if not dry_run:
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(TEMPLATE_ROOT / rel, dest)
    return copied, skipped


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("target", type=Path, help="repository to install into")
    parser.add_argument("--dry-run", action="store_true", help="report without copying")
    args = parser.parse_args(argv)

    target = args.target.resolve()
    if not target.is_dir():
        print(f"install: not a directory: {target}", file=sys.stderr)
        return 2
    if target == TEMPLATE_ROOT:
        print("install: target is the template itself", file=sys.stderr)
        return 2

    copied, skipped = install(target, args.dry_run)
    verb = "would copy" if args.dry_run else "copied"
    for rel in copied:
        print(f"{verb}: {rel.as_posix()}")
    for rel in skipped:
        print(f"exists, skipped: {rel.as_posix()}")
    print(f"install: {len(copied)} {verb}, {len(skipped)} skipped", file=sys.stderr)
    if skipped:
        print("install: reconcile skipped files by hand or with harness/prompts/bootstrap.md", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
