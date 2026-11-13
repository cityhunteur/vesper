#!/usr/bin/env python3
"""Turn pytest-json-report files into the results file `baselinectl --mode upstream-compare` reads (GH #172).

Usage: python3 -I collect.py OUT.json --collected COLLECTED.txt [--meta META.json] [--exitcode N] [--log PYTEST.log]
           [REPORT.json ...]
       python3 -I collect.py --self-test

OUT.json is {nodeid: {"outcome": ..., "message": ...}}. The outcome is the worst phase: a failure in setup or teardown
is "error". A later report overrides an earlier one for the same test (a rerun). Every node id in COLLECTED.txt (one
per line, from `pytest --collect-only -q`) that no report mentions is "hung": the run was stopped while it ran or
before it started, which the comparator must explain like a failure. When the run was stopped, pytest-json-report
never wrote its file, so `--log` recovers each finished test from pytest-xdist's progress lines
(`[gw0] [ 3%] PASSED nodeid`), keeping the worst outcome a test logged (a teardown ERROR after a PASSED).

The message is the primary exception only (pytest's crash message, else the first `E` line), so a chained exception
cannot lend a test another class's symptom; a failure's message wins over a skip's. META.json gets pytest's exit code
and the modules that failed to collect, which `baselinectl --mode upstream-compare --meta` turns into failures.
"""
import re
import json
import sys


MAX_MESSAGE = 16000
CHAIN = ("During handling of the above exception", "The above exception was the direct cause")


def phase_message(p):
    crash = p.get("crash") or {}
    longrepr = p.get("longrepr") or ""
    if not isinstance(longrepr, str):
        longrepr = str(longrepr)
    first = crash.get("message") or ""
    # pytest's crash message is the last exception of a chain; the symptom is the first one, before "During handling
    # of the above exception" or "The above exception was the direct cause".
    chain = [m for m in CHAIN if m in longrepr]
    if chain:
        head = longrepr.split(min(chain, key=longrepr.index), 1)[0]
        e_lines = [l.strip()[2:].strip() for l in head.splitlines() if l.strip().startswith("E ")]
        if e_lines:
            first = " ".join(e_lines)
    if not first:
        e_lines = [l.strip()[2:].strip() for l in longrepr.splitlines() if l.strip().startswith("E ")]
        first = e_lines[0] if e_lines else (longrepr.strip().splitlines()[-1] if longrepr.strip() else "")
    msg = " ".join(first.split())
    # The comparator refuses a vulnerability class on a shortened message, so the marker must stay last.
    return msg if len(msg) <= MAX_MESSAGE else msg[:MAX_MESSAGE] + " [truncated]"


def result(test):
    outcome, failure, skip = test["outcome"], "", ""
    # pytest-xdist reports a worker that died mid-test (a --timeout-method=thread timeout, an OOM kill) as a failed
    # phase named "???"; its longrepr names the crash, so it is read after the usual three, like a call failure.
    extra = [k for k, v in test.items() if isinstance(v, dict) and "outcome" in v and k not in ("setup", "call", "teardown")]
    for name in ("setup", "call", "teardown", *extra):
        p = test.get(name) or {}
        if p.get("outcome") == "failed":
            if name in ("setup", "teardown"):
                outcome = "error"
            elif outcome not in ("failed", "error"):
                outcome = "failed"
            failure = failure or phase_message(p)
        elif p.get("outcome") == "skipped":
            skip = skip or phase_message(p)
    if outcome not in ("passed", "failed", "error", "skipped", "xfailed", "xpassed"):
        outcome = "error"
    return {"outcome": outcome, "message": failure if outcome in ("failed", "error") else (skip or failure)}


def meta(report):
    errors = [c.get("nodeid") or "?" for c in report.get("collectors", []) if c.get("outcome") == "failed"]
    return {"exitcode": report.get("exitcode"), "collection_errors": errors}


