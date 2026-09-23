package intake

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"mime"
	"mime/multipart"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/rumevents"
)

// browser-intake.<site> — Real User Monitoring, from every SDK.
//
//	config: browser SDK `proxy`, dd-sdk-ios `customEndpoint`,
//	        dd-sdk-android `useCustomEndpoint` — see docs/rum-sdk-setup.md
//
// ONE HOST for browser, iOS and Android, because Datadog itself puts all
// three on browser-intake-<site>: there is no mobile-intake. Mirroring that
// is what lets a user redirect an SDK by changing a host and nothing else.
// The platforms differ in how they authenticate and how they signal
// compression, not in where they send, so those differences live in the engine
// and the gate below rather than in three hosts.
//
//	POST /api/v2/rum       NDJSON of RUM events  -> rum_views, rum_events,
//	                                                rum_telemetry, rum_timeseries
//	POST /api/v2/logs      NDJSON or JSON array  -> logs (router_logs.go)
//	POST /api/v2/replay    multipart             -> rum_replay_segments
//	POST /api/v2/spans     NDJSON of envelopes   -> rum_spans
//	POST /api/v2/profile   multipart             -> (router_profiling.go)
//	POST /api/v2/debugger  NDJSON or multipart   -> (router_profiling.go)
//
// Anything that does not decode goes to raw_payloads under the "rum" label,
// per intake/raw.go — a batch that arrives once, from somebody's real app, is
// not something a log line can replace.

// rumEngine is the handler browser-intake.<site> is served by.
//
// It is an http.Handler around the gin engine rather than the engine itself,
// and both things it does above the router have to happen there:
//
//   - the browser SDK's `proxy` string form sends the real path inside
//     ?ddforward=, and gin picks a route before any middleware of ours runs,
//     so rewriting the URL inside the engine would rewrite nothing;
//
//   - CORS has to be on EVERY response this host produces, and gin runs a
//     route group's middleware only for a request that MATCHED a route in it.
//     A path this build does not serve — a track a newer SDK added, a
//     misconfigured client — falls through to NoRoute, whose handler chain
//     does not include the group. Setting the headers inside the group would
//     therefore leave them off exactly the 404s that need them most: to the
//     browser SDK a cross-origin response it may not read is status 0 while
//     online, which it counts as SUCCESS and drops the batch for. The
//     unhandled endpoint would be invisible on both sides.
//
// The preflight is answered here for the same reason, rather than by OPTIONS
// routes: a preflight for a path we do not serve must still be answered, so
// the browser gets as far as the POST and sees the 404 it can act on.
func (a *Server) rumEngine() http.Handler {
	origins := rumAllowedOrigins()
	e := a.engineAuth(a.rumGate(), a.routeRUM)

	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		rumUnwrapForward(r)
		rumSetCORS(w.Header(), r.Header.Get("Origin"), origins)

		// Preflight carries no credential by definition — the browser strips
		// them — so it is answered before anything can ask for one.
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}

		e.ServeHTTP(w, r)
	})
}

func (a *Server) routeRUM(g *gin.RouterGroup) {
	g.POST("/api/v2/rum", a.HandleRUM)
	// Logs from an SDK are the same wire format the agent sends, so they go
	// to the same handler; only the framing differs and parseLogs sniffs it.
	g.POST("/api/v2/logs", a.HandleLogs)
	g.POST("/api/v2/replay", a.HandleRUMReplay)
	g.POST("/api/v2/spans", a.HandleRUMSpans)
	// The browser profiler's multipart is the same envelope the tracers send,
	// so it reuses that handler under its own label.
	g.POST("/api/v2/profile", a.handleProfile("profile-browser"))
	g.POST("/api/v2/debugger", a.HandleDebugger)

	// No OPTIONS routes: rumEngine answers every preflight above the router,
	// including for paths that are not in this list.
}

// ---------------------------------------------------------------------------
// CORS and authentication
// ---------------------------------------------------------------------------

// rumGate is the key check for this host. CORS is NOT here — rumEngine sets
// it above the router, and the comment there says why — but the ORDER it
// creates is the whole point of this split.
//
// The browser SDK retries only on 408, 429 and 5xx, or on a network error
// while navigator.onLine is false. A CORS failure reaches it as status 0 while
// online, which it treats as SUCCESS — it drops the batch and never tells
// anyone. So a 403 without an Access-Control-Allow-Origin header does not read
// as "bad key" in the browser; it reads as nothing at all, and the operator
// sees an app that sends no data and a server with no errors in it. The header
// being on the response before the key is even looked at is what makes a
// misconfigured key visible.
//
// Authentication accepts the header (iOS, Android) and the query parameter
// (browser). The browser has no choice: its fetch sets zero headers on
// purpose, to stay a "simple request" that needs no preflight and to remain
// interchangeable with sendBeacon, which cannot set headers at all. A key in
// a URL lands in every proxy access log in front of us — which is why
// RequireAPIKey does not offer this by default and why this is the one intake
// that asks for it explicitly.
func (a *Server) rumGate() gin.HandlerFunc {
	return RequireAPIKeyFrom(a.Store,
		KeyFromHeader("Dd-Api-Key"),
		KeyFromQuery("dd-api-key"),
	)
}

// rumCORSHeaders is what a preflight is told it may send. DD-API-KEY and the
// DD-EVP-* family are here for the mobile SDKs and for a browser deployment
// that chose headers over the query string; Content-Encoding is here because
// a proxy in front of the page may add it.
const rumCORSHeaders = "Content-Type, Content-Encoding, DD-API-KEY, DD-CLIENT-TOKEN, " +
	"DD-EVP-ORIGIN, DD-EVP-ORIGIN-VERSION, DD-REQUEST-ID, DD-IDEMPOTENCY-KEY"

