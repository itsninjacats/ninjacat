package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// The APM family: everything that arrives on trace.agent.<site>.
//
//	spans                    POST /api/v0.2/traces
//	apm_stats                POST /api/v0.2/stats
//	dsm_pipeline_stats       POST /api/v0.1/pipeline_stats
//	dsm_backlogs             ditto, the bucket's backlog list
//	dsm_bucket_transactions  ditto, the bucket's two packed blobs
//	dsm_messages             POST /api/v2/data_streams_messages
//
// Six tables in ONE file, against registry.go's "a table owns a file", and
// deliberately: the file name decides the order Go runs init() in, and that
// order is the EDF wire identity of every type registered after the registry
// landed. Six files here would interleave with rows_raw.go's registrations
// and shuffle a list CLAUDE.md says never to reorder. One file named after
// the family sorts once, after everything that exists today, and appends its
// types as a block. Schema: schema/migrations/0003_traces.sql.

// ---------------------------------------------------------------------------
// Shared: a decoded DDSketch beside the bytes it came from
// ---------------------------------------------------------------------------

// SketchSummary is one latency distribution: the bytes exactly as the sender
// shipped them, plus what they decoded to.
//
// THE RAW BYTES ARE THE RECORD. A DDSketch merges with other sketches and
// answers any quantile afterwards, so keeping only p50 and p99 would answer
// two questions forever and make cross-service or cross-hour aggregation
// impossible. The decoded fields exist so the common query does not have to
// unmarshal a blob, not because they are enough.
//
// State keeps the three outcomes the decoder already distinguishes apart from
// success: "absent" (no bytes at all), "undecodable" (bytes that are not a
// DDSketch — a real signal, usually version skew), "empty" (a valid sketch
// with no values) and "ok". Collapsing them into one NULL would merge "there
// were no error spans" with "something is shipping us garbage".
//
// Count/Sum/Min/Max are POINTERS: they exist only in the "ok" state, and a
// zero would read as a genuine measurement of zero.
type SketchSummary struct {
	Raw   string
	State string

	Count *float64
	Sum   *float64
	Min   *float64
	Max   *float64

	// The positive-value store, index and count in parallel — the same shape
	// the metric sketches table uses in 0001.
	BinKeys   []int32
	BinCounts []float64
}

// Sketch states, as the Enum8 columns declare them.
const (
	SketchAbsent      = "absent"
	SketchUndecodable = "undecodable"
	SketchEmpty       = "empty"
	SketchOK          = "ok"
)

// args returns the eight columns a summary occupies, in schema order.
//
// The zero value must still be insertable, so an unset State becomes
// "absent": an empty string is not a member of the Enum8 and ClickHouse
// would reject the whole batch — every row in it, not just this one.
func (s SketchSummary) args() []any {
	state := s.State
	if state == "" {
		state = SketchAbsent
	}
	return []any{
		s.Raw, state,
		s.Count, s.Sum, s.Min, s.Max,
		orEmptySlice(s.BinKeys), orEmptySlice(s.BinCounts),
	}
}

// ---------------------------------------------------------------------------
// spans
// ---------------------------------------------------------------------------

// SpansWriter is the process handlers send WriteSpans to.
const SpansWriter = gen.Atom("storage_spans")

// WriteSpans carries a batch of spans. Concrete slice, never []Row.
type WriteSpans struct{ Spans []SpanRow }

func (m WriteSpans) rows() []Row { return toRows(m.Spans) }

