//! Persistent data disk for the System VM (GH #164, TD-025).
//!
//! The launcher attaches a sparse disk image from macOS as a writable virtio block device after the boot media
//! and the rootfs. The System VM finds it through `vesper_data=/dev/vdX` on the kernel command line, formats it
//! as ext4 on first use and mounts it for containerd, dockerd and the Vesper tools
//! (`systemvm/overlay/usr/local/bin/vesper-data`).

use super::LaunchConfig;

/// ext4 label the System VM gives the disk on first use; a disk with any other label is never formatted.
pub const DATA_DISK_LABEL: &str = "vesper-data";

/// Guest device name of the data disk: the next virtio block device after the boot media and the rootfs.
pub fn data_disk_device(config: &LaunchConfig) -> String {
    let before = usize::from(config.boot_media_path.is_some()) + usize::from(config.rootfs_path.is_some());
    format!("/dev/vd{}", (b'a' + before as u8) as char)
}

/// Adds the data disk at `path` (absolute, on macOS) to `config` and names its device on the kernel command line.
pub fn attach_data_disk(config: &mut LaunchConfig, path: &str) -> Result<(), String> {
    let path = path.trim();
    if !path.starts_with('/') {
        return Err(format!("data disk path must be absolute: {path:?}"));
    }
    if config.data_disk_path.is_some() {
        return Err("a data disk is already attached".to_string());
    }
    let device = data_disk_device(config);
    config.data_disk_path = Some(path.to_string());
    config.kernel_cmdline = format!("{} vesper_data={device}", config.kernel_cmdline.trim_end());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemvm::{default_launch_config, LaunchInputs};

    fn config(boot_media: Option<&str>, rootfs: Option<&str>) -> LaunchConfig {
        default_launch_config(&LaunchInputs {
            kernel_path: "/k",
            rootfs_path: rootfs,
            initrd_path: Some("/i"),
            boot_media_path: boot_media,
            kernel_cmdline: Some("console=hvc0 modules=virtio_blk"),
            ready_pattern: None,
            max_boot_ms: None,
            probe: None,
            vsock_bridge_path: None,
            agent_port: None,
            nested_virtualization: None,
            console_log_path: "/c.log",
        })
        .expect("config")
    }

    #[test]
    fn data_disk_follows_boot_media_and_rootfs() {
        assert_eq!(data_disk_device(&config(None, None)), "/dev/vda");
        assert_eq!(data_disk_device(&config(Some("/m.iso"), None)), "/dev/vdb");
        assert_eq!(data_disk_device(&config(Some("/m.iso"), Some("/r.ext4"))), "/dev/vdc");
    }

    #[test]
    fn attach_names_the_device_on_the_command_line() {
        let mut c = config(Some("/m.iso"), None);
        attach_data_disk(&mut c, "/Users/u/data.img").expect("attach");
        assert_eq!(c.data_disk_path.as_deref(), Some("/Users/u/data.img"));
        assert_eq!(c.kernel_cmdline, "console=hvc0 modules=virtio_blk vesper_data=/dev/vdb");
        let json = serde_json::to_value(&c).expect("json");
        assert_eq!(json["data_disk_path"], "/Users/u/data.img");
    }

    #[test]
    fn attach_rejects_relative_paths_and_a_second_disk() {
        let mut c = config(Some("/m.iso"), None);
        assert!(attach_data_disk(&mut c, "run/data.img").is_err());
        attach_data_disk(&mut c, "/d1.img").expect("first");
        assert!(attach_data_disk(&mut c, "/d2.img").is_err());
    }

    #[test]
    fn configs_without_the_field_still_parse() {
        let mut json = serde_json::to_value(config(None, None)).expect("json");
        json.as_object_mut().expect("object").remove("data_disk_path");
        let parsed: LaunchConfig = serde_json::from_value(json).expect("parse");
        assert!(parsed.data_disk_path.is_none());
    }
}