// rumAllowedOrigins reads NINJACAT_RUM_ALLOWED_ORIGINS: a comma-separated
// list of origins allowed to post RUM data. Unset, empty or "*" means echo
// whatever Origin the request carried.
//
// Echoing is the default deliberately. The credential here is meant to be
// public — it is compiled into a web page — so an origin allowlist is not
// what keeps anyone out; it is a convenience for an operator who wants one,
// and making it mandatory would turn every new subdomain into silent data
// loss of exactly the kind this whole gate exists to prevent.
func rumAllowedOrigins() []string {
	raw := os.Getenv("NINJACAT_RUM_ALLOWED_ORIGINS")
	if raw == "" || strings.TrimSpace(raw) == "*" {
		return nil
	}
	var out []string
	for _, o := range strings.Split(raw, ",") {
		if o = strings.TrimSpace(o); o != "" {
			out = append(out, o)
		}
	}
	return out
}

// rumSetCORS puts the access-control headers on the response. It takes the
// header map rather than a *gin.Context because it runs above the router,
// where there is no context yet and where nothing — not an abort, not a
// NoRoute, not a panic recovery — can get a response out without it.
func rumSetCORS(h http.Header, origin string, allowed []string) {
	switch {
	case origin == "":
		// Not a browser: iOS, Android, curl. There is no origin to allow, and
		// "*" keeps the response usable by anything that does check.
		h.Set("Access-Control-Allow-Origin", "*")
	case len(allowed) == 0:
		h.Set("Access-Control-Allow-Origin", origin)
		// The answer varies by request header, so a cache must key on it.
		h.Add("Vary", "Origin")
	default:
		h.Add("Vary", "Origin")
		for _, a := range allowed {
			if a == origin {
				h.Set("Access-Control-Allow-Origin", origin)
				break
			}
		}
		// No match: no header, the browser blocks the read, and the operator
		// gets the failure they asked for by configuring a list.
	}

	h.Set("Access-Control-Allow-Methods", "POST, GET, OPTIONS")
	h.Set("Access-Control-Allow-Headers", rumCORSHeaders)
	h.Set("Access-Control-Max-Age", "86400")
}

// ---------------------------------------------------------------------------
// Transport quirks: ddforward, dd-evp-encoding
// ---------------------------------------------------------------------------

// rumUnwrapForward turns the browser SDK's proxy-as-a-string form back into
// the request it stands for.
//
// `proxy: '<url>'` makes the SDK send POST <url>?ddforward=<encoded path and
// query>, with everything that matters — the track, ddsource, the key — inside
// that one parameter. `proxy` as a FUNCTION is the form we document, because
// it produces a byte-identical Datadog request; this exists so the string form
// does not land on NoRoute with the payload attached.
//
// The forwarded query wins on conflict and the outer parameters are kept where
// they do not collide, so nothing an intermediary added is thrown away.
func rumUnwrapForward(r *http.Request) {
	outer := r.URL.Query()
	fwd := outer.Get("ddforward")
	if fwd == "" {
		return
	}

	u, err := url.Parse(fwd)
	if err != nil || u.Path == "" {
		// Leave the request alone: a 404 naming the path it actually sent is
		// more use to whoever misconfigured this than a guess would be.
		log.Printf("[rum] ddforward=%q is not a path+query, routing the request as it came", fwd)
		return
	}

	merged := u.Query()
	outer.Del("ddforward")
	for k, vs := range outer {
		if _, taken := merged[k]; !taken {
			merged[k] = vs
		}
	}

	r.URL.Path = u.Path
	r.URL.RawQuery = merged.Encode()
	r.RequestURI = r.URL.RequestURI()
}

// rumBody reads the request body, inflating it when the browser said so in
// the query string.
//
// The browser sends NO Content-Encoding header — the only signal is
// ?dd-evp-encoding=deflate — so Decompress(), which keys off the header, has
// already passed the body through untouched. iOS (deflate) and Android (gzip)
// do use the header and are handled there; this is the browser's case only.
//
// Failure to inflate is not fatal here for the same reason it is not fatal in
// Decompress: a mislabelled body that a decoder could still read must not be
// lost. What comes back then is the original bytes, which either parse or end
// up in raw_payloads.
func rumBody(c *gin.Context, label string) ([]byte, bool) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return nil, false
	}
	if len(body) == 0 || !strings.EqualFold(c.Query("dd-evp-encoding"), "deflate") {
		return body, true
	}

	out, err := decompress("deflate", body)
	if err != nil {
		log.Printf("[%s] dd-evp-encoding=deflate but the body does not inflate (%v), passing it through", label, err)
	}
	return out, true
}

// ---------------------------------------------------------------------------
// The request block every RUM row carries
// ---------------------------------------------------------------------------

// rumRequestColumns are the query parameters that already have a column, and
// the two credential spellings. Everything else goes to query_extra; these do
// not, so the JSON stays the record of what we did NOT otherwise keep, and so
// that a key never becomes durable in a table.
var rumRequestColumns = map[string]bool{
	"ddsource":              true,
	"dd-evp-origin":         true,
	"dd-evp-origin-version": true,
	"dd-request-id":         true,
	"dd-evp-encoding":       true,
	"batch_time":            true,
	"_dd.api":               true,
	"_dd.retry_count":       true,
	"_dd.retry_after":       true,
	"dd-api-key":            true,
	"api_key":               true,
}

