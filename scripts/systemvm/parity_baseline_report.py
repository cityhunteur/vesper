#!/usr/bin/env python3
"""Assemble or validate the Backend A parity baseline report (GH #21).

Usage:
  parity_baseline_report.py --out REPORT --suite PATH --lock PATH --archives PATH --registry PATH
      [--metadata PATH] [--host TEXT] [--commit SHA] [--drift-pct N]
  parity_baseline_report.py --validate REPORT --lock PATH --archives PATH --registry PATH
  parity_baseline_report.py --self-test

The suite report is `systemvmctl control parity-suite` output (steps named `conformance:<check>`,
`perf_run_1`, `perf_run_2` with `k=v` tokens in `detail`). The result carries the exact baseline lock
revisions, the Firecracker release and binary version, the compat exception summary (ids, status), the
conformance checks, both performance runs and their comparability, and `passed`. `--validate` re-checks a
committed report against the current lock, archives and registry so CI can publish it as an artifact.
Standard library only.
"""
import argparse
import json
import re
import sys
import tempfile
import unittest
from datetime import datetime, timezone

DEFAULT_DRIFT_PCT = 20.0
PERF_KEYS = ("boot_p50_ms", "boot_p95_ms", "boot_max_ms", "cycle_p50_ms", "create_rate_per_min", "wall_ms", "failures", "n")


def read_lock(path):
    """Minimal parser for `baselines:` in design-v0.baselines.lock.yaml (name -> {repo, revision})."""
    out, current, in_baselines = {}, None, False
    for line in open(path, encoding="utf-8"):
        if re.match(r"^baselines:\s*$", line):
            in_baselines = True
            continue
        if in_baselines and re.match(r"^\S", line):
            in_baselines = False
        if not in_baselines:
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


def read_archives(path):
    """Firecracker release tag and archive digest from systemvm/archives.yaml."""
    name, out = None, {}
    for line in open(path, encoding="utf-8"):
        m = re.match(r"^  - name:\s*(\S+)", line)
        if m:
            name = m.group(1)
            out[name] = {}
            continue
        m = re.match(r"^    (version|url|sha256):\s*(\S+)", line)
        if m and name:
            out[name][m.group(1)] = m.group(2)
    return out


def read_registry(path):
    """Compat exception ids, statuses and issues from docs/specs/compat-exceptions.yaml."""
    exceptions, current = [], None
    for line in open(path, encoding="utf-8"):
        m = re.match(r"^  - id:\s*\"?([^\"\s]+)\"?", line)
        if m:
            current = {"id": m.group(1)}
            exceptions.append(current)
            continue
        m = re.match(r"^    (status|issue|expiry_date|target_release):\s*\"?([^\"\s]+)\"?", line)
        if m and current is not None:
            current[m.group(1)] = m.group(2)
    return exceptions


def tokens(detail):
    out = {}
    for part in detail.split():
        if "=" in part:
            k, v = part.split("=", 1)
            out[k] = v
    return out


def num(value):
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def assemble(suite, lock, archives, registry, metadata=None, host="", commit="", drift_pct=DEFAULT_DRIFT_PCT):
    steps = suite.get("steps", [])
    checks = [{"name": s["name"].split(":", 1)[1], "ok": bool(s["ok"]), "detail": s.get("detail", "")}
              for s in steps if s["name"].startswith("conformance:")]
    summary = next((tokens(s.get("detail", "")) for s in steps if s["name"] == "conformance_summary"), {})
    runs = []
    for s in steps:
        if re.match(r"^perf_run_\d+$", s["name"]):
            t = tokens(s.get("detail", ""))
            runs.append({k: (num(t.get(k)) if k != "n" else int(num(t.get(k)) or 0)) for k in PERF_KEYS} | {"ok": bool(s["ok"])})
    comparable, drift = False, None
    if len(runs) >= 2 and all(r["ok"] and r.get("boot_p50_ms") for r in runs[:2]):
        a, b = runs[0]["boot_p50_ms"], runs[1]["boot_p50_ms"]
        drift = abs(a - b) * 100.0 / max(min(a, b), 1.0)
        comparable = drift <= drift_pct and all((r.get("failures") or 0) == 0 for r in runs[:2])
    fc = archives.get("firecracker", {})
    open_ex = [e for e in registry if e.get("status", "").lower() == "open"]
    report = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "host": {"description": host, "commit": commit},
        "baseline": {
            name: {"repo": entry.get("repo", ""), "revision": entry.get("revision", "")} for name, entry in lock.items()
        },
        "firecracker_binary": {
            "release": fc.get("version", ""),
            "archive_sha256": fc.get("sha256", ""),
            "api_version": summary.get("firecracker_version", ""),
        },
        "parity_metadata": metadata or [],
        "compat_exceptions": {
            "open": len(open_ex),
            "closed": len(registry) - len(open_ex),
            "open_ids": [e["id"] for e in open_ex],
            "entries": registry,
        },
        "conformance": {
            "passed": sum(1 for c in checks if c["ok"]),
            "failed": sum(1 for c in checks if not c["ok"]),
            "checks": checks,
        },
        "performance": {"runs": runs, "drift_pct": drift, "drift_limit_pct": drift_pct, "comparable": comparable},
    }
    report["passed"] = bool(checks) and report["conformance"]["failed"] == 0 and comparable
    return report


