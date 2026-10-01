/// Tests of Routers/Rum.fs beyond the golden fixtures.
///
/// The RUM intake is the one surface whose clients are web pages and phones,
/// and almost every failure it can have is invisible from outside: a browser
/// that loses a batch to CORS reports success, an Android app that gets a 200
/// drops the batch, and a mobile batch can carry events from hours ago.
module NinjaCat.Api.Intake.Tests.RumTests

open System
open System.IO
open System.Net
open System.Text
open System.Text.Json
open System.Text.Json.Nodes
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Intake.Tests.Golden

let private json (text: string) : JsonElement = JsonDocument.Parse(text).RootElement

let private utf8 (value: string) : byte[] = Encoding.UTF8.GetBytes value

let private child (name: string) (node: JsonNode) : JsonNode = node[name]

let private text (node: JsonNode) : string = node.GetValue<string>()

/// The peer address every Go httptest request has.
let private peer = IPAddress.Parse "192.0.2.1"

let private httpContext (method: string) (url: string) (headers: (string * string) list) : HttpContext =
    let http = DefaultHttpContext()

    let path, query =
        match url.IndexOf '?' with
        | -1 -> url, ""
        | i -> url.Substring(0, i), url.Substring i

    http.Request.Method <- method
    http.Request.Host <- HostString "browser-intake.example.test"
    http.Request.Path <- PathString path
    http.Request.QueryString <- QueryString query

    for name, value in headers do
        http.Request.Headers[name] <- StringValues value

    http.Connection.RemoteIpAddress <- peer
    http

/// browser-intake as it is served: the routes behind the wrapper.
let private serve (allowedOrigins: string list) (sink: CapturingSink) (http: HttpContext) (body: byte[]) : Response =
    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    Replay.browserIntake deps allowedOrigins http body

/// A POST with the test key in the header, as the mobile SDKs send it.
let private post (sink: CapturingSink) (url: string) (headers: (string * string) list) (body: byte[]) : Response =
    serve [] sink (httpContext "POST" url (("Dd-Api-Key", Replay.testKey) :: headers)) body

let private header (name: string) (response: Response) : string =
    response.Headers |> List.tryFind (fun (n, _) -> n = name) |> Option.map snd |> Option.defaultValue ""

let private requestOf (url: string) (headers: (string * string) list) : Request =
    { Http = httpContext "POST" url headers
      Body = [||]
      Key = Some { ID = "test"; Name = "test"; TenantID = "t" }
      Sink = CapturingSink()
      Log = NullLogger.Instance }

let private sender: RumRequest = Rum.requestInfo (requestOf "/api/v2/rum" [])

let private multipart (parts: (string * string * byte[]) list) : string * byte[] =
    let boundary = "rum-tests-boundary"
    use buffer = new MemoryStream()
    let write (text: string) = buffer.Write(ReadOnlySpan(utf8 text))

    for name, fileName, data in parts do
        write $"--{boundary}\r\n"

        if fileName = "" then
            write $"Content-Disposition: form-data; name=\"{name}\"\r\n\r\n"
        else
            write $"Content-Disposition: form-data; name=\"{name}\"; filename=\"{fileName}\"\r\n"
            write "Content-Type: application/octet-stream\r\n\r\n"

        buffer.Write(ReadOnlySpan data)
        write "\r\n"

    write $"--{boundary}--\r\n"
    $"multipart/form-data; boundary={boundary}", buffer.ToArray()

let private action =
    """{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},"action":{"id":"a","type":"click"}}"""

/// A view as a browser sends it: Web Vitals in the modern view.performance
/// block with the deprecated flat fields beside them.
let private view =
    """{"type":"view","date":1591283924940,
        "application":{"id":"ac8218cf-498b-4d33-bd44-151095959547"},
        "session":{"id":"cacbf45c-3a05-48ce-b066-d76349460599","type":"user"},
        "view":{"id":"623d50fd-75cf-4025-97d2-e51ff94171f6","loading_type":"initial_load",
                "time_spent":245512755000,"largest_contentful_paint":1,"cumulative_layout_shift":0.9,
                "first_contentful_paint":420725000,"error":{"count":2},"frustration":{"count":9},
                "performance":{"cls":{"score":0.1},"lcp":{"timestamp":20000000}}},
        "_dd":{"document_version":9},"device":{"brand":"Apple"},"os":{"name":"iOS"},
        "feature_flags":{"feature_one":true}}"""

[<Fact>]
let ``the browser profiler's quota check is admitted with the key in DD-CLIENT-TOKEN`` () =
    let ask (headers: (string * string) list) =
        let sink = CapturingSink()
        let response = serve [] sink (httpContext "GET" "/api/v2/profiling/quota?session_id=s-1" headers) [||]
        Assert.Empty sink.Writes
        response.Status, Text.Encoding.UTF8.GetString response.Body

    Assert.Equal(
        (200, """{"data":{"attributes":{"admitted":true,"reason":"quota_ok"}}}"""),
        ask [ "DD-CLIENT-TOKEN", Replay.testKey; "Origin", "https://shop.example" ]
    )

    Assert.Equal(403, fst (ask [ "DD-CLIENT-TOKEN", "not-a-key" ]))

