#!/usr/bin/env python3
"""Turn pytest-json-report files into the results file `baselinectl --mode upstream-compare` reads (GH #172).

Usage: python3 -I collect.py OUT.json --collected COLLECTED.txt [--meta META.json] [--exitcode N] [--log PYTEST.log]
           [--junit JUNIT.xml] [--alone ALONE.json] [--alone-junit ALONE-JUNIT.xml] [REPORT.json ...]
       python3 -I collect.py --self-test

OUT.json is {nodeid: {"outcome": ..., "message": ...}}. The outcome is the worst phase: a failure in setup or teardown
is "error". A later report overrides an earlier one for the same test (a rerun). Every node id in COLLECTED.txt (one
per line, from `pytest --collect-only -q`) that no report mentions is "hung": the run was stopped while it ran or
before it started, which the comparator must explain like a failure. When the run was stopped, pytest-json-report
never wrote its file, so `--log` recovers each finished test from pytest-xdist's progress lines
(`[gw0] [ 3%] PASSED nodeid`), keeping the worst outcome a test logged (a teardown ERROR after a PASSED).

Upstream marks some tests `@pytest.mark.flaky(reruns=N)` (pytest-rerunfailures). pytest-json-report then keeps one
entry with outcome "rerun", an attempt that was retried, and not the final attempt; `--log` gives that attempt: the
worst outcome logged after the test's last RERUN line. pytest's JUnit file writes no failure for an attempt that
pytest-rerunfailures retried (its outcome is "rerun"), so a failure in a test's testcase is the final attempt's, with its
whole text (one testcase per test in the runs on record, 2026-11-17); `--junit` gives a rerun test that failed in the
end that attempt's message, so an exception class can match it (TD-059). Without a log, or with no line after the last RERUN, a rerun
test is an "error". `--alone-junit` does the same for the isolated rerun, where it also gives the final outcome.

`--alone` is the report of the runner's isolated rerun (decision D-031): the tests that matched no exception class
in the full run, run again one at a time with nothing else loading the host. A test that passes there becomes
{"outcome": "passed", "under_load": {outcome, message}}, which the comparator counts as passed and lists; one that
does not keeps its full-run result and gains "alone": {outcome, message}, which the comparator classifies (D-039). A test the full run did not report is
never taken from ALONE.json, and an isolated session that pytest did not end normally (an exit status other than 0
or 1, or a collection error) is not used at all: the full run's results stand.

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


def alone_usable(report):
    m = meta(report)
    return m["exitcode"] in (0, 1) and not m["collection_errors"]


def merge_alone(results, alone_tests, alone_junit=None):
    for test in alone_tests:
        nodeid = test["nodeid"]
        if nodeid not in results or results[nodeid]["outcome"] in ("passed", "xfailed", "xpassed"):
            continue
        if test["outcome"] == "rerun":
            # The final attempt from the isolated session's JUnit file; without it the rerun stays an error.
            again = final_attempt(nodeid, alone_junit) or {"outcome": "error", "message": "rerun (pytest-rerunfailures) in the isolated rerun"}
        else:
            again = result(test)
        first = results[nodeid]
        if again["outcome"] == "passed":
            results[nodeid] = {"outcome": "passed", "message": "", "under_load": {"outcome": first["outcome"], "message": first["message"]}}
        else:
            first["alone"] = again


def meta(report):
    errors = [c.get("nodeid") or "?" for c in report.get("collectors", []) if c.get("outcome") == "failed"]
    return {"exitcode": report.get("exitcode"), "collection_errors": errors}


# A node id can contain spaces (`...-1000.0 KB/sec]`), so it runs to the end of the line.
PROGRESS = re.compile(r"^\[gw\d+\] \[\s*\d+%\] (PASSED|FAILED|ERROR|SKIPPED|XFAIL|XPASS|RERUN) (.+?)\s*$")
FROM_LOG = {"PASSED": "passed", "XFAIL": "xfailed", "XPASS": "xpassed", "SKIPPED": "skipped", "FAILED": "failed", "ERROR": "error"}
WORST = ["passed", "xfailed", "xpassed", "skipped", "failed", "error"]


def log_outcomes(lines):
    """{nodeid: (worst outcome logged after its last RERUN line or None, reruns)}."""
    found = {}
    for line in lines:
        m = PROGRESS.match(line)
        if not m:
            continue
        status, nodeid = m.group(1), m.group(2)
        outcome, reruns = found.get(nodeid, (None, 0))
        if status == "RERUN":
            found[nodeid] = (None, reruns + 1)
        elif outcome is None or WORST.index(FROM_LOG[status]) > WORST.index(outcome):
            found[nodeid] = (FROM_LOG[status], reruns)
    return found


def from_log(path):
    with open(path, errors="replace") as f:
        found = log_outcomes(f)
    return {n: {"outcome": o, "message": "from the progress log; the JSON report was not written"}
            for n, (o, _) in found.items() if o is not None}


def resolve_rerun(nodeid, logged, junit=None):
    outcome, reruns = logged.get(nodeid, (None, 0))
    if outcome is None:
        return {"outcome": "error", "message": "rerun (pytest-rerunfailures) with no final result in the progress log"}
    final = final_attempt(nodeid, junit)
    # The final attempt's own message, so a class can match it; the log's outcome stands (TD-059).
    if outcome in ("failed", "error") and final and final["outcome"] in ("failed", "error") and final["message"]:
        return {"outcome": outcome, "message": final["message"]}
    return {"outcome": outcome, "message": f"{outcome} after {reruns} rerun(s) (pytest-rerunfailures), from the progress log"}


def junit_key(nodeid):
    """The (classname, name) pytest's JUnit file gives a node id: `a/b.py::C::t[x]` is ("a.b.C", "t[x]"), as pytest's
    `mangle_test_address` makes them (the parameters are split off first, so a `::` inside them stays)."""
    path, bracket, params = nodeid.partition("[")
    names = path.split("::")
    if len(names) < 2:
        return None
    names[0] = re.sub(r"\.py$", "", names[0].replace("/", "."))
    names[-1] += bracket + params
    return (".".join(names[:-1]), names[-1])


def junit_results(path):
    """{(classname, name): {outcome, message}}, the last testcase of each name. A retried attempt writes no failure,
    so a failure here is the final attempt's."""
    import xml.etree.ElementTree as ET
    found = {}
    for case in ET.parse(path).getroot().iter("testcase"):
        key = (case.get("classname") or "", case.get("name") or "")
        outcome, message = "passed", ""
        for tag, out in (("error", "error"), ("failure", "failed"), ("skipped", "skipped")):
            el = case.find(tag)
            if el is not None:
                outcome = out
                message = phase_message({"crash": {"message": el.get("message") or ""}, "longrepr": el.text or ""})
                break
        found[key] = {"outcome": outcome, "message": message}
    return found


