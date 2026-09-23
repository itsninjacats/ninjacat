package intake

import (
	"bytes"
	"compress/gzip"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"mime"
	"mime/multipart"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/pprof/profile"
	"github.com/itsninjacats/server/apps/storage"
)

// Three hosts fed by the trace-agent proxies and the host profiler.
//
// The trace-agent is a pure proxy on all three, so datadog-agent has no Go
// type for any of these bodies (docs §6). That does not mean they are opaque:
// the profiler's attachments are pprof, a published format with a published
// parser (github.com/google/pprof/profile), and the event parts are JSON.
// Decoded with upstream types, never with structs invented here.
//
// What is stored where (apps/storage/rows_profiling.go,
// schema/migrations/0012_profiling.sql, docs/tables/profiling.md):
//
//	intake.profile.<site>   /api/v2/profile, /v1/input   -> profiles
//	debugger-intake.<site>  /api/v2/debugger              -> debugger_logs (json variant)
//	                                                          debugger_diagnostics (diagnostics variant)
//	                                                          symdb_uploads (symdb variant)
//	sourcemap-intake.<site> /api/v2/srcmap                -> symbol_uploads
//
// A body that fails to decode, or a debugger multipart that is neither the
// symdb nor the diagnostics shape, goes to storeRaw instead — see the "raw"
// reason string passed at each call site below.

// intake.profile.<site> — Continuous Profiler.
//
//	config: apm_config.profiling_dd_url, internal_profiling.profile_dd_url
//
// /api/v2/profile is what the trace-agent forwards from /profiling/v1/input:
// the tracer's own multipart, passed through byte for byte. /v1/input is the
// old path, still used by the agent's internal profiling (dd-trace-go).
func (a *Server) routeProfile(g *gin.RouterGroup) {
	g.POST("/api/v2/profile", a.handleProfile("profile"))
	g.POST("/v1/input", a.handleProfile("profile-v1"))
}

func (a *Server) handleProfile(label string) gin.HandlerFunc {
	return func(c *gin.Context) {
		defer c.JSON(http.StatusAccepted, gin.H{})
		body, err := c.GetRawData()
		if err != nil {
			log.Printf("[%s] cannot read body: %v", label, err)
			return
		}
		// The raw payload outlives every path out of this handler: a decoder
		// that fails must not make the bytes disappear (storeRaw below).
		defer func() { _ = body }()
		ct := c.GetHeader("Content-Type")
		parts, err := profParts(label, ct, body)
		if err != nil {
			log.Printf("[%s] not multipart (%v), raw follows", label, err)
			describe(label, ct, body)
			a.storeRaw(c, "profiling", "decode_error",
				"not multipart, content-type "+orUnknown(ct)+": "+err.Error(), body)
			return
		}

		// The event part carries the submission: which attachments follow,
		// the interval they cover, and the tags that identify the process.
		var event map[string]any
		if ev, ok := parts["event"]; ok {
			event = profDecodeEvent(label, ev)
		}

		// Every other part is a pprof attachment. Those that parse land in
		// profiles under their form name; those that do not stay only in
		// parts, as the raw bytes they arrived as.
		profiles := make(map[string]*profile.Profile, len(parts))
		for _, name := range sortedKeys(parts) {
			if name == "event" {
				continue
			}
			if prof := profDecodeProfile(label, name, parts[name]); prof != nil {
				profiles[name] = prof
			}
		}

		tenant := TenantFromContext(c)
		if tenant == "" {
			return
		}
		row := profileRow(tenant, label, c, parts, event, profiles)
		a.store(storage.ProfilesWriter, storage.WriteProfiles{Profiles: []storage.ProfileRow{row}}, 1)
	}
}

