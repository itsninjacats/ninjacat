/// Tests of JsonFields.fs: the one reader of a JSON object's members.
module NinjaCat.Api.Intake.Tests.JsonFieldsTests

open System
open System.Text
open System.Text.Json
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Tests.Golden.Requests

/// The object's fields, and the list its mismatches go to.
let private read (text: string) : JsonFields.Fields * JsonFields.Mismatches =
    let bad = JsonFields.Mismatches()
    JsonFields.fields bad "" (json text), bad

[<Fact>]
let ``a member is found by its exact name, and of two the last counts`` () =
    let upper, _ = read """{"NAME":"x"}"""
    Assert.Equal("", upper.String "name")
    let twice, _ = read """{"name":"a","Name":"b","name":"c"}"""
    Assert.Equal("c", twice.String "name")
    // Null is the same as absent, also when an earlier member had a value.
    let thenNull, _ = read """{"name":"a","name":null}"""
    Assert.Equal("", thenNull.String "name")

[<Fact>]
let ``null and a missing key leave every kind of field at its zero value, without a mismatch`` () =
    let f, bad =
        read """{"s":null,"n":null,"i":null,"u":null,"f":null,"b":null,"list":null,"o":null,"items":null}"""

    Assert.Equal("", f.String "s")
    Assert.Equal("", f.Number "n")
    Assert.Equal(0L, f.Int64 "i")
    Assert.Equal(0UL, f.UInt64 "u")
    Assert.Equal(0.0, f.Float "f")
    Assert.False(f.Bool "b")
    Assert.Equal(None, f.OptionalBool "b")
    Assert.Equal(None, f.OptionalInt32 "i")
    Assert.Empty(f.Strings "list")
    Assert.Equal("", (f.Object "o").String "x")
    Assert.True((f.OptionalObject "o").IsNone)
    Assert.Empty(f.Objects "items")
    Assert.Equal("", f.String "missing")
    Assert.Empty bad

[<Theory>]
[<InlineData("""{"v":5}""")>]
[<InlineData("""{"v":true}""")>]
[<InlineData("""{"v":{}}""")>]
let ``a string field refuses anything but a string`` (text: string) =
    let f, bad = read text
    Assert.Equal("", f.String "v")
    Assert.Single bad |> ignore

[<Theory>]
[<InlineData("1.0")>]
[<InlineData("1e2")>]
[<InlineData("\"1\"")>]
[<InlineData("2147483648")>]
let ``an int32 is read from plain digits within its range`` (literal: string) =
    let f, bad = read $"""{{"v":{literal}}}"""
    Assert.Equal(0, f.Int32 "v")
    Assert.Single bad |> ignore

[<Fact>]
let ``integers at the edges of their types`` () =
    let f, bad =
        read """{"zero":-0,"max":18446744073709551615,"port":65535,"small":-2147483648,"big":9223372036854775807}"""

    Assert.Equal(0, f.Int32 "zero")
    Assert.Equal(UInt64.MaxValue, f.UInt64 "max")
    Assert.Equal(65535us, f.UInt16 "port")
    Assert.Equal(Int32.MinValue, f.Int32 "small")
    Assert.Equal(Int64.MaxValue, f.Int64 "big")
    Assert.Empty bad

[<Theory>]
[<InlineData("-1")>]
[<InlineData("-0")>]
[<InlineData("18446744073709551616")>]
[<InlineData("1.0")>]
let ``an unsigned integer refuses a sign, a fraction and an overflow`` (literal: string) =
    let f, bad = read $"""{{"v":{literal}}}"""
    Assert.Equal(0UL, f.UInt64 "v")
    Assert.Single bad |> ignore

[<Fact>]
let ``a uint16 and a uint32 refuse what does not fit`` () =
    let f, bad = read """{"port":65536,"index":4294967296}"""
    Assert.Equal(0us, f.UInt16 "port")
    Assert.Equal(0u, f.UInt32 "index")
    Assert.Equal(2, bad.Count)

[<Fact>]
let ``a float too large for its type is refused, a string is not a float`` () =
    let f, bad = read """{"huge":1e999,"single":1e39,"text":"1","fine":0.1}"""
    Assert.Equal(0.0, f.Float "huge")
    Assert.Equal(0.0f, f.Float32 "single")
    Assert.Equal(0.0, f.Float "text")
    Assert.Equal(0.1f, f.Float32 "fine")
    Assert.Equal(3, bad.Count)

