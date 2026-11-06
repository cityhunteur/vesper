#!/usr/bin/env python3
"""Assemble the System VM reliability health report (GH #16).

Usage:
  reliability_report.py --out REPORT --cycles N --boot-report PATH [--control-report PATH]
      [--docker-report PATH] [--services-report PATH] --leak-before PATH --leak-after PATH
      [--host TEXT] [--commit SHA] [--failure TEXT ...]
  reliability_report.py --self-test

Each input is a JSON file written by the other tools; missing optional inputs are recorded as
`not_run`. The result has `passed` plus a `failures[]` list whose entries name the step, the
reason and the evidence path, so a failure is actionable from the report alone. Standard
library only.
"""
import argparse
import json
import os
import statistics
import sys
import tempfile
import unittest
from datetime import datetime, timezone

DRIFT_FACTOR = 1.5  # last-five mean may not exceed first-five mean by more than this
LEAK_KEYS = ("launcher_processes", "proxy_processes", "sockets", "pid_files")


def load(path):
    if not path:
        return None
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def boot_section(report, cycles):
    failures = []
    if report is None:
        return {"status": "not_run"}, [{"step": "boot_stress", "reason": "no boot report", "evidence": None}]
    boot_ms = report.get("boot_ms") or []
    section = {
        "status": "ok" if report.get("failed", 1) == 0 and report.get("iterations") == cycles else "failed",
        "cycles": report.get("iterations"),
        "successful": report.get("successful"),
        "failed": report.get("failed"),
        "max_boot_ms": report.get("max_boot_ms"),
        "mean_boot_ms": int(statistics.mean(boot_ms)) if boot_ms else None,
        "threshold_ms": report.get("max_boot_allowed_ms"),
        "failure_messages": report.get("failures") or [],
    }
    if len(boot_ms) >= 10:
        first = statistics.mean(boot_ms[:5])
        last = statistics.mean(boot_ms[-5:])
        section["drift_first5_ms"] = int(first)
        section["drift_last5_ms"] = int(last)
        section["drift_ok"] = last <= first * DRIFT_FACTOR
        if not section["drift_ok"]:
            failures.append({"step": "boot_drift", "reason": f"last-five mean {int(last)} ms exceeds first-five mean {int(first)} ms × {DRIFT_FACTOR}", "evidence": "run/systemvm/boot.log"})
    if section["status"] != "ok":
        failures.append({"step": "boot_stress", "reason": f"{report.get('failed')} of {report.get('iterations')} cycles failed (expected {cycles} cycles): {'; '.join(section['failure_messages'][:3])}", "evidence": "run/systemvm/boot.log, run/systemvm/console.log"})
    return section, failures


def suite_section(name, report, evidence):
    if report is None:
        return {"status": "not_run"}, [{"step": name, "reason": "not run", "evidence": evidence}]
    steps = report.get("steps") or []
    failed = [s for s in steps if not s.get("ok")]
    section = {"status": "ok" if report.get("passed") else "failed", "steps": len(steps), "failed_steps": [s.get("name") for s in failed]}
    failures = [{"step": f"{name}.{s.get('name')}", "reason": str(s.get("detail"))[:300], "evidence": evidence} for s in failed]
    if not report.get("passed") and not failed:
        failures.append({"step": name, "reason": "report not marked passed", "evidence": evidence})
    return section, failures


def leak_section(before, after):
    failures = []
    if before is None or after is None:
        return {"status": "not_run"}, [{"step": "leaks", "reason": "missing before/after snapshot", "evidence": None}]
    section = {"status": "ok", "before": before, "after": after}
    for key in LEAK_KEYS:
        if after.get(key, 0) != 0:
            section["status"] = "failed"
            failures.append({"step": f"leaks.{key}", "reason": f"{after.get(key)} left after the run (before: {before.get(key)})", "evidence": after.get(key + "_detail") or "run/"})
    return section, failures


