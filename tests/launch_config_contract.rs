//! TD-044: the launch config's JSON contract between `systemvmctl` (Rust, `src/systemvm.rs`) and `vesper-vmlauncher`
//! (Swift). This test holds `testdata/contracts/launch-config.full.json` equal to a config with every field set;
//! `scripts/macos/launcher-config-contract.sh` (CI's `launcher-build` job) has the launcher decode that file and
//! print it back, and the two must be equal. A renamed or dropped field fails one side or the other.
//! `VESPER_UPDATE_GOLDEN=1 cargo test --test launch_config_contract` rewrites the file.

use vesper::systemvm::{LaunchConfig, ProbeSettings, SharedDirectory};

const GOLDEN: &str = "testdata/contracts/launch-config.full.json";

fn full() -> LaunchConfig {
    LaunchConfig {
        kernel_path: "/k/vmlinuz".into(),
        rootfs_path: Some("/k/rootfs.img".into()),
        initrd_path: Some("/k/initramfs".into()),
        boot_media_path: Some("/k/boot.iso".into()),
        kernel_cmdline: "console=hvc0 quiet".into(),
        ready_pattern: "login:".into(),
        max_boot_ms: Some(9000),
        probe: Some(ProbeSettings {
            login_user: "root".into(),
            shell_prompt: "~# ".into(),
            commands: vec!["uname -a".into(), "echo ok".into()],
            success_pattern: "ok".into(),
            shutdown_command: "poweroff".into(),
        }),
        vsock_bridge_path: Some("/run/vsock/3.sock".into()),
        agent_port: Some(1024),
        nested_virtualization: Some("auto".into()),
        network_mode: "nat".into(),
        cpu_count: 3,
        memory_mib: 3072,
        console_log_path: "/run/console.log".into(),
        shared_directories: vec![
            SharedDirectory {
                tag: "share0".into(),
                host_path: "/Users/a".into(),
                read_only: false,
                watch: true,
            },
            SharedDirectory {
                tag: "share1".into(),
                host_path: "/Users/b".into(),
                read_only: true,
                watch: false,
            },
            SharedDirectory {
                tag: "share2".into(),
                host_path: "/Users/c".into(),
                read_only: false,
                watch: false,
            },
        ],
        data_disk_path: Some("/Users/a/data.img".into()),
        bridge_allowed_ports: vec![1024, 2375, 2400],
    }
}

#[test]
fn the_golden_launch_config_has_every_field_set() {
    let value = serde_json::to_value(full()).unwrap();
    if std::env::var_os("VESPER_UPDATE_GOLDEN").is_some() {
        std::fs::write(GOLDEN, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
    }
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(GOLDEN).unwrap()).unwrap();
    assert_eq!(
        value, golden,
        "regenerate with VESPER_UPDATE_GOLDEN=1 and update the launcher's LaunchConfig"
    );
    // Every field is present, so a field the launcher drops shows as a difference on macOS.
    let keys = value.as_object().unwrap().len();
    assert_eq!(
        keys, 18,
        "a new LaunchConfig field: set it in full() and add it to the launcher's struct"
    );
}
