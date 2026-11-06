# launcher

## Overview

`vesper-vmlauncher` is the thin Swift Virtualization.framework helper that
boots one System VM launch config (ADR
[0001](../docs/design-docs/adr/0001-systemvm-launcher-swift-helper.md)). It is
a Swift Package (`Package.swift`, tools version 5.9, macOS 13+, Swift language
mode 5) with a single executable target in `Sources/vesper-vmlauncher/main.swift`.
Policy lives in Rust (`src/systemvm.rs`) and in the launch config; the helper
only maps the config onto `VZVirtualMachineConfiguration`, watches the
console, and reports.

## Interface

- Build and sign: `make vmlauncher-build` (`scripts/macos/build-vmlauncher.sh`), output `build/vmlauncher/vesper-vmlauncher` ad-hoc signed with `packaging/macos/vesper-vmlauncher.entitlements`.
- CLI: `vesper-vmlauncher [--config PATH] [--ready-pattern TEXT] [--timeout-seconds N] [--keep-running]`.
  - `--config` default `build/systemvm/launch-config.json`; decoded as `LaunchConfig { kernel_path, rootfs_path?, initrd_path?, boot_media_path?, kernel_cmdline, ready_pattern?, network_mode, cpu_count, memory_mib, console_log_path }` (the JSON written by `systemvmctl launch-config`).
  - `--ready-pattern` overrides `ready_pattern` from the config; the fallback is `login:`.
  - `--timeout-seconds` default `60`, must be positive.
  - `--keep-running`: after the ready pattern, keep the VM up until the guest stops.
- stdout: exactly one JSON line `{"status":..., "elapsed_ms":..., "ready_pattern":..., "console_log":...}` with status `ready`, `timeout`, `start-failed`, `stopped`, `stopped-before-ready` or `error`.
- Exit codes: `0` ready pattern seen (VM stopped) or guest stopped after ready; `1` VM failed to start, stopped before ready, or stopped with an error; `2` timeout; `3` bad arguments, unreadable or invalid config, missing file, unsupported `network_mode`, or `VZVirtualMachine.isSupported == false`.

## Invariants

- Block devices are attached in order: `boot_media_path` read-only first, then `rootfs_path` writable, so a kernel command line can address them as `/dev/vda` and `/dev/vdb`.
- `network_mode` must be `nat` (one `VZVirtioNetworkDeviceConfiguration` with `VZNATNetworkDeviceAttachment`) or `none`; anything else exits 3 before the VM is created.
- `cpu_count` and `memory_mib` are clamped to the framework's allowed ranges; the config is `validate()`d before start.
- The console is a `VZVirtioConsoleDeviceSerialPortConfiguration` (guest `hvc0`) backed by pipes; every byte is appended to `console_log_path` (created or truncated at start) and the last 64 KiB is searched for the ready pattern.
- The helper never changes the launch config, never downloads anything, and has no defaults for image paths.
- After the ready pattern (without `--keep-running`) the VM is stopped with `VZVirtualMachine.stop` and the process exits; the guest is not shut down cleanly.

## Side Effects

- Creates the parent directory of `console_log_path` and truncates that file.
- Starts a Virtualization.framework VM with NAT networking (host-side NAT interface managed by macOS) and an entropy device; no sockets or ports are opened on the host.
- Prints one JSON line to stdout and error text to stderr.

## Verification

- Build: `make vmlauncher-build` (macOS only; also run by the `launcher-build` job of `.github/workflows/systemvm-image.yml`).
- Boot: `make systemvm-fetch systemvm-launch-config systemvm-boot-smoke` (see `docs/process/systemvm-bringup.md`).
- No unit tests; the behaviour is covered by the boot-smoke report and the console log under `run/systemvm/`.
