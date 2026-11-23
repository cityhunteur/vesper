---
status: verified
owner: cityhunteur
last_reviewed: 2026-11-23
---

# Nightly evidence (TD-005)

GitHub's macOS runners are virtual machines without nested virtualization, so they cannot boot the System VM. The
System VM suites run on an M3 or later Mac. `make nightly` runs them as one evidence bundle that CI can check and
keep (D-084). Owner-hardware evidence never counts as remote green: a bundle says `"source": "owner-hardware"` and
`"counts_as_remote_green": false`, and `scripts/ci/nightly_evidence.py` refuses a bundle that says otherwise.

## Run

```sh
make nightly                 # run into run/nightly/<UTC time>/ (logs, results.tsv, bundle/)
make nightly PUBLISH=1       # also copy the bundle to docs/generated/nightly/ to commit
CYCLES=50 make nightly       # more reliability boot cycles (default 10)
NIGHTLY_SUITES="systemvm-control-suite ports-suite" make nightly   # another suite list
```

The run:

1. Stops the dev loop, Docker and the System VM, and builds the launcher, the guest's Go binaries and the guest
   image from the current commit.
2. Runs `make systemvm-reliability` (boot cycles, then the control and services suites and a Docker smoke on a VM
   it starts and stops).
3. Starts the System VM with the snapshot share (`make systemvm-up-snapshot`) and the Docker socket.
4. Runs the suites: `systemvm-control-suite`, `systemvm-services-suite`, `systemvm-launchapi-suite`,
   `systemvm-vesperd-suite`, `systemvm-snapshot-suite`, `systemvm-replay-suite`, `docker-conformance` and
   `ports-suite`.
5. Writes `bundle/manifest.json` and `bundle/reports/<target>.json`: the git SHA, whether the tree had changes not
   committed, the baseline revisions, the host, and for each suite its exit status, its verdict, its step counts and
   the SHA-256 of its report. A report older than its suite's start is not that run's, and counts as missing.

It exits 1 if a suite failed and leaves the System VM and the Docker socket running.

## Publish and check

Commit `docs/generated/nightly/` after `make nightly PUBLISH=1`, from a clean tree. The `Nightly Evidence` workflow
(`.github/workflows/nightly-evidence.yml`) runs on every pull request, on a push that changes the bundle or
`docs/specs`, every day, and by manual dispatch. It:

- validates the bundle (schema, each report's SHA-256, the owner-hardware fields) and uploads it as the artifact
  `nightly-evidence-<sha>`;
- warns when the bundle is more than 7 days old, came from a dirty tree, or has a failed suite;
- fails only when a document in `docs/specs` cites `docs/generated/nightly/` while the bundle is in one of those
  states, so a parity claim can cite only current, clean, passing evidence.

Run the same check locally with `python3 -I scripts/ci/nightly_evidence.py --check`.

## A schedule (not installed)

Nothing installs a schedule. To run the evidence every night, the owner can load a launchd agent such as this one,
with the paths changed, in `~/Library/LaunchAgents/dev.vesper.nightly.plist`, and then run
`launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.vesper.nightly.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>dev.vesper.nightly</string>
  <key>ProgramArguments</key>
  <array><string>/bin/zsh</string><string>-lc</string><string>cd /path/to/vesper &amp;&amp; make nightly</string></array>
  <key>StartCalendarInterval</key><dict><key>Hour</key><integer>3</integer><key>Minute</key><integer>15</integer></dict>
  <key>StandardOutPath</key><string>/path/to/vesper/run/nightly/launchd.log</string>
  <key>StandardErrorPath</key><string>/path/to/vesper/run/nightly/launchd.log</string>
</dict>
</plist>
```

A scheduled run does not publish: a person reviews `run/nightly/<time>/bundle` and publishes with `PUBLISH=1`. The
host binaries refuse to start under a sandboxed parent (ADR 0012); a launchd agent is not sandboxed. The Mac must be
awake at that time, and the System VM and Docker socket are restarted by the run.

## Limits

- One Mac, one run: the bundle shows the suites passed on that host at that commit, not on every supported host.
- The upstream Firecracker suite (`make systemvm-upstream-suite`, about 80 minutes) and the Linux reference run
  (CE-0003) are not part of it.
- A self-hosted Apple-silicon runner would make this remote evidence; TD-005 stays open for it.
