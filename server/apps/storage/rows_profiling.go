package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// profiles, debugger_logs, debugger_diagnostics, symdb_uploads and
// symbol_uploads — the five tables fed by intake/router_profiling.go's three
// hosts (intake.profile, debugger-intake, sourcemap-intake). One file because
// they share a router and its conventions: multipart parsing via profParts,
// the event-JSON-plus-attachment(s) shape, and DECODED-ONLY upstream types
// with no published Go struct of their own (docs/tables/profiling.md).

const (
	ProfilesWriter            = gen.Atom("storage_profiles")
	DebuggerLogsWriter        = gen.Atom("storage_debugger_logs")
	DebuggerDiagnosticsWriter = gen.Atom("storage_debugger_diagnostics")
	SymdbUploadsWriter        = gen.Atom("storage_symdb_uploads")
	SymbolUploadsWriter       = gen.Atom("storage_symbol_uploads")
)

type WriteProfiles struct{ Profiles []ProfileRow }
type WriteDebuggerLogs struct{ Logs []DebuggerLogRow }
type WriteDebuggerDiagnostics struct{ Diagnostics []DebuggerDiagnosticRow }
type WriteSymdbUploads struct{ Uploads []SymdbUploadRow }
type WriteSymbolUploads struct{ Uploads []SymbolUploadRow }

func (m WriteProfiles) rows() []Row            { return toRows(m.Profiles) }
func (m WriteDebuggerLogs) rows() []Row        { return toRows(m.Logs) }
func (m WriteDebuggerDiagnostics) rows() []Row { return toRows(m.Diagnostics) }
func (m WriteSymdbUploads) rows() []Row        { return toRows(m.Uploads) }
func (m WriteSymbolUploads) rows() []Row       { return toRows(m.Uploads) }

// ProfileRow is one multipart submission to intake.profile.<site>'s
// /api/v2/profile or /v1/input, stored in ninjacat.profiles.
//
// Samples are NOT exploded into rows: a single CPU profile routinely carries
// tens of thousands of them, and pprof (github.com/google/pprof/profile) is
// a perfectly good reader for the bytes kept in AttachBytes — exploding would
// multiply write volume by the sample count to reproduce a shape an existing
// tool already parses for free. What IS kept per attachment is the pprof
// envelope (sample types/units, period, counts) as parallel arrays, so a
// query can filter profiles by shape without re-parsing every attachment.
type ProfileRow struct {
	TenantID   string
	ReceivedAt time.Time

	// Variant is the router's own label for which path produced the row:
	// "profile" for /api/v2/profile (the trace-agent's proxy of the tracer's
	// own multipart), "profile-v1" for /v1/input (dd-trace-go's internal
	// profiling). Left open-ended rather than an Enum8 so a future RUM
	// profiler reuse ("browser", see rum-spec.md) needs no migration.
	Variant string

	DDEvpOrigin        string
	DDEvpOriginVersion string

	// Event is the "event" part, byte for byte, before any decode — the
	// lossless copy. Every field below it is a convenience extract read from
	// the map[string]any decode (profDecodeEvent), never the source of truth.
	Event string

	StartRaw    string
	StartParsed *time.Time
	EndRaw      string
	EndParsed   *time.Time

	Family   string
	Version  string
	Runtime  string
	Language string

	// TagsProfiler is tags_profiler split and parsed as a multiset, same as
	// every other Datadog tag column — see docs/decisions/0001-tags-are-a-multiset.md.
	// The wire shape here is a THIRD tag convention (a single comma-joined
	// string, neither the flat array nor the map[string]string the rest of
	// the intake sees), so it goes through splitDDTags first.
	TagsProfiler map[string][]string

	// Attachments: parallel arrays, one entry per multipart part other than
	// "event", in the order profParts returned them (wire order).
	AttachName          []string
	AttachBytes         []string // raw part bytes exactly as received — gzip if the part was, since profile.ParseData undoes that internally and this column is the undecoded original.
	AttachSize          []uint64
	AttachParsed        []uint8 // 1 when profile.ParseData accepted the part as pprof, 0 otherwise — every other Attach* field for that index is zero-valued when this is 0.
	AttachSampleTypes   [][]string
	AttachSampleUnits   [][]string
	AttachSampleCount   []uint64
	AttachTimeNanos     []int64
	AttachDurationNanos []int64
	AttachPeriodType    []string
	AttachPeriod        []int64
	AttachMappingCount  []uint32
	AttachLocationCount []uint32
	AttachFunctionCount []uint32
}

