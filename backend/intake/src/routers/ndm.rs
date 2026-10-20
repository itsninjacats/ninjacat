//! Network Devices — four hosts, one product, all JSON.
//!
//!   ndm-intake.<site>         /api/v2/ndm        devices, interfaces, addresses, topology
//!   ndm-intake.<site>         /api/v2/ndmconfig  device configuration backups
//!   snmp-traps-intake.<site>  /api/v2/ndmtraps   SNMP traps, each under a "trap" key
//!   ndmflow-intake.<site>     /api/v2/ndmflow    NetFlow / sFlow / IPFIX
//!   netpath-intake.<site>     /api/v2/netpath    traceroute results
//!
//!   config: none; event platform tracks.
//!
//! They share a router because they are one product to the operator, even though Datadog gave
//! each its own hostname. Field names are the json tags of the agent's own types:
//! comp/netflow/payload, pkg/networkpath/payload, pkg/networkdevice/metadata,
//! pkg/networkconfigmanagement/report and comp/snmptraps/formatter.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::LazyLock;

use bytes::Bytes;
use ninjapaws_storage::rows::ndm::{
    DeviceStatus, NdmDevice, NdmDeviceConfig, NdmInterface, NdmIpAddress, NdmMetadataObject, NetflowFlow, NetworkPath,
    SnmpTrap,
};
use regex::Regex;
use serde_json::Value;
use tako::types::Response;

use crate::fields::{Fields, Mismatches, fields};
use crate::json;
use crate::server::{Ctx, Routes, accepted};
use crate::wire::{tags, text, time};

/// ndm-intake.<site>, snmp-traps-intake.<site>, ndmflow-intake.<site>, netpath-intake.<site>
pub fn mount(routes: &mut Routes) {
    routes.post("/api/v2/ndm", handle_metadata);
    routes.post("/api/v2/ndmconfig", handle_config);
    routes.post("/api/v2/ndmtraps", handle_traps);
    routes.post("/api/v2/ndmflow", handle_flow);
    routes.post("/api/v2/netpath", handle_netpath);
}

/// The value of the first `key:value` tag in a comma-joined ddtags, or "".
fn tag_value(ddtags: &str, key: &str) -> String {
    let prefix = format!("{key}:");
    ddtags.split(',').find_map(|tag| tag.strip_prefix(&prefix)).unwrap_or_default().to_string()
}

fn whole_number(text: &str) -> Option<i64> {
    text.parse().ok()
}

/// The first value of a batch that did not fit, as the note says it.
fn first_problem(bad: &Mismatches) -> Option<String> {
    bad.borrow().first().cloned()
}

/// The payload's root keys: metadata.NetworkDevicesMetadata.
static METADATA_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| {
    BTreeSet::from([
        "subnet",
        "namespace",
        "integration",
        "devices",
        "interfaces",
        "ip_addresses",
        "links",
        "vpn_tunnels",
        "netflow_exporters",
        "diagnoses",
        "device_oids",
        "scan_status",
        "collect_timestamp",
    ])
});

/// The wire sends 1 for reachable and 2 for unreachable. Anything else, an absent field included,
/// is "unknown" rather than a guess.
pub fn device_status(number: &str) -> DeviceStatus {
    match whole_number(number) {
        Some(1) => DeviceStatus::Reachable,
        Some(2) => DeviceStatus::Unreachable,
        _ => DeviceStatus::Unknown,
    }
}

#[derive(Debug, Default)]
pub struct MetadataRows {
    pub devices: Vec<NdmDevice>,
    pub interfaces: Vec<NdmInterface>,
    pub addresses: Vec<NdmIpAddress>,
    pub objects: Vec<NdmMetadataObject>,
}