[<Fact>]
let ``a number as text keeps the digits as written, and takes a number inside a string`` () =
    let f, bad = read """{"a":1.50,"b":1E3,"c":"123"}"""
    Assert.Equal("1.50", f.Number "a")
    Assert.Equal("1E3", f.Number "b")
    Assert.Equal("123", f.Number "c")
    Assert.Empty bad

[<Theory>]
[<InlineData("\"abc\"")>]
[<InlineData("\"\"")>]
[<InlineData("\"12\\n\"")>]
[<InlineData("true")>]
let ``a number as text refuses what is not a number`` (literal: string) =
    let f, bad = read $"""{{"v":{literal}}}"""
    Assert.Equal("", f.Number "v")
    Assert.Single bad |> ignore

[<Fact>]
let ``a bool is a bool, and an optional one tells false from absent`` () =
    let f, bad = read """{"yes":true,"no":false,"number":1}"""
    Assert.Equal(Some true, f.OptionalBool "yes")
    Assert.Equal(Some false, f.OptionalBool "no")
    Assert.Equal(None, f.OptionalBool "absent")
    Assert.Empty bad
    Assert.Equal(None, f.OptionalBool "number")
    Assert.Single bad |> ignore

[<Fact>]
let ``a list of strings takes null as an empty string, and refuses anything else`` () =
    let f, bad = read """{"a":["x",null,"y"],"b":[],"c":["x",5],"d":"x"}"""
    Assert.Equal<string[]>([| "x"; ""; "y" |], f.Strings "a")
    Assert.Empty(f.Strings "b")
    Assert.Empty bad
    Assert.Empty(f.Strings "c")
    Assert.Empty(f.Strings "d")
    Assert.Equal(2, bad.Count)

[<Fact>]
let ``a raw member is the text as written, null included, and empty when absent`` () =
    let f, _ = read """{"a" :  [ 1 , 2 ] ,"b":null}"""
    Assert.Equal("[ 1 , 2 ]", f.Raw "a")
    Assert.Equal("null", f.Raw "b")
    Assert.Equal("", f.Raw "c")

[<Fact>]
let ``an object member that is not an object is refused; an optional one is None only when null or missing`` () =
    let f, bad = read """{"a":"x","b":[],"c":{},"d":{"ip":"1"}}"""
    Assert.Equal("", (f.Object "a").String "ip")
    Assert.Equal("", (f.Object "b").String "ip")
    Assert.Equal(2, bad.Count)
    Assert.True((f.OptionalObject "c").IsSome)
    Assert.Equal("1", (f.Object "d").String "ip")
    Assert.True((f.OptionalObject "missing").IsNone)
    Assert.Equal(2, bad.Count)

[<Fact>]
let ``a list of objects reads a null element as an empty object, and refuses the rest`` () =
    let f, bad = read """{"ok":[null,{"ip":"a"}],"mixed":[5,{"ip":"b"}],"wrong":{}}"""
    Assert.Equal<string list>([ ""; "a" ], f.Objects "ok" |> List.map (fun item -> item.String "ip"))
    Assert.Empty bad
    Assert.Equal<string list>([ ""; "b" ], f.Objects "mixed" |> List.map (fun item -> item.String "ip"))
    Assert.Single bad |> ignore
    Assert.Empty(f.Objects "wrong")
    Assert.Equal(2, bad.Count)

[<Fact>]
let ``a mismatch says where it is, and the rest of the document is still read`` () =
    let f, bad = read """{"devices":[{"id":"a"},{"id":7,"name":"second"}]}"""
    let devices = f.Objects "devices"
    Assert.Equal<string list>([ "a"; "" ], devices |> List.map (fun d -> d.String "id"))
    Assert.Equal("second", devices[1].String "name")
    Assert.Equal<string list>([ "devices[1].id: expected a string, got number" ], List.ofSeq bad)

[<Fact>]
let ``a root that is not an object is refused, and null is an empty object`` () =
    let bad = JsonFields.Mismatches()
    Assert.Equal("", (JsonFields.fields bad "" (json "null")).String "x")
    Assert.Empty bad
    Assert.Equal("", (JsonFields.fields bad "0" (json "[]")).String "x")
    Assert.Equal<string list>([ "0: expected an object, got array" ], List.ofSeq bad)