func (r ProfileRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Variant, r.DDEvpOrigin, r.DDEvpOriginVersion,
		r.Event, r.StartRaw, r.StartParsed, r.EndRaw, r.EndParsed,
		r.Family, r.Version, r.Runtime, r.Language, orEmpty(r.TagsProfiler),
		orEmptySlice(r.AttachName), orEmptySlice(r.AttachBytes), orEmptySlice(r.AttachSize),
		orEmptySlice(r.AttachParsed), orEmptySlice(r.AttachSampleTypes), orEmptySlice(r.AttachSampleUnits),
		orEmptySlice(r.AttachSampleCount), orEmptySlice(r.AttachTimeNanos), orEmptySlice(r.AttachDurationNanos),
		orEmptySlice(r.AttachPeriodType), orEmptySlice(r.AttachPeriod), orEmptySlice(r.AttachMappingCount),
		orEmptySlice(r.AttachLocationCount), orEmptySlice(r.AttachFunctionCount))
}

// DebuggerLogRow is one entry from a Dynamic Instrumentation logs/snapshots
// batch — the "json" variant of /api/v2/debugger, JSON array or NDJSON — in
// ninjacat.debugger_logs.
type DebuggerLogRow struct {
	TenantID   string
	ReceivedAt time.Time
	Service    string
	DDSource   string

	// DDTags is the ?ddtags= query string, split and parsed as a multiset —
	// it travels alongside every entry in the batch, not per entry.
	DDTags map[string][]string

	// Entry is the array element (or NDJSON line), byte for byte.
	Entry string

	// ExtraKeys names Entry's top-level keys other than service/ddsource —
	// there is no fixed schema here to decode against (the upstream
	// dyninst logSender type is not published), so "extra" is everything.
	ExtraKeys []string
}

func (r DebuggerLogRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Service, r.DDSource,
		orEmpty(r.DDTags), r.Entry, orEmptySlice(r.ExtraKeys))
}

// DebuggerDiagnosticRow is one uploader.DiagnosticMessage (decoded
// generically — the upstream type is unexported) from the "diagnostics"
// variant of /api/v2/debugger, in ninjacat.debugger_diagnostics.
type DebuggerDiagnosticRow struct {
	TenantID   string
	ReceivedAt time.Time

	// Timestamp is the message's own claim, parsed best-effort; Nullable
	// because a missing or unparseable value must stay missing, never
	// collapse to ReceivedAt or the epoch — see profParseTimestamp.
	Timestamp *time.Time

	Service      string
	DDSource     string
	RuntimeID    string
	ProbeID      string
	Status       string
	ProbeVersion string

	// Exception is optional on the wire (debugger.diagnostics.exception);
	// both fields are Nullable together so "no exception" and "an exception
	// with an empty message" stay distinct.
	ExceptionType    *string
	ExceptionMessage *string

	// Message is the array element, byte for byte.
	Message string
}

func (r DebuggerDiagnosticRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Timestamp, r.Service, r.DDSource,
		r.RuntimeID, r.ProbeID, r.Status, r.ProbeVersion,
		r.ExceptionType, r.ExceptionMessage, r.Message)
}

// SymdbUploadRow is one symbol-database upload — the "symdb" variant of
// /api/v2/debugger (multipart with both file and event parts) — in
// ninjacat.symdb_uploads.
//
// The event part and the envelope inside the gzip'd file describe the same
// upload but are two different producers/serializers: the event spells its
// keys camelCase (UploadID from "uploadId"), the file's envelope spells them
// snake_case ("upload_id"). Both are kept, under Env-prefixed names for the
// file's spelling, rather than merged into one guess — the drift itself is
// worth being able to see.
type SymdbUploadRow struct {
	TenantID   string
	ReceivedAt time.Time

	Service   string
	Version   string
	Language  string
	RuntimeID string

	// UploadID/BatchNum are kept as the wire's literal text (not parsed to an
	// integer): they are identifiers, not quantities, and nothing here
	// guarantees they stay numeric across producers.
	UploadID string
	BatchNum string

	// Final/AttachmentSize are Nullable: the event may not carry them at all,
	// and that is a different state from "false" / "0 bytes".
	Final          *uint8
	AttachmentSize *uint64

	// File is the gzip'd file part exactly as received — the lossless copy;
	// InflatedSize/ScopeCount/ScopesOK and the Env* columns below are
	// convenience extracts of what is already fully present here.
	File         string
	InflatedSize uint64
	ScopeCount   uint32 // top-level scopes only; nested child scopes are not counted here — see docs/tables/profiling.md
	ScopesOK     uint8  // 1 when the envelope's "scopes" key was present and decoded; 0 when it was missing or malformed (the envelope itself may still have decoded fine — see dbgDecodeSymdbFile)

	EnvService  string
	EnvVersion  string
	EnvLanguage string
	EnvUploadID string
	EnvBatchNum string
	EnvFinal    string
}

func (r SymdbUploadRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Service, r.Version, r.Language,
		r.RuntimeID, r.UploadID, r.BatchNum, r.Final, r.AttachmentSize,
		r.File, r.InflatedSize, r.ScopeCount, r.ScopesOK,
		r.EnvService, r.EnvVersion, r.EnvLanguage, r.EnvUploadID, r.EnvBatchNum, r.EnvFinal)
}