// profileRow assembles one ninjacat.profiles row from an already-parsed
// multipart submission: event is profDecodeEvent's result (nil when the
// "event" part was absent or not a JSON object), profs is form name -> parsed
// pprof for every attachment that parsed. Every part other than "event"
// becomes one attachment entry regardless of whether it parsed.
func profileRow(tenant, label string, c *gin.Context, parts map[string][]byte,
	event map[string]any, profs map[string]*profile.Profile) storage.ProfileRow {

	row := storage.ProfileRow{
		TenantID:           tenant,
		ReceivedAt:         time.Now().UTC(),
		Variant:            label,
		DDEvpOrigin:        c.GetHeader("Dd-Evp-Origin"),
		DDEvpOriginVersion: c.GetHeader("Dd-Evp-Origin-Version"),
	}
	if raw, ok := parts["event"]; ok {
		row.Event = string(raw)
	}
	if event != nil {
		row.StartRaw = rawText(event["start"])
		row.StartParsed = profParseTimestamp(event["start"])
		row.EndRaw = rawText(event["end"])
		row.EndParsed = profParseTimestamp(event["end"])
		row.Family = asString(event["family"])
		row.Version = asString(event["version"])
		row.Runtime = asString(event["runtime"])
		row.Language = asString(event["language"])
		if tags, ok := event["tags_profiler"].(string); ok {
			row.TagsProfiler = tagsToMultiMap(splitDDTags(tags))
		}
	}

	for _, name := range sortedKeys(parts) {
		if name == "event" {
			continue
		}
		data := parts[name]
		row.AttachName = append(row.AttachName, name)
		row.AttachBytes = append(row.AttachBytes, string(data))
		row.AttachSize = append(row.AttachSize, uint64(len(data)))

		var parsedFlag uint8
		sampleTypes, sampleUnits := []string{}, []string{}
		var sampleCount uint64
		var timeNanos, durationNanos, period int64
		var periodType string
		var mappingCount, locationCount, functionCount uint32
		if prof, ok := profs[name]; ok {
			parsedFlag = 1
			sampleTypes = make([]string, 0, len(prof.SampleType))
			sampleUnits = make([]string, 0, len(prof.SampleType))
			for _, st := range prof.SampleType {
				sampleTypes = append(sampleTypes, st.Type)
				sampleUnits = append(sampleUnits, st.Unit)
			}
			sampleCount = uint64(len(prof.Sample))
			timeNanos = prof.TimeNanos
			durationNanos = prof.DurationNanos
			periodType = profPeriodUnit(prof)
			period = prof.Period
			mappingCount = uint32(len(prof.Mapping))
			locationCount = uint32(len(prof.Location))
			functionCount = uint32(len(prof.Function))
		}
		row.AttachParsed = append(row.AttachParsed, parsedFlag)
		row.AttachSampleTypes = append(row.AttachSampleTypes, sampleTypes)
		row.AttachSampleUnits = append(row.AttachSampleUnits, sampleUnits)
		row.AttachSampleCount = append(row.AttachSampleCount, sampleCount)
		row.AttachTimeNanos = append(row.AttachTimeNanos, timeNanos)
		row.AttachDurationNanos = append(row.AttachDurationNanos, durationNanos)
		row.AttachPeriodType = append(row.AttachPeriodType, periodType)
		row.AttachPeriod = append(row.AttachPeriod, period)
		row.AttachMappingCount = append(row.AttachMappingCount, mappingCount)
		row.AttachLocationCount = append(row.AttachLocationCount, locationCount)
		row.AttachFunctionCount = append(row.AttachFunctionCount, functionCount)
	}
	return row
}

// profDecodeEvent decodes the JSON event that heads every profile submission
// and returns it whole; nil when it is not a JSON object.
func profDecodeEvent(label string, data []byte) map[string]any {
	var ev map[string]any
	if err := jsonDecode(data, &ev); err != nil {
		log.Printf("[%s] event: %v", label, err)
		return nil
	}
	log.Printf("[%s] event start=%v end=%v family=%v version=%v attachments=%v",
		label, orUnset(ev["start"]), orUnset(ev["end"]), ev["family"], ev["version"], ev["attachments"])
	if tags, ok := ev["tags_profiler"].(string); ok && tags != "" {
		log.Printf("   tags %s", tags)
	}
	return ev
}

// profDecodeProfile decodes one pprof attachment and returns it whole; nil
// when the bytes are not pprof.
//
// profile.ParseData handles the gzip these arrive in. What it gives back is
// the real thing: samples, locations, functions, sample types, period. The
// log shows the per-type totals to confirm a profile is not empty.
func profDecodeProfile(label, name string, data []byte) *profile.Profile {
	prof, err := profile.ParseData(data)
	if err != nil {
		log.Printf("[%s] %s: not pprof (%v), %d B", label, name, err, len(data))
		return nil
	}

	kinds := make([]string, 0, len(prof.SampleType))
	totals := make([]int64, len(prof.SampleType))
	for i, st := range prof.SampleType {
		kinds = append(kinds, st.Type+"/"+st.Unit)
		for _, smp := range prof.Sample {
			if i < len(smp.Value) {
				totals[i] += smp.Value[i]
			}
		}
	}
	log.Printf("[%s] %s pprof: %d samples, %d locations, %d functions, period=%d %s",
		label, name, len(prof.Sample), len(prof.Location), len(prof.Function),
		prof.Period, profPeriodUnit(prof))
	for i, k := range kinds {
		log.Printf("   %s total=%d", k, totals[i])
	}
	return prof
}

func profPeriodUnit(p *profile.Profile) string {
	if p.PeriodType == nil {
		return ""
	}
	return p.PeriodType.Type + "/" + p.PeriodType.Unit
}

