/// Durations as people write them: in a query, a URL or a setting.
module NinjaCat.Api.Engine.Duration

open System
open System.Globalization
open System.Text.RegularExpressions

let private seconds: Map<string, float> =
    Map
        [ "ns", 1e-9
          "us", 1e-6
          "µs", 1e-6
          "ms", 1e-3
          "s", 1.0
          "m", 60.0
          "h", 3600.0
          "d", 86400.0
          "w", 604800.0 ]

let private pair = @"(\d+(?:\.\d*)?|\.\d+)(ns|us|µs|ms|s|m|h|d|w)"

/// A sign, then one or more number-and-unit pairs: "15s", "-1h30m", "1.5h",
/// "300ms", "7d", "2w".
let parse (text: string) : Result<TimeSpan, string> =
    let m = Regex.Match(text, $"^([+-])?(?:{pair})+$")

    if not m.Success then
        Error $"invalid duration \"{text}\""
    else
        let total =
            Regex.Matches(text, pair)
            |> Seq.sumBy (fun part -> Double.Parse(part.Groups[1].Value, CultureInfo.InvariantCulture) * seconds[part.Groups[2].Value])

        Ok(TimeSpan.FromSeconds(if m.Groups[1].Value = "-" then -total else total))
