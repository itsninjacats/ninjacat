package intake

import (
	"bytes"
	"log"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// rawHeaders is the allowlist of request headers kept with a raw payload.
//
// An allowlist, not a blocklist. A blocklist is one forgotten header away from
// writing Dd-Api-Key into a table with a 30-day TTL, and credentials are
// exactly what a debugging dump attracts. These names identify the sender and
// let a request be correlated with an agent log; nothing here is a secret.
//
// Written in the canonical form net/http stores them in, which is what
// c.GetHeader looks up — DD-EVP-ORIGIN on the wire and Dd-Evp-Origin here are
// the same header.
var rawHeaders = []string{
	"Via",
	"User-Agent",
	"Dd-Evp-Origin",
	"Dd-Evp-Origin-Version",
	"Dd-Request-Id",
	"Dd-Agent-Hostname",
	"Dd-Agent-Env",
	"Datadog-Container-Id",
	"X-Datadog-Additional-Tags",
	"X-Datadog-Container-Tags",
	"X-Requested-With",
	"X-Dd-Hostname",
	"X-Dd-Processagentversion",
	"X-Dd-Request-Id",
}

// storeRaw keeps a request we could not turn into rows.
//
// THE RULE, and it is a project rule rather than a suggestion: a handler that
// cannot decode a body, or that receives a shape with no published schema,
// MUST hand the bytes here. Decode SUCCESS paths never do — this is a
// fallback, not a mirror of the traffic, and duplicating what already became
// rows would cost the volume of the whole intake for nothing.
//
// The reason is arithmetic. About forty Datadog intakes still have no decoder
// on our side, and the payload that would tell us what one of them sends
// arrives once, from somebody's real agent. Logging "unhandled" and dropping
// it means reverse-engineering that format needs a lab reproduction; keeping
// it means a SELECT.
//
//	intake  the router's label for the endpoint family ("dbm", "trace", "rum")
//	reason  "no_schema" | "decode_error" | "unexpected_shape"
//	note    free text — the decoder error, the field that was missing
//	body    the body the handler saw
//
// NOTE ON body: it is already DECOMPRESSED. The Decompress middleware
// (body.go) unwraps zstd/gzip/deflate before any handler runs, so what is
// stored is readable text or protobuf, while the content_encoding column
// records what the sender used. Pass the same bytes the handler tried to
// decode — re-compressing them here would defeat the point.
//
// A request with no tenant is dropped, not stored under a guess: tenant_id is
// the first ORDER BY column of every table and inventing a value would put
// rows somewhere no query looks. In practice this cannot happen behind
// RequireAPIKey, so it is logged as the configuration fault it would be.
func (a *Server) storeRaw(c *gin.Context, intake, reason, note string, body []byte) {
	tenant := TenantFromContext(c)
	if tenant == "" {
		log.Printf("[raw] %s %s: no tenant in context, payload dropped (%d B)",
			intake, c.Request.URL.Path, len(body))
		return
	}
	if rawIsProbe(body) {
		// The agent sweeps every intake at startup with an empty object or
		// an empty body and no X-Requested-With header (seen from a real
		// 7.83 agent on contlcycle, contimage, sbom, genresources, ndmtraps,
		// data_streams_messages and logs). Those bytes carry nothing, so a
		// row per probe per boot would only bury the payloads this table
		// exists for.
		log.Printf("[raw] %s %s: empty probe body, not stored", intake, c.Request.URL.Path)
		return
	}

	headers := make(map[string]string, len(rawHeaders))
	for _, h := range rawHeaders {
		if v := c.GetHeader(h); v != "" {
			headers[h] = v
		}
	}

	row := storage.RawPayloadRow{
		TenantID:   tenant,
		ReceivedAt: time.Now().UTC(),
		Intake:     intake,
		Reason:     reason,
		Method:     c.Request.Method,
		// The Host header is the dispatch key of this whole server
		// (routes.go), so a raw payload without it is missing the field that
		// says which product sent it.
		Host:            c.Request.Host,
		Path:            c.Request.URL.Path,
		Query:           c.Request.URL.Query(),
		ContentType:     c.GetHeader("Content-Type"),
		ContentEncoding: c.GetHeader("Content-Encoding"),
		Headers:         headers,
		Body:            string(body),
		BodyBytes:       uint64(len(body)),
		Note:            note,
	}

	a.store(storage.RawPayloadsWriter, storage.WriteRawPayloads{Payloads: []storage.RawPayloadRow{row}}, 1)
}

// rawIsProbe reports whether a body is one of the agent's connectivity
// probes: nothing, or a bare empty JSON object or array. Anything else, even
// a single byte of garbage, is worth keeping.
func rawIsProbe(body []byte) bool {
	switch string(bytes.TrimSpace(body)) {
	case "", "{}", "[]":
		return true
	}
	return false
}