# A node id can contain spaces (`...-1000.0 KB/sec]`), so it runs to the end of the line.
PROGRESS = re.compile(r"^\[gw\d+\] \[\s*\d+%\] (PASSED|FAILED|ERROR|SKIPPED|XFAIL|XPASS) (.+?)\s*$")
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


def self_test():
    ok = lambda n, msg="": {"outcome": "passed", "longrepr": msg} if not msg else {"outcome": n, "longrepr": msg}
    crash = lambda m: {"outcome": "failed", "crash": {"message": m}, "longrepr": "E   " + m + "\nE   During handling: Other"}
    # A teardown failure after a skipped call is an error with the failure's message, not the skip's.
    r = result({"outcome": "skipped", "setup": ok("passed"), "call": {"outcome": "skipped", "longrepr": "Skipped: x86"},
                "teardown": crash("MemoryUsageExceededError: ('Memory usage (9.1 MiB)")})
    assert r == {"outcome": "error", "message": "MemoryUsageExceededError: ('Memory usage (9.1 MiB)"}, r
    # Only the primary exception: the chained one is dropped.
    r = result({"outcome": "failed", "call": crash("AssertionError: x")})
    assert r == {"outcome": "failed", "message": "AssertionError: x"}, r
    chained = {"outcome": "failed", "crash": {"message": "RuntimeError: cleanup"},
               "longrepr": "E   ValueError: first\nE   more\n\nDuring handling of the above exception, another exception occurred:\n\nE   RuntimeError: cleanup"}
    r = result({"outcome": "failed", "call": chained})
    assert r == {"outcome": "failed", "message": "ValueError: first more"}, r
    # A worker crash (the `???` phase) fails the test and gives its message.
    r = result({"outcome": "passed", "setup": ok("passed"), "???": {"outcome": "failed", "longrepr": "[gw1] linux\nworker 'gw1' crashed while running 't'"}})
    assert r == {"outcome": "failed", "message": "worker 'gw1' crashed while running 't'"}, r
    # A long message is cut with a marker the comparator can see, never silently.
    r = result({"outcome": "failed", "call": crash("x" * (MAX_MESSAGE + 5))})
    assert r["message"].endswith(" [truncated]") and len(r["message"]) == MAX_MESSAGE + len(" [truncated]"), len(r["message"])
    # Node ids with spaces survive the progress log.
    m = PROGRESS.match("[gw0] [ 92%] FAILED a.py::t[x-1000.0 KB/sec] ")
    assert m and m.group(2) == "a.py::t[x-1000.0 KB/sec]", m
    assert meta({"exitcode": 2, "collectors": [{"nodeid": "a.py", "outcome": "failed"}, {"nodeid": "", "outcome": "passed"}]}) == \
        {"exitcode": 2, "collection_errors": ["a.py"]}
    print("collect.py self-test: ok")


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if len(argv) < 3 or argv[1] != "--collected":
        sys.exit(__doc__)
    out, collected_path, rest = argv[0], argv[2], argv[3:]
    log = meta_path = None
    if rest[:1] == ["--meta"]:
        meta_path, rest = rest[1], rest[2:]
    exitcode = None
    if rest[:1] == ["--exitcode"]:
        exitcode, rest = int(rest[1]), rest[2:]
    if rest[:1] == ["--log"]:
        log, rest = rest[1], rest[2:]
    reports = rest
    results = {}
    run_meta = {"exitcode": None, "collection_errors": []}
    if log:
        results.update(from_log(log))
    for path in reports:
        with open(path) as f:
            report = json.load(f)
        run_meta = meta(report)
        for test in report.get("tests", []):
            results[test["nodeid"]] = result(test)
    # The report's exit status is pytest's own; without a report (a stopped run) the container's status stands in,
    # so the comparator never sees an unknown status from a run that did end.
    if run_meta["exitcode"] is None and exitcode is not None:
        run_meta["exitcode"] = exitcode
    if meta_path:
        with open(meta_path, "w") as f:
            json.dump(run_meta, f)
            f.write("\n")
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
