# Pointing a RUM SDK at ninjacat

Datadog serves browser, iOS and Android RUM from **one** host,
`browser-intake-<site>` — there is no separate mobile intake. ninjacat mirrors
that: everything goes to `browser-intake.<your-site>`, and the differences
between the platforms (where the key lives, how compression is signalled) are
handled inside the router, not by giving each platform its own name.

What lands where is in [docs/tables/rum.md](tables/rum.md). The router is
`intake/router_rum.go`.

## The endpoints

All of them answer **202** with `{}`.

| path | body | who sends it |
|---|---|---|
| `POST /api/v2/rum` | NDJSON, one event per line | browser, iOS, Android (events, SDK telemetry, crashes, ANRs) |
| `POST /api/v2/logs` | NDJSON (browser) or JSON array (mobile) | all three |
| `POST /api/v2/replay` | multipart | Session Replay segments and canvas resources |
| `POST /api/v2/spans` | NDJSON of `{"spans":[…],"env":…}` | iOS, Android only |
| `POST /api/v2/profile` | multipart | browser profiler, mobile profiling |
| `POST /api/v2/debugger` | NDJSON or multipart | browser dynamic instrumentation |
| `GET /ping`, `GET /_health` | — | you, checking the host is up. No key needed |

**202, not 200, and that is not a style choice.** Android logs a `200` as
`UnknownHttpError` and drops the batch; iOS treats anything but `202` as
unexpected and drops it silently; the browser accepts anything outside
408/429/5xx. `202` is the only answer all three agree means "kept".

## TLS is not optional for mobile

iOS App Transport Security and Android's `ConnectionSpec.RESTRICTED_TLS` both
require HTTPS with a certificate from a public CA. A self-signed certificate or
a plain `http://` endpoint is rejected **on the device**, before a request is
made, and the failure appears only in the app's own logs. Put a real certificate
in front of `browser-intake.<your-site>`.

## Browser (`@datadog/browser-rum`, `@datadog/browser-logs`)

Use `proxy` **as a function**. This is public SDK API, not a workaround: the
request stays byte-identical to native Datadog traffic — same `/api/v2/<track>`
path, same query parameters, the key readable without unwrapping anything.

```js
import { datadogRum } from '@datadog/browser-rum'

datadogRum.init({
  applicationId: '<your application id>',
  clientToken: '<your ninjacat API key>',
  proxy: ({ path, parameters }) =>
    `https://browser-intake.example.com${path}?${parameters}`,
  service: 'shop-web',
  env: 'prod',
  sessionSampleRate: 100,
})
```

`site` cannot be used to redirect the browser SDK: it is validated against
`/(datadog|ddog|datad0g|dd0g)/`, and the host it composes is a different
registrable domain anyway.

`proxy` as a **string** also works — the SDK then posts to
`<proxy>?ddforward=<url-encoded path and query>` and we unwrap it — but the
function form is what to document to your own developers. A relative
`proxy: '/rum'` resolves against `document.baseURI`, which is same-origin and
needs no CORS at all; that is the simplest deployment if you already have a
reverse proxy.

### CORS

Nothing to configure for the common case: the server echoes the request's
`Origin` on every response, including its own 403s. To restrict it, set

```
NINJACAT_RUM_ALLOWED_ORIGINS=https://shop.example,https://admin.example
```

Unset, empty or `*` means echo. An origin that is not on the list gets no
`Access-Control-Allow-Origin` header, and its data is refused by the browser.

Be deliberate about that, because **a CORS failure is invisible**. The SDK
retries only on 408, 429 and 5xx, or on a network error while
`navigator.onLine === false`. A blocked cross-origin response reaches it as
status 0 while online, which it treats as *success* — it discards the batch and
tells nobody. An origin missing from the list is a silent, total loss of that
origin's data, not an error you will find in a log.

The actual data requests are "simple requests" (`text/plain` and
`multipart/form-data` bodies, no custom headers) and never preflight. The server
answers any `OPTIONS` on this host with 204 and a 24-hour
`Access-Control-Max-Age`, for the deployments that do preflight.

The access-control headers go on **every** response this host produces — a 202,
a 403 for a bad key, a 404 for a path this build does not serve — because they
are set above the router, not inside the authenticated route group. A path we do
not know yet (a track a newer SDK added) therefore answers 404 with the header,
which the SDK can see and report, instead of a blocked response it would count
as a success.

### Compression

The browser sends **no** `Content-Encoding` header. Its only signal is
`?dd-evp-encoding=deflate`, which the router reads itself. Nothing to configure.
Either spelling is kept in the `evp_encoding` column, because the payload is
stored decoded and nothing else would remember that the sender compressed.
With `compressIntakeRequests: true` and the page unloading mid-encode, the SDK
sends two requests — one compressed, one plain; both are handled independently.

## iOS (`dd-sdk-ios`)

Every module has its own `customEndpoint`, and it replaces the **entire URL,
including the path**, with no validation. Give each module its full path — a
bare host produces `POST /?ddsource=ios`, which we answer with a 404 naming the
path, on purpose.

```swift
Datadog.initialize(
    with: Datadog.Configuration(
        clientToken: "<your ninjacat API key>",
        env: "prod",
        service: "shop-ios"
    ),
    trackingConsent: .granted
)

