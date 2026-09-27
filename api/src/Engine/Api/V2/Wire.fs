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

/// NinjaCat's own: the band anomalies() draws around a series, aligned with
/// `times`. Null where the band is not known (too little history before).
type NinjacatBounds =
    { Upper: float option list
      Lower: float option list }

/// NinjaCat's own: what forecast() predicts past the window. Its own times,
/// all after `to`, so `times` and `values` keep meaning the window only.
type NinjacatForecast =
    { Times: int64 list
      Values: float list
      Upper: float list
      Lower: float list }

type TimeseriesResponseAttributes =
    { Series: TimeseriesResponseSeries list
      /// Bucket starts, unix milliseconds, shared by every series. Only buckets
      /// where some series has data — Datadog's recorded responses skip the rest.
      Times: int64 list
      /// One row per series, one value per time; null where that series has
      /// no data in that bucket.
      Values: float option list list
      /// Parallel to `series`, present only when a formula asked for it.
      ///
      /// Not in Datadog's spec, which has no field for a band or a forecast
      /// at all — its UI must get them some other way. Fields of our own are
      /// safe to add: datadog-api-client-go keeps unknown fields in
      /// AdditionalProperties, and the Python client's models accept them by
      /// default. The `ninjacat_` prefix keeps them clear of any field
      /// Datadog may add later.
      NinjacatBounds: NinjacatBounds option list option
      NinjacatForecast: NinjacatForecast option list option }

type TimeseriesResponse =
    { /// Not in the spec, but every recorded Datadog response carries "0".
      Id: string
      /// `TimeseriesFormulaResponseType`: always "timeseries_response".
      Type: string
      Attributes: TimeseriesResponseAttributes }

/// The response body of `POST /api/v2/query/timeseries`.
type TimeseriesFormulaQueryResponse = { Data: TimeseriesResponse }

// --- scalar: the request -----------------------------------------------------------

/// One element of a scalar request's `queries[]` (spec: the metrics variant of
/// ScalarQuery). `aggregator` is required: how the query's window becomes one
/// value — `MetricsAggregator`: avg, min, max, sum, last, percentile, mean,
/// l2norm or area.
type MetricsScalarQuery =
    { DataSource: string
      Name: string option
      Query: string
      Aggregator: string }

type ScalarFormulaRequestAttributes =
    { From: int64 option
      To: int64 option
      Queries: MetricsScalarQuery list
      Formulas: QueryFormula list option }

type ScalarFormulaRequest =
    { /// `ScalarFormulaRequestType`: always "scalar_request".
      Type: string
      Attributes: ScalarFormulaRequestAttributes }

/// The request body of `POST /api/v2/query/scalar`.
type ScalarFormulaQueryRequest = { Data: ScalarFormulaRequest }

// --- scalar: the response ----------------------------------------------------------

/// A column of group tags: `name` is the tag key, and `values` holds, per row,
/// that tag's values (an array, as a tag can hold several).
type GroupScalarColumn =
    { Name: string
      /// Always "group".
      Type: string
      Values: string list list }

/// `ScalarMeta`. `unit` is left out for now, as on timeseries series.
type ScalarMeta = { Unit: string option }

/// A column of numbers: one formula's value per row.
type DataScalarColumn =
    { Name: string
      /// Always "number".
      Type: string
      /// Null where a row's group has no value for this formula.
      Values: float option list
      Meta: ScalarMeta }

type ScalarFormulaResponseAtrributes =
    { /// `ScalarColumn`, a oneOf: GroupScalarColumn or DataScalarColumn,
      /// serialized by their own shapes.
      Columns: obj list }

type ScalarResponse =
    { /// Always "scalar_response".
      Type: string
      /// The spec's own spelling of the schema name is kept above.
      Attributes: ScalarFormulaResponseAtrributes }

/// The response body of `POST /api/v2/query/scalar`.
type ScalarFormulaQueryResponse = { Data: ScalarResponse }
