package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// RUM — everything the browser, iOS and Android SDKs send on
// browser-intake.<site>: rum_views, rum_events, rum_telemetry, rum_timeseries,
// rum_replay_segments, rum_spans. See schema/migrations/0013_rum.sql for why
// these are six tables and not one or twenty, and intake/router_rum.go for
// what fills them.
//
// Every row here carries the whole payload it came from in a String column
// (Event / Segment / Span). That is the point rather than an afterthought: the
// RUM schema has well over a hundred optional fields per variant and grows
// with every SDK release, so the typed columns are a filtering index over the
// event and the event is the record.

// The processes handlers send to. One per table, never a pool — a burst of
// replay blobs must not delay view updates.
const (
	RumViewsWriter          = gen.Atom("storage_rum_views")
	RumEventsWriter         = gen.Atom("storage_rum_events")
	RumTelemetryWriter      = gen.Atom("storage_rum_telemetry")
	RumTimeseriesWriter     = gen.Atom("storage_rum_timeseries")
	RumReplaySegmentsWriter = gen.Atom("storage_rum_replay_segments")
	RumSpansWriter          = gen.Atom("storage_rum_spans")
)

// Messages. Concrete slices, never []Row — see the note in messages.go.
type (
	WriteRumViews          struct{ Views []RumViewRow }
	WriteRumEvents         struct{ Events []RumEventRow }
	WriteRumTelemetry      struct{ Events []RumTelemetryRow }
	WriteRumTimeseries     struct{ Series []RumTimeseriesRow }
	WriteRumReplaySegments struct{ Segments []RumReplaySegmentRow }
	WriteRumSpans          struct{ Spans []RumSpanRow }
)

func (m WriteRumViews) rows() []Row          { return toRows(m.Views) }
func (m WriteRumEvents) rows() []Row         { return toRows(m.Events) }
func (m WriteRumTelemetry) rows() []Row      { return toRows(m.Events) }
func (m WriteRumTimeseries) rows() []Row     { return toRows(m.Series) }
func (m WriteRumReplaySegments) rows() []Row { return toRows(m.Segments) }
func (m WriteRumSpans) rows() []Row          { return toRows(m.Spans) }

// RumRequest is the block of request-level columns every RUM table starts
// with, in the order every INSERT lists them.
//
// It is a named field rather than an embedded struct because Ergo's encoder
// resolves types by name and an anonymous field is one more thing to be sure
// about at the moment storage moves to its own node; a plain field costs
// nothing and is unambiguous.
//
// WHY THESE COLUMNS AT ALL: a RUM event has no hostname, no agent and no
// container id. The only thing that identifies its sender is the request it
// arrived in — which SDK (evp_origin), which version, which batch
// (request_id/idempotency_key), from where (remote_addr, user_agent). Losing
// that block would leave a table of events nobody can attribute.
type RumRequest struct {
	TenantID   string
	ReceivedAt time.Time

	DDSource         string
	EVPOrigin        string
	EVPOriginVersion string

	// EVPEncoding is how the batch arrived compressed — "deflate" from the
	// browser's ?dd-evp-encoding, "gzip"/"deflate" from the mobile SDKs'
	// Content-Encoding, "" from a sender that compressed nothing. The body is
	// stored decoded, so nothing else in the row remembers this.
	EVPEncoding string

	RequestID      string
	IdempotencyKey string

	// Browser only: ?_dd.api=fetch|beacon, the transport the batch travelled
	// on. A beacon is fire-and-forget at page unload, so its failures are
	// invisible to the page and a gap in beacon traffic means something
	// different from a gap in fetch traffic.
	DDAPI string

	// Pointers, because absent is not zero: a first attempt has no retry
	// count, and "retried zero times" is a different fact from "never
	// retried". nil reaches the Nullable column as NULL.
	BatchTime  *time.Time
	RetryCount *uint32
	RetryAfter *int64

	RemoteAddr string
	UserAgent  string

	// QueryExtra is every query parameter without a column of its own, as
	// JSON, credentials removed. It is the `event` column's bargain applied to
	// the URL: an SDK release that adds a parameter must not make it
	// invisible.
	QueryExtra string
}

