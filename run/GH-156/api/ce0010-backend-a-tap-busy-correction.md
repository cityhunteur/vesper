# CE-0010: correction of the duplicate-tap capture

- Case: `PUT /network-interfaces/{iface_id}#duplicate_host_dev_name_preboot`.
- Earlier capture (WOR-20 step 62): derived from source, not from a run, with glibc's `strerror(EBUSY)` text
  "Device or resource busy (os error 16)".
- Run (2026-11-13, `make systemvm-replay-suite`, branch `ce0010-vesperd-taps`): the pinned release binary
  `firecracker-v1.16.0-aarch64` (systemvm/archives.yaml), opening `tap0` a second time in the microVM's own network
  namespace, answered 400 with "Network config error: Could not create the network device: Open tap device failed:
  Error while creating ifreq structure: Resource busy (os error 16). Invalid TUN/TAP Backend provided by tap0. Check
  our documentation on setting up the network devices."
- Why: Firecracker's release binaries are static musl builds; Rust's `io::Error` text comes from the C library's
  `strerror_r`, and musl names `EBUSY` "Resource busy". A glibc build of the same source would print the old text.
- Correction: the capture, the model constant `NETWORK_TAP_OPEN_BUSY_DETAIL` (`src/api.rs`) and
  `tests/api_contract.rs` now use the musl text of the pinned release binary.