// SpanRow is one span, with the agent, tracer and chunk it arrived under
// copied onto it.
//
// WireFormat says which of the two shapes produced the row ("v04" or "idx"),
// and it is not decoration: several columns exist in only one of them, so a
// zero means "this format has no such field" for one row and "the tracer left
// it unset" for the other. Nothing else can tell those apart.
//
// The five ContainerDebug fields are pointers because the whole sub-message is
// optional and only present when the agent struggled to resolve the container
// tags — the usual case is nothing to report, not a zero-length struggle.
type SpanRow struct {
	TenantID   string
	ReceivedAt time.Time

	WireFormat string // "v04" | "idx"

	AgentHostname      string
	AgentEnv           string
	AgentVersion       string
	TargetTPS          float64
	ErrorTPS           float64
	RareSamplerEnabled uint8

	// AgentTags, TracerTags and ChunkTags are protobuf map<string,string>,
	// not Datadog's flat "key:value" list, so they are plain maps — the
	// multiset shape exists for a repetition a protobuf map cannot have.
	AgentTags map[string]string

	ContainerID     string
	Language        string
	LanguageVersion string
	TracerVersion   string
	RuntimeID       string
	TracerEnv       string
	TracerHostname  string
	AppVersion      string
	TracerTags      map[string]string

	// TracerAttributesJSON and ChunkAttributesJSON hold the idx typed form of
	// the same attributes, where a value can be bytes, an array or a nested
	// key-value list. Empty for v0.4, which has no typed attributes.
	TracerAttributesJSON string

	ContainerDebugError                *string
	ContainerDebugLatencyMs            *int64
	ContainerDebugWasBuffered          *uint8
	ContainerDebugBufferMs             *int64
	ContainerDebugBufferEvictionReason *string

	Priority            int32
	Origin              string
	DroppedTrace        uint8
	SamplingMechanism   uint32
	ChunkTags           map[string]string
	ChunkAttributesJSON string

	Service  string
	Name     string
	Resource string
	SpanType string

	// The 128-bit trace id, normalized out of two very different wire shapes:
	// v0.4 splits it between Span.TraceID and Meta["_dd.p.tid"], idx carries
	// all 16 bytes on the chunk. Integers end to end — a trace id through a
	// float64 loses its low bits.
	TraceID     uint64
	TraceIDHigh uint64

	SpanID uint64
	// 0 means root. The wire has no has-parent bit, so the zero is kept.
	ParentID uint64

	Start      time.Time
	DurationNs int64
	Error      int32

	Kind        string
	SpanEnv     string
	SpanVersion string
	Component   string

	Meta    map[string]string
	Metrics map[string]float64
	// MetaStruct values are msgpack blobs with no published per-key type, so
	// they stay raw bytes in a Go string — a byte string, not text.
	MetaStruct map[string]string

	// AttributesJSON is the idx typed attribute map in full. meta/metrics
	// above carry the scalar projection of the same data so one query spans
	// both formats; this carries what projection cannot hold.
	AttributesJSON string

	// Span links and events as parallel arrays: the index IS the wire order,
	// which protobuf preserves and a reader needs.
	LinkTraceID     []uint64
	LinkTraceIDHigh []uint64
	LinkSpanID      []uint64
	LinkAttributes  []string
	LinkTracestate  []string
	LinkFlags       []uint32

	EventTime       []time.Time
	EventName       []string
	EventAttributes []string
}

func (r SpanRow) AppendTo(b driver.Batch) error {
	return b.Append(
		r.TenantID, r.ReceivedAt, r.WireFormat,
		r.AgentHostname, r.AgentEnv, r.AgentVersion,
		r.TargetTPS, r.ErrorTPS, r.RareSamplerEnabled, orEmpty(r.AgentTags),
		r.ContainerID, r.Language, r.LanguageVersion, r.TracerVersion,
		r.RuntimeID, r.TracerEnv, r.TracerHostname, r.AppVersion,
		orEmpty(r.TracerTags), r.TracerAttributesJSON,
		r.ContainerDebugError, r.ContainerDebugLatencyMs, r.ContainerDebugWasBuffered,
		r.ContainerDebugBufferMs, r.ContainerDebugBufferEvictionReason,
		r.Priority, r.Origin, r.DroppedTrace, r.SamplingMechanism,
		orEmpty(r.ChunkTags), r.ChunkAttributesJSON,
		r.Service, r.Name, r.Resource, r.SpanType,
		r.TraceID, r.TraceIDHigh, r.SpanID, r.ParentID,
		r.Start, r.DurationNs, r.Error,
		r.Kind, r.SpanEnv, r.SpanVersion, r.Component,
		orEmpty(r.Meta), orEmpty(r.Metrics), orEmpty(r.MetaStruct), r.AttributesJSON,
		orEmptySlice(r.LinkTraceID), orEmptySlice(r.LinkTraceIDHigh), orEmptySlice(r.LinkSpanID),
		orEmptySlice(r.LinkAttributes), orEmptySlice(r.LinkTracestate), orEmptySlice(r.LinkFlags),
		orEmptySlice(r.EventTime), orEmptySlice(r.EventName), orEmptySlice(r.EventAttributes),
	)
}

