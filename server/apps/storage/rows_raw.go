package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// raw_payloads — the fallback table, and the reference implementation of the
// per-file table pattern (registry.go).
//
// Everything one table needs lives in this file: the writer's process name,
// its config, the message, the row, and the init() that registers both. No
// shared list is edited, so two people can add two tables in parallel without
// touching the same lines.
//
// What lands here: a payload a handler could not decode, or one from an
// intake with no published schema yet. See intake/raw.go for the rule and
// schema/migrations/0002_raw_payloads.sql for the columns.

// RawPayloadsWriter is the process handlers send WriteRawPayloads to.
const RawPayloadsWriter = gen.Atom("storage_raw_payloads")

// WriteRawPayloads carries a batch of undecodable payloads. Concrete slice,
// never []Row — see the note in messages.go.
type WriteRawPayloads struct{ Payloads []RawPayloadRow }

func (m WriteRawPayloads) rows() []Row { return toRows(m.Payloads) }

// RawPayloadRow is one request kept verbatim.
//
// Body is ALREADY DECOMPRESSED — intake's Decompress middleware unwraps the
// request before any handler runs — while ContentEncoding records what the
// sender actually used. Storing the decompressed bytes is the point: whoever
// opens this row wants to read the payload, not to re-implement the codec.
type RawPayloadRow struct {
	TenantID   string
	ReceivedAt time.Time

	// Intake is the router's own label for the endpoint family ("dbm",
	// "trace", "rum"); Reason is one of the bounded values the column
	// documents ("no_schema", "decode_error", "unexpected_shape").
	Intake string
	Reason string

	Method string
	Host   string
	Path   string

	// Query is the parsed query string: values are slices because a parameter
	// may legally repeat.
	Query map[string][]string

	ContentType     string
	ContentEncoding string

	// Headers is the allowlist built in intake/raw.go. It never carries
	// Dd-Api-Key or Authorization.
	Headers map[string]string

	Body      string
	BodyBytes uint64
	Note      string
}

// AppendTo must match the INSERT column list registered below, argument for
// argument. Nothing checks that at compile time; storagetest's arity test does
// it at test time, off the zero row handed to registerWriter — so this file
// gets that test without touching any list outside it.
func (r RawPayloadRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Intake, r.Reason,
		r.Method, r.Host, r.Path, orEmpty(r.Query),
		r.ContentType, r.ContentEncoding, orEmpty(r.Headers),
		r.Body, r.BodyBytes, r.Note)
}

func init() {
	registerWriter(RawPayloadsWriter, WriterConfig{
		Name: "raw_payloads",
		Insert: `INSERT INTO raw_payloads
			(tenant_id, received_at, intake, reason, method, host, path, query,
			 content_type, content_encoding, headers, body, body_bytes, note)`,
		// Bulky documents, like k8s_manifests: a row is a whole request body,
		// so these counts stand for megabytes rather than rows. Tight ceilings
		// and a single flush in flight — this is a fallback path, never the
		// hot one, and it must not be able to starve the tables that matter.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, RawPayloadRow{})
	registerTypes(WriteRawPayloads{}, RawPayloadRow{})
}