// args returns the fifteen shared columns in INSERT order. Each row's
// AppendTo starts from this and appends its own.
func (q RumRequest) args() []any {
	return []any{
		q.TenantID, q.ReceivedAt,
		q.DDSource, q.EVPOrigin, q.EVPOriginVersion, q.EVPEncoding,
		q.RequestID, q.IdempotencyKey,
		q.DDAPI, q.BatchTime, q.RetryCount, q.RetryAfter,
		q.RemoteAddr, q.UserAgent, q.QueryExtra,
	}
}

// RumViewRow is one view in rum_views, upserted on (application, session,
// view) with DocumentVersion as the ReplacingMergeTree version.
//
// The counters and Web Vitals are POINTERS throughout. A view that reports no
// crash sub-object has not measured zero crashes — it is a platform that does
// not produce crashes at all, and a 0 there would turn "unknown" into a fact
// somebody averages.
type RumViewRow struct {
	Req RumRequest

	Date            time.Time
	ApplicationID   string
	SessionID       string
	ViewID          string
	DocumentVersion uint64

	// "view" or "view_update" — which wire form produced this version.
	EventType string

	Service      string
	Version      string
	BuildVersion string
	BuildID      string
	Source       string

	SessionType             string
	SessionHasReplay        *bool
	SessionIsActive         *bool
	SessionSampledForReplay *bool

	UsrID          string
	UsrName        string
	UsrEmail       string
	UsrAnonymousID string
	AccountID      string
	AccountName    string

	ViewURL      string
	ViewName     string
	ViewReferrer string

	ViewLoadingType    string
	ViewLoadingTime    *int64
	ViewTimeSpent      int64
	ViewIsActive       *bool
	ViewIsSlowRendered *bool

	ActionCount      *int64
	ErrorCount       *int64
	CrashCount       *int64
	LongTaskCount    *int64
	FrozenFrameCount *int64
	ResourceCount    *int64
	FrustrationCount *int64

	LCP  *int64
	CLS  *float64
	INP  *int64
	FCP  *int64
	FID  *int64
	FBC  *int64
	TTFB *int64

	DeviceType  string
	DeviceBrand string
	DeviceModel string
	DeviceName  string
	OSName      string
	OSVersion   string

	ConnectivityStatus string

	// JSON text: both are free-form maps whose values are arbitrary JSON.
	Context      string
	FeatureFlags string

	Tags map[string][]string

	// The whole event, verbatim.
	Event string
}

func (r RumViewRow) AppendTo(b driver.Batch) error {
	return b.Append(append(r.Req.args(),
		r.Date, r.ApplicationID, r.SessionID, r.ViewID, r.DocumentVersion, r.EventType,
		r.Service, r.Version, r.BuildVersion, r.BuildID, r.Source,
		r.SessionType, r.SessionHasReplay, r.SessionIsActive, r.SessionSampledForReplay,
		r.UsrID, r.UsrName, r.UsrEmail, r.UsrAnonymousID, r.AccountID, r.AccountName,
		r.ViewURL, r.ViewName, r.ViewReferrer,
		r.ViewLoadingType, r.ViewLoadingTime, r.ViewTimeSpent, r.ViewIsActive, r.ViewIsSlowRendered,
		r.ActionCount, r.ErrorCount, r.CrashCount, r.LongTaskCount, r.FrozenFrameCount,
		r.ResourceCount, r.FrustrationCount,
		r.LCP, r.CLS, r.INP, r.FCP, r.FID, r.FBC, r.TTFB,
		r.DeviceType, r.DeviceBrand, r.DeviceModel, r.DeviceName, r.OSName, r.OSVersion,
		r.ConnectivityStatus, r.Context, r.FeatureFlags, orEmpty(r.Tags), r.Event,
	)...)
}

