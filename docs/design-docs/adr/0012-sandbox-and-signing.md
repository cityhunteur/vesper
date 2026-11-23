---
status: accepted
owner: cityhunteur
last_reviewed: 2026-11-23
---

# 0012. Host binaries apply a seatbelt profile to themselves at start, and are ad-hoc signed with the hardened runtime

Date: 2026-11-22. Supersedes: none. Superseded by: none.

## Context and problem

The owner approved sandboxing and ad-hoc signing for the host binaries in D-066 (programme step E). Three binaries
run on macOS: `vesper-vmlauncher` (Swift, Virtualization.framework), `vesperd` and `vesper-proxy` (Rust). Today:

- Only the launcher is signed (`scripts/macos/build-vmlauncher.sh`, ad hoc, `com.apple.security.virtualization`).
  `cargo` builds of `vesperd` and `vesper-proxy` are not signed with their plists, so they run with no sandbox,
  although `packaging/macos/vesperd.entitlements` and `vesper-proxy.entitlements` grant
  `com.apple.security.app-sandbox`.
- `scripts/macos/sandbox-spike/run.sh` (macOS 27.0.1, Apple silicon, 2026-11-22) shows:
  1. An unbundled binary signed with `com.apple.security.app-sandbox` is stopped before `main` (exit status 133,
     SIGTRAP). The shipped plists would stop both Rust binaries if they were ever used.
  2. `sandbox_init(profile, 0, &err)` with a profile string sandboxes the calling process in place: the pid does
     not change, an allowed read works, other reads and writes fail with `Operation not permitted`, and a second
     call cannot widen the sandbox (it fails).
  3. Under a parent that is already sandboxed, `sandbox_init` fails (`Operation not permitted`), and the process
     would run with only the parent's sandbox.
  4. With an `Info.plist` embedded in `__TEXT,__info_plist`, an App Sandbox binary starts, but may bind a socket only
     under its container `~/Library/Containers/<id>/Data/`, not in the repository (`Operation not permitted`).
- What each binary touches:
  - The launcher: the launch config, kernel, initramfs and boot media under `build/systemvm`, the console log, the
    vsock bridge socket it creates, the data disk image, every share the user names (any absolute path, with
    FSEvents on them), and Virtualization.framework's services.
  - `vesper-proxy`: its listen socket and `<socket>.lock`, the bridge socket, `run/vesper/ports.json`, loopback TCP
    and UDP ports of any number, its `--entitlements-file`.
  - `vesperd`: its socket and `<socket>.lock` (`--socket`, else the first of `XDG_RUNTIME_DIR`, `TMPDIR` and `HOME`), the bridge socket, the baseline
    lock (a path relative to the working directory), `--launch-config`, and on the model backend the host paths
    named in API requests (`boot_source.rs`, `logger_metrics.rs`, `pmem.rs`). It runs no other program: the
    agent, not `vesperd`, opens `--stage` sources inside shares, and `systemvmctl`, not `vesperd`, runs the
    launcher for `--host-capabilities` (corrected 2026-11-23, slice 2).

## Decision

