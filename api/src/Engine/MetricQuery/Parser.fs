/// Parser for Datadog's metric query language, written with FParsec.
///
/// Three layers, each built on the one before:
///   1. the tag filter inside `{...}`
///   2. a metric query: `avg:system.cpu.user{env:prod} by {host}.rollup(sum, 60)`
///   3. expressions: arithmetic and function calls over metric queries (v1
///      `query=`) or over query names (v2 formulas)
///
/// A note on `attempt`: FParsec does not backtrack by default. Once a parser
/// has consumed input, the alternatives after it in `<|>` or `choice` are not
/// tried. `attempt p` makes `p` rewind to where it started when it fails.
/// Every `attempt` below says why that place needs it.
module NinjaCat.Api.Engine.MetricQuery.Parser

open System
open FParsec
open NinjaCat.Api.Engine.MetricQuery.Ast

type private P<'T> = Parser<'T, unit>

let private ws: P<unit> = spaces

let private toResult (result: ParserResult<'T, unit>) : Result<'T, string> =
    match result with
    | Success(value, _, _) -> Result.Ok value
    | Failure(message, _, _) -> Result.Error message

let private andOf (filters: TagFilter list) =
    match filters with
    | [ one ] -> one
    | many -> And many

let private orOf (filters: TagFilter list) =
    match filters with
    | [ one ] -> one
    | many -> Or many

// ---------------------------------------------------------------------------------------
// 1. Tag filter
//
// Datadog has two syntaxes for a filter and refuses a mix of them:
//   symbolic    {env:prod,!host:a}                  `,` is AND, `!` is NOT
//   functional  {env:prod AND NOT (host:a OR host:b) AND zone IN (x, y)}
// ---------------------------------------------------------------------------------------

// `*` is the wildcard, `$` a dashboard template variable (`{$env}`).
let private isTagChar (c: char) = Char.IsLetterOrDigit c || "_-:./*$".Contains c

let private tagWord: P<string> = many1Satisfy isTagChar <?> "tag"

/// `env:prod` is a Tag, `canary` a Bare tag. Only the first `:` splits, as
/// values may hold more (`url:http://x`). A key with nothing after its colon
/// takes the next word: Datadog accepts `{servicename: ec2}`.
let private term: P<TagFilter> =
    parse {
        let! word = tagWord

        match word.IndexOf ':' with
        | -1 -> return Bare word
        | colon when colon = word.Length - 1 ->
            do! ws
            let! value = tagWord
            return Tag(word.Substring(0, colon), value)
        | colon -> return Tag(word.Substring(0, colon), word.Substring(colon + 1))
    }

// --- symbolic ---------------------------------------------------------------------------

let private symbolicItem: P<TagFilter> =
    parse {
        let! negated = opt (pchar '!')
        let! t = term
        do! ws
        return if negated.IsSome then Not t else t
    }

let private symbolic: P<TagFilter> = sepBy1 symbolicItem (pchar ',' >>. ws) |>> andOf

// --- functional -------------------------------------------------------------------------

// Datadog documents each keyword in upper and lower case only: `AND`, `and`.
let private keywords = set [ "AND"; "and"; "OR"; "or"; "NOT"; "not"; "IN"; "in" ]

let private keyword (word: string) : P<unit> =
    // attempt: `AND` must not eat the first letters of a tag like `ANDROID`.
    attempt ((pstring word <|> pstring (word.ToLowerInvariant())) >>. notFollowedBy (satisfy isTagChar))
    >>. ws
    <?> word

// A function rather than a value, because it has to fit where a parser of any
// result type is expected, and F# allows no generic values.
let private mixedSyntax () : P<'T> =
    (pchar ',' <|> pchar '!') >>. fail "symbolic syntax (',' and '!') cannot be mixed with AND/OR/NOT/IN"

let private functionalTerm: P<TagFilter> =
    // attempt: a keyword where a term was expected is not a tag named `AND`;
    // rewind, so the caller reads it as the keyword.
    attempt (
        parse {
            let! word = lookAhead tagWord

            if keywords.Contains word then
                return! fail $"unexpected keyword '{word}'"
            else
                return! term
        }
    )

let private inList: P<string list> =
    between (pchar '(' >>. ws) (pchar ')' >>. ws) (sepBy (tagWord .>> ws) (pchar ',' >>. ws))

/// A term, or `key IN (…)` / `key NOT IN (…)`, where the term is the key.
let private termOrIn: P<TagFilter> =
    let isIn = keyword "IN" >>% false
    // attempt: `NOT` followed by something other than `IN` is not ours.
    let isNotIn = attempt (keyword "NOT" >>. keyword "IN") >>% true

    parse {
        let! t = functionalTerm
        do! ws
        let! negatedIn = opt (isIn <|> isNotIn)

        match negatedIn, t with
        | None, _ -> return t
        | Some negated, Bare key ->
            let! values = inList
            return if negated then Not(In(key, values)) else In(key, values)
        | Some _, _ -> return! fail "IN needs a bare tag key on its left, e.g. 'region IN (a, b)'"
    }

// NOT binds tighter than AND, and AND tighter than OR. The parsers refer to
// each other (a parenthesis holds a whole filter again), so they are declared
// first and defined after.
let private functional, private functionalRef = createParserForwardedToRef<TagFilter, unit> ()
let private unary, private unaryRef = createParserForwardedToRef<TagFilter, unit> ()

unaryRef.Value <-
    choice
        [ keyword "NOT" >>. unary |>> Not
          between (pchar '(' >>. ws) (pchar ')' >>. ws) functional
          mixedSyntax ()
          termOrIn ]

let private conjunction: P<TagFilter> = sepBy1 unary (keyword "AND") |>> andOf

functionalRef.Value <- sepBy1 conjunction (keyword "OR") |>> orOf

// --- the braces -------------------------------------------------------------------------

/// `{*}`, `{}`, or a filter in either syntax.
let tagFilter: P<TagFilter> =
    let close = pchar '}'
    let everything = ((pchar '*' >>. ws >>. close) <|> close) >>% All

    pchar '{' >>. ws
    >>. choice
        [ attempt everything
          // attempt: when the symbolic syntax does not reach `}` (it met an
          // AND, say), rewind to just after `{` and try the functional one.
          attempt (symbolic .>> close)
          // A `,` left after a functional filter means the syntaxes were mixed.
          functional .>> (close <|> mixedSyntax ()) ]

let parseTagFilter (input: string) : Result<TagFilter, string> =
    run (ws >>. tagFilter .>> ws .>> eof) input |> toResult

// ---------------------------------------------------------------------------------------
// 2. Metric query
//
//   avg:system.cpu.user{env:prod} by {host}.rollup(sum, 60)
//   agg metric          filter    group-by  modifiers
// ---------------------------------------------------------------------------------------

let private identifier: P<string> =
    many1Satisfy2 isLetter (fun c -> isLetter c || isDigit c || c = '_') <?> "identifier"

// Starts with a letter; later segments may start with a digit (`jetty.5xx_responses`).
let private metricName: P<string> =
    many1Satisfy2 isLetter (fun c -> isLetter c || isDigit c || c = '_' || c = '.') <?> "metric name"

// attempt: in `system.cpu.user{*}` the word `system` reads as an identifier too;
// with no `:` after it, rewind so it is read again as the metric name.
let private spaceAgg: P<string option> = opt (attempt (identifier .>> pchar ':'))

/// `by {host, env}`. Datadog accepts `{bar:baz}by{host}`, with no spaces.
let private groupBy: P<string list> =
    parse {
        // attempt: spaces not followed by `by {` belong to whatever comes next.
        do! attempt (ws >>. skipString "by" >>. ws >>. skipChar '{')
        do! ws
        let! keys = sepBy (tagWord .>> ws) (pchar ',' >>. ws)
        do! skipChar '}'
        return keys
    }

// `'mean'` or `"UTC"`. No escapes: no documented argument needs them.
let private quoted: P<string> =
    let inQuotes (q: char) = between (pchar q) (pchar q) (manySatisfy (fun c -> c <> q))
    inQuotes '\'' <|> inQuotes '"'

// Not pfloat: it also reads `NaN` and `Infinity`, which would make a metric
// named `nan.errors` start as a number. A minus sign is the expression layer's.
let private number: P<float> =
    numberLiteral (NumberLiteralOptions.AllowFraction ||| NumberLiteralOptions.AllowExponent) "number"
    |>> fun n -> float n.String

let private literal: P<Literal> =
    choice [ number |>> Num; quoted |>> Quoted; identifier |>> Word ] .>> ws

/// `.rollup(sum, 60)`, `.as_count()`
let private modifier: P<Modifier> =
    // attempt: a `.` not followed by a name and `(` is not a modifier.
    attempt (
        parse {
            do! ws
            do! skipChar '.'
            let! name = identifier
            do! ws
            let! args = between (pchar '(' >>. ws) (pchar ')') (sepBy literal (pchar ',' >>. ws))
            return { Name = name; Args = args }
        }
    )

let private plainQuery: P<MetricQuery> =
    parse {
        let! agg = spaceAgg
        let! metric = metricName
        let! filter = tagFilter
        // WARNING(undocumented): whether Datadog accepts modifiers before `by`.
        // Its own queries put them after; we accept both.
        let! before = many modifier
        let! groups = opt groupBy
        let! after = many modifier

        return
            { SpaceAgg = agg
              Metric = metric
              Filter = filter
              GroupBy = defaultArg groups []
              Modifiers = before @ after }
    }

/// `sum:(gauge{*}).weighted()`: the form Datadog documents for weighted().
let private parenthesisedQuery: P<MetricQuery> =
    parse {
        // attempt: `avg:` without a `(` after it is the start of a plain query.
        let! agg = attempt (identifier .>> pchar ':' .>> ws .>> pchar '(')
        do! ws
        let! inner = plainQuery
        do! ws
        do! skipChar ')'
        let! more = many modifier
        return { inner with SpaceAgg = Some agg; Modifiers = inner.Modifiers @ more }
    }

let metricQuery: P<MetricQuery> = parenthesisedQuery <|> plainQuery

let parseMetricQuery (input: string) : Result<MetricQuery, string> =
    run (ws >>. metricQuery .>> ws .>> eof) input |> toResult

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

/// v1 and v2 differ in one more thing: a bare word inside a call. In v1 a leaf
/// needs braces, so `mean` in `top(x{*}, 5, mean, desc)` can only be a word.
/// In v2 it could be a query named `mean`; it is read as a name, and analysis
/// turns it back into a word where the function expects one.
type private BareWords =
    | WordsAreLiterals
    | WordsAreLeaves

let private expressionOf (bareWords: BareWords) (leaf: P<'Leaf>) : P<Expr<'Leaf>> =
    let sum, sumRef = createParserForwardedToRef<Expr<'Leaf>, unit> ()

    // The forms overlap, so the order matters: `direction='above'` looks like a
    // bare word until its `=`, and a bare word looks like the start of a leaf.
    let argument: P<Arg<'Leaf>> =
        let named =
            parse {
                // attempt: a word with no `=` after it is not a named argument.
                let! name = attempt (identifier .>> ws .>> pchar '=' .>> ws)
                let! value = literal
                return Named(name, value)
            }

        let quotedWord = quoted .>> ws |>> fun s -> Lit(Quoted s)
        // attempt: a word is bare only when `,` or `)` follows it.
        let bareWord = attempt (identifier .>> ws .>> followedBy (pchar ',' <|> pchar ')')) |>> fun w -> Lit(Word w)
        let value = sum |>> Value

        match bareWords with
        | WordsAreLiterals -> choice [ named; quotedWord; bareWord; value ]
        | WordsAreLeaves -> choice [ named; quotedWord; value ]

    let call: P<Expr<'Leaf>> =
        parse {
            // attempt: a name without `(` after it was the start of a leaf.
            let! name = attempt (identifier .>> ws .>> pchar '(')
            do! ws
            let! args = sepBy (argument .>> ws) (pchar ',' >>. ws)
            do! skipChar ')'
            return Call(name, args)
        }

    let primary: P<Expr<'Leaf>> =
        choice [ number |>> Number; between (pchar '(' >>. ws) (pchar ')') sum; call; leaf |>> Leaf ] .>> ws

    let unary, unaryRef = createParserForwardedToRef<Expr<'Leaf>, unit> ()

    unaryRef.Value <-
        choice
            [ parse {
                  do! skipChar '-'
                  do! ws

                  match! unary with
                  // `-3600` is a number, not a negated one.
                  | Number n -> return Number -n
                  | e -> return Neg e
              }
              primary ]

    // Left to right, so `a - b - c` is `(a - b) - c`.
    let leftToRight (operators: (char * BinaryOp) list) (operand: P<Expr<'Leaf>>) : P<Expr<'Leaf>> =
        let operator = choice [ for c, op in operators -> pchar c >>. ws >>% op ]

        parse {
            let! first = operand
            let! rest = many (operator .>>. operand)
            return rest |> List.fold (fun left (op, right) -> Binary(op, left, right)) first
        }

    let product = leftToRight [ '*', Mul; '/', Div ] unary
    sumRef.Value <- leftToRight [ '+', Add; '-', Sub ] product
    sum

/// A v2 formula. Whether its names name queries is checked later: Datadog
/// itself stores formulas naming queries that do not exist.
let formula: P<Formula> = ws >>. expressionOf WordsAreLeaves identifier .>> eof

let parseFormula (input: string) : Result<Formula, string> = run formula input |> toResult

/// A v1 `query=`: one or more expressions, separated by commas.
let program: P<Program> =
    ws >>. sepBy1 (expressionOf WordsAreLiterals metricQuery) (pchar ',' >>. ws) .>> eof

let parseProgram (input: string) : Result<Program, string> = run program input |> toResult