// profParts reads a multipart body into form name -> bytes, logging one line
// per part (name, filename, content type, size) in wire order as it goes.
// The caller decodes the parts it knows.
//
// The boundary comes from the Content-Type header; without it there is
// nothing to parse. A repeated form name keeps the last part and says so;
// none of the producers here send duplicates, so the warning is the alarm.
func profParts(label, contentType string, body []byte) (map[string][]byte, error) {
	mediaType, params, err := mime.ParseMediaType(contentType)
	if err != nil {
		return nil, err
	}
	if !strings.HasPrefix(mediaType, "multipart/") {
		return nil, &profNotMultipartError{mediaType: mediaType}
	}
	boundary := params["boundary"]
	if boundary == "" {
		return nil, &profNotMultipartError{mediaType: "multipart without boundary"}
	}

	log.Printf("[%s] %s %d B", label, mediaType, len(body))
	out := make(map[string][]byte)
	mr := multipart.NewReader(bytes.NewReader(body), boundary)
	for {
		p, err := mr.NextPart()
		if err == io.EOF {
			break
		}
		if err != nil {
			return out, err
		}
		data, err := io.ReadAll(p)
		if err != nil {
			return out, err
		}
		log.Printf("   part name=%q filename=%q type=%s %d B",
			p.FormName(), p.FileName(), orUnknown(p.Header.Get("Content-Type")), len(data))
		if prev, dup := out[p.FormName()]; dup {
			log.Printf("   part name=%q repeats, replacing the earlier %d B", p.FormName(), len(prev))
		}
		out[p.FormName()] = data
	}
	return out, nil
}

// debugger-intake.<site> — Dynamic Instrumentation and symbol database.
//
//	config: apm_config.debugger_diagnostics_dd_url, apm_config.symdb_dd_url
//
// One path, three producers, told apart by Content-Type and part names:
//
//	application/json                  []json.RawMessage — DI snapshots/logs
//	                                  (dyninst logSender, DEBUGGER track)
//	multipart: event only             []uploader.DiagnosticMessage —
//	                                  diagnostics (event.json)
//	multipart: file + event           symdb — file.gz (gzip JSON), then event
func (a *Server) routeDebugger(g *gin.RouterGroup) {
	g.POST("/api/v2/debugger", a.HandleDebugger)
}

// HandleDebugger accepts diagnostics, DI logs and symdb uploads. Each variant
// ends with its own rows going to its own table; they never share a writer.
func (a *Server) HandleDebugger(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[debugger] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails must not make the bytes disappear (storeRaw below).
	defer func() { _ = body }()

	ct := c.GetHeader("Content-Type")
	ddtags := c.Query("ddtags")
	mediaType, _, _ := mime.ParseMediaType(ct)
	if mediaType != "multipart/form-data" {
		// JSON array or NDJSON straight from the dyninst logs uploader, or
		// whatever the tracer pushed through /debugger/v2/input.
		log.Printf("[debugger] variant=json ddtags=%q", ddtags)
		raw := dbgDecodeLogs(ct, body)
		if raw == nil {
			a.storeRaw(c, "debugger", "decode_error",
				"logs variant: not a JSON array or NDJSON, content-type "+orUnknown(ct), body)
			return
		}

		tenant := TenantFromContext(c)
		if tenant == "" || len(raw) == 0 {
			return
		}
		tags := tagsToMultiMap(splitDDTags(ddtags))
		now := time.Now().UTC()
		rows := make([]storage.DebuggerLogRow, 0, len(raw))
		for _, entry := range raw {
			var m map[string]any
			_ = jsonDecode(entry, &m) // best-effort: on failure m stays nil, Entry still keeps the raw bytes
			rows = append(rows, storage.DebuggerLogRow{
				TenantID:   tenant,
				ReceivedAt: now,
				Service:    asString(m["service"]),
				DDSource:   asString(m["ddsource"]),
				DDTags:     tags,
				Entry:      string(entry),
				ExtraKeys:  extraKeysExcept(m, "service", "ddsource"),
			})
		}
		a.store(storage.DebuggerLogsWriter, storage.WriteDebuggerLogs{Logs: rows}, len(rows))
		return
	}

	parts, err := profParts("debugger", ct, body)
	if err != nil {
		log.Printf("[debugger] multipart: %v", err)
		describe("debugger", ct, body)
		a.storeRaw(c, "debugger", "decode_error",
			"multipart, content-type "+orUnknown(ct)+": "+err.Error(), body)
		return
	}

	file, hasFile := parts["file"]
	event, hasEvent := parts["event"]
	switch {
	case hasFile && hasEvent:
		log.Printf("[debugger] variant=symdb ddtags=%q", ddtags)
		symdbEvent := dbgDecodeSymdbEvent(event)
		envelope, scopes, inflatedSize := dbgDecodeSymdbFile(file)

		tenant := TenantFromContext(c)
		if tenant == "" {
			return
		}
		row := symdbUploadRow(tenant, event, file, ddtags, symdbEvent, envelope, scopes, inflatedSize)
		a.store(storage.SymdbUploadsWriter, storage.WriteSymdbUploads{Uploads: []storage.SymdbUploadRow{row}}, 1)
	case hasEvent:
		log.Printf("[debugger] variant=diagnostics ddtags=%q", ddtags)
		raw, diagnostics := dbgDecodeDiagnostics(event)
		if raw == nil {
			// dbgDecodeDiagnostics returns (nil, nil) only on a genuine decode
			// failure (the event part is not a JSON array) — distinct from a
			// legitimately empty "[]" batch, which decodes to a non-nil empty
			// slice and falls through to the ordinary empty-batch return below.
			a.storeRaw(c, "debugger", "decode_error",
				"diagnostics variant: event part is not a JSON array", body)
			return
		}

		tenant := TenantFromContext(c)
		if tenant == "" || len(raw) == 0 {
			return
		}
		tags := tagsToMultiMap(splitDDTags(ddtags))
		now := time.Now().UTC()
		rows := make([]storage.DebuggerDiagnosticRow, 0, len(raw))
		for i, r := range raw {
			rows = append(rows, debuggerDiagnosticRow(tenant, now, tags, r, diagnostics[i]))
		}
		a.store(storage.DebuggerDiagnosticsWriter, storage.WriteDebuggerDiagnostics{Diagnostics: rows}, len(rows))
	default:
		log.Printf("[debugger] variant=unknown parts=%s ddtags=%q",
			strings.Join(sortedKeys(parts), ","), ddtags)
		a.storeRaw(c, "debugger", "unexpected_shape",
			"multipart with neither file nor event parts: "+strings.Join(sortedKeys(parts), ","), body)
	}
}

