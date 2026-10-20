//! The parser of the metric query language, written with winnow. (F#: `MetricQuery/Parser.fs`,
//! written with FParsec.)
//!
//! Three layers, each built on the one before: the tag filter inside `{...}`; a metric query,
//! `avg:system.cpu.user{env:prod} by {host}.rollup(sum, 60)`; expressions, arithmetic and function
//! calls over metric queries (v1 `query=`, monitors) or over query names (v2 formulas).
//!
//! The grammar keeps FParsec's rule: once a parser has consumed input, a failure after it is final
//! and no alternative is tried (`cut_err`). `attempt` marks each place that rewinds instead, and
//! says why that place needs it.

use std::fmt;

use winnow::ModalResult;
use winnow::Parser;
use winnow::ascii::{digit0, digit1};
use winnow::combinator::{alt, cut_err, eof, not, opt, peek, preceded, repeat, terminated};
use winnow::error::{ContextError, ErrMode, FromExternalError, StrContext, StrContextValue};
use winnow::token::{one_of, take_while};

use crate::metric_query::ast::{Arg, BinaryOp, Expr, Formula, Literal, MetricQuery, Modifier, Program, TagFilter};

type Error = ErrMode<ContextError>;

/// A message of the grammar's own, as FParsec's `fail` gives one.
#[derive(Debug)]
struct Message(String);

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Message {}

fn failure(input: &&str, message: String) -> Error {
    ErrMode::Backtrack(ContextError::from_external_error(input, Message(message)))
}

fn pchar<'i>(c: char) -> impl Parser<&'i str, char, Error> {
    c.context(StrContext::Expected(StrContextValue::CharLiteral(c)))
}

fn pstring<'i>(s: &'static str) -> impl Parser<&'i str, &'i str, Error> {
    s.context(StrContext::Expected(StrContextValue::StringLiteral(s)))
}

fn description(what: &'static str) -> StrContext {
    StrContext::Expected(StrContextValue::Description(what))
}

/// Spaces, tabs and line breaks, as FParsec's `spaces` skips them.
pub fn ws(input: &mut &str) -> ModalResult<()> {
    take_while(0.., [' ', '\t', '\r', '\n']).void().parse_next(input)
}

/// FParsec's `attempt`: on failure, rewind to where `parser` started and let the alternatives
/// after it be tried.
fn attempt<'i, O>(mut parser: impl Parser<&'i str, O, Error>) -> impl Parser<&'i str, O, Error> {
    move |input: &mut &'i str| {
        let start = *input;
        parser.parse_next(input).map_err(|e| {
            *input = start;
            e.backtrack()
        })
    }
}

/// One or more `item`s with `separator` between them. A separator commits: an item must follow.
fn sep_by1<'i, O, S>(
    mut item: impl Parser<&'i str, O, Error>,
    mut separator: impl Parser<&'i str, S, Error>,
) -> impl Parser<&'i str, Vec<O>, Error> {
    move |input: &mut &'i str| {
        let mut items = vec![item.parse_next(input)?];
        while opt(separator.by_ref()).parse_next(input)?.is_some() {
            items.push(cut_err(item.by_ref()).parse_next(input)?);
        }
        Ok(items)
    }
}

/// Zero or more, as `sep_by1`.
fn sep_by<'i, O, S>(
    item: impl Parser<&'i str, O, Error>,
    separator: impl Parser<&'i str, S, Error>,
) -> impl Parser<&'i str, Vec<O>, Error> {
    opt(sep_by1(item, separator)).map(Option::unwrap_or_default)
}

fn comma(input: &mut &str) -> ModalResult<()> {
    (pchar(','), ws).void().parse_next(input)
}

pub fn and_of(mut filters: Vec<TagFilter>) -> TagFilter {
    match filters.len() {
        0 => TagFilter::All,
        1 => filters.remove(0),
        _ => TagFilter::And(filters),
    }
}

fn or_of(mut filters: Vec<TagFilter>) -> TagFilter {
    match filters.len() {
        1 => filters.remove(0),
        _ => TagFilter::Or(filters),
    }
}

// ---------------------------------------------------------------------------------------
// 1. Tag filter
//
// Datadog has two syntaxes for a filter and refuses a mix of them:
//   symbolic    {env:prod,!host:a}                  `,` is AND, `!` is NOT
//   functional  {env:prod AND NOT (host:a OR host:b) AND zone IN (x, y)}
// ---------------------------------------------------------------------------------------

/// `*` is the wildcard, `$` a dashboard template variable (`{$env}`).
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || "_-:./*$".contains(c)
}

fn tag_word(input: &mut &str) -> ModalResult<String> {
    take_while(1.., is_tag_char).context(description("tag")).map(String::from).parse_next(input)
}

/// `env:prod` is a Tag, `canary` a Bare tag. Only the first `:` splits, as values may hold more
/// (`url:http://x`). A key with nothing after its colon takes the next word: Datadog accepts
/// `{servicename: ec2}`.
fn term(input: &mut &str) -> ModalResult<TagFilter> {
    let word = tag_word(input)?;
    match word.find(':') {
        None => Ok(TagFilter::Bare(word)),
        Some(colon) if colon == word.len() - 1 => {
            ws(input)?;
            let value = cut_err(tag_word).parse_next(input)?;
            Ok(TagFilter::Tag(word[..colon].to_string(), value))
        }
        Some(colon) => Ok(TagFilter::Tag(word[..colon].to_string(), word[colon + 1..].to_string())),
    }
}

// --- symbolic ---------------------------------------------------------------------------

fn symbolic_item(input: &mut &str) -> ModalResult<TagFilter> {
    let negated = opt(pchar('!')).parse_next(input)?.is_some();
    let t = if negated { cut_err(term).parse_next(input)? } else { term(input)? };
    ws(input)?;
    Ok(if negated { TagFilter::Not(Box::new(t)) } else { t })
}

fn symbolic(input: &mut &str) -> ModalResult<TagFilter> {
    sep_by1(symbolic_item, comma).map(and_of).parse_next(input)
}

