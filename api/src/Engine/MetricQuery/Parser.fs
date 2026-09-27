/// Parser for Datadog's metric query language, built with FParsec.
///
/// Built bottom-up, one piece at a time. So far: the tag filter inside `{...}`.
module NinjaCat.Api.Engine.MetricQuery.Parser

open System
open FParsec
open NinjaCat.Api.Engine.MetricQuery.Ast

// No user state is threaded through the parsers yet.
type private P<'T> = Parser<'T, unit>

// ---------------------------------------------------------------------------
// Tag filter: what goes between `{` and `}`.
//
// Datadog accepts two surface syntaxes and refuses a mix of them
// (docs: metrics/advanced-filtering):
//
//   symbolic    env:prod,!host:a,region:us-*      `,` = AND, `!` = NOT
//   functional  env:prod AND NOT (host:a OR host:b) AND zone IN (a, b)
//
// Strategy: try the symbolic form first; if it does not reach `}`, rewind and
// try the functional form. The functional parser treats `,` and `!` as errors
// with a message that names the mix, so `{a:1, b:2 AND c:3}` fails with an
// explanation rather than a bare "expected }".
// ---------------------------------------------------------------------------

/// Characters a tag (key or value) may contain. Unicode letters are allowed
/// because Datadog lower-cases tags but does not restrict them to ASCII. `*` is
/// the wildcard, `$` a dashboard template variable (`{$env}`), which Datadog
/// accepts even in API queries.
let private isTagChar c =
    Char.IsLetterOrDigit c
    || c = '_' || c = '-' || c = ':' || c = '.' || c = '/' || c = '*' || c = '$'

let private ws: P<unit> = spaces

/// The functional keywords. Datadog documents each in upper and lower case
/// only (`AND`, `and`) — not `And` — and so do we.
let private keywords = set [ "AND"; "and"; "OR"; "or"; "NOT"; "not"; "IN"; "in" ]

/// A keyword, as a whole word: `AND` must not match the start of `ANDROID`.
let private keyword (kw: string) : P<unit> =
    attempt ((pstring kw <|> pstring (kw.ToLowerInvariant())) >>. notFollowedBy (satisfy isTagChar))
    >>. ws
    <?> kw

/// One raw tag word, as typed: `env:prod`, `web-*`, `servicename:`.
let private tagWord: P<string> = many1Satisfy isTagChar <?> "tag"

/// A tag word turned into a filter.
///
/// Splits at the first `:` — values may contain further colons
/// (`url:http://x`). A key followed by nothing, `servicename:`, takes the next
/// word as its value: Datadog accepted `{servicename: ec2}` in the wild.
let private term: P<TagFilter> =
    tagWord
    >>= fun word ->
        match word.IndexOf ':' with
        | -1 -> preturn (Bare word)
        | i when i = word.Length - 1 -> ws >>. tagWord |>> fun v -> Tag(word[.. i - 1], v)
        | i -> preturn (Tag(word[.. i - 1], word[i + 1 ..]))

// --- symbolic: env:prod,!host:a ---------------------------------------------

let private symbolicItem: P<TagFilter> =
    (pchar '!' >>. term |>> Not) <|> term

let private symbolic: P<TagFilter> =
    sepBy1 (symbolicItem .>> ws) (pchar ',' >>. ws)
    |>> function
        | [ single ] -> single
        | many -> And many

// --- functional: env:prod AND NOT (host:a OR host:b) ------------------------

/// A term in functional syntax: any word except a keyword, which would
/// otherwise be read as a key-less tag named `AND`.
let private functionalTerm: P<TagFilter> =
    attempt (
        lookAhead tagWord
        >>= fun w -> if keywords.Contains w then fail $"unexpected keyword '{w}'" else term
    )

/// Fails with a named error on `,` or `!`. A function so that it can stand
/// wherever a parser of any result type is expected.
let private mixError () : P<'T> =
    (pchar ',' <|> pchar '!')
    >>. fail "symbolic syntax (',' and '!') cannot be mixed with AND/OR/NOT/IN"