def final_attempt(nodeid, junit):
    return (junit or {}).get(junit_key(nodeid))


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
    # A rerun resolves to the attempt after the last RERUN line, not the worst of all attempts.
    logged = log_outcomes(["[gw0] [ 5%] FAILED a.py::t ", "[gw0] [ 5%] RERUN a.py::t ", "[gw0] [ 9%] PASSED a.py::t ",
                           "[gw1] [ 6%] RERUN a.py::u ", "[gw1] [ 9%] PASSED a.py::v "])
    assert resolve_rerun("a.py::t", logged)["outcome"] == "passed", logged
    assert resolve_rerun("a.py::u", logged)["outcome"] == "error", logged
    assert resolve_rerun("a.py::w", {})["outcome"] == "error"
    # The isolated rerun: a pass replaces the result and keeps the first; a failure is recorded beside it; a test
    # that passed, or that the full run never reported, is left alone.
    res = {"a": {"outcome": "failed", "message": "m"}, "b": {"outcome": "error", "message": "n"}, "c": {"outcome": "passed", "message": ""}}
    merge_alone(res, [{"nodeid": "a", "outcome": "passed"}, {"nodeid": "b", "outcome": "failed", "call": crash("X: y")},
                      {"nodeid": "c", "outcome": "failed", "call": crash("X: y")}, {"nodeid": "d", "outcome": "passed"}])
    assert res["a"] == {"outcome": "passed", "message": "", "under_load": {"outcome": "failed", "message": "m"}}, res["a"]
    assert res["b"] == {"outcome": "error", "message": "n", "alone": {"outcome": "failed", "message": "X: y"}}, res["b"]
    assert res["c"] == {"outcome": "passed", "message": ""} and "d" not in res, res
    assert alone_usable({"exitcode": 1, "collectors": []}) and not alone_usable({"exitcode": 3}) and not alone_usable({})
    assert not alone_usable({"exitcode": 0, "collectors": [{"nodeid": "a.py", "outcome": "failed"}]})
    # TD-059: the JUnit file's last testcase of a name is the final attempt; its whole message, primary exception only.
    import os, tempfile
    xml = ('<testsuites><testsuite>'
           '<testcase classname="t.a.C" name="f[1]"/>'
           '<testcase classname="t.a.C" name="f[1]"><failure message="RuntimeError: late">E   TimeoutError: boot\n'
           'During handling of the above exception, another exception occurred:\nE   RuntimeError: late</failure></testcase>'
           '<testcase classname="t.a" name="g"/><testcase classname="t.a" name="g"><error message="X: y">E   X: y</error></testcase>'
           '</testsuite></testsuites>')
    fd, path = tempfile.mkstemp(suffix=".xml")
    with os.fdopen(fd, "w") as f:
        f.write(xml)
    try:
        junit = junit_results(path)
    finally:
        os.unlink(path)
    assert junit_key("t/a.py::C::f[1]") == ("t.a.C", "f[1]") and junit_key("t/a.py::g") == ("t.a", "g"), junit
    assert junit_key("t/a.py::f[x::y]") == ("t.a", "f[x::y]") and junit_key("t/a.py") is None
    assert junit[("t.a.C", "f[1]")] == {"outcome": "failed", "message": "TimeoutError: boot"}, junit
    logged = log_outcomes(["[gw0] [ 5%] RERUN t/a.py::C::f[1] ", "[gw0] [ 9%] FAILED t/a.py::C::f[1] "])
    assert resolve_rerun("t/a.py::C::f[1]", logged, junit) == {"outcome": "failed", "message": "TimeoutError: boot"}
    assert resolve_rerun("t/a.py::C::f[1]", logged)["message"].startswith("failed after 1 rerun(s)")
    # The isolated rerun: a rerun test takes its final attempt from the JUnit file, else stays an error.
    res = {"t/a.py::g": {"outcome": "failed", "message": "m"}, "t/a.py::h": {"outcome": "failed", "message": "m"}}
    merge_alone(res, [{"nodeid": "t/a.py::g", "outcome": "rerun"}, {"nodeid": "t/a.py::h", "outcome": "rerun"}], junit)
    assert res["t/a.py::g"]["alone"] == {"outcome": "error", "message": "X: y"}, res
    assert res["t/a.py::h"]["alone"]["outcome"] == "error" and "isolated rerun" in res["t/a.py::h"]["alone"]["message"], res
    passed = {"t/a.py::C::f[1]": {"outcome": "failed", "message": "m"}}
    merge_alone(passed, [{"nodeid": "t/a.py::C::f[1]", "outcome": "rerun"}], {("t.a.C", "f[1]"): {"outcome": "passed", "message": ""}})
    assert passed["t/a.py::C::f[1]"]["outcome"] == "passed" and "under_load" in passed["t/a.py::C::f[1]"], passed
    print("collect.py self-test: ok")