// ---------------------------------------------------------------------------
// apm_stats
// ---------------------------------------------------------------------------

// APMStatsWriter is the process handlers send WriteAPMStats to.
const APMStatsWriter = gen.Atom("storage_apm_stats")

// WriteAPMStats carries a batch of grouped stats.
type WriteAPMStats struct{ Stats []APMStatRow }

func (m WriteAPMStats) rows() []Row { return toRows(m.Stats) }

// APMStatRow is one pb.ClientGroupedStats with its three enclosing levels
// denormalized onto it.
//
// These counts are COMPLETE — the agent's concentrator sees every trace
// before the sampler runs — which is the whole reason this table is not
// derived from spans.
type APMStatRow struct {
	TenantID   string
	ReceivedAt time.Time

	AgentHostname  string
	AgentEnv       string
	AgentVersion   string
	ClientComputed uint8
	SplitPayload   uint8

	ClientHostname         string
	ClientEnv              string
	ClientVersion          string
	ClientLang             string
	ClientTracerVersion    string
	ClientRuntimeID        string
	ClientSequence         uint64
	ClientAgentAggregation string
	ClientService          string
	ClientContainerID      string

	// The one genuine Datadog tag list in the trace protocol: a flat
	// []string of "key:value", so it gets the multiset shape.
	ClientTags map[string][]string

	ClientGitCommitSha    string
	ClientImageTag        string
	ClientProcessTags     string
	ClientProcessTagsHash uint64

	BucketStart      time.Time
	BucketDurationNs uint64
	AgentTimeShiftNs int64

	Service        string
	Name           string
	Resource       string
	HTTPStatusCode uint32
	SpanType       string
	DBType         string
	Hits           uint64
	Errors         uint64
	DurationNs     uint64
	Synthetics     uint8
	TopLevelHits   uint64
	SpanKind       string
	PeerTags       []string

	// IsTraceRoot is an Enum8 in ClickHouse, so it travels as its NAME:
	// "not_set", "true" or "false". Three states, not a bool — "the tracer
	// did not say" is a real answer.
	IsTraceRoot string

	GRPCStatusCode         string
	HTTPMethod             string
	HTTPEndpoint           string
	ServiceSource          string
	SpanDerivedPrimaryTags []string
	AdditionalMetricTags   []string

	OkSummary    SketchSummary
	ErrorSummary SketchSummary
}

func (r APMStatRow) AppendTo(b driver.Batch) error {
	isRoot := r.IsTraceRoot
	if isRoot == "" {
		// Not a member of the Enum8; the zero value has to stay insertable
		// because one bad row fails the whole batch around it.
		isRoot = "not_set"
	}

	args := []any{
		r.TenantID, r.ReceivedAt,
		r.AgentHostname, r.AgentEnv, r.AgentVersion, r.ClientComputed, r.SplitPayload,
		r.ClientHostname, r.ClientEnv, r.ClientVersion, r.ClientLang, r.ClientTracerVersion,
		r.ClientRuntimeID, r.ClientSequence, r.ClientAgentAggregation, r.ClientService,
		r.ClientContainerID, orEmpty(r.ClientTags),
		r.ClientGitCommitSha, r.ClientImageTag, r.ClientProcessTags, r.ClientProcessTagsHash,
		r.BucketStart, r.BucketDurationNs, r.AgentTimeShiftNs,
		r.Service, r.Name, r.Resource, r.HTTPStatusCode, r.SpanType, r.DBType,
		r.Hits, r.Errors, r.DurationNs, r.Synthetics, r.TopLevelHits, r.SpanKind,
		orEmptySlice(r.PeerTags), isRoot,
		r.GRPCStatusCode, r.HTTPMethod, r.HTTPEndpoint, r.ServiceSource,
		orEmptySlice(r.SpanDerivedPrimaryTags), orEmptySlice(r.AdditionalMetricTags),
	}
	args = append(args, r.OkSummary.args()...)
	args = append(args, r.ErrorSummary.args()...)
	return b.Append(args...)
}