// dbgDecodeLogs decodes a DI logs batch and returns every entry, in order;
// nil when the body is neither a JSON array nor NDJSON (or empty).
//
// The dyninst logSender/tracer sends a JSON array; the browser SDK's debugger
// track sends NDJSON instead (rum-spec.md's browser note) — sniffed off the
// first non-whitespace byte rather than assumed, since both producers can
// reach this same endpoint.
func dbgDecodeLogs(contentType string, body []byte) []json.RawMessage {
	trimmed := bytes.TrimLeft(body, " \t\r\n")
	if len(trimmed) == 0 {
		log.Printf("[debugger] logs: empty body")
		return nil
	}

	var batch []json.RawMessage
	switch trimmed[0] {
	case '[':
		if err := jsonDecode(body, &batch); err != nil {
			log.Printf("[debugger] logs: not a JSON array (%v), raw follows", err)
			describe("debugger", contentType, body)
			return nil
		}
	case '{':
		for _, line := range bytes.Split(trimmed, []byte("\n")) {
			line = bytes.TrimSpace(line)
			if len(line) == 0 {
				continue
			}
			// Copied out of trimmed's backing array, which this function
			// does not own past its return.
			entry := make(json.RawMessage, len(line))
			copy(entry, line)
			batch = append(batch, entry)
		}
	default:
		log.Printf("[debugger] logs: not JSON (first byte %q), raw follows", trimmed[0])
		describe("debugger", contentType, body)
		return nil
	}

	log.Printf("[debugger] logs: %d entries, %d B", len(batch), len(body))
	if len(batch) == 0 {
		return batch
	}
	var first map[string]any
	if err := jsonDecode(batch[0], &first); err != nil {
		log.Printf("   first entry: %v", err)
		return batch
	}
	log.Printf("   first entry service=%v ddsource=%v keys: %s",
		first["service"], first["ddsource"], strings.Join(sortedKeys(first), ", "))
	return batch
}

