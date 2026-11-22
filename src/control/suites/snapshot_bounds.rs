//! Steps of the snapshot suite that show where snapshots meet the System VM's limits: the data disk's root reserve
//! (TD-053, D-076) and the kernel's missing userfaultfd (CE-0011).

use super::launchapi::first;
use super::vesperd::{Client, VesperdRun};
use crate::control::Session;
use std::path::Path;

/// CE-0011: the System VM's kernel has no userfaultfd (CONFIG_USERFAULTFD is not set), so a load with the Uffd memory
/// backend gets Firecracker's own refusal. Firecracker ends after a failed load, so this runs in a vesperd of its own.
pub(super) fn uffd_load_refused(
    v: &VesperdRun,
    id: &str,
    flags: &[String],
) -> Result<((), String), String> {
    let (vu, _, _) = v.start_with(id, "u.sock", flags)?;
    let socket = vu.socket.clone();
    let r = Client::connect(&socket).and_then(|mut api| {
        api.call(
            "PUT",
            "/snapshot/load",
            Some(r#"{"snapshot_path":"/snap.vmstate","mem_backend":{"backend_type":"Uffd","backend_path":"/uffd.sock"},"resume_vm":false}"#),
        )
    });
    drop(vu);
    let _ = std::fs::remove_file(&socket);
    let r = r?;
    if r.status != 400 || !r.body.contains("uffd") {
        return Err(format!(
            "PUT /snapshot/load (Uffd): {} {}",
            r.status, r.body
        ));
    }
    Ok((
        (),
        format!(
            "PUT /snapshot/load (Uffd; no userfaultfd in the kernel) -> 400 {}",
            r.body
        ),
    ))
}

/// TD-053 (D-076): Firecracker writes as a non-root uid, so the data disk's root-reserved blocks stop its snapshot
/// with its own ENOSPC, while root (dockerd) can still write. 64 MiB is left to non-root.
pub(super) fn jail_write_stops_at_root_reserve(
    control: &mut Session,
    socket: &Path,
) -> Result<((), String), String> {
    let mut api = Client::connect(socket)?;
    let (_, set) = control.data(&["test-root-reserve", "67108864"])?;
    let full = api.call(
        "PUT",
        "/snapshot/create",
        Some(r#"{"snapshot_type":"Full","snapshot_path":"/snap3.vmstate","mem_file_path":"/snap3.mem"}"#),
    );
    let (_, root) = control.data(&["test-root-reserve", "rootwrite"])?;
    let (_, back) = control.data(&["test-root-reserve", "default"])?;
    let full = full?;
    if full.status != 400 || !full.body.contains("No space left on device") {
        return Err(format!(
            "{}; create past the reserve: {} {}",
            first(&set),
            full.status,
            full.body
        ));
    }
    if first(&root) != "ROOT-RESERVE rootwrite=ok"
        || !first(&back).starts_with("ROOT-RESERVE blocks=")
    {
        return Err(format!("{}; {}", first(&root), first(&back)));
    }
    Ok((
        (),
        format!(
            "{}; PUT /snapshot/create -> 400 {}; {}; {}",
            first(&set),
            full.body,
            first(&root),
            first(&back)
        ),
    ))
}