/// The rows of one NetworkDevicesMetadata, or what did not fit.
pub fn metadata_rows(tenant: &str, payload: &Value) -> Result<MetadataRows, String> {
    let bad = Mismatches::default();
    let p = fields(&bad, "", payload);

    let space = p.string("namespace");
    let subnet = p.string("subnet");
    let integration = p.string("integration");
    // Whole seconds: the agent fills it from time.Unix().
    let collected = time::from_unix_seconds(p.int64("collect_timestamp"));
    let extra = json::other_members(&METADATA_KEYS, json::compact, Some(payload));

    let device_row = |d: Fields| NdmDevice {
        tenant_id: tenant.into(),
        namespace: space.clone(),
        subnet: subnet.clone(),
        integration: integration.clone(),
        collect_timestamp: collected,
        extra: extra.clone(),
        device_id: d.string("id"),
        id_tags: tags::to_multi_map(d.strings("id_tags")),
        tags: tags::to_multi_map(d.strings("tags")),
        ip_address: d.string("ip_address"),
        status: device_status(&d.number("status")),
        ping_status: device_status(&d.number("ping_status")),
        name: d.string("name"),
        description: d.string("description"),
        sys_object_id: d.string("sys_object_id"),
        location: d.string("location"),
        profile: d.string("profile"),
        profile_version: d.uint64("profile_version"),
        vendor: d.string("vendor"),
        device_subnet: d.string("subnet"),
        serial_number: d.string("serial_number"),
        version: d.string("version"),
        product_name: d.string("product_name"),
        model: d.string("model"),
        os_name: d.string("os_name"),
        os_version: d.string("os_version"),
        os_hostname: d.string("os_hostname"),
        device_integration: d.string("integration"),
        device_type: d.string("device_type"),
    };

    // The interface's status numbers are stored as sent; one that is not a whole number is 0.
    let status_number = |number: String| whole_number(&number).map_or(0, |n| n as u8);

    let interface_row = |i: Fields| NdmInterface {
        tenant_id: tenant.into(),
        namespace: space.clone(),
        subnet: subnet.clone(),
        integration: integration.clone(),
        collect_timestamp: collected,
        extra: extra.clone(),
        device_id: i.string("device_id"),
        id_tags: tags::to_multi_map(i.strings("id_tags")),
        if_index: i.int32("index"),
        raw_id: i.string("raw_id"),
        raw_id_type: i.string("raw_id_type"),
        name: i.string("name"),
        alias: i.string("alias"),
        description: i.string("description"),
        mac_address: i.string("mac_address"),
        admin_status: status_number(i.number("admin_status")),
        oper_status: status_number(i.number("oper_status")),
        kind: i.int32("type") as u32,
        is_physical: i.optional_bool("is_physical").map(text::flag),
        meraki_enabled: i.optional_bool("meraki_enabled").map(text::flag),
        meraki_status: i.string("meraki_status"),
    };

    let address_row = |a: Fields| NdmIpAddress {
        tenant_id: tenant.into(),
        namespace: space.clone(),
        subnet: subnet.clone(),
        integration: integration.clone(),
        collect_timestamp: collected,
        extra: extra.clone(),
        interface_id: a.string("interface_id"),
        ip_address: a.string("ip_address"),
        prefixlen: a.int32("prefixlen"),
    };

    // The other lists are kept whole. Only some of their shapes carry a device or an interface
    // id; the ones that do not leave it empty, and a malformed element still keeps its text.
    let object_row = |kind: &str, item: &Value| {
        let unnoted = Mismatches::default();
        let ids = fields(&unnoted, "", item);
        NdmMetadataObject {
            tenant_id: tenant.into(),
            namespace: space.clone(),
            subnet: subnet.clone(),
            integration: integration.clone(),
            collect_timestamp: collected,
            extra: extra.clone(),
            kind: kind.into(),
            device_id: ids.string("device_id"),
            interface_id: ids.string("interface_id"),
            object: json::compact(item),
        }
    };
    let objects_of = |kind: &str, list: &str| -> Vec<NdmMetadataObject> {
        p.items(list).into_iter().map(|item| object_row(kind, item)).collect()
    };

    let objects: Vec<Vec<NdmMetadataObject>> = vec![
        objects_of("link", "links"),
        objects_of("vpn_tunnel", "vpn_tunnels"),
        objects_of("netflow_exporter", "netflow_exporters"),
        objects_of("diagnosis", "diagnoses"),
        objects_of("device_oid", "device_oids"),
        p.find("scan_status").map(|status| object_row("scan_status", status)).into_iter().collect(),
    ];
    let rows = MetadataRows {
        devices: p.objects("devices").into_iter().map(device_row).collect(),
        interfaces: p.objects("interfaces").into_iter().map(interface_row).collect(),
        addresses: p.objects("ip_addresses").into_iter().map(address_row).collect(),
        objects: objects.into_iter().flatten().collect(),
    };
    match first_problem(&bad) {
        None => Ok(rows),
        Some(problem) => Err(problem),
    }
}

pub async fn handle_metadata(ctx: Ctx, body: Bytes) -> Response {
    match json::try_parse_list(&body) {
        Err(e) => ctx.raw("ndm", "decode_error", &e, &body),
        Ok(batch) => {
            let mut all = MetadataRows::default();
            for (i, entry) in batch.iter().enumerate() {
                match metadata_rows(ctx.tenant(), entry) {
                    Err(e) => ctx.raw("ndm", "decode_error", &format!("entry {i}: {e}"), &json::raw_bytes(entry)),
                    Ok(rows) => {
                        all.devices.extend(rows.devices);
                        all.interfaces.extend(rows.interfaces);
                        all.addresses.extend(rows.addresses);
                        all.objects.extend(rows.objects);
                    }
                }
            }
            ctx.write(all.devices);
            ctx.write(all.interfaces);
            ctx.write(all.addresses);
            ctx.write(all.objects);
        }
    }
    accepted()
}

/// report.NCMPayload, plus agent_hostname and inventories: both are on the wire though the agent's
/// struct does not document them.
static CONFIG_KEYS: LazyLock<BTreeSet<&str>> =
    LazyLock::new(|| BTreeSet::from(["namespace", "configs", "collect_timestamp", "agent_hostname", "inventories"]));

/// The rows of one NCMPayload, or what did not fit.
pub fn config_rows(tenant: &str, payload: &Value) -> Result<Vec<NdmDeviceConfig>, String> {
    let bad = Mismatches::default();
    let p = fields(&bad, "", payload);

    let space = p.string("namespace");
    // Seconds, like a config's own timestamp below: both are time.Unix().
    let collected = time::from_unix_seconds(p.int64("collect_timestamp"));
    let agent_hostname = p.string("agent_hostname");
    let inventories = p.raw("inventories");
    let extra = json::other_members(&CONFIG_KEYS, json::compact, Some(payload));

    let rows = p
        .objects("configs")
        .into_iter()
        .map(|config| NdmDeviceConfig {
            tenant_id: tenant.into(),
            namespace: space.clone(),
            collect_timestamp: collected,
            agent_hostname: agent_hostname.clone(),
            inventories: inventories.clone(),
            extra: extra.clone(),
            device_id: config.string("device_id"),
            device_ip: config.string("device_ip"),
            config_type: config.string("config_type"),
            config_source: config.string("config_source"),
            timestamp: time::from_unix_seconds(config.int64("timestamp")),
            tags: tags::to_multi_map(config.strings("tags")),
            content: config.string("content"),
        })
        .collect();
    match first_problem(&bad) {
        None => Ok(rows),
        Some(problem) => Err(problem),
    }
}