// RumEventRow is one action, error, resource, long_task, vital or transition
// in rum_events. The per-kind columns of the other kinds stay empty; the kind
// itself is EventType, and the whole event is in Event either way.
type RumEventRow struct {
	Req RumRequest

	Date          time.Time
	ApplicationID string
	SessionID     string
	ViewID        string
	EventType     string

	Service      string
	Version      string
	BuildVersion string
	BuildID      string
	Source       string

	SessionType      string
	SessionHasReplay *bool

	UsrID          string
	UsrName        string
	UsrEmail       string
	UsrAnonymousID string
	AccountID      string
	AccountName    string

	ViewURL      string
	ViewName     string
	ViewReferrer string

	DeviceType         string
	DeviceBrand        string
	DeviceModel        string
	DeviceName         string
	OSName             string
	OSVersion          string
	ConnectivityStatus string

	ActionType string
	ActionName string
	ActionID   string
	// One click can be classified several ways at once (rage AND dead), so
	// this is a list, not a value.
	ActionFrustrationTypes []string

	ErrorID          string
	ErrorMessage     string
	ErrorType        string
	ErrorSource      string
	ErrorStack       string
	ErrorIsCrash     *bool
	ErrorHandling    string
	ErrorFingerprint string

	ResourceID         string
	ResourceType       string
	ResourceURL        string
	ResourceMethod     string
	ResourceStatusCode *int64
	ResourceDuration   *int64
	ResourceSize       *int64

	LongTaskID       string
	LongTaskDuration *int64

	VitalID       string
	VitalType     string
	VitalName     string
	VitalDuration *int64

	Context string
	Tags    map[string][]string
	Event   string
}

func (r RumEventRow) AppendTo(b driver.Batch) error {
	return b.Append(append(r.Req.args(),
		r.Date, r.ApplicationID, r.SessionID, r.ViewID, r.EventType,
		r.Service, r.Version, r.BuildVersion, r.BuildID, r.Source,
		r.SessionType, r.SessionHasReplay,
		r.UsrID, r.UsrName, r.UsrEmail, r.UsrAnonymousID, r.AccountID, r.AccountName,
		r.ViewURL, r.ViewName, r.ViewReferrer,
		r.DeviceType, r.DeviceBrand, r.DeviceModel, r.DeviceName, r.OSName, r.OSVersion,
		r.ConnectivityStatus,
		r.ActionType, r.ActionName, r.ActionID, orEmptySlice(r.ActionFrustrationTypes),
		r.ErrorID, r.ErrorMessage, r.ErrorType, r.ErrorSource, r.ErrorStack,
		r.ErrorIsCrash, r.ErrorHandling, r.ErrorFingerprint,
		r.ResourceID, r.ResourceType, r.ResourceURL, r.ResourceMethod,
		r.ResourceStatusCode, r.ResourceDuration, r.ResourceSize,
		r.LongTaskID, r.LongTaskDuration,
		r.VitalID, r.VitalType, r.VitalName, r.VitalDuration,
		r.Context, orEmpty(r.Tags), r.Event,
	)...)
}

// RumTelemetryRow is one type:"telemetry" event — the SDK reporting on itself.
type RumTelemetryRow struct {
	Req RumRequest

	Date          time.Time
	ApplicationID string
	SessionID     string
	ViewID        string
	ActionID      string

	Type          string
	Status        string
	TelemetryType string

	Message    string
	ErrorStack string
	ErrorKind  string

	// The whole telemetry.configuration object, as JSON.
	Configuration string
	UsageFeature  string

	Service string
	Version string
	Source  string

	EffectiveSampleRate  *float64
	ExperimentalFeatures []string

	DeviceBrand        string
	DeviceModel        string
	DeviceArchitecture string
	OSName             string
	OSVersion          string
	OSBuild            string

	Event string
}

func (r RumTelemetryRow) AppendTo(b driver.Batch) error {
	return b.Append(append(r.Req.args(),
		r.Date, r.ApplicationID, r.SessionID, r.ViewID, r.ActionID,
		r.Type, r.Status, r.TelemetryType,
		r.Message, r.ErrorStack, r.ErrorKind,
		r.Configuration, r.UsageFeature,
		r.Service, r.Version, r.Source,
		r.EffectiveSampleRate, orEmptySlice(r.ExperimentalFeatures),
		r.DeviceBrand, r.DeviceModel, r.DeviceArchitecture,
		r.OSName, r.OSVersion, r.OSBuild,
		r.Event,
	)...)
}

// RumTimeseriesRow is one mobile cpu/memory sample run. Timestamps and values
// stay as the parallel arrays they arrived as — see the migration for why they
// are not exploded into one row per sample.
type RumTimeseriesRow struct {
	Req RumRequest

	Date          time.Time
	ApplicationID string
	SessionID     string
	ViewID        string

	Name   string
	ID     string
	Schema string

	Start      time.Time
	End        time.Time
	Timestamps []int64

	// data.values, as JSON: a different object per variant.
	Values string

	Service string
	Version string
	Source  string
	Context string
	Tags    map[string][]string
	Event   string
}

