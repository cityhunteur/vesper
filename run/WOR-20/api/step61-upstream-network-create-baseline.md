# Step 61 upstream network create baseline

- pinned Firecracker revision: `083c4cf69281178acc910f7f857f88e69c51d3b5`
- scope: preboot `PUT /network-interfaces/{iface_id}` stored-config and `/vm/config` export behavior

## Source packet

- `src/firecracker/src/api_server/request/net.rs`
  - `parse_put_net()` deserializes the request into the network-interface configuration model.
  - `parse_put_net()` rejects path/body id mismatches with the formatted string already captured in step 15.
  - accepted requests continue into the preboot network-device insertion path rather than the runtime update path.
- `src/vmm/src/rpc_interface.rs`
  - network-interface creation is handled on the preboot resource-configuration path, while runtime mutation uses the separate update action already captured in step 15.
- `src/vmm/src/resources.rs`
  - full configuration export includes configured network interfaces in `/vm/config`.

## Derived baseline facts

- Preboot `PUT /network-interfaces/{iface_id}` should accept a valid request body and return `204 No Content`.
- After a successful preboot create request, `/vm/config` should include the stored network-interface configuration.

## Local parity decision

- This slice can truthfully tighten the harness by adding explicit full-router proof for the existing network-interface create and export behavior, then seeding the final pending route in the generated behavior matrix without changing request handling.