def validate(report, lock, archives, registry):
    """Returns a list of problems; empty when the committed report matches the current lock and registry."""
    problems = []
    for name, entry in lock.items():
        got = report.get("baseline", {}).get(name, {}).get("revision")
        if got != entry.get("revision"):
            problems.append(f"baseline.{name}.revision {got!r} != lock {entry.get('revision')!r}")
    fc = archives.get("firecracker", {})
    if report.get("firecracker_binary", {}).get("release") != fc.get("version"):
        problems.append(f"firecracker_binary.release {report.get('firecracker_binary', {}).get('release')!r} != archives {fc.get('version')!r}")
    rel = (fc.get("version") or "").lstrip("v")
    if rel and report.get("firecracker_binary", {}).get("api_version") != rel:
        problems.append(f"firecracker_binary.api_version != release {rel}")
    if report.get("conformance", {}).get("failed", 1) != 0 or not report.get("conformance", {}).get("checks"):
        problems.append("conformance has failures or no checks")
    if not report.get("performance", {}).get("comparable"):
        problems.append("performance runs are not comparable")
    reg_ids = {e["id"]: e for e in registry}
    for cid in report.get("compat_exceptions", {}).get("open_ids", []):
        if cid not in reg_ids:
            problems.append(f"compat exception {cid} missing from registry")
        elif reg_ids[cid].get("status", "").lower() != "open":
            problems.append(f"compat exception {cid} is not open in the registry")
    for e in registry:
        if e.get("status", "").lower() == "open" and e["id"] not in report.get("compat_exceptions", {}).get("open_ids", []):
            problems.append(f"open compat exception {e['id']} is not summarised in the report")
    if not report.get("passed"):
        problems.append("report.passed is false")
    return problems