func (r RumTimeseriesRow) AppendTo(b driver.Batch) error {
	return b.Append(append(r.Req.args(),
		r.Date, r.ApplicationID, r.SessionID, r.ViewID,
		r.Name, r.ID, r.Schema,
		r.Start, r.End, orEmptySlice(r.Timestamps), r.Values,
		r.Service, r.Version, r.Source, r.Context, orEmpty(r.Tags), r.Event,
	)...)
}

// RumReplaySegmentRow is one blob part of a /api/v2/replay upload, with the
// metadata object that described it.
//
// Segment holds the zlib bytes EXACTLY as received. Nothing inflates them: the
// intake stores replay without processing, and the SDK's encoder emits streams
// built to be concatenated later.
type RumReplaySegmentRow struct {
	Req RumRequest

	ApplicationID string
	SessionID     string
	ViewID        string
	Source        string

	// "segment" or "resource".
	Variant      string
	PartName     string
	PartFilename string

	Start *time.Time
	End   *time.Time

	RecordsCount    *int64
	HasFullSnapshot *bool
	IndexInView     *int64
	CreationReason  string

	RawSegmentSize        *int64
	CompressedSegmentSize *int64

	Segment string
	Image   string
	Event   string
}

func (r RumReplaySegmentRow) AppendTo(b driver.Batch) error {
	return b.Append(append(r.Req.args(),
		r.ApplicationID, r.SessionID, r.ViewID, r.Source,
		r.Variant, r.PartName, r.PartFilename,
		r.Start, r.End, r.RecordsCount, r.HasFullSnapshot, r.IndexInView, r.CreationReason,
		r.RawSegmentSize, r.CompressedSegmentSize,
		r.Segment, r.Image, r.Event,
	)...)
}

// RumSpanRow is one span out of a /api/v2/spans envelope.
//
// The three ids are STRINGS. The mobile SDKs encode them as decimal or hex
// text of 64- and 128-bit numbers; parsing them would need a guess about the
// base, and a float64 anywhere on that path drops digits above 2^53.
type RumSpanRow struct {
	Req RumRequest

	TraceID  string
	SpanID   string
	ParentID string

	Name     string
	Service  string
	Resource string
	Type     string

	// From the envelope around the span, not from the span.
	Env string

	Start      time.Time
	DurationNS int64
	Error      int8

	// meta's nested objects (device, os) arrive flattened to dotted keys.
	Meta    map[string]string
	Metrics map[string]float64

	Span string
	// Envelope keys other than "spans" and "env", as JSON.
	EnvelopeExtra string
}

func (r RumSpanRow) AppendTo(b driver.Batch) error {
	return b.Append(append(r.Req.args(),
		r.TraceID, r.SpanID, r.ParentID,
		r.Name, r.Service, r.Resource, r.Type, r.Env,
		r.Start, r.DurationNS, r.Error,
		orEmpty(r.Meta), orEmpty(r.Metrics),
		r.Span, r.EnvelopeExtra,
	)...)
}