// ---------------------------------------------------------------------------
// Data Streams Monitoring
// ---------------------------------------------------------------------------

// DSMPipelineStatsWriter, DSMBacklogsWriter, DSMBucketTransactionsWriter and
// DSMMessagesWriter are the four Data Streams tables. The first three are fed
// by one request each (a StatsPayload fans out into all three), the fourth by
// a different endpoint entirely.
const (
	DSMPipelineStatsWriter      = gen.Atom("storage_dsm_pipeline_stats")
	DSMBacklogsWriter           = gen.Atom("storage_dsm_backlogs")
	DSMBucketTransactionsWriter = gen.Atom("storage_dsm_bucket_transactions")
	DSMMessagesWriter           = gen.Atom("storage_dsm_messages")
)

type WriteDSMPipelineStats struct{ Points []DSMPipelineStatRow }
type WriteDSMBacklogs struct{ Backlogs []DSMBacklogRow }
type WriteDSMBucketTransactions struct{ Buckets []DSMBucketTransactionRow }
type WriteDSMMessages struct{ Messages []DSMMessageRow }

func (m WriteDSMPipelineStats) rows() []Row      { return toRows(m.Points) }
func (m WriteDSMBacklogs) rows() []Row           { return toRows(m.Backlogs) }
func (m WriteDSMBucketTransactions) rows() []Row { return toRows(m.Buckets) }
func (m WriteDSMMessages) rows() []Row           { return toRows(m.Messages) }

// DSMCommon is the payload-and-bucket context every Data Streams row carries.
//
// Embedded rather than repeated because the three tables below are three
// children of the same bucket and drifting field sets between them would make
// a join impossible. It is NOT a Row: it has no writer and no INSERT, and its
// args() is spliced into each row's own.
type DSMCommon struct {
	TenantID   string
	ReceivedAt time.Time

	Env           string
	Service       string
	TracerVersion string
	Lang          string
	Version       string
	ProcessTags   []string
	ProductMask   uint64

	BucketStart      time.Time
	BucketDurationNs uint64
}

func (c DSMCommon) args() []any {
	return []any{
		c.TenantID, c.ReceivedAt,
		c.Env, c.Service, c.TracerVersion, c.Lang, c.Version,
		orEmptySlice(c.ProcessTags), c.ProductMask,
		c.BucketStart, c.BucketDurationNs,
	}
}

// DSMPipelineStatRow is one StatsPoint: a checkpoint in a queue pathway.
//
// Hash and ParentHash rebuild the pathway graph by self-join, so they are
// carried as the 64-bit integers they are on the wire.
type DSMPipelineStatRow struct {
	DSMCommon

	EdgeTags      []string
	Hash          uint64
	ParentHash    uint64
	TimestampType string

	// Three sketches, all in seconds except PayloadSize, which is a
	// distribution of message sizes in bytes. dd-trace-go's field name reads
	// like a scalar; its type ([]byte) says otherwise.
	PathwayLatency SketchSummary
	EdgeLatency    SketchSummary
	PayloadSize    SketchSummary

	// The proxy headers the trace-agent adds on the way through. Container
	// identity appears NOWHERE in the body, so these are the only link from a
	// pathway to a workload.
	Via             string
	AdditionalTags  string
	ContainerTags   string
	ContentEncoding string

	// Keys the decoder did not recognise, and their values as JSON.
	// dd-trace-go adds fields to this payload with no version negotiation, so
	// an unknown key is expected — and a name without a value would tell us a
	// field exists and nothing about what it carries.
	UnknownKeys []string
	UnknownJSON string
}

func (r DSMPipelineStatRow) AppendTo(b driver.Batch) error {
	args := r.DSMCommon.args()
	args = append(args, orEmptySlice(r.EdgeTags), r.Hash, r.ParentHash, r.TimestampType)
	args = append(args, r.PathwayLatency.args()...)
	args = append(args, r.EdgeLatency.args()...)
	args = append(args, r.PayloadSize.args()...)
	args = append(args, r.Via, r.AdditionalTags, r.ContainerTags, r.ContentEncoding,
		orEmptySlice(r.UnknownKeys), r.UnknownJSON)
	return b.Append(args...)
}