[<Fact>]
let ``a view keeps its identity, its version and the SDK's date`` () =
    let row = Rum.viewRow sender "view" (json view) view

    Assert.Equal("ac8218cf-498b-4d33-bd44-151095959547", row.ApplicationID)
    Assert.Equal("623d50fd-75cf-4025-97d2-e51ff94171f6", row.ViewID)
    // The ReplacingMergeTree version.
    Assert.Equal(9UL, row.DocumentVersion)
    // Milliseconds, not seconds and not the arrival time.
    Assert.Equal(Time.fromUnixMillis 1591283924940L, row.Date)
    // Averaging loading_time across initial_load and route_change means
    // nothing, so the type must survive.
    Assert.Equal("initial_load", row.ViewLoadingType)
    Assert.Equal(245512755000L, row.ViewTimeSpent)
    Assert.Equal(Some 2L, row.ErrorCount)
    Assert.Equal(Some 9L, row.FrustrationCount)
    Assert.Equal("Apple", row.DeviceBrand)
    Assert.Equal("iOS", row.OSName)
    Assert.Contains("feature_one", row.FeatureFlags)
    Assert.Equal(view, row.Event)

[<Fact>]
let ``a view's modern vitals win over the deprecated fields, which remain the fallback`` () =
    let row = Rum.viewRow sender "view" (json view) view

    Assert.Equal(Some 20000000L, row.LCP)
    Assert.Equal(Some 0.1, row.CLS)
    // No view.performance.fcp in this event: the flat field answers.
    Assert.Equal(Some 420725000L, row.FCP)
    Assert.Equal(None, row.INP)

[<Fact>]
let ``a view with no crash object has not measured zero crashes`` () =
    let row = Rum.viewRow sender "view" (json view) view
    Assert.Equal(None, row.CrashCount)

[<Fact>]
let ``a view_update lands in rum_views under the same identity`` () =
    let update =
        """{"type":"view_update","date":1712345678901,"application":{"id":"app"},"session":{"id":"s","type":"user"},"view":{"id":"v","url":"https://example.com/home","time_spent":245512755000},"_dd":{"document_version":2,"format_version":2}}"""

    let sink = CapturingSink()
    let response = post sink "/api/v2/rum?ddsource=browser" [] (utf8 update)

    Assert.Equal(202, response.Status)
    let row = Assert.Single(sink.Rows<RumViewRow>())
    Assert.Equal("view_update", row.EventType)
    Assert.Equal(("app", "s", "v"), (row.ApplicationID, row.SessionID, row.ViewID))
    // Without it a view_update can never win a merge.
    Assert.Equal(2UL, row.DocumentVersion)

let private eventOf (kind: string) (body: string) : RumEventRow =
    let row = Rum.eventRow sender kind (json body) body
    Assert.Equal(kind, row.EventType)
    row

[<Fact>]
let ``an action carries its label from target.name and every frustration type`` () =
    let row =
        eventOf
            "action"
            """{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
                "action":{"id":"a1","type":"click","target":{"name":"Buy now"},
                          "frustration":{"type":["rage_click","dead_click"]}}}"""

    Assert.Equal("Buy now", row.ActionName)
    // One click can be classified twice.
    Assert.Equal<string[]>([| "rage_click"; "dead_click" |], row.ActionFrustrationTypes)
    Assert.Equal("", row.ErrorMessage)
    Assert.Equal("", row.ResourceURL)

[<Fact>]
let ``a crash report is an ordinary error with is_crash set`` () =
    let row =
        eventOf
            "error"
            """{"type":"error","date":1,"application":{"id":"app"},"session":{"id":"s"},
                "error":{"id":"e1","message":"boom","source":"source","is_crash":true,
                         "handling":"unhandled","fingerprint":"fp","stack":"at x"}}"""

    Assert.Equal(Some true, row.ErrorIsCrash)
    Assert.Equal("boom", row.ErrorMessage)
    Assert.Equal("fp", row.ErrorFingerprint)

[<Fact>]
let ``an error that never mentions is_crash is unknown, not false`` () =
    let row =
        eventOf
            "error"
            """{"type":"error","date":1,"application":{"id":"app"},"session":{"id":"s"},
                "error":{"id":"e1","message":"boom","source":"source"}}"""

    Assert.Equal(None, row.ErrorIsCrash)

[<Fact>]
let ``a resource that returned 0 bytes is not a resource with no size`` () =
    let row =
        eventOf
            "resource"
            """{"type":"resource","date":1,"application":{"id":"app"},"session":{"id":"s"},
                "resource":{"id":"r1","type":"xhr","url":"https://x/y","method":"GET",
                            "status_code":204,"duration":1200,"size":0}}"""

    Assert.Equal(Some 0L, row.ResourceSize)
    Assert.Equal(Some 204L, row.ResourceStatusCode)
    Assert.Equal(None, row.LongTaskDuration)

[<Fact>]
let ``a vital keeps the sub-discriminator that picked its variant`` () =
    let row =
        eventOf
            "vital"
            """{"type":"vital","date":1,"application":{"id":"app"},"session":{"id":"s"},
                "vital":{"id":"v1","type":"duration","name":"checkout","duration":900}}"""

    Assert.Equal("duration", row.VitalType)
    Assert.Equal("checkout", row.VitalName)
    Assert.Equal(Some 900L, row.VitalDuration)

/// Tags are a multiset: nothing forbids two tags sharing a key, and every SDK
/// lets an app add its own.
[<Fact>]
let ``an event keeps repeated tags`` () =
    let row =
        eventOf
            "action"
            """{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
                "ddtags":"env:prod,team:web,team:payments,beta"}"""

    Assert.Equal(3, row.Tags.Count)
    Assert.Equal<string[]>([| "prod" |], row.Tags["env"])
    Assert.Equal<string[]>([| "web"; "payments" |], row.Tags["team"])
    Assert.Equal<string[]>([| "" |], row.Tags["beta"])