// rumRequestInfo builds the block of sender columns shared by every RUM table.
//
// A RUM event has no hostname, no agent and no container id: the request is
// the only thing that says who sent it. Browser puts its identity in the query
// string (it sets no headers at all), iOS and Android put theirs in headers,
// so each field is looked for in both places.
func rumRequestInfo(c *gin.Context) storage.RumRequest {
	q := c.Request.URL.Query()

	req := storage.RumRequest{
		TenantID:         TenantFromContext(c),
		ReceivedAt:       time.Now().UTC(),
		DDSource:         q.Get("ddsource"),
		EVPOrigin:        firstNonEmpty(q.Get("dd-evp-origin"), c.GetHeader("Dd-Evp-Origin")),
		EVPOriginVersion: firstNonEmpty(q.Get("dd-evp-origin-version"), c.GetHeader("Dd-Evp-Origin-Version")),
		RequestID:        firstNonEmpty(q.Get("dd-request-id"), c.GetHeader("Dd-Request-Id")),
		IdempotencyKey:   c.GetHeader("Dd-Idempotency-Key"),
		// How the batch arrived compressed, as the sender spelled it: the
		// browser can only say it in the query string, iOS and Android use
		// Content-Encoding, which Decompress() has already acted on and left
		// in place. The body is stored decoded, so this column is the only
		// record that compression happened at all — and an SDK release that
		// silently stops compressing is a bandwidth regression that would
		// otherwise be visible nowhere.
		EVPEncoding: firstNonEmpty(q.Get("dd-evp-encoding"), c.GetHeader("Content-Encoding")),
		DDAPI:       q.Get("_dd.api"),
		RemoteAddr:  c.ClientIP(),
		UserAgent:   c.GetHeader("User-Agent"),
	}

	if ms, ok := parseInt64(q.Get("batch_time")); ok {
		t := time.UnixMilli(ms).UTC()
		req.BatchTime = &t
	}

	// Browser: _dd.retry_count / _dd.retry_after as query parameters.
	// iOS and Android: ddtags=retry_count:N,retry_after:CODE — different
	// spelling, same two facts. retry_after is stored as sent: a delay in
	// milliseconds from the browser, the HTTP status that caused the retry
	// from the mobile SDKs.
	retry := map[string]string{}
	for _, t := range splitDDTags(q.Get("ddtags")) {
		k, v := splitTag(t)
		retry[k] = v
	}
	if n, ok := parseInt64(firstNonEmpty(q.Get("_dd.retry_count"), retry["retry_count"])); ok && n >= 0 {
		u := uint32(n)
		req.RetryCount = &u
	}
	if n, ok := parseInt64(firstNonEmpty(q.Get("_dd.retry_after"), retry["retry_after"])); ok {
		req.RetryAfter = &n
	}

	req.QueryExtra = rumQueryExtra(q)
	return req
}

// rumQueryExtra renders the query parameters that got no column as JSON, or
// "" when there are none. Values stay slices: a parameter may legally repeat,
// and collapsing it would be the same mistake a plain tag map makes.
func rumQueryExtra(q url.Values) string {
	extra := make(map[string][]string, len(q))
	for k, v := range q {
		if !rumRequestColumns[strings.ToLower(k)] {
			extra[k] = v
		}
	}
	if len(extra) == 0 {
		return ""
	}
	out, err := json.Marshal(extra)
	if err != nil {
		return ""
	}
	return string(out)
}

// ---------------------------------------------------------------------------
// /api/v2/rum
// ---------------------------------------------------------------------------