def _readable(path):
    """A JUnit file that is missing or not XML is left out, with a line: the run's other results stand."""
    import xml.etree.ElementTree as ET
    try:
        ET.parse(path)
        return True
    except (OSError, ET.ParseError) as err:
        print(f"collect: JUnit file {path} not used: {err}")
        return False


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
    junit = None
    if rest[:1] == ["--junit"]:
        junit, rest = (junit_results(rest[1]) if _readable(rest[1]) else None), rest[2:]
    alone = None
    if rest[:1] == ["--alone"]:
        alone, rest = rest[1], rest[2:]
    alone_junit = None
    if rest[:1] == ["--alone-junit"]:
        alone_junit, rest = (junit_results(rest[1]) if _readable(rest[1]) else None), rest[2:]
    reports = rest
    results = {}
    run_meta = {"exitcode": None, "collection_errors": []}
    logged = {}
    if log:
        with open(log, errors="replace") as f:
            logged = log_outcomes(f)
        # Tests the log shows finished, for a run whose JSON report was never written.
        if not reports:
            results.update(from_log(log))
    for path in reports:
        with open(path) as f:
            report = json.load(f)
        run_meta = meta(report)
        for test in report.get("tests", []):
            results[test["nodeid"]] = resolve_rerun(test["nodeid"], logged, junit) if test["outcome"] == "rerun" else result(test)
    # The report's exit status is pytest's own; without a report (a stopped run) the container's status stands in,
    # so the comparator never sees an unknown status from a run that did end.
    if run_meta["exitcode"] is None and exitcode is not None:
        run_meta["exitcode"] = exitcode
    if meta_path:
        with open(meta_path, "w") as f:
            json.dump(run_meta, f)
            f.write("\n")
    if alone:
        with open(alone) as f:
            alone_report = json.load(f)
        if alone_usable(alone_report):
            merge_alone(results, alone_report.get("tests", []), alone_junit)
        else:
            print(f"collect: the isolated rerun did not end normally ({meta(alone_report)}); its results are not used")
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