pub async fn handle_config(ctx: Ctx, body: Bytes) -> Response {
    match json::try_parse_list(&body) {
        Err(e) => ctx.raw("ndmconfig", "decode_error", &e, &body),
        Ok(batch) => {
            let mut rows = Vec::new();
            for (i, entry) in batch.iter().enumerate() {
                match config_rows(ctx.tenant(), entry) {
                    Err(e) => ctx.raw("ndmconfig", "decode_error", &format!("entry {i}: {e}"), &json::raw_bytes(entry)),
                    Ok(configs) => rows.extend(configs),
                }
            }
            ctx.write(rows);
        }
    }
    accepted()
}

/// What the formatter writes under "trap" for every trap. Any other key is an enrichment: a
/// variable the MIB resolved to a name.
static TRAP_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| {
    BTreeSet::from([
        "ddsource",
        "ddtags",
        "timestamp",
        "uptime",
        "snmpTrapOID",
        "snmpTrapName",
        "snmpTrapMIB",
        "enterpriseOID",
        "genericTrap",
        "specificTrap",
        "variables",
    ])
});

/// The row of one trap (the object under "trap"), or what did not fit.
pub fn trap_row(tenant: &str, trap: &Value) -> Result<SnmpTrap, String> {
    let bad = Mismatches::default();
    let t = fields(&bad, "", trap);
    let ddtags = t.string("ddtags");
    let variables = t.objects("variables");

    let row = SnmpTrap {
        tenant_id: tenant.into(),
        // Milliseconds, unlike the collect timestamps above: the listener stamps a trap with
        // UnixMilli().
        timestamp: time::from_unix_millis(t.int64("timestamp")),
        ddsource: t.string("ddsource"),
        ddtags: tags::to_multi_map(tags::split_ddtags(&ddtags)),
        device: tag_value(&ddtags, "snmp_device"),
        uptime: whole_number(&t.number("uptime")).map_or(0, |n| n as u32),
        snmp_trap_oid: t.string("snmpTrapOID"),
        snmp_trap_name: t.string("snmpTrapName"),
        snmp_trap_mib: t.string("snmpTrapMIB"),
        enterprise_oid: t.string("enterpriseOID"),
        generic_trap: t.optional_int32("genericTrap"),
        specific_trap: t.optional_int32("specificTrap"),
        var_oids: variables.iter().map(|v| v.string("oid")).collect(),
        var_types: variables.iter().map(|v| v.string("type")).collect(),
        // A variable's value has the type of its OID, so it stays JSON.
        var_values: variables.iter().map(|v| v.raw("value")).collect(),
        enriched: json::other_members(&TRAP_KEYS, json::compact, Some(trap)),
        raw: json::compact(trap),
    };
    match first_problem(&bad) {
        None => Ok(row),
        Some(problem) => Err(problem),
    }
}

pub async fn handle_traps(ctx: Ctx, body: Bytes) -> Response {
    match json::try_parse_list(&body) {
        Err(e) => ctx.raw("ndmtraps", "decode_error", &e, &body),
        Ok(batch) => {
            let mut rows = Vec::new();
            for (i, entry) in batch.iter().enumerate() {
                let keep = |reason: &str, note: String| ctx.raw("ndmtraps", reason, &note, &json::raw_bytes(entry));
                if !entry.is_object() && !entry.is_null() {
                    keep("decode_error", format!("entry {i}: expected an object, got {}", json::kind(entry)));
                } else {
                    match entry.get("trap") {
                        None => keep("unexpected_shape", format!("entry {i}: no trap object")),
                        Some(trap) => match trap_row(ctx.tenant(), trap) {
                            Err(e) => keep("decode_error", format!("entry {i}: trap: {e}")),
                            Ok(row) => rows.push(row),
                        },
                    }
                }
            }
            ctx.write(rows);
        }
    }
    accepted()
}

/// The root keys the agent's FlowPayload writes for its own fields. Every other root key is one
/// of the flow's additional fields.
pub static FLOW_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| {
    BTreeSet::from([
        "flush_timestamp",
        "type",
        "sampling_rate",
        "direction",
        "start",
        "end",
        "bytes",
        "packets",
        "ether_type",
        "ip_protocol",
        "tos",
        "dscp",
        "dscp_name",
        "device",
        "exporter",
        "source",
        "destination",
        "ingress",
        "egress",
        "host",
        "tcp_flags",
        "next_hop",
    ])
});

/// The agent never writes an "additional_fields" key: its marshaller spreads the configured extra
/// fields over the root object, next to "bytes" and "source". They are collected back here, each
/// value as JSON text. A hand-made request may spell the map out.
fn additional_fields(flow: &Value) -> BTreeMap<String, String> {
    let mut found = BTreeMap::new();
    if let Value::Object(object) = flow {
        for (name, value) in object {
            if name == "additional_fields" {
                found.extend(json::members(value));
            } else if !FLOW_KEYS.contains(name.as_str()) {
                found.insert(name.clone(), value.clone());
            }
        }
    }
    json::member_texts(json::compact_sorted, &found)
}