// dbgDecodeDiagnostics decodes probe status messages and returns each one
// both as its raw bytes and as a best-effort decode; both slices are nil
// together when the body is not a JSON array. The wire shape is
// uploader.DiagnosticMessage: service, ddsource, timestamp, and the probe
// under debugger.diagnostics (runtimeId, probeId, status, probeVersion,
// optional exception). An entry that itself fails to decode gets a nil
// decoded[i] rather than dropping the whole batch — raw[i] still carries its
// bytes. The log stops after 20; the slices do not.
func dbgDecodeDiagnostics(data []byte) ([]json.RawMessage, []map[string]any) {
	var raw []json.RawMessage
	if err := jsonDecode(data, &raw); err != nil {
		log.Printf("[debugger] diagnostics: %v", err)
		return nil, nil
	}
	decoded := make([]map[string]any, len(raw))
	for i, r := range raw {
		var m map[string]any
		if err := jsonDecode(r, &m); err != nil {
			log.Printf("[debugger] diagnostics: entry %d: %v", i, err)
			continue
		}
		decoded[i] = m
	}

	log.Printf("[debugger] diagnostics: %d messages", len(raw))
	const show = 20
	for i, msg := range decoded {
		if i == show {
			log.Printf("   ... and %d more", len(decoded)-show)
			break
		}
		diag, _ := nested(msg, "debugger", "diagnostics").(map[string]any)
		log.Printf("   service=%v runtimeId=%v probeId=%v status=%v probeVersion=%v ts=%v",
			msg["service"], diag["runtimeId"], diag["probeId"], diag["status"],
			diag["probeVersion"], orUnset(msg["timestamp"]))
		if exc, ok := diag["exception"].(map[string]any); ok {
			log.Printf("      exception %v: %v", exc["type"], exc["message"])
		}
	}
	return raw, decoded
}

// debuggerDiagnosticRow converts one decoded DiagnosticMessage into its row.
// msg may be nil when that one entry failed to decode (dbgDecodeDiagnostics
// keeps going past a single bad entry) — every extracted column is then
// empty/NULL, but raw (and so Message) still carries the original bytes.
func debuggerDiagnosticRow(tenant string, arrival time.Time, ddtags map[string][]string, raw json.RawMessage, msg map[string]any) storage.DebuggerDiagnosticRow {
	diag, _ := nested(msg, "debugger", "diagnostics").(map[string]any)
	row := storage.DebuggerDiagnosticRow{
		TenantID:     tenant,
		ReceivedAt:   arrival,
		Timestamp:    profParseTimestamp(msg["timestamp"]),
		Service:      asString(msg["service"]),
		DDSource:     asString(msg["ddsource"]),
		DDTags:       ddtags,
		RuntimeID:    asString(diag["runtimeId"]),
		ProbeID:      asString(diag["probeId"]),
		Status:       asString(diag["status"]),
		ProbeVersion: asString(diag["probeVersion"]),
		Message:      string(raw),
	}
	if exc, ok := diag["exception"].(map[string]any); ok {
		t := asString(exc["type"])
		m := asString(exc["message"])
		row.ExceptionType = &t
		row.ExceptionMessage = &m
	}
	return row
}

// dbgDecodeSymdbEvent decodes the event part of a symdb upload — the metadata
// that lets the backend do its bookkeeping — and returns it whole; nil when
// it is not a JSON object.
func dbgDecodeSymdbEvent(event []byte) map[string]any {
	var ev map[string]any
	if err := jsonDecode(event, &ev); err != nil {
		log.Printf("[debugger] symdb event: %v", err)
		return nil
	}
	log.Printf("[debugger] symdb event service=%v version=%v language=%v runtimeId=%v uploadId=%v batch=%v final=%v attachmentSize=%v",
		ev["service"], ev["version"], ev["language"], ev["runtimeId"],
		ev["uploadId"], ev["batchNum"], ev["final"], ev["attachmentSize"])
	return ev
}

// dbgDecodeSymdbFile inflates the file part of a symdb upload and returns the
// JSON envelope — {service, version, language, upload_id, batch_num,
// scopes: [...], final} — as key -> raw JSON, the scopes decoded (one package
// scope per entry), and the inflated size. The envelope is nil when the part
// is not gzip or not a JSON object; the scopes are nil when that key is
// missing or malformed while the envelope still stands. The log stops after
// 10 scopes; the slice does not.
func dbgDecodeSymdbFile(file []byte) (map[string]json.RawMessage, []map[string]any, int) {
	raw, err := gunzip(file)
	if err != nil {
		log.Printf("[debugger] symdb file: %v, %d B", err, len(file))
		return nil, nil, 0
	}
	var env map[string]json.RawMessage
	if err := jsonDecode(raw, &env); err != nil {
		log.Printf("[debugger] symdb file: gzip ok (%d B -> %d B) but not a JSON object: %v",
			len(file), len(raw), err)
		return nil, nil, len(raw)
	}
	var scopes []map[string]any
	if rawScopes, ok := env["scopes"]; !ok {
		log.Printf("[debugger] symdb file: no scopes key")
	} else if err := jsonDecode(rawScopes, &scopes); err != nil {
		log.Printf("[debugger] symdb file: scopes: %v", err)
		scopes = nil
	}
	log.Printf("[debugger] symdb file: gzip %d B -> %d B, service=%s version=%s language=%s upload_id=%s batch_num=%s final=%s, %d scopes, keys: %s",
		len(file), len(raw), env["service"], env["version"], env["language"],
		env["upload_id"], env["batch_num"], env["final"], len(scopes),
		strings.Join(sortedKeys(env), ", "))

	const show = 10
	for i, sc := range scopes {
		if i == show {
			log.Printf("   ... and %d more", len(scopes)-show)
			break
		}
		children, _ := sc["scopes"].([]any)
		log.Printf("   %v %v: %d nested scopes", sc["scope_type"], sc["name"], len(children))
	}
	return env, scopes, len(raw)
}