// --- functional -------------------------------------------------------------------------

/// Datadog documents each keyword in upper and lower case only: `AND`, `and`.
const KEYWORDS: [&str; 8] = ["AND", "and", "OR", "or", "NOT", "not", "IN", "in"];

fn keyword<'i>(word: &'static str) -> impl Parser<&'i str, (), Error> {
    let lower = word.to_lowercase();
    move |input: &mut &'i str| {
        // attempt: `AND` must not eat the first letters of a tag like `ANDROID`.
        attempt((alt((word, lower.as_str())), not(one_of(is_tag_char))))
            .context(description(word))
            .parse_next(input)?;
        ws(input)
    }
}

fn mixed_syntax<T>(input: &mut &str) -> ModalResult<T> {
    one_of([',', '!'])
        .context(StrContext::Expected(StrContextValue::CharLiteral(',')))
        .context(StrContext::Expected(StrContextValue::CharLiteral('!')))
        .parse_next(input)?;
    Err(failure(input, "symbolic syntax (',' and '!') cannot be mixed with AND/OR/NOT/IN".to_string()).cut())
}

/// A keyword where a term was expected is not a tag named `AND`: it fails without consuming, so
/// the caller reads it as the keyword.
fn functional_term(input: &mut &str) -> ModalResult<TagFilter> {
    let word = peek(tag_word).parse_next(input)?;
    if KEYWORDS.contains(&word.as_str()) {
        return Err(failure(input, format!("unexpected keyword '{word}'")));
    }
    attempt(term).parse_next(input)
}

fn in_list(input: &mut &str) -> ModalResult<Vec<String>> {
    (pchar('('), ws).parse_next(input)?;
    let values = cut_err(sep_by(terminated(tag_word, ws), comma)).parse_next(input)?;
    cut_err((pchar(')'), ws)).parse_next(input)?;
    Ok(values)
}

/// A term, or `key IN (…)` / `key NOT IN (…)`, where the term is the key.
fn term_or_in(input: &mut &str) -> ModalResult<TagFilter> {
    let is_in = keyword("IN").value(false);
    // attempt: `NOT` followed by something other than `IN` is not ours.
    let is_not_in = attempt((keyword("NOT"), keyword("IN"))).value(true);

    let t = functional_term(input)?;
    ws(input)?;
    match (opt(alt((is_in, is_not_in))).parse_next(input)?, t) {
        (None, t) => Ok(t),
        (Some(negated), TagFilter::Bare(key)) => {
            let values = cut_err(in_list).parse_next(input)?;
            let filter = TagFilter::In(key, values);
            Ok(if negated { TagFilter::Not(Box::new(filter)) } else { filter })
        }
        (Some(_), _) => {
            Err(failure(input, "IN needs a bare tag key on its left, e.g. 'region IN (a, b)'".to_string()).cut())
        }
    }
}

/// NOT binds tighter than AND, and AND tighter than OR.
fn filter_unary(input: &mut &str) -> ModalResult<TagFilter> {
    alt((
        preceded(keyword("NOT"), cut_err(filter_unary)).map(|f| TagFilter::Not(Box::new(f))),
        preceded((pchar('('), ws), cut_err(terminated(functional, (pchar(')'), ws)))),
        mixed_syntax,
        term_or_in,
    ))
    .parse_next(input)
}

fn conjunction(input: &mut &str) -> ModalResult<TagFilter> {
    sep_by1(filter_unary, keyword("AND")).map(and_of).parse_next(input)
}

fn functional(input: &mut &str) -> ModalResult<TagFilter> {
    sep_by1(conjunction, keyword("OR")).map(or_of).parse_next(input)
}

// --- the braces -------------------------------------------------------------------------

fn everything(input: &mut &str) -> ModalResult<TagFilter> {
    alt(((pchar('*'), ws, pchar('}')).void(), pchar('}').void())).value(TagFilter::All).parse_next(input)
}

fn braced(input: &mut &str) -> ModalResult<TagFilter> {
    ws(input)?;
    alt((
        attempt(everything),
        // attempt: when the symbolic syntax does not reach `}` (it met an AND, say), rewind to
        // just after `{` and try the functional one.
        attempt(terminated(symbolic, pchar('}'))),
        // A `,` left after a functional filter means the syntaxes were mixed.
        terminated(functional, cut_err(alt((mixed_syntax, pchar('}').void())))),
    ))
    .parse_next(input)
}

/// `{*}`, `{}`, or a filter in either syntax.
pub fn tag_filter(input: &mut &str) -> ModalResult<TagFilter> {
    pchar('{').parse_next(input)?;
    cut_err(braced).parse_next(input)
}

pub fn parse_tag_filter(input: &str) -> Result<TagFilter, String> {
    run(input, tag_filter)
}

// ---------------------------------------------------------------------------------------
// 2. Metric query
//
//   avg:system.cpu.user{env:prod} by {host}.rollup(sum, 60)
//   agg metric          filter    group-by  modifiers
// ---------------------------------------------------------------------------------------

fn identifier(input: &mut &str) -> ModalResult<String> {
    (
        one_of(|c: char| c.is_alphabetic()),
        take_while(0.., |c: char| c.is_alphabetic() || c.is_ascii_digit() || c == '_'),
    )
        .take()
        .context(description("identifier"))
        .map(String::from)
        .parse_next(input)
}

/// Starts with a letter; later segments may start with a digit (`jetty.5xx_responses`).
fn metric_name(input: &mut &str) -> ModalResult<String> {
    (
        one_of(|c: char| c.is_alphabetic()),
        take_while(0.., |c: char| c.is_alphabetic() || c.is_ascii_digit() || c == '_' || c == '.'),
    )
        .take()
        .context(description("metric name"))
        .map(String::from)
        .parse_next(input)
}