fn flow_row(bad: &Mismatches, tenant: &str, index: usize, flow: &Value) -> NetflowFlow {
    let f = fields(bad, &index.to_string(), flow);
    let source = f.object("source");
    let destination = f.object("destination");
    // The struct's own map: nothing is read from it here, but a value that is not an object is
    // refused all the same.
    f.object("additional_fields");

    NetflowFlow {
        tenant_id: tenant.into(),
        flush_timestamp: time::from_unix_millis(f.int64("flush_timestamp")),
        flow_type: f.string("type"),
        sampling_rate: f.uint64("sampling_rate"),
        direction: f.string("direction"),
        flow_start: time::from_unix_seconds(f.uint64("start") as i64),
        flow_end: time::from_unix_seconds(f.uint64("end") as i64),
        bytes: f.uint64("bytes"),
        packets: f.uint64("packets"),
        ether_type: f.string("ether_type"),
        ip_protocol: f.string("ip_protocol"),
        tos: f.uint32("tos"),
        dscp: f.uint32("dscp"),
        dscp_name: f.string("dscp_name"),
        device_namespace: f.object("device").string("namespace"),
        exporter_ip: f.object("exporter").string("ip"),
        source_ip: source.string("ip"),
        // A port is text: a number, or `*` for an ephemeral one.
        source_port: source.string("port"),
        source_mac: source.string("mac"),
        source_mask: source.string("mask"),
        source_reverse_dns_hostname: source.string("reverse_dns_hostname"),
        destination_ip: destination.string("ip"),
        destination_port: destination.string("port"),
        destination_mac: destination.string("mac"),
        destination_mask: destination.string("mask"),
        destination_reverse_dns_hostname: destination.string("reverse_dns_hostname"),
        ingress_interface_index: f.object("ingress").object("interface").uint32("index"),
        egress_interface_index: f.object("egress").object("interface").uint32("index"),
        host: f.string("host"),
        tcp_flags: f.strings("tcp_flags"),
        next_hop_ip: f.object("next_hop").string("ip"),
        additional_fields: additional_fields(flow),
    }
}

/// The rows of a batch of flows. One value that does not fit refuses the whole batch.
pub fn decode_flows(tenant: &str, body: &[u8]) -> Result<Vec<NetflowFlow>, String> {
    let flows = json::try_parse_list(body)?;
    let bad = Mismatches::default();
    let rows = flows.iter().enumerate().map(|(i, flow)| flow_row(&bad, tenant, i, flow)).collect();
    match first_problem(&bad) {
        None => Ok(rows),
        Some(problem) => Err(problem),
    }
}

pub async fn handle_flow(ctx: Ctx, body: Bytes) -> Response {
    match decode_flows(ctx.tenant(), &body) {
        Err(e) => ctx.raw("ndmflow", "decode_error", &e, &body),
        Ok(rows) => ctx.write(rows),
    }
    accepted()
}

/// An address in its canonical text; None when it is not one. "" is no address (a hop that did
/// not answer) and stays "". Stricter than a plain parse: four plain decimal parts for IPv4, no
/// zone for IPv6.
pub fn ip_text(text: &str) -> Option<String> {
    static IPV4: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"\A(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})\z").unwrap()
    });
    static IPV6: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[0-9a-fA-F:.]+\z").unwrap());
    if text.is_empty() {
        Some(String::new())
    } else if IPV4.is_match(text) {
        text.parse::<IpAddr>().ok().map(|address| address.to_string())
    } else if text.contains(':') && IPV6.is_match(text) {
        let address: Ipv6Addr = text.parse().ok()?;
        match address.to_ipv4_mapped() {
            Some(v4) => Some(v4.to_string()),
            None => Some(ipv6_text(address)),
        }
    } else {
        None
    }
}

/// An IPv6 address as the F# server printed it: RFC 5952, except that an address whose last 32
/// bits read as IPv4 (an IPv4-compatible `::a.b.c.d`, SIIT's `::ffff:0:a.b.c.d`, ISATAP's
/// `…:0:5efe:a.b.c.d`) ends in dotted decimal. The standard library prints those in hex.
fn ipv6_text(address: Ipv6Addr) -> String {
    let words = address.segments();
    let compatible = words[..4] == [0, 0, 0, 0]
        && words[6] != 0
        && ((words[4] == 0 && (words[5] == 0 || words[5] == 0xffff)) || (words[4] == 0xffff && words[5] == 0));
    let isatap = words[4] == 0 && words[5] == 0x5efe;
    if !compatible && !isatap {
        return address.to_string();
    }
    let [a, b, c, d] = address.octets()[12..] else { unreachable!() };
    // The six leading words, with their longest run of two or more zeros (the first of equals)
    // written as "::".
    let head = &words[..6];
    let mut longest = (0, 0);
    let mut i = 0;
    while i < head.len() {
        let start = i;
        while i < head.len() && head[i] == 0 {
            i += 1;
        }
        if i - start > longest.1 - longest.0 {
            longest = (start, i);
        }
        i += 1;
    }
    let hex = |part: &[u16]| part.iter().map(|w| format!("{w:x}")).collect::<Vec<_>>().join(":");
    let head = if longest.1 - longest.0 >= 2 {
        format!("{}::{}", hex(&head[..longest.0]), hex(&head[longest.1..]))
    } else {
        format!("{}:", hex(head))
    };
    let separator = if head.ends_with(':') { "" } else { ":" };
    format!("{head}{separator}{a}.{b}.{c}.{d}")
}