[<Fact>]
let ``members: the last of two equal keys wins, and only an object has any`` () =
    let found = Json.members (json """{"a":1,"b":2,"a":3}""")
    Assert.Equal<string list>([ "a"; "b" ], found.Keys |> List.ofSeq)
    Assert.Equal("3", found["a"].GetRawText())
    Assert.True((Json.members (json "null")).IsEmpty)
    Assert.True((Json.members (json "[1]")).IsEmpty)

[<Fact>]
let ``compactSorted writes sorted keys, no whitespace, and numbers digit for digit`` () =
    let value = json """{ "c": {"z": 1, "a": 2}, "a": 1.50, "b": [1E3, "x", null, true], "n": 9007199254740993 }"""
    Assert.Equal("""{"a":1.50,"b":[1E3,"x",null,true],"c":{"a":2,"z":1},"n":9007199254740993}""", Json.compactSorted value)
    Assert.Equal("\"s\"", Json.compactSorted (json "\"s\""))

[<Fact>]
let ``parse reads nesting deeper than System.Text.Json's default limit`` () =
    let deep = String('[', 200) + String(']', 200)
    Assert.True((Json.tryParse (Encoding.UTF8.GetBytes deep)).IsOk)
    Assert.True((Json.tryParse (Encoding.UTF8.GetBytes "not json")).IsError)

[<Fact>]
let ``half a surrogate pair reads as U+FFFD, a whole pair and an escaped backslash are left alone`` () =
    // System.Text.Json alone throws on the first.
    let read (body: string) : string =
        match Json.tryParse (Encoding.UTF8.GetBytes body) with
        | Ok root -> root.GetProperty("v").GetString()
        | Error e -> failwith e

    Assert.Equal("a\uFFFDb", read """{"v":"a\ud800b"}""")
    Assert.Equal("\uFFFDz", read """{"v":"\uDFFFz"}""")
    Assert.Equal("\uFFFD\U0001F600\uFFFD", read """{"v":"\ud800\ud83d\ude00\udc00"}""")
    Assert.Equal("\\ud800", read """{"v":"\\ud800"}""")

[<Fact>]
let ``bytes that are not UTF-8 read as U+FFFD`` () =
    let body = Array.concat [ Encoding.UTF8.GetBytes """{"v":"a"""; [| 0xffuy |]; Encoding.UTF8.GetBytes "b\"}" ]

    match Json.tryParse body with
    | Ok root -> Assert.Equal("a\uFFFDb", root.GetProperty("v").GetString())
    | Error e -> failwith e

[<Fact>]
let ``a Unix time out of DateTime's range becomes the nearest instant`` () =
    Assert.Equal(DateTime(2023, 11, 14, 22, 13, 20, DateTimeKind.Utc), Time.fromUnixSeconds 1700000000L)
    Assert.Equal(DateTime(9999, 12, 31, 23, 59, 59, DateTimeKind.Utc), Time.fromUnixSeconds Int64.MaxValue)
    Assert.Equal(DateTime(1, 1, 1, 0, 0, 0, DateTimeKind.Utc), Time.fromUnixMillis Int64.MinValue)
    Assert.Equal(DateTime(9999, 12, 31, 23, 59, 59, 999, DateTimeKind.Utc), Time.fromUnixMillis Int64.MaxValue)

[<Fact>]
let ``a member that has to be there is noted when it is missing or null, and so is one that is not acceptable`` () =
    let f, bad = read """{"a":1,"b":null}"""
    f.Require "a"
    f.Require "b"
    f.Require "c"
    f.Invalid("a", "expected 2 to 3, got 1")
    Assert.Equal<string list>([ "b: missing"; "c: missing"; "a: expected 2 to 3, got 1" ], List.ofSeq bad)
    Assert.Equal<string list>([ "a"; "b" ], f.Members.Keys |> List.ofSeq)

[<Fact>]
let ``an optional member is None when missing, null, or of the wrong type`` () =
    let f, bad = read """{"s":"x","n":7,"list":["a"],"null":null}"""
    Assert.Equal(Some "x", f.OptionalString "s")
    Assert.Equal(Some 7L, f.OptionalInt64 "n")
    Assert.Equal<string[] option>(Some [| "a" |], f.OptionalStrings "list")
    Assert.Equal(None, f.OptionalString "null")
    Assert.Equal(None, f.OptionalInt64 "missing")
    Assert.Empty bad
    Assert.Equal(None, f.OptionalString "n")
    Assert.Equal<string list>([ "n: expected a string, got number" ], List.ofSeq bad)
