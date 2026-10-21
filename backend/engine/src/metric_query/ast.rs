//! The syntax tree of Datadog's metric query language: the string behind `GET /api/v1/query`, a
//! dashboard widget's `q`, and the metric part of a monitor.
//!
//! This is syntax only. Whether `ewma_7` exists, whether `top` takes four arguments, or whether
//! `p95:` is legal on a gauge is the plan's to check (`query::plan`); keeping it out is what lets
//! the parser accept the corners of the language Datadog never documented.

/// One condition inside `{...}`.
///
/// Datadog has two surface syntaxes for the same thing, symbolic (`env:prod,!host:a`) and
/// functional (`env:prod AND NOT host:a`), and forbids mixing them. Both parse into this one tree.
#[derive(Clone, Debug, PartialEq)]
pub enum TagFilter {
    /// `{*}`, and also `{}`.
    All,
    /// `key:value`. The value may contain `*` wildcards, kept verbatim.
    Tag(String, String),
    /// A tag with no key, e.g. `{prod}` or `{web-*}`.
    Bare(String),
    /// `key IN (a, b)`.
    In(String, Vec<String>),
    Not(Box<TagFilter>),
    And(Vec<TagFilter>),
    Or(Vec<TagFilter>),
}

/// A plain value: what modifiers and named arguments take.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    /// `60`, `0.5`.
    Num(f64),
    /// `'mean'` or `"UTC"`; the quote style is not kept.
    Quoted(String),
    /// A bare word: `sum` in `.rollup(sum, 60)`, `zero` in `.fill(zero)`.
    Word(String),
}

/// A `.name(args)` suffix on a metric query: `rollup`, `as_count`, `fill`, …
#[derive(Clone, Debug, PartialEq)]
pub struct Modifier {
    pub name: String,
    pub args: Vec<Literal>,
}

/// The atom of the language: one metric, filtered, grouped and modified.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricQuery {
    /// `avg` in `avg:system.cpu.user{*}`; `p95` for distributions. None means the default, which
    /// depends on the metric type.
    pub space_agg: Option<String>,
    pub metric: String,
    pub filter: TagFilter,
    pub group_by: Vec<String>,
    /// In source order; the order is meaningful to Datadog, so it is kept.
    pub modifiers: Vec<Modifier>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Arithmetic and function calls over some kind of leaf.
///
/// The leaf is a parameter because Datadog has two languages with the same arithmetic and
/// functions and different bottoms:
///
/// ```text
/// v1 query     (avg:a{*} - avg:b{*}) / avg:a{*}     leaf = MetricQuery
/// v2 formula   (query1 - query2) / query1           leaf = a query's name
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum Expr<L> {
    Leaf(L),
    Number(f64),
    Neg(Box<Expr<L>>),
    Binary(BinaryOp, Box<Expr<L>>, Box<Expr<L>>),
    /// `abs(...)`, `top(...)`, `timeshift(...)`: every function that wraps a query or an
    /// expression.
    Call(String, Vec<Arg<L>>),
}

/// An argument to a function.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg<L> {
    /// Anything that yields series or a number: `avg:x{*}`, `query1`, `10`, `a / 2`.
    Value(Expr<L>),
    /// `'mean'`, `desc`. Numbers are never here: they are expressions (`-3600`, `2 * 60`) and
    /// arrive as `Value`.
    Lit(Literal),
    /// `direction='above'`, as the algorithm functions take them.
    Named(String, Literal),
}

/// A v2 `formula`: expressions over query names.
pub type Formula = Expr<String>;

/// A whole v1 `query=` value. It accepts several expressions separated by commas; each becomes its
/// own group of series, tagged with its `query_index`.
pub type Program = Vec<Expr<MetricQuery>>;
