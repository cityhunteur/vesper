---
status: proposed
owner: cityhunteur
last_reviewed: 2026-11-22
---

# 0012. Host binaries run under generated seatbelt profiles, ad-hoc signed with the hardened runtime

Date: 2026-11-22. Supersedes: none. Superseded by: none.

## Context and problem

The owner approved sandboxing and ad-hoc signing for the host binaries in D-066 (programme step E). Three binaries
run on macOS: `vesper-vmlauncher` (Swift, Virtualization.framework), `vesperd` and `vesper-proxy` (Rust). Today:

- Only the launcher is signed (`scripts/macos/build-vmlauncher.sh`, ad-hoc, `com.apple.security.virtualization`).
  `cargo` builds of `vesperd` and `vesper-proxy` are not signed with their plists, so they run with no sandbox,
  although `packaging/macos/vesperd.entitlements` and `vesper-proxy.entitlements` grant
  `com.apple.security.app-sandbox`.
- A spike on 2026-11-22 (macOS 27, Apple silicon) signed `vesper-proxy` ad hoc with its plist:
  1. With no bundle identifier the process is stopped at start with SIGTRAP (exit status 133), before `main`. So
     the shipped plists would stop both Rust binaries if they were ever used.
  2. With an `Info.plist` embedded in `__TEXT,__info_plist` (`CFBundleIdentifier`), it starts. It cannot then bind
     its socket in the repository (`listen on unix socket …/run/sbx-spike/docker.sock: Operation not permitted`);
     it can bind one under its container, `~/Library/Containers/<id>/Data/`.
  3. `/usr/bin/sandbox-exec -p '<profile>'` still runs a process under a given profile.
- What each binary touches:
  - The launcher: the launch config, kernel, initramfs and boot media under `build/systemvm`, the console log, the
    vsock bridge socket it creates, the data disk image, every share the user names (any absolute path, FSEvents on
    them), and Virtualization.framework's services.
  - `vesper-proxy`: its listen socket (`run/vesper/docker.sock`), the bridge socket, `run/vesper/ports.json`,
    loopback TCP and UDP ports of any number, and its entitlements file.
  - `vesperd`: its socket (`XDG_RUNTIME_DIR` or `--socket`), the bridge socket, `--stage` sources inside shares, and
    on the model backend the host paths named in API requests.

## Decision

1. Each host binary runs under a seatbelt profile that it generates at start from its own arguments and launch
   config, then applies by running itself again under `/usr/bin/sandbox-exec -p <profile>`. A marker in the
   environment stops a second wrap. The profile denies by default and allows only the binary's own files: the
   paths listed above, by absolute path; the system libraries (`(import "system.sb")`); loopback networking and the
   named Unix sockets. The order is `vesper-proxy` first, then `vesperd`, then the launcher, which also needs the
   Mach services Virtualization.framework uses; each step lands only with its runtime suites passing under the
   profile. `VESPER_NO_SANDBOX=1` runs a binary without its profile, for diagnosis, and says so in its first log
   line.
2. All three binaries are signed ad hoc with the hardened runtime (`codesign --options runtime`) and their plists.
   `com.apple.security.app-sandbox` is removed from the `vesperd` and `vesper-proxy` plists: an unbundled binary
   with it is stopped at start, and the App Sandbox's container rule would move every runtime path. The
   `vesperd` plist keeps `com.apple.security.virtualization` only if `vesperd` itself calls Virtualization.framework;
   otherwise its plist becomes empty. No key is added.
3. Tests: a sandbox suite runs real shares, the data disk and `docker.sock` with every binary under its profile
   (`make docker-conformance`, `make binds-suite`, `make ports-suite`, `make systemvm-vesperd-suite`), and probes
   that each binary cannot read a file outside its profile (a file in `~/Library` it never names). CI's macOS job
   runs `codesign --verify --strict` on all three and compares each signed entitlement set with its plist
   (TD-010 did this for the launcher).
4. No app bundle, launchd agent or release (TD-046 keeps those for a release plan).

## Alternatives considered

- **App Sandbox with embedded bundle identifiers.** Supported by Apple and enforced by the kernel. But every path
  must sit in the binary's container or behind static temporary-exception entitlements: `docker.sock` and the
  bridge socket would move into `~/Library/Containers/…`, the three binaries would need an App Group to share
  sockets (a team identifier, which ad-hoc signing does not have), and shares the user names at run time cannot be
  reached without a file dialog. The launcher, a command-line process, has no dialog.
- **Signing and the hardened runtime only.** Cheap and safe, but it does not sandbox anything, which is what the
  owner approved.
- **`sandbox_init` with a profile string from inside the process.** The same profiles without a second exec, but
  the call that takes a profile string is private API.

## Rationale and consequences

- The profile names exactly what each binary may touch, so a compromised `vesper-proxy` cannot read the user's
  files or open sockets outside its own. Profiles are generated from the same arguments the binary already
  validates, so a new share needs no new entitlement.
- `sandbox-exec` is marked deprecated by Apple and could be removed; the binaries then fall back to running without
  a profile only if `VESPER_NO_SANDBOX=1` is set, otherwise they refuse to start, so a missing sandbox is never
  silent. That risk is recorded as debt.
- Each binary gains a profile generator and a test of it; the runtime suites run under the profiles.
- Removing the App Sandbox key changes two plists without widening them; the exact-set check (TD-010) is updated in
  the same change.

## Implementation and migration impact

Slices, each with its runtime evidence: (1) `vesper-proxy` profile, signing with the hardened runtime, plist
change, sandbox probe, CI verify; (2) `vesperd`, both backends; (3) the launcher, with its Virtualization.framework
services found by tracing a boot under a logging profile first. A slice that cannot pass its suites under the
profile stops and returns here.

## References

- D-066 (owner approval), D-065 (programme step E), TD-010, TD-046, SECURITY.md, `packaging/macos/`.
- Spike evidence: the three results above (run on 2026-11-22; the spike's files were in the session scratchpad).
