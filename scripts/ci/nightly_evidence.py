#!/usr/bin/env python3
"""Owner-hardware evidence bundles for the System VM suites (TD-005, D-084).

GitHub's macOS runners cannot boot Virtualization.framework guests, so the System VM suites run on an M3 or later
Mac (`make nightly`). This tool:

  --build    turns one run's results into a bundle: manifest.json (the git SHA and whether the tree was clean, the
             baseline metadata, the host, and for each suite its exit status, its own verdict, step counts and the
             SHA-256 of its report) and reports/<target>.json, the reports themselves.
  --check    validates a bundle (default docs/generated/nightly): schema, report hashes, and that it never claims to
             be remote green. It warns, with exit 0, when the bundle is old, from a dirty tree, or has a failed suite.
             It fails only when a parity document in docs/specs cites the bundle while it is old, dirty or failed:
             owner-hardware evidence never counts as remote green, and a parity claim may cite only current evidence.
  --self-test runs the unit tests below.
"""
import argparse
import datetime as dt
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

SCHEMA = 1
DEFAULT_MAX_AGE_DAYS = 7
CITATION = "docs/generated/nightly/"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 16), b""):
            h.update(chunk)
    return h.hexdigest()


def now_utc():
    return dt.datetime.now(dt.timezone.utc).replace(microsecond=0)


def iso(t):
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def parse_iso(text):
    return dt.datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=dt.timezone.utc)


def verdict(report):
    """(passed, steps_total, steps_ok) from a suite report: its own `passed` and `steps`, or a replay `summary`."""
    if isinstance(report.get("summary"), dict) and "cases" in report["summary"]:
        s = report["summary"]
        bad = s.get("mismatched", 0) + s.get("setup_failed", 0) + s.get("backend_error", 0)
        return bad == 0, s["cases"], s.get("matched", 0) + s.get("not_applicable", 0)
    steps = report.get("steps") or []
    ok = sum(1 for st in steps if st.get("ok"))
    passed = report.get("passed")
    return (bool(passed) if passed is not None else ok == len(steps)), len(steps), ok


def baseline_from_metadata(path):
    if not path or not os.path.exists(path):
        return {}
    data = json.load(open(path, encoding="utf-8"))
    out = {}
    for item in data.get("metadata", []):
        key = item.get("key", "")
        if key.startswith("baseline.") and key.endswith(".revision"):
            out[key[len("baseline."):-len(".revision")]] = item.get("value", "")
    return out


def build(out_dir, results_tsv, git_sha, git_dirty, started_at, host, metadata=None, finished_at=None):
    """results_tsv: lines `target<TAB>rc<TAB>report path<TAB>start epoch`. A report older than its target's start
    is not this run's and counts as missing."""
    out = pathlib.Path(out_dir)
    (out / "reports").mkdir(parents=True, exist_ok=True)
    suites = []
    for line in open(results_tsv, encoding="utf-8"):
        if not line.strip():
            continue
        target, rc, report, start = line.rstrip("\n").split("\t")
        entry = {"target": target, "rc": int(rc), "passed": False, "steps_total": 0, "steps_ok": 0}
        if os.path.exists(report) and os.path.getmtime(report) >= float(start):
            dest = out / "reports" / f"{target}.json"
            shutil.copyfile(report, dest)
            passed, total, ok = verdict(json.load(open(dest, encoding="utf-8")))
            entry.update(passed=passed and int(rc) == 0, steps_total=total, steps_ok=ok,
                         report=f"reports/{target}.json", sha256=sha256(dest))
        else:
            entry["report_missing"] = True
        suites.append(entry)
    manifest = {
        "schema": SCHEMA,
        "source": "owner-hardware",
        "counts_as_remote_green": False,
        "git_sha": git_sha,
        "git_dirty": bool(git_dirty),
        "started_at": started_at,
        "finished_at": finished_at or iso(now_utc()),
        "host": host,
        "baseline": baseline_from_metadata(metadata),
        "suites": suites,
        "passed": bool(suites) and all(s["passed"] for s in suites),
    }
    json.dump(manifest, open(out / "manifest.json", "w", encoding="utf-8"), indent=2, sort_keys=True)
    return manifest