/// `key IN (a, b)` or `key NOT IN (a, b)`, after the key has been read.
let private inList: P<string list> =
    between (pchar '(' >>. ws) (pchar ')' >>. ws) (sepBy (tagWord .>> ws) (pchar ',' >>. ws))

let private functional, private functionalRef = createParserForwardedToRef<TagFilter, unit> ()

let private unary, private unaryRef = createParserForwardedToRef<TagFilter, unit> ()

/// A term, optionally followed by `IN (...)` / `NOT IN (...)`.
///
/// `IN` only makes sense after a key-less word — the word *is* the key — so
/// `env:prod IN (...)` is refused here rather than silently misread.
let private termOrIn: P<TagFilter> =
    functionalTerm .>> ws
    >>= fun t ->
        let inClause negate =
            match t with
            | Bare key -> inList |>> fun vs -> if negate then Not(In(key, vs)) else In(key, vs)
            | _ -> fail "IN needs a bare tag key on its left, e.g. 'region IN (a, b)'"

        choice [
            keyword "IN" >>. inClause false
            attempt (keyword "NOT" >>. keyword "IN") >>. inClause true
            preturn t
        ]

unaryRef.Value <-
    choice [
        keyword "NOT" >>. unary |>> Not
        between (pchar '(' >>. ws) (pchar ')' >>. ws) functional
        mixError ()
        termOrIn
    ]

/// Precedence: NOT binds tightest, then AND, then OR — the usual order, and
/// the one Datadog's own examples rely on.
let private chainOf (kw: string) (ctor: TagFilter list -> TagFilter) (next: P<TagFilter>) =
    sepBy1 next (keyword kw)
    |>> function
        | [ single ] -> single
        | many -> ctor many

functionalRef.Value <- chainOf "OR" Or (chainOf "AND" And unary)

// --- the braces ---------------------------------------------------------------

/// `{*}`, `{}`, or a filter in either syntax.
let tagFilter: P<TagFilter> =
    let close = pchar '}'
    let all = (pchar '*' >>. ws >>. close) <|> close >>% All
    // `attempt` makes the symbolic branch rewind fully when it does not reach
    // `}`, so the functional branch starts again from just after `{`.
    // A `,` after a functional expression means the two syntaxes were mixed
    // (`{a:1, b:2 AND c:3}`), so it gets the named error, not "expected }".
    let body =
        choice [ attempt all; attempt (symbolic .>> close); functional .>> (close <|> mixError ()) ]
    pchar '{' >>. ws >>. body

/// Parses a complete `{...}` filter; for tests and for callers that only
/// have a scope.
let parseTagFilter (input: string) : Result<TagFilter, string> =
    match run (ws >>. tagFilter .>> ws .>> eof) input with
    | Success(result, _, _) -> Result.Ok result
    | Failure(message, _, _) -> Result.Error message

// ---------------------------------------------------------------------------
// Metric query: [agg:]metric{filter} [by {tags}] [.modifier(args)]...
//
//   avg:system.cpu.user{env:prod} by {host}.rollup(sum, 60)
//   └┬┘ └──────┬──────┘└───┬────┘ └───┬───┘└───────┬───────┘
//   agg     metric      filter     group-by     modifiers
//
// Only the prefix `agg:` and the braces are fixed; everything else is
// optional. Wrapping functions (`top(...)`) and arithmetic are the expression
// layer, not this one.
// ---------------------------------------------------------------------------

/// `avg`, `sum`, `p95`, `histogram`… Any word: whether it is a real aggregator
/// for this metric's type is decided after parsing.
let private identifier: P<string> =
    many1Satisfy2 isLetter (fun c -> isLetter c || isDigit c || c = '_') <?> "identifier"

/// Datadog metric names: must start with a letter, then letters, digits, `_`
/// and `.`. Segments may start with a digit — `jetty.5xx_responses` was
/// accepted in the wild.
let private metricName: P<string> =
    many1Satisfy2 isLetter (fun c -> isLetter c || isDigit c || c = '_' || c = '.') <?> "metric name"