/// An SDK ships against a schema snapshot months older than ours and adds
/// keys between releases.
[<Fact>]
let ``an event keeps keys no schema declares, and 64-bit integers keep their digits`` () =
    let row =
        eventOf
            "action"
            """{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
                "action":{"id":"a1","type":"click","brand_new_field":42},
                "context":{"cart_value":9007199254740993,"nested":{"a":[1,"b"]}},
                "unknown_top_level":{"x":1}}"""

    // 2^53+1: through a float it would come back as ...992.
    Assert.Contains("9007199254740993", row.Context)
    Assert.Contains("unknown_top_level", row.Event)
    Assert.Contains("brand_new_field", row.Event)

let private telemetryEvent (telemetry: string) : string =
    """{"_dd":{"format_version":2},"type":"telemetry","date":1591284175342,"service":"sample-sdk","source":"browser","version":"1.2.3","application":{"id":"ac8218cf"},"session":{"id":"cacbf45c"},"view":{"id":"623d50fd"},"action":{"id":"ae3a5d82"},"experimental_features":["foo"],"telemetry":"""
    + telemetry
    + "}"

[<Fact>]
let ``an error telemetry event keeps its status, message and error`` () =
    let event =
        telemetryEvent
            """{"status":"error","message":"XHR error POST https://app.datadoghq.com/api/v1/logs-analytics/aggregate?type=rum","error":{"stack":"Failed to load","kind":"TypeError"},"custom":"property"}"""

    let row = Rum.telemetryRow sender (json event) event

    Assert.Equal("telemetry", row.Type)
    Assert.Equal("error", row.Status)
    Assert.Equal("XHR error POST https://app.datadoghq.com/api/v1/logs-analytics/aggregate?type=rum", row.Message)
    Assert.Equal("Failed to load", row.ErrorStack)
    Assert.Equal("TypeError", row.ErrorKind)
    Assert.Equal(("ac8218cf", "cacbf45c"), (row.ApplicationID, row.SessionID))
    Assert.Equal<string[]>([| "foo" |], row.ExperimentalFeatures)

/// The configuration object is the answer to "why is this app sending
/// nothing?", so it is kept whole.
[<Fact>]
let ``a configuration telemetry event keeps the whole configuration`` () =
    let event =
        telemetryEvent """{"type":"configuration","configuration":{"session_sample_rate":100,"use_proxy":true}}"""

    let row = Rum.telemetryRow sender (json event) event

    Assert.Equal("configuration", row.TelemetryType)
    Assert.Contains("session_sample_rate", row.Configuration)
    Assert.Contains("use_proxy", row.Configuration)

[<Fact>]
let ``a usage telemetry event keeps the feature`` () =
    let event =
        telemetryEvent """{"type":"usage","usage":{"feature":"set-tracking-consent","tracking_consent":"granted"}}"""

    let row = Rum.telemetryRow sender (json event) event

    Assert.Equal("usage", row.TelemetryType)
    Assert.Equal("set-tracking-consent", row.UsageFeature)

/// start/end are NANOSECONDS while `date` is milliseconds. Mixing the two
/// would put a sample run a million times too far from its event.
[<Fact>]
let ``a timeseries keeps its arrays in order and its nanoseconds`` () =
    let event =
        """{"type":"timeseries","date":1591283924940,"application":{"id":"app"},
            "session":{"id":"s"},"source":"ios",
            "timeseries":{"id":"ts1","name":"memory","schema":"object-v2",
                "start":1591283924940000000,"end":1591283925940000000,
                "data":{"timestamps":[1591283924940000000,1591283925440000000],
                        "values":{"memory_footprint":[100,200]}}}}"""

    let row = Rum.timeseriesRow sender (json event) event

    Assert.Equal(("memory", "ts1", "object-v2"), (row.Name, row.ID, row.Schema))
    Assert.Equal(UnixNanos 1591283924940000000L, row.Start)
    Assert.Equal(UnixNanos 1591283925940000000L, row.End)
    Assert.Equal(Time.fromUnixMillis 1591283924940L, row.Date)
    Assert.Equal<int64[]>([| 1591283924940000000L; 1591283925440000000L |], row.Timestamps)
    Assert.Contains("memory_footprint", row.Values)

/// The span format is hand-rolled per SDK with no published schema: ids are
/// text of 64- and 128-bit numbers, meta hides two nested objects among flat
/// keys, and metrics are numbers.
[<Fact>]
let ``a span keeps its ids as text and gets its meta flattened`` () =
    let span =
        """{"trace_id":"9007199254740993","span_id":2,"parent_id":"0",
            "name":"urlsession.request","service":"app","resource":"GET /x","type":"custom",
            "start":1591283924940000000,"duration":1500000,"error":1,
            "meta":{"_dd.source":"mobile","device":{"brand":"Apple","model":"iPhone"},
                    "os":{"name":"iOS"},"tracer.version":"2.0.0","nulled":null},
            "metrics":{"_top_level":1,"_sampling_priority_v1":2}}"""

    let row = Rum.spanRow sender "prod" "" (json span) span

    Assert.Equal("9007199254740993", row.TraceID)
    // An id sent as a JSON number keeps the digits it arrived with.
    Assert.Equal("2", row.SpanID)
    Assert.Equal("prod", row.Env)
    Assert.Equal(UnixNanos 1591283924940000000L, row.Start)
    Assert.Equal(1500000L, row.DurationNS)
    Assert.Equal(1y, row.Error)

    let expectedMeta =
        Map
            [ "device.brand", "Apple"
              "device.model", "iPhone"
              "os.name", "iOS"
              "_dd.source", "mobile"
              "tracer.version", "2.0.0"
              // Sent as null is not the same as not sent.
              "nulled", "" ]

    Assert.Equal<Map<string, string>>(expectedMeta, row.Meta)
    Assert.Equal(2.0, row.Metrics["_sampling_priority_v1"])
    Assert.Equal(span, row.Span)

