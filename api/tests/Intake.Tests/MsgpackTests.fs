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
    match decode [ 0xdduy; 0xffuy; 0xffuy; 0xffuy; 0xffuy ] with
    | Error message -> Assert.Contains("exceeds", message)
    | Ok value -> Assert.Fail $"decoded {value}"

[<Fact>]
let ``an empty or truncated body is an error`` () =
    Assert.True(Result.isError (decode []))
    Assert.True(Result.isError (decode [ 0xa5uy; byte 'h' ]))

[<Fact>]
let ``the reader walks a known layout`` () =
    // [ "svc", 42, nil ]
    let reader = MsgpackReader [| 0x93uy; 0xa3uy; byte 's'; byte 'v'; byte 'c'; 0x2auy; 0xc0uy |]
    Assert.Equal(3, reader.ReadArrayHeader())
    Assert.Equal("svc", reader.ReadString())
    Assert.Equal(42L, reader.ReadInt64())
    Assert.True(reader.TryReadNil())
    Assert.True reader.AtEnd

[<Fact>]
let ``a map key that is not a string is refused`` () =
    // {bin"a": 1}
    Assert.True(Result.isError (decode [ 0x81uy; 0xc4uy; 0x01uy; byte 'a'; 0x01uy ]))
