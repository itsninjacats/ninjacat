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

	"github.com/gin-gonic/gin"
	"github.com/google/pprof/profile"
)

// Three hosts fed by the trace-agent proxies and the host profiler.
//
// The trace-agent is a pure proxy on all three, so datadog-agent has no Go
// type for any of these bodies (docs §6). That does not mean they are opaque:
// the profiler's attachments are pprof, a published format with a published
// parser (github.com/google/pprof/profile), and the event parts are JSON.
// Decoded with upstream types, never with structs invented here.
//
// Every handler ends with the decoded payload set aside in a variable, one per
// wire variant, complete and unconverted. The log lines along the way may
// abbreviate (first entry, first N scopes); the variables never do.

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
		// that fails or does not exist yet must not make the bytes disappear.
		defer func() { _ = body }()
		ct := c.GetHeader("Content-Type")
		parts, err := profParts(label, ct, body)
		if err != nil {
			log.Printf("[%s] not multipart (%v), raw follows", label, err)
			describe(label, ct, body)
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

		// TODO(ninjacat): tables. Complete, unconverted, ready to take.
		_ = event    // the event part, decoded (numbers as json.Number)
		_ = profiles // form name -> *profile.Profile, one per pprof attachment
		_ = parts    // form name -> raw bytes, every part as it came in
	}
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
// ends with its own decoded value set aside; they never share a variable.
func (a *Server) HandleDebugger(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[debugger] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	ct := c.GetHeader("Content-Type")
	ddtags := c.Query("ddtags")
	mediaType, _, _ := mime.ParseMediaType(ct)
	if mediaType != "multipart/form-data" {
		// JSON array straight from the dyninst logs uploader, or whatever
		// the tracer pushed through /debugger/v2/input.
		log.Printf("[debugger] variant=json ddtags=%q", ddtags)
		logs := dbgDecodeLogs(ct, body)

		// TODO(ninjacat): tables. Complete, unconverted, ready to take.
		_ = logs   // []json.RawMessage, one entry per DI snapshot/log
		_ = ddtags // the ?ddtags= query string, as sent
		return
	}

	parts, err := profParts("debugger", ct, body)
	if err != nil {
		log.Printf("[debugger] multipart: %v", err)
		describe("debugger", ct, body)
		return
	}

	file, hasFile := parts["file"]
	event, hasEvent := parts["event"]
	switch {
	case hasFile && hasEvent:
		log.Printf("[debugger] variant=symdb ddtags=%q", ddtags)
		symdbEvent := dbgDecodeSymdbEvent(event)
		symdb, scopes := dbgDecodeSymdbFile(file)

		// TODO(ninjacat): tables. Complete, unconverted, ready to take.
		_ = symdbEvent // the event part, decoded (numbers as json.Number)
		_ = symdb      // the inflated file part: envelope key -> raw JSON, scopes included
		_ = scopes     // the envelope's scopes, decoded; all of them, not the logged 10
		_ = ddtags
	case hasEvent:
		log.Printf("[debugger] variant=diagnostics ddtags=%q", ddtags)
		diagnostics := dbgDecodeDiagnostics(event)

		// TODO(ninjacat): tables. Complete, unconverted, ready to take.
		_ = diagnostics // []map[string]any, one per DiagnosticMessage; all of them, not the logged 20
		_ = ddtags
	default:
		log.Printf("[debugger] variant=unknown parts=%s ddtags=%q",
			strings.Join(sortedKeys(parts), ","), ddtags)

		// TODO(ninjacat): tables. Complete, unconverted, ready to take.
		_ = parts // form name -> raw bytes, whatever this producer sent
		_ = ddtags
	}
}

// dbgDecodeLogs decodes a DI logs batch and returns it whole; nil when the
// body is not a JSON array. The entries are opaque json.RawMessage upstream
// too, so the log shows the count and the shape of the first one.
func dbgDecodeLogs(contentType string, body []byte) []json.RawMessage {
	var batch []json.RawMessage
	if err := jsonDecode(body, &batch); err != nil {
		log.Printf("[debugger] logs: not a JSON array (%v), raw follows", err)
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

// dbgDecodeDiagnostics decodes probe status messages and returns them whole;
// nil when the part is not a JSON array. The wire shape is
// uploader.DiagnosticMessage: service, ddsource, timestamp, and the probe
// under debugger.diagnostics (runtimeId, probeId, status, probeVersion,
// optional exception). The log stops after 20; the slice does not.
func dbgDecodeDiagnostics(data []byte) []map[string]any {
	var batch []map[string]any
	if err := jsonDecode(data, &batch); err != nil {
		log.Printf("[debugger] diagnostics: %v", err)
		return nil
	}
	log.Printf("[debugger] diagnostics: %d messages", len(batch))
	const show = 20
	for i, msg := range batch {
		if i == show {
			log.Printf("   ... and %d more", len(batch)-show)
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
	return batch
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
// scopes: [...], final} — as key -> raw JSON, plus the scopes decoded, one
// package scope per entry. The envelope is nil when the part is not gzip or
// not a JSON object; the scopes are nil when that key is missing or malformed
// while the envelope still stands. The log stops after 10 scopes; the slice
// does not.
func dbgDecodeSymdbFile(file []byte) (map[string]json.RawMessage, []map[string]any) {
	raw, err := gunzip(file)
	if err != nil {
		log.Printf("[debugger] symdb file: %v, %d B", err, len(file))
		return nil, nil
	}
	var env map[string]json.RawMessage
	if err := jsonDecode(raw, &env); err != nil {
		log.Printf("[debugger] symdb file: gzip ok (%d B -> %d B) but not a JSON object: %v",
			len(file), len(raw), err)
		return nil, nil
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
	return env, scopes
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
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	ct := c.GetHeader("Content-Type")
	parts, err := profParts("srcmap", ct, body)
	if err != nil {
		log.Printf("[srcmap] multipart: %v", err)
		describe("srcmap", ct, body)
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

	elf, hasELF := parts["elf_symbol_file"]
	if hasELF {
		log.Printf("[srcmap] elf_symbol_file %d B, %s", len(elf), elfHeader(elf))
	}

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = meta  // the event part, decoded (numbers as json.Number)
	_ = elf   // the ELF symbol file, raw bytes, unparsed; nil when absent
	_ = parts // form name -> raw bytes, every part as it came in
}

// elfHeader checks the ELF ident bytes without parsing the file: magic, then
// class (32/64-bit) and data encoding (endianness).
func elfHeader(data []byte) string {
	if len(data) < 6 || !bytes.HasPrefix(data, []byte{0x7f, 'E', 'L', 'F'}) {
		return fmt.Sprintf("not ELF, first bytes %x", data[:min(len(data), 4)])
	}
	class := map[byte]string{1: "ELF32", 2: "ELF64"}[data[4]]
	if class == "" {
		class = "ELF?"
	}
	endian := map[byte]string{1: "LE", 2: "BE"}[data[5]]
	if endian == "" {
		endian = "?"
	}
	return class + " " + endian
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
