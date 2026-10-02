/// The golden fixtures: requests recorded from the Go server this intake
/// replaced, each with its answer and the rows it stored (see
/// Fixtures/README.md). A fixture changed on purpose since says so in its
/// `edited` key. The intake must produce what the fixture holds.
module NinjaCat.Api.Intake.Tests.GoldenTests

open Xunit
open NinjaCat.Api.Intake.Tests.Golden

let fixtures: obj[] seq =
    Replay.selected |> Seq.map (fun f -> [| box f.Id |])

[<Theory>]
[<MemberData(nameof fixtures)>]
let ``a recorded request is answered and stored as its fixture says`` (id: string) =
    let fixture = Replay.all |> List.find (fun f -> f.Id = id)

    match Replay.run fixture with
    | [] -> ()
    | differences -> Assert.Fail(String.concat "\n" (id :: differences))
