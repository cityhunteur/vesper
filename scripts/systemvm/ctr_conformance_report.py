#!/usr/bin/env python3
"""Assemble or validate the ctr conformance report for the Vesper shim (GH #26).

Usage:
  ctr_conformance_report.py --out REPORT --shim PATH --bundle PATH --restart PATH --lock PATH [--metadata PATH]
      [--host TEXT] [--commit SHA]
  ctr_conformance_report.py --validate REPORT --lock PATH
  ctr_conformance_report.py --self-test

The three inputs are `systemvmctl control shim-suite|bundle-suite|restart-suite` reports. The result carries
the baseline lock revisions, the `baselinectl` metadata, every step grouped by suite, the counts, the list of
checklist items covered, and `passed`. `--validate` re-checks a committed report against the current lock so
CI can publish it as an artifact. Standard library only.
"""
import argparse
import json
import re
import sys
import tempfile
import unittest
from datetime import datetime, timezone

CHECKLIST = {
    "ctr run creates and starts container": ("shim", "e2e_run_exit_code_and_output"),
    "ctr kill and ctr metrics return expected data": ("shim", "task_metrics"),
    "ctr kill tears the task down": ("shim", "kill_detached"),
    "restart resilience": ("restart", "task_survives_restart"),
    "block-first rootfs": ("bundle", "layered_rootfs_boots"),
}
OPEN_GAPS = ["ctr exec attaches to running container (guest agent, WOR-32)", "CI boots the System VM (TD-005)"]


def read_lock(path):
    out, current, in_b = {}, None, False
    for line in open(path, encoding="utf-8"):
        if re.match(r"^baselines:\s*$", line):
            in_b = True
            continue
        if in_b and re.match(r"^\S", line):
            in_b = False
        if not in_b:
            continue
        m = re.match(r"^  ([A-Za-z0-9_]+):\s*$", line)
        if m:
            current = m.group(1)
            out[current] = {}
            continue
        m = re.match(r'^    (repo|revision):\s*"([^"]*)"', line)
        if m and current:
            out[current][m.group(1)] = m.group(2)
    return out


def assemble(suites, lock, metadata=None, host="", commit=""):
    report = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "host": {"description": host, "commit": commit},
        "baseline": {k: {"repo": v.get("repo", ""), "revision": v.get("revision", "")} for k, v in lock.items()},
        "parity_metadata": metadata or [],
        "suites": {},
        "checklist": {},
        "open_gaps": OPEN_GAPS,
    }
    total = failed = 0
    for name, suite in suites.items():
        steps = suite.get("steps", [])
        report["suites"][name] = {
            "passed": bool(suite.get("passed")),
            "steps": [{"name": s["name"], "ok": bool(s["ok"]), "detail": s.get("detail", "")} for s in steps],
        }
        total += len(steps)
        failed += sum(1 for s in steps if not s["ok"])
    for item, (suite, step) in CHECKLIST.items():
        steps = {s["name"]: s["ok"] for s in report["suites"].get(suite, {}).get("steps", [])}
        report["checklist"][item] = bool(steps.get(step, False))
    report["steps_total"], report["steps_failed"] = total, failed
    report["passed"] = total > 0 and failed == 0 and all(s["passed"] for s in report["suites"].values()) and all(report["checklist"].values())
    return report


def validate(report, lock):
    problems = []
    for name, entry in lock.items():
        got = report.get("baseline", {}).get(name, {}).get("revision")
        if got != entry.get("revision"):
            problems.append(f"baseline.{name}.revision {got!r} != lock {entry.get('revision')!r}")
    for suite in ("shim", "bundle", "restart"):
        if not report.get("suites", {}).get(suite, {}).get("passed"):
            problems.append(f"suite {suite} missing or failed")
    for item, ok in report.get("checklist", {}).items():
        if not ok:
            problems.append(f"checklist item not met: {item}")
    if report.get("steps_failed", 1) != 0 or not report.get("passed"):
        problems.append("report has failed steps or passed is false")
    return problems


class Tests(unittest.TestCase):
    LOCK = 'version: 1\nbaselines:\n  firecracker:\n    repo: "a/b"\n    revision: "abc"\n'

    def suites(self, fail=False):
        mk = lambda names: {"passed": not fail, "steps": [{"name": n, "ok": not fail, "detail": ""} for n in names]}
        return {"shim": mk(["e2e_run_exit_code_and_output", "task_metrics", "kill_detached"]),
                "bundle": mk(["layered_rootfs_boots"]), "restart": mk(["task_survives_restart"])}

    def test_round_trip(self):
        d = tempfile.mkdtemp(); p = f"{d}/lock.yaml"; open(p, "w").write(self.LOCK)
        lock = read_lock(p)
        r = assemble(self.suites(), lock, host="h", commit="c")
        self.assertTrue(r["passed"]); self.assertEqual(r["steps_total"], 5); self.assertTrue(all(r["checklist"].values()))
        self.assertEqual(validate(r, lock), [])
        r["baseline"]["firecracker"]["revision"] = "stale"
        self.assertTrue(any("revision" in x for x in validate(r, lock)))
        bad = assemble(self.suites(fail=True), lock)
        self.assertFalse(bad["passed"]); self.assertTrue(any("checklist" in x for x in validate(bad, lock)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out"); ap.add_argument("--shim"); ap.add_argument("--bundle"); ap.add_argument("--restart")
    ap.add_argument("--lock", default="docs/specs/design-v0.baselines.lock.yaml"); ap.add_argument("--metadata")
    ap.add_argument("--host", default=""); ap.add_argument("--commit", default="")
    ap.add_argument("--validate"); ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        sys.argv = sys.argv[:1]; unittest.main(); return
    lock = read_lock(a.lock)
    if a.validate:
        problems = validate(json.load(open(a.validate)), lock)
        for p in problems: print(f"FAIL {p}")
        print("ctr conformance report valid" if not problems else f"ctr conformance report invalid ({len(problems)} problem(s))")
        sys.exit(1 if problems else 0)
    if not (a.out and a.shim and a.bundle and a.restart):
        ap.error("--out, --shim, --bundle and --restart are required unless --validate or --self-test")
    suites = {"shim": json.load(open(a.shim)), "bundle": json.load(open(a.bundle)), "restart": json.load(open(a.restart))}
    metadata = json.load(open(a.metadata)).get("metadata") if a.metadata else None
    r = assemble(suites, lock, metadata, a.host, a.commit)
    json.dump(r, open(a.out, "w"), indent=1); open(a.out, "a").write("\n")
    for s, v in r["suites"].items():
        for st in v["steps"]:
            if not st["ok"]: print(f"FAIL {s}:{st['name']}: {st['detail']}")
    print(("ctr conformance passed" if r["passed"] else "ctr conformance FAILED") + f" ({r['steps_total']} steps, {a.out})")
    sys.exit(0 if r["passed"] else 1)


if __name__ == "__main__":
    main()
