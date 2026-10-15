// A JSON parser in FParsec, built one rule at a time.
// Run:  dotnet fsi api/learn/json.fsx
// Simplified on purpose: strings have no escapes (\" \n ...).

#r "nuget: FParsec, 1.1.1"
open FParsec

let test label (p: Parser<'T, unit>) (input: string) =
    match run (p .>> eof) input with
    | Success(result, _, _) -> printfn "%-10s %-32s → %A" label input result
    | Failure(msg, _, _) -> printfn "%-10s %-32s → FAIL\n%s" label input (msg.TrimEnd())

// ---- STEP 1: what we want at the end — the boxes -----------------------------
type Json =
    | JNull
    | JBool of bool
    | JNumber of float
    | JString of string
    | JArray of Json list
    | JObject of (string * Json) list

// ---- STEP 2: the grammar in words ---------------------------------------------
//   value  = null | true | false | number | string | array | object
//   array  = "[" value, value, ... "]"
//   object = "{" string ":" value, ... "}"
//
// Note: value mentions array, and array mentions value. That circle is the
// whole difficulty, and FParsec has one tool for it (step 3e).

// ---- STEP 3: one small parser per line of the grammar ------------------------

// 3a. the simplest ones: fixed words
let jnull  = pstring "null"  >>% JNull
let jtrue  = pstring "true"  >>% JBool true
let jfalse = pstring "false" >>% JBool false
printfn "\n--- 3a. fixed words"
test "jnull" jnull "null"
test "jtrue" jtrue "true"

// 3b. a number: FParsec already has one
let jnumber = pfloat |>> JNumber
printfn "\n--- 3b. number"
test "jnumber" jnumber "42"
test "jnumber" jnumber "-3.5"

// 3c. a string: a quote, any characters except a quote, a quote
let stringLiteral = between (pchar '"') (pchar '"') (manySatisfy (fun c -> c <> '"'))
let jstring = stringLiteral |>> JString
printfn "\n--- 3c. string"
test "jstring" jstring "\"hello\""

// 3d. a helper: the same parser, but skipping spaces after it
let ws = spaces
let token (p: Parser<'T, unit>) = p .>> ws

// 3e. the circle: value needs array, array needs value.
//     Make a placeholder for value now, fill it in later.
let jvalue, jvalueRef = createParserForwardedToRef<Json, unit> ()

// 3f. array: [ value , value , ... ]  — uses jvalue, which is still empty
let jarray =
    between (token (pchar '[')) (pchar ']') (sepBy (token jvalue) (token (pchar ',')))
    |>> JArray

// 3g. object: { "key" : value , ... }
let pair = token stringLiteral .>> token (pchar ':') .>>. token jvalue
let jobject =
    between (token (pchar '{')) (pchar '}') (sepBy pair (token (pchar ',')))
    |>> JObject

// 3h. fill in the placeholder: a value is ONE of these
jvalueRef.Value <- choice [ jnull; jtrue; jfalse; jnumber; jstring; jarray; jobject ]

// ---- try it -------------------------------------------------------------------
printfn "\n--- 3f. array"
test "jarray" jarray "[1, 2, 3]"
test "jarray" jarray "[]"
printfn "\n--- 3g. object"
test "jobject" jobject "{\"a\": 1}"
printfn "\n--- whole values, nested"
test "jvalue" jvalue "[1, [2, [3]]]"
test "jvalue" jvalue "{\"env\": \"prod\", \"tags\": [\"a\", \"b\"], \"on\": true}"
printfn "\n--- errors"
test "jvalue" jvalue "[1, 2"
test "jvalue" jvalue "{\"a\" 1}"
