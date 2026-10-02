/// The syntax tree of Datadog's metric query language — the string behind
/// `GET /api/v1/query?query=`, a dashboard widget's `q`, and the metric part of
/// a monitor.
///
/// This is syntax only. Nothing here knows whether `ewma_7` exists, whether
/// `top` takes four arguments, or whether `p95:` is legal on a gauge: those
/// checks need the function catalog and the metric's type, and belong to the
/// analysis step after parsing. Keeping them out is what lets the parser accept
/// the corners of the language Datadog never documented.
///
/// Grammar and sources: api/docs/metric-query-language.md.
module NinjaCat.Api.Engine.MetricQuery.Ast

/// One condition inside `{...}`.
///
/// Datadog has two surface syntaxes for the same thing — symbolic
/// (`env:prod,!host:a`) and functional (`env:prod AND NOT host:a`) — and
/// forbids mixing them. Both parse into this one tree; the printer picks the
/// symbolic form whenever the tree can be written in it.
type TagFilter =
    /// `{*}`, and also `{}`.
    | All
    /// `key:value`. The value may contain `*` wildcards, kept verbatim.
    | Tag of key: string * value: string
    /// A tag with no key, e.g. `{prod}` or `{web-*}`.
    | Bare of value: string
    /// `key IN (a, b)`.
    | In of key: string * values: string list
    | Not of TagFilter
    | And of TagFilter list
    | Or of TagFilter list

/// A plain value: what modifiers and named arguments take.
type Literal =
    /// `60`, `0.5`.
    | Num of float
    /// `'mean'` or `"UTC"` — the quote style is not kept.
    | Quoted of string
    /// A bare word: `sum` in `.rollup(sum, 60)`, `zero` in `.fill(zero)`.
    | Word of string

/// A `.name(args)` suffix on a metric query: `rollup`, `as_count`, `fill`, …
type Modifier = { Name: string; Args: Literal list }

/// The atom of the language: one metric, filtered, grouped and modified.
type MetricQuery =
    {
        /// `avg` in `avg:system.cpu.user{*}`; `p95` for distributions. None
        /// means the default, which depends on the metric type.
        SpaceAgg: string option
        Metric: string
        Filter: TagFilter
        GroupBy: string list
        /// In source order; the order is meaningful to Datadog, so it is kept.
        Modifiers: Modifier list
    }

type BinaryOp =
    | Add
    | Sub
    | Mul
    | Div

/// Arithmetic and function calls over some kind of leaf.
///
/// The leaf is a parameter because Datadog has two languages with the same
/// arithmetic and functions and different bottoms:
///
///   v1 query     (avg:a{*} - avg:b{*}) / avg:a{*}     leaf = MetricQuery
///   v2 formula   (query1 - query2) / query1           leaf = a query's name
type Expr<'Leaf> =
    | Leaf of 'Leaf
    | Number of float
    | Neg of Expr<'Leaf>
    | Binary of BinaryOp * Expr<'Leaf> * Expr<'Leaf>
    /// `abs(...)`, `top(...)`, `timeshift(...)` — every function that wraps a
    /// query or an expression.
    | Call of name: string * args: Arg<'Leaf> list

/// An argument to a function.
and Arg<'Leaf> =
    /// Anything that yields series or a number: `avg:x{*}`, `query1`, `10`,
    /// `a / 2`.
    | Value of Expr<'Leaf>
    /// `'mean'`, `desc`. Numbers are never here: they are expressions
    /// (`-3600`, `2 * 60`) and arrive as `Value`.
    | Lit of Literal
    /// `direction='above'`, as the algorithm functions take them.
    | Named of name: string * value: Literal

/// A v2 `formula`: expressions over query names.
type Formula = Expr<string>

/// A whole v1 `query=` value. It accepts several expressions separated by
/// commas; each becomes its own group of series, tagged with its `query_index`.
type Program = Expr<MetricQuery> list