[<Fact>]
let ``the browser's identity is read from the query string, and the key never becomes durable`` () =
    let request =
        Rum.requestInfo (
            requestOf
                ("/api/v2/rum?ddsource=browser&dd-api-key=k&dd-evp-origin=browser"
                 + "&dd-evp-origin-version=5.23.0&dd-request-id=req-1&batch_time=1591283924940&_dd.api=beacon")
                []
        )

    Assert.Equal(("browser", "browser", "5.23.0"), (request.DDSource, request.EVPOrigin, request.EVPOriginVersion))
    Assert.Equal(("req-1", "beacon"), (request.RequestID, request.DDAPI))
    Assert.Equal(Some(Time.fromUnixMillis 1591283924940L), request.BatchTime)
    // A first attempt has no retry count: "retried zero times" is a
    // different fact.
    Assert.Equal(None, request.RetryCount)
    Assert.Equal(None, request.RetryAfter)
    Assert.Equal("", request.QueryExtra)

[<Fact>]
let ``a browser retry is read from _dd.retry_count and _dd.retry_after`` () =
    let request =
        Rum.requestInfo (requestOf "/api/v2/rum?ddsource=browser&_dd.retry_count=2&_dd.retry_after=1000" [])

    Assert.Equal(Some 2u, request.RetryCount)
    Assert.Equal(Some 1000L, request.RetryAfter)

[<Fact>]
let ``a mobile retry is the same two facts spelled inside ddtags, and identity comes from headers`` () =
    let request =
        Rum.requestInfo (
            requestOf
                "/api/v2/rum?ddsource=ios&ddtags=retry_count:3,retry_after:503"
                [ "Dd-Evp-Origin", "ios"
                  "Dd-Evp-Origin-Version", "3.17.0"
                  "Dd-Request-Id", "req-2"
                  "Dd-Idempotency-Key", "sha1-of-body" ]
        )

    Assert.Equal(("ios", "3.17.0", "req-2"), (request.EVPOrigin, request.EVPOriginVersion, request.RequestID))
    // What tells a retry from a new batch.
    Assert.Equal("sha1-of-body", request.IdempotencyKey)
    Assert.Equal(Some 3u, request.RetryCount)
    // The HTTP status that caused the retry, as the mobile SDKs send it.
    Assert.Equal(Some 503L, request.RetryAfter)

[<Fact>]
let ``a negative retry count and an unparsable batch time are absent, not zero`` () =
    let request =
        Rum.requestInfo (requestOf "/api/v2/rum?_dd.retry_count=-1&batch_time=abc&_dd.retry_after=%2B5" [])

    Assert.Equal(None, request.RetryCount)
    Assert.Equal(None, request.BatchTime)
    Assert.Equal(Some 5L, request.RetryAfter)

[<Theory>]
[<InlineData("/api/v2/rum?ddsource=browser&dd-evp-encoding=deflate", "", "deflate")>]
[<InlineData("/api/v2/rum?ddsource=android", "gzip", "gzip")>]
[<InlineData("/api/v2/rum?ddsource=browser", "", "")>]
let ``the compression signal is kept, from the query string or from Content-Encoding``
    (url: string, contentEncoding: string, expected: string)
    =
    let headers = if contentEncoding = "" then [] else [ "Content-Encoding", contentEncoding ]
    let request = Rum.requestInfo (requestOf url headers)

    Assert.Equal(expected, request.EVPEncoding)
    Assert.DoesNotContain("dd-evp-encoding", request.QueryExtra)

[<Fact>]
let ``a parameter with no column is kept, with every value it repeated`` () =
    let request = Rum.requestInfo (requestOf "/api/v2/rum?ddsource=browser&brand_new=1&brand_new=2&api_key=zzz" [])
    let extra = json request.QueryExtra

    Assert.Equal<string list>([ "1"; "2" ], [ for value in extra.GetProperty("brand_new").EnumerateArray() -> value.GetString() ])
    Assert.Equal(1, extra.EnumerateObject() |> Seq.length)

[<Fact>]
let ``the sender's address is the peer when nothing was forwarded`` () =
    let request = Rum.requestInfo (requestOf "/api/v2/rum" [ "User-Agent", "Mozilla/5.0" ])

    Assert.Equal("192.0.2.1", request.RemoteAddr)
    Assert.Equal("Mozilla/5.0", request.UserAgent)

/// Gin's ClientIP with the defaults the Go engine ran on.
[<Theory>]
[<InlineData("X-Forwarded-For", "203.0.113.7, 10.0.0.1", "203.0.113.7")>]
[<InlineData("X-Real-Ip", "198.51.100.9", "198.51.100.9")>]
[<InlineData("X-Forwarded-For", "203.0.113.7, not-an-address", "192.0.2.1")>]
let ``a forwarded address wins over the peer, unless the header holds something else``
    (name: string, value: string, expected: string)
    =
    let request = Rum.requestInfo (requestOf "/api/v2/rum" [ name, value ])
    Assert.Equal(expected, request.RemoteAddr)

let private forwarded (target: string) : string = Uri.EscapeDataString target

[<Fact>]
let ``a forwarded path and query become the request`` () =
    let http =
        httpContext "POST" ("/rum-proxy?ddforward=" + forwarded "/api/v2/rum?ddsource=browser&dd-api-key=k&dd-request-id=r1") []

    Rum.unwrapForward NullLogger.Instance http

    Assert.Equal("/api/v2/rum", http.Request.Path.Value)
    Assert.Equal("browser", http.Request.Query["ddsource"].ToString())
    Assert.Equal("k", http.Request.Query["dd-api-key"].ToString())
    Assert.Equal("r1", http.Request.Query["dd-request-id"].ToString())
    // Left in, it would be stored as an unknown parameter.
    Assert.False(http.Request.Query.ContainsKey "ddforward")