/// attempt: in `system.cpu.user{*}` the word `system` reads as an identifier too; with no `:`
/// after it, rewind so it is read again as the metric name.
fn space_agg(input: &mut &str) -> ModalResult<Option<String>> {
    opt(attempt(terminated(identifier, pchar(':')))).parse_next(input)
}

/// `by {host, env}`. Datadog accepts `{bar:baz}by{host}`, with no spaces.
fn group_by(input: &mut &str) -> ModalResult<Vec<String>> {
    // attempt: spaces not followed by `by {` belong to whatever comes next.
    attempt((ws, pstring("by"), ws, pchar('{'))).parse_next(input)?;
    ws(input)?;
    let keys = cut_err(sep_by(terminated(tag_word, ws), comma)).parse_next(input)?;
    cut_err(pchar('}')).parse_next(input)?;
    Ok(keys)
}

fn in_quotes(q: char, input: &mut &str) -> ModalResult<String> {
    pchar(q).parse_next(input)?;
    let text = take_while(0.., move |c: char| c != q).parse_next(input)?;
    cut_err(pchar(q)).parse_next(input)?;
    Ok(text.to_string())
}

/// `'mean'` or `"UTC"`. No escapes: no documented argument needs them.
pub fn quoted(input: &mut &str) -> ModalResult<String> {
    alt((|i: &mut &str| in_quotes('\'', i), |i: &mut &str| in_quotes('"', i))).parse_next(input)
}

/// A number literal, finite, without a sign: a minus is the expression layer's.
///
/// Digits, an optional fraction and an optional exponent, as FParsec's `numberLiteral` with
/// fraction and exponent allowed reads them. Not `NaN` or `Infinity`, which would make a metric
/// named `nan.errors` start as a number.
pub fn number(input: &mut &str) -> ModalResult<f64> {
    let text = (
        digit1.context(description("number")),
        opt(('.', digit0)),
        opt((one_of(['e', 'E']), cut_err((opt(one_of(['+', '-'])), digit1.context(description("decimal digit")))))),
    )
        .take()
        .parse_next(input)?;
    // `1e999` reads as infinity.
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(value),
        _ => Err(failure(input, format!("{text} is too large a number")).cut()),
    }
}

fn literal(input: &mut &str) -> ModalResult<Literal> {
    let l = alt((number.map(Literal::Num), quoted.map(Literal::Quoted), identifier.map(Literal::Word)))
        .parse_next(input)?;
    ws(input)?;
    Ok(l)
}

fn modifier_body(input: &mut &str) -> ModalResult<Modifier> {
    ws(input)?;
    pchar('.').parse_next(input)?;
    let name = identifier(input)?;
    ws(input)?;
    (pchar('('), ws).parse_next(input)?;
    let args = sep_by(literal, comma).parse_next(input)?;
    pchar(')').parse_next(input)?;
    Ok(Modifier { name, args })
}

/// `.rollup(sum, 60)`, `.as_count()`
fn modifier(input: &mut &str) -> ModalResult<Modifier> {
    // attempt: a `.` not followed by a name and `(` is not a modifier.
    attempt(modifier_body).parse_next(input)
}

fn modifiers(input: &mut &str) -> ModalResult<Vec<Modifier>> {
    repeat(0.., modifier).parse_next(input)
}

fn after_metric_name(input: &mut &str) -> ModalResult<(TagFilter, Vec<String>, Vec<Modifier>)> {
    let filter = tag_filter(input)?;
    // WARNING(undocumented): whether Datadog accepts modifiers before `by`. Its own queries put
    // them after; we accept both.
    let mut all = modifiers(input)?;
    let groups = opt(group_by).parse_next(input)?;
    all.extend(modifiers(input)?);
    Ok((filter, groups.unwrap_or_default(), all))
}

fn plain_query(input: &mut &str) -> ModalResult<MetricQuery> {
    let space_agg = space_agg(input)?;
    let metric = if space_agg.is_some() { cut_err(metric_name).parse_next(input)? } else { metric_name(input)? };
    let (filter, group_by, modifiers) = cut_err(after_metric_name).parse_next(input)?;
    Ok(MetricQuery { space_agg, metric, filter, group_by, modifiers })
}

fn parenthesised_body(input: &mut &str) -> ModalResult<MetricQuery> {
    ws(input)?;
    let mut inner = plain_query(input)?;
    ws(input)?;
    pchar(')').parse_next(input)?;
    inner.modifiers.extend(modifiers(input)?);
    Ok(inner)
}

/// `sum:(gauge{*}).weighted()`: the form Datadog documents for weighted().
fn parenthesised_query(input: &mut &str) -> ModalResult<MetricQuery> {
    // attempt: `avg:` without a `(` after it is the start of a plain query.
    let agg = attempt(terminated(identifier, (pchar(':'), ws, pchar('(')))).parse_next(input)?;
    let inner = cut_err(parenthesised_body).parse_next(input)?;
    Ok(MetricQuery { space_agg: Some(agg), ..inner })
}

pub fn metric_query(input: &mut &str) -> ModalResult<MetricQuery> {
    alt((parenthesised_query, plain_query)).parse_next(input)
}

pub fn parse_metric_query(input: &str) -> Result<MetricQuery, String> {
    run(input, metric_query)
}

// ---------------------------------------------------------------------------------------
// 3. Expressions
//
//   sum      = product (('+' | '-') product)*
//   product  = unary   (('*' | '/') unary)*
//   unary    = '-' unary | primary
//   primary  = number | '(' sum ')' | name '(' arguments ')' | leaf
//
// Written once over any leaf: metric queries in v1, query names in v2 formulas.
// ---------------------------------------------------------------------------------------

/// v1 and v2 differ in one more thing: a bare word inside a call. In v1 a leaf needs braces, so
/// `mean` in `top(x{*}, 5, mean, desc)` can only be a word. In v2 it could be a query named
/// `mean`; it is read as a name, and analysis turns it back into a word where the function
/// expects one.
#[derive(Clone, Copy)]
enum BareWords {
    AreLiterals,
    AreLeaves,
}