def check(bundle, docs_root=".", max_age_days=DEFAULT_MAX_AGE_DAYS, now=None):
    """Returns (errors, warnings, blocking): errors and blocking fail the check, warnings do not."""
    errors, warnings, blocking = [], [], []
    bundle = pathlib.Path(bundle)
    cited = sorted(
        str(p.relative_to(docs_root)) for p in pathlib.Path(docs_root, "docs/specs").glob("*")
        if p.is_file() and CITATION in p.read_text(encoding="utf-8", errors="replace")
    ) if pathlib.Path(docs_root, "docs/specs").is_dir() else []
    manifest_path = bundle / "manifest.json"
    if not manifest_path.exists():
        warnings.append(f"no owner-hardware evidence is published in {bundle} (run make nightly PUBLISH=1)")
        if cited:
            blocking.append(f"{', '.join(cited)} cite {CITATION}, but no bundle is published")
        return errors, warnings, blocking
    try:
        m = json.load(open(manifest_path, encoding="utf-8"))
    except (OSError, ValueError) as e:
        return [f"{manifest_path}: {e}"], warnings, blocking
    if m.get("schema") != SCHEMA:
        errors.append(f"schema {m.get('schema')!r}, want {SCHEMA}")
    if m.get("counts_as_remote_green") is not False or m.get("source") != "owner-hardware":
        errors.append("an owner-hardware bundle must say source owner-hardware and counts_as_remote_green false")
    for s in m.get("suites", []):
        if s.get("report_missing"):
            continue
        path = bundle / s.get("report", "")
        if not path.is_file():
            errors.append(f"{s.get('target')}: report {s.get('report')} is missing")
        elif sha256(path) != s.get("sha256"):
            errors.append(f"{s.get('target')}: report {s.get('report')} does not match its SHA-256")
    problems = []
    try:
        age = ((now or now_utc()) - parse_iso(m["finished_at"])).total_seconds() / 86400
        if age > max_age_days:
            problems.append(f"the bundle is {age:.1f} days old (limit {max_age_days})")
    except (KeyError, ValueError):
        errors.append("finished_at is missing or not YYYY-MM-DDTHH:MM:SSZ")
    if m.get("git_dirty"):
        problems.append(f"it was made from a tree with changes not committed (at {m.get('git_sha', '?')[:12]})")
    failed = [s.get("target") for s in m.get("suites", []) if not s.get("passed")]
    if failed:
        problems.append(f"suites failed or have no report: {', '.join(failed)}")
    warnings += problems
    if problems and cited:
        blocking.append(f"{', '.join(cited)} cite {CITATION} while {'; '.join(problems)}")
    return errors, warnings, blocking