[<Fact>]
let ``an outer parameter the proxy added survives the merge, and the forwarded one wins a conflict`` () =
    let http =
        httpContext
            "POST"
            ("/rum-proxy?tenant=acme&ddsource=outer&ddforward=" + forwarded "/api/v2/logs?ddsource=browser")
            []

    Rum.unwrapForward NullLogger.Instance http

    Assert.Equal("/api/v2/logs", http.Request.Path.Value)
    Assert.Equal("acme", http.Request.Query["tenant"].ToString())
    Assert.Equal("browser", http.Request.Query["ddsource"].ToString())

[<Theory>]
[<InlineData("/api/v2/rum?ddsource=browser")>]
[<InlineData("/rum-proxy?ddforward=%3Fddsource%3Dbrowser")>]
[<InlineData("/rum-proxy?ddforward=api%2Fv2%2Frum")>]
let ``a request with no ddforward, or one that is not a path, is left as it came`` (url: string) =
    let http = httpContext "POST" url []
    let path = http.Request.Path.Value
    let query = http.Request.QueryString.Value

    Rum.unwrapForward NullLogger.Instance http

    Assert.Equal(path, http.Request.Path.Value)
    Assert.Equal(query, http.Request.QueryString.Value)

/// The proxy-as-a-string form end to end: the SDK posts to its own path and
/// everything that matters, the key included, is inside ddforward.
[<Fact>]
let ``a batch sent through ddforward is routed, admitted and stored`` () =
    let sink = CapturingSink()

    let target =
        forwarded ("/api/v2/rum?ddsource=browser&dd-api-key=" + Replay.testKey + "&dd-evp-origin=browser")

    let response = serve [] sink (httpContext "POST" ("/my-rum-proxy?ddforward=" + target) []) (utf8 action)

    Assert.Equal(202, response.Status)
    let row = Assert.Single(sink.Rows<RumEventRow>())
    Assert.Equal(("browser", "browser"), (row.Req.DDSource, row.Req.EVPOrigin))
    Assert.Equal("", row.Req.QueryExtra)

[<Theory>]
[<InlineData("/api/v2/rum")>]
// A preflight for a path that is not served must be answered too, so the
// browser gets as far as the POST and sees the 404.
[<InlineData("/api/v2/not-invented-yet")>]
let ``a preflight is answered 204 with the access-control headers and needs no key`` (path: string) =
    let http =
        httpContext
            "OPTIONS"
            path
            [ "Origin", "https://shop.example"
              "Access-Control-Request-Method", "POST"
              "Access-Control-Request-Headers", "content-type" ]

    let response = serve [] (CapturingSink()) http [||]

    Assert.Equal(204, response.Status)
    Assert.Empty response.Body
    Assert.Equal("https://shop.example", header "Access-Control-Allow-Origin" response)
    Assert.Equal("Origin", header "Vary" response)
    Assert.Contains("POST", header "Access-Control-Allow-Methods" response)
    Assert.Contains("DD-API-KEY", header "Access-Control-Allow-Headers" response)
    // Without it every request would preflight again.
    Assert.Equal("86400", header "Access-Control-Max-Age" response)

/// Without the header on a 403 or a 404 the browser blocks the read, the SDK
/// sees status 0 while online, counts the batch as delivered and drops it.
[<Theory>]
[<InlineData("POST", "/api/v2/rum?ddsource=browser&dd-api-key=wrong", 403)>]
[<InlineData("POST", "/api/v2/not-invented-yet", 404)>]
[<InlineData("GET", "/ping", 200)>]
let ``every answer carries the origin, the refusals included`` (method: string, url: string, status: int) =
    let http = httpContext method url [ "Origin", "https://shop.example" ]
    let response = serve [] (CapturingSink()) http (utf8 action)

    Assert.Equal(status, response.Status)
    Assert.Equal("https://shop.example", header "Access-Control-Allow-Origin" response)

[<Fact>]
let ``a request with no origin is not a browser and is answered with a star`` () =
    let response = post (CapturingSink()) "/api/v2/rum" [] (utf8 action)

    Assert.Equal("*", header "Access-Control-Allow-Origin" response)
    Assert.Equal("", header "Vary" response)

[<Theory>]
[<InlineData(null)>]
[<InlineData("")>]
[<InlineData("*")>]
[<InlineData(" * ")>]
[<InlineData(" , ")>]
let ``no allowlist, an empty one or a star all mean "echo the origin"`` (raw: string) =
    Assert.Empty(Rum.parseAllowedOrigins raw)

/// An allowlist is the operator's choice and is honoured exactly: an origin
/// on it is allowed, one that is not gets no header at all.
[<Theory>]
[<InlineData("https://shop.example", "https://shop.example")>]
[<InlineData("https://admin.example", "https://admin.example")>]
[<InlineData("https://evil.example", "")>]
let ``an allowlist admits its origins and gives the others no header`` (origin: string, expected: string) =
    let allowed = Rum.parseAllowedOrigins "https://shop.example, https://admin.example"
    Assert.Equal<string list>([ "https://shop.example"; "https://admin.example" ], allowed)

    let response = serve allowed (CapturingSink()) (httpContext "OPTIONS" "/api/v2/rum" [ "Origin", origin ]) [||]

    Assert.Equal(expected, header "Access-Control-Allow-Origin" response)
    Assert.Equal("Origin", header "Vary" response)