/// `avg:` — the identifier and its colon, or nothing.
///
/// `attempt` because the identifier alone looks exactly like the start of a
/// metric name: in `system.cpu.user{*}` we read `system`, find `.` instead of
/// `:`, and must rewind to read it again as the metric.
let private spaceAgg: P<string option> = opt (attempt (identifier .>> pchar ':'))

/// `by {host, env}`. Datadog accepted `{bar:baz}by{host}` with no spaces at
/// all, so `by` needs no whitespace around it.
let private groupBy: P<string list> =
    let keys = sepBy (tagWord .>> ws) (pchar ',' >>. ws)
    attempt (ws >>. pstring "by" >>. ws >>. pchar '{') >>. ws >>. keys .>> pchar '}'

// --- literals: what modifiers and named arguments take -------------------------

/// `'mean'` or `"UTC"`. No escapes: nothing in Datadog's documented arguments
/// needs them.
let private quoted: P<string> =
    let inQuotes (q: char) = between (pchar q) (pchar q) (manySatisfy ((<>) q))
    inQuotes '\'' <|> inQuotes '"'

/// `60`, `0.5`, `1e6`. Not `pfloat`: that also reads `NaN` and `Infinity`, so
/// a metric named `nan.errors` would start as a number. The sign is left to
/// the expression layer's unary minus.
let private number: P<float> =
    numberLiteral (NumberLiteralOptions.AllowFraction ||| NumberLiteralOptions.AllowExponent) "number"
    |>> fun n -> float n.String

let private literal: P<Literal> =
    choice [ number |>> Num; quoted |>> Quoted; identifier |>> Word ] .>> ws

/// `.rollup(sum, 60)`, `.as_count()`.
let private modifier: P<Modifier> =
    let args = between (pchar '(' >>. ws) (pchar ')') (sepBy literal (pchar ',' >>. ws))
    // attempt: a `.` not followed by name-and-parenthesis is not ours to eat.
    attempt (ws >>. pchar '.' >>. identifier .>>. (ws >>. args))
    |>> fun (name, args) -> { Name = name; Args = args }

// --- the query ------------------------------------------------------------------

/// Modifiers are accepted both before and after `by {...}` and kept in source
/// order.
///
/// WARNING(undocumented): Datadog's own queries put modifiers after `by`;
/// whether it also accepts them before is unknown. We accept both, because
/// accepting more than Datadog is the cheaper error.
let private body: P<TagFilter * string list * Modifier list> =
    tuple4 tagFilter (many modifier) (opt groupBy) (many modifier)
    |>> fun (filter, before, groups, after) -> filter, defaultArg groups [], before @ after

let private plainQuery: P<MetricQuery> =
    spaceAgg .>>. metricName .>>. body
    |>> fun ((agg, metric), (filter, groups, mods)) ->
        { SpaceAgg = agg; Metric = metric; Filter = filter; GroupBy = groups; Modifiers = mods }

/// `sum:(gauge{*}).weighted()` — the documented form for `weighted()`: the
/// aggregator outside the parentheses, the rest of the query inside, and
/// modifiers after the closing one.
let private parenthesisedQuery: P<MetricQuery> =
    attempt (identifier .>> pchar ':' .>> ws .>> pchar '(') .>> ws
    .>>. plainQuery .>> ws .>> pchar ')' .>>. many modifier
    |>> fun ((agg, inner), mods) ->
        { inner with SpaceAgg = Some agg; Modifiers = inner.Modifiers @ mods }

let metricQuery: P<MetricQuery> = parenthesisedQuery <|> plainQuery

let parseMetricQuery (input: string) : Result<MetricQuery, string> =
    match run (ws >>. metricQuery .>> ws .>> eof) input with
    | Success(result, _, _) -> Result.Ok result
    | Failure(message, _, _) -> Result.Error message

// ---------------------------------------------------------------------------
// Expressions: arithmetic and function calls over a leaf.
//
//   expr    = term   (('+' | '-') term)*        lowest precedence
//   term    = unary  (('*' | '/') unary)*
//   unary   = '-' unary | primary
//   primary = number | '(' expr ')' | call | leaf
//   call    = name '(' arg, arg, ... ')'
//
// Written once, as a function of the leaf parser, and used twice: with metric
// queries as leaves for v1 `query=`, with query names for v2 `formula`.
// ---------------------------------------------------------------------------