struct Grammar<L> {
    bare_words: BareWords,
    leaf: fn(&mut &str) -> ModalResult<L>,
}

const V1: Grammar<MetricQuery> = Grammar { bare_words: BareWords::AreLiterals, leaf: metric_query };
const FORMULA: Grammar<String> = Grammar { bare_words: BareWords::AreLeaves, leaf: identifier };

fn named_argument<L>(input: &mut &str) -> ModalResult<Arg<L>> {
    // attempt: a word with no `=` after it is not a named argument.
    let name = attempt(terminated(identifier, (ws, pchar('='), ws))).parse_next(input)?;
    let value = cut_err(literal).parse_next(input)?;
    Ok(Arg::Named(name, value))
}

fn quoted_word<L>(input: &mut &str) -> ModalResult<Arg<L>> {
    terminated(quoted, ws).map(|s| Arg::Lit(Literal::Quoted(s))).parse_next(input)
}

fn bare_word<L>(input: &mut &str) -> ModalResult<Arg<L>> {
    // attempt: a word is bare only when `,` or `)` follows it.
    attempt(terminated(identifier, (ws, peek(alt((pchar(','), pchar(')')))))))
        .map(|w| Arg::Lit(Literal::Word(w)))
        .parse_next(input)
}

/// The forms overlap, so the order matters: `direction='above'` looks like a bare word until its
/// `=`, and a bare word looks like the start of a leaf.
fn argument<L>(g: &Grammar<L>, input: &mut &str) -> ModalResult<Arg<L>> {
    let value = |i: &mut &str| sum(g, i).map(Arg::Value);
    match g.bare_words {
        BareWords::AreLiterals => alt((named_argument, quoted_word, bare_word, value)).parse_next(input),
        BareWords::AreLeaves => alt((named_argument, quoted_word, value)).parse_next(input),
    }
}

fn call<L>(g: &Grammar<L>, input: &mut &str) -> ModalResult<Expr<L>> {
    // attempt: a name without `(` after it was the start of a leaf.
    let name = attempt(terminated(identifier, (ws, pchar('(')))).parse_next(input)?;
    ws(input)?;
    let args = cut_err(sep_by(terminated(|i: &mut &str| argument(g, i), ws), comma)).parse_next(input)?;
    cut_err(pchar(')')).parse_next(input)?;
    Ok(Expr::Call(name, args))
}

fn parenthesised<L>(g: &Grammar<L>, input: &mut &str) -> ModalResult<Expr<L>> {
    (pchar('('), ws).parse_next(input)?;
    let e = sum(g, input).map_err(ErrMode::cut)?;
    cut_err(pchar(')')).parse_next(input)?;
    Ok(e)
}

fn primary<L>(g: &Grammar<L>, input: &mut &str) -> ModalResult<Expr<L>> {
    let e = alt((
        number.map(Expr::Number),
        |i: &mut &str| parenthesised(g, i),
        |i: &mut &str| call(g, i),
        g.leaf.map(Expr::Leaf),
    ))
    .parse_next(input)?;
    ws(input)?;
    Ok(e)
}

fn unary<L>(g: &Grammar<L>, input: &mut &str) -> ModalResult<Expr<L>> {
    if opt(pchar('-')).parse_next(input)?.is_none() {
        return primary(g, input);
    }
    ws(input)?;
    match unary(g, input).map_err(ErrMode::cut)? {
        // `-3600` is a number, not a negated one.
        Expr::Number(n) => Ok(Expr::Number(-n)),
        e => Ok(Expr::Neg(Box::new(e))),
    }
}

fn operator(operators: [(char, BinaryOp); 2], input: &mut &str) -> ModalResult<BinaryOp> {
    let [(a, op_a), (b, op_b)] = operators;
    terminated(alt((pchar(a).value(op_a), pchar(b).value(op_b))), ws).parse_next(input)
}

