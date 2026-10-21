//! Datadog's v2 query API as it is on the wire.
//!
//! Every type is named after its schema in Datadog's OpenAPI spec (datadog-api-client,
//! `.generator/schemas/v2/openapi.yaml`), so one grep finds it there and in every generated
//! client. Nothing here is validated; that is what `timeseries` and `scalar` do with it. Unknown
//! fields are ignored: dashboards also send `semantic_mode`, `cross_org_uuids` and the like.
//! Required fields are options, so that their absence is ours to report.

use serde::{Deserialize, Serialize};

// --- the timeseries request ------------------------------------------------------------------

/// One element of `queries[]`.
///
/// In the spec that element is `TimeseriesQuery`, a oneOf of metrics, events, APM, process…
/// queries. Only the metrics variant is modelled; the others are told apart by `data_source` and
/// refused before their own fields matter.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct MetricsTimeseriesQuery {
    pub data_source: Option<String>,
    /// Optional for metrics queries.
    pub name: Option<String>,
    pub query: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct FormulaLimit {
    pub count: Option<i32>,
    /// `QuerySortOrder`: "asc" or "desc", default "desc".
    pub order: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct QueryFormula {
    pub formula: Option<String>,
    pub limit: Option<FormulaLimit>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct TimeseriesFormulaRequestAttributes {
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// A hint: Datadog may widen it, and picks one itself when absent.
    pub interval: Option<i64>,
    pub queries: Option<Vec<MetricsTimeseriesQuery>>,
    pub formulas: Option<Vec<QueryFormula>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct TimeseriesFormulaRequest {
    /// `TimeseriesFormulaRequestType`: always "timeseries_request".
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub attributes: Option<TimeseriesFormulaRequestAttributes>,
}

/// The request body of `POST /api/v2/query/timeseries`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct TimeseriesFormulaQueryRequest {
    pub data: Option<TimeseriesFormulaRequest>,
}

// --- the timeseries response -----------------------------------------------------------------

/// One series: which formula (or query) it answers and which group it is. `values[i]` in the
/// attributes belongs to `series[i]`.
///
/// `unit` is left out for now: the spec makes it optional and nullable, and the metric's unit is
/// not looked up yet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimeseriesResponseSeries {
    /// `["host:web-1", "env:prod"]`, in the order of the query's `by`.
    pub group_tags: Vec<String>,
    pub query_index: usize,
}

/// NinjaCat's own: the band anomalies() draws around a series, aligned with `times`. Null where
/// the band is not known (too little history before).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NinjacatBounds {
    pub upper: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
}

/// NinjaCat's own: what forecast() predicts past the window. Its own times, all after `to`, so
/// `times` and `values` keep meaning the window only.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NinjacatForecast {
    pub times: Vec<i64>,
    pub values: Vec<f64>,
    pub upper: Vec<f64>,
    pub lower: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimeseriesResponseAttributes {
    pub series: Vec<TimeseriesResponseSeries>,
    /// Bucket starts, unix milliseconds, shared by every series. Only buckets where some series
    /// has data: Datadog's recorded responses skip the rest.
    pub times: Vec<i64>,
    /// One row per series, one value per time; null where that series has no data in that bucket.
    pub values: Vec<Vec<Option<f64>>>,
    /// Parallel to `series`, present only when a formula asked for it.
    ///
    /// Not in Datadog's spec, which has no field for a band or a forecast at all. Fields of our
    /// own are safe to add: datadog-api-client-go keeps unknown fields in AdditionalProperties,
    /// and the Python client's models accept them by default. The `ninjacat_` prefix keeps them
    /// clear of any field Datadog may add later.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ninjacat_bounds: Option<Vec<Option<NinjacatBounds>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ninjacat_forecast: Option<Vec<Option<NinjacatForecast>>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimeseriesResponse {
    /// Not in the spec, but every recorded Datadog response carries "0".
    pub id: String,
    /// `TimeseriesFormulaResponseType`: always "timeseries_response".
    #[serde(rename = "type")]
    pub kind: String,
    pub attributes: TimeseriesResponseAttributes,
}

/// The response body of `POST /api/v2/query/timeseries`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimeseriesFormulaQueryResponse {
    pub data: TimeseriesResponse,
}

// --- the scalar request ----------------------------------------------------------------------

/// One element of a scalar request's `queries[]` (spec: the metrics variant of ScalarQuery).
/// `aggregator` is required: how the query's window becomes one value, `MetricsAggregator`: avg,
/// min, max, sum, last, percentile, mean, l2norm or area.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct MetricsScalarQuery {
    pub data_source: Option<String>,
    pub name: Option<String>,
    pub query: Option<String>,
    pub aggregator: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ScalarFormulaRequestAttributes {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub queries: Option<Vec<MetricsScalarQuery>>,
    pub formulas: Option<Vec<QueryFormula>>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ScalarFormulaRequest {
    /// `ScalarFormulaRequestType`: always "scalar_request".
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub attributes: Option<ScalarFormulaRequestAttributes>,
}

/// The request body of `POST /api/v2/query/scalar`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ScalarFormulaQueryRequest {
    pub data: Option<ScalarFormulaRequest>,
}

// --- the scalar response ---------------------------------------------------------------------

/// A column of group tags: `name` is the tag key, and `values` holds, per row, that tag's values
/// (an array, as a tag can hold several).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GroupScalarColumn {
    pub name: String,
    /// Always "group".
    #[serde(rename = "type")]
    pub kind: String,
    pub values: Vec<Vec<String>>,
}

/// `ScalarMeta`. `unit` is left out for now, as on timeseries series.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScalarMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// A column of numbers: one formula's value per row.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DataScalarColumn {
    pub name: String,
    /// Always "number".
    #[serde(rename = "type")]
    pub kind: String,
    /// Null where a row's group has no value for this formula.
    pub values: Vec<Option<f64>>,
    pub meta: ScalarMeta,
}

/// `ScalarColumn`, a oneOf: each serialized by its own shape.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ScalarColumn {
    Group(GroupScalarColumn),
    Data(DataScalarColumn),
}

/// The spec's own spelling of the schema name is kept.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScalarFormulaResponseAtrributes {
    pub columns: Vec<ScalarColumn>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScalarResponse {
    /// Always "scalar_response".
    #[serde(rename = "type")]
    pub kind: String,
    pub attributes: ScalarFormulaResponseAtrributes,
}

/// The response body of `POST /api/v2/query/scalar`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ScalarFormulaQueryResponse {
    pub data: ScalarResponse,
}