// SymbolUploadRow is one native symbol upload to sourcemap-intake.<site>'s
// /api/v2/srcmap, in ninjacat.symbol_uploads.
type SymbolUploadRow struct {
	TenantID   string
	ReceivedAt time.Time

	Type          string
	Arch          string
	GNUBuildID    string
	GoBuildID     string
	FileHash      string
	SymbolSource  string
	Origin        string
	OriginVersion string
	Filename      string

	// Meta is the "event" part, byte for byte — the lossless copy of which
	// the nine fields above are convenience extracts (symbolUploadRequestMetadata
	// from comp/host-profiler/symboluploader is unexported, hence the
	// generic decode on the intake side).
	Meta string

	HasELF        uint8
	ELFClass      string // "ELF32" / "ELF64" / "ELF?" / "" when HasELF is 0
	ELFEndianness string // "LE" / "BE" / "?" / "" when HasELF is 0
	ELF           string // the elf_symbol_file part, raw bytes, unparsed
	ELFSize       uint64

	// OtherParts holds every multipart part besides "event" and
	// "elf_symbol_file" — nothing this handler names, but nothing a future
	// producer sends gets thrown away either.
	OtherParts map[string]string
}

func (r SymbolUploadRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Type, r.Arch, r.GNUBuildID, r.GoBuildID,
		r.FileHash, r.SymbolSource, r.Origin, r.OriginVersion, r.Filename,
		r.Meta, r.HasELF, r.ELFClass, r.ELFEndianness, r.ELF, r.ELFSize,
		orEmpty(r.OtherParts))
}

func init() {
	registerWriter(ProfilesWriter, WriterConfig{
		Name: "profiles",
		Insert: `INSERT INTO profiles
			(tenant_id, received_at, variant, dd_evp_origin, dd_evp_origin_version,
			 event, start_raw, start_parsed, end_raw, end_parsed,
			 family, version, runtime, language, tags_profiler,
			 attach_name, attach_bytes, attach_size, attach_parsed,
			 attach_sample_types, attach_sample_units, attach_sample_count,
			 attach_time_nanos, attach_duration_nanos, attach_period_type, attach_period,
			 attach_mapping_count, attach_location_count, attach_function_count)`,
		// Bulky-document class, same as k8s_manifests: a row is a whole
		// profiling submission and its pprof attachments can run to
		// megabytes, so row counts stand for memory rather than events. Tight
		// ceiling, a single flush in flight.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, ProfileRow{})

	registerWriter(DebuggerLogsWriter, WriterConfig{
		Name: "debugger_logs",
		Insert: `INSERT INTO debugger_logs
			(tenant_id, received_at, service, ddsource, ddtags, entry, extra_keys)`,
		// Trickle-in class, same as check_runs: DI log entries only exist
		// while a probe is attached to a running service, not continuously
		// like the main logs table.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, DebuggerLogRow{})

	registerWriter(DebuggerDiagnosticsWriter, WriterConfig{
		Name: "debugger_diagnostics",
		Insert: `INSERT INTO debugger_diagnostics
			(tenant_id, received_at, timestamp, service, ddsource, runtime_id,
			 probe_id, status, probe_version, exception_type, exception_message, message)`,
		// Human-scale class, same as events/k8s_actions: a probe's status
		// changes a handful of times per install/removal, not per request.
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}, DebuggerDiagnosticRow{})

	registerWriter(SymdbUploadsWriter, WriterConfig{
		Name: "symdb_uploads",
		Insert: `INSERT INTO symdb_uploads
			(tenant_id, received_at, service, version, language, runtime_id,
			 upload_id, batch_num, final, attachment_size,
			 file, inflated_size, scope_count, scopes_ok,
			 env_service, env_version, env_language, env_upload_id, env_batch_num, env_final)`,
		// Bulky-document class, same as k8s_manifests: file carries an
		// entire gzip'd symbol-database batch.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, SymdbUploadRow{})

	registerWriter(SymbolUploadsWriter, WriterConfig{
		Name: "symbol_uploads",
		Insert: `INSERT INTO symbol_uploads
			(tenant_id, received_at, type, arch, gnu_build_id, go_build_id, file_hash,
			 symbol_source, origin, origin_version, filename, meta,
			 has_elf, elf_class, elf_endianness, elf, elf_size, other_parts)`,
		// Bulky-document class, same as k8s_manifests: elf carries a whole
		// ELF symbol file.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, SymbolUploadRow{})

	registerTypes(
		WriteProfiles{}, ProfileRow{},
		WriteDebuggerLogs{}, DebuggerLogRow{},
		WriteDebuggerDiagnostics{}, DebuggerDiagnosticRow{},
		WriteSymdbUploads{}, SymdbUploadRow{},
		WriteSymbolUploads{}, SymbolUploadRow{},
	)
}