RUM.enable(with: RUM.Configuration(
    applicationID: "<your application id>",
    customEndpoint: URL(string: "https://browser-intake.example.com/api/v2/rum")!
))

Logs.enable(with: Logs.Configuration(
    customEndpoint: URL(string: "https://browser-intake.example.com/api/v2/logs")!
))

Trace.enable(with: Trace.Configuration(
    customEndpoint: URL(string: "https://browser-intake.example.com/api/v2/spans")!
))

SessionReplay.enable(with: SessionReplay.Configuration(
    replaySampleRate: 100,
    customEndpoint: URL(string: "https://browser-intake.example.com/api/v2/replay")!
))
```

The key goes in the `DD-API-KEY` header; compression is `Content-Encoding:
deflate` (or absent, when compressing would enlarge a small batch) and is
handled by the shared middleware.

## Android (`dd-sdk-android`)

Same shape, `useCustomEndpoint` per channel, also unvalidated and also
replacing host and path.

```kotlin
Datadog.initialize(
    context,
    Configuration.Builder(
        clientToken = "<your ninjacat API key>",
        env = "prod",
        service = "shop-android"
    ).build(),
    TrackingConsent.GRANTED
)

RumFeature: Rum.enable(
    RumConfiguration.Builder("<your application id>")
        .useCustomEndpoint("https://browser-intake.example.com/api/v2/rum")
        .build()
)

Logs.enable(
    LogsConfiguration.Builder()
        .useCustomEndpoint("https://browser-intake.example.com/api/v2/logs")
        .build()
)

Trace.enable(
    TraceConfiguration.Builder()
        .useCustomEndpoint("https://browser-intake.example.com/api/v2/spans")
        .build()
)

SessionReplay.enable(
    SessionReplayConfiguration.Builder(100f)
        .useCustomEndpoint("https://browser-intake.example.com/api/v2/replay")
        .build()
)
```

Android compresses with `Content-Encoding: gzip` (not deflate), which the shared
middleware already handles. Multipart bodies — replay, profiling — are never
gzipped at the HTTP layer.

## What arrives, and what that means operationally

- **Old timestamps are correct, not a defect.** All three mobile SDKs persist
  events to disk before sending. A batch can carry events up to eighteen hours
  old: an offline device, `TrackingConsent.pending`, or a crash from a previous
  launch reported at the next one. We never clamp or rewrite `date`; arrival is
  recorded separately in `received_at`.
- **A view arrives many times.** The same `view.id` recurs with a growing
  `_dd.document_version`; `rum_views` is a `ReplacingMergeTree` keyed on it.
  Query it with `FINAL`.
- **A WebView event inside a mobile batch keeps `source:"browser"`** even though
  the request says `ios`/`android`. Do not assume the event's source matches the
  request's origin.
- **SDK telemetry arrives on `/api/v2/rum` too**, from every SDK, including one
  configured for Logs only. It goes to `rum_telemetry` and is the first place to
  look when an app "sends nothing".

## Open items

1. **Client tokens are API keys today.** A `clientToken` is embedded in a public
   web page or a shipped app binary, so by design it should be **write-only** —
   able to submit data, never to authorise a read. `api_key` has no scope column
   yet (`frontend/src/lib/server/db/schema.ts`, `apps/apikeys.Key`), so a RUM
   credential currently carries full rights. The intended fix is additive: a
   `scope` column defaulting to `'full'`, propagated through
   `apikeys.Keeper.fetch()`, checked by the RUM gate — a `write_only` key
   refused on every non-ingest endpoint, a `full` key still accepted for ingest
   so nobody has to mint two. Until then, **mint a dedicated key per RUM
   application** so it can be revoked on its own.
2. **A key in a URL leaks.** Browser traffic has no alternative — its `fetch`
   sets zero headers so the request stays a simple request and stays
   interchangeable with `sendBeacon` — but the key lands in every proxy access
   log in front of us. That is an argument for keeping the write-only scope
   narrow once it exists, and for not reusing an agent key here.
3. **`/api/v2/logs` from an SDK loses its timestamp.** `HandleLogs` reads a
   `timestamp` attribute; the SDKs write `date` — a millisecond number on
   browser and iOS, an ISO-8601 **string** on Android. Every SDK log currently
   falls back to arrival time. The fix belongs in `router_logs.go`.
4. **Browser `/api/v2/debugger` is NDJSON**, while `HandleDebugger`'s JSON
   variant expects an array. The same framing gap as above, in
   `router_profiling.go`.
5. **Android's `/api/v0.2/stats`** (3.14+, opt-in, msgpack) may be consumable by
   the existing `HandleAPMStats` — path and header auth match the agent's — but
   nobody has compared the bytes against `pb.ClientStatsPayload`. Capture a
   sample first (`DEBUG=true`, see the capture lab).
6. **`/api/v2/spans` placement.** It is routed with RUM because it shares a
   host, a credential and a sender with mobile RUM and nothing with the agent's
   msgpack traces. Grouping it with the trace tables instead is a defensible
   alternative and remains an open call.
