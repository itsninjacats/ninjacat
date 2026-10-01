/// The golden fixtures: every request the Go server's own tests sent, with
/// the answer and the rows Go produced (see Fixtures/README.md). The F#
/// intake must produce the same.
module NinjaCat.Api.Intake.Tests.GoldenTests

open Xunit
open NinjaCat.Api.Intake.Tests.Golden

let fixtures: obj[] seq =
    Replay.selected |> Seq.map (fun f -> [| box f.Id |])

[<Theory>]
[<MemberData(nameof fixtures)>]
let ``answers and stores what the Go server did`` (id: string) =
    let fixture = Replay.all |> List.find (fun f -> f.Id = id)

    match Replay.run fixture with
    | [] -> ()
    | differences -> Assert.Fail(String.concat "\n" (id :: differences))