// symdbUploadRow assembles one ninjacat.symdb_uploads row. rawEvent is the
// event part's bytes exactly as received — kept whole regardless of whether
// decoded parses it, the same lossless-copy contract every sibling table in
// this file follows. decoded is dbgDecodeSymdbEvent's result (nil on decode
// failure); envelope/scopes are dbgDecodeSymdbFile's — independently
// nilable, see that function's doc comment.
func symdbUploadRow(tenant string, rawEvent, file []byte, ddtags string, decoded map[string]any,
	envelope map[string]json.RawMessage, scopes []map[string]any, inflatedSize int) storage.SymdbUploadRow {

	row := storage.SymdbUploadRow{
		TenantID:     tenant,
		ReceivedAt:   time.Now().UTC(),
		Event:        string(rawEvent),
		DDTags:       tagsToMultiMap(splitDDTags(ddtags)),
		File:         string(file),
		InflatedSize: uint64(inflatedSize),
		ScopeCount:   uint32(len(scopes)),
	}
	if scopes != nil {
		row.ScopesOK = 1
	}
	if decoded != nil {
		row.Service = asString(decoded["service"])
		row.Version = asString(decoded["version"])
		row.Language = asString(decoded["language"])
		row.RuntimeID = asString(decoded["runtimeId"])
		// UploadID and BatchNum both travel as the wire's literal text
		// (rawText, not asString) — a producer that sends uploadId as a bare
		// JSON number must not lose it to asString's string-only check, and
		// both fields need the exact same handling since they are the same
		// kind of identifier.
		row.UploadID = rawText(decoded["uploadId"])
		row.BatchNum = rawText(decoded["batchNum"])
		row.Final = asBool(decoded["final"])
		row.AttachmentSize = asUint64(decoded["attachmentSize"])
	}
	if envelope != nil {
		row.EnvService = rawJSONText(envelope, "service")
		row.EnvVersion = rawJSONText(envelope, "version")
		row.EnvLanguage = rawJSONText(envelope, "language")
		row.EnvUploadID = rawJSONText(envelope, "upload_id")
		row.EnvBatchNum = rawJSONText(envelope, "batch_num")
		row.EnvFinal = rawJSONText(envelope, "final")
	}
	return row
}

// symdbMaxInflated bounds what we are willing to inflate from one batch. The
// encoder flushes at ~2 MiB compressed; symbol JSON compresses well, but not
// this well.
const symdbMaxInflated = 256 << 20