// DSMBacklogRow is one Backlog: how far behind a consumer is.
//
// Tags is an ORDERED array, not a map: the tracer builds the list and its
// order is part of the identity it hashes on.
type DSMBacklogRow struct {
	DSMCommon

	Tags  []string
	Value int64
}

func (r DSMBacklogRow) AppendTo(b driver.Batch) error {
	args := r.DSMCommon.args()
	args = append(args, orEmptySlice(r.Tags), r.Value)
	return b.Append(args...)
}

// DSMBucketTransactionRow keeps a bucket's two packed binary blobs verbatim.
//
// Not decoded on purpose: the layout is hand-rolled binary that dd-trace-go
// says must match the Java tracer byte for byte, so a decoder written here
// against today's comment would silently mis-read tomorrow's records. The
// bytes are the record; whoever needs the transactions parses them.
type DSMBucketTransactionRow struct {
	DSMCommon

	Transactions             string
	TransactionCheckpointIDs string
}

func (r DSMBucketTransactionRow) AppendTo(b driver.Batch) error {
	args := r.DSMCommon.args()
	args = append(args, r.Transactions, r.TransactionCheckpointIDs)
	return b.Append(args...)
}

// DSMMessageRow is one element of the /api/v2/data_streams_messages array.
//
// The producer is not in the open agent repository, so the element is stored
// as the JSON text it arrived as and no shape is invented. Keys is the one
// thing extractable without guessing: it turns "what does this track send?"
// into a GROUP BY. Position keeps the batch order, which is the only ordering
// these messages have.
type DSMMessageRow struct {
	TenantID   string
	ReceivedAt time.Time

	Position uint32
	Message  string
	Keys     []string

	DDEVPOrigin        string
	DDEVPOriginVersion string
	ContentEncoding    string
}

func (r DSMMessageRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Position, r.Message,
		orEmptySlice(r.Keys), r.DDEVPOrigin, r.DDEVPOriginVersion, r.ContentEncoding)
}

