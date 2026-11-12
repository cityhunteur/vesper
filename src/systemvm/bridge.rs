//! The vsock bridge allow-list (TD-027, ADR 0007, decision D-022).
//!
//! `vesper-vmlauncher` serves a Unix socket on macOS through which a client names a System VM vsock port with a
//! `CONNECT <port>` preface. The launcher connects only to the ports listed in the launch config and refuses every
//! other port, so a same-user process on macOS cannot reach a System VM listener that was never meant for it.

use super::LaunchConfig;

/// Docker Engine relay in the System VM (`vesper-docker-relay`).
pub const DOCKER_RELAY_PORT: u32 = 2375;
/// Published-port relay (`vesper-port-relay`, ADR 0005).
pub const PORT_RELAY_PORT: u32 = 2400;
/// Share notification sink (`vesper-share-notify`, GH #150).
pub const SHARE_NOTIFY_PORT: u32 = 2401;

/// The ports the owner approved (D-022): the agent's, the Docker relay, the port relay and the share sink.
pub fn default_bridge_ports(agent_port: u32) -> Vec<u32> {
    let mut ports = vec![agent_port, DOCKER_RELAY_PORT, PORT_RELAY_PORT, SHARE_NOTIFY_PORT];
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Sets the allow-list on a config that serves a bridge; a config without a bridge keeps an empty list.
pub fn apply_bridge_allow_list(config: &mut LaunchConfig) {
    if config.vsock_bridge_path.is_some() {
        config.bridge_allowed_ports = default_bridge_ports(config.agent_port.unwrap_or(1024));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemvm::{default_launch_config, LaunchInputs};

    fn config(bridge: Option<&str>) -> LaunchConfig {
        default_launch_config(&LaunchInputs {
            kernel_path: "/k",
            rootfs_path: None,
            initrd_path: Some("/i"),
            boot_media_path: None,
            kernel_cmdline: None,
            ready_pattern: None,
            max_boot_ms: None,
            probe: None,
            vsock_bridge_path: bridge,
            agent_port: Some(1024),
            nested_virtualization: None,
            console_log_path: "/c.log",
        })
        .expect("config")
    }

    #[test]
    fn the_approved_ports_and_nothing_else() {
        assert_eq!(default_bridge_ports(1024), vec![1024, 2375, 2400, 2401]);
        assert_eq!(default_bridge_ports(2375), vec![2375, 2400, 2401]);
    }

    #[test]
    fn a_bridge_gets_the_allow_list_and_no_bridge_gets_none() {
        let mut with = config(Some("/run/3.sock"));
        apply_bridge_allow_list(&mut with);
        assert_eq!(with.bridge_allowed_ports, vec![1024, 2375, 2400, 2401]);
        let json = serde_json::to_value(&with).expect("json");
        assert_eq!(json["bridge_allowed_ports"], serde_json::json!([1024, 2375, 2400, 2401]));
        let mut without = config(None);
        apply_bridge_allow_list(&mut without);
        assert!(without.bridge_allowed_ports.is_empty());
    }

    #[test]
    fn the_repository_lock_yields_the_allow_list() {
        let lock = crate::systemvm::load_image_lock(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/specs/systemvm-image.lock.yaml"))
            .expect("lock");
        let dir = tempfile::tempdir().expect("tempdir");
        if let Some(overlay) = &lock.overlay {
            std::fs::write(dir.path().join(overlay.file.trim()), b"x").expect("overlay stub");
        }
        let config = crate::systemvm::launch_config_from_lock(&lock, dir.path(), "/c.log").expect("config");
        assert_eq!(config.bridge_allowed_ports, vec![1024, 2375, 2400, 2401]);
    }
}
