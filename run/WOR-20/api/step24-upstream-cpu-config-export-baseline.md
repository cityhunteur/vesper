# Step 24 upstream cpu-config export baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /cpu-config` stored state and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/cpu_configuration.rs`
  - `parse_put_cpu_config()` parses the request into `CustomCpuTemplate` and wraps `VmmAction::PutCpuConfiguration`.
- `src/vmm/src/resources.rs`
  - `VmResources::set_custom_cpu_template()` stores the custom template on `machine_config`.
  - `impl From<&VmResources> for VmmConfig` sets `cpu_config: None` when exporting the full VM configuration.
- `src/vmm/src/vmm_config/machine_config.rs`
  - `MachineConfig` uses `skip_serializing_if = "is_none_or_custom_template"` for `cpu_template`, so custom CPU templates are not serialized via `machine-config` either.

## Derived baseline facts

- Preboot `PUT /cpu-config` should accept a valid custom CPU template payload and store CPU configuration state for later use.
- `/vm/config` should omit `cpu-config` even after successful preboot `PUT /cpu-config`.

## Local parity decision

- This slice needs a real behavior fix, not just more harness work: Vesper currently exports `cpu-config` in `/vm/config`, which overclaims pinned Firecracker behavior.