fn ip(fields: &Fields, name: &str) -> String {
    fields.read(name, "an IP address", String::new(), |value| json::string_of(value).and_then(|text| ip_text(&text)))
}

/// One traceroute run, with the parts of it that are read more than once.
struct Run<'b, 'a> {
    run: Fields<'b, 'a>,
    from: Fields<'b, 'a>,
    to: Fields<'b, 'a>,
    hops: Vec<Fields<'b, 'a>>,
}

/// What belongs to a hop: an array per run.
fn per_hop<T>(runs: &[Run], read: impl Fn(&Fields) -> T) -> Vec<Vec<T>> {
    runs.iter().map(|run| run.hops.iter().map(&read).collect()).collect()
}

fn path_row(bad: &Mismatches, tenant: &str, index: usize, path: &Value) -> NetworkPath {
    let p = fields(bad, &index.to_string(), path);
    let source = p.object("source");
    let via = source.optional_object("via");
    let destination = p.object("destination");
    let traceroute = p.object("traceroute");
    let hop_count = traceroute.object("hop_count");
    let probe = p.object("e2e_probe");
    let rtt = probe.object("rtt");

    let runs: Vec<Run> = traceroute
        .objects("runs")
        .into_iter()
        .map(|run| Run { from: run.object("source"), to: run.object("destination"), hops: run.objects("hops"), run })
        .collect();

    NetworkPath {
        tenant_id: tenant.into(),
        timestamp: time::from_unix_millis(p.int64("timestamp")),
        agent_version: p.string("agent_version"),
        namespace: p.string("namespace"),
        test_config_id: p.string("test_config_id"),
        test_config_name: p.string("test_config_name"),
        test_result_id: p.string("test_result_id"),
        test_run_id: p.string("test_run_id"),
        origin: p.string("origin"),
        test_run_type: p.string("test_run_type"),
        test_config_source: p.string("test_config_source"),
        source_product: p.string("source_product"),
        collector_type: p.string("collector_type"),
        protocol: p.string("protocol"),
        source_name: source.string("name"),
        source_display_name: source.string("display_name"),
        source_hostname: source.string("hostname"),
        // With a `via`, an unresolved alias is "", not NULL.
        source_via_subnet_alias: via.as_ref().map(|v| v.object("subnet").string("alias")),
        source_via_interface_hardware_addr: via.as_ref().map(|v| v.object("interface").string("hardware_addr")),
        source_network_id: source.string("network_id"),
        source_service: source.string("service"),
        source_container_id: source.string("container_id"),
        source_public_ip: source.string("public_ip"),
        destination_hostname: destination.string("hostname"),
        destination_port: destination.uint16("port"),
        destination_service: destination.string("service"),
        hop_count_avg: hop_count.float("avg"),
        hop_count_min: hop_count.int64("min") as i32,
        hop_count_max: hop_count.int64("max") as i32,
        run_ids: runs.iter().map(|run| run.run.string("run_id")).collect(),
        run_source_ips: runs.iter().map(|run| ip(&run.from, "ip_address")).collect(),
        run_source_ports: runs.iter().map(|run| run.from.uint16("port")).collect(),
        run_destination_ips: runs.iter().map(|run| ip(&run.to, "ip_address")).collect(),
        run_destination_ports: runs.iter().map(|run| run.to.uint16("port")).collect(),
        run_destination_reverse_dns: runs.iter().map(|run| run.to.strings("reverse_dns")).collect(),
        hop_ttls: per_hop(&runs, |hop| hop.int64("ttl") as i32),
        hop_ips: per_hop(&runs, |hop| ip(hop, "ip_address")),
        hop_reverse_dns: per_hop(&runs, |hop| hop.strings("reverse_dns")),
        hop_rtts: per_hop(&runs, |hop| hop.float("rtt")),
        hop_reachable: per_hop(&runs, |hop| text::flag(hop.bool("reachable"))),
        e2e_rtts: probe.floats("rtts"),
        e2e_packets_sent: probe.int64("packets_sent") as i32,
        e2e_packets_received: probe.int64("packets_received") as i32,
        e2e_packet_loss_percentage: probe.float32("packet_loss_percentage"),
        e2e_jitter: probe.float("jitter"),
        e2e_rtt_avg: rtt.float("avg"),
        e2e_rtt_min: rtt.float("min"),
        e2e_rtt_max: rtt.float("max"),
        tags: tags::to_multi_map(p.strings("tags")),
    }
}

/// The rows of a batch of network paths. One value that does not fit refuses the whole batch.
pub fn decode_paths(tenant: &str, body: &[u8]) -> Result<Vec<NetworkPath>, String> {
    let paths = json::try_parse_list(body)?;
    let bad = Mismatches::default();
    let rows = paths.iter().enumerate().map(|(i, path)| path_row(&bad, tenant, i, path)).collect();
    match first_problem(&bad) {
        None => Ok(rows),
        Some(problem) => Err(problem),
    }
}

