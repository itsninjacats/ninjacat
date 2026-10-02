// FParsec, step by step, on the tag filter from src/Engine/MetricQuery/Parser.fs.
// Run:  dotnet fsi api/learn/fparsec-lessons.fsx
// Change anything and rerun — that is the point of this file.

#r "nuget: FParsec, 1.1.1"
open FParsec

/// Runs a parser and prints the result or the error, so every lesson shows
/// exactly what FParsec returns.
let test label (p: Parser<'T, unit>) (input: string) =
    match run p input with
    | Success(result, _, pos) -> printfn "%-28s %-26s → OK   %A   (stopped at column %d)" label $"\"{input}\"" result pos.Column
    | Failure(msg, _, _) -> printfn "%-28s %-26s → FAIL\n%s" label $"\"{input}\"" (msg.TrimEnd())

printfn "\n=== 1. A parser is a value; run applies it to text ==="
test "pchar '{'" (pchar '{') "{env:prod}"
test "pchar '{'" (pchar '{') "env:prod"
test "pstring \"AND\"" (pstring "AND") "AND host:a"

printfn "\n=== 2. Reading many characters ==="
let isTagChar c = isLetter c || isDigit c || c = '_' || c = '-' || c = ':' || c = '.' || c = '*'
let tagWord = many1Satisfy isTagChar
test "tagWord" tagWord "env:prod,host:a"
test "tagWord" tagWord ",oops"

printfn "\n=== 3. Sequencing: the dot shows the side you keep ==="
test "pchar '{' .>>. tagWord" (pchar '{' .>>. tagWord) "{env:prod}"
test "pchar '{' >>. tagWord" (pchar '{' >>. tagWord) "{env:prod}"
test "{ >>. tagWord .>> }" (pchar '{' >>. tagWord .>> pchar '}') "{env:prod}"
test "between" (between (pchar '{') (pchar '}') tagWord) "{env:prod}"

printfn "\n=== 4. Transforming the result: |>> ==="
type TagFilter =
    | All
    | Tag of key: string * value: string
    | Bare of string
    | Not of TagFilter
    | And of TagFilter list
    | Or of TagFilter list
let splitTag (w: string) =
    match w.IndexOf ':' with
    | -1 -> Bare w
    | i -> Tag(w[.. i - 1], w[i + 1 ..])
let term = tagWord |>> splitTag
test "term" term "env:prod"
test "term" term "example"
test "pchar '*' >>% All" (pchar '*' >>% All) "*"

printfn "\n=== 5. Repetition and separators ==="
test "sepBy1 term (pchar ',')" (sepBy1 term (pchar ',')) "env:prod,host:a,example"
let item = (pchar '!' >>. term |>> Not) <|> term
test "sepBy1 item ','" (sepBy1 item (pchar ',')) "env:prod,!host:a"

printfn "\n=== 6. Choice <|> and THE rule: consumed input means no second chance ==="
// pstring is all-or-nothing, so it never shows the problem. A parser built
// from steps does: "NOT IN" reads NOT, then fails on IN, having eaten "NOT ".
let notIn = pstring "NOT" >>. spaces >>. pstring "IN"
let notTerm = pstring "NOT" >>. spaces >>. tagWord
test "notIn <|> notTerm" (notIn <|> notTerm) "NOT host:a"
test "attempt notIn <|> notTerm" (attempt notIn <|> notTerm) "NOT host:a"

printfn "\n=== 7. Our two syntaxes: why the symbolic try needs attempt ==="
let close = pchar '}'
let symbolic = sepBy1 item (pchar ',') |>> function [ x ] -> x | xs -> And xs
let kw (s: string) = attempt (pstring s .>> notFollowedBy (satisfy isTagChar)) .>> spaces
let functionalAnd = sepBy1 (term .>> spaces) (kw "AND") |>> function [ x ] -> x | xs -> And xs
let withoutAttempt = pchar '{' >>. ((symbolic .>> close) <|> (functionalAnd .>> close))
let withAttempt = pchar '{' >>. (attempt (symbolic .>> close) <|> (functionalAnd .>> close))
test "without attempt" withoutAttempt "{env:prod AND host:a}"
test "with attempt" withAttempt "{env:prod AND host:a}"
test "with attempt" withAttempt "{env:prod,!host:a}"

printfn "\n=== 8. Deciding from what was parsed: >>= ==="
// "servicename:" with nothing after the colon takes the next word as its value.
let termWithSpace =
    tagWord >>= fun w ->
        if w.EndsWith ":" then spaces >>. tagWord |>> fun v -> Tag(w.TrimEnd ':', v)
        else preturn (splitTag w)
test "termWithSpace" termWithSpace "servicename: ec2"
test "termWithSpace" termWithSpace "env:prod"

printfn "\n=== 9. Recursion: a parser that contains itself ==="
// NOT (a OR b) — the thing inside the parentheses is the whole grammar again.
let expr, exprRef = createParserForwardedToRef<TagFilter, unit> ()
let unary =
    choice [
        kw "NOT" >>. expr |>> Not
        between (pchar '(' .>> spaces) (pchar ')' .>> spaces) expr
        term .>> spaces
    ]
let chain k ctor p = sepBy1 p (kw k) |>> function [ x ] -> x | xs -> ctor xs
exprRef.Value <- chain "OR" Or (chain "AND" And unary)
test "expr" expr "a:1 OR b:2 AND c:3"
test "expr" expr "NOT (a:1 OR b:2)"

printfn "\n=== 10. Errors: labels with <?> and custom messages with fail ==="
test "tagWord (no label)" tagWord "}"
test "tagWord <?> \"tag\"" (tagWord <?> "tag") "}"
let noMix = pchar ',' >>. fail "symbolic ',' cannot be mixed with AND/OR"
test "functional then noMix" (functionalAnd .>> (close <|> noMix)) "a:1 AND b:2, c:3}"
