use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, to_vec};
use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::net::Ipv4Addr;
use std::sync::{Arc, RwLock};

use crate::api_matrix::{
    RouteSemantics, SurfaceStubMatrix, generate_surface_stub_matrix, validate_surface_stub_matrix,
};

const DEFAULT_APP_NAME: &str = "Firecracker";
const DEFAULT_INSTANCE_ID: &str = "anonymous-instance";
const DEFAULT_MEM_SIZE_MIB: usize = 128;
const DEFAULT_MEMORY_HOTPLUG_BLOCK_SIZE_MIB: usize = 2;
const DEFAULT_MEMORY_HOTPLUG_SLOT_SIZE_MIB: usize = 128;
const MAX_SUPPORTED_VCPUS: u8 = 32;
const ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE: &str =
    "The requested operation is not supported after starting the microVM.";
const ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE: &str =
    "The requested operation is not supported before starting the microVM.";
const SEND_CTRL_ALT_DEL_AARCH64_MESSAGE: &str = "SendCtrlAltDel does not supported on aarch64.";
const MISSING_KERNEL_CONFIG_MESSAGE: &str = "Cannot start microvm without kernel configuration.";
const INVALID_VSOCK_GUEST_CID_MESSAGE: &str = "Guest CID must be greater than or equal to 3.";
const METRICS_ALREADY_INITIALIZED_MESSAGE: &str = "Cannot initialize the metrics system due to bad user input: Reinitialization of metrics not allowed.";
const SNAPSHOT_CREATE_REQUIRES_PAUSED_MESSAGE: &str =
    "operation PUT /snapshot/create is not allowed while instance state is running";
const SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE: &str =
    "Load snapshot can only be called before configuring resources other than logger and metrics.";
const SNAPSHOT_LOAD_MISSING_MEMORY_SOURCE_MESSAGE: &str = "An error occurred when deserializing the json body of a request: missing field: either `mem_backend` or `mem_file_path` is required.";
const SNAPSHOT_LOAD_TOO_MANY_MEMORY_SOURCES_MESSAGE: &str = "An error occurred when deserializing the json body of a request: too many fields: either `mem_backend` or `mem_file_path` exclusively is required.";
const BALLOON_NOT_CONFIGURED_MESSAGE: &str = "Balloon config error: No balloon device found.";
const BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE: &str =
    "Internal VMM error: Failed perform action on device: Device not found";
const DRIVE_ID_PATH_MISMATCH_MESSAGE: &str =
    "The id from the path does not match the id from the body!";
const DRIVE_ROOT_PMEM_CONFLICT_MESSAGE: &str = "Drive config error: Attempt to add block as a root device while the root device defined as a pmem device";
const DRIVE_ROOT_ALREADY_EXISTS_MESSAGE: &str =
    "Drive config error: A root block device already exists!";
const DRIVE_UPDATE_NOT_FOUND_MESSAGE: &str = "Drive config error: Unable to patch the block device: Failed perform action on device: Device not found Please verify the request arguments.";
const NETWORK_UPDATE_NOT_FOUND_MESSAGE: &str = "Network config error: Unable to update the net device: Failed perform action on device: Device not found";
const NETWORK_TAP_OPEN_BUSY_DETAIL: &str = "Device or resource busy (os error 16)";
const PMEM_ID_PATH_MISMATCH_MESSAGE: &str =
    "The id from the path does not match the id from the body!";
const PMEM_ROOT_BLOCK_CONFLICT_MESSAGE: &str = "Pmem device error: Attempt to add pmem as a root device while the root device defined as a block device";
const PMEM_ROOT_ALREADY_EXISTS_MESSAGE: &str =
    "Pmem device error: A root pmem device already exist";
const PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE: &str =
    "Pmem device error: Unable to create the virtio-pmem device: Error backing file size is 0";
const BALLOON_TOO_MANY_PAGES_REQUESTED_MESSAGE: &str =
    "Balloon config error: Amount of pages requested is too large.";
const BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE: &str =
    "Balloon update error: Failed perform action on device: Device not found";
const BALLOON_HINTING_NOT_ENABLED_MESSAGE: &str =
    "Balloon update error: Balloon: Attempting to use hinting when not enabled";
const BALLOON_HINTING_STOP_COMMAND: u32 = 0;
const BALLOON_HINTING_DONE_COMMAND: u32 = 1;
const BALLOON_HINTING_FIRST_RUN_COMMAND: u32 = 2;
const BALLOON_STATS_DISABLED_MESSAGE: &str =
    "Internal VMM error: Balloon: Received stats query when stats are disabled.";
const BALLOON_STATS_STATE_CHANGE_MESSAGE: &str =
    "Balloon update error: Balloon: Statistics cannot be enabled/disabled after activation.";
const MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE: &str =
    "The memory size (MiB) is smaller than the previously set balloon device target size.";
const MEMORY_HOTPLUG_NOT_CONFIGURED_MESSAGE: &str = "Memory hotplug device is not configured.";
const INVALID_MEMORY_HOTPLUG_BLOCK_SIZE_MESSAGE: &str =
    "The block size of the hotpluggable memory device must be at least 2 MiB and a power of 2.";
const INVALID_MEMORY_HOTPLUG_SLOT_SIZE_MESSAGE: &str = "The slot size of the hotpluggable memory device must be at least 128 MiB and a multiple of the block size.";
const INVALID_MEMORY_HOTPLUG_TOTAL_SIZE_MESSAGE: &str = "The total size of the hotpluggable memory device must be at least the slot size and a multiple of the slot size.";
const INVALID_MEMORY_HOTPLUG_REQUESTED_SIZE_MESSAGE: &str = "The requested size of the hotpluggable memory device must be a multiple of the block size and no larger than the total size.";
const MMDS_NOT_INITIALIZED_MESSAGE: &str = "The MMDS data store is not initialized.";
const MMDS_DATA_STORE_LIMIT_EXCEEDED_MESSAGE: &str = "The MMDS patch request doesn't fit.";
const MMDS_EMPTY_NETWORK_INTERFACE_LIST_MESSAGE: &str =
    "The list of network interface IDs that allow forwarding MMDS requests is empty.";
const MMDS_INVALID_IPV4_ADDRESS_MESSAGE: &str = "The MMDS IPv4 address is not link local.";
const MMDS_INVALID_NETWORK_INTERFACE_ID_MESSAGE: &str = "The list of network interface IDs provided contains at least one ID that does not correspond to any existing network interface.";
const MMDS_DATA_STORE_LIMIT_BYTES: usize = 51_200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub fault_message: String,
    pub code: String,
    pub path: String,
    pub method: String,
    pub baseline_firecracker_revision: String,
}

#[derive(Debug, Clone)]
pub struct OperationStatus {
    pub status_code: StatusCode,
    pub code: String,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VmState {
    PreBoot,
    Running,
    Paused,
}

impl VmState {
    fn guard_str(self) -> &'static str {
        match self {
            VmState::PreBoot => "preboot",
            VmState::Running => "running",
            VmState::Paused => "paused",
        }
    }

