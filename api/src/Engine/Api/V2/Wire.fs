/// Datadog's v2 query API, as it is on the wire.
///
/// Every type here is named after its schema in Datadog's OpenAPI spec
/// (datadog-api-client, .generator/schemas/v2/openapi.yaml), so one grep finds
/// it there and in every generated client. Nothing here is validated; that is
/// what Timeseries.fs does with it.
///
/// Field names become snake_case through Json.options. Unknown fields are
/// ignored — dashboards also send `semantic_mode`, `cross_org_uuids` and the
/// like. Required strings may still arrive as null and `from`/`to` are options:
/// the deserializer does not enforce presence (see Json.fs), so the reader of
/// each type does.
module NinjaCat.Api.Engine.Api.V2.Wire

/// One element of `queries[]`.
///
/// In the spec that element is `TimeseriesQuery`, a oneOf of metrics, events,
/// APM, process… queries. Only the metrics variant is modelled; the others are
/// told apart by `data_source` and refused before their own fields matter.
type MetricsTimeseriesQuery =
    { DataSource: string
      /// Optional for metrics queries.
      Name: string option
      Query: string }

type FormulaLimit =
    { Count: int option
      /// `QuerySortOrder`: "asc" or "desc", default "desc".
      Order: string option }

type QueryFormula =
    { Formula: string
      Limit: FormulaLimit option }

type TimeseriesFormulaRequestAttributes =
    { From: int64 option
      To: int64 option
      /// A hint: Datadog may widen it, and picks one itself when absent.
      Interval: int64 option
      Queries: MetricsTimeseriesQuery list
      Formulas: QueryFormula list option }

type TimeseriesFormulaRequest =
    { /// `TimeseriesFormulaRequestType`: always "timeseries_request".
      Type: string
      Attributes: TimeseriesFormulaRequestAttributes }

/// The request body of `POST /api/v2/query/timeseries`.
type TimeseriesFormulaQueryRequest = { Data: TimeseriesFormulaRequest }

// --- the response ------------------------------------------------------------------

/// One series: which formula (or query) it answers and which group it is.
/// `values[i]` in the attributes belongs to `series[i]`.
///
/// `unit` is left out for now: the spec makes it optional and nullable, and
/// the metric's unit is not looked up yet.
type TimeseriesResponseSeries =
    { /// `["host:web-1", "env:prod"]`, in the order of the query's `by`.
      GroupTags: string list
      QueryIndex: int }

type TimeseriesResponseAttributes =
    { Series: TimeseriesResponseSeries list
      /// Bucket starts, unix milliseconds, shared by every series. Only buckets
      /// where some series has data — Datadog's recorded responses skip the rest.
      Times: int64 list
      /// One row per series, one value per time; null where that series has
      /// no data in that bucket.
      Values: float option list list }

type TimeseriesResponse =
    { /// Not in the spec, but every recorded Datadog response carries "0".
      Id: string
      /// `TimeseriesFormulaResponseType`: always "timeseries_response".
      Type: string
      Attributes: TimeseriesResponseAttributes }

/// The response body of `POST /api/v2/query/timeseries`.
type TimeseriesFormulaQueryResponse = { Data: TimeseriesResponse }
