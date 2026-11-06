# launcher

## Overview

`vesper-vmlauncher` is the thin Swift Virtualization.framework helper that
boots one System VM launch config (ADR
[0001](../docs/design-docs/adr/0001-systemvm-launcher-swift-helper.md)). It is
a Swift Package (`Package.swift`, tools version 5.9, macOS 13+, Swift language
mode 5) with a single executable target in `Sources/vesper-vmlauncher/main.swift`.
Policy lives in Rust (`src/systemvm.rs`) and in the launch config; the helper
only maps the config onto `VZVirtualMachineConfiguration`, watches the
console, drives the optional console probe the config describes, serves the
optional vsock bridge socket the config names, and reports.

## Interface

- Build and sign: `make vmlauncher-build` (`scripts/macos/build-vmlauncher.sh`), output `build/vmlauncher/vesper-vmlauncher` ad-hoc signed with `packaging/macos/vesper-vmlauncher.entitlements`.
- CLI: `vesper-vmlauncher [--config PATH] [--ready-pattern TEXT] [--timeout-seconds N] [--keep-running] [--skip-probe]`.
  - `--config` default `build/systemvm/launch-config.json`; decoded as `LaunchConfig { kernel_path, rootfs_path?, initrd_path?, boot_media_path?, kernel_cmdline, ready_pattern?, max_boot_ms?, probe?: { login_user, shell_prompt, commands[], success_pattern, shutdown_command }, network_mode, cpu_count, memory_mib, console_log_path }` (the JSON written by `systemvmctl launch-config`). `max_boot_ms` is ignored here; `systemvmctl boot-smoke` enforces it.
  - `--ready-pattern` overrides `ready_pattern` from the config; the fallback is `login:`.
  - `--timeout-seconds` default `60`, must be positive; covers the boot and the whole probe.
  - `--keep-running`: run the probe without its shutdown command (or stop after the ready pattern when there is no probe) and keep the VM up until the guest powers off or the launcher receives SIGTERM (it then stops the VM and exits 0 when it was ready).
  - `--skip-probe`: ignore the config's `probe` and stop right after the ready pattern.
- stdout: one JSON line per status change, `{"status":..., "elapsed_ms":..., "ready_pattern":..., "console_log":...}`: `bridge-listening` (with `"bridge":PATH`) right after start when `vsock_bridge_path` is set, `ready`, then for a probe `probe-ok` or `probe-failed`, `running` (keep-running), `terminating` (SIGTERM), then `stopped`; or `timeout`, `probe-timeout`, `start-failed`, `stopped-before-ready`, `stopped-before-probe`, `error`.
- vsock bridge (`vsock_bridge_path` in the config): a Unix stream socket created with mode 0600 (parent directory created, stale file unlinked). Per client the launcher reads one line of at most 64 bytes; it must be `CONNECT <port>`; the launcher then connects to that guest vsock port through the VM's `VZVirtioSocketDevice` and relays bytes in both directions with 64 KiB buffers until either side closes (half-close is forwarded with `shutdown(SHUT_WR)`). No reply line is sent, matching `vesper-proxy`'s preface. Bad prefaces and failed guest connects are logged to stderr and the client is closed. The socket is unlinked when the VM stops or the launcher is terminated.
- Probe sequence after `ready`: type `login_user`; wait for `shell_prompt`; for each of `commands` type it and wait for `shell_prompt`; after the last prompt require `success_pattern` in the console output produced since the last command was typed; type `shutdown_command` and wait for the guest to stop. Input is typed only after the awaited pattern appeared.
- Exit codes: `0` ready pattern seen and VM stopped (probe, if any, succeeded); `1` VM failed to start, stopped before ready, stopped before the probe succeeded, or stopped with an error; `2` timeout before ready or before the probe finished; `3` bad arguments, unreadable or invalid config, missing file, unsupported `network_mode`, or `VZVirtualMachine.isSupported == false`; `4` probe commands finished without the success pattern.

## Invariants

- The VM always has one `VZVirtioSocketDeviceConfiguration`; the bridge is served only when the config names a path.
- Block devices are attached in order: `boot_media_path` read-only first, then `rootfs_path` writable, so a kernel command line can address them as `/dev/vda` and `/dev/vdb`.
- `network_mode` must be `nat` (one `VZVirtioNetworkDeviceConfiguration` with `VZNATNetworkDeviceAttachment`) or `none`; anything else exits 3 before the VM is created.
- `cpu_count` and `memory_mib` are clamped to the framework's allowed ranges; the config is `validate()`d before start.
- The console is a `VZVirtioConsoleDeviceSerialPortConfiguration` (guest `hvc0`) backed by pipes; every byte is appended to `console_log_path` (created or truncated at start) and the last 64 KiB is searched for the ready pattern.
- The helper never changes the launch config, never downloads anything, and has no defaults for image paths.
- Without a probe (and without `--keep-running`) the VM is stopped with `VZVirtualMachine.stop` right after the ready pattern; the guest is not shut down cleanly. With a probe the guest shuts itself down via `shutdown_command` and the launcher exits from `guestDidStop`.
- The console echoes typed input, so the success pattern is searched only in output after the last command; `ProbeSettings::validate` in Rust rejects a pattern that appears verbatim in a typed line.

## Side Effects

- Creates the parent directory of `console_log_path` and truncates that file.
- Starts a Virtualization.framework VM with NAT networking (host-side NAT interface managed by macOS), an entropy device and a virtio socket device; the only host socket is the bridge Unix socket when configured (owner-only). SIGTERM is caught (`SIG_IGN` plus a dispatch signal source).
- Prints one JSON line to stdout and error text to stderr.

## Verification

- Build: `make vmlauncher-build` (macOS only; also run by the `launcher-build` job of `.github/workflows/systemvm-image.yml`).
- Boot: `make systemvm-fetch systemvm-launch-config vmlauncher-build systemvm-boot-smoke` (see `docs/process/systemvm-bringup.md`); each boot logs in, obtains a DHCP lease from the NAT, pings the gateway and powers off.
- Bridge: `make systemvm-up`, `make systemvm-control-suite`, `make systemvm-down` (GH #13 checklist through the real bridge).
- No unit tests; the behaviour is covered by the boot-smoke report and the console log under `run/systemvm/`.