/// How a bare word inside a call is read — the one place the two languages
/// differ beyond the leaf.
type private BareWords =
    /// v1: a leaf needs braces (`x{*}`), so a word with none, like `mean` in
    /// `top(x{*}, 5, mean, desc)`, can only be a literal.
    | AsLiterals
    /// v2: `query1` is both a leaf and a bare word. It is read as a leaf;
    /// analysis turns it back into a word where the function wants one.
    | AsLeaves

let private expressionOf (bareWords: BareWords) (leaf: P<'Leaf>) : P<Expr<'Leaf>> =
    let expr, exprRef = createParserForwardedToRef<Expr<'Leaf>, unit> ()

    /// Tried in this order because the forms overlap:
    ///   direction='above'   named — looks like a bare word until the `=`
    ///   'mean', "UTC"       quoted
    ///   mean                bare word, v1 only — when `,` or `)` follows
    ///   a{*} / 2, -3600     any expression
    let arg: P<Arg<'Leaf>> =
        let named = attempt (identifier .>> ws .>> pchar '=' .>> ws) .>>. literal |>> Named
        let quotedArg = quoted .>> ws |>> (Quoted >> Lit)
        let bare = attempt (identifier .>> ws .>> followedBy (pchar ',' <|> pchar ')')) |>> (Word >> Lit)

        match bareWords with
        | AsLiterals -> choice [ named; quotedArg; bare; expr |>> Value ]
        | AsLeaves -> choice [ named; quotedArg; expr |>> Value ]

    /// `exclude_null(...)`. `attempt` on name-plus-parenthesis: without the
    /// `(` the name was the start of a leaf, and must be read again as one.
    let call =
        attempt (identifier .>> ws .>> pchar '(') .>> ws
        .>>. sepBy (arg .>> ws) (pchar ',' >>. ws)
        .>> pchar ')'
        |>> Call

    let primary =
        choice [
            number |>> Number
            between (pchar '(' >>. ws) (pchar ')') expr
            call
            leaf |>> Leaf
        ]
        .>> ws

    let unary, unaryRef = createParserForwardedToRef<Expr<'Leaf>, unit> ()

    unaryRef.Value <-
        (pchar '-' >>. ws >>. unary
         |>> function
             // `-3600` is a number, not an operation on one — keeps
             // `timeshift(q, -3600)` a plain literal for everything downstream.
             | Number n -> Number -n
             | e -> Neg e)
        <|> primary

    /// Left-associative, so `a - b - c` is `(a - b) - c`, as in arithmetic.
    let binary (ops: (char * BinaryOp) list) (next: P<Expr<'Leaf>>) =
        let op = choice [ for c, o in ops -> pchar c >>. ws >>% fun l r -> Binary(o, l, r) ]
        chainl1 next op

    exprRef.Value <- binary [ '+', Add; '-', Sub ] (binary [ '*', Mul; '/', Div ] unary)
    expr

let private toResult =
    function
    | Success(result, _, _) -> Result.Ok result
    | Failure(message, _, _) -> Result.Error message

// --- v2 formula ------------------------------------------------------------------

/// A query name in a formula: `a`, `query1`, `my_query_1`. Whether it names a
/// query in the request is checked later — Datadog itself stores formulas
/// naming queries that do not exist.
let private queryName: P<string> = identifier

let formula: P<Formula> = ws >>. expressionOf AsLeaves queryName .>> eof

let parseFormula (input: string) : Result<Formula, string> = run formula input |> toResult

// --- v1 query= -------------------------------------------------------------------

/// A whole v1 `query=`: one or more expressions, separated by commas.
let program: P<Program> =
    ws >>. sepBy1 (expressionOf AsLiterals metricQuery) (pchar ',' >>. ws) .>> eof

let parseProgram (input: string) : Result<Program, string> = run program input |> toResult
