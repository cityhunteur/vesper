use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, to_vec};
use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::sync::{Arc, RwLock};

mod actions;
mod balloon;
mod boot_source;
mod errors;
mod logger_metrics;
mod machine_config;
mod mmds;
mod pmem;
#[cfg(test)]
mod tests;
mod vm_config;

use self::actions::handle_actions;
use self::balloon::*;
use self::boot_source::handle_put_boot_source;
pub use self::errors::ErrorEnvelope;
use self::errors::*;
use self::logger_metrics::{handle_put_cpu_config, handle_put_logger, handle_put_metrics};
use self::machine_config::{
    handle_patch_machine_config, handle_put_machine_config, machine_config_response,
};
use self::mmds::*;
use self::pmem::{PmemConfigRequest, handle_patch_pmem, handle_put_pmem};
use self::vm_config::{
    firecracker_version_response, full_vm_config_response, handle_patch_vm, instance_info_response,
};
use crate::api_matrix::{
    RouteSemantics, SurfaceStubMatrix, generate_surface_stub_matrix, validate_surface_stub_matrix,
};

const DEFAULT_APP_NAME: &str = "Firecracker";
const DEFAULT_INSTANCE_ID: &str = "anonymous-instance";
const DEFAULT_MEM_SIZE_MIB: usize = 128;
const DEFAULT_MEMORY_HOTPLUG_BLOCK_SIZE_MIB: usize = 2;
const DEFAULT_MEMORY_HOTPLUG_SLOT_SIZE_MIB: usize = 128;
const MAX_SUPPORTED_VCPUS: u8 = 32;
const NETWORK_MTU_MIN: u16 = 68;
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