func init() {
	// The shared request block, spelled once so the six INSERTs below cannot
	// drift from each other. Kept as a string constant rather than a helper
	// because WriterConfig.Insert is what storagetest counts columns in.
	const req = `tenant_id, received_at, ddsource, evp_origin, evp_origin_version, evp_encoding,
		 request_id, idempotency_key, dd_api, batch_time, retry_count, retry_after,
		 remote_addr, user_agent, query_extra`

	registerWriter(RumViewsWriter, WriterConfig{
		Name: "rum_views",
		Insert: `INSERT INTO rum_views
			(` + req + `,
			 date, application_id, session_id, view_id, document_version, event_type,
			 service, version, build_version, build_id, source,
			 session_type, session_has_replay, session_is_active, session_sampled_for_replay,
			 usr_id, usr_name, usr_email, usr_anonymous_id, account_id, account_name,
			 view_url, view_name, view_referrer,
			 view_loading_type, view_loading_time, view_time_spent, view_is_active,
			 view_is_slow_rendered,
			 action_count, error_count, crash_count, long_task_count, frozen_frame_count,
			 resource_count, frustration_count,
			 lcp, cls, inp, fcp, fid, fbc, ttfb,
			 device_type, device_brand, device_model, device_name, os_name, os_version,
			 connectivity_status, context, feature_flags, ddtags, event)`,
		// Bursty-per-pass, like k8s_resources: an active page re-sends its
		// view every few seconds and every row carries the whole event JSON,
		// so the row ceilings stand for real memory rather than for rows.
		MaxRows: 2000, FlushInterval: 3 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, RumViewRow{})

	registerWriter(RumEventsWriter, WriterConfig{
		Name: "rum_events",
		Insert: `INSERT INTO rum_events
			(` + req + `,
			 date, application_id, session_id, view_id, event_type,
			 service, version, build_version, build_id, source,
			 session_type, session_has_replay,
			 usr_id, usr_name, usr_email, usr_anonymous_id, account_id, account_name,
			 view_url, view_name, view_referrer,
			 device_type, device_brand, device_model, device_name, os_name, os_version,
			 connectivity_status,
			 action_type, action_name, action_id, action_frustration_types,
			 error_id, error_message, error_type, error_source, error_stack,
			 error_is_crash, error_handling, error_fingerprint,
			 resource_id, resource_type, resource_url, resource_method,
			 resource_status_code, resource_duration, resource_size,
			 long_task_id, long_task_duration,
			 vital_id, vital_type, vital_name, vital_duration,
			 context, ddtags, event)`,
		// The high-volume table of this family, the logs class scaled down:
		// resources alone are dozens per page load. Smaller batches than logs
		// because each row carries the whole event JSON, so 20k rows in a
		// buffer would be tens of megabytes rather than a few.
		MaxRows: 10000, FlushInterval: 2 * time.Second,
		BufferLimit: 100_000, MaxInFlight: 4,
	}, RumEventRow{})

	registerWriter(RumTelemetryWriter, WriterConfig{
		Name: "rum_telemetry",
		Insert: `INSERT INTO rum_telemetry
			(` + req + `,
			 date, application_id, session_id, view_id, action_id,
			 type, status, telemetry_type,
			 message, error_stack, error_kind, configuration, usage_feature,
			 service, version, source,
			 effective_sample_rate, experimental_features,
			 device_brand, device_model, device_architecture,
			 os_name, os_version, os_build,
			 event)`,
		// Low-frequency status, the check_runs class: telemetry trickles in
		// (a configuration event per session, a usage event per API call an
		// app makes rarely), so a small threshold and a longer timer.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, RumTelemetryRow{})

	registerWriter(RumTimeseriesWriter, WriterConfig{
		Name: "rum_timeseries",
		Insert: `INSERT INTO rum_timeseries
			(` + req + `,
			 date, application_id, session_id, view_id,
			 name, timeseries_id, timeseries_schema,
			 start, end, timestamps, values,
			 service, version, source, context, ddtags, event)`,
		// Same class as telemetry: mobile-only, one event per sampling run
		// rather than per sample.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, RumTimeseriesRow{})

	registerWriter(RumReplaySegmentsWriter, WriterConfig{
		Name: "rum_replay_segments",
		Insert: `INSERT INTO rum_replay_segments
			(` + req + `,
			 application_id, session_id, view_id, source,
			 variant, part_name, part_filename,
			 start, end, records_count, has_full_snapshot, index_in_view, creation_reason,
			 raw_segment_size, compressed_segment_size,
			 segment, image, event)`,
		// Whole-document payloads, the k8s_manifests class: a row is a
		// compressed recording segment, so these counts stand for megabytes.
		// Tight ceilings and one flush in flight.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, RumReplaySegmentRow{})

	registerWriter(RumSpansWriter, WriterConfig{
		Name: "rum_spans",
		Insert: `INSERT INTO rum_spans
			(` + req + `,
			 trace_id, span_id, parent_id,
			 name, service, resource, type, env,
			 start, duration_ns, error, meta, metrics, span, envelope_extra)`,
		// Steady high-frequency, the metrics class: one span per network
		// call on every instrumented mobile app.
		MaxRows: 5000, FlushInterval: 2 * time.Second,
	}, RumSpanRow{})

	registerTypes(
		WriteRumViews{}, WriteRumEvents{}, WriteRumTelemetry{},
		WriteRumTimeseries{}, WriteRumReplaySegments{}, WriteRumSpans{},
		RumRequest{},
		RumViewRow{}, RumEventRow{}, RumTelemetryRow{},
		RumTimeseriesRow{}, RumReplaySegmentRow{}, RumSpanRow{},
	)
}