1. **In-process seatbelt.** The first statement of each binary's `main`, before any thread, runtime, socket, lock or
   log file, builds the binary's profile and calls `sandbox_init(profile, 0, &err)`. The profile denies by default,
   imports `system.sb`, and allows only the files and sockets above.
   - Paths are canonicalised with `realpath` of the deepest existing ancestor plus the rest, so `/tmp` and `/var`
     become `/private/…`. A file is a `(literal …)`, a directory the binary creates or reads is a `(subpath …)`.
     One function chooses each path for both the profile and the code that opens it.
   - A path with NUL, a newline or another control character is refused. Backslash and double quote are escaped as
     SBPL string escapes. A unit test builds profiles from paths with `"`, `)`, `\` and spaces and checks the parsed
     result.
   - Network rules are exact. `vesper-proxy`: `network-bind` and `network-inbound` on `localhost:*` only, and
     `network-outbound` only to its named Unix sockets. `vesperd`: its socket inbound and the bridge outbound.
     Neither may start a process.
   - **Fail closed.** If `sandbox_init` fails, for example under a parent that is already sandboxed, the binary
     refuses to start and says so: "cannot apply the sandbox profile: <error>; is the parent process sandboxed?".
     After the call, it checks itself: opening a canary path the profile denies must fail with `EPERM`, or it
     refuses to start. `--no-sandbox`, a command-line flag (no environment variable), runs without the profile for
     diagnosis; it prints a warning in the first log line and on every start, and suites and CI assert it is not
     passed.
   - The launcher's read-only modes (`--host-capabilities`, `--print-config`, `--watch-dry-run`) need no VM; they
     run under the profile of a parent that starts them, such as `vesperd`, and skip their own call there.
   - The model backend reads and writes only under `--host-root` directories (default: the working directory and
     the per-user temporary directory macOS assigns); a request for a path outside them gets the error envelope Firecracker gives for a path it cannot
     open, and the compat registry records the narrowing.
   - The order is `vesper-proxy` first, then `vesperd`, then the launcher. Each lands only with its runtime suites
     passing under its profile. The launcher's slice starts with a spike: boot once with denials reported
     (`(deny default (with report))`) and add each named Mach service with its evidence; no broad `mach-lookup`.
     Virtualization.framework's own XPC processes are started by launchd and are outside the launcher's profile.
2. **Signing.** `make sign` signs all three binaries ad hoc with the hardened runtime (`codesign --options
   runtime`) and their plists, after each build; `dev-up`, `docker-up` and the sandbox suites run the signed
   binaries and check their signature. `com.apple.security.app-sandbox` is removed from the `vesperd` and
   `vesper-proxy` plists: it removes a restriction that never took effect, and the seatbelt profile replaces it
   (D-066). `vesperd` does not call Virtualization.framework, so its plist loses
   `com.apple.security.virtualization` as well, and `vesperd --entitlements-file` stops requiring it, in the same
   change as `scripts/ci/validate-entitlements.sh`, `scripts/macos/dev-up.sh` and SECURITY.md. No key is added; no
   `com.apple.security.cs.*` exception is needed; `get-task-allow` is never shipped (debug an unsigned `cargo` build
   with `--no-sandbox`).
3. **Tests.** A sandbox suite runs `make docker-conformance`, `make binds-suite`, `make ports-suite` and
   `make systemvm-vesperd-suite` with every binary signed and under its profile, and negative probes in each
   binary: write outside its paths, read `~/.ssh`, connect to a Unix socket it does not name, an outbound loopback
   TCP connection, bind a non-loopback address, start `/bin/sh`, a share named with other letter case, and a hard
   link in a share to a file outside it. CI's macOS job runs `codesign --verify --strict` on all three and compares
   each signed entitlement set with its plist (TD-010 does this for the launcher).
4. No app bundle, launchd agent or release (TD-046 keeps those for a release plan).

## Alternatives considered

- **`sandbox-exec` around a re-exec of the binary.** The same profiles, but a second process or an `execv` with a
  marker in the environment: a marker anyone can set turns the sandbox off silently, and pid files and signals
  must follow the exec. `sandbox-exec` also takes `-D` parameters, which avoids escaping; in-process
  `sandbox_init` keeps the pid and needs no marker, so escaping with tests was chosen instead.
- **App Sandbox with embedded bundle identifiers.** Supported by Apple and enforced by the kernel. But every path
  must sit in the binary's container or behind static temporary-exception entitlements (a widening the owner must
  approve): `docker.sock` and the bridge socket would move into `~/Library/Containers/…`, sharing sockets among the
  three would need an App Group (a team identifier, which ad-hoc signing does not have), and shares the user names
  at run time cannot be reached without a file dialog, which a command-line launcher does not have.
- **Signing and the hardened runtime only.** Cheap, but it sandboxes nothing, which is what the owner approved.

## Rationale and consequences

- The profile names what each binary may touch. The limit is real but not narrow for `vesper-proxy`: through the
  Docker socket and the bridge, which reaches the agent (1024), dockerd (2375) and the port relays (2400, 2401), an
  attacker in the proxy can still reach every share and the System VM. The profile stops it reading the user's
  other files and opening other sockets. A bridge socket for each client, with its own port set, is recorded as
  debt (TD-089).
- `sandbox_init` is deprecated, like `sandbox-exec`. If Apple removes it, the binaries refuse to start without
  `--no-sandbox`, so a missing sandbox is never silent. Recorded as debt.
- The first `unsafe` code in the Rust crate is this one FFI call, in one function with its safety comment.
- Running inside a sandboxed agent or IDE now fails at start with a distinct message; `--no-sandbox` is the
  documented way out for diagnosis.
- Hardened runtime: `DYLD_*` variables are ignored and `lldb` cannot attach to a signed binary.

## Implementation and migration impact

Slices, each with its runtime evidence and its own PR: (1) `vesper-proxy`: profile, self-check, `--no-sandbox`,
`make sign`, plist change, negative probes, CI verify; (2) `vesperd`, both backends, `--host-root`, its plist and
`--entitlements-file`; (3) the launcher, after its denial-report spike. A slice that cannot pass its suites under
its profile stops and returns here.

Slice 2 (2026-11-23, D-081): `vesperd` checks its command line, makes its socket directory, then applies the
profile before the tokio runtime. Its rules: read the files it names (baseline lock, swagger, surface stub matrix,
launch config), `stat` its socket directory, take `<socket>.lock`, listen on and connect to its socket, read and
connect to the bridge socket, and on the model backend read and write under each `--host-root` (CE-0013). The
`stage` and `store` subcommands get the same rules without a socket. The canary and the home directories a rule may
not cover come from the user database (`getpwuid_r`) as well as `HOME`, and the default temporary root from
`confstr`, so no environment value widens the profile. `cargo test --test vesperd_sandbox` shows seven refusals on each backend.

## References

- D-066 (owner approval), D-065 (programme step E), D-080 (this ADR), TD-010, TD-046, TD-089, SECURITY.md,
  `packaging/macos/`, `scripts/macos/sandbox-spike/run.sh`.
- Reviews on 2026-11-22: an EGM design review and a security council, both "accept with changes"; their changes
  are in this text (`run/egm/ADR-0012-review.md`).