    fn instance_str(self) -> &'static str {
        match self {
            VmState::PreBoot => "Not started",
            VmState::Running => "Running",
            VmState::Paused => "Paused",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouteSpec {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone)]
struct RouteTemplate {
    method: String,
    template: String,
    parts: Vec<RoutePart>,
}

#[derive(Debug, Clone)]
enum RoutePart {
    Literal(String),
    Param,
}

#[derive(Debug, Clone)]
struct RouteMatch<'a> {
    template: &'a RouteTemplate,
    path: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum HugePages {
    #[default]
    #[serde(rename = "None")]
    None,
    #[serde(rename = "2M")]
    Huge2M,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum CpuTemplate {
    #[cfg(target_arch = "x86_64")]
    C3,
    #[cfg(target_arch = "x86_64")]
    T2,
    #[cfg(target_arch = "x86_64")]
    T2S,
    #[cfg(target_arch = "x86_64")]
    T2CL,
    #[cfg(target_arch = "x86_64")]
    T2A,
    #[cfg(target_arch = "aarch64")]
    V1N1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
enum CpuTemplateRequest {
    None,
    #[cfg(target_arch = "x86_64")]
    C3,
    #[cfg(target_arch = "x86_64")]
    T2,
    #[cfg(target_arch = "x86_64")]
    T2S,
    #[cfg(target_arch = "x86_64")]
    T2CL,
    #[cfg(target_arch = "x86_64")]
    T2A,
    #[cfg(target_arch = "aarch64")]
    V1N1,
}

impl CpuTemplateRequest {
    fn into_stored(self) -> Option<CpuTemplate> {
        match self {
            CpuTemplateRequest::None => None,
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::C3 => Some(CpuTemplate::C3),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2 => Some(CpuTemplate::T2),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2S => Some(CpuTemplate::T2S),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2CL => Some(CpuTemplate::T2CL),
            #[cfg(target_arch = "x86_64")]
            CpuTemplateRequest::T2A => Some(CpuTemplate::T2A),
            #[cfg(target_arch = "aarch64")]
            CpuTemplateRequest::V1N1 => Some(CpuTemplate::V1N1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct MachineConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_template: Option<CpuTemplate>,
    smt: bool,
    mem_size_mib: usize,
    track_dirty_pages: bool,
    vcpu_count: u8,
    huge_pages: HugePages,
}

impl Default for MachineConfig {
    fn default() -> Self {
        Self {
            cpu_template: None,
            smt: false,
            mem_size_mib: DEFAULT_MEM_SIZE_MIB,
            track_dirty_pages: false,
            vcpu_count: 1,
            huge_pages: HugePages::None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum LoggerLevel {
    Off,
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl<'de> Deserialize<'de> for LoggerLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let key = String::deserialize(deserializer)?;
        match key.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "trace" => Ok(Self::Trace),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warn" | "warning" => Ok(Self::Warn),
            "error" => Ok(Self::Error),
            _ => Err(D::Error::custom("Invalid LevelFilter")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoggerConfig {
    log_path: Option<String>,
    level: LoggerLevel,
    show_level: bool,
    show_log_origin: bool,
    module: Option<String>,
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            log_path: None,
            level: LoggerLevel::Info,
            show_level: false,
            show_log_origin: false,
            module: None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LoggerConfigRequest {
    #[serde(default)]
    log_path: Option<String>,
    #[serde(default)]
    level: Option<LoggerLevel>,
    #[serde(default)]
    show_level: Option<bool>,
    #[serde(default)]
    show_log_origin: Option<bool>,
    #[serde(default)]
    module: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsConfig {
    metrics_path: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SerialConfig {
    serial_out_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct TokenBucket {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    one_time_burst: Option<u64>,
    refill_time: u64,
    size: u64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct RateLimiterConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    bandwidth: Option<TokenBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    ops: Option<TokenBucket>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct EntropyDeviceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    rate_limiter: Option<RateLimiterConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
enum DriveCacheType {
    #[default]
    Unsafe,
    Writeback,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
enum DriveIoEngine {
    Sync,
    Async,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct DriveConfigRequest {
    drive_id: String,
    is_root_device: bool,
    #[serde(default)]
    partuuid: Option<String>,
    #[serde(default)]
    cache_type: DriveCacheType,
    #[serde(default)]
    is_read_only: Option<bool>,
    #[serde(default)]
    path_on_host: Option<String>,
    #[serde(default)]
    rate_limiter: Option<RateLimiterConfig>,
    #[serde(default)]
    io_engine: Option<DriveIoEngine>,
    #[serde(default)]
    socket: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PartialDriveConfigRequest {
    drive_id: String,
    #[serde(default)]
    path_on_host: Option<String>,
    #[serde(default)]
    rate_limiter: Option<RateLimiterConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum CpuidRegister {
    Eax,
    Ebx,
    Ecx,
    Edx,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct CpuidRegisterModifier {
    register: CpuidRegister,
    bitmap: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct CpuidLeafModifier {
    leaf: String,
    subleaf: String,
    flags: i32,
    modifiers: Vec<CpuidRegisterModifier>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct MsrModifier {
    addr: String,
    bitmap: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ArmRegisterModifier {
    addr: String,
    bitmap: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct VcpuFeatureModifier {
    index: i32,
    bitmap: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct CpuConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    kvm_capabilities: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    cpuid_modifiers: Option<Vec<CpuidLeafModifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    msr_modifiers: Option<Vec<MsrModifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    reg_modifiers: Option<Vec<ArmRegisterModifier>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    vcpu_features: Option<Vec<VcpuFeatureModifier>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct VsockConfigRequest {
    #[serde(default)]
    vsock_id: Option<String>,
    guest_cid: u32,
    uds_path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct PmemConfigRequest {
    id: String,
    path_on_host: String,
    #[serde(default)]
    root_device: bool,
    #[serde(default)]
    read_only: bool,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct NetworkInterfaceConfigRequest {
    #[serde(default)]
    guest_mac: Option<String>,
    host_dev_name: String,
    iface_id: String,
    #[serde(default)]
    rx_rate_limiter: Option<RateLimiterConfig>,
    #[serde(default)]
    tx_rate_limiter: Option<RateLimiterConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PartialNetworkInterfaceConfigRequest {
    iface_id: String,
    #[serde(default)]
    rx_rate_limiter: Option<RateLimiterConfig>,
    #[serde(default)]
    tx_rate_limiter: Option<RateLimiterConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
enum SnapshotType {
    Full,
    Diff,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SnapshotCreateRequest {
    mem_file_path: String,
    snapshot_path: String,
    #[serde(default)]
    snapshot_type: Option<SnapshotType>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
enum MemoryBackendType {
    File,
    Uffd,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct MemoryBackendRequest {
    backend_type: MemoryBackendType,
    backend_path: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct NetworkOverrideRequest {
    iface_id: String,
    host_dev_name: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SnapshotLoadRequest {
    snapshot_path: String,
    #[serde(default)]
    enable_diff_snapshots: Option<bool>,
    #[serde(default)]
    track_dirty_pages: Option<bool>,
    #[serde(default)]
    mem_file_path: Option<String>,
    #[serde(default)]
    mem_backend: Option<MemoryBackendRequest>,
    #[serde(default)]
    resume_vm: Option<bool>,
    #[serde(default)]
    network_overrides: Option<Vec<NetworkOverrideRequest>>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
enum MmdsVersion {
    #[default]
    V1,
    V2,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct BalloonDeviceConfig {
    amount_mib: u32,
    deflate_on_oom: bool,
    #[serde(default)]
    stats_polling_interval_s: u16,
    #[serde(default)]
    free_page_hinting: bool,
    #[serde(default)]
    free_page_reporting: bool,
}

fn default_mmds_ipv4_address() -> Ipv4Addr {
    Ipv4Addr::new(169, 254, 169, 254)
}

fn network_id_path_mismatch_message(path_iface_id: &str, body_iface_id: &str) -> String {
    format!(
        "The id from the path [{path_iface_id}] does not match the id from the body [{body_iface_id}]!"
    )
}

fn network_guest_mac_in_use_message(guest_mac: &str) -> String {
    format!("Network config error: The MAC address is already in use: {guest_mac}")
}

fn network_tap_open_message(host_dev_name: &str) -> String {
    format!(
        "Network config error: Could not create the network device: Open tap device failed: Error while creating ifreq structure: {NETWORK_TAP_OPEN_BUSY_DETAIL}. Invalid TUN/TAP Backend provided by {host_dev_name}. Check our documentation on setting up the network devices."
    )
}

fn drive_backing_file_access_message(err: &std::io::Error, path: &str) -> String {
    format!(
        "Drive config error: Unable to create the virtio block device: Virtio backend error: Error manipulating the backing file: {err} {path}"
    )
}

fn pmem_backing_file_access_message(err: &std::io::Error) -> String {
    format!(
        "Pmem device error: Unable to create the virtio-pmem device: Error accessing backing file: {err}"
    )
}

fn validate_drive_backing_file(payload: &DriveConfigRequest) -> Result<(), String> {
    let Some(path_on_host) = payload.path_on_host.as_deref() else {
        return Ok(());
    };

    if payload.socket.is_some() {
        return Ok(());
    }

    OpenOptions::new()
        .read(true)
        .write(!payload.is_read_only.unwrap_or(false))
        .open(path_on_host)
        .map_err(|err| drive_backing_file_access_message(&err, path_on_host))?;

    Ok(())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct MmdsConfig {
    #[serde(default)]
    version: MmdsVersion,
    network_interfaces: Vec<String>,
    #[serde(default)]
    ipv4_address: Option<Ipv4Addr>,
    #[serde(default)]
    imds_compat: bool,
}

impl MmdsConfig {
    fn normalized(mut self) -> Self {
        if self.ipv4_address.is_none() {
            self.ipv4_address = Some(default_mmds_ipv4_address());
        }
        self
    }
}

fn default_memory_hotplug_block_size_mib() -> usize {
    DEFAULT_MEMORY_HOTPLUG_BLOCK_SIZE_MIB
}

fn default_memory_hotplug_slot_size_mib() -> usize {
    DEFAULT_MEMORY_HOTPLUG_SLOT_SIZE_MIB
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct MemoryHotplugConfig {
    #[serde(default = "default_memory_hotplug_block_size_mib")]
    block_size_mib: usize,
    #[serde(default = "default_memory_hotplug_slot_size_mib")]
    slot_size_mib: usize,
    total_size_mib: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct MemoryHotplugSizeUpdate {
    requested_size_mib: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BalloonSizeUpdate {
    amount_mib: u32,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BalloonStatsIntervalUpdate {
    stats_polling_interval_s: u16,
}

fn default_acknowledge_on_stop() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BalloonHintingStartRequest {
    #[serde(default = "default_acknowledge_on_stop")]
    acknowledge_on_stop: bool,
}

impl Default for BalloonHintingStartRequest {
    fn default() -> Self {
        Self {
            acknowledge_on_stop: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct BalloonStatsResponse {
    target_pages: u32,
    actual_pages: u32,
    target_mib: u32,
    actual_mib: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    swap_in: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    swap_out: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    major_faults: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    minor_faults: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    free_memory: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_memory: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_memory: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disk_caches: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hugetlb_allocations: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hugetlb_failures: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oom_kill: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alloc_stall: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    async_scan: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_scan: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    async_reclaim: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    direct_reclaim: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct BalloonHintingStatusResponse {
    host_cmd: u32,
    guest_cmd: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BalloonHintingState {
    host_cmd: u32,
    guest_cmd: Option<u32>,
    next_run_cmd: u32,
    acknowledge_on_stop: bool,
}

impl Default for BalloonHintingState {
    fn default() -> Self {
        Self {
            host_cmd: BALLOON_HINTING_STOP_COMMAND,
            guest_cmd: None,
            next_run_cmd: BALLOON_HINTING_FIRST_RUN_COMMAND,
            acknowledge_on_stop: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct MemoryHotplugStatus {
    block_size_mib: usize,
    plugged_size_mib: usize,
    requested_size_mib: usize,
    slot_size_mib: usize,
    total_size_mib: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MemoryHotplugState {
    config: MemoryHotplugConfig,
    plugged_size_mib: usize,
    requested_size_mib: usize,
}

impl MemoryHotplugState {
    fn new(config: MemoryHotplugConfig) -> Self {
        Self {
            config,
            plugged_size_mib: 0,
            requested_size_mib: 0,
        }
    }

    fn status(&self) -> MemoryHotplugStatus {
        MemoryHotplugStatus {
            block_size_mib: self.config.block_size_mib,
            plugged_size_mib: self.plugged_size_mib,
            requested_size_mib: self.requested_size_mib,
            slot_size_mib: self.config.slot_size_mib,
            total_size_mib: self.config.total_size_mib,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct VsockConfig {
    guest_cid: u32,
    uds_path: String,
}

impl From<VsockConfigRequest> for VsockConfig {
    fn from(value: VsockConfigRequest) -> Self {
        Self {
            guest_cid: value.guest_cid,
            uds_path: value.uds_path,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineConfigPutRequest {
    vcpu_count: u8,
    mem_size_mib: usize,
    #[serde(default)]
    smt: bool,
    #[serde(default)]
    cpu_template: Option<CpuTemplateRequest>,
    #[serde(default)]
    track_dirty_pages: bool,
    #[serde(default)]
    huge_pages: HugePages,
}

impl From<MachineConfigPutRequest> for MachineConfig {
    fn from(value: MachineConfigPutRequest) -> Self {
        Self {
            cpu_template: value.cpu_template.and_then(CpuTemplateRequest::into_stored),
            smt: value.smt,
            mem_size_mib: value.mem_size_mib,
            track_dirty_pages: value.track_dirty_pages,
            vcpu_count: value.vcpu_count,
            huge_pages: value.huge_pages,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineConfigPatchRequest {
    #[serde(default)]
    vcpu_count: Option<u8>,
    #[serde(default)]
    mem_size_mib: Option<usize>,
    #[serde(default)]
    smt: Option<bool>,
    #[serde(default)]
    cpu_template: Option<CpuTemplateRequest>,
    #[serde(default)]
    track_dirty_pages: Option<bool>,
    #[serde(default)]
    huge_pages: Option<HugePages>,
}

impl MachineConfigPatchRequest {
    fn is_empty(&self) -> bool {
        self.vcpu_count.is_none()
            && self.mem_size_mib.is_none()
            && self.smt.is_none()
            && self.cpu_template.is_none()
            && self.track_dirty_pages.is_none()
            && self.huge_pages.is_none()
    }
}

#[derive(Debug, Serialize)]
struct InstanceInfoResponse {
    app_name: String,
    id: String,
    state: String,
    vmm_version: String,
}

#[derive(Debug, Serialize)]
struct FirecrackerVersionResponse {
    firecracker_version: String,
}

#[derive(Debug, Serialize)]
struct FullVmConfigurationResponse {
    #[serde(rename = "boot-source")]
    boot_source: BootSourceConfig,
    #[serde(rename = "cpu-config")]
    #[serde(skip_serializing_if = "Option::is_none")]
    cpu_config: Option<CpuConfig>,
    #[serde(rename = "machine-config")]
    machine_config: MachineConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    balloon: Option<BalloonDeviceConfig>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    drives: Vec<DriveConfigRequest>,
    #[serde(rename = "network-interfaces")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    network_interfaces: Vec<NetworkInterfaceConfigRequest>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[serde(default)]
    pmem: Vec<PmemConfigRequest>,
    #[serde(rename = "mmds-config")]
    #[serde(skip_serializing_if = "Option::is_none")]
    mmds_config: Option<MmdsConfig>,
    #[serde(rename = "memory-hotplug")]
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_hotplug: Option<MemoryHotplugConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entropy: Option<EntropyDeviceConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    vsock: Option<VsockConfig>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct BootSourceConfig {
    kernel_image_path: String,
    initrd_path: Option<String>,
    boot_args: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct VmStateChangeRequest {
    state: VmStateChange,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum ActionType {
    FlushMetrics,
    InstanceStart,
    SendCtrlAltDel,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionRequest {
    action_type: ActionType,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum VmStateChange {
    Paused,
    Resumed,
}

#[derive(Debug)]
pub struct AppState {
    routes: Vec<RouteTemplate>,
    matrix: BTreeMap<String, OperationStatus>,
    semantics: BTreeMap<String, RouteSemantics>,
    firecracker_revision: String,
    firecracker_version: String,
    vm_state: RwLock<VmState>,
    boot_source: RwLock<BootSourceConfig>,
    cpu_config: RwLock<Option<CpuConfig>>,
    logger_config: RwLock<LoggerConfig>,
    machine_config: RwLock<MachineConfig>,
    metrics_config: RwLock<Option<MetricsConfig>>,
    serial_config: RwLock<SerialConfig>,
    balloon_config: RwLock<Option<BalloonDeviceConfig>>,
    balloon_hinting_state: RwLock<Option<BalloonHintingState>>,
    drives: RwLock<BTreeMap<String, DriveConfigRequest>>,
    network_interfaces: RwLock<BTreeMap<String, NetworkInterfaceConfigRequest>>,
    mmds_config: RwLock<Option<MmdsConfig>>,
    mmds_data_store: RwLock<Option<Value>>,
    pmem_devices: RwLock<BTreeMap<String, PmemConfigRequest>>,
    memory_hotplug: RwLock<Option<MemoryHotplugState>>,
    entropy_config: RwLock<Option<EntropyDeviceConfig>>,
    vsock_config: RwLock<Option<VsockConfig>>,
}

impl AppState {
    pub fn new(
        routes: &[RouteSpec],
        firecracker_revision: String,
        firecracker_version: String,
    ) -> Result<Self, String> {
        let surface_stub_matrix =
            generate_surface_stub_matrix(routes, &firecracker_revision, &firecracker_version)?;
        Self::new_with_matrix(
            routes,
            firecracker_revision,
            firecracker_version,
            &surface_stub_matrix,
        )
    }

    pub fn new_with_matrix(
        routes: &[RouteSpec],
        firecracker_revision: String,
        firecracker_version: String,
        surface_stub_matrix: &SurfaceStubMatrix,
    ) -> Result<Self, String> {
        if routes.is_empty() {
            return Err("at least one route is required".to_string());
        }
        if firecracker_revision.trim().is_empty() {
            return Err("firecracker revision is required".to_string());
        }
        if firecracker_version.trim().is_empty() {
            return Err("firecracker version is required".to_string());
        }

        let mut unique = HashMap::new();
        let mut templates = Vec::new();
        for route in routes {
            let template = RouteTemplate::new(&route.method, &route.path);
            let key = operation_key(&template.method, &template.template);
            if unique.contains_key(&key) {
                continue;
            }
            unique.insert(key, ());
            templates.push(template);
        }

        validate_surface_stub_matrix(
            surface_stub_matrix,
            routes,
            &firecracker_revision,
            &firecracker_version,
        )?;

        let mut matrix = BTreeMap::new();
        let mut semantics = BTreeMap::new();
        for operation in &surface_stub_matrix.operations {
            let key = operation_key(&operation.method, &operation.path);
            semantics.insert(key.clone(), operation.semantics);
            matrix.insert(
                key,
                OperationStatus {
                    status_code: StatusCode::from_u16(operation.stub_response.status_code)
                        .map_err(|err| {
                            format!(
                                "invalid matrix status code {} for {} {}: {err}",
                                operation.stub_response.status_code,
                                operation.method,
                                operation.path
                            )
                        })?,
                    code: operation.stub_response.code.clone(),
                    message: operation.stub_response.message.clone(),
                },
            );
        }

        Ok(Self {
            routes: templates,
            matrix,
            semantics,
            firecracker_revision,
            firecracker_version,
            vm_state: RwLock::new(VmState::PreBoot),
            boot_source: RwLock::new(BootSourceConfig::default()),
            cpu_config: RwLock::new(None),
            logger_config: RwLock::new(LoggerConfig::default()),
            machine_config: RwLock::new(MachineConfig::default()),
            metrics_config: RwLock::new(None),
            serial_config: RwLock::new(SerialConfig::default()),
            balloon_config: RwLock::new(None),
            balloon_hinting_state: RwLock::new(None),
            drives: RwLock::new(BTreeMap::new()),
            network_interfaces: RwLock::new(BTreeMap::new()),
            mmds_config: RwLock::new(None),
            mmds_data_store: RwLock::new(None),
            pmem_devices: RwLock::new(BTreeMap::new()),
            memory_hotplug: RwLock::new(None),
            entropy_config: RwLock::new(None),
            vsock_config: RwLock::new(None),
        })
    }

    pub fn router(self) -> Router {
        Router::new()
            .fallback(any(handle_all))
            .with_state(Arc::new(self))
    }
}

impl RouteTemplate {
    fn new(method: &str, template: &str) -> Self {
        let method = method.trim().to_ascii_uppercase();
        let template = clean_path(template);
        let parts = split_path(&template)
            .iter()
            .map(|part| {
                if part.starts_with('{') && part.ends_with('}') {
                    RoutePart::Param
                } else {
                    RoutePart::Literal(part.to_string())
                }
            })
            .collect();

        Self {
            method,
            template,
            parts,
        }
    }

    fn matches(&self, method: &Method, path: &str) -> bool {
        if self.method != method.as_str().to_ascii_uppercase() {
            return false;
        }

        let incoming_path = clean_path(path);
        let incoming = split_path(&incoming_path);
        if incoming.len() != self.parts.len() {
            return false;
        }

        for (part, incoming_segment) in self.parts.iter().zip(incoming.iter()) {
            match part {
                RoutePart::Param => {
                    if incoming_segment.is_empty() {
                        return false;
                    }
                }
                RoutePart::Literal(expected) => {
                    if expected != incoming_segment {
                        return false;
                    }
                }
            }
        }

        true
    }

    fn path_param(&self, actual_path: &str, param_name: &str) -> Option<String> {
        let incoming_path = clean_path(actual_path);
        let incoming = split_path(&incoming_path);
        if incoming.len() != self.parts.len() {
            return None;
        }

        for (segment, part) in incoming.iter().zip(split_path(&self.template)) {
            if part == format!("{{{param_name}}}") {
                return Some((*segment).to_string());
            }
        }

        None
    }
}

async fn handle_all(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> Response {
    let path = clean_path(uri.path());

    let route = state
        .routes
        .iter()
        .find(|route| route.matches(&method, &path));

    let Some(route) = route else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::NOT_FOUND,
            "NotFound",
            "route not found",
            &path,
            method.as_str(),
        );
    };

    let route_match = RouteMatch {
        template: route,
        path: path.clone(),
    };

    if route_match.template.method == "PUT" && route_match.template.template == "/actions" {
        return handle_actions(&state, &method, &route_match.template.template, body);
    }

    if route_match.template.method == "PATCH" && route_match.template.template == "/vm" {
        return handle_patch_vm(&state, &method, &route_match.template.template, body);
    }

    if let Some(message) = validate_state_guard(&state, route_match.template) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidState",
            &message,
            &route_match.template.template,
            &route_match.template.method,
        );
    }

    match (
        route_match.template.method.as_str(),
        route_match.template.template.as_str(),
    ) {
        ("GET", "/") => return instance_info_response(&state),
        ("GET", "/balloon") => {
            return handle_get_balloon(&state, &method, &route_match.template.template);
        }
        ("GET", "/balloon/statistics") => {
            return handle_get_balloon_statistics(&state, &method, &route_match.template.template);
        }
        ("GET", "/balloon/hinting/status") => {
            return handle_get_balloon_hinting_status(
                &state,
                &method,
                &route_match.template.template,
            );
        }
        ("GET", "/version") => return firecracker_version_response(&state),
        ("GET", "/machine-config") => return machine_config_response(&state),
        ("PUT", "/balloon") => {
            return handle_put_balloon(&state, &method, &route_match.template.template, body);
        }
        ("PATCH", "/balloon") => {
            return handle_patch_balloon(&state, &method, &route_match.template.template, body);
        }
        ("PATCH", "/balloon/statistics") => {
            return handle_patch_balloon_statistics(
                &state,
                &method,
                &route_match.template.template,
                body,
            );
        }
        ("PATCH", "/balloon/hinting/start") => {
            return handle_patch_balloon_hinting_start(
                &state,
                &method,
                &route_match.template.template,
                body,
            );
        }
        ("PATCH", "/balloon/hinting/stop") => {
            return handle_patch_balloon_hinting_stop(
                &state,
                &method,
                &route_match.template.template,
            );
        }
        ("PUT", "/cpu-config") => {
            return handle_put_cpu_config(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/logger") => {
            return handle_put_logger(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/metrics") => {
            return handle_put_metrics(&state, &method, &route_match.template.template, body);
        }
        ("GET", "/mmds") => {
            return handle_get_mmds(&state, &method, &route_match.template.template);
        }
        ("PATCH", "/mmds") => {
            return handle_patch_mmds(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/mmds") => {
            return handle_put_mmds(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/mmds/config") => {
            return handle_put_mmds_config(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/serial") => {
            return handle_put_serial(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/entropy") => {
            return handle_put_entropy(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/vsock") => {
            return handle_put_vsock(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/boot-source") => {
            return handle_put_boot_source(&state, &method, &route_match.template.template, body);
        }
        ("PUT", "/machine-config") => {
            return handle_put_machine_config(
                &state,
                &method,
                &route_match.template.template,
                body,
            );
        }
        ("PATCH", "/machine-config") => {
            return handle_patch_machine_config(
                &state,
                &method,
                &route_match.template.template,
                body,
            );
        }
        ("PUT", "/drives/{drive_id}") => {
            return handle_put_drive_stub(&state, &method, &route_match, body);
        }
        ("PATCH", "/drives/{drive_id}") => {
            return handle_patch_drive_stub(&state, &method, &route_match, body);
        }
        ("PUT", "/network-interfaces/{iface_id}") => {
            return handle_put_network_interface_stub(&state, &method, &route_match, body);
        }
        ("PATCH", "/network-interfaces/{iface_id}") => {
            return handle_patch_network_interface_stub(&state, &method, &route_match, body);
        }
        ("GET", "/hotplug/memory") => {
            return handle_get_memory_hotplug(&state, &method, &route_match.template.template);
        }
        ("PATCH", "/hotplug/memory") => {
            return handle_patch_memory_hotplug(
                &state,
                &method,
                &route_match.template.template,
                body,
            );
        }
        ("PUT", "/hotplug/memory") => {
            return handle_put_memory_hotplug(
                &state,
                &method,
                &route_match.template.template,
                body,
            );
        }
        ("PUT", "/pmem/{id}") => return handle_put_pmem_stub(&state, &method, &route_match, body),
        ("PUT", "/snapshot/create") => {
            return handle_put_snapshot_create_stub(&state, &method, &route_match, body);
        }
        ("PUT", "/snapshot/load") => {
            return handle_put_snapshot_load_stub(&state, &method, &route_match, body);
        }
        ("PATCH", "/vm") => {
            return handle_patch_vm(&state, &method, &route_match.template.template, body);
        }
        ("GET", "/vm/config") => return full_vm_config_response(&state),
        _ => {}
    }

    matrix_response(&state, route_match.template)
}

fn matrix_response(state: &Arc<AppState>, route: &RouteTemplate) -> Response {
    let key = operation_key(&route.method, &route.template);
    let status = state.matrix.get(&key);
    let Some(status) = status else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::INTERNAL_SERVER_ERROR,
            "InternalMatrixError",
            "operation status not found",
            &route.template,
            &route.method,
        );
    };

    error_response(
        &state.firecracker_revision,
        status.status_code,
        &status.code,
        &status.message,
        &route.template,
        &route.method,
    )
}

fn instance_info_response(state: &Arc<AppState>) -> Response {
    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");
    let body = InstanceInfoResponse {
        app_name: DEFAULT_APP_NAME.to_string(),
        id: DEFAULT_INSTANCE_ID.to_string(),
        state: vm_state.instance_str().to_string(),
        vmm_version: state.firecracker_version.clone(),
    };
    (StatusCode::OK, Json(body)).into_response()
}

fn firecracker_version_response(state: &Arc<AppState>) -> Response {
    (
        StatusCode::OK,
        Json(FirecrackerVersionResponse {
            firecracker_version: state.firecracker_version.clone(),
        }),
    )
        .into_response()
}

fn machine_config_response(state: &Arc<AppState>) -> Response {
    let config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();
    (StatusCode::OK, Json(config)).into_response()
}

fn full_vm_config_response(state: &Arc<AppState>) -> Response {
    let boot_source = state
        .boot_source
        .read()
        .expect("boot source lock poisoned while reading")
        .clone();
    let config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();
    let mut drives: Vec<DriveConfigRequest> = state
        .drives
        .read()
        .expect("drive config lock poisoned while reading")
        .values()
        .cloned()
        .collect();
    if let Some(root_index) = drives.iter().position(|drive| drive.is_root_device) {
        if root_index != 0 {
            let root_drive = drives.remove(root_index);
            drives.insert(0, root_drive);
        }
    }
    let network_interfaces = state
        .network_interfaces
        .read()
        .expect("network interface config lock poisoned while reading")
        .values()
        .cloned()
        .collect();
    let mmds_config = state
        .mmds_config
        .read()
        .expect("mmds config lock poisoned while reading")
        .clone();
    let pmem = state
        .pmem_devices
        .read()
        .expect("pmem config lock poisoned while reading")
        .values()
        .cloned()
        .collect();
    let memory_hotplug = state
        .memory_hotplug
        .read()
        .expect("memory hotplug lock poisoned while reading")
        .as_ref()
        .map(|state| state.config.clone());
    let entropy = state
        .entropy_config
        .read()
        .expect("entropy config lock poisoned while reading")
        .clone();
    let vsock = state
        .vsock_config
        .read()
        .expect("vsock config lock poisoned while reading")
        .clone();
    (
        StatusCode::OK,
        Json(FullVmConfigurationResponse {
            boot_source,
            cpu_config: None,
            machine_config: config,
            balloon,
            drives,
            network_interfaces,
            pmem,
            mmds_config,
            memory_hotplug,
            entropy,
            vsock,
        }),
    )
        .into_response()
}

fn handle_actions(state: &Arc<AppState>, method: &Method, template: &str, body: Bytes) -> Response {
    let payload: ActionRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    match payload.action_type {
        ActionType::FlushMetrics => {
            let vm_state = *state
                .vm_state
                .read()
                .expect("vm state lock poisoned while reading");
            if vm_state == VmState::PreBoot {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidState",
                    ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE,
                    template,
                    method.as_str(),
                );
            }

            StatusCode::NO_CONTENT.into_response()
        }
        ActionType::InstanceStart => {
            let mut vm_state = state
                .vm_state
                .write()
                .expect("vm state lock poisoned while writing");

            if *vm_state != VmState::PreBoot {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidState",
                    ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE,
                    template,
                    method.as_str(),
                );
            }

            let boot_source = state
                .boot_source
                .read()
                .expect("boot source lock poisoned while reading")
                .clone();
            if boot_source.kernel_image_path.is_empty() {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    MISSING_KERNEL_CONFIG_MESSAGE,
                    template,
                    method.as_str(),
                );
            }

            *vm_state = VmState::Running;
            StatusCode::NO_CONTENT.into_response()
        }
        ActionType::SendCtrlAltDel => {
            #[cfg(target_arch = "aarch64")]
            {
                error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    SEND_CTRL_ALT_DEL_AARCH64_MESSAGE,
                    template,
                    method.as_str(),
                )
            }

            #[cfg(target_arch = "x86_64")]
            {
                let vm_state = *state
                    .vm_state
                    .read()
                    .expect("vm state lock poisoned while reading");
                if vm_state == VmState::PreBoot {
                    return error_response(
                        &state.firecracker_revision,
                        StatusCode::BAD_REQUEST,
                        "InvalidState",
                        ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE,
                        template,
                        method.as_str(),
                    );
                }

                StatusCode::NO_CONTENT.into_response()
            }
        }
    }
}

fn handle_put_boot_source(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BootSourceConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if let Err(message) = validate_boot_source(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &message,
            template,
            method.as_str(),
        );
    }

    *state
        .boot_source
        .write()
        .expect("boot source lock poisoned while writing") = payload;

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_logger(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: LoggerConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if let Err(message) = validate_logger_request(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &message,
            template,
            method.as_str(),
        );
    }

    let mut config = state
        .logger_config
        .write()
        .expect("logger config lock poisoned while writing");
    if let Some(log_path) = payload.log_path {
        config.log_path = Some(log_path);
    }
    if let Some(level) = payload.level {
        config.level = level;
    }
    if let Some(show_level) = payload.show_level {
        config.show_level = show_level;
    }
    if let Some(show_log_origin) = payload.show_log_origin {
        config.show_log_origin = show_log_origin;
    }
    if let Some(module) = payload.module {
        config.module = Some(module);
    }

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_cpu_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: CpuConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    *state
        .cpu_config
        .write()
        .expect("cpu config lock poisoned while writing") = Some(payload);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_metrics(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MetricsConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if let Err(message) = validate_metrics_request(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &message,
            template,
            method.as_str(),
        );
    }

    let mut config = state
        .metrics_config
        .write()
        .expect("metrics config lock poisoned while writing");
    if config.is_some() {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            METRICS_ALREADY_INITIALIZED_MESSAGE,
            template,
            method.as_str(),
        );
    }

    *config = Some(payload);
    StatusCode::NO_CONTENT.into_response()
}

fn handle_get_mmds(state: &Arc<AppState>, method: &Method, template: &str) -> Response {
    if let Err(response) = ensure_mmds_runtime_access(state, method, template) {
        return response;
    }

    let value = state
        .mmds_data_store
        .read()
        .expect("mmds data store lock poisoned while reading")
        .clone();

    mmds_value_response(value)
}

fn handle_patch_mmds(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    if let Err(response) = ensure_mmds_runtime_access(state, method, template) {
        return response;
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let mut mmds_data_store = state
        .mmds_data_store
        .write()
        .expect("mmds data store lock poisoned while writing");
    let Some(current_value) = mmds_data_store.as_mut() else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            MMDS_NOT_INITIALIZED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    let mut merged = current_value.clone();
    apply_json_merge_patch(&mut merged, &payload);

    if let Err(message) = validate_mmds_data_store_limit(&merged) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::PAYLOAD_TOO_LARGE,
            "PayloadTooLarge",
            message,
            template,
            method.as_str(),
        );
    }

    *current_value = merged;
    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_mmds(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    if let Err(response) = ensure_mmds_runtime_access(state, method, template) {
        return response;
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if let Err(message) = validate_mmds_data_store_limit(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::PAYLOAD_TOO_LARGE,
            "PayloadTooLarge",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .mmds_data_store
        .write()
        .expect("mmds data store lock poisoned while writing") = Some(payload);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_mmds_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MmdsConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let normalized = payload.normalized();
    if let Err(message) = validate_mmds_config(state, &normalized) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .mmds_config
        .write()
        .expect("mmds config lock poisoned while writing") = Some(normalized);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_serial(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: SerialConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    *state
        .serial_config
        .write()
        .expect("serial config lock poisoned while writing") = payload;

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_entropy(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: EntropyDeviceConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    *state
        .entropy_config
        .write()
        .expect("entropy config lock poisoned while writing") = Some(payload);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_vsock(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: VsockConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if payload.guest_cid < 3 {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            INVALID_VSOCK_GUEST_CID_MESSAGE,
            template,
            method.as_str(),
        );
    }

    *state
        .vsock_config
        .write()
        .expect("vsock config lock poisoned while writing") = Some(payload.into());

    StatusCode::NO_CONTENT.into_response()
}

fn handle_get_balloon(state: &Arc<AppState>, method: &Method, template: &str) -> Response {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();

    let Some(balloon) = balloon else {
        let vm_state = *state
            .vm_state
            .read()
            .expect("vm state lock poisoned while reading");
        let message = if vm_state == VmState::PreBoot {
            BALLOON_NOT_CONFIGURED_MESSAGE
        } else {
            BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE
        };
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    };

    (StatusCode::OK, Json(balloon)).into_response()
}

fn handle_put_balloon(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BalloonDeviceConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if let Err(message) = validate_balloon_config(state, &payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .balloon_config
        .write()
        .expect("balloon config lock poisoned while writing") = Some(payload.clone());
    *state
        .balloon_hinting_state
        .write()
        .expect("balloon hinting state lock poisoned while writing") =
        payload.free_page_hinting.then(BalloonHintingState::default);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_balloon(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BalloonSizeUpdate = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let machine_config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();
    let mut balloon = state
        .balloon_config
        .write()
        .expect("balloon config lock poisoned while writing");
    let Some(balloon) = balloon.as_mut() else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    if payload.amount_mib as usize > machine_config.mem_size_mib {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &balloon_update_too_large_message(machine_config.mem_size_mib),
            template,
            method.as_str(),
        );
    }

    balloon.amount_mib = payload.amount_mib;
    StatusCode::NO_CONTENT.into_response()
}

fn handle_get_balloon_statistics(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Response {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();
    let Some(balloon) = balloon else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    if balloon.stats_polling_interval_s == 0 {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_STATS_DISABLED_MESSAGE,
            template,
            method.as_str(),
        );
    }

    (
        StatusCode::OK,
        Json(balloon_stats_response_for(balloon.amount_mib)),
    )
        .into_response()
}

fn handle_get_balloon_hinting_status(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Response {
    match validated_balloon_hinting_state(state, method, template) {
        Ok(hinting_state) => (
            StatusCode::OK,
            Json(BalloonHintingStatusResponse {
                host_cmd: hinting_state.host_cmd,
                guest_cmd: hinting_state.guest_cmd,
            }),
        )
            .into_response(),
        Err(response) => response,
    }
}

fn handle_patch_balloon_hinting_start(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload = if body.is_empty() {
        BalloonHintingStartRequest::default()
    } else {
        match serde_json::from_slice::<BalloonHintingStartRequest>(&body) {
            Ok(value) => value,
            Err(_) => {
                return error_response(
                    &state.firecracker_revision,
                    StatusCode::BAD_REQUEST,
                    "InvalidRequest",
                    "invalid JSON payload",
                    template,
                    method.as_str(),
                );
            }
        }
    };

    if let Err(response) = ensure_balloon_hinting_enabled(state, method, template) {
        return response;
    }

    let mut hinting_state = state
        .balloon_hinting_state
        .write()
        .expect("balloon hinting state lock poisoned while writing");
    let hinting_state = hinting_state.get_or_insert_with(BalloonHintingState::default);
    hinting_state.host_cmd = hinting_state.next_run_cmd;
    hinting_state.next_run_cmd = hinting_state.next_run_cmd.saturating_add(1);
    hinting_state.acknowledge_on_stop = payload.acknowledge_on_stop;

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_balloon_hinting_stop(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Response {
    if let Err(response) = ensure_balloon_hinting_enabled(state, method, template) {
        return response;
    }

    let mut hinting_state = state
        .balloon_hinting_state
        .write()
        .expect("balloon hinting state lock poisoned while writing");
    let hinting_state = hinting_state.get_or_insert_with(BalloonHintingState::default);
    hinting_state.host_cmd = BALLOON_HINTING_DONE_COMMAND;

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_balloon_statistics(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: BalloonStatsIntervalUpdate = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let mut balloon = state
        .balloon_config
        .write()
        .expect("balloon config lock poisoned while writing");
    let Some(balloon) = balloon.as_mut() else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    let stats_were_enabled = balloon.stats_polling_interval_s > 0;
    let stats_should_be_enabled = payload.stats_polling_interval_s > 0;
    if stats_were_enabled != stats_should_be_enabled {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_STATS_STATE_CHANGE_MESSAGE,
            template,
            method.as_str(),
        );
    }

    balloon.stats_polling_interval_s = payload.stats_polling_interval_s;
    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_machine_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MachineConfigPutRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let config = MachineConfig::from(payload);
    if let Err(message) = validate_machine_config(&config) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    if let Err(message) = validate_machine_config_against_balloon(state, config.mem_size_mib) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .machine_config
        .write()
        .expect("machine config lock poisoned while writing") = config;

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_machine_config(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MachineConfigPatchRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if payload.is_empty() {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "PATCH /machine-config requires at least one field",
            template,
            method.as_str(),
        );
    }

    let current = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        .clone();

    let updated = MachineConfig {
        cpu_template: match payload.cpu_template {
            Some(template) => template.into_stored(),
            None => current.cpu_template,
        },
        smt: payload.smt.unwrap_or(current.smt),
        mem_size_mib: payload.mem_size_mib.unwrap_or(current.mem_size_mib),
        track_dirty_pages: payload
            .track_dirty_pages
            .unwrap_or(current.track_dirty_pages),
        vcpu_count: payload.vcpu_count.unwrap_or(current.vcpu_count),
        huge_pages: payload.huge_pages.unwrap_or(current.huge_pages),
    };

    if let Err(message) = validate_machine_config(&updated) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    if let Err(message) = validate_machine_config_against_balloon(state, updated.mem_size_mib) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .machine_config
        .write()
        .expect("machine config lock poisoned while writing") = updated;

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_vm(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: VmStateChangeRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let mut vm_state = state
        .vm_state
        .write()
        .expect("vm state lock poisoned while writing");
    match (payload.state, *vm_state) {
        (_, VmState::PreBoot) => error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidState",
            ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE,
            template,
            method.as_str(),
        ),
        (VmStateChange::Paused, VmState::Running) => {
            *vm_state = VmState::Paused;
            StatusCode::NO_CONTENT.into_response()
        }
        (VmStateChange::Resumed, VmState::Paused) => {
            *vm_state = VmState::Running;
            StatusCode::NO_CONTENT.into_response()
        }
        _ => error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidState",
            &state_invalid_message(operation_key(method.as_str(), template), *vm_state),
            template,
            method.as_str(),
        ),
    }
}

fn handle_put_drive_stub(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: DriveConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let path_drive_id = route
        .template
        .path_param(&route.path, "drive_id")
        .unwrap_or_default();
    if payload.drive_id != path_drive_id {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            DRIVE_ID_PATH_MISMATCH_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    if payload.is_root_device
        && state
            .pmem_devices
            .read()
            .expect("pmem config lock poisoned while reading")
            .values()
            .any(|pmem| pmem.root_device)
    {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            DRIVE_ROOT_PMEM_CONFLICT_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    let mut drives = state
        .drives
        .write()
        .expect("drive config lock poisoned while writing");
    if payload.is_root_device
        && drives
            .iter()
            .any(|(id, drive)| id != &payload.drive_id && drive.is_root_device)
    {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            DRIVE_ROOT_ALREADY_EXISTS_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    if let Err(message) = validate_drive_backing_file(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &message,
            &route.template.template,
            method.as_str(),
        );
    }

    drives.insert(payload.drive_id.clone(), payload);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_drive_stub(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: PartialDriveConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let path_drive_id = route
        .template
        .path_param(&route.path, "drive_id")
        .unwrap_or_default();
    if payload.drive_id != path_drive_id {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            DRIVE_ID_PATH_MISMATCH_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    let mut drives = state
        .drives
        .write()
        .expect("drive config lock poisoned while writing");
    let Some(existing) = drives.get_mut(&payload.drive_id) else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            DRIVE_UPDATE_NOT_FOUND_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    };

    if let Some(path_on_host) = payload.path_on_host {
        existing.path_on_host = Some(path_on_host);
    }
    if let Some(rate_limiter) = payload.rate_limiter {
        existing.rate_limiter = Some(rate_limiter);
    }

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_network_interface_stub(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: NetworkInterfaceConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let path_iface_id = route
        .template
        .path_param(&route.path, "iface_id")
        .unwrap_or_default();
    if payload.iface_id != path_iface_id {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &network_id_path_mismatch_message(&path_iface_id, &payload.iface_id),
            &route.template.template,
            method.as_str(),
        );
    }

    let mut interfaces = state
        .network_interfaces
        .write()
        .expect("network interface config lock poisoned while writing");
    if let Some(guest_mac) = payload.guest_mac.as_deref() {
        let guest_mac_conflict = interfaces.values().any(|existing| {
            existing.iface_id != payload.iface_id
                && existing.guest_mac.as_deref() == Some(guest_mac)
        });
        if guest_mac_conflict {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &network_guest_mac_in_use_message(guest_mac),
                &route.template.template,
                method.as_str(),
            );
        }
    }

    let host_dev_name_conflict = interfaces.values().any(|existing| {
        existing.iface_id != payload.iface_id && existing.host_dev_name == payload.host_dev_name
    });
    if host_dev_name_conflict {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &network_tap_open_message(&payload.host_dev_name),
            &route.template.template,
            method.as_str(),
        );
    }

    interfaces.insert(payload.iface_id.clone(), payload);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_patch_network_interface_stub(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: PartialNetworkInterfaceConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let path_iface_id = route
        .template
        .path_param(&route.path, "iface_id")
        .unwrap_or_default();
    if payload.iface_id != path_iface_id {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &network_id_path_mismatch_message(&path_iface_id, &payload.iface_id),
            &route.template.template,
            method.as_str(),
        );
    }

    let mut interfaces = state
        .network_interfaces
        .write()
        .expect("network interface config lock poisoned while writing");
    let Some(existing) = interfaces.get_mut(&payload.iface_id) else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            NETWORK_UPDATE_NOT_FOUND_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    };

    if let Some(rx_rate_limiter) = payload.rx_rate_limiter {
        existing.rx_rate_limiter = Some(rx_rate_limiter);
    }
    if let Some(tx_rate_limiter) = payload.tx_rate_limiter {
        existing.tx_rate_limiter = Some(tx_rate_limiter);
    }

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_pmem_stub(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: PmemConfigRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let path_id = route
        .template
        .path_param(&route.path, "id")
        .unwrap_or_default();
    if payload.id != path_id {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            PMEM_ID_PATH_MISMATCH_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    if payload.root_device
        && state
            .drives
            .read()
            .expect("drive config lock poisoned while reading")
            .values()
            .any(|drive| drive.is_root_device)
    {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            PMEM_ROOT_BLOCK_CONFLICT_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    let mut pmem_devices = state
        .pmem_devices
        .write()
        .expect("pmem config lock poisoned while writing");
    if payload.root_device
        && pmem_devices
            .iter()
            .any(|(id, device)| id != &payload.id && device.root_device)
    {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            PMEM_ROOT_ALREADY_EXISTS_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    let backing_file = OpenOptions::new()
        .read(true)
        .write(!payload.read_only)
        .open(&payload.path_on_host);
    let backing_file = match backing_file {
        Ok(file) => file,
        Err(err) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &pmem_backing_file_access_message(&err),
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let backing_file_len = match backing_file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(err) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                &pmem_backing_file_access_message(&err),
                &route.template.template,
                method.as_str(),
            );
        }
    };
    if backing_file_len == 0 {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    pmem_devices.insert(payload.id.clone(), payload);

    StatusCode::NO_CONTENT.into_response()
}

fn handle_get_memory_hotplug(state: &Arc<AppState>, method: &Method, template: &str) -> Response {
    let memory_hotplug = state
        .memory_hotplug
        .read()
        .expect("memory hotplug lock poisoned while reading")
        .clone();

    let Some(memory_hotplug) = memory_hotplug else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            MEMORY_HOTPLUG_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    (StatusCode::OK, Json(memory_hotplug.status())).into_response()
}

fn handle_patch_memory_hotplug(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MemoryHotplugSizeUpdate = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    let mut memory_hotplug = state
        .memory_hotplug
        .write()
        .expect("memory hotplug lock poisoned while writing");
    let Some(memory_hotplug) = memory_hotplug.as_mut() else {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            MEMORY_HOTPLUG_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        );
    };

    if let Err(message) = validate_memory_hotplug_size_update(&memory_hotplug.config, &payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    memory_hotplug.requested_size_mib = payload.requested_size_mib;
    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_memory_hotplug(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
    body: Bytes,
) -> Response {
    let payload: MemoryHotplugConfig = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                template,
                method.as_str(),
            );
        }
    };

    if let Err(message) = validate_memory_hotplug_config(&payload) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            message,
            template,
            method.as_str(),
        );
    }

    *state
        .memory_hotplug
        .write()
        .expect("memory hotplug lock poisoned while writing") =
        Some(MemoryHotplugState::new(payload));

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_snapshot_create_stub(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let _: SnapshotCreateRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");
    match vm_state {
        VmState::PreBoot => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidState",
                ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE,
                &route.template.template,
                method.as_str(),
            );
        }
        VmState::Running => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidState",
                SNAPSHOT_CREATE_REQUIRES_PAUSED_MESSAGE,
                &route.template.template,
                method.as_str(),
            );
        }
        VmState::Paused => {}
    }

    StatusCode::NO_CONTENT.into_response()
}

fn handle_put_snapshot_load_stub(
    state: &Arc<AppState>,
    method: &Method,
    route: &RouteMatch<'_>,
    body: Bytes,
) -> Response {
    let payload: SnapshotLoadRequest = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return error_response(
                &state.firecracker_revision,
                StatusCode::BAD_REQUEST,
                "InvalidRequest",
                "invalid JSON payload",
                &route.template.template,
                method.as_str(),
            );
        }
    };

    let has_mem_file = payload.mem_file_path.is_some();
    let has_mem_backend = payload.mem_backend.is_some();
    if !has_mem_file && !has_mem_backend {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            SNAPSHOT_LOAD_MISSING_MEMORY_SOURCE_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    if has_mem_file && has_mem_backend {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            SNAPSHOT_LOAD_TOO_MANY_MEMORY_SOURCES_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    if snapshot_load_has_blocking_config(state) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidState",
            SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE,
            &route.template.template,
            method.as_str(),
        );
    }

    let loaded_vm_state = if payload.resume_vm.unwrap_or(false) {
        VmState::Running
    } else {
        VmState::Paused
    };
    *state
        .vm_state
        .write()
        .expect("vm state lock poisoned while writing") = loaded_vm_state;

    StatusCode::NO_CONTENT.into_response()
}

fn snapshot_load_has_blocking_config(state: &Arc<AppState>) -> bool {
    let boot_source = state
        .boot_source
        .read()
        .expect("boot source lock poisoned while reading");
    if !boot_source.kernel_image_path.is_empty()
        || boot_source.initrd_path.is_some()
        || boot_source.boot_args.is_some()
    {
        return true;
    }

    if state
        .cpu_config
        .read()
        .expect("cpu config lock poisoned while reading")
        .is_some()
    {
        return true;
    }

    if *state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        != MachineConfig::default()
    {
        return true;
    }

    if state
        .drives
        .read()
        .expect("drive config lock poisoned while reading")
        .is_empty()
        == false
    {
        return true;
    }

    if state
        .network_interfaces
        .read()
        .expect("network interface config lock poisoned while reading")
        .is_empty()
        == false
    {
        return true;
    }

    if state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .is_some()
    {
        return true;
    }

    if state
        .mmds_config
        .read()
        .expect("mmds config lock poisoned while reading")
        .is_some()
    {
        return true;
    }

    if state
        .pmem_devices
        .read()
        .expect("pmem config lock poisoned while reading")
        .is_empty()
        == false
    {
        return true;
    }

    if state
        .memory_hotplug
        .read()
        .expect("memory hotplug lock poisoned while reading")
        .is_some()
    {
        return true;
    }

    if state
        .entropy_config
        .read()
        .expect("entropy config lock poisoned while reading")
        .is_some()
    {
        return true;
    }

    if state
        .vsock_config
        .read()
        .expect("vsock config lock poisoned while reading")
        .is_some()
    {
        return true;
    }

    false
}

fn validate_state_guard(state: &Arc<AppState>, route: &RouteTemplate) -> Option<String> {
    let semantics = state
        .semantics
        .get(&operation_key(&route.method, &route.template))
        .copied()
        .unwrap_or(RouteSemantics::Any);

    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");

    match semantics {
        RouteSemantics::Any | RouteSemantics::PreBootRuntime => None,
        RouteSemantics::PreBoot if vm_state != VmState::PreBoot => {
            Some(ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE.to_string())
        }
        RouteSemantics::PostBoot if vm_state == VmState::PreBoot => {
            Some(ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE.to_string())
        }
        _ => None,
    }
}

fn validate_boot_source(config: &BootSourceConfig) -> Result<(), String> {
    File::open(&config.kernel_image_path)
        .map_err(|err| format!("The kernel file cannot be opened: {err}"))?;

    if let Some(path) = &config.initrd_path {
        File::open(path).map_err(|err| {
            format!(
                "The initrd file cannot be opened due to invalid path or invalid permissions. {err}"
            )
        })?;
    }

    Ok(())
}

fn validate_logger_request(config: &LoggerConfigRequest) -> Result<(), String> {
    if let Some(path) = &config.log_path {
        validate_writable_path(path).map_err(|err| format!("Failed to open target file: {err}"))?;
    }

    Ok(())
}

fn validate_metrics_request(config: &MetricsConfig) -> Result<(), String> {
    validate_writable_path(&config.metrics_path)
        .map_err(|err| format!("Cannot initialize the metrics system due to bad user input: {err}"))
}

fn validate_writable_path(path: &str) -> Result<(), std::io::Error> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
}

fn validate_machine_config(config: &MachineConfig) -> Result<(), &'static str> {
    #[cfg(target_arch = "aarch64")]
    if config.smt {
        return Err("Enabling simultaneous multithreading is not supported on aarch64.");
    }

    if config.vcpu_count == 0 || config.vcpu_count > MAX_SUPPORTED_VCPUS {
        return Err(
            "The number of vCPUs must be greater than 0, less than 32 and must be 1 or an even number if SMT is enabled.",
        );
    }

    if config.smt && config.vcpu_count > 1 && config.vcpu_count % 2 == 1 {
        return Err(
            "The number of vCPUs must be greater than 0, less than 32 and must be 1 or an even number if SMT is enabled.",
        );
    }

    if config.mem_size_mib == 0
        || matches!(config.huge_pages, HugePages::Huge2M) && config.mem_size_mib % 2 != 0
    {
        return Err(
            "The memory size (MiB) is either 0, or not a multiple of the configured page size.",
        );
    }

    Ok(())
}

fn validate_balloon_config(
    state: &Arc<AppState>,
    config: &BalloonDeviceConfig,
) -> Result<(), &'static str> {
    let machine_config = state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading");
    if config.amount_mib as usize > machine_config.mem_size_mib {
        return Err(BALLOON_TOO_MANY_PAGES_REQUESTED_MESSAGE);
    }

    Ok(())
}

fn validate_machine_config_against_balloon(
    state: &Arc<AppState>,
    mem_size_mib: usize,
) -> Result<(), &'static str> {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading");
    if balloon
        .as_ref()
        .map(|config| config.amount_mib as usize > mem_size_mib)
        .unwrap_or(false)
    {
        return Err(MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE);
    }

    Ok(())
}

fn validate_mmds_config(state: &Arc<AppState>, config: &MmdsConfig) -> Result<(), &'static str> {
    if config.network_interfaces.is_empty() {
        return Err(MMDS_EMPTY_NETWORK_INTERFACE_LIST_MESSAGE);
    }

    let ipv4_address = config
        .ipv4_address
        .unwrap_or_else(default_mmds_ipv4_address);
    if !is_valid_link_local_mmds_ipv4(ipv4_address) {
        return Err(MMDS_INVALID_IPV4_ADDRESS_MESSAGE);
    }

    let interfaces = state
        .network_interfaces
        .read()
        .expect("network interface config lock poisoned while reading");
    if !config
        .network_interfaces
        .iter()
        .all(|iface_id| interfaces.contains_key(iface_id))
    {
        return Err(MMDS_INVALID_NETWORK_INTERFACE_ID_MESSAGE);
    }

    Ok(())
}

fn validate_mmds_data_store_limit(value: &Value) -> Result<(), &'static str> {
    if to_vec(value)
        .map(|payload| payload.len() > MMDS_DATA_STORE_LIMIT_BYTES)
        .unwrap_or(false)
    {
        return Err(MMDS_DATA_STORE_LIMIT_EXCEEDED_MESSAGE);
    }

    Ok(())
}

fn ensure_mmds_runtime_access(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Result<(), Response> {
    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");
    if vm_state == VmState::PreBoot {
        return Ok(());
    }

    if state
        .mmds_config
        .read()
        .expect("mmds config lock poisoned while reading")
        .is_some()
    {
        Ok(())
    } else {
        Err(error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            MMDS_NOT_INITIALIZED_MESSAGE,
            template,
            method.as_str(),
        ))
    }
}

fn is_valid_link_local_mmds_ipv4(ipv4_address: Ipv4Addr) -> bool {
    match ipv4_address.octets() {
        [169, 254, 0, _] => false,
        [169, 254, 255, _] => false,
        [169, 254, _, _] => true,
        _ => false,
    }
}

fn apply_json_merge_patch(target: &mut Value, patch: &Value) {
    let Some(patch_object) = patch.as_object() else {
        *target = patch.clone();
        return;
    };

    if !target.is_object() {
        *target = Value::Object(serde_json::Map::new());
    }

    let target_object = target
        .as_object_mut()
        .expect("target must be an object after normalization");
    for (key, value) in patch_object {
        if value.is_null() {
            target_object.remove(key);
            continue;
        }

        let entry = target_object.entry(key.clone()).or_insert(Value::Null);
        apply_json_merge_patch(entry, value);
    }
}

fn validate_memory_hotplug_config(config: &MemoryHotplugConfig) -> Result<(), &'static str> {
    if config.block_size_mib < DEFAULT_MEMORY_HOTPLUG_BLOCK_SIZE_MIB
        || !config.block_size_mib.is_power_of_two()
    {
        return Err(INVALID_MEMORY_HOTPLUG_BLOCK_SIZE_MESSAGE);
    }

    if config.slot_size_mib < DEFAULT_MEMORY_HOTPLUG_SLOT_SIZE_MIB
        || config.slot_size_mib % config.block_size_mib != 0
    {
        return Err(INVALID_MEMORY_HOTPLUG_SLOT_SIZE_MESSAGE);
    }

    if config.total_size_mib < config.slot_size_mib
        || config.total_size_mib % config.slot_size_mib != 0
    {
        return Err(INVALID_MEMORY_HOTPLUG_TOTAL_SIZE_MESSAGE);
    }

    Ok(())
}

fn validate_memory_hotplug_size_update(
    config: &MemoryHotplugConfig,
    payload: &MemoryHotplugSizeUpdate,
) -> Result<(), &'static str> {
    if payload.requested_size_mib > config.total_size_mib
        || payload.requested_size_mib % config.block_size_mib != 0
    {
        return Err(INVALID_MEMORY_HOTPLUG_REQUESTED_SIZE_MESSAGE);
    }

    Ok(())
}

fn balloon_update_too_large_message(mem_size_mib: usize) -> String {
    format!("Balloon update error: Balloon: Requested memory should be less than {mem_size_mib}MiB")
}

fn balloon_stats_response_for(amount_mib: u32) -> BalloonStatsResponse {
    let amount_pages = amount_mib.saturating_mul(256);
    BalloonStatsResponse {
        target_pages: amount_pages,
        actual_pages: amount_pages,
        target_mib: amount_mib,
        actual_mib: amount_mib,
        ..BalloonStatsResponse::default()
    }
}

fn ensure_balloon_hinting_enabled(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Result<(), Response> {
    let balloon = state
        .balloon_config
        .read()
        .expect("balloon config lock poisoned while reading")
        .clone();
    let Some(balloon) = balloon else {
        return Err(error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE,
            template,
            method.as_str(),
        ));
    };

    if !balloon.free_page_hinting {
        return Err(error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            BALLOON_HINTING_NOT_ENABLED_MESSAGE,
            template,
            method.as_str(),
        ));
    }

    Ok(())
}

fn validated_balloon_hinting_state(
    state: &Arc<AppState>,
    method: &Method,
    template: &str,
) -> Result<BalloonHintingState, Response> {
    ensure_balloon_hinting_enabled(state, method, template)?;

    Ok(state
        .balloon_hinting_state
        .read()
        .expect("balloon hinting state lock poisoned while reading")
        .clone()
        .unwrap_or_default())
}

fn state_invalid_message(operation: String, state: VmState) -> String {
    format!(
        "operation {operation} is not allowed while instance state is {}",
        state.guard_str()
    )
}

fn error_response(
    firecracker_revision: &str,
    status: StatusCode,
    code: &str,
    message: &str,
    path: &str,
    method: &str,
) -> Response {
    let body = ErrorEnvelope {
        fault_message: message.to_string(),
        code: code.to_string(),
        path: clean_path(path),
        method: method.to_ascii_uppercase(),
        baseline_firecracker_revision: firecracker_revision.to_string(),
    };
    (status, Json(body)).into_response()
}

fn mmds_value_response(value: Option<Value>) -> Response {
    let body = match value {
        None => "{}".to_string(),
        Some(Value::Null) => "{}".to_string(),
        Some(other) => serde_json::to_string(&other).expect("serialize MMDS value"),
    };

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

fn operation_key(method: &str, path: &str) -> String {
    format!(
        "{} {}",
        method.trim().to_ascii_uppercase(),
        clean_path(path)
    )
}

fn clean_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return "/".to_string();
    }

    let with_leading = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };

    if with_leading.len() > 1 {
        with_leading.trim_end_matches('/').to_string()
    } else {
        with_leading
    }
}

fn split_path(path: &str) -> Vec<&str> {
    if path == "/" {
        return Vec::new();
    }
    path.trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use tempfile::NamedTempFile;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        AppState::new(
            &[
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/actions".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/boot-source".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/balloon".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/balloon".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/balloon/statistics".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/balloon/statistics".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/balloon/hinting/status".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/balloon/hinting/start".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/balloon/hinting/stop".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/balloon".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/version".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/machine-config".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/cpu-config".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/logger".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/machine-config".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/metrics".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/mmds".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/mmds".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/mmds".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/mmds/config".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/serial".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/entropy".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/vsock".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/machine-config".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/drives/{drive_id}".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/drives/{drive_id}".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/network-interfaces/{iface_id}".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/network-interfaces/{iface_id}".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/hotplug/memory".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/hotplug/memory".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/hotplug/memory".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/pmem/{id}".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/snapshot/create".to_string(),
                },
                RouteSpec {
                    method: "PUT".to_string(),
                    path: "/snapshot/load".to_string(),
                },
                RouteSpec {
                    method: "GET".to_string(),
                    path: "/vm/config".to_string(),
                },
                RouteSpec {
                    method: "PATCH".to_string(),
                    path: "/vm".to_string(),
                },
            ],
            "abc123".to_string(),
            "1.16.0-dev".to_string(),
        )
        .unwrap()
    }

    fn put_boot_source_request(
        kernel_path: &str,
        initrd_path: Option<&str>,
    ) -> Request<axum::body::Body> {
        let body = match initrd_path {
            Some(initrd_path) => {
                format!(r#"{{"kernel_image_path":"{kernel_path}","initrd_path":"{initrd_path}"}}"#)
            }
            None => format!(r#"{{"kernel_image_path":"{kernel_path}"}}"#),
        };

        Request::builder()
            .method("PUT")
            .uri("/boot-source")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    }

    #[tokio::test]
    async fn transitions_to_running_on_instance_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let initrd = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                Some(initrd.path().to_str().unwrap()),
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn rejects_preboot_mutation_after_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let _ = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/boot-source")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE);
    }

    #[tokio::test]
    async fn returns_default_instance_identity() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["app_name"], "Firecracker");
        assert_eq!(payload["id"], "anonymous-instance");
        assert_eq!(payload["state"], "Not started");
        assert_eq!(payload["vmm_version"], "1.16.0-dev");
    }

    #[tokio::test]
    async fn updates_machine_config_and_exports_vm_config() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"vcpu_count":2,"mem_size_mib":256,"track_dirty_pages":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"mem_size_mib":512}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

        let machine_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/machine-config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(machine_response.status(), StatusCode::OK);
        let machine_body = to_bytes(machine_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let machine_payload: serde_json::Value = serde_json::from_slice(&machine_body).unwrap();
        assert_eq!(machine_payload["vcpu_count"], 2);
        assert_eq!(machine_payload["mem_size_mib"], 512);
        assert_eq!(machine_payload["track_dirty_pages"], true);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(
            vm_config_payload["boot-source"]["kernel_image_path"],
            kernel.path().to_str().unwrap()
        );
        assert_eq!(vm_config_payload["machine-config"]["mem_size_mib"], 512);
        assert_eq!(vm_config_payload["machine-config"]["vcpu_count"], 2);
    }

    #[tokio::test]
    async fn stores_balloon_config_and_exports_vm_config() {
        let router = test_state().router();

        let initial_get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initial_get_response.status(), StatusCode::BAD_REQUEST);
        let initial_get_body = to_bytes(initial_get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let initial_get_payload: ErrorEnvelope = serde_json::from_slice(&initial_get_body).unwrap();
        assert_eq!(initial_get_payload.code, "InvalidRequest");
        assert_eq!(
            initial_get_payload.fault_message,
            BALLOON_NOT_CONFIGURED_MESSAGE
        );

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":1,"deflate_on_oom":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_body = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let get_payload: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(get_payload["amount_mib"], 1);
        assert_eq!(get_payload["deflate_on_oom"], true);
        assert_eq!(get_payload["stats_polling_interval_s"], 0);
        assert_eq!(get_payload["free_page_hinting"], false);
        assert_eq!(get_payload["free_page_reporting"], false);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["balloon"]["amount_mib"], 1);
        assert_eq!(vm_config_payload["balloon"]["deflate_on_oom"], true);
        assert_eq!(vm_config_payload["balloon"]["stats_polling_interval_s"], 0);
        assert_eq!(vm_config_payload["balloon"]["free_page_hinting"], false);
        assert_eq!(vm_config_payload["balloon"]["free_page_reporting"], false);
    }

    #[tokio::test]
    async fn rejects_balloon_config_larger_than_guest_memory() {
        let router = test_state().router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":129,"deflate_on_oom":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(
            payload.fault_message,
            BALLOON_TOO_MANY_PAGES_REQUESTED_MESSAGE
        );
    }

    #[tokio::test]
    async fn rejects_machine_config_smaller_than_balloon_target() {
        let router = test_state().router();

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":64,"deflate_on_oom":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        for request in [
            Request::builder()
                .method("PATCH")
                .uri("/machine-config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(r#"{"mem_size_mib":32}"#))
                .unwrap(),
            Request::builder()
                .method("PUT")
                .uri("/machine-config")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"vcpu_count":1,"mem_size_mib":32}"#,
                ))
                .unwrap(),
        ] {
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
            assert_eq!(payload.code, "InvalidRequest");
            assert_eq!(
                payload.fault_message,
                MACHINE_CONFIG_INCOMPATIBLE_BALLOON_SIZE_MESSAGE
            );
        }
    }

    #[tokio::test]
    async fn runtime_balloon_read_and_update_require_configured_device() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_response.status(), StatusCode::BAD_REQUEST);
        let get_body = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let get_payload: ErrorEnvelope = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(get_payload.code, "InvalidRequest");
        assert_eq!(
            get_payload.fault_message,
            BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE
        );

        let patch_response = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"amount_mib":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch_response.status(), StatusCode::BAD_REQUEST);
        let patch_body = to_bytes(patch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let patch_payload: ErrorEnvelope = serde_json::from_slice(&patch_body).unwrap();
        assert_eq!(patch_payload.code, "InvalidRequest");
        assert_eq!(
            patch_payload.fault_message,
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE
        );
    }

    #[tokio::test]
    async fn updates_balloon_target_after_boot_and_rejects_oversized_runtime_target() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":1,"deflate_on_oom":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"amount_mib":64}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

        let get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_body = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let get_payload: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(get_payload["amount_mib"], 64);
        assert_eq!(get_payload["deflate_on_oom"], true);

        let oversized_response = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"amount_mib":129}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized_response.status(), StatusCode::BAD_REQUEST);
        let oversized_body = to_bytes(oversized_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let oversized_payload: ErrorEnvelope = serde_json::from_slice(&oversized_body).unwrap();
        assert_eq!(oversized_payload.code, "InvalidRequest");
        assert_eq!(
            oversized_payload.fault_message,
            balloon_update_too_large_message(DEFAULT_MEM_SIZE_MIB)
        );
    }

    #[tokio::test]
    async fn runtime_balloon_stats_require_configured_device_and_enabled_stats() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let missing_get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/statistics")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_get_response.status(), StatusCode::BAD_REQUEST);
        let missing_get_body = to_bytes(missing_get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let missing_get_payload: ErrorEnvelope = serde_json::from_slice(&missing_get_body).unwrap();
        assert_eq!(missing_get_payload.code, "InvalidRequest");
        assert_eq!(
            missing_get_payload.fault_message,
            BALLOON_RUNTIME_NOT_CONFIGURED_MESSAGE
        );

        let missing_patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon/statistics")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"stats_polling_interval_s":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_patch_response.status(), StatusCode::BAD_REQUEST);
        let missing_patch_body = to_bytes(missing_patch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let missing_patch_payload: ErrorEnvelope =
            serde_json::from_slice(&missing_patch_body).unwrap();
        assert_eq!(missing_patch_payload.code, "InvalidRequest");
        assert_eq!(
            missing_patch_payload.fault_message,
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE
        );

        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":1,"deflate_on_oom":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let disabled_get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/statistics")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disabled_get_response.status(), StatusCode::BAD_REQUEST);
        let disabled_get_body = to_bytes(disabled_get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let disabled_get_payload: ErrorEnvelope =
            serde_json::from_slice(&disabled_get_body).unwrap();
        assert_eq!(disabled_get_payload.code, "InvalidRequest");
        assert_eq!(
            disabled_get_payload.fault_message,
            BALLOON_STATS_DISABLED_MESSAGE
        );

        let disabled_patch_response = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon/statistics")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"stats_polling_interval_s":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disabled_patch_response.status(), StatusCode::BAD_REQUEST);
        let disabled_patch_body = to_bytes(disabled_patch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let disabled_patch_payload: ErrorEnvelope =
            serde_json::from_slice(&disabled_patch_body).unwrap();
        assert_eq!(disabled_patch_payload.code, "InvalidRequest");
        assert_eq!(
            disabled_patch_payload.fault_message,
            BALLOON_STATS_STATE_CHANGE_MESSAGE
        );
    }

    #[tokio::test]
    async fn runtime_balloon_stats_return_required_fields_and_allow_nonzero_interval_updates() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":64,"deflate_on_oom":true,"stats_polling_interval_s":1}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/statistics")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_body = to_bytes(get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let get_payload: serde_json::Value = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(get_payload["target_mib"], 64);
        assert_eq!(get_payload["actual_mib"], 64);
        assert_eq!(get_payload["target_pages"], 16384);
        assert_eq!(get_payload["actual_pages"], 16384);

        let patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon/statistics")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"stats_polling_interval_s":2}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

        let disable_response = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon/statistics")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"stats_polling_interval_s":0}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disable_response.status(), StatusCode::BAD_REQUEST);
        let disable_body = to_bytes(disable_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let disable_payload: ErrorEnvelope = serde_json::from_slice(&disable_body).unwrap();
        assert_eq!(disable_payload.code, "InvalidRequest");
        assert_eq!(
            disable_payload.fault_message,
            BALLOON_STATS_STATE_CHANGE_MESSAGE
        );
    }

    #[tokio::test]
    async fn runtime_balloon_hinting_status_requires_configured_enabled_device() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let firecracker = test_state().firecracker_revision;

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let unconfigured_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/hinting/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unconfigured_response.status(), StatusCode::BAD_REQUEST);
        let unconfigured_body = to_bytes(unconfigured_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let unconfigured_payload: ErrorEnvelope =
            serde_json::from_slice(&unconfigured_body).unwrap();
        assert_eq!(unconfigured_payload.code, "InvalidRequest");
        assert_eq!(
            unconfigured_payload.fault_message,
            BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE
        );
        assert_eq!(
            unconfigured_payload.baseline_firecracker_revision,
            firecracker
        );

        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let firecracker = test_state().firecracker_revision;

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let disabled_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/hinting/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(disabled_response.status(), StatusCode::BAD_REQUEST);
        let disabled_body = to_bytes(disabled_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let disabled_payload: ErrorEnvelope = serde_json::from_slice(&disabled_body).unwrap();
        assert_eq!(disabled_payload.code, "InvalidRequest");
        assert_eq!(
            disabled_payload.fault_message,
            BALLOON_HINTING_NOT_ENABLED_MESSAGE
        );
        assert_eq!(disabled_payload.baseline_firecracker_revision, firecracker);
    }

    #[tokio::test]
    async fn runtime_balloon_hinting_status_returns_idle_status_when_enabled() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/hinting/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["host_cmd"], 0);
        assert!(payload["guest_cmd"].is_null());
    }

    #[tokio::test]
    async fn runtime_balloon_hinting_control_requires_configured_enabled_device() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let firecracker = test_state().firecracker_revision;

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        for (path, body) in [
            ("/balloon/hinting/start", axum::body::Body::empty()),
            ("/balloon/hinting/stop", axum::body::Body::from("{not-json")),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PATCH")
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
            assert_eq!(payload.code, "InvalidRequest");
            assert_eq!(payload.fault_message, BALLOON_UPDATE_NOT_CONFIGURED_MESSAGE);
            assert_eq!(payload.baseline_firecracker_revision, firecracker);
        }

        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let firecracker = test_state().firecracker_revision;

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":false}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        for path in ["/balloon/hinting/start", "/balloon/hinting/stop"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PATCH")
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
            assert_eq!(payload.code, "InvalidRequest");
            assert_eq!(payload.fault_message, BALLOON_HINTING_NOT_ENABLED_MESSAGE);
            assert_eq!(payload.baseline_firecracker_revision, firecracker);
        }
    }

    #[tokio::test]
    async fn runtime_balloon_hinting_start_and_stop_follow_low_risk_control_flow() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":64,"deflate_on_oom":true,"free_page_hinting":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let first_start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon/hinting/start")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_start_response.status(), StatusCode::NO_CONTENT);

        let first_status_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/hinting/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_status_response.status(), StatusCode::OK);
        let first_status_body = to_bytes(first_status_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let first_status: serde_json::Value = serde_json::from_slice(&first_status_body).unwrap();
        assert_eq!(first_status["host_cmd"], BALLOON_HINTING_FIRST_RUN_COMMAND);
        assert!(first_status["guest_cmd"].is_null());

        let second_start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon/hinting/start")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"acknowledge_on_stop":false}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_start_response.status(), StatusCode::NO_CONTENT);

        let second_status_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/hinting/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_status_response.status(), StatusCode::OK);
        let second_status_body = to_bytes(second_status_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let second_status: serde_json::Value = serde_json::from_slice(&second_status_body).unwrap();
        assert_eq!(
            second_status["host_cmd"],
            BALLOON_HINTING_FIRST_RUN_COMMAND + 1
        );
        assert!(second_status["guest_cmd"].is_null());

        let stop_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/balloon/hinting/stop")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from("{not-json"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stop_response.status(), StatusCode::NO_CONTENT);

        let stopped_status_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/balloon/hinting/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(stopped_status_response.status(), StatusCode::OK);
        let stopped_status_body = to_bytes(stopped_status_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let stopped_status: serde_json::Value =
            serde_json::from_slice(&stopped_status_body).unwrap();
        assert_eq!(stopped_status["host_cmd"], BALLOON_HINTING_DONE_COMMAND);
        assert!(stopped_status["guest_cmd"].is_null());
    }

    #[tokio::test]
    async fn stores_memory_hotplug_config_and_exports_runtime_status() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let initial_vm_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initial_vm_config_response.status(), StatusCode::OK);
        let initial_vm_config_body = to_bytes(initial_vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let initial_vm_config_payload: serde_json::Value =
            serde_json::from_slice(&initial_vm_config_body).unwrap();
        assert!(initial_vm_config_payload.get("memory-hotplug").is_none());

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/hotplug/memory")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"total_size_mib":1024}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["memory-hotplug"]["block_size_mib"], 2);
        assert_eq!(vm_config_payload["memory-hotplug"]["slot_size_mib"], 128);
        assert_eq!(vm_config_payload["memory-hotplug"]["total_size_mib"], 1024);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let status_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/hotplug/memory")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(status_response.status(), StatusCode::OK);
        let status_body = to_bytes(status_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let status_payload: serde_json::Value = serde_json::from_slice(&status_body).unwrap();
        assert_eq!(status_payload["block_size_mib"], 2);
        assert_eq!(status_payload["slot_size_mib"], 128);
        assert_eq!(status_payload["total_size_mib"], 1024);
        assert_eq!(status_payload["plugged_size_mib"], 0);
        assert_eq!(status_payload["requested_size_mib"], 0);

        let patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/hotplug/memory")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"requested_size_mib":512}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch_response.status(), StatusCode::NO_CONTENT);

        let updated_status_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/hotplug/memory")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated_status_response.status(), StatusCode::OK);
        let updated_status_body = to_bytes(updated_status_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let updated_status_payload: serde_json::Value =
            serde_json::from_slice(&updated_status_body).unwrap();
        assert_eq!(updated_status_payload["requested_size_mib"], 512);
    }

    #[tokio::test]
    async fn stores_mmds_config_and_preserves_runtime_datastore() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let initial_get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/mmds")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initial_get_response.status(), StatusCode::OK);
        let initial_get_body = to_bytes(initial_get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let initial_get_payload: serde_json::Value =
            serde_json::from_slice(&initial_get_body).unwrap();
        assert_eq!(initial_get_payload, serde_json::json!({}));

        let put_mmds_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"meta":{"name":"vesper"},"remove":"later"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_mmds_response.status(), StatusCode::NO_CONTENT);

        let patch_mmds_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/mmds")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"meta":{"version":"1"},"remove":null}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(patch_mmds_response.status(), StatusCode::NO_CONTENT);

        let get_mmds_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/mmds")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get_mmds_response.status(), StatusCode::OK);
        let get_mmds_body = to_bytes(get_mmds_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let get_mmds_payload: serde_json::Value = serde_json::from_slice(&get_mmds_body).unwrap();
        assert_eq!(get_mmds_payload["meta"]["name"], "vesper");
        assert_eq!(get_mmds_payload["meta"]["version"], "1");
        assert!(get_mmds_payload.get("remove").is_none());

        let put_network_interface_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_network_interface_response.status(),
            StatusCode::NO_CONTENT
        );

        let put_mmds_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds/config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"network_interfaces":["eth0"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_mmds_config_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(
            vm_config_payload["mmds-config"]["network_interfaces"][0],
            "eth0"
        );
        assert_eq!(vm_config_payload["mmds-config"]["version"], "V1");
        assert_eq!(
            vm_config_payload["mmds-config"]["ipv4_address"],
            "169.254.169.254"
        );
        assert_eq!(vm_config_payload["mmds-config"]["imds_compat"], false);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let runtime_get_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/mmds")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(runtime_get_response.status(), StatusCode::OK);
        let runtime_get_body = to_bytes(runtime_get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let runtime_get_payload: serde_json::Value =
            serde_json::from_slice(&runtime_get_body).unwrap();
        assert_eq!(runtime_get_payload["meta"]["name"], "vesper");
        assert_eq!(runtime_get_payload["meta"]["version"], "1");

        let runtime_patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/mmds")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"meta":{"build":"2"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(runtime_patch_response.status(), StatusCode::NO_CONTENT);

        let updated_runtime_get_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/mmds")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(updated_runtime_get_response.status(), StatusCode::OK);
        let updated_runtime_get_body =
            to_bytes(updated_runtime_get_response.into_body(), usize::MAX)
                .await
                .unwrap();
        let updated_runtime_get_payload: serde_json::Value =
            serde_json::from_slice(&updated_runtime_get_body).unwrap();
        assert_eq!(updated_runtime_get_payload["meta"]["name"], "vesper");
        assert_eq!(updated_runtime_get_payload["meta"]["version"], "1");
        assert_eq!(updated_runtime_get_payload["meta"]["build"], "2");
    }

    #[tokio::test]
    async fn mmds_patch_requires_initialized_store_and_runtime_config() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let preboot_patch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/mmds")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"meta":{"name":"vesper"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(preboot_patch_response.status(), StatusCode::BAD_REQUEST);
        let preboot_patch_body = to_bytes(preboot_patch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let preboot_patch_payload: ErrorEnvelope =
            serde_json::from_slice(&preboot_patch_body).unwrap();
        assert_eq!(preboot_patch_payload.code, "InvalidRequest");
        assert_eq!(
            preboot_patch_payload.fault_message,
            MMDS_NOT_INITIALIZED_MESSAGE
        );

        let put_mmds_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"meta":{"name":"vesper"}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_mmds_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let runtime_get_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/mmds")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(runtime_get_response.status(), StatusCode::BAD_REQUEST);
        let runtime_get_body = to_bytes(runtime_get_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let runtime_get_payload: ErrorEnvelope = serde_json::from_slice(&runtime_get_body).unwrap();
        assert_eq!(runtime_get_payload.code, "InvalidRequest");
        assert_eq!(
            runtime_get_payload.fault_message,
            MMDS_NOT_INITIALIZED_MESSAGE
        );
    }

    #[tokio::test]
    async fn rejects_invalid_mmds_config_payloads_and_oversized_store() {
        let router = test_state().router();

        let empty_networks_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds/config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"network_interfaces":[]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(empty_networks_response.status(), StatusCode::BAD_REQUEST);
        let empty_networks_body = to_bytes(empty_networks_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let empty_networks_payload: ErrorEnvelope =
            serde_json::from_slice(&empty_networks_body).unwrap();
        assert_eq!(empty_networks_payload.code, "InvalidRequest");
        assert_eq!(
            empty_networks_payload.fault_message,
            MMDS_EMPTY_NETWORK_INTERFACE_LIST_MESSAGE
        );

        let put_network_interface_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_network_interface_response.status(),
            StatusCode::NO_CONTENT
        );

        let unknown_interface_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds/config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"network_interfaces":["missing"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(unknown_interface_response.status(), StatusCode::BAD_REQUEST);
        let unknown_interface_body = to_bytes(unknown_interface_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let unknown_interface_payload: ErrorEnvelope =
            serde_json::from_slice(&unknown_interface_body).unwrap();
        assert_eq!(unknown_interface_payload.code, "InvalidRequest");
        assert_eq!(
            unknown_interface_payload.fault_message,
            MMDS_INVALID_NETWORK_INTERFACE_ID_MESSAGE
        );

        let invalid_ipv4_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds/config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"network_interfaces":["eth0"],"ipv4_address":"10.0.0.1"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_ipv4_response.status(), StatusCode::BAD_REQUEST);
        let invalid_ipv4_body = to_bytes(invalid_ipv4_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let invalid_ipv4_payload: ErrorEnvelope =
            serde_json::from_slice(&invalid_ipv4_body).unwrap();
        assert_eq!(invalid_ipv4_payload.code, "InvalidRequest");
        assert_eq!(
            invalid_ipv4_payload.fault_message,
            MMDS_INVALID_IPV4_ADDRESS_MESSAGE
        );

        let oversized_payload = format!(
            r#"{{"blob":"{}"}}"#,
            "a".repeat(MMDS_DATA_STORE_LIMIT_BYTES)
        );
        let oversized_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(oversized_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(oversized_response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let oversized_body = to_bytes(oversized_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let oversized_payload: ErrorEnvelope = serde_json::from_slice(&oversized_body).unwrap();
        assert_eq!(oversized_payload.code, "PayloadTooLarge");
        assert_eq!(
            oversized_payload.fault_message,
            MMDS_DATA_STORE_LIMIT_EXCEEDED_MESSAGE
        );
    }

    #[tokio::test]
    async fn validates_and_exports_boot_source() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let initrd = NamedTempFile::new().unwrap();

        let put_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                Some(initrd.path().to_str().unwrap()),
            ))
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(
            vm_config_payload["boot-source"]["kernel_image_path"],
            kernel.path().to_str().unwrap()
        );
        assert_eq!(
            vm_config_payload["boot-source"]["initrd_path"],
            initrd.path().to_str().unwrap()
        );
    }

    #[tokio::test]
    async fn stores_cpu_config_without_exporting_it() {
        let router = test_state().router();

        let initial_vm_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(initial_vm_config_response.status(), StatusCode::OK);
        let initial_vm_config_body = to_bytes(initial_vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let initial_vm_config_payload: serde_json::Value =
            serde_json::from_slice(&initial_vm_config_body).unwrap();
        assert!(initial_vm_config_payload.get("cpu-config").is_none());

        let cpu_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/cpu-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"kvm_capabilities":["1","!2"],"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"eax","bitmap":"0bxxxxxxxxxxxxxxxxxxxxxxxxxxxx0001"}]}],"msr_modifiers":[{"addr":"0x10a","bitmap":"0b0000000000000000000000000000000000000000000000000000000000000001"}],"reg_modifiers":[{"addr":"0x0","bitmap":"0b1"}],"vcpu_features":[{"index":0,"bitmap":"0b00000000000000000000000001100000"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cpu_config_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert!(vm_config_payload.get("cpu-config").is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_cpu_config_payloads() {
        let router = test_state().router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/cpu-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"invalid","bitmap":"0b1"}]}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(payload.path, "/cpu-config");
    }

    #[tokio::test]
    async fn rejects_invalid_boot_source_payloads() {
        let router = test_state().router();

        let invalid_kernel_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/boot-source")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"kernel_image_path":"/definitely/missing-kernel"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_kernel_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(invalid_kernel_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert!(
            payload
                .fault_message
                .starts_with("The kernel file cannot be opened:")
        );
    }

    #[tokio::test]
    async fn rejects_invalid_memory_hotplug_payloads() {
        let router = test_state().router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/hotplug/memory")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"total_size_mib":130}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(
            payload.fault_message,
            INVALID_MEMORY_HOTPLUG_TOTAL_SIZE_MESSAGE
        );
    }

    #[tokio::test]
    async fn rejects_instance_start_without_boot_source() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(payload.fault_message, MISSING_KERNEL_CONFIG_MESSAGE);
    }

    #[tokio::test]
    async fn rejects_flush_metrics_before_start() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"FlushMetrics"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE);
    }

    #[tokio::test]
    async fn flush_metrics_succeeds_after_start_without_metrics_init() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let flush_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"FlushMetrics"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(flush_response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn rejects_instance_start_after_boot_with_firecracker_postboot_message() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let first_start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_start_response.status(), StatusCode::NO_CONTENT);

        let second_start_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_start_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(second_start_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_POSTBOOT_MESSAGE);
    }

    #[tokio::test]
    #[cfg(target_arch = "x86_64")]
    async fn rejects_send_ctrl_alt_del_before_start() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"action_type":"SendCtrlAltDel"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, ACTION_NOT_SUPPORTED_PREBOOT_MESSAGE);
    }

    #[tokio::test]
    #[cfg(target_arch = "x86_64")]
    async fn send_ctrl_alt_del_succeeds_after_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let ctrl_alt_del_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"action_type":"SendCtrlAltDel"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(ctrl_alt_del_response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    #[cfg(target_arch = "aarch64")]
    async fn rejects_send_ctrl_alt_del_on_aarch64() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"action_type":"SendCtrlAltDel"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(payload.fault_message, SEND_CTRL_ALT_DEL_AARCH64_MESSAGE);
    }

    #[tokio::test]
    async fn merges_logger_updates_and_rejects_invalid_targets() {
        let state = Arc::new(test_state());

        let first_response = handle_put_logger(
            &state,
            &Method::PUT,
            "/logger",
            Bytes::from_static(br#"{"level":"warning"}"#),
        );
        assert_eq!(first_response.status(), StatusCode::NO_CONTENT);

        let second_response = handle_put_logger(
            &state,
            &Method::PUT,
            "/logger",
            Bytes::from_static(br#"{"show_level":true,"module":"api_server::request"}"#),
        );
        assert_eq!(second_response.status(), StatusCode::NO_CONTENT);

        let logger_config = state
            .logger_config
            .read()
            .expect("logger config lock poisoned while reading")
            .clone();
        assert_eq!(logger_config.level, LoggerLevel::Warn);
        assert!(logger_config.show_level);
        assert!(!logger_config.show_log_origin);
        assert_eq!(logger_config.module.as_deref(), Some("api_server::request"));

        let invalid_response = handle_put_logger(
            &state,
            &Method::PUT,
            "/logger",
            Bytes::from_static(br#"{"log_path":"/definitely/missing-dir/logger.log"}"#),
        );
        assert_eq!(invalid_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(invalid_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert!(
            payload
                .fault_message
                .starts_with("Failed to open target file:")
        );
    }

    #[tokio::test]
    async fn initializes_metrics_once_and_omits_runtime_export() {
        let router = test_state().router();
        let tempdir = tempfile::tempdir().unwrap();
        let metrics_path = tempdir.path().join("metrics.jsonl");
        let metrics_payload = format!(r#"{{"metrics_path":"{}"}}"#, metrics_path.display());

        let first_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/metrics")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(metrics_payload.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_response.status(), StatusCode::NO_CONTENT);

        let duplicate_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/metrics")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(metrics_payload))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(duplicate_response.status(), StatusCode::BAD_REQUEST);
        let duplicate_body = to_bytes(duplicate_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let duplicate_payload: ErrorEnvelope = serde_json::from_slice(&duplicate_body).unwrap();
        assert_eq!(duplicate_payload.code, "InvalidRequest");
        assert_eq!(
            duplicate_payload.fault_message,
            METRICS_ALREADY_INITIALIZED_MESSAGE
        );

        let vm_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert!(vm_config_payload.get("logger").is_none());
        assert!(vm_config_payload.get("metrics").is_none());
    }

    #[tokio::test]
    async fn stores_serial_config_and_omits_runtime_export() {
        let state = Arc::new(test_state());

        let serial_response = handle_put_serial(
            &state,
            &Method::PUT,
            "/serial",
            Bytes::from_static(br#"{"serial_out_path":"serial.log"}"#),
        );
        assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);

        let serial_config = state
            .serial_config
            .read()
            .expect("serial config lock poisoned while reading")
            .clone();
        assert_eq!(serial_config.serial_out_path.as_deref(), Some("serial.log"));

        let vm_config_response = full_vm_config_response(&state);
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert!(vm_config_payload.get("serial").is_none());
    }

    #[tokio::test]
    async fn stores_entropy_config_and_exports_it() {
        let router = test_state().router();

        let entropy_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/entropy")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"rate_limiter":{"bandwidth":{"size":4096,"refill_time":1000,"one_time_burst":512},"ops":{"size":32,"refill_time":100}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(entropy_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(
            vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["size"],
            4096
        );
        assert_eq!(
            vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["refill_time"],
            1000
        );
        assert_eq!(
            vm_config_payload["entropy"]["rate_limiter"]["bandwidth"]["one_time_burst"],
            512
        );
        assert_eq!(
            vm_config_payload["entropy"]["rate_limiter"]["ops"]["size"],
            32
        );
        assert_eq!(
            vm_config_payload["entropy"]["rate_limiter"]["ops"]["refill_time"],
            100
        );
        assert!(
            vm_config_payload["entropy"]["rate_limiter"]["ops"]
                .get("one_time_burst")
                .is_none()
        );
    }

    #[tokio::test]
    async fn rejects_entropy_after_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let entropy_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/entropy")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(entropy_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(entropy_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
    }

    #[tokio::test]
    async fn rejects_cpu_config_after_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let cpu_config_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/cpu-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"kvm_capabilities":["1"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cpu_config_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(cpu_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
    }

    #[tokio::test]
    async fn stores_vsock_config_and_omits_deprecated_export_field() {
        let router = test_state().router();

        let vsock_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/vsock")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock","vsock_id":"deprecated"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vsock_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["vsock"]["guest_cid"], 5);
        assert_eq!(
            vm_config_payload["vsock"]["uds_path"],
            "/tmp/vesper-vsock.sock"
        );
        assert!(vm_config_payload["vsock"].get("vsock_id").is_none());
    }

    #[tokio::test]
    async fn rejects_invalid_vsock_guest_cid() {
        let router = test_state().router();

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/vsock")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"guest_cid":2,"uds_path":"/tmp/vesper-vsock.sock"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
        assert_eq!(payload.fault_message, INVALID_VSOCK_GUEST_CID_MESSAGE);
    }

    #[tokio::test]
    async fn drive_stub_validates_path_body_id_match() {
        let router = test_state().router();
        let root_backing_file = NamedTempFile::new().unwrap();

        let mismatch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"drive_id":"data","is_root_device":true,"path_on_host":"/tmp/rootfs.ext4"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
        let mismatch_body = to_bytes(mismatch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let mismatch_payload: ErrorEnvelope = serde_json::from_slice(&mismatch_body).unwrap();
        assert_eq!(mismatch_payload.code, "InvalidRequest");
        assert_eq!(
            mismatch_payload.fault_message,
            DRIVE_ID_PATH_MISMATCH_MESSAGE
        );

        let valid_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["drives"][0]["drive_id"], "rootfs");
    }

    #[tokio::test]
    async fn drive_stub_enforces_root_device_conflicts() {
        let router = test_state().router();
        let pmem_root_backing_file = NamedTempFile::new().unwrap();
        pmem_root_backing_file.as_file().set_len(0x1000).unwrap();

        let pmem_root_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/pmem0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"pmem0","path_on_host":"{}","root_device":true}}"#,
                        pmem_root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pmem_root_response.status(), StatusCode::NO_CONTENT);

        let pmem_conflict_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"drive_id":"rootfs","is_root_device":true,"path_on_host":"/tmp/rootfs.ext4"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pmem_conflict_response.status(), StatusCode::BAD_REQUEST);
        let pmem_conflict_body = to_bytes(pmem_conflict_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let pmem_conflict_payload: ErrorEnvelope =
            serde_json::from_slice(&pmem_conflict_body).unwrap();
        assert_eq!(pmem_conflict_payload.code, "InvalidRequest");
        assert_eq!(
            pmem_conflict_payload.fault_message,
            DRIVE_ROOT_PMEM_CONFLICT_MESSAGE
        );

        let second_router = test_state().router();
        let first_root_backing_file = NamedTempFile::new().unwrap();
        let replacement_root_backing_file = NamedTempFile::new().unwrap();
        let first_root_response = second_router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        first_root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_root_response.status(), StatusCode::NO_CONTENT);

        let second_root_response = second_router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/data")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"drive_id":"data","is_root_device":true,"path_on_host":"/tmp/data.ext4"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_root_response.status(), StatusCode::BAD_REQUEST);
        let second_root_body = to_bytes(second_root_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let second_root_payload: ErrorEnvelope = serde_json::from_slice(&second_root_body).unwrap();
        assert_eq!(second_root_payload.code, "InvalidRequest");
        assert_eq!(
            second_root_payload.fault_message,
            DRIVE_ROOT_ALREADY_EXISTS_MESSAGE
        );

        let update_existing_root_response = second_router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        replacement_root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            update_existing_root_response.status(),
            StatusCode::NO_CONTENT
        );

        let vm_config_response = second_router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["drives"][0]["drive_id"], "rootfs");
        assert_eq!(
            vm_config_payload["drives"][0]["path_on_host"],
            replacement_root_backing_file.path().display().to_string()
        );
    }

    #[tokio::test]
    async fn drive_stub_rejects_missing_backing_files() {
        let router = test_state().router();
        let missing_backing_file = "/definitely/missing-drive.ext4";

        let missing_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        missing_backing_file
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_response.status(), StatusCode::BAD_REQUEST);
        let missing_body = to_bytes(missing_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let missing_payload: ErrorEnvelope = serde_json::from_slice(&missing_body).unwrap();
        assert_eq!(missing_payload.code, "InvalidRequest");
        assert_eq!(
            missing_payload.fault_message,
            format!(
                "Drive config error: Unable to create the virtio block device: Virtio backend error: Error manipulating the backing file: No such file or directory (os error 2) {}",
                missing_backing_file
            )
        );

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert!(vm_config_payload["drives"].is_null());
    }

    #[tokio::test]
    async fn drive_stub_defaults_cache_type_in_export() {
        let router = test_state().router();
        let root_backing_file = NamedTempFile::new().unwrap();

        let create_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["drives"][0]["cache_type"], "Unsafe");
    }

    #[tokio::test]
    async fn drive_export_orders_root_device_first() {
        let router = test_state().router();
        let data_backing_file = NamedTempFile::new().unwrap();
        let root_backing_file = NamedTempFile::new().unwrap();

        let data_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/data")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"data","is_root_device":false,"path_on_host":"{}"}}"#,
                        data_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(data_response.status(), StatusCode::NO_CONTENT);

        let root_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(root_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["drives"][0]["drive_id"], "rootfs");
        assert_eq!(vm_config_payload["drives"][1]["drive_id"], "data");
    }

    #[tokio::test]
    async fn runtime_drive_patch_requires_existing_drive_and_updates_config() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let root_backing_file = NamedTempFile::new().unwrap();

        let create_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let missing_drive_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/drives/data")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"drive_id":"data","path_on_host":"/tmp/data.ext4"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_drive_response.status(), StatusCode::BAD_REQUEST);
        let missing_drive_body = to_bytes(missing_drive_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let missing_drive_payload: ErrorEnvelope =
            serde_json::from_slice(&missing_drive_body).unwrap();
        assert_eq!(missing_drive_payload.code, "InvalidRequest");
        assert_eq!(
            missing_drive_payload.fault_message,
            DRIVE_UPDATE_NOT_FOUND_MESSAGE
        );

        let update_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"drive_id":"rootfs","path_on_host":"/tmp/rootfs-updated.ext4"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(
            vm_config_payload["drives"][0]["path_on_host"],
            "/tmp/rootfs-updated.ext4"
        );
    }

    #[tokio::test]
    async fn network_interface_patch_stub_validates_path_body_id_match() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let create_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::NO_CONTENT);

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let mismatch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth1","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
        let mismatch_body = to_bytes(mismatch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let mismatch_payload: ErrorEnvelope = serde_json::from_slice(&mismatch_body).unwrap();
        assert_eq!(mismatch_payload.code, "InvalidRequest");
        assert_eq!(
            mismatch_payload.fault_message,
            network_id_path_mismatch_message("eth0", "eth1")
        );

        let missing_interface_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth9")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth9","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_interface_response.status(), StatusCode::BAD_REQUEST);
        let missing_interface_body = to_bytes(missing_interface_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let missing_interface_payload: ErrorEnvelope =
            serde_json::from_slice(&missing_interface_body).unwrap();
        assert_eq!(missing_interface_payload.code, "InvalidRequest");
        assert_eq!(
            missing_interface_payload.fault_message,
            NETWORK_UPDATE_NOT_FOUND_MESSAGE
        );

        let valid_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth0","rx_rate_limiter":{"bandwidth":{"size":1024,"refill_time":1000}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(
            vm_config_payload["network-interfaces"][0]["iface_id"],
            "eth0"
        );
        assert_eq!(
            vm_config_payload["network-interfaces"][0]["rx_rate_limiter"]["bandwidth"]["size"],
            1024
        );
    }

    #[tokio::test]
    async fn pmem_stub_validates_path_body_id_match() {
        let router = test_state().router();
        let backing_file = NamedTempFile::new().unwrap();
        backing_file.as_file().set_len(0x1000).unwrap();

        let mismatch_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"id":"data","path_on_host":"/tmp/rootfs.img"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(mismatch_response.status(), StatusCode::BAD_REQUEST);
        let mismatch_body = to_bytes(mismatch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let mismatch_payload: ErrorEnvelope = serde_json::from_slice(&mismatch_body).unwrap();
        assert_eq!(mismatch_payload.code, "InvalidRequest");
        assert_eq!(
            mismatch_payload.fault_message,
            PMEM_ID_PATH_MISMATCH_MESSAGE
        );

        let valid_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"rootfs","path_on_host":"{}"}}"#,
                        backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["pmem"][0]["id"], "rootfs");
        assert_eq!(vm_config_payload["pmem"][0]["root_device"], false);
        assert_eq!(vm_config_payload["pmem"][0]["read_only"], false);
    }

    #[tokio::test]
    async fn pmem_stub_rejects_invalid_backing_files() {
        let router = test_state().router();
        let missing_dir = tempfile::tempdir().unwrap();
        let missing_path = missing_dir.path().join("missing.img");
        let zero_sized_backing_file = NamedTempFile::new().unwrap();

        let missing_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/pmem0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"pmem0","path_on_host":"{}"}}"#,
                        missing_path.display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(missing_response.status(), StatusCode::BAD_REQUEST);
        let missing_body = to_bytes(missing_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let missing_payload: ErrorEnvelope = serde_json::from_slice(&missing_body).unwrap();
        assert_eq!(missing_payload.code, "InvalidRequest");
        assert_eq!(
            missing_payload.fault_message,
            "Pmem device error: Unable to create the virtio-pmem device: Error accessing backing file: No such file or directory (os error 2)"
        );

        let zero_sized_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/pmem0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"pmem0","path_on_host":"{}"}}"#,
                        zero_sized_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(zero_sized_response.status(), StatusCode::BAD_REQUEST);
        let zero_sized_body = to_bytes(zero_sized_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let zero_sized_payload: ErrorEnvelope = serde_json::from_slice(&zero_sized_body).unwrap();
        assert_eq!(zero_sized_payload.code, "InvalidRequest");
        assert_eq!(
            zero_sized_payload.fault_message,
            PMEM_BACKING_FILE_ZERO_SIZE_MESSAGE
        );

        let vm_config_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert!(vm_config_payload["pmem"].is_null());
    }

    #[tokio::test]
    async fn pmem_stub_enforces_root_device_conflicts() {
        let router = test_state().router();
        let root_backing_file = NamedTempFile::new().unwrap();
        root_backing_file.as_file().set_len(0x1000).unwrap();
        let replacement_backing_file = NamedTempFile::new().unwrap();
        replacement_backing_file.as_file().set_len(0x1000).unwrap();
        let drive_root_backing_file = NamedTempFile::new().unwrap();

        let drive_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/drives/rootfs")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"drive_id":"rootfs","is_root_device":true,"path_on_host":"{}"}}"#,
                        drive_root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(drive_response.status(), StatusCode::NO_CONTENT);

        let block_root_conflict_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/pmem0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"pmem0","path_on_host":"{}","root_device":true}}"#,
                        root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            block_root_conflict_response.status(),
            StatusCode::BAD_REQUEST
        );
        let block_root_conflict_body =
            to_bytes(block_root_conflict_response.into_body(), usize::MAX)
                .await
                .unwrap();
        let block_root_conflict_payload: ErrorEnvelope =
            serde_json::from_slice(&block_root_conflict_body).unwrap();
        assert_eq!(block_root_conflict_payload.code, "InvalidRequest");
        assert_eq!(
            block_root_conflict_payload.fault_message,
            PMEM_ROOT_BLOCK_CONFLICT_MESSAGE
        );

        let second_router = test_state().router();
        let first_root_backing_file = NamedTempFile::new().unwrap();
        first_root_backing_file.as_file().set_len(0x1000).unwrap();
        let second_root_backing_file = NamedTempFile::new().unwrap();
        second_root_backing_file.as_file().set_len(0x1000).unwrap();
        let first_root_response = second_router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/pmem0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"pmem0","path_on_host":"{}","root_device":true}}"#,
                        first_root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first_root_response.status(), StatusCode::NO_CONTENT);

        let second_root_response = second_router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/pmem1")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"pmem1","path_on_host":"{}","root_device":true}}"#,
                        second_root_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(second_root_response.status(), StatusCode::BAD_REQUEST);
        let second_root_body = to_bytes(second_root_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let second_root_payload: ErrorEnvelope = serde_json::from_slice(&second_root_body).unwrap();
        assert_eq!(second_root_payload.code, "InvalidRequest");
        assert_eq!(
            second_root_payload.fault_message,
            PMEM_ROOT_ALREADY_EXISTS_MESSAGE
        );

        let update_existing_root_response = second_router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/pmem/pmem0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"id":"pmem0","path_on_host":"{}","root_device":true,"read_only":true}}"#,
                        replacement_backing_file.path().display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            update_existing_root_response.status(),
            StatusCode::NO_CONTENT
        );

        let vm_config_response = second_router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/vm/config")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vm_config_response.status(), StatusCode::OK);
        let vm_config_body = to_bytes(vm_config_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let vm_config_payload: serde_json::Value = serde_json::from_slice(&vm_config_body).unwrap();
        assert_eq!(vm_config_payload["pmem"][0]["id"], "pmem0");
        assert_eq!(vm_config_payload["pmem"][0]["root_device"], true);
        assert_eq!(vm_config_payload["pmem"][0]["read_only"], true);
    }

    #[tokio::test]
    async fn snapshot_load_stub_requires_exactly_one_memory_source() {
        let router = test_state().router();

        for (body, expected_message) in [
            (
                r#"{"snapshot_path":"state.snap"}"#,
                SNAPSHOT_LOAD_MISSING_MEMORY_SOURCE_MESSAGE,
            ),
            (
                r#"{"snapshot_path":"state.snap","mem_file_path":"mem.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                SNAPSHOT_LOAD_TOO_MANY_MEMORY_SOURCES_MESSAGE,
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri("/snapshot/load")
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
            assert_eq!(payload.code, "InvalidRequest");
            assert_eq!(payload.fault_message, expected_message);
        }

        let valid_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(valid_response.status(), StatusCode::NO_CONTENT);

        let instance_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(instance_response.status(), StatusCode::OK);
        let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
        assert_eq!(payload["state"], "Paused");
    }

    #[tokio::test]
    async fn snapshot_load_stub_rejects_when_non_logger_resources_are_configured() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let response_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
    }

    #[tokio::test]
    async fn snapshot_load_stub_allows_logger_metrics_and_serial() {
        let router = test_state().router();
        let tempdir = tempfile::tempdir().unwrap();
        let metrics_path = tempdir.path().join("snapshot-load-metrics.jsonl");

        let logger_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/logger")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"level":"info"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(logger_response.status(), StatusCode::NO_CONTENT);

        let metrics_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/metrics")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(format!(
                        r#"{{"metrics_path":"{}"}}"#,
                        metrics_path.display()
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(metrics_response.status(), StatusCode::NO_CONTENT);

        let serial_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/serial")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"serial_out_path":"serial.log"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);

        let snapshot_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(snapshot_response.status(), StatusCode::NO_CONTENT);

        let instance_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(instance_response.status(), StatusCode::OK);
        let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
        assert_eq!(payload["state"], "Paused");
    }

    #[tokio::test]
    async fn snapshot_create_stub_requires_paused_vm() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let running_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/create")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"mem_file_path":"mem.snap","snapshot_path":"state.snap"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(running_response.status(), StatusCode::BAD_REQUEST);
        let running_body = to_bytes(running_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let running_payload: ErrorEnvelope = serde_json::from_slice(&running_body).unwrap();
        assert_eq!(running_payload.code, "InvalidState");
        assert_eq!(
            running_payload.fault_message,
            SNAPSHOT_CREATE_REQUIRES_PAUSED_MESSAGE
        );

        let pause_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/vm")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"state":"Paused"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pause_response.status(), StatusCode::NO_CONTENT);

        let paused_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/create")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"mem_file_path":"mem.snap","snapshot_path":"state.snap"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(paused_response.status(), StatusCode::NO_CONTENT);

        let instance_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(instance_response.status(), StatusCode::OK);
        let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
        assert_eq!(payload["state"], "Paused");
    }

    #[tokio::test]
    async fn snapshot_load_stub_resumes_vm_when_requested() {
        let router = test_state().router();

        let snapshot_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_backend":{"backend_type":"File","backend_path":"mem.snap"},"resume_vm":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(snapshot_response.status(), StatusCode::NO_CONTENT);

        let instance_response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(instance_response.status(), StatusCode::OK);
        let instance_body = to_bytes(instance_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&instance_body).unwrap();
        assert_eq!(payload["state"], "Running");
    }

    #[tokio::test]
    async fn rejects_vsock_after_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let vsock_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/vsock")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(vsock_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(vsock_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
    }

    #[tokio::test]
    async fn allows_serial_after_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let serial_response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/serial")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"serial_out_path":"serial.log"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(serial_response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn allows_logger_and_metrics_after_start() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();
        let tempdir = tempfile::tempdir().unwrap();
        let metrics_path = tempdir.path().join("metrics-after-start.jsonl");

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        for (path, body) in [
            ("/logger", r#"{"level":"info"}"#.to_string()),
            (
                "/metrics",
                format!(r#"{{"metrics_path":"{}"}}"#, metrics_path.display()),
            ),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(path)
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
    }

    #[tokio::test]
    async fn rejects_invalid_machine_config_payloads() {
        let router = test_state().router();

        let invalid_mem_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"vcpu_count":2,"mem_size_mib":0}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(invalid_mem_response.status(), StatusCode::BAD_REQUEST);

        let empty_patch_response = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/machine-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(empty_patch_response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(empty_patch_response.into_body(), usize::MAX)
            .await
            .unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidRequest");
    }

    #[tokio::test]
    async fn rejects_vm_pause_before_start() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/vm")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"state":"Paused"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
    }

    #[tokio::test]
    async fn pauses_and_resumes_vm_state() {
        let router = test_state().router();
        let kernel = NamedTempFile::new().unwrap();

        let boot_source_response = router
            .clone()
            .oneshot(put_boot_source_request(
                kernel.path().to_str().unwrap(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(boot_source_response.status(), StatusCode::NO_CONTENT);

        let start_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/actions")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"action_type":"InstanceStart"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(start_response.status(), StatusCode::NO_CONTENT);

        let pause_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/vm")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"state":"Paused"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pause_response.status(), StatusCode::NO_CONTENT);

        let paused_state = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(paused_state.status(), StatusCode::OK);
        let paused_body = to_bytes(paused_state.into_body(), usize::MAX)
            .await
            .unwrap();
        let paused_payload: serde_json::Value = serde_json::from_slice(&paused_body).unwrap();
        assert_eq!(paused_payload["state"], "Paused");

        let resume_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/vm")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"state":"Resumed"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resume_response.status(), StatusCode::NO_CONTENT);

        let resumed_state = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resumed_state.status(), StatusCode::OK);
        let resumed_body = to_bytes(resumed_state.into_body(), usize::MAX)
            .await
            .unwrap();
        let resumed_payload: serde_json::Value = serde_json::from_slice(&resumed_body).unwrap();
        assert_eq!(resumed_payload["state"], "Running");
    }

    #[tokio::test]
    async fn returns_not_found_for_unknown_route() {
        let router = test_state().router();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/unknown")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn snapshot_load_rejects_memory_hotplug_configuration() {
        let router = test_state().router();

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/hotplug/memory")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"total_size_mib":1024}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
    }

    #[tokio::test]
    async fn snapshot_load_rejects_cpu_config_configuration() {
        let router = test_state().router();

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/cpu-config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"kvm_capabilities":["1","!2"],"cpuid_modifiers":[{"leaf":"0x1","subleaf":"0x0","flags":0,"modifiers":[{"register":"eax","bitmap":"0bxxxxxxxxxxxxxxxxxxxxxxxxxxxx0001"}]}],"msr_modifiers":[{"addr":"0x10a","bitmap":"0b0000000000000000000000000000000000000000000000000000000000000001"}],"reg_modifiers":[{"addr":"0x0","bitmap":"0b1"}],"vcpu_features":[{"index":0,"bitmap":"0b00000000000000000000000001100000"}]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
    }

    #[tokio::test]
    async fn snapshot_load_rejects_entropy_configuration() {
        let router = test_state().router();

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/entropy")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"rate_limiter":{"bandwidth":{"size":4096,"refill_time":1000,"one_time_burst":512},"ops":{"size":32,"refill_time":100}}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
    }

    #[tokio::test]
    async fn snapshot_load_rejects_vsock_configuration() {
        let router = test_state().router();

        let put_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/vsock")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"guest_cid":5,"uds_path":"/tmp/vesper-vsock.sock","vsock_id":"deprecated"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
    }

    #[tokio::test]
    async fn snapshot_load_rejects_balloon_configuration() {
        let router = test_state().router();

        let put_balloon_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/balloon")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"amount_mib":64,"deflate_on_oom":true}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_balloon_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
    }

    #[tokio::test]
    async fn snapshot_load_rejects_mmds_configuration() {
        let router = test_state().router();

        let put_network_interface_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/network-interfaces/eth0")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"iface_id":"eth0","host_dev_name":"tap0"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            put_network_interface_response.status(),
            StatusCode::NO_CONTENT
        );

        let put_mmds_config_response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/mmds/config")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"network_interfaces":["eth0"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(put_mmds_config_response.status(), StatusCode::NO_CONTENT);

        let response = router
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/snapshot/load")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"snapshot_path":"state.snap","mem_file_path":"state.mem"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let payload: ErrorEnvelope = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload.code, "InvalidState");
        assert_eq!(payload.fault_message, SNAPSHOT_LOAD_RESOURCE_GUARD_MESSAGE);
    }
}
