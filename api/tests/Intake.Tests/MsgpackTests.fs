module NinjaCat.Api.Intake.Tests.MsgpackTests

open Xunit
open NinjaCat.Api.Intake

let private decode (bytes: byte list) = Msgpack.decode (Array.ofList bytes)

[<Fact>]
let ``scalars`` () =
    Assert.Equal(Ok MsgNil, decode [ 0xc0uy ])
    Assert.Equal(Ok(MsgBool true), decode [ 0xc3uy ])
    Assert.Equal(Ok(MsgUInt 7UL), decode [ 0x07uy ])
    Assert.Equal(Ok(MsgInt -1L), decode [ 0xffuy ])
    Assert.Equal(Ok(MsgUInt 300UL), decode [ 0xcduy; 0x01uy; 0x2cuy ])
    Assert.Equal(Ok(MsgInt -300L), decode [ 0xd1uy; 0xfeuy; 0xd4uy ])
    Assert.Equal(Ok(MsgFloat 1.5), decode [ 0xcbuy; 0x3fuy; 0xf8uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy ])
    Assert.Equal(Ok(MsgFloat32 1.5f), decode [ 0xcauy; 0x3fuy; 0xc0uy; 0uy; 0uy ])
    Assert.Equal(Ok(MsgStr "hi"), decode [ 0xa2uy; byte 'h'; byte 'i' ])

[<Fact>]
let ``containers keep their order`` () =
    // {"a": [1, nil], "b": "x"}
    let body = [ 0x82uy; 0xa1uy; byte 'a'; 0x92uy; 0x01uy; 0xc0uy; 0xa1uy; byte 'b'; 0xa1uy; byte 'x' ]
    Assert.Equal(Ok(MsgMap [ "a", MsgArray [ MsgUInt 1UL; MsgNil ]; "b", MsgStr "x" ]), decode body)

[<Fact>]
let ``a forged length is refused before anything is allocated`` () =
    // array32 claiming four billion elements, with no elements behind it.
    Assert.True(Result.isError (decode [ 0xdduy; 0xffuy; 0xffuy; 0xffuy; 0xffuy ]))

[<Fact>]
let ``an empty or truncated body is an error`` () =
    Assert.True(Result.isError (decode []))
    Assert.True(Result.isError (decode [ 0xa5uy; byte 'h' ]))

[<Fact>]
let ``fields are read by name: the last of two equal keys wins, nil is absent`` () =
    // {"n": 1, "s": "a", "n": 7, "gone": nil}
    let body = [ 0x84uy; 0xa1uy; byte 'n'; 0x01uy; 0xa1uy; byte 's'; 0xa1uy; byte 'a'; 0xa1uy; byte 'n'; 0x07uy; 0xa4uy; byte 'g'; byte 'o'; byte 'n'; byte 'e'; 0xc0uy ]

    match decode body with
    | Error e -> Assert.Fail e
    | Ok root ->
        let problems = ResizeArray<string>()
        let fields = MsgFields(problems, "", root)
        Assert.Equal((7UL, "a", "", 0L), (fields.UInt64 "n", fields.String "s", fields.String "gone", fields.Int64 "missing"))
        Assert.Empty problems
        Assert.Equal<Map<string, MsgValue>>(Map [ "gone", MsgNil; "s", MsgStr "a" ], fields.Unknown(set [ "n" ]))

[<Fact>]
let ``a field of the wrong type keeps its default and is noted with its path`` () =
    // {"items": [{"count": "three"}, nil, {"count": -1}]}
    let item (value: byte list) = [ 0x81uy; 0xa5uy; byte 'c'; byte 'o'; byte 'u'; byte 'n'; byte 't' ] @ value

    let body =
        [ 0x81uy; 0xa5uy; byte 'i'; byte 't'; byte 'e'; byte 'm'; byte 's'; 0x93uy ]
        @ item [ 0xa5uy; byte 't'; byte 'h'; byte 'r'; byte 'e'; byte 'e' ]
        @ [ 0xc0uy ]
        @ item [ 0xffuy ]

    match decode body with
    | Error e -> Assert.Fail e
    | Ok root ->
        let problems = ResizeArray<string>()
        let items = MsgFields(problems, "", root).Maps "items"
        // The nil element is left out, and the others keep their place in the path.
        Assert.Equal<uint64 list>([ 0UL; 0UL ], items |> List.map (fun item -> item.UInt64 "count"))

        Assert.Equal<string list>(
            [ "items/0/count: expected an unsigned integer, got a string"
              "items/2/count: expected an unsigned integer, got the integer -1" ],
            List.ofSeq problems
        )

[<Fact>]
let ``a map key that is not a string is refused`` () =
    // {bin"a": 1}
    Assert.True(Result.isError (decode [ 0x81uy; 0xc4uy; 0x01uy; byte 'a'; 0x01uy ]))

[<Fact>]
let ``a body nested without end is an error, not a stack overflow`` () =
    // A hundred thousand one-element array headers, one inside the other.
    match Msgpack.decode (Array.create 100_000 0x91uy) with
    | Error message -> Assert.Contains("nesting", message)
    | Ok _ -> Assert.Fail "decoded"