/// The fixtures Go recorded on its bare engine or behind rumEngine. The
/// golden replay runs them without the wrapper and without a peer address,
/// and overrides.json excuses the CORS headers and RemoteAddr there; here
/// they go through the wrapper with Go's peer, and both are compared.
let recordedOnTheEngine: obj[] seq =
    Replay.all
    |> List.filter (fun f -> f.RouteSets = [ "routeRUM" ] && text (child "host" (child "request" f.Json)) = "example.com")
    |> Seq.map (fun f -> [| box f.Id |])

[<Theory>]
[<MemberData(nameof recordedOnTheEngine)>]
let ``a recorded request gets Go's headers and Go's sender address through the wrapper`` (id: string) =
    let fixture = (Replay.all |> List.find (fun f -> f.Id = id)).Json
    let request = child "request" fixture
    let expected = child "response" fixture

    let headers =
        [ for pair in (child "headers" request).AsObject() -> pair.Key, text (pair.Value.AsArray()[0]) ]

    let http = httpContext (text (child "method" request)) (text (child "url" request)) headers
    let sink = CapturingSink()
    let response = serve [] sink http (Convert.FromBase64String(text (child "body_b64" request)))

    Assert.Equal((child "status" expected).GetValue<int>(), response.Status)

    for pair in (child "headers" expected).AsObject() do
        let want = [ for value in pair.Value.AsArray() -> text value ]
        let got = response.Headers |> List.filter (fun (name, _) -> name = pair.Key) |> List.map snd
        Assert.Equal<string list>(want, got)

    let sends = (child "sends" fixture).AsArray()
    let writes = sink.Writes
    Assert.Equal(sends.Count, writes.Length)

    for i in 0 .. sends.Count - 1 do
        if writes[i].Writer.StartsWith "storage_rum_" then
            let rows = (child "args" sends[i]).AsArray()
            Assert.Equal(rows.Count, writes[i].Args.Length)

            for k in 0 .. rows.Count - 1 do
                // remote_addr is the thirteenth column of every RUM table.
                let recorded = rows[k].AsArray()[12]
                Assert.Equal(text recorded, writes[i].Args[k][12] :?> string)

let private kind (event: string) : string =
    match Rum.classify (json event) with
    | Ok(Rum.View eventType) -> "rum_views:" + eventType
    | Ok(Rum.Plain eventType) -> "rum_events:" + eventType
    | Ok Rum.Telemetry -> "rum_telemetry"
    | Ok Rum.Timeseries -> "rum_timeseries"
    | Error unknownType -> "unknown:" + unknownType