pub async fn handle_netpath(ctx: Ctx, body: Bytes) -> Response {
    match decode_paths(ctx.tenant(), &body) {
        Err(e) => ctx.raw("netpath", "decode_error", &e, &body),
        Ok(rows) => ctx.write(rows),
    }
    accepted()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Value {
        json::try_parse(text.as_bytes()).unwrap()
    }

    fn flows(body: &str) -> Vec<NetflowFlow> {
        decode_flows("test", body.as_bytes()).expect("flows")
    }

    fn path(body: &str) -> NetworkPath {
        let mut rows = decode_paths("test", body.as_bytes()).expect("paths");
        assert_eq!(rows.len(), 1);
        rows.remove(0)
    }

    fn texts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn additional_fields_spread_over_the_root_come_back_64_bit_digits_intact() {
        // What the agent's FlowPayload.MarshalJSON writes: no "additional_fields" key, the extra
        // fields next to the struct's own.
        let rows = flows(
            r#"[{"bgp_next_hop":"10.0.0.254","bytes":9223372036854775808,"custom_obj":{"a":"b"},
                 "destination":{"ip":"","port":"","mac":"","mask":""},"device":{"namespace":"default"},
                 "direction":"","dscp":0,"dscp_name":"","egress":{"interface":{"index":0}},"end":0,
                 "ether_type":"IPv4","exporter":{"ip":"10.0.0.1"},"flush_timestamp":1758326400123,"host":"",
                 "ingress":{"interface":{"index":0}},"ip_protocol":"TCP","next_hop":{"ip":""},
                 "packets":9007199254740993,"sampling_rate":0,"source":{"ip":"","port":"","mac":"","mask":""},
                 "start":0,"tcp_flags":["SYN","ACK"],"tos":0,"type":"netflow9","vlan_id":9007199254740993},
                {"type":"sflow5","ip_protocol":"UDP"}]"#,
        );
        assert_eq!(rows.len(), 2);
        let f = &rows[0];
        // Beyond what a float holds exactly.
        assert_eq!(f.bytes, 9223372036854775808);
        assert_eq!(f.packets, 9007199254740993);
        assert_eq!(f.flush_timestamp, time::from_unix_millis(1758326400123));
        assert_eq!(f.tcp_flags, ["SYN", "ACK"]);
        assert_eq!(
            f.additional_fields,
            texts(&[
                ("bgp_next_hop", "\"10.0.0.254\""),
                ("custom_obj", r#"{"a":"b"}"#),
                ("vlan_id", "9007199254740993")
            ])
        );
        assert!(rows[1].additional_fields.is_empty());
    }

    #[test]
    fn the_structs_keys_are_known_additional_fields_is_not_one_of_them() {
        for key in ["flush_timestamp", "type", "bytes", "source", "next_hop", "tcp_flags", "device", "exporter"] {
            assert!(FLOW_KEYS.contains(key));
        }
        assert!(!FLOW_KEYS.contains("additional_fields"));
    }

    #[test]
    fn a_spelled_out_additional_fields_object_is_taken_too_next_to_root_extras() {
        let rows = flows(r#"{"type":"netflow9","additional_fields":{"n":9007199254740993},"extra":true}"#);
        assert_eq!(rows[0].additional_fields, texts(&[("extra", "true"), ("n", "9007199254740993")]));
    }

    #[test]
    fn an_additional_fields_value_is_json_text_a_string_keeps_its_quotes_an_object_is_compact_with_sorted_keys() {
        // serde_json keeps an integer's digits; a float is written back as the float it read.
        let rows = flows(r#"[{"note":"hello","nested":{ "b" : 1.50, "a" : [1000, null] }}]"#);
        assert_eq!(
            rows[0].additional_fields,
            texts(&[("nested", r#"{"a":[1000,null],"b":1.5}"#), ("note", "\"hello\"")])
        );
    }

    #[test]
    fn a_key_in_another_case_is_an_additional_field_and_fills_nothing() {
        let rows = flows(r#"[{"TYPE":"upper","Source":{"IP":"1.1.1.1"}}]"#);
        assert_eq!(rows[0].flow_type, "");
        assert_eq!(rows[0].source_ip, "");
        assert_eq!(rows[0].additional_fields, texts(&[("Source", r#"{"IP":"1.1.1.1"}"#), ("TYPE", "\"upper\"")]));
    }

    #[test]
    fn half_a_surrogate_pair_in_a_flow_reads_as_the_replacement_character() {
        let rows = flows(r#"[{"x":"\ud800","type":"t\ud800","nested":{"k":"\udc00v"}}]"#);
        assert_eq!(rows[0].flow_type, "t\u{fffd}");
        assert_eq!(rows[0].additional_fields, texts(&[("nested", "{\"k\":\"\u{fffd}v\"}"), ("x", "\"\u{fffd}\"")]));
    }

    #[test]
    fn a_null_flow_is_a_flow_of_zero_values() {
        let rows = flows("[null]");
        assert_eq!(rows[0].flow_type, "");
        assert_eq!(rows[0].bytes, 0);
        assert_eq!(rows[0].flow_start, time::from_unix_seconds(0));
    }

    #[test]
    fn one_value_of_the_wrong_type_refuses_the_whole_batch_of_flows() {
        let body = r#"[{"type":"netflow9","bytes":1},{"type":"netflow9","bytes":"many"}]"#;
        assert_eq!(
            decode_flows("test", body.as_bytes()).unwrap_err(),
            "1.bytes: expected an unsigned integer, got string"
        );
    }

    #[test]
    fn a_device_status_is_reachable_unreachable_or_unknown_rather_than_a_guess() {
        for (number, name) in
            [("1", "reachable"), ("2", "unreachable"), ("", "unknown"), ("99", "unknown"), ("not-a-number", "unknown")]
        {
            assert_eq!(device_status(number).name(), name);
        }
    }

    #[test]
    fn an_interface_flag_is_true_false_or_not_reported() {
        let rows = metadata_rows(
            "test",
            &value(r#"{"interfaces":[{"is_physical":true,"meraki_enabled":false},{"device_id":"d"}]}"#),
        )
        .unwrap();
        assert_eq!(rows.interfaces[0].is_physical, Some(1));
        assert_eq!(rows.interfaces[0].meraki_enabled, Some(0));
        assert_eq!(rows.interfaces[1].is_physical, None);
        assert_eq!(rows.interfaces[1].meraki_enabled, None);
    }

    #[test]
    fn the_other_lists_are_kept_whole_with_the_ids_the_element_carries() {
        let rows = metadata_rows(
            "test",
            &value(
                r#"{"namespace":"default","collect_timestamp":1700000000,"x":1,
                    "links":[{"id":"link1","source_type":"lldp"}],
                    "vpn_tunnels":[{"device_id":"dev1","interface_id":"dev1:1","status":"up"}]}"#,
            ),
        )
        .unwrap();
        // No scan_status was sent, so there is no row for one.
        let kinds: Vec<&str> = rows.objects.iter().map(|o| o.kind.as_str()).collect();
        assert_eq!(kinds, ["link", "vpn_tunnel"]);
        assert_eq!(rows.objects[0].device_id, "");
        assert_eq!(rows.objects[0].object, r#"{"id":"link1","source_type":"lldp"}"#);
        let tunnel = &rows.objects[1];
        assert_eq!((tunnel.device_id.as_str(), tunnel.interface_id.as_str()), ("dev1", "dev1:1"));
        assert_eq!(tunnel.namespace, "default");
        assert_eq!(tunnel.collect_timestamp, time::from_unix_seconds(1700000000));
        assert_eq!(tunnel.extra, texts(&[("x", "1")]));
    }

    #[test]
    fn scan_status_is_one_object_and_one_row_a_malformed_list_element_keeps_its_text() {
        let rows = metadata_rows(
            "test",
            &value(r#"{"scan_status":{"device_id":"dev1","scan_status":"completed"},"diagnoses":["not-an-object"]}"#),
        )
        .unwrap();
        let kinds: Vec<&str> = rows.objects.iter().map(|o| o.kind.as_str()).collect();
        assert_eq!(kinds, ["diagnosis", "scan_status"]);
        assert_eq!(rows.objects[0].object, "\"not-an-object\"");
        assert_eq!(rows.objects[0].device_id, "");
        assert_eq!(rows.objects[1].device_id, "dev1");
    }

    #[test]
    fn a_config_payload_repeats_its_own_fields_on_every_config() {
        let rows = config_rows(
            "test",
            &value(
                r#"{"namespace":"default","collect_timestamp":1700000000,"agent_hostname":"agent01",
                    "configs":[{"device_id":"a","timestamp":1700000100},{"device_id":"b","tags":["env:prod"]}]}"#,
            ),
        )
        .unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.device_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(rows.iter().all(|r| r.agent_hostname == "agent01"));
        assert_eq!(rows[0].inventories, "");
        assert_eq!(rows[0].timestamp, time::from_unix_seconds(1700000100));
        assert_eq!(rows[1].timestamp, time::from_unix_seconds(0));
    }

    #[test]
    fn a_v1_trap_carries_its_enterprise_oid_and_both_trap_numbers_zero_included() {
        let row = trap_row(
            "test",
            &value(
                r#"{"ddsource":"snmp-traps","ddtags":"namespace:default,snmp_device:10.0.0.5",
                    "timestamp":1700000200,"uptime":123456,"snmpTrapOID":"1.3.6.1.6.3.1.1.5.3",
                    "enterpriseOID":"1.3.6.1.4.1.9","genericTrap":2,"specificTrap":0,
                    "variables":[{"oid":"1.3.6.1.2.1.2.2.1.1","type":"Integer","value":1}],
                    "customEnrichedKey":"resolved-value"}"#,
            ),
        )
        .unwrap();
        assert_eq!(row.device, "10.0.0.5");
        assert_eq!(row.enterprise_oid, "1.3.6.1.4.1.9");
        assert_eq!((row.generic_trap, row.specific_trap), (Some(2), Some(0)));
        assert_eq!(row.uptime, 123456);
        assert_eq!(row.var_oids, ["1.3.6.1.2.1.2.2.1.1"]);
        assert_eq!(row.var_types, ["Integer"]);
        assert_eq!(row.var_values, ["1"]);
        assert_eq!(row.enriched, texts(&[("customEnrichedKey", "\"resolved-value\"")]));
    }

    #[test]
    fn a_traps_timestamp_is_milliseconds_and_a_v2_trap_has_no_v1_fields() {
        let row = trap_row(
            "test",
            &value(r#"{"ddsource":"snmp-traps","ddtags":"snmp_device:10.0.0.6","timestamp":1758326400123,"snmpTrapOID":"1.3"}"#),
        )
        .unwrap();
        assert_eq!(row.timestamp, time::from_unix_millis(1758326400123));
        assert_eq!((row.generic_trap, row.specific_trap), (None, None));
        assert_eq!(row.enterprise_oid, "");
        assert!(row.ddtags.contains_key("snmp_device"));
    }

    #[test]
    fn an_address_is_stored_in_its_canonical_text() {
        for (sent, stored) in [
            ("", ""),
            ("10.0.0.1", "10.0.0.1"),
            ("::ffff:1.2.3.4", "1.2.3.4"),
            ("2001:DB8:0:0:0:0:0:1", "2001:db8::1"),
            // An IPv4-compatible address keeps its dotted tail, as the F# server printed it.
            ("::1.2.3.4", "::1.2.3.4"),
            ("1:0:0:2:0:0:0:3", "1:0:0:2::3"),
            ("1:0:0:2:3:0:0:4", "1::2:3:0:0:4"),
            ("0:0:0:0:0:0:0:0", "::"),
            ("1:2:3:4:5:6:7:0", "1:2:3:4:5:6:7:0"),
            ("1:2:3:0:5:6:7:8", "1:2:3:0:5:6:7:8"),
            ("::1", "::1"),
            ("fe80::5efe:102:304", "fe80::5efe:1.2.3.4"),
        ] {
            assert_eq!(ip_text(sent).as_deref(), Some(stored), "{sent}");
        }
    }

    #[test]
    fn what_is_not_an_address_is_refused() {
        for sent in ["bad", "1.2.3.04", " 1.2.3.4", "1.2.3", "1.2.3.256", "fe80::1%eth0", "[::1]"] {
            assert_eq!(ip_text(sent), None, "{sent}");
        }
    }

    #[test]
    fn no_via_gives_null_a_via_that_resolved_to_nothing_gives_empty_strings() {
        let row = path(r#"[{"timestamp":1758326400123,"source":{"hostname":"h1"}}]"#);
        assert_eq!((row.source_via_subnet_alias, row.source_via_interface_hardware_addr), (None, None));
        assert_eq!(row.timestamp, time::from_unix_millis(1758326400123));

        let row = path(r#"[{"source":{"hostname":"h1","via":{}}}]"#);
        assert_eq!(row.source_via_subnet_alias.as_deref(), Some(""));
        assert_eq!(row.source_via_interface_hardware_addr.as_deref(), Some(""));

        let row = path(r#"[{"source":{"via":{"subnet":{"alias":"subnet-1"},"interface":{"hardware_addr":"aa:bb"}}}}]"#);
        assert_eq!(row.source_via_subnet_alias.as_deref(), Some("subnet-1"));
        assert_eq!(row.source_via_interface_hardware_addr.as_deref(), Some("aa:bb"));
    }

    #[test]
    fn runs_are_parallel_arrays_and_what_belongs_to_a_hop_is_an_array_per_run() {
        let row = path(
            r#"[{"traceroute":{"runs":[
                  {"run_id":"run1",
                   "source":{"ip_address":"10.0.0.1","port":1234},
                   "destination":{"ip_address":"10.0.0.2","port":443,"reverse_dns":["dest.example.com"]},
                   "hops":[{"ttl":1,"ip_address":"10.0.0.254","rtt":1.5,"reachable":true},
                           {"ttl":2,"ip_address":"","reachable":false}]},
                  {"run_id":"run2",
                   "source":{"ip_address":"","port":0},
                   "destination":{"ip_address":"","port":0},
                   "hops":[{"ttl":1,"ip_address":"10.0.0.253","reverse_dns":["a.example.com","b.example.com"],"reachable":true}]}]}}]"#,
        );
        assert_eq!(row.run_ids, ["run1", "run2"]);
        assert_eq!(row.run_source_ips, ["10.0.0.1", ""]);
        assert_eq!(row.run_source_ports, [1234, 0]);
        assert_eq!(row.run_destination_ips, ["10.0.0.2", ""]);
        assert_eq!(row.run_destination_ports, [443, 0]);
        assert_eq!(row.run_destination_reverse_dns, vec![vec!["dest.example.com".to_string()], vec![]]);
        assert_eq!(row.hop_ttls, vec![vec![1, 2], vec![1]]);
        // A hop that did not answer has no address and no time.
        assert_eq!(row.hop_ips, vec![vec!["10.0.0.254", ""], vec!["10.0.0.253"]]);
        assert_eq!(row.hop_rtts, vec![vec![1.5, 0.0], vec![0.0]]);
        assert_eq!(row.hop_reachable, vec![vec![1, 0], vec![1]]);
        assert_eq!(row.hop_reverse_dns, vec![vec![vec![], vec![]], vec![vec!["a.example.com", "b.example.com"]]]);
    }

    #[test]
    fn an_address_that_is_not_one_refuses_the_batch_of_paths() {
        let body = r#"[{"traceroute":{"runs":[{"hops":[{"ip_address":"nowhere"}]}]}}]"#;
        assert!(decode_paths("test", body.as_bytes()).is_err());
    }

    #[test]
    fn a_bare_object_is_a_batch_of_one() {
        assert_eq!(flows(r#" {"type":"netflow5"} "#)[0].flow_type, "netflow5");
        assert_eq!(path(r#"{"namespace":"default"}"#).namespace, "default");
    }
}
