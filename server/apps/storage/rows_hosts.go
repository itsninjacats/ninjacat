package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// hosts — host metadata, the one "current state" table fed directly by a
// writer rather than by a materialized view.

type WriteHosts struct{ Hosts []HostRow }

func (m WriteHosts) rows() []Row { return toRows(m.Hosts) }

// HostRow is host metadata from /intake/, stored in ninjacat.hosts.
// The table is a ReplacingMergeTree keyed on (tenant_id, host), so repeated
// writes for the same host collapse to the newest one.
//
// The /intake/ host envelope is loose and shifts between agent versions, so
// the fields below are read by name rather than through a fixed struct, and
// the sections whose shape moves are kept as their JSON text. An EMPTY STRING
// in one of those means the agent did not send the section at all, which is
// not the same statement as "{}".
type HostRow struct {
	TenantID     string
	Host         string
	SeenAt       time.Time
	AgentVersion string
	OS           string
	Platform     map[string]string
	CPU          map[string]string
	Memory       map[string]string
	Tags         map[string][]string

	// Agent identity beyond its version: the installation's uuid, which
	// flavour of the agent this is (agent / iot / cluster-agent / heroku) and
	// which Python the checks run on.
	UUID          string
	AgentFlavor   string
	PythonVersion string

	// Sub-objects kept verbatim. Flattening them would need a column per key
	// of a structure the agent is free to change between releases.
	Meta       string
	Network    string
	Filesystem string
	Logs       string
	OTLP       string

	// Flat objects, where the keys ARE the question ("which install method?").
	SystemStats   map[string]string
	InstallMethod map[string]string
	ProxyInfo     map[string]string
	ContainerMeta map[string]string

	// Pointers because an older agent omits both keys, and "FIPS is off" is a
	// different statement from "this agent does not know about FIPS".
	FIPSMode         *uint8
	FIPSProxyEnabled *uint8

	// HostTags is every tag SOURCE, not only "system": the key is the source
	// (system, google_tags, a custom provider) and the value is that source's
	// whole tag list. Only "system" used to reach a row, so on a cloud host
	// the provider's tags were silently discarded.
	HostTags map[string][]string

	// Escape hatches: gohai sections beyond the five with columns, and
	// top-level envelope keys this decoder does not know. Values JSON-encoded.
	GohaiExtra  map[string]string
	IntakeExtra map[string]string

	// Resources is the legacy V5 process snapshot that rides along on the host
	// payload, verbatim. The process intake owns that format; parsing it twice
	// would mean two decoders to keep in step.
	Resources string
}

func (r HostRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Host, r.SeenAt, r.AgentVersion, r.OS,
		orEmpty(r.Platform), orEmpty(r.CPU), orEmpty(r.Memory), orEmpty(r.Tags),
		r.UUID, r.AgentFlavor, r.PythonVersion,
		r.Meta, orEmpty(r.SystemStats), r.Network, r.Filesystem, r.Logs,
		orEmpty(r.InstallMethod), orEmpty(r.ProxyInfo), r.OTLP, orEmpty(r.ContainerMeta),
		r.FIPSMode, r.FIPSProxyEnabled,
		orEmpty(r.HostTags), orEmpty(r.GohaiExtra), orEmpty(r.IntakeExtra), r.Resources)
}

func init() {
	registerWriter(HostsWriter, WriterConfig{
		Name: "hosts",
		Insert: `INSERT INTO hosts
			(tenant_id, host, seen_at, agent_version, os, platform, cpu, memory, tags,
			 uuid, agent_flavor, python_version, meta, system_stats, network, filesystem,
			 logs, install_method, proxy_info, otlp, container_meta,
			 fips_mode, fips_proxy_enabled, host_tags, gohai_extra, intake_extra, resources)`,
		// One row per host every ~20s. ReplacingMergeTree collapses duplicates.
		MaxRows: 100, FlushInterval: 10 * time.Second,
	}, HostRow{})
}