func init() {
	registerWriter(SpansWriter, WriterConfig{
		Name: "spans",
		Insert: `INSERT INTO spans
			(tenant_id, received_at, wire_format,
			 agent_hostname, agent_env, agent_version,
			 target_tps, error_tps, rare_sampler_enabled, agent_tags,
			 container_id, language, language_version, tracer_version,
			 runtime_id, tracer_env, tracer_hostname, app_version,
			 tracer_tags, tracer_attributes_json,
			 container_debug_error, container_debug_latency_ms, container_debug_was_buffered,
			 container_debug_buffer_ms, container_debug_buffer_eviction_reason,
			 priority, origin, dropped_trace, sampling_mechanism,
			 chunk_tags, chunk_attributes_json,
			 service, name, resource, span_type,
			 trace_id, trace_id_high, span_id, parent_id,
			 start, duration_ns, error,
			 kind, span_env, span_version, component,
			 meta, metrics, meta_struct, attributes_json,
			 link_trace_id, link_trace_id_high, link_span_id,
			 link_attributes, link_tracestate, link_flags,
			 event_time, event_name, event_attributes)`,
		// The logs class — this is the highest-volume table of the six — but
		// a span row is far heavier than a log line (three maps, nine arrays),
		// so the batch is half the size logs use and the buffer stays at the
		// default rather than logs' 500k. Between "highest-volume" and
		// "bulky periodic snapshot"; nearer the first.
		MaxRows: 10_000, FlushInterval: 2 * time.Second,
		BufferLimit: 200_000, MaxInFlight: 4,
	}, SpanRow{})

	registerWriter(APMStatsWriter, WriterConfig{
		Name: "apm_stats",
		Insert: `INSERT INTO apm_stats
			(tenant_id, received_at,
			 agent_hostname, agent_env, agent_version, client_computed, split_payload,
			 client_hostname, client_env, client_version, client_lang, client_tracer_version,
			 client_runtime_id, client_sequence, client_agent_aggregation, client_service,
			 client_container_id, client_tags,
			 client_git_commit_sha, client_image_tag, client_process_tags, client_process_tags_hash,
			 bucket_start, bucket_duration_ns, agent_time_shift_ns,
			 service, name, resource, http_status_code, span_type, db_type,
			 hits, errors, duration_ns, synthetics, top_level_hits, span_kind,
			 peer_tags, is_trace_root,
			 grpc_status_code, http_method, http_endpoint, service_source,
			 span_derived_primary_tags, additional_metric_tags,
			 ok_summary, ok_summary_state, ok_count, ok_sum, ok_min, ok_max,
			 ok_bin_keys, ok_bin_counts,
			 error_summary, error_summary_state, error_count, error_sum, error_min, error_max,
			 error_bin_keys, error_bin_counts)`,
		// Bursty per pass, like k8s_resources: the agent flushes a
		// concentrator bucket every ten seconds and it arrives all at once,
		// carrying two sketch blobs per row.
		MaxRows: 2_000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, APMStatRow{})

	registerWriter(DSMPipelineStatsWriter, WriterConfig{
		Name: "dsm_pipeline_stats",
		Insert: `INSERT INTO dsm_pipeline_stats
			(tenant_id, received_at,
			 env, service, tracer_version, lang, version, process_tags, product_mask,
			 bucket_start, bucket_duration_ns,
			 edge_tags, hash, parent_hash, timestamp_type,
			 pathway_latency, pathway_latency_state,
			 pathway_count, pathway_sum, pathway_min, pathway_max,
			 pathway_bin_keys, pathway_bin_counts,
			 edge_latency, edge_latency_state,
			 edge_count, edge_sum, edge_min, edge_max,
			 edge_bin_keys, edge_bin_counts,
			 payload_size, payload_size_state,
			 payload_size_count, payload_size_sum, payload_size_min, payload_size_max,
			 payload_size_bin_keys, payload_size_bin_counts,
			 via, additional_tags, container_tags, content_encoding,
			 unknown_keys, unknown_json)`,
		// Same shape as apm_stats — a bucket per flush, three sketches a row.
		MaxRows: 2_000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, DSMPipelineStatRow{})

	registerWriter(DSMBacklogsWriter, WriterConfig{
		Name: "dsm_backlogs",
		Insert: `INSERT INTO dsm_backlogs
			(tenant_id, received_at,
			 env, service, tracer_version, lang, version, process_tags, product_mask,
			 bucket_start, bucket_duration_ns,
			 tags, value)`,
		// A handful per bucket at most: small rows, few of them, so the timer
		// does the flushing — the hosts class.
		MaxRows: 500, FlushInterval: 10 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, DSMBacklogRow{})

	registerWriter(DSMBucketTransactionsWriter, WriterConfig{
		Name: "dsm_bucket_transactions",
		Insert: `INSERT INTO dsm_bucket_transactions
			(tenant_id, received_at,
			 env, service, tracer_version, lang, version, process_tags, product_mask,
			 bucket_start, bucket_duration_ns,
			 transactions, transaction_checkpoint_ids)`,
		// One row per bucket, each carrying two opaque blobs: the bulky
		// document class, tight ceilings and one flush in flight.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, DSMBucketTransactionRow{})

	registerWriter(DSMMessagesWriter, WriterConfig{
		Name: "dsm_messages",
		Insert: `INSERT INTO dsm_messages
			(tenant_id, received_at, position, message, keys,
			 dd_evp_origin, dd_evp_origin_version, content_encoding)`,
		// An event-platform track: batched JSON arriving in bursts, each row
		// a whole document. Sized like container_images — periodic, bulky,
		// not hot.
		MaxRows: 500, FlushInterval: 10 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, DSMMessageRow{})

	registerTypes(
		WriteSpans{}, SpanRow{},
		WriteAPMStats{}, APMStatRow{},
		WriteDSMPipelineStats{}, DSMPipelineStatRow{},
		WriteDSMBacklogs{}, DSMBacklogRow{},
		WriteDSMBucketTransactions{}, DSMBucketTransactionRow{},
		WriteDSMMessages{}, DSMMessageRow{},
	)
}
