/// What only the monitor service reads. The connections, the metrics
/// retention and self-monitoring are the shared Config's.
module NinjaCat.Api.Monitors.Settings

open System
open NinjaCat.Api.Platform

type Settings =
    { Platform: Config.Config
      /// Loads and evaluations running at once on this node
      /// (NINJACAT_MONITOR_CONCURRENCY, 8).
      Concurrency: int
      /// The most groups one evaluation may yield (NINJACAT_MONITOR_MAX_GROUPS,
      /// 1000); more is an error, never a truncation.
      MaxGroups: int
      /// Where other nodes reach this one over Akka remoting
      /// (NINJACAT_MONITOR_HOST, NINJACAT_MONITOR_PORT).
      Host: string
      Port: int }

/// A set value that is not a positive whole number stops the start rather
/// than falling back to the default.
let private positive (key: string) (fallback: int) : int =
    match Environment.GetEnvironmentVariable key with
    | null
    | "" -> fallback
    | raw ->
        match Int32.TryParse raw with
        | true, n when n > 0 -> n
        | _ -> failwith $"{key} must be a positive whole number, got \"{raw}\""

let load () : Settings =
    { Platform = Config.load ()
      Concurrency = positive "NINJACAT_MONITOR_CONCURRENCY" 8
      MaxGroups = positive "NINJACAT_MONITOR_MAX_GROUPS" 1000
      Host =
        match Environment.GetEnvironmentVariable "NINJACAT_MONITOR_HOST" with
        | null
        | "" -> "localhost"
        | host -> host
      Port = positive "NINJACAT_MONITOR_PORT" 2552 }
