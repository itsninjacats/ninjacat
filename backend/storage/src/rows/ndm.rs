//! Network Device Monitoring: eight tables. The first four come off /api/v2/ndm and repeat the
//! payload's namespace, subnet, integration and collect time, plus `extra`: the payload's
//! top-level keys nothing here declares, as JSON text by name.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};

use super::Tags;
use crate::column;
use crate::table::{Limits, Table};

/// A whole inventory lands at once, not bit by bit.
const INVENTORY: Limits =
    Limits { max_rows: 2000, flush_interval: Duration::from_secs(5), buffer_limit: 50_000, max_in_flight: 2 };

#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct NdmDevice {
    pub tenant_id: String,
    pub namespace: String,
    pub subnet: String,
    pub integration: String,
    #[serde(with = "column::secs")]
    pub collect_timestamp: DateTime<Utc>,
    pub extra: BTreeMap<String, String>,
    pub device_id: String,
    pub id_tags: Tags,
    pub tags: Tags,
    pub ip_address: String,
    pub status: DeviceStatus,
    pub ping_status: DeviceStatus,
    pub name: String,
    pub description: String,
    pub sys_object_id: String,
    pub location: String,
    pub profile: String,
    pub profile_version: u64,
    pub vendor: String,
    /// The device's own subnet and integration, next to the payload's.
    pub device_subnet: String,
    pub serial_number: String,
    pub version: String,
    pub product_name: String,
    pub model: String,
    pub os_name: String,
    pub os_version: String,
    pub os_hostname: String,
    pub device_integration: String,
    pub device_type: String,
}

/// The column's Enum8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceStatus {
    Unknown = 0,
    Reachable = 1,
    Unreachable = 2,
}

impl DeviceStatus {
    pub fn name(self) -> &'static str {
        match self {
            DeviceStatus::Unknown => "unknown",
            DeviceStatus::Reachable => "reachable",
            DeviceStatus::Unreachable => "unreachable",
        }
    }
}

impl Serialize for DeviceStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(self.name())
        } else {
            serializer.serialize_i8(*self as i8)
        }
    }
}

impl Table for NdmDevice {
    const WRITER: &'static str = "storage_ndm_devices";
    const TABLE: &'static str = "ndm_devices";
    const LIMITS: Limits = INVENTORY;
}

#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct NdmInterface {
    pub tenant_id: String,
    pub namespace: String,
    pub subnet: String,
    pub integration: String,
    #[serde(with = "column::secs")]
    pub collect_timestamp: DateTime<Utc>,
    pub extra: BTreeMap<String, String>,
    pub device_id: String,
    pub id_tags: Tags,
    pub if_index: i32,
    pub raw_id: String,
    pub raw_id_type: String,
    pub name: String,
    pub alias: String,
    pub description: String,
    pub mac_address: String,
    /// The wire's numbers, not decoded into names.
    pub admin_status: u8,
    pub oper_status: u8,
    #[serde(rename = "type")]
    pub kind: u32,
    /// None is "not reported", which is neither true nor false.
    pub is_physical: Option<u8>,
    pub meraki_enabled: Option<u8>,
    pub meraki_status: String,
}

impl Table for NdmInterface {
    const WRITER: &'static str = "storage_ndm_interfaces";
    const TABLE: &'static str = "ndm_interfaces";
    const LIMITS: Limits = INVENTORY;
}

#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct NdmIpAddress {
    pub tenant_id: String,
    pub namespace: String,
    pub subnet: String,
    pub integration: String,
    #[serde(with = "column::secs")]
    pub collect_timestamp: DateTime<Utc>,
    pub extra: BTreeMap<String, String>,
    pub interface_id: String,
    pub ip_address: String,
    pub prefixlen: i32,
}

impl Table for NdmIpAddress {
    const WRITER: &'static str = "storage_ndm_ip_addresses";
    const TABLE: &'static str = "ndm_ip_addresses";
    const LIMITS: Limits = INVENTORY;
}

/// One element of the payload's other lists (links, vpn_tunnels, netflow_exporters, diagnoses,
/// device_oids) or its scan_status, kept whole.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct NdmMetadataObject {
    pub tenant_id: String,
    pub namespace: String,
    pub subnet: String,
    pub integration: String,
    #[serde(with = "column::secs")]
    pub collect_timestamp: DateTime<Utc>,
    pub extra: BTreeMap<String, String>,
    pub kind: String,
    pub device_id: String,
    pub interface_id: String,
    pub object: String,
}

impl Table for NdmMetadataObject {
    const WRITER: &'static str = "storage_ndm_metadata_objects";
    const TABLE: &'static str = "ndm_metadata_objects";
    const LIMITS: Limits = INVENTORY;
}

/// One device configuration backup from /api/v2/ndmconfig. The payload's fields are repeated on
/// every config it carried.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct NdmDeviceConfig {
    pub tenant_id: String,
    pub namespace: String,
    #[serde(with = "column::secs")]
    pub collect_timestamp: DateTime<Utc>,
    pub agent_hostname: String,
    pub inventories: String,
    pub extra: BTreeMap<String, String>,
    pub device_id: String,
    pub device_ip: String,
    pub config_type: String,
    pub config_source: String,
    #[serde(with = "column::secs")]
    pub timestamp: DateTime<Utc>,
    pub tags: Tags,
    pub content: String,
}