def assemble(args_dict):
    cycles = args_dict["cycles"]
    boot, f1 = boot_section(load(args_dict.get("boot_report")), cycles)
    control, f2 = suite_section("control_suite", load(args_dict.get("control_report")), "run/systemvm/control-report.json")
    docker, f3 = suite_section("docker_smoke", load(args_dict.get("docker_report")), "run/systemvm/docker-smoke-report.json")
    services, f4 = suite_section("services_suite", load(args_dict.get("services_report")), "run/systemvm/services-report.json")
    leaks, f5 = leak_section(load(args_dict.get("leak_before")), load(args_dict.get("leak_after")))
    extra = [{"step": "suite", "reason": text, "evidence": "run/systemvm/reliability.log"} for text in (args_dict.get("failure") or [])]
    failures = f1 + f2 + f3 + f4 + f5 + extra
    return {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "host": args_dict.get("host") or "",
        "commit": args_dict.get("commit") or "",
        "cycles": cycles,
        "boot_stress": boot,
        "control_suite": control,
        "docker_smoke": docker,
        "services_suite": services,
        "leaks": leaks,
        "failures": failures,
        "passed": not failures,
    }


class SelfTest(unittest.TestCase):
    def write(self, name, data):
        path = os.path.join(self.dir, name)
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(data, handle)
        return path

    def setUp(self):
        self.dir = tempfile.mkdtemp()

    def good_inputs(self, cycles=12):
        boot = {"iterations": cycles, "successful": cycles, "failed": 0, "failures": [], "boot_ms": [1000 + i for i in range(cycles)], "max_boot_ms": 1000 + cycles - 1, "max_boot_allowed_ms": 5000}
        suite = {"passed": True, "steps": [{"name": "handshake", "ok": True, "detail": "x"}]}
        leaks = {"launcher_processes": 0, "proxy_processes": 0, "sockets": 0, "pid_files": 0}
        return {
            "cycles": cycles,
            "boot_report": self.write("boot.json", boot),
            "control_report": self.write("control.json", suite),
            "docker_report": self.write("docker.json", suite),
            "services_report": self.write("services.json", suite),
            "leak_before": self.write("before.json", leaks),
            "leak_after": self.write("after.json", leaks),
            "host": "test", "commit": "abc", "failure": [],
        }

    def test_passes_when_everything_is_green(self):
        report = assemble(self.good_inputs())
        self.assertTrue(report["passed"], report["failures"])
        self.assertEqual(report["boot_stress"]["status"], "ok")
        self.assertTrue(report["boot_stress"]["drift_ok"])
        self.assertEqual(report["leaks"]["status"], "ok")

    def test_failures_are_actionable(self):
        inputs = self.good_inputs()
        bad_boot = {"iterations": 12, "successful": 11, "failed": 1, "failures": ["iteration 7: timeout"], "boot_ms": [1000] * 7 + [4000] * 5, "max_boot_ms": 4000, "max_boot_allowed_ms": 5000}
        inputs["boot_report"] = self.write("boot.json", bad_boot)
        inputs["leak_after"] = self.write("after.json", {"launcher_processes": 1, "proxy_processes": 0, "sockets": 2, "pid_files": 0, "sockets_detail": "run/vesper/docker.sock run/systemvm/vsock/3.sock"})
        inputs["docker_report"] = self.write("docker.json", {"passed": False, "steps": [{"name": "docker_info", "ok": False, "detail": "Cannot connect"}]})
        inputs["control_report"] = None
        report = assemble(inputs)
        self.assertFalse(report["passed"])
        steps = {f["step"] for f in report["failures"]}
        self.assertEqual(steps, {"boot_stress", "boot_drift", "docker_smoke.docker_info", "leaks.launcher_processes", "leaks.sockets", "control_suite"})
        leak = next(f for f in report["failures"] if f["step"] == "leaks.sockets")
        self.assertIn("docker.sock", leak["evidence"])
        self.assertEqual(report["control_suite"]["status"], "not_run")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--out")
    parser.add_argument("--cycles", type=int)
    parser.add_argument("--boot-report")
    parser.add_argument("--control-report")
    parser.add_argument("--docker-report")
    parser.add_argument("--services-report")
    parser.add_argument("--leak-before")
    parser.add_argument("--leak-after")
    parser.add_argument("--host", default="")
    parser.add_argument("--commit", default="")
    parser.add_argument("--failure", action="append", default=[])
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(SelfTest)
        result = unittest.TextTestRunner(verbosity=1).run(suite)
        sys.exit(0 if result.wasSuccessful() else 1)
    if not args.out or args.cycles is None:
        parser.error("--out and --cycles are required")
    report = assemble(vars(args))
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)
        handle.write("\n")
    print(f"reliability report: passed={str(report['passed']).lower()} failures={len(report['failures'])} -> {args.out}")
    for failure in report["failures"]:
        print(f"FAIL {failure['step']}: {failure['reason']} (see {failure['evidence']})")
    sys.exit(0 if report["passed"] else 1)


if __name__ == "__main__":
    main()