// gunzip inflates data whole. A stream that would exceed symdbMaxInflated is
// an error, never a silently truncated result.
func gunzip(data []byte) ([]byte, error) {
	zr, err := gzip.NewReader(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	defer zr.Close()
	out, err := io.ReadAll(io.LimitReader(zr, symdbMaxInflated+1))
	if err != nil {
		return nil, err
	}
	if len(out) > symdbMaxInflated {
		return nil, fmt.Errorf("inflates past %d B, refusing to truncate", symdbMaxInflated)
	}
	return out, nil
}

// sourcemap-intake.<site> — native symbol upload from the host profiler.
//
//	config: profiling::symbol_uploader::symbol_endpoints[].site
//
// Multipart with elf_symbol_file first, then event (JSON metadata: arch,
// build ids, file_hash, symbol_source...). Usually zstd-compressed as a
// whole — Content-Encoding, handled by Decompress(). The agent only sends
// this after /api/v2/profiles/symbols/query (router_api.go's business)
// told it the symbols are missing.
func (a *Server) routeSourcemap(g *gin.RouterGroup) {
	g.POST("/api/v2/srcmap", a.HandleSourcemap)
}

// HandleSourcemap accepts ELF symbol files with their metadata event.
func (a *Server) HandleSourcemap(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[srcmap] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails must not make the bytes disappear (storeRaw below).
	defer func() { _ = body }()
	ct := c.GetHeader("Content-Type")
	parts, err := profParts("srcmap", ct, body)
	if err != nil {
		log.Printf("[srcmap] multipart: %v", err)
		describe("srcmap", ct, body)
		a.storeRaw(c, "srcmap", "decode_error",
			"multipart, content-type "+orUnknown(ct)+": "+err.Error(), body)
		return
	}

	// symbolUploadRequestMetadata from comp/host-profiler/symboluploader.
	var meta map[string]any
	if ev, ok := parts["event"]; ok {
		if err := jsonDecode(ev, &meta); err != nil {
			log.Printf("[srcmap] event: %v", err)
			meta = nil
		} else {
			log.Printf("[srcmap] event type=%v arch=%v gnu_build_id=%v go_build_id=%v file_hash=%v symbol_source=%v origin=%v/%v filename=%v",
				meta["type"], meta["arch"], meta["gnu_build_id"], meta["go_build_id"],
				meta["file_hash"], meta["symbol_source"], meta["origin"],
				meta["origin_version"], meta["filename"])
		}
	}

	if elf, ok := parts["elf_symbol_file"]; ok {
		log.Printf("[srcmap] elf_symbol_file %d B, %s", len(elf), elfHeader(elf))
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	row := symbolUploadRow(tenant, parts, meta)
	a.store(storage.SymbolUploadsWriter, storage.WriteSymbolUploads{Uploads: []storage.SymbolUploadRow{row}}, 1)
}

// symbolUploadRow assembles one ninjacat.symbol_uploads row. meta is the
// event part's decode (nil when absent or malformed). OtherParts carries
// every multipart part besides "event" and "elf_symbol_file" verbatim, so a
// producer sending an extra part is still visible even though this handler
// does not name it.
func symbolUploadRow(tenant string, parts map[string][]byte, meta map[string]any) storage.SymbolUploadRow {
	elf, hasELF := parts["elf_symbol_file"]
	row := storage.SymbolUploadRow{
		TenantID:   tenant,
		ReceivedAt: time.Now().UTC(),
		Meta:       string(parts["event"]),
		HasELF:     boolToUint8(hasELF),
		ELF:        string(elf),
		ELFSize:    uint64(len(elf)),
	}
	if hasELF {
		row.ELFClass, row.ELFEndianness = elfInfo(elf)
	}
	if meta != nil {
		row.Type = asString(meta["type"])
		row.Arch = asString(meta["arch"])
		row.GNUBuildID = asString(meta["gnu_build_id"])
		row.GoBuildID = asString(meta["go_build_id"])
		row.FileHash = asString(meta["file_hash"])
		row.SymbolSource = asString(meta["symbol_source"])
		row.Origin = asString(meta["origin"])
		row.OriginVersion = asString(meta["origin_version"])
		row.Filename = asString(meta["filename"])
	}

	other := make(map[string]string, len(parts))
	for name, data := range parts {
		if name == "event" || name == "elf_symbol_file" {
			continue
		}
		other[name] = string(data)
	}
	row.OtherParts = other
	return row
}

// elfInfo sniffs the ELF ident bytes without parsing the file: magic, then
// class (32/64-bit) and data encoding (endianness). Both results are "" when
// the bytes are not ELF at all — the caller already knows that from hasELF/
// HasELF, so this never invents a value to fill the column with.
func elfInfo(data []byte) (class, endianness string) {
	if len(data) < 6 || !bytes.HasPrefix(data, []byte{0x7f, 'E', 'L', 'F'}) {
		return "", ""
	}
	class = map[byte]string{1: "ELF32", 2: "ELF64"}[data[4]]
	if class == "" {
		class = "ELF?"
	}
	endianness = map[byte]string{1: "LE", 2: "BE"}[data[5]]
	if endianness == "" {
		endianness = "?"
	}
	return class, endianness
}

// elfHeader is elfInfo rendered for a log line.
func elfHeader(data []byte) string {
	if len(data) < 6 || !bytes.HasPrefix(data, []byte{0x7f, 'E', 'L', 'F'}) {
		return fmt.Sprintf("not ELF, first bytes %x", data[:min(len(data), 4)])
	}
	class, endianness := elfInfo(data)
	return class + " " + endianness
}

// jsonDecode unmarshals one JSON value into v with numbers kept as
// json.Number, so 64-bit integers (timestamps, ids, sizes) never round-trip
// through float64. Trailing data after the value is an error, as it is for
// json.Unmarshal.
func jsonDecode(data []byte, v any) error {
	dec := json.NewDecoder(bytes.NewReader(data))
	dec.UseNumber()
	if err := dec.Decode(v); err != nil {
		return err
	}
	if _, err := dec.Token(); err != io.EOF {
		return fmt.Errorf("trailing data after JSON value")
	}
	return nil
}

// orUnset renders an optional decoded field for a log line: "unset" when the
// key was absent or null, so a missing timestamp never reads as a zero.
func orUnset(v any) any {
	if v == nil {
		return "unset"
	}
	return v
}

// rawText renders a decoded JSON value as text, verbatim: a json.Number
// prints as the exact digits it decoded from (json.Number.String is the
// original text, so this never rounds through float64), a string prints
// unquoted, anything else uses Go's %v. "" for a nil/absent value — the
// caller pairs this with a Nullable(*) parse of the same value, so nothing is
// lost by using "" here rather than a sentinel.
func rawText(v any) string {
	if v == nil {
		return ""
	}
	if s, ok := v.(string); ok {
		return s
	}
	return fmt.Sprintf("%v", v)
}

// asString returns v as a string when it decoded as one, "" otherwise — for
// reading a field off a map[string]any built by jsonDecode, where the caller
// cannot assume the producer sent the type it expects.
func asString(v any) string {
	s, _ := v.(string)
	return s
}

// asBool reports a JSON boolean as a Nullable(UInt8): 1/0 when the key
// decoded as a real bool, nil when it was absent or some other type — so
// "not sent" and "sent false" stay distinguishable in the Nullable column.
func asBool(v any) *uint8 {
	b, ok := v.(bool)
	if !ok {
		return nil
	}
	var n uint8
	if b {
		n = 1
	}
	return &n
}

// asUint64 reads a json.Number as a Nullable(UInt64) — nil when the key was
// absent, not a number, or negative, which are the only ways a byte count
// could legitimately fail to be "not reported".
func asUint64(v any) *uint64 {
	n, ok := v.(json.Number)
	if !ok {
		return nil
	}
	i, err := n.Int64()
	if err != nil || i < 0 {
		return nil
	}
	u := uint64(i)
	return &u
}

// profParseTimestamp reads a profiler event's start/end, or a debugger
// diagnostic's timestamp, off the wire. Datadog's profiler sends these as
// RFC3339 strings; some producers send epoch numbers instead, in seconds,
// milliseconds, microseconds or nanoseconds depending on the client — the
// magnitude bands below are the same heuristic a human reading the raw number
// would use. nil means "could not tell", never a guessed time: the caller
// always keeps the original value fully readable next to this parse (a *_raw
// column, or the row's own raw JSON), so a nil here is a missed convenience
// column, never a lost fact.
func profParseTimestamp(v any) *time.Time {
	switch val := v.(type) {
	case string:
		for _, layout := range []string{time.RFC3339Nano, time.RFC3339} {
			if t, err := time.Parse(layout, val); err == nil {
				t = t.UTC()
				return &t
			}
		}
		return nil
	case json.Number:
		i, err := val.Int64()
		if err != nil || i <= 0 {
			return nil
		}
		var t time.Time
		switch {
		case i >= 1e18:
			t = time.Unix(0, i) // nanoseconds
		case i >= 1e15:
			t = time.UnixMicro(i)
		case i >= 1e12:
			t = time.UnixMilli(i)
		default:
			t = time.Unix(i, 0) // seconds
		}
		t = t.UTC()
		return &t
	default:
		return nil
	}
}

// rawJSONText renders one key of a decoded JSON object as plain text: a JSON
// string unquotes to its bare value, anything else (number, bool, null,
// nested object/array) keeps its literal JSON form. Used for the symdb file
// envelope, whose fields travel as json.RawMessage because the whole object
// is read generically (dbgDecodeSymdbFile) — "" when the key is absent.
func rawJSONText(m map[string]json.RawMessage, key string) string {
	raw, ok := m[key]
	if !ok {
		return ""
	}
	var s string
	if err := json.Unmarshal(raw, &s); err == nil {
		return s
	}
	return string(raw)
}

// extraKeysExcept returns m's keys, sorted, minus the ones the caller already
// reads into their own columns — the same "record the names, not the values"
// pattern K8sActionRow.ExtraKeys uses for a payload with no fixed schema of
// its own.
func extraKeysExcept(m map[string]any, exclude ...string) []string {
	if len(m) == 0 {
		return nil
	}
	skip := make(map[string]bool, len(exclude))
	for _, k := range exclude {
		skip[k] = true
	}
	keys := make([]string, 0, len(m))
	for k := range m {
		if !skip[k] {
			keys = append(keys, k)
		}
	}
	sort.Strings(keys)
	return keys
}

// nested walks a decoded JSON object by key path, returning nil if any step
// is missing or not an object.
func nested(m map[string]any, path ...string) any {
	var cur any = m
	for _, k := range path {
		obj, ok := cur.(map[string]any)
		if !ok {
			return nil
		}
		cur = obj[k]
	}
	return cur
}

func sortedKeys[V any](m map[string]V) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

type profNotMultipartError struct{ mediaType string }

func (e *profNotMultipartError) Error() string {
	return "content-type is " + orUnknown(e.mediaType)
}