class Tests(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        (self.tmp / "run").mkdir()
        ok = {"passed": True, "steps": [{"name": "a", "ok": True}, {"name": "b", "ok": True}]}
        json.dump(ok, open(self.tmp / "run/control-report.json", "w"))
        json.dump({"summary": {"cases": 3, "matched": 2, "mismatched": 0, "setup_failed": 0, "backend_error": 0,
                               "not_applicable": 1}}, open(self.tmp / "run/replay-report.json", "w"))
        start = os.path.getmtime(self.tmp / "run/control-report.json") - 1
        tsv = self.tmp / "results.tsv"
        tsv.write_text(f"control\t0\t{self.tmp}/run/control-report.json\t{start}\n"
                       f"replay\t0\t{self.tmp}/run/replay-report.json\t{start}\n")
        self.tsv, self.start = tsv, start
        self.bundle = self.tmp / "bundle"

    def make(self, **kw):
        args = dict(git_sha="a" * 40, git_dirty=False, started_at="2026-11-23T00:00:00Z", host="test",
                    finished_at="2026-11-23T01:00:00Z")
        args.update(kw)
        return build(self.bundle, self.tsv, **args)

    def now(self, days):
        return parse_iso("2026-11-23T01:00:00Z") + dt.timedelta(days=days)

    def test_build_records_each_suite_and_its_hash(self):
        m = self.make()
        self.assertTrue(m["passed"])
        self.assertEqual([s["target"] for s in m["suites"]], ["control", "replay"])
        self.assertEqual(m["suites"][1]["steps_ok"], 3)
        self.assertFalse(m["counts_as_remote_green"])
        self.assertEqual(check(self.bundle, self.tmp, now=self.now(1)), ([], [], []))

    def test_a_report_older_than_its_suite_is_missing(self):
        self.tsv.write_text(f"control\t0\t{self.tmp}/run/control-report.json\t{self.start + 3600}\n")
        m = self.make()
        self.assertTrue(m["suites"][0]["report_missing"])
        self.assertFalse(m["passed"])

    def test_a_nonzero_exit_fails_the_suite(self):
        self.tsv.write_text(f"control\t2\t{self.tmp}/run/control-report.json\t{self.start}\n")
        self.assertFalse(self.make()["passed"])

    def test_edited_report_is_an_error(self):
        self.make()
        (self.bundle / "reports/control.json").write_text("{}")
        errors, _, _ = check(self.bundle, self.tmp, now=self.now(1))
        self.assertTrue(any("SHA-256" in e for e in errors), errors)

    def test_claiming_remote_green_is_an_error(self):
        self.make()
        m = json.load(open(self.bundle / "manifest.json"))
        m["counts_as_remote_green"] = True
        json.dump(m, open(self.bundle / "manifest.json", "w"))
        errors, _, _ = check(self.bundle, self.tmp, now=self.now(1))
        self.assertTrue(any("remote_green" in e for e in errors), errors)

    def test_old_dirty_or_failed_bundles_warn_but_do_not_block_uncited(self):
        self.make(git_dirty=True)
        errors, warnings, blocking = check(self.bundle, self.tmp, now=self.now(30))
        self.assertEqual(errors, [])
        self.assertEqual(blocking, [])
        self.assertTrue(any("days old" in w for w in warnings) and any("not committed" in w for w in warnings))

    def test_a_parity_claim_citing_old_evidence_blocks(self):
        self.make()
        (self.tmp / "docs/specs").mkdir(parents=True)
        (self.tmp / "docs/specs/parity.md").write_text(f"see {CITATION}manifest.json\n")
        self.assertEqual(check(self.bundle, self.tmp, now=self.now(1))[2], [])
        blocking = check(self.bundle, self.tmp, now=self.now(30))[2]
        self.assertTrue(blocking and "parity.md" in blocking[0], blocking)

    def test_no_bundle_warns_and_blocks_only_when_cited(self):
        errors, warnings, blocking = check(self.tmp / "none", self.tmp, now=self.now(0))
        self.assertEqual((errors, blocking), ([], []))
        self.assertTrue(warnings)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--build", metavar="OUT_DIR")
    ap.add_argument("--results")
    ap.add_argument("--git-sha")
    ap.add_argument("--git-dirty", action="store_true")
    ap.add_argument("--started-at")
    ap.add_argument("--host", default="")
    ap.add_argument("--metadata")
    ap.add_argument("--check", metavar="BUNDLE", nargs="?", const="docs/generated/nightly")
    ap.add_argument("--docs-root", default=".")
    ap.add_argument("--max-age-days", type=float, default=DEFAULT_MAX_AGE_DAYS)
    a = ap.parse_args()
    if a.self_test:
        result = unittest.TextTestRunner(verbosity=1).run(unittest.defaultTestLoader.loadTestsFromTestCase(Tests))
        return 0 if result.wasSuccessful() else 1
    if a.build:
        m = build(a.build, a.results, a.git_sha, a.git_dirty, a.started_at, a.host, a.metadata)
        for s in m["suites"]:
            state = "passed" if s["passed"] else ("NO REPORT" if s.get("report_missing") else "FAILED")
            print(f"nightly: {s['target']}: {state} (exit {s['rc']}, {s['steps_ok']}/{s['steps_total']})")
        print(f"nightly: {'passed' if m['passed'] else 'FAILED'}; bundle {a.build}")
        return 0 if m["passed"] else 1
    if a.check:
        errors, warnings, blocking = check(a.check, a.docs_root, a.max_age_days)
        gh = os.environ.get("GITHUB_ACTIONS") == "true"
        for w in warnings:
            print(f"::warning::nightly evidence: {w}" if gh else f"warning: {w}")
        for e in errors + blocking:
            print(f"::error::nightly evidence: {e}" if gh else f"error: {e}")
        if not (errors or blocking):
            print(f"nightly evidence: {a.check} valid ({len(warnings)} warning(s))")
        return 1 if errors or blocking else 0
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