impl Table for NdmDeviceConfig {
    const WRITER: &'static str = "storage_ndm_device_configs";
    const TABLE: &'static str = "ndm_device_configs";
    // A row is a whole configuration file: megabytes, not bytes.
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(10), buffer_limit: 5_000, max_in_flight: 1 };
}

#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SnmpTrap {
    pub tenant_id: String,
    #[serde(with = "column::secs")]
    pub timestamp: DateTime<Utc>,
    pub ddsource: String,
    pub ddtags: Tags,
    /// The `snmp_device` tag: the device that sent the trap.
    pub device: String,
    pub uptime: u32,
    pub snmp_trap_oid: String,
    pub snmp_trap_name: String,
    pub snmp_trap_mib: String,
    pub enterprise_oid: String,
    /// SNMP v1 only: None on a v2/v3 trap, which is not "generic trap 0".
    pub generic_trap: Option<i32>,
    pub specific_trap: Option<i32>,
    pub var_oids: Vec<String>,
    pub var_types: Vec<String>,
    pub var_values: Vec<String>,
    /// The trap's keys with no column, as JSON text by name.
    pub enriched: BTreeMap<String, String>,
    pub raw: String,
}

impl Table for SnmpTrap {
    const WRITER: &'static str = "storage_snmp_traps";
    const TABLE: &'static str = "snmp_traps";
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(5), buffer_limit: 20_000, max_in_flight: 2 };
}

#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct NetflowFlow {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub flush_timestamp: DateTime<Utc>,
    pub flow_type: String,
    pub sampling_rate: u64,
    pub direction: String,
    #[serde(with = "column::secs")]
    pub flow_start: DateTime<Utc>,
    #[serde(with = "column::secs")]
    pub flow_end: DateTime<Utc>,
    pub bytes: u64,
    pub packets: u64,
    pub ether_type: String,
    pub ip_protocol: String,
    pub tos: u32,
    pub dscp: u32,
    pub dscp_name: String,
    pub device_namespace: String,
    pub exporter_ip: String,
    pub source_ip: String,
    pub source_port: String,
    pub source_mac: String,
    pub source_mask: String,
    pub source_reverse_dns_hostname: String,
    pub destination_ip: String,
    pub destination_port: String,
    pub destination_mac: String,
    pub destination_mask: String,
    pub destination_reverse_dns_hostname: String,
    pub ingress_interface_index: u32,
    pub egress_interface_index: u32,
    pub host: String,
    pub tcp_flags: Vec<String>,
    pub next_hop_ip: String,
    /// The flow's configured extra fields, each value as JSON text.
    pub additional_fields: BTreeMap<String, String>,
}

impl Table for NetflowFlow {
    const WRITER: &'static str = "storage_netflow_flows";
    const TABLE: &'static str = "netflow_flows";
    // The busiest NDM table, in the class of logs.
    const LIMITS: Limits =
        Limits { max_rows: 20_000, flush_interval: Duration::from_secs(5), buffer_limit: 500_000, max_in_flight: 2 };
}

/// One traceroute result. Runs are parallel arrays; what belongs to a hop is an array per run.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct NetworkPath {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub agent_version: String,
    pub namespace: String,
    pub test_config_id: String,
    pub test_config_name: String,
    pub test_result_id: String,
    pub test_run_id: String,
    pub origin: String,
    pub test_run_type: String,
    pub test_config_source: String,
    pub source_product: String,
    pub collector_type: String,
    pub protocol: String,
    pub source_name: String,
    pub source_display_name: String,
    pub source_hostname: String,
    /// None when the payload had no `via`: the agent could not resolve a route, which is not the
    /// same as resolving one to an empty value.
    pub source_via_subnet_alias: Option<String>,
    pub source_via_interface_hardware_addr: Option<String>,
    pub source_network_id: String,
    pub source_service: String,
    pub source_container_id: String,
    pub source_public_ip: String,
    pub destination_hostname: String,
    pub destination_port: u16,
    pub destination_service: String,
    pub hop_count_avg: f64,
    pub hop_count_min: i32,
    pub hop_count_max: i32,
    pub run_ids: Vec<String>,
    pub run_source_ips: Vec<String>,
    pub run_source_ports: Vec<u16>,
    pub run_destination_ips: Vec<String>,
    pub run_destination_ports: Vec<u16>,
    pub run_destination_reverse_dns: Vec<Vec<String>>,
    pub hop_ttls: Vec<Vec<i32>>,
    pub hop_ips: Vec<Vec<String>>,
    pub hop_reverse_dns: Vec<Vec<Vec<String>>>,
    pub hop_rtts: Vec<Vec<f64>>,
    pub hop_reachable: Vec<Vec<u8>>,
    pub e2e_rtts: Vec<f64>,
    pub e2e_packets_sent: i32,
    pub e2e_packets_received: i32,
    pub e2e_packet_loss_percentage: f32,
    pub e2e_jitter: f64,
    pub e2e_rtt_avg: f64,
    pub e2e_rtt_min: f64,
    pub e2e_rtt_max: f64,
    pub tags: Tags,
}

impl Table for NetworkPath {
    const WRITER: &'static str = "storage_network_paths";
    const TABLE: &'static str = "network_paths";
    // Scheduled tests trickle in.
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}