/// What the Go server did with each of these, recorded from it.
[<Theory>]
[<InlineData("""{"type":"view"}""", "rum_views:view")>]
[<InlineData("""{"type":"view_update"}""", "rum_views:view_update")>]
[<InlineData("""{"type":"transition"}""", "rum_events:transition")>]
[<InlineData("""{"type":"long_task"}""", "rum_events:long_task")>]
[<InlineData("""{"type":"resource"}""", "rum_events:resource")>]
[<InlineData("""{"type":"vital","vital":{"type":"duration"}}""", "rum_events:vital")>]
[<InlineData("""{"type":"vital","vital":{"type":"operation_step"}}""", "rum_events:vital")>]
[<InlineData("""{"type":"vital","vital":{"type":"app_launch"}}""", "rum_events:vital")>]
[<InlineData("""{"type":"vital","vital":{"type":"brand_new"}}""", "unknown:vital")>]
[<InlineData("""{"type":"vital"}""", "unknown:vital")>]
[<InlineData("""{"type":"vital","vital":"x"}""", "unknown:vital")>]
[<InlineData("""{"type":"telemetry","telemetry":{"status":"error"}}""", "rum_telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":"log","status":"debug"}}""", "rum_telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":"configuration"}}""", "rum_telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":"configuration","status":"error"}}""", "rum_telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":"usage"}}""", "rum_telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":"log","status":"warn"}}""", "unknown:telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":"log"}}""", "unknown:telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":"brand_new","status":"error"}}""", "unknown:telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"type":null,"status":"error"}}""", "unknown:telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":{"status":["error"]}}""", "unknown:telemetry")>]
[<InlineData("""{"type":"telemetry"}""", "unknown:telemetry")>]
[<InlineData("""{"type":"telemetry","telemetry":5}""", "unknown:telemetry")>]
[<InlineData("""{"type":"timeseries","timeseries":{"name":"memory"}}""", "rum_timeseries")>]
[<InlineData("""{"type":"timeseries","timeseries":{"name":"cpu"}}""", "rum_timeseries")>]
[<InlineData("""{"type":"timeseries","timeseries":{"name":"battery"}}""", "unknown:timeseries")>]
[<InlineData("""{"type":"timeseries"}""", "unknown:timeseries")>]
[<InlineData("""{"type":"brand_new"}""", "unknown:brand_new")>]
[<InlineData("""{"type":7}""", "unknown:")>]
[<InlineData("""{"TYPE":"action"}""", "unknown:")>]
[<InlineData("""{}""", "unknown:")>]
let ``an event's table is decided by its discriminator fields`` (event: string, expected: string) =
    Assert.Equal(expected, kind event)

[<Fact>]
let ``blank lines and a proxy's \r\n do not make events`` () =
    let sink = CapturingSink()
    let body = "\r\n" + action + "\r\n\r\n  " + action + "  \n\n"
    let response = post sink "/api/v2/rum?ddsource=browser" [] (utf8 body)

    Assert.Equal(202, response.Status)
    let rows = sink.Rows<RumEventRow>()
    Assert.Equal(2, rows.Length)
    Assert.All(rows, (fun row -> Assert.Equal(action, row.Event)))
    Assert.Empty(sink.Rows<RawPayloadRow>())

[<Fact>]
let ``a line that is not an object is kept raw, and the rest of the batch is stored`` () =
    let sink = CapturingSink()
    let body = "[1,2]\nnull\n{bad\n" + action
    let response = post sink "/api/v2/rum?ddsource=browser" [] (utf8 body)

    Assert.Equal(202, response.Status)
    Assert.Single(sink.Rows<RumEventRow>()) |> ignore
    let raws = sink.Rows<RawPayloadRow>()
    Assert.Equal<string list>([ "[1,2]"; "null"; "{bad" ], raws |> List.map (fun raw -> Encoding.UTF8.GetString raw.Body))
    Assert.All(raws, (fun raw -> Assert.Equal(("rum", "decode_error"), (raw.Intake, raw.Reason))))

/// 202 always: a 500 would make every SDK retry the same batch forever.
[<Fact>]
let ``a date beyond year 9999 and a string that is not UTF-8 are stored, not answered with a 500`` () =
    let sink = CapturingSink()
    let farFuture = """{"type":"action","date":999999999999999999,"application":{"id":"far"}}"""

    let brokenText =
        Array.concat [ utf8 """{"type":"action","date":1,"application":{"id":" """; [| 0xffuy; 0xfeuy |]; utf8 "\"}}" ]

    let body = Array.concat [ utf8 farFuture; utf8 "\n"; brokenText ]
    let response = post sink "/api/v2/rum?batch_time=999999999999999999" [] body

    Assert.Equal(202, response.Status)
    let rows = sink.Rows<RumEventRow>()
    Assert.Equal(2, rows.Length)
    Assert.Equal(9999, rows[0].Date.Year)
    Assert.Contains("�", rows[1].ApplicationID)

[<Fact>]
let ``the agent's diagnose sweep and an empty body store nothing`` () =
    let sink = CapturingSink()

    for url in [ "/api/v2/rum"; "/api/v2/replay"; "/api/v2/spans" ] do
        let swept = post sink url [ "X-Requested-With", "datadog-agent-diagnose" ] (utf8 action)
        let empty = post sink url [] [||]
        Assert.Equal((202, 202), (swept.Status, empty.Status))
        Assert.Equal("{}", Encoding.UTF8.GetString swept.Body)

    Assert.Empty sink.Writes

[<Theory>]
[<InlineData("browser segment", "segment", "", "segment")>]
[<InlineData("mobile segment", "file0", "", "segment")>]
[<InlineData("canvas resource", "image", "resource", "resource")>]
[<InlineData("a part named something else, classified by the metadata", "blob", "resource", "resource")>]
[<InlineData("a part named something else, with nothing to go by", "blob", "", "segment")>]
let ``a replay part is a segment or a resource`` (_case: string, partName: string, metaType: string, expected: string) =
    let meta = if metaType = "" then json "{}" else json $"""{{"type":"{metaType}"}}"""
    Assert.Equal(expected, Rum.replayVariant partName meta)

[<Fact>]
let ``a browser segment is stored untouched with its metadata`` () =
    let sink = CapturingSink()

    let meta =
        """{"application":{"id":"app"},"session":{"id":"s"},"view":{"id":"v"},"source":"browser","start":1591283924940,"end":1591283925940,"records_count":12,"has_full_snapshot":true,"index_in_view":3,"creation_reason":"init","raw_segment_size":4096,"compressed_segment_size":512}"""

    let segment = [| 0x78uy; 0x9cuy; 0x00uy; 0xffuy |]
    let contentType, body = multipart [ "segment", "sess-1", segment; "event", "blob", utf8 meta ]
    let response = post sink "/api/v2/replay?ddsource=browser" [ "Content-Type", contentType ] body

    Assert.Equal(202, response.Status)
    let row = Assert.Single(sink.Rows<RumReplaySegmentRow>())
    Assert.Equal(("segment", "segment", "sess-1"), (row.Variant, row.PartName, row.PartFilename))
    Assert.Equal<byte[]>(segment, row.Segment)
    Assert.Empty row.Image
    Assert.Equal(("app", "s", "v", "browser"), (row.ApplicationID, row.SessionID, row.ViewID, row.Source))
    Assert.Equal(Some(Time.fromUnixMillis 1591283924940L), row.Start)
    Assert.Equal(Some 3L, row.IndexInView)
    Assert.Equal("init", row.CreationReason)
    Assert.Equal(meta, row.Event)

/// The mobile canvas variant repeats the part name; a map keyed by name
/// would keep only the last image and shuffle the pairing.
[<Fact>]
let ``repeated image parts each get a row, paired with metadata by position`` () =
    let sink = CapturingSink()

    let meta =
        """[{"type":"resource","application":{"id":"a0"}},{"type":"resource","application":{"id":"a1"}}]"""

    let contentType, body =
        multipart [ "image", "i0", utf8 "img0"; "image", "i1", utf8 "img1"; "event", "", utf8 meta ]

    post sink "/api/v2/replay?ddsource=android" [ "Content-Type", contentType ] body |> ignore

    let rows = sink.Rows<RumReplaySegmentRow>()
    Assert.Equal<string list>([ "a0"; "a1" ], rows |> List.map _.ApplicationID)
    Assert.Equal<string list>([ "img0"; "img1" ], rows |> List.map (fun row -> Encoding.UTF8.GetString row.Image))
    Assert.All(rows, (fun row -> Assert.Equal("resource", row.Variant)))
    Assert.Equal("""{"type":"resource","application":{"id":"a1"}}""", rows[1].Event)

[<Fact>]
let ``a blob with no metadata entry is still stored`` () =
    let sink = CapturingSink()

    let contentType, body =
        multipart [ "file0", "file0", utf8 "a"; "file1", "file1", utf8 "b"; "event", "", utf8 """[{"view":{"id":"v0"}}]""" ]

    post sink "/api/v2/replay" [ "Content-Type", contentType ] body |> ignore

    let rows = sink.Rows<RumReplaySegmentRow>()
    Assert.Equal<string list>([ "v0"; "" ], rows |> List.map _.ViewID)
    Assert.Equal("", rows[1].Event)

[<Fact>]
let ``an event part that is neither an object nor an array is kept raw, and the blob is still stored`` () =
    let sink = CapturingSink()
    let contentType, body = multipart [ "segment", "blob", utf8 "zlib"; "event", "", utf8 "not json" ]

    post sink "/api/v2/replay" [ "Content-Type", contentType ] body |> ignore

    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal(("decode_error", "not json"), (raw.Reason, Encoding.UTF8.GetString raw.Body))
    let row = Assert.Single(sink.Rows<RumReplaySegmentRow>())
    Assert.Equal("", row.Event)

[<Theory>]
[<InlineData("application/json", "replay body is not multipart: content-type is application/json")>]
[<InlineData("", "replay body is not multipart: mime: no media type")>]
[<InlineData("multipart/form-data", "replay body is not multipart: multipart without boundary")>]
let ``a replay body that is not multipart is kept raw`` (contentType: string, note: string) =
    let sink = CapturingSink()
    let headers = if contentType = "" then [] else [ "Content-Type", contentType ]
    let response = post sink "/api/v2/replay" headers (utf8 """{"a":1}""")

    Assert.Equal(202, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal(("rum", "unexpected_shape", note), (raw.Intake, raw.Reason, raw.Note))
    Assert.Empty(sink.Rows<RumReplaySegmentRow>())

[<Fact>]
let ``a multipart body that breaks off is kept raw whole, not stored in part`` () =
    let sink = CapturingSink()
    let contentType, body = multipart [ "segment", "blob", utf8 "zlib"; "event", "", utf8 """{"view":{"id":"v"}}""" ]
    let truncated = body[.. body.Length - 21]

    post sink "/api/v2/replay" [ "Content-Type", contentType ] truncated |> ignore

    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("unexpected_shape", raw.Reason)
    Assert.Equal<byte[]>(truncated, raw.Body)
    Assert.Empty(sink.Rows<RumReplaySegmentRow>())

[<Fact>]
let ``an envelope without spans and a span that is not an object are kept raw`` () =
    let sink = CapturingSink()

    let body =
        """{"env":"prod"}"""
        + "\n"
        + """{"spans":{"a":1}}"""
        + "\n"
        + """{"spans":["str",{"trace_id":"t1"}],"env":"prod","new_key":{"x":1}}"""

    let response = post sink "/api/v2/spans?ddsource=ios" [] (utf8 body)

    Assert.Equal(202, response.Status)

    Assert.Equal<(string * string) list>(
        [ "unexpected_shape", """{"env":"prod"}"""
          "decode_error", """{"spans":{"a":1}}"""
          "decode_error", "\"str\"" ],
        sink.Rows<RawPayloadRow>() |> List.map (fun raw -> raw.Reason, Encoding.UTF8.GetString raw.Body)
    )

    let row = Assert.Single(sink.Rows<RumSpanRow>())
    Assert.Equal(("t1", "prod"), (row.TraceID, row.Env))
    Assert.Equal("""{"new_key":{"x":1}}""", row.EnvelopeExtra)

[<Fact>]
let ``a span's meta keeps numbers, booleans and arrays as text, and its metrics keep only numbers`` () =
    let span =
        """{"trace_id":9007199254740993,"error":300,"meta":{"num":1.50,"yes":true,"arr":[1,"a"],"empty":{},"device":{"deep":{"er":"x"}}},"metrics":{"a":1,"b":2.5,"c":"x","d":null}}"""

    let row = Rum.spanRow sender "" "" (json span) span

    Assert.Equal("9007199254740993", row.TraceID)
    // Go's int8 conversion wraps.
    Assert.Equal(44y, row.Error)

    Assert.Equal<Map<string, string>>(
        Map [ "num", "1.50"; "yes", "true"; "arr", """[1,"a"]"""; "device.deep.er", "x" ],
        row.Meta
    )

    Assert.Equal<Map<string, float>>(Map [ "a", 1.0; "b", 2.5 ], row.Metrics)

/// Column lists and value lists are written apart and can only disagree at
/// insert time otherwise.
[<Fact>]
let ``every RUM table gives one value per column`` () =
    let empty = json "{}"

    let part: MultipartPart =
        { Name = "segment"
          FileName = ""
          ContentType = ""
          Data = [||] }

    let arity (table: Table<'row>) (row: 'row) : string * int * int =
        table.Name, table.Columns.Length, (table.Values row).Length

    let tables =
        [ arity RumViews.table (Rum.viewRow sender "view" empty "{}")
          arity RumEvents.table (Rum.eventRow sender "action" empty "{}")
          arity RumTelemetry.table (Rum.telemetryRow sender empty "{}")
          arity RumTimeseries.table (Rum.timeseriesRow sender empty "{}")
          arity RumReplaySegments.table (Rum.replayRow sender part empty "{}")
          arity RumSpans.table (Rum.spanRow sender "" "" empty "{}") ]

    Assert.Equal<string list>(
        [ "rum_views"; "rum_events"; "rum_telemetry"; "rum_timeseries"; "rum_replay_segments"; "rum_spans" ],
        tables |> List.map (fun (name, _, _) -> name)
    )

    for name, columns, values in tables do
        Assert.True((columns = values), $"{name}: {columns} columns, {values} values")