// HandleRUM accepts the RUM event track: NDJSON, one JSON object per line,
// from browser, iOS, Android and everything built on them.
//
// 202 ALWAYS. Android logs a 200 as UnknownHttpError and drops the batch with
// it; iOS treats anything but 202 as unexpected and drops it silently; the
// browser accepts anything outside 408/429/5xx. 202 is the only answer all
// three agree means "kept".
//
// A line that no variant matches is not a failure of the batch: the SDKs ship
// on schema snapshots months apart and a newer one will send an event type
// this build has never seen. That line goes to raw_payloads and the rest of
// the batch is stored.
func (a *Server) HandleRUM(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	if isDiagnose(c) {
		return
	}

	body, ok := rumBody(c, "rum")
	if !ok || len(body) == 0 {
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	req := rumRequestInfo(c)
	lines := rumNDJSON(body)
	log.Printf("[rum] %d events, ddsource=%s origin=%s/%s request_id=%s",
		len(lines), req.DDSource, req.EVPOrigin, req.EVPOriginVersion, req.RequestID)

	// No tenant means no key, which RequireAPIKeyFrom already refused — so
	// this is a configuration fault, not traffic. Answer as before, store
	// nothing: tenant_id is the first ORDER BY column of every table and a
	// guessed value puts rows where no query looks.
	if req.TenantID == "" {
		return
	}

	var (
		views     []storage.RumViewRow
		events    []storage.RumEventRow
		telemetry []storage.RumTelemetryRow
		series    []storage.RumTimeseriesRow
	)

	for _, line := range lines {
		ev, err := rumevents.Decode(line)
		if err != nil {
			var unknown *rumevents.UnknownEventError
			if errors.As(err, &unknown) {
				a.storeRaw(c, "rum", "unknown_event", unknown.Type, line)
			} else {
				a.storeRaw(c, "rum", "decode_error", err.Error(), line)
			}
			continue
		}

		obj, err := rumDecodeObject(line)
		if err != nil {
			// rumevents.Decode just succeeded on these bytes, so this cannot
			// happen — but if it ever does, the line is kept rather than lost.
			a.storeRaw(c, "rum", "decode_error", err.Error(), line)
			continue
		}

		// The interfaces are the discriminator, not a string comparison:
		// rumevents picked the variant from the event's own discriminator
		// fields, and its type is that decision made once.
		switch ev.(type) {
		case rumevents.TelemetryEvent:
			telemetry = append(telemetry, rumTelemetryRow(req, obj, line))
		case rumevents.RumTimeseriesEvent:
			series = append(series, rumTimeseriesRow(req, obj, line))
		default:
			switch ev.EventType() {
			case "view", "view_update":
				views = append(views, rumViewRow(req, ev.EventType(), obj, line))
			default:
				events = append(events, rumEventRow(req, ev.EventType(), obj, line))
			}
		}
	}

	a.store(storage.RumViewsWriter, storage.WriteRumViews{Views: views}, len(views))
	a.store(storage.RumEventsWriter, storage.WriteRumEvents{Events: events}, len(events))
	a.store(storage.RumTelemetryWriter, storage.WriteRumTelemetry{Events: telemetry}, len(telemetry))
	a.store(storage.RumTimeseriesWriter, storage.WriteRumTimeseries{Series: series}, len(series))
}

// rumNDJSON splits a batch into lines, dropping empty ones. The SDKs write no
// trailing newline and \r never appears, but a proxy that rewrote the body
// might add one, and a blank line is not an event.
func rumNDJSON(body []byte) [][]byte {
	raw := bytes.Split(body, []byte("\n"))
	out := make([][]byte, 0, len(raw))
	for _, l := range raw {
		if l = bytes.TrimSpace(l); len(l) > 0 {
			out = append(out, l)
		}
	}
	return out
}

// rumViewRow projects a view or view_update onto the columns rum_views
// filters by. The event itself is kept whole in the last column — these are a
// filtering index over it, not a replacement for it.
//
// view and view_update land in the SAME row: they carry the same
// application/session/view identity and the same _dd.document_version, and
// view_update is the newer wire spelling of an update this table already
// models as a replacement. event_type records which spelling won.
func rumViewRow(req storage.RumRequest, eventType string, o rumObj, raw []byte) storage.RumViewRow {
	v := o.obj("view")
	perf := v.obj("performance")

	return storage.RumViewRow{
		Req:             req,
		Date:            wireTimeMillis(o.int64("date")),
		ApplicationID:   o.str("application", "id"),
		SessionID:       o.str("session", "id"),
		ViewID:          o.str("view", "id"),
		DocumentVersion: uint64(max(o.int64("_dd", "document_version"), 0)),
		EventType:       eventType,

		Service:      o.str("service"),
		Version:      o.str("version"),
		BuildVersion: o.str("build_version"),
		BuildID:      o.str("build_id"),
		Source:       o.str("source"),

		SessionType:             o.str("session", "type"),
		SessionHasReplay:        o.boolPtr("session", "has_replay"),
		SessionIsActive:         o.boolPtr("session", "is_active"),
		SessionSampledForReplay: o.boolPtr("session", "sampled_for_replay"),

		UsrID:          o.str("usr", "id"),
		UsrName:        o.str("usr", "name"),
		UsrEmail:       o.str("usr", "email"),
		UsrAnonymousID: o.str("usr", "anonymous_id"),
		AccountID:      o.str("account", "id"),
		AccountName:    o.str("account", "name"),

		ViewURL:      v.str("url"),
		ViewName:     v.str("name"),
		ViewReferrer: v.str("referrer"),

		ViewLoadingType:    v.str("loading_type"),
		ViewLoadingTime:    v.intPtr("loading_time"),
		ViewTimeSpent:      v.int64("time_spent"),
		ViewIsActive:       v.boolPtr("is_active"),
		ViewIsSlowRendered: v.boolPtr("is_slow_rendered"),

		ActionCount:      v.intPtr("action", "count"),
		ErrorCount:       v.intPtr("error", "count"),
		CrashCount:       v.intPtr("crash", "count"),
		LongTaskCount:    v.intPtr("long_task", "count"),
		FrozenFrameCount: v.intPtr("frozen_frame", "count"),
		ResourceCount:    v.intPtr("resource", "count"),
		FrustrationCount: v.intPtr("frustration", "count"),

		// Web Vitals moved from flat view.* fields into view.performance.*
		// and both spellings are still on the wire — the modern block first,
		// the deprecated field as the fallback, so one column answers for
		// either SDK generation.
		LCP:  firstIntPtr(perf.intPtr("lcp", "timestamp"), v.intPtr("largest_contentful_paint")),
		CLS:  firstFloatPtr(perf.floatPtr("cls", "score"), v.floatPtr("cumulative_layout_shift")),
		INP:  firstIntPtr(perf.intPtr("inp", "duration"), v.intPtr("interaction_to_next_paint")),
		FCP:  firstIntPtr(perf.intPtr("fcp", "timestamp"), v.intPtr("first_contentful_paint")),
		FID:  firstIntPtr(perf.intPtr("fid", "duration"), v.intPtr("first_input_delay")),
		FBC:  perf.intPtr("fbc", "timestamp"),
		TTFB: v.intPtr("first_byte"),

		DeviceType:  o.str("device", "type"),
		DeviceBrand: o.str("device", "brand"),
		DeviceModel: o.str("device", "model"),
		DeviceName:  o.str("device", "name"),
		OSName:      o.str("os", "name"),
		OSVersion:   o.str("os", "version"),

		ConnectivityStatus: o.str("connectivity", "status"),

		Context:      o.jsonAt("context"),
		FeatureFlags: o.jsonAt("feature_flags"),
		Tags:         tagsToMultiMap(splitDDTags(o.str("ddtags"))),
		Event:        string(raw),
	}
}

// rumEventRow projects an action, error, resource, long_task, vital or
// transition. Each kind fills its own block of columns and leaves the others
// empty; all of them keep the whole event.
func rumEventRow(req storage.RumRequest, eventType string, o rumObj, raw []byte) storage.RumEventRow {
	return storage.RumEventRow{
		Req:           req,
		Date:          wireTimeMillis(o.int64("date")),
		ApplicationID: o.str("application", "id"),
		SessionID:     o.str("session", "id"),
		ViewID:        o.str("view", "id"),
		EventType:     eventType,

		Service:      o.str("service"),
		Version:      o.str("version"),
		BuildVersion: o.str("build_version"),
		BuildID:      o.str("build_id"),
		Source:       o.str("source"),

		SessionType:      o.str("session", "type"),
		SessionHasReplay: o.boolPtr("session", "has_replay"),

		UsrID:          o.str("usr", "id"),
		UsrName:        o.str("usr", "name"),
		UsrEmail:       o.str("usr", "email"),
		UsrAnonymousID: o.str("usr", "anonymous_id"),
		AccountID:      o.str("account", "id"),
		AccountName:    o.str("account", "name"),

		ViewURL:      o.str("view", "url"),
		ViewName:     o.str("view", "name"),
		ViewReferrer: o.str("view", "referrer"),

		DeviceType:         o.str("device", "type"),
		DeviceBrand:        o.str("device", "brand"),
		DeviceModel:        o.str("device", "model"),
		DeviceName:         o.str("device", "name"),
		OSName:             o.str("os", "name"),
		OSVersion:          o.str("os", "version"),
		ConnectivityStatus: o.str("connectivity", "status"),

		ActionType: o.str("action", "type"),
		// The label a user sees is action.target.name, not a field on action.
		ActionName: o.str("action", "target", "name"),
		ActionID:   o.str("action", "id"),
		// One click can be classified several ways at once — a rage click
		// that is also a dead click — so this is a list on the wire and a
		// list here.
		ActionFrustrationTypes: o.strings("action", "frustration", "type"),

		ErrorID:      o.str("error", "id"),
		ErrorMessage: o.str("error", "message"),
		ErrorType:    o.str("error", "type"),
		ErrorSource:  o.str("error", "source"),
		ErrorStack:   o.str("error", "stack"),
		// The crash and ANR reports the mobile SDKs write at the NEXT app
		// launch arrive as ordinary errors with this flag set, which is why
		// their date can predate the session they belong to.
		ErrorIsCrash:     o.boolPtr("error", "is_crash"),
		ErrorHandling:    o.str("error", "handling"),
		ErrorFingerprint: o.str("error", "fingerprint"),

		ResourceID:         o.str("resource", "id"),
		ResourceType:       o.str("resource", "type"),
		ResourceURL:        o.str("resource", "url"),
		ResourceMethod:     o.str("resource", "method"),
		ResourceStatusCode: o.intPtr("resource", "status_code"),
		ResourceDuration:   o.intPtr("resource", "duration"),
		ResourceSize:       o.intPtr("resource", "size"),

		LongTaskID:       o.str("long_task", "id"),
		LongTaskDuration: o.intPtr("long_task", "duration"),

		VitalID:       o.str("vital", "id"),
		VitalType:     o.str("vital", "type"),
		VitalName:     o.str("vital", "name"),
		VitalDuration: o.intPtr("vital", "duration"),

		Context: o.jsonAt("context"),
		Tags:    tagsToMultiMap(splitDDTags(o.str("ddtags"))),
		Event:   string(raw),
	}
}

// rumTelemetryRow projects a type:"telemetry" event — the SDK reporting on
// itself. It arrives on this same track from every SDK, including one
// configured for Logs only.
func rumTelemetryRow(req storage.RumRequest, o rumObj, raw []byte) storage.RumTelemetryRow {
	t := o.obj("telemetry")

	return storage.RumTelemetryRow{
		Req:           req,
		Date:          wireTimeMillis(o.int64("date")),
		ApplicationID: o.str("application", "id"),
		SessionID:     o.str("session", "id"),
		ViewID:        o.str("view", "id"),
		ActionID:      o.str("action", "id"),

		Type:          o.str("type"),
		Status:        t.str("status"),
		TelemetryType: t.str("type"),

		Message:    t.str("message"),
		ErrorStack: t.str("error", "stack"),
		ErrorKind:  t.str("error", "kind"),
		// ~150 optional keys that change with every SDK release: worth
		// keeping whole, not worth a column each.
		Configuration: t.jsonAt("configuration"),
		UsageFeature:  t.str("usage", "feature"),

		Service: o.str("service"),
		Version: o.str("version"),
		Source:  o.str("source"),

		EffectiveSampleRate:  o.floatPtr("effective_sample_rate"),
		ExperimentalFeatures: o.strings("experimental_features"),

		DeviceBrand:        t.str("device", "brand"),
		DeviceModel:        t.str("device", "model"),
		DeviceArchitecture: t.str("device", "architecture"),
		OSName:             t.str("os", "name"),
		OSVersion:          t.str("os", "version"),
		OSBuild:            t.str("os", "build"),

		Event: string(raw),
	}
}

// rumTimeseriesRow projects a type:"timeseries" event: a mobile-only run of
// cpu or memory samples, timestamps in one array and the measured quantities
// in a parallel structure.
//
// start/end are NANOSECONDS here, unlike `date`, which is milliseconds. The
// arrays stay as they arrived so timestamps[i] keeps pointing at values.*[i].
func rumTimeseriesRow(req storage.RumRequest, o rumObj, raw []byte) storage.RumTimeseriesRow {
	ts := o.obj("timeseries")

	return storage.RumTimeseriesRow{
		Req:           req,
		Date:          wireTimeMillis(o.int64("date")),
		ApplicationID: o.str("application", "id"),
		SessionID:     o.str("session", "id"),
		ViewID:        o.str("view", "id"),

		Name:   ts.str("name"),
		ID:     ts.str("id"),
		Schema: ts.str("schema"),

		Start:      wireTimeNanos(ts.int64("start")),
		End:        wireTimeNanos(ts.int64("end")),
		Timestamps: ts.int64s("data", "timestamps"),
		Values:     ts.jsonAt("data", "values"),

		Service: o.str("service"),
		Version: o.str("version"),
		Source:  o.str("source"),
		Context: o.jsonAt("context"),
		Tags:    tagsToMultiMap(splitDDTags(o.str("ddtags"))),
		Event:   string(raw),
	}
}

// ---------------------------------------------------------------------------
// /api/v2/replay
// ---------------------------------------------------------------------------

// HandleRUMReplay accepts Session Replay uploads: a multipart body of one or
// more binary parts plus one `event` part of metadata.
//
// THE BLOBS ARE NOT DECODED, and that is the design rather than a shortcut.
// Datadog's own intake stores segments "without any processing", and the
// browser SDK's encoder deliberately emits zlib streams that can be
// CONCATENATED later — inflating and re-deflating would throw that property
// away. rumevents/replay models both segment formats for the day a viewer
// needs them; nothing at ingest does.
//
// Three shapes on one path:
//
//	browser segment    part "segment" (zlib) + "event" (one object)
//	mobile  segment    parts "file0".."fileN" (zlib) + "event" (an array,
//	                   one entry per part, in the same order)
//	resource (canvas)  parts "image" (raw, repeated on mobile) + "event"
func (a *Server) HandleRUMReplay(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	if isDiagnose(c) {
		return
	}

	body, ok := rumBody(c, "rum-replay")
	if !ok || len(body) == 0 {
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	ct := c.GetHeader("Content-Type")
	parts, err := rumParts("rum-replay", ct, body)
	if err != nil {
		log.Printf("[rum-replay] not multipart (%v), raw follows", err)
		describe("rum-replay", ct, body)
		a.storeRaw(c, "rum", "unexpected_shape", "replay body is not multipart: "+err.Error(), body)
		return
	}

	req := rumRequestInfo(c)
	if req.TenantID == "" {
		return
	}

	// The metadata part is one object on browser and an array on mobile,
	// because a mobile batch spans several views and ships a segment for
	// each. Either way entry i describes blob i, in wire order.
	var metas []rumObj
	var metaRaw []string
	for _, p := range parts {
		if p.Name != "event" {
			continue
		}
		metas, metaRaw = rumReplayMetadata(p.Data)
		if metas == nil {
			a.storeRaw(c, "rum", "decode_error", "replay event part is neither an object nor an array", p.Data)
		}
	}

	rows := make([]storage.RumReplaySegmentRow, 0, len(parts))
	for i, p := range parts {
		if p.Name == "event" {
			continue
		}
		var meta rumObj
		var raw string
		if n := len(rows); n < len(metas) {
			meta, raw = metas[n], metaRaw[n]
		}
		rows = append(rows, rumReplaySegmentRow(req, p, meta, raw))
		_ = i
	}

	if len(metas) > 0 && len(rows) != len(metas) {
		// Not fatal — every blob is still stored — but it means the pairing
		// above put metadata on the wrong row for some of them, and that is
		// worth seeing rather than guessing at later.
		log.Printf("[rum-replay] %d blob parts against %d metadata entries: the pairing is by position and they disagree",
			len(rows), len(metas))
	}

	a.store(storage.RumReplaySegmentsWriter, storage.WriteRumReplaySegments{Segments: rows}, len(rows))
}

// rumReplayMetadata decodes the `event` part, which is one object (browser) or
// an array of them (iOS, Android). It returns the entries both decoded and as
// the JSON text stored per row; nil when the part is neither shape.
func rumReplayMetadata(data []byte) ([]rumObj, []string) {
	if bytes.HasPrefix(bytes.TrimSpace(data), []byte("[")) {
		var arr []json.RawMessage
		if err := jsonDecode(data, &arr); err != nil {
			return nil, nil
		}
		objs := make([]rumObj, 0, len(arr))
		raws := make([]string, 0, len(arr))
		for _, e := range arr {
			o, err := rumDecodeObject(e)
			if err != nil {
				o = rumObj{}
			}
			objs = append(objs, o)
			raws = append(raws, string(e))
		}
		return objs, raws
	}

	o, err := rumDecodeObject(data)
	if err != nil {
		return nil, nil
	}
	return []rumObj{o}, []string{string(data)}
}

func rumReplaySegmentRow(req storage.RumRequest, p rumPart, meta rumObj, metaRaw string) storage.RumReplaySegmentRow {
	row := storage.RumReplaySegmentRow{
		Req:           req,
		ApplicationID: meta.str("application", "id"),
		SessionID:     meta.str("session", "id"),
		ViewID:        meta.str("view", "id"),
		Source:        meta.str("source"),

		Variant:      rumReplayVariant(p.Name, meta),
		PartName:     p.Name,
		PartFilename: p.Filename,

		Start: meta.timePtrMillis("start"),
		End:   meta.timePtrMillis("end"),

		RecordsCount:    meta.intPtr("records_count"),
		HasFullSnapshot: meta.boolPtr("has_full_snapshot"),
		// Browser sends an index, Android sends an explicit null (an upstream
		// TODO) and iOS omits the key. Three states, so a pointer.
		IndexInView:    meta.intPtr("index_in_view"),
		CreationReason: meta.str("creation_reason"),

		RawSegmentSize:        meta.intPtr("raw_segment_size"),
		CompressedSegmentSize: meta.intPtr("compressed_segment_size"),

		Event: metaRaw,
	}

	// The bytes go in the column that says what they are. Neither is touched:
	// a segment is still the zlib stream the SDK produced, an image is still
	// the raw canvas blob.
	if row.Variant == "resource" {
		row.Image = string(p.Data)
	} else {
		row.Segment = string(p.Data)
	}
	return row
}

// rumReplayVariant tells a recording segment from a canvas resource. The part
// name is the primary signal — "image" for resources, "segment"/"file<i>" for
// recordings — and the metadata's own "type" is the fallback for a producer
// that names its parts differently.
func rumReplayVariant(partName string, meta rumObj) string {
	switch {
	case strings.HasPrefix(partName, "image"):
		return "resource"
	case partName == "segment", strings.HasPrefix(partName, "file"):
		return "segment"
	case meta.str("type") == "resource":
		return "resource"
	default:
		return "segment"
	}
}

// rumPart is one part of a multipart body, with everything that identifies it.
type rumPart struct {
	Name        string
	Filename    string
	ContentType string
	Data        []byte
}

// rumParts reads a multipart body into an ORDERED slice, keeping repeats.
//
// It is not profParts (router_profiling.go), and the difference is the reason
// it exists: profParts returns a map keyed by form name, so a repeated name
// keeps only the last part. Replay uses repeated names by design — the mobile
// canvas-resource variant sends several parts all called "image" — and order
// is load-bearing, because the metadata array's entry i describes blob i. A
// map would silently drop every image but the last and shuffle the pairing.
func rumParts(label, contentType string, body []byte) ([]rumPart, error) {
	mediaType, params, err := mime.ParseMediaType(contentType)
	if err != nil {
		return nil, err
	}
	if !strings.HasPrefix(mediaType, "multipart/") {
		return nil, fmt.Errorf("content-type is %s", orUnknown(mediaType))
	}
	boundary := params["boundary"]
	if boundary == "" {
		return nil, errors.New("multipart without boundary")
	}

	log.Printf("[%s] %s %d B", label, mediaType, len(body))
	var out []rumPart
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
		out = append(out, rumPart{
			Name:        p.FormName(),
			Filename:    p.FileName(),
			ContentType: p.Header.Get("Content-Type"),
			Data:        data,
		})
	}
	return out, nil
}

// ---------------------------------------------------------------------------
// /api/v2/spans
// ---------------------------------------------------------------------------

// HandleRUMSpans accepts the mobile SDKs' own trace format.
//
// This is NOT the agent's /v0.4/traces: it is NDJSON of
// {"spans":[...],"env":"..."} envelopes with a flat, hand-rolled span shape,
// produced only by dd-sdk-ios and dd-sdk-android, with no published JSON
// Schema anywhere. It lives on this host because it shares a host, a
// credential and a sender with mobile RUM, and shares nothing at all with the
// msgpack tracer payloads.
func (a *Server) HandleRUMSpans(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	if isDiagnose(c) {
		return
	}

	body, ok := rumBody(c, "rum-spans")
	if !ok || len(body) == 0 {
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	req := rumRequestInfo(c)
	lines := rumNDJSON(body)
	log.Printf("[rum-spans] %d envelopes, origin=%s/%s", len(lines), req.EVPOrigin, req.EVPOriginVersion)
	if req.TenantID == "" {
		return
	}

	var rows []storage.RumSpanRow
	for _, line := range lines {
		// Decoded as raw messages first so each span's own bytes survive
		// exactly as sent — a re-marshalled object would reorder its keys.
		var envelope map[string]json.RawMessage
		if err := jsonDecode(line, &envelope); err != nil {
			a.storeRaw(c, "rum", "decode_error", err.Error(), line)
			continue
		}

		rawSpans, ok := envelope["spans"]
		if !ok {
			a.storeRaw(c, "rum", "unexpected_shape", "span envelope without a spans array", line)
			continue
		}
		var spans []json.RawMessage
		if err := jsonDecode(rawSpans, &spans); err != nil {
			a.storeRaw(c, "rum", "decode_error", "spans is not an array: "+err.Error(), line)
			continue
		}

		env := ""
		if raw, ok := envelope["env"]; ok {
			_ = json.Unmarshal(raw, &env)
		}
		extra := rumEnvelopeExtra(envelope)

		for _, rawSpan := range spans {
			o, err := rumDecodeObject(rawSpan)
			if err != nil {
				a.storeRaw(c, "rum", "decode_error", "span is not an object: "+err.Error(), rawSpan)
				continue
			}
			rows = append(rows, rumSpanRow(req, env, extra, o, rawSpan))
		}
	}

	a.store(storage.RumSpansWriter, storage.WriteRumSpans{Spans: rows}, len(rows))
}

// rumEnvelopeExtra keeps whatever the envelope carried besides spans and env,
// as JSON. Usually "": it exists so a key a future SDK release adds is stored
// rather than counted.
func rumEnvelopeExtra(envelope map[string]json.RawMessage) string {
	extra := make(map[string]json.RawMessage, len(envelope))
	for k, v := range envelope {
		if k != "spans" && k != "env" {
			extra[k] = v
		}
	}
	if len(extra) == 0 {
		return ""
	}
	out, err := json.Marshal(extra)
	if err != nil {
		return ""
	}
	return string(out)
}

func rumSpanRow(req storage.RumRequest, env, envelopeExtra string, o rumObj, raw []byte) storage.RumSpanRow {
	return storage.RumSpanRow{
		Req: req,
		// Opaque text, never a number. iOS writes the low 64 bits as hex,
		// Android's schema types all three as strings, and parsing them would
		// need a guess about the base — while a float64 anywhere on that path
		// drops digits above 2^53.
		TraceID:  o.text("trace_id"),
		SpanID:   o.text("span_id"),
		ParentID: o.text("parent_id"),

		Name:     o.str("name"),
		Service:  o.str("service"),
		Resource: o.str("resource"),
		Type:     o.str("type"),
		Env:      env,

		Start:      wireTimeNanos(o.int64("start")),
		DurationNS: o.int64("duration"),
		Error:      int8(o.int64("error")),

		Meta:    rumFlattenMeta(o.obj("meta")),
		Metrics: rumMetrics(o.obj("metrics")),

		Span:          string(raw),
		EnvelopeExtra: envelopeExtra,
	}
}

// rumFlattenMeta turns the span's meta object into a flat map with dotted
// keys. meta.device and meta.os arrive as nested objects while every other
// key is flat already, so flattening is what makes meta['device.brand']
// reachable instead of dropping the nested branches.
//
// A plain Map, NOT the tag multiset shape: this is a JSON object and a JSON
// object cannot repeat a key.
func rumFlattenMeta(o rumObj) map[string]string {
	if len(o) == 0 {
		return nil
	}
	out := make(map[string]string, len(o))
	rumFlattenInto(out, "", o)
	return out
}

func rumFlattenInto(out map[string]string, prefix string, o rumObj) {
	for k, v := range o {
		key := k
		if prefix != "" {
			key = prefix + "." + k
		}
		switch t := v.(type) {
		case map[string]any:
			rumFlattenInto(out, key, rumObj(t))
		case string:
			out[key] = t
		case nil:
			// An explicit null is not an empty string; recording the key with
			// no value keeps "sent as null" distinct from "not sent".
			out[key] = ""
		default:
			// Numbers, booleans and arrays: rendered rather than dropped.
			// json.Number stringifies to its exact wire text.
			out[key] = rumText(v)
		}
	}
}

// rumMetrics reads the span's metrics object. Values are float64 there by
// definition of the column, and json.Number parses without the intermediate
// any-to-float64 assertion that would round a large integer.
func rumMetrics(o rumObj) map[string]float64 {
	if len(o) == 0 {
		return nil
	}
	out := make(map[string]float64, len(o))
	for k, v := range o {
		if n, ok := v.(json.Number); ok {
			if f, err := n.Float64(); err == nil {
				out[k] = f
			}
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Reading a decoded event
// ---------------------------------------------------------------------------

// rumObj is one decoded JSON object, numbers kept as json.Number.
//
// WHY A GENERIC MAP when rumevents has 148 generated structs. The variant is
// chosen by rumevents.Decode — that is the part a hand-rolled reader would get
// wrong — but the sixteen variants are sixteen unrelated Go types whose `view`
// and `session` sub-objects are DIFFERENT types with identical JSON. Projecting
// each of them onto one row would mean sixteen near-identical conversion
// functions, and RumViewEvent and RumViewUpdateEvent alone would need two that
// must never disagree. One path-walking reader over the same bytes is one
// implementation to get right, it handles a field a struct does not declare yet
// (rumevents keeps those in AdditionalProperties, where a typed projection
// would not look), and the whole event is stored verbatim beside it either way.
//
// No float64 anywhere: numbers arrive as json.Number and are converted
// straight to int64 or float64 on demand, so a 64-bit id or timestamp keeps
// every digit.
type rumObj map[string]any

// rumDecodeObject decodes one JSON object with numbers as json.Number.
func rumDecodeObject(data []byte) (rumObj, error) {
	var o rumObj
	if err := jsonDecode(data, &o); err != nil {
		return nil, err
	}
	return o, nil
}

// at walks a key path and returns the value, or nil if any step is missing or
// is not an object.
func (o rumObj) at(path ...string) any {
	var cur any = map[string]any(o)
	for _, k := range path {
		m, ok := cur.(map[string]any)
		if !ok {
			return nil
		}
		cur = m[k]
	}
	return cur
}

// obj returns a sub-object, or an empty one — so a chain of lookups on a
// missing branch is empty rather than a nil dereference.
func (o rumObj) obj(path ...string) rumObj {
	if m, ok := o.at(path...).(map[string]any); ok {
		return rumObj(m)
	}
	return rumObj{}
}

// str returns a string field, or "" when it is absent or another type. A
// value of the wrong type is not coerced: it stays in the event column, where
// it is visible as what it was.
func (o rumObj) str(path ...string) string {
	s, _ := o.at(path...).(string)
	return s
}

// text renders a field that may legally be a string OR a number — the span
// ids, which one SDK writes as hex text and another as a JSON number.
func (o rumObj) text(path ...string) string {
	return rumText(o.at(path...))
}

func rumText(v any) string {
	switch t := v.(type) {
	case nil:
		return ""
	case string:
		return t
	case json.Number:
		return t.String()
	case bool:
		return strconv.FormatBool(t)
	default:
		out, err := json.Marshal(t)
		if err != nil {
			return ""
		}
		return string(out)
	}
}

// int64 returns an integer field, or 0 when it is absent. For a column that
// must tell absent from zero, use intPtr.
func (o rumObj) int64(path ...string) int64 {
	if p := o.intPtr(path...); p != nil {
		return *p
	}
	return 0
}

// intPtr returns an integer field, or nil when it is absent — nil reaches a
// Nullable column as NULL. A view that sends no crash sub-object has not
// measured zero crashes; it is a platform with no crashes to measure, and a 0
// there would turn "unknown" into a number somebody averages.
func (o rumObj) intPtr(path ...string) *int64 {
	n, ok := o.at(path...).(json.Number)
	if !ok {
		return nil
	}
	v, err := n.Int64()
	if err != nil {
		// A number written with a decimal point where the schema says
		// integer: take it as a float rather than losing the field.
		f, ferr := n.Float64()
		if ferr != nil {
			return nil
		}
		v = int64(f)
	}
	return &v
}

func (o rumObj) floatPtr(path ...string) *float64 {
	n, ok := o.at(path...).(json.Number)
	if !ok {
		return nil
	}
	v, err := n.Float64()
	if err != nil {
		return nil
	}
	return &v
}

func (o rumObj) boolPtr(path ...string) *bool {
	b, ok := o.at(path...).(bool)
	if !ok {
		return nil
	}
	return &b
}

// timePtrMillis reads a millisecond epoch into a time, or nil when it is
// absent. Never the epoch as a sentinel: a replay resource carries no window
// at all, and 1970 would put it on a timeline.
func (o rumObj) timePtrMillis(path ...string) *time.Time {
	p := o.intPtr(path...)
	if p == nil {
		return nil
	}
	t := time.UnixMilli(*p).UTC()
	return &t
}

// strings reads an array of strings, keeping wire order. A lone string is
// accepted as a one-element list, because several SDK fields are documented as
// arrays and sent as scalars by at least one of them.
func (o rumObj) strings(path ...string) []string {
	switch t := o.at(path...).(type) {
	case []any:
		out := make([]string, 0, len(t))
		for _, v := range t {
			out = append(out, rumText(v))
		}
		return out
	case string:
		return []string{t}
	default:
		return nil
	}
}

// int64s reads an array of integers, keeping wire order — the pairing between
// timestamps[i] and values.*[i] is the data.
func (o rumObj) int64s(path ...string) []int64 {
	arr, ok := o.at(path...).([]any)
	if !ok {
		return nil
	}
	out := make([]int64, 0, len(arr))
	for _, v := range arr {
		n, ok := v.(json.Number)
		if !ok {
			continue
		}
		if i, err := n.Int64(); err == nil {
			out = append(out, i)
		}
	}
	return out
}

// jsonAt renders a sub-object or array back to JSON for a String column, or
// "" when it is absent. json.Number re-encodes as its exact wire text, so a
// large integer inside a context map survives; key order is Go's, and the
// wire's order is preserved in the event column beside it.
func (o rumObj) jsonAt(path ...string) string {
	v := o.at(path...)
	if v == nil {
		return ""
	}
	out, err := json.Marshal(v)
	if err != nil {
		return ""
	}
	return string(out)
}

// ---------------------------------------------------------------------------
// Small shared helpers
// ---------------------------------------------------------------------------

// wireTimeNanos turns a nanosecond epoch into a time, falling back to arrival
// time when there is none — the same rule wireTimeMillis follows, so a missing
// timestamp never reads as 1970.
func wireTimeNanos(nanos int64) time.Time {
	if nanos <= 0 {
		return time.Now().UTC()
	}
	return time.Unix(0, nanos).UTC()
}

func firstIntPtr(values ...*int64) *int64 {
	for _, v := range values {
		if v != nil {
			return v
		}
	}
	return nil
}

func firstFloatPtr(values ...*float64) *float64 {
	for _, v := range values {
		if v != nil {
			return v
		}
	}
	return nil
}

func parseInt64(s string) (int64, bool) {
	if s == "" {
		return 0, false
	}
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil {
		return 0, false
	}
	return n, true
}
