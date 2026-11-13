#!/usr/bin/env python3
"""Turn pytest-json-report files into the results file `baselinectl --mode upstream-compare` reads (GH #172).

Usage: python3 -I collect.py OUT.json --collected COLLECTED.txt [--log PYTEST.log] [REPORT.json ...]

OUT.json is {nodeid: {"outcome": ..., "message": ...}}. The outcome is the worst phase: a failure in setup or teardown
is "error". A later report overrides an earlier one for the same test (a rerun). Every node id in COLLECTED.txt (one
per line, from `pytest --collect-only -q`) that no report mentions is "hung": the run was stopped while it ran or
before it started, which the comparator must explain like a failure. When the run was stopped, pytest-json-report
never wrote its file, so `--log` recovers each finished test from pytest-xdist's progress lines
(`[gw0] [ 3%] PASSED nodeid`), keeping the worst outcome a test logged (a teardown ERROR after a PASSED).
"""
import re
import json
import sys


def phase_message(p):
    crash = p.get("crash") or {}
    lines = [crash.get("message") or ""]
    longrepr = p.get("longrepr") or ""
    if not isinstance(longrepr, str):
        longrepr = str(longrepr)
    lines += [l.strip()[2:].strip() for l in longrepr.splitlines() if l.strip().startswith("E ")][:20]
    if not any(lines):
        lines.append(longrepr.strip().splitlines()[-1] if longrepr.strip() else "")
    return " | ".join(" ".join(l.split()) for l in lines if l)[:2000]


def result(test):
    outcome, message = test["outcome"], ""
    for name in ("setup", "call", "teardown"):
        p = test.get(name) or {}
        if p.get("outcome") == "failed":
            if name != "call":
                outcome = "error"
            message = message or phase_message(p)
        elif p.get("outcome") == "skipped" and not message:
            message = phase_message(p)
    if outcome not in ("passed", "failed", "error", "skipped", "xfailed", "xpassed"):
        outcome = "error"
    return {"outcome": outcome, "message": message}


PROGRESS = re.compile(r"^\[gw\d+\] \[\s*\d+%\] (PASSED|FAILED|ERROR|SKIPPED|XFAIL|XPASS) (\S+)")
FROM_LOG = {"PASSED": "passed", "XFAIL": "xfailed", "XPASS": "xpassed", "SKIPPED": "skipped", "FAILED": "failed", "ERROR": "error"}
WORST = ["passed", "xfailed", "xpassed", "skipped", "failed", "error"]


def from_log(path):
    found = {}
    with open(path, errors="replace") as f:
        for line in f:
            m = PROGRESS.match(line)
            if m:
                outcome, nodeid = FROM_LOG[m.group(1)], m.group(2)
                if nodeid not in found or WORST.index(outcome) > WORST.index(found[nodeid]):
                    found[nodeid] = outcome
    return {n: {"outcome": o, "message": "from the progress log; the JSON report was not written"} for n, o in found.items()}


def main(argv):
    if len(argv) < 3 or argv[1] != "--collected":
        sys.exit(__doc__)
    out, collected_path, rest = argv[0], argv[2], argv[3:]
    log = None
    if rest[:1] == ["--log"]:
        log, rest = rest[1], rest[2:]
    reports = rest
    results = {}
    if log:
        results.update(from_log(log))
    for path in reports:
        with open(path) as f:
            for test in json.load(f).get("tests", []):
                results[test["nodeid"]] = result(test)
    with open(collected_path) as f:
        collected = [l.strip() for l in f if "::" in l and not l.startswith(("=", " "))]
    hung = [n for n in collected if n not in results]
    for nodeid in hung:
        results[nodeid] = {"outcome": "hung", "message": "no result: the run stopped while this test ran or before it started"}
    with open(out, "w") as f:
        json.dump(dict(sorted(results.items())), f, indent=1)
        f.write("\n")
    counts = {}
    for r in results.values():
        counts[r["outcome"]] = counts.get(r["outcome"], 0) + 1
    print(f"collect: {len(results)} tests ({len(collected)} collected): {dict(sorted(counts.items()))}")


if __name__ == "__main__":
    main(sys.argv[1:])