class Tests(unittest.TestCase):
    LOCK = 'version: 1\nbaselines:\n  firecracker:\n    repo: "a/b"\n    revision: "abc"\n  containerd:\n    repo: "c/d"\n    revision: "def"\nreporting:\n  x: 1\n'
    ARCH = "archives:\n  - name: firecracker\n    version: v1.16.0\n    url: https://x\n    sha256: 11\n"
    REG = 'version: 1\nexceptions:\n  - id: CE-0001\n    owner: o\n    issue: 21\n    expiry_date: "2027-01-01"\n    target_release: ga\n    status: open\n'

    def files(self):
        d = tempfile.mkdtemp()
        paths = {}
        for name, text in (("lock", self.LOCK), ("arch", self.ARCH), ("reg", self.REG)):
            p = f"{d}/{name}.yaml"
            open(p, "w").write(text)
            paths[name] = p
        return paths

    def suite(self, fail=False, p50b=1500):
        return {"steps": [
            {"name": "conformance:version", "ok": True, "detail": "status 200", "elapsed_ms": 1},
            {"name": "conformance:pause", "ok": not fail, "detail": "status 204", "elapsed_ms": 1},
            {"name": "conformance_summary", "ok": True, "detail": "passed=2 failed=0 firecracker_version=1.16.0", "elapsed_ms": 1},
            {"name": "perf_run_1", "ok": True, "detail": "n=3 boot_p50_ms=1500 boot_p95_ms=1520 boot_max_ms=1520 cycle_p50_ms=2400 create_rate_per_min=24.5 wall_ms=7300 failures=0", "elapsed_ms": 1},
            {"name": "perf_run_2", "ok": True, "detail": f"n=3 boot_p50_ms={p50b} boot_p95_ms=1530 boot_max_ms=1530 cycle_p50_ms=2410 create_rate_per_min=24.3 wall_ms=7400 failures=0", "elapsed_ms": 1},
        ]}

    def test_assemble_and_validate(self):
        p = self.files()
        lock, arch, reg = read_lock(p["lock"]), read_archives(p["arch"]), read_registry(p["reg"])
        self.assertEqual(lock["firecracker"]["revision"], "abc")
        self.assertEqual(arch["firecracker"]["version"], "v1.16.0")
        self.assertEqual(reg[0]["id"], "CE-0001")
        r = assemble(self.suite(), lock, arch, reg, host="h", commit="c")
        self.assertTrue(r["passed"])
        self.assertEqual(r["conformance"]["passed"], 2)
        self.assertEqual(r["compat_exceptions"]["open_ids"], ["CE-0001"])
        self.assertEqual(r["firecracker_binary"]["api_version"], "1.16.0")
        self.assertEqual(validate(r, lock, arch, reg), [])

    def test_failures(self):
        p = self.files()
        lock, arch, reg = read_lock(p["lock"]), read_archives(p["arch"]), read_registry(p["reg"])
        self.assertFalse(assemble(self.suite(fail=True), lock, arch, reg)["passed"])
        self.assertFalse(assemble(self.suite(p50b=2000), lock, arch, reg)["performance"]["comparable"])
        r = assemble(self.suite(), lock, arch, reg)
        r["baseline"]["firecracker"]["revision"] = "stale"
        self.assertTrue(any("firecracker.revision" in x for x in validate(r, lock, arch, reg)))
        r = assemble(self.suite(), lock, arch, reg)
        r["compat_exceptions"]["open_ids"] = []
        self.assertTrue(any("not summarised" in x for x in validate(r, lock, arch, reg)))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out")
    ap.add_argument("--suite")
    ap.add_argument("--lock", default="docs/specs/design-v0.baselines.lock.yaml")
    ap.add_argument("--archives", default="systemvm/archives.yaml")
    ap.add_argument("--registry", default="docs/specs/compat-exceptions.yaml")
    ap.add_argument("--metadata")
    ap.add_argument("--host", default="")
    ap.add_argument("--commit", default="")
    ap.add_argument("--drift-pct", type=float, default=DEFAULT_DRIFT_PCT)
    ap.add_argument("--validate")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        sys.argv = sys.argv[:1]
        unittest.main()
        return
    lock, archives, registry = read_lock(args.lock), read_archives(args.archives), read_registry(args.registry)
    if args.validate:
        problems = validate(json.load(open(args.validate)), lock, archives, registry)
        for p in problems:
            print(f"FAIL {p}")
        print("parity baseline report valid" if not problems else f"parity baseline report invalid ({len(problems)} problem(s))")
        sys.exit(1 if problems else 0)
    if not (args.out and args.suite):
        ap.error("--out and --suite are required unless --validate or --self-test")
    metadata = json.load(open(args.metadata)).get("metadata") if args.metadata else None
    report = assemble(json.load(open(args.suite)), lock, archives, registry, metadata, args.host, args.commit, args.drift_pct)
    json.dump(report, open(args.out, "w"), indent=1)
    open(args.out, "a").write("\n")
    for c in report["conformance"]["checks"]:
        if not c["ok"]:
            print(f"FAIL conformance:{c['name']}: {c['detail']}")
    if not report["performance"]["comparable"]:
        print(f"FAIL performance: runs not comparable (drift {report['performance']['drift_pct']})")
    print(("parity baseline passed" if report["passed"] else "parity baseline FAILED") + f" ({args.out})")
    sys.exit(0 if report["passed"] else 1)


if __name__ == "__main__":
    main()