/// A pinned route. The type is `spec::Route` (TD-020); this name is kept for one release for callers outside the
/// crate.
pub use crate::spec::Route as RouteSpec;

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
    #[serde(default)]
    rate_limiter: Option<TokenBucket>,
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
struct NetworkInterfaceConfigRequest {
    #[serde(default)]
    guest_mac: Option<String>,
    host_dev_name: String,
    iface_id: String,
    #[serde(default)]
    mtu: Option<u16>,
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

/// Firecracker's `VsockOverride` does not deny unknown fields.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct VsockOverrideRequest {
    uds_path: String,
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
    #[serde(default)]
    vsock_override: Option<VsockOverrideRequest>,
    #[serde(default)]
    clock_realtime: bool,
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

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct BootSourceConfig {
    kernel_image_path: String,
    initrd_path: Option<String>,
    boot_args: Option<String>,
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

    // Firecracker v1.16.0 sends post-boot device PUTs to PCI hotplug after parsing the body, so
    // these handlers apply their own state check (`reject_runtime_hotplug`) instead of the guard.
    match (
        route_match.template.method.as_str(),
        route_match.template.template.as_str(),
    ) {
        ("PUT", "/drives/{drive_id}") => {
            return handle_put_drive_stub(&state, &method, &route_match, body);
        }
        ("PUT", "/network-interfaces/{iface_id}") => {
            return handle_put_network_interface_stub(&state, &method, &route_match, body);
        }
        ("PUT", "/pmem/{id}") => return handle_put_pmem(&state, &method, &route_match, body),
        _ => {}
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
        ("PATCH", "/drives/{drive_id}") => {
            return handle_patch_drive_stub(&state, &method, &route_match, body);
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
        ("PATCH", "/pmem/{id}") => return handle_patch_pmem(&state, &method, &route_match, body),
        ("PUT", "/snapshot/create") => {
            return handle_put_snapshot_create_stub(&state, &method, &route_match, body);
        }
        ("PUT", "/snapshot/load") => {
            return handle_put_snapshot_load_stub(&state, &method, &route_match, body);
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

    // Firecracker v1.16.0 does not check a minimum guest CID (`vmm_config/vsock.rs`).
    *state
        .vsock_config
        .write()
        .expect("vsock config lock poisoned while writing") = Some(payload.into());

    StatusCode::NO_CONTENT.into_response()
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

    if let Some(response) = reject_runtime_hotplug(state, method, &route.template.template) {
        return response;
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

    if let Some(response) = reject_runtime_hotplug(state, method, &route.template.template) {
        return response;
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

    if let Some(mtu) = payload.mtu.filter(|mtu| *mtu < NETWORK_MTU_MIN) {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            &network_invalid_mtu_message(mtu),
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
            &message,
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
            &message,
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

    // Firecracker rejects `clock_realtime` on aarch64 while building the microVM from the
    // snapshot (`BuildMicrovmFromSnapshotError::UnsupportedClockRealtime`); x86_64 accepts it.
    #[cfg(target_arch = "aarch64")]
    if payload.clock_realtime {
        return error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            SNAPSHOT_LOAD_CLOCK_REALTIME_AARCH64_MESSAGE,
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

    // `PUT /cpu-config` does not set Firecracker's `boot_path` (`set_custom_cpu_template`), so it does not block a load.

    if *state
        .machine_config
        .read()
        .expect("machine config lock poisoned while reading")
        != MachineConfig::default()
    {
        return true;
    }

    if !state
        .drives
        .read()
        .expect("drive config lock poisoned while reading")
        .is_empty()
    {
        return true;
    }

    if !state
        .network_interfaces
        .read()
        .expect("network interface config lock poisoned while reading")
        .is_empty()
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

    if !state
        .pmem_devices
        .read()
        .expect("pmem config lock poisoned while reading")
        .is_empty()
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

/// Post-boot `PUT /drives`, `/pmem` and `/network-interfaces` reach Firecracker's
/// `DeviceManager::hotplug_device`, which fails with `VmmActionError::PciNotEnabled` because
/// `vesperd` models a Firecracker process started without `--enable-pci`.
fn reject_runtime_hotplug(state: &AppState, method: &Method, template: &str) -> Option<Response> {
    let vm_state = *state
        .vm_state
        .read()
        .expect("vm state lock poisoned while reading");
    (vm_state != VmState::PreBoot).then(|| {
        error_response(
            &state.firecracker_revision,
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            PCI_NOT_ENABLED_MESSAGE,
            template,
            method.as_str(),
        )
    })
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

/// Firecracker's `MemoryHotplugConfig::validate` (`vmm_config/memory_hotplug.rs` at the pinned revision): the same
/// order and the same `displaydoc` texts, behind `VmmActionError::MemoryHotplugConfig`.
fn validate_memory_hotplug_config(config: &MemoryHotplugConfig) -> Result<(), String> {
    let why = if config.block_size_mib < DEFAULT_MEMORY_HOTPLUG_BLOCK_SIZE_MIB {
        format!("Block size must not be lower than {DEFAULT_MEMORY_HOTPLUG_BLOCK_SIZE_MIB} MiB")
    } else if !config.block_size_mib.is_power_of_two() {
        "Block size must be a power of 2".to_string()
    } else if config.slot_size_mib < DEFAULT_MEMORY_HOTPLUG_SLOT_SIZE_MIB {
        format!("Slot size must not be lower than {DEFAULT_MEMORY_HOTPLUG_SLOT_SIZE_MIB} MiB")
    } else if !config.slot_size_mib.is_multiple_of(config.block_size_mib) {
        format!(
            "Slot size must be a multiple of block size ({} MiB)",
            config.block_size_mib
        )
    } else if config.total_size_mib < config.slot_size_mib {
        format!(
            "Total size must not be lower than slot size ({} MiB)",
            config.slot_size_mib
        )
    } else if !config.total_size_mib.is_multiple_of(config.slot_size_mib) {
        format!(
            "Total size must be a multiple of slot size ({} MiB)",
            config.slot_size_mib
        )
    } else {
        return Ok(());
    };
    Err(format!("Memory hotplug config error: {why}"))
}

/// Firecracker's `VirtioMem::update_requested_size` checks (`devices/virtio/mem/device.rs`): a multiple of the block
/// size and no larger than the region; the size in the message is in bytes.
fn validate_memory_hotplug_size_update(
    config: &MemoryHotplugConfig,
    payload: &MemoryHotplugSizeUpdate,
) -> Result<(), String> {
    if !payload
        .requested_size_mib
        .is_multiple_of(config.block_size_mib)
        || payload.requested_size_mib > config.total_size_mib
    {
        let bytes = (payload.requested_size_mib as u64) << 20;
        return Err(format!(
            "Memory hotplug update error: Failed to create memory hotplug device: Size {bytes} is invalid: it must be a multiple of block size and less than the total size"
        ));
    }
    Ok(())
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