/// Left to right, so `a - b - c` is `(a - b) - c`.
fn left_to_right<L>(
    operators: [(char, BinaryOp); 2],
    mut operand: impl FnMut(&mut &str) -> ModalResult<Expr<L>>,
    input: &mut &str,
) -> ModalResult<Expr<L>> {
    let mut left = operand(input)?;
    while let Some(op) = opt(|i: &mut &str| operator(operators, i)).parse_next(input)? {
        let right = operand(input).map_err(ErrMode::cut)?;
        left = Expr::Binary(op, Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn product<L>(g: &Grammar<L>, input: &mut &str) -> ModalResult<Expr<L>> {
    left_to_right([('*', BinaryOp::Mul), ('/', BinaryOp::Div)], |i| unary(g, i), input)
}

fn sum<L>(g: &Grammar<L>, input: &mut &str) -> ModalResult<Expr<L>> {
    left_to_right([('+', BinaryOp::Add), ('-', BinaryOp::Sub)], |i| product(g, i), input)
}

/// A v2 formula. Whether its names name queries is checked later: Datadog itself stores formulas
/// naming queries that do not exist.
pub fn parse_formula(input: &str) -> Result<Formula, String> {
    run(input, |i| sum(&FORMULA, i))
}

/// A v1 `query=`: one or more expressions, separated by commas.
pub fn parse_program(input: &str) -> Result<Program, String> {
    run(input, |i| sep_by1(expression, comma).parse_next(i))
}

/// One expression with its metric queries inline, as v1 and monitors write it. It does not need
/// the end of the input: a monitor's query goes on with a comparator after it.
pub fn expression(input: &mut &str) -> ModalResult<Expr<MetricQuery>> {
    sum(&V1, input)
}

// --- running ------------------------------------------------------------------------------

fn whole<T>(parser: &mut impl FnMut(&mut &str) -> ModalResult<T>, input: &mut &str) -> ModalResult<T> {
    ws(input)?;
    let value = parser(input)?;
    ws(input)?;
    eof.context(description("end of input")).parse_next(input)?;
    Ok(value)
}

/// Runs `parser` over the whole of `input`, spaces around it allowed, and turns a failure into one
/// line of text naming the column and what was expected there.
pub fn run<T>(input: &str, mut parser: impl FnMut(&mut &str) -> ModalResult<T>) -> Result<T, String> {
    let mut rest = input;
    match whole(&mut parser, &mut rest) {
        Ok(value) => Ok(value),
        Err(e) => {
            let error = e.into_inner().unwrap_or_default();
            Err(message(input, input.len() - rest.len(), &error))
        }
    }
}

/// FParsec's message on one line: `Error in Ln: 1 Col: 23; Expecting: '}'`. The column counts
/// UTF-16 units, as FParsec's did.
fn message(input: &str, offset: usize, error: &ContextError) -> String {
    let before = input[..offset].replace("\r\n", "\n");
    let line = before.split(['\n', '\r']).count();
    let column = before.rsplit(['\n', '\r']).next().unwrap_or("").encode_utf16().count() + 1;

    let mut expected: Vec<String> = Vec::new();
    for context in error.context() {
        if let StrContext::Expected(value) = context {
            let text = match value {
                StrContextValue::CharLiteral(c) => format!("'{c}'"),
                StrContextValue::StringLiteral(s) => format!("'{s}'"),
                StrContextValue::Description(s) => s.to_string(),
                other => other.to_string(),
            };
            if !expected.contains(&text) {
                expected.push(text);
            }
        }
    }

    let mut parts = vec![format!("Error in Ln: {line} Col: {column}")];
    if let Some((last, rest)) = expected.split_last() {
        if rest.is_empty() {
            parts.push(format!("Expecting: {last}"));
        } else {
            parts.push(format!("Expecting: {} or {last}", rest.join(", ")));
        }
    }
    if let Some(cause) = error.cause() {
        parts.push(cause.to_string());
    }
    parts.join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric_query::ast::BinaryOp::{Add, Div, Mul, Sub};
    use crate::metric_query::ast::Literal::{Num, Quoted, Word};
    use crate::metric_query::ast::TagFilter::{All, And, Bare, In, Or};

    fn tag(k: &str, v: &str) -> TagFilter {
        TagFilter::Tag(k.into(), v.into())
    }

    fn not(f: TagFilter) -> TagFilter {
        TagFilter::Not(Box::new(f))
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    // --- tag filters (F#: TagFilterTests.fs) -------------------------------------------

    fn filter_parses(input: &str, expected: TagFilter) {
        match parse_tag_filter(input) {
            Ok(actual) => assert_eq!(actual, expected, "{input}"),
            Err(e) => panic!("'{input}' did not parse:\n{e}"),
        }
    }

    fn filter_fails(input: &str, fragment: &str) {
        match parse_tag_filter(input) {
            Ok(actual) => panic!("'{input}' should not parse, got {actual:?}"),
            Err(e) => assert!(e.contains(fragment), "'{input}': '{e}' does not contain '{fragment}'"),
        }
    }

    #[test]
    fn star_and_empty_both_mean_all() {
        filter_parses("{*}", All);
        filter_parses("{}", All);
        filter_parses("{ * }", All);
    }

    #[test]
    fn single_tag() {
        filter_parses("{env:prod}", tag("env", "prod"));
    }

    #[test]
    fn comma_means_and() {
        filter_parses("{environment:foo,host:foo}", And(vec![tag("environment", "foo"), tag("host", "foo")]));
    }

    #[test]
    fn bang_negates() {
        // Seen in datadog-api-client-go SLO cassettes.
        filter_parses("{type:good,!type:ignored}", And(vec![tag("type", "good"), not(tag("type", "ignored"))]));
    }

    #[test]
    fn key_less_tags_alone_and_mixed() {
        // terraform-provider-datadog TestAccDatadogMonitor_Updated.
        filter_parses("{example,framework:chronos}", And(vec![Bare("example".into()), tag("framework", "chronos")]));
    }

    #[test]
    fn wildcards_are_kept_verbatim() {
        filter_parses("{!device:/dev/loop*}", not(tag("device", "/dev/loop*")));
        filter_parses("{region:*east*}", tag("region", "*east*"));
    }

    #[test]
    fn value_may_contain_further_colons() {
        filter_parses("{url:http://x}", tag("url", "http://x"));
    }

    #[test]
    fn space_after_the_colon() {
        // terraform-provider-datadog resource_datadog_monitor_test.go.
        filter_parses("{servicename: ec2}", tag("servicename", "ec2"));
    }

    #[test]
    fn template_variables() {
        filter_parses(
            "{$troux_uuid,$container_guid}",
            And(vec![Bare("$troux_uuid".into()), Bare("$container_guid".into())]),
        );
    }

    #[test]
    fn spaces_around_commas() {
        filter_parses("{ a:1 , b:2 }", And(vec![tag("a", "1"), tag("b", "2")]));
    }

    #[test]
    fn and_and_or_with_parentheses() {
        // Datadog docs, advanced filtering.
        filter_parses(
            "{env:staging AND (availability-zone:us-east-1a OR availability-zone:us-east-1c)}",
            And(vec![
                tag("env", "staging"),
                Or(vec![tag("availability-zone", "us-east-1a"), tag("availability-zone", "us-east-1c")]),
            ]),
        );
    }

    #[test]
    fn and_binds_tighter_than_or() {
        filter_parses("{a:1 OR b:2 AND c:3}", Or(vec![tag("a", "1"), And(vec![tag("b", "2"), tag("c", "3")])]));
    }

    #[test]
    fn in_and_not_in() {
        filter_parses(
            "{env:shop.ist AND availability-zone IN (us-east-1a, us-east-1b, us-east4-b)}",
            And(vec![
                tag("env", "shop.ist"),
                In("availability-zone".into(), strings(&["us-east-1a", "us-east-1b", "us-east4-b"])),
            ]),
        );
        filter_parses(
            "{env:prod AND location NOT IN (atlanta,seattle,las-vegas)}",
            And(vec![tag("env", "prod"), not(In("location".into(), strings(&["atlanta", "seattle", "las-vegas"])))]),
        );
    }

    #[test]
    fn in_with_odd_whitespace() {
        // datadog-api-client-go examples/v1/monitors/CreateMonitor_1303514967.go.
        filter_parses(
            "{aws_product IN (amplify ,athena, backup, bedrock ) }",
            In("aws_product".into(), strings(&["amplify", "athena", "backup", "bedrock"])),
        );
    }

    #[test]
    fn not_prefix() {
        filter_parses("{NOT host:a}", not(tag("host", "a")));
    }

    #[test]
    fn lower_case_keywords() {
        filter_parses("{a:1 and not b:2}", And(vec![tag("a", "1"), not(tag("b", "2"))]));
    }

    #[test]
    fn a_keyword_prefix_is_still_a_tag() {
        filter_parses("{ANDROID}", Bare("ANDROID".into()));
    }

    #[test]
    fn mixing_the_two_syntaxes_is_refused() {
        filter_fails("{a:1, b:2 AND c:3}", "cannot be mixed");
        filter_fails("{env:prod AND !host:a}", "cannot be mixed");
    }

    #[test]
    fn in_after_a_key_value_is_refused() {
        filter_fails("{env:prod IN (a)}", "bare tag key");
    }

    #[test]
    fn unclosed_brace() {
        filter_fails("{env:prod", "Expecting");
    }

    // --- metric queries (F#: MetricQueryTests.fs) ----------------------------------------

    fn q(metric: &str) -> MetricQuery {
        MetricQuery { space_agg: None, metric: metric.into(), filter: All, group_by: vec![], modifiers: vec![] }
    }

    fn agg(space_agg: &str, metric: &str) -> MetricQuery {
        MetricQuery { space_agg: Some(space_agg.into()), ..q(metric) }
    }

    fn m(name: &str, args: Vec<Literal>) -> Modifier {
        Modifier { name: name.into(), args }
    }

    fn query_parses(input: &str, expected: MetricQuery) {
        match parse_metric_query(input) {
            Ok(actual) => assert_eq!(actual, expected, "{input}"),
            Err(e) => panic!("'{input}' did not parse:\n{e}"),
        }
    }

    fn query_fails(input: &str, fragment: &str) {
        match parse_metric_query(input) {
            Ok(actual) => panic!("'{input}' should not parse, got {actual:?}"),
            Err(e) => assert!(e.contains(fragment), "'{input}': '{e}' does not contain '{fragment}'"),
        }
    }

    #[test]
    fn bare_metric() {
        // datadog-api-client-python examples/v1/metrics/QueryMetrics.py.
        query_parses("system.cpu.idle{*}", q("system.cpu.idle"));
    }

    #[test]
    fn space_aggregator() {
        query_parses("avg:system.cpu.user{*}", agg("avg", "system.cpu.user"));
        query_parses("p90:dist.dd.dogweb.latency{*}", agg("p90", "dist.dd.dogweb.latency"));
        query_parses("histogram:trace.Load{*}", agg("histogram", "trace.Load"));
    }

    #[test]
    fn filter_is_the_tag_filter_parser() {
        query_parses(
            "avg:system.cpu.user{env:prod,!host:a}",
            MetricQuery {
                filter: And(vec![tag("env", "prod"), not(tag("host", "a"))]),
                ..agg("avg", "system.cpu.user")
            },
        );
    }

    #[test]
    fn group_by_keys() {
        query_parses(
            "avg:system.cpu.user{*} by {host}",
            MetricQuery { group_by: strings(&["host"]), ..agg("avg", "system.cpu.user") },
        );
        query_parses(
            "sum:x{*} by {service, team, app}",
            MetricQuery { group_by: strings(&["service", "team", "app"]), ..agg("sum", "x") },
        );
    }

    #[test]
    fn group_by_with_no_spaces_at_all() {
        // The one v1 query recorded with a 200 in datadog-api-client-go (TestMetrics.yaml).
        query_parses(
            "avg:Test_TestMetrics_1618221195{bar:baz}by{host}",
            MetricQuery {
                filter: tag("bar", "baz"),
                group_by: strings(&["host"]),
                ..agg("avg", "Test_TestMetrics_1618221195")
            },
        );
    }

    #[test]
    fn modifiers_after_group_by() {
        query_parses(
            "avg:x{*} by {app_name,bosh_id}.rollup(avg, 3600)",
            MetricQuery {
                group_by: strings(&["app_name", "bosh_id"]),
                modifiers: vec![m("rollup", vec![Word("avg".into()), Num(3600.0)])],
                ..agg("avg", "x")
            },
        );
    }

    #[test]
    fn several_modifiers_in_order() {
        query_parses(
            "sum:x{*}.as_count().rollup(sum, 60).fill(zero)",
            MetricQuery {
                modifiers: vec![
                    m("as_count", vec![]),
                    m("rollup", vec![Word("sum".into()), Num(60.0)]),
                    m("fill", vec![Word("zero".into())]),
                ],
                ..agg("sum", "x")
            },
        );
    }

    #[test]
    fn calendar_rollup_words() {
        // Cloud cost queries in datadog-api-client-go examples.
        query_parses(
            "sum:aws.cost{*}.rollup(sum, monthly)",
            MetricQuery {
                modifiers: vec![m("rollup", vec![Word("sum".into()), Word("monthly".into())])],
                ..agg("sum", "aws.cost")
            },
        );
    }

    #[test]
    fn metric_segment_starting_with_a_digit() {
        // terraform-provider-datadog TestAccDatadogMonitor_Updated.
        query_parses(
            "org.eclipse.jetty.ServletContextHandler.5xx_responses{example}",
            MetricQuery {
                filter: Bare("example".into()),
                ..q("org.eclipse.jetty.ServletContextHandler.5xx_responses")
            },
        );
    }

    #[test]
    fn aggregator_outside_parentheses_for_weighted() {
        // Datadog docs, smoothing functions.
        query_parses(
            "sum:(kubernetes.cpu.usage{*}).weighted()",
            MetricQuery { modifiers: vec![m("weighted", vec![])], ..agg("sum", "kubernetes.cpu.usage") },
        );
    }

    #[test]
    fn missing_braces_are_refused() {
        query_fails("avg:system.cpu.user", "Expecting: '{'");
    }

    #[test]
    fn unknown_trailing_text_is_refused() {
        query_fails("avg:x{*} banana", "end of input");
    }

    #[test]
    fn errors_are_one_line_with_the_column() {
        assert_eq!(parse_metric_query("avg:x{env:prod"), Err("Error in Ln: 1 Col: 15; Expecting: '}'".to_string()));
        assert_eq!(
            parse_tag_filter("{a:1, b:2 AND c:3}"),
            Err("Error in Ln: 1 Col: 6; symbolic syntax (',' and '!') cannot be mixed with AND/OR/NOT/IN".to_string())
        );
        assert_eq!(
            parse_metric_query("avg:x{*} banana"),
            Err("Error in Ln: 1 Col: 10; Expecting: end of input".to_string())
        );
    }

    // --- expressions (F#: ExpressionTests.fs) ----------------------------------------------

    fn leaf(space_agg: Option<&str>, metric: &str) -> Expr<MetricQuery> {
        Expr::Leaf(MetricQuery { space_agg: space_agg.map(String::from), ..q(metric) })
    }

    fn call_of<L>(name: &str, args: Vec<Arg<L>>) -> Expr<L> {
        Expr::Call(name.into(), args)
    }

    fn binary<L>(op: BinaryOp, a: Expr<L>, b: Expr<L>) -> Expr<L> {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    fn program_parses(input: &str, expected: Program) {
        match parse_program(input) {
            Ok(actual) => assert_eq!(actual, expected, "{input}"),
            Err(e) => panic!("'{input}' did not parse:\n{e}"),
        }
    }

    fn program_fails(input: &str, fragment: &str) {
        match parse_program(input) {
            Ok(actual) => panic!("'{input}' should not parse, got {actual:?}"),
            Err(e) => assert!(e.contains(fragment), "'{input}': '{e}' does not contain '{fragment}'"),
        }
    }

    #[test]
    fn function_wrapping_a_query() {
        // Datadog docs, exclusion functions.
        program_parses(
            "exclude_null(avg:system.load.1{*} by {host})",
            vec![call_of(
                "exclude_null",
                vec![Arg::Value(Expr::Leaf(MetricQuery {
                    group_by: strings(&["host"]),
                    ..agg("avg", "system.load.1")
                }))],
            )],
        );
    }

    #[test]
    fn functions_nest() {
        program_parses(
            "abs(log10(x{*}))",
            vec![call_of("abs", vec![Arg::Value(call_of("log10", vec![Arg::Value(leaf(None, "x"))]))])],
        );
    }

    #[test]
    fn top_with_numbers_and_quoted_strings() {
        // terraform-provider-datadog TestAccDatadogDashboardTopListWithStyle.
        program_parses(
            "top(avg:x{*}, 10, 'sum', 'desc')",
            vec![call_of(
                "top",
                vec![
                    Arg::Value(leaf(Some("avg"), "x")),
                    Arg::Value(Expr::Number(10.0)),
                    Arg::Lit(Quoted("sum".into())),
                    Arg::Lit(Quoted("desc".into())),
                ],
            )],
        );
    }

    #[test]
    fn negative_argument_is_a_plain_number() {
        // Datadog docs, timeshift.
        program_parses(
            "timeshift(avg:system.load.1{*}, -1209600)",
            vec![call_of(
                "timeshift",
                vec![Arg::Value(leaf(Some("avg"), "system.load.1")), Arg::Value(Expr::Number(-1209600.0))],
            )],
        );
    }

    #[test]
    fn double_quoted_arguments() {
        program_parses(
            "calendar_shift(x{*}, \"-1d\", \"UTC\")",
            vec![call_of(
                "calendar_shift",
                vec![Arg::Value(leaf(None, "x")), Arg::Lit(Quoted("-1d".into())), Arg::Lit(Quoted("UTC".into()))],
            )],
        );
    }

    #[test]
    fn named_arguments_mixed_with_positional() {
        // datadog-api-client-go TestMonitorLifecycle cassette, no spaces after commas.
        program_parses(
            "anomalies(sum:m{*}.as_count(),'agile',2,direction='below',interval=300,count_default_zero='true')",
            vec![call_of(
                "anomalies",
                vec![
                    Arg::Value(Expr::Leaf(MetricQuery { modifiers: vec![m("as_count", vec![])], ..agg("sum", "m") })),
                    Arg::Lit(Quoted("agile".into())),
                    Arg::Value(Expr::Number(2.0)),
                    Arg::Named("direction".into(), Quoted("below".into())),
                    Arg::Named("interval".into(), Num(300.0)),
                    Arg::Named("count_default_zero".into(), Quoted("true".into())),
                ],
            )],
        );
    }

    #[test]
    fn bare_word_argument() {
        program_parses(
            "top(x{*}, 5, mean, desc)",
            vec![call_of(
                "top",
                vec![
                    Arg::Value(leaf(None, "x")),
                    Arg::Value(Expr::Number(5.0)),
                    Arg::Lit(Word("mean".into())),
                    Arg::Lit(Word("desc".into())),
                ],
            )],
        );
    }

    #[test]
    fn multiplication_before_addition() {
        program_parses(
            "a{*} + b{*} * 2",
            vec![binary(Add, leaf(None, "a"), binary(Mul, leaf(None, "b"), Expr::Number(2.0)))],
        );
    }

    #[test]
    fn left_to_right_within_one_level() {
        program_parses(
            "a{*} - b{*} - c{*}",
            vec![binary(Sub, binary(Sub, leaf(None, "a"), leaf(None, "b")), leaf(None, "c"))],
        );
    }

    #[test]
    fn parentheses_override_precedence() {
        program_parses(
            "(a{*} + b{*}) * 100",
            vec![binary(Mul, binary(Add, leaf(None, "a"), leaf(None, "b")), Expr::Number(100.0))],
        );
    }

    #[test]
    fn parenthesised_queries_with_modifiers() {
        // terraform-provider-datadog BadEventsToMetricQueryTransition cassette.
        match parse_program("(sum:my.metric{status:good}.as_count()) + (sum:my.metric{status:bad}.as_count())")
            .as_deref()
        {
            Ok([Expr::Binary(Add, good, bad)]) => match (good.as_ref(), bad.as_ref()) {
                (Expr::Leaf(good), Expr::Leaf(bad)) => {
                    assert_eq!(good.filter, tag("status", "good"));
                    assert_eq!(bad.filter, tag("status", "bad"));
                }
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn arithmetic_inside_a_function() {
        program_parses(
            "abs(a{*} / b{*})",
            vec![call_of("abs", vec![Arg::Value(binary(Div, leaf(None, "a"), leaf(None, "b")))])],
        );
    }

    #[test]
    fn unary_minus_on_a_query() {
        program_parses("-a{*}", vec![Expr::Neg(Box::new(leaf(None, "a")))]);
    }

    #[test]
    fn comma_separates_expressions_at_the_top() {
        program_parses(
            "avg:a{*}, avg:b{x:1,y:2}",
            vec![
                leaf(Some("avg"), "a"),
                Expr::Leaf(MetricQuery { filter: And(vec![tag("x", "1"), tag("y", "2")]), ..agg("avg", "b") }),
            ],
        );
    }

    #[test]
    fn a_metric_named_like_nan_is_still_a_metric() {
        program_parses("nan.errors{*}", vec![leaf(None, "nan.errors")]);
    }

    #[test]
    fn a_function_name_without_parenthesis_is_a_metric() {
        program_parses("abs{*}", vec![leaf(None, "abs")]);
    }

    #[test]
    fn unclosed_call() {
        program_fails("abs(x{*}", "Expecting");
    }

    #[test]
    fn numbers() {
        program_parses("x{*} * 1.", vec![binary(Mul, leaf(None, "x"), Expr::Number(1.0))]);
        program_parses("x{*} * 1.5e2", vec![binary(Mul, leaf(None, "x"), Expr::Number(150.0))]);
        program_parses("x{*} * 2E-1", vec![binary(Mul, leaf(None, "x"), Expr::Number(0.2))]);
        program_fails("x{*} * 1e999", "1e999 is too large a number");
        program_fails("x{*} * 2e", "Expecting: decimal digit");
    }

    // --- formulas (F#: FormulaTests.fs) ------------------------------------------------------

    fn name(n: &str) -> Formula {
        Expr::Leaf(n.into())
    }

    fn formula_parses(input: &str, expected: Formula) {
        match parse_formula(input) {
            Ok(actual) => assert_eq!(actual, expected, "{input}"),
            Err(e) => panic!("'{input}' did not parse:\n{e}"),
        }
    }

    fn formula_fails(input: &str) {
        if let Ok(actual) = parse_formula(input) {
            panic!("'{input}' should not parse, got {actual:?}");
        }
    }

    // Every formula below is from the corpus of Datadog client and Terraform fixtures unless
    // noted.

    #[test]
    fn a_single_query_name() {
        // The most common formula by far: a widget shows only formulas, so even one query is
        // displayed through the formula that names it.
        formula_parses("query1", name("query1"));
    }

    #[test]
    fn arithmetic_between_names() {
        formula_parses("query2/query1", binary(Div, name("query2"), name("query1")));
        formula_parses("a+b", binary(Add, name("a"), name("b")));
    }

    #[test]
    fn parentheses_and_precedence() {
        formula_parses(
            "(query1-query2)/query1",
            binary(Div, binary(Sub, name("query1"), name("query2")), name("query1")),
        );
    }

    #[test]
    fn redundant_parentheses_and_decimals() {
        formula_parses(
            "(((errors * 0.2)) / (query * 0.3))",
            binary(Div, binary(Mul, name("errors"), Expr::Number(0.2)), binary(Mul, name("query"), Expr::Number(0.3))),
        );
    }

    #[test]
    fn functions_over_names() {
        formula_parses("hour_before(query1)", call_of("hour_before", vec![Arg::Value(name("query1"))]));
        formula_parses("default_zero(query1)", call_of("default_zero", vec![Arg::Value(name("query1"))]));
    }

    #[test]
    fn top_takes_quoted_words() {
        // Datadog docs, rank functions, written over a formula.
        formula_parses(
            "top(a, 10, 'mean', 'desc')",
            call_of(
                "top",
                vec![
                    Arg::Value(name("a")),
                    Arg::Value(Expr::Number(10.0)),
                    Arg::Lit(Quoted("mean".into())),
                    Arg::Lit(Quoted("desc".into())),
                ],
            ),
        );
    }

    #[test]
    fn bare_words_are_read_as_names() {
        // `mean` could be a query name or a literal; the parser cannot tell, so it takes it as a
        // name and analysis decides from what `top` expects.
        formula_parses(
            "top(a, 10, mean, desc)",
            call_of(
                "top",
                vec![
                    Arg::Value(name("a")),
                    Arg::Value(Expr::Number(10.0)),
                    Arg::Value(name("mean")),
                    Arg::Value(name("desc")),
                ],
            ),
        );
    }

    #[test]
    fn negative_shift() {
        formula_parses(
            "timeshift(a, -3600)",
            call_of("timeshift", vec![Arg::Value(name("a")), Arg::Value(Expr::Number(-3600.0))]),
        );
    }

    #[test]
    fn a_metric_query_is_not_a_formula() {
        // Formulas name queries; the queries themselves go in `queries[]`.
        formula_fails("avg:system.cpu.user{*}");
    }

    #[test]
    fn dangling_operator() {
        formula_fails("a +");
    }
}
