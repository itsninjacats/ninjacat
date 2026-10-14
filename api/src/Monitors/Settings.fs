module NinjaCat.Api.Monitors.Settings

open System
open NinjaCat.Api.Platform

let systemName = "ninjacat-monitors"

type Formation =
    | Seeds of string list
    /// Without a selector, the pods labelled `app=ninjacat-monitors`.
    | Kubernetes of podLabelSelector: string option

type Node =
    { Host: string
      Port: int
      Formation: Formation
      ManagementPort: int }

type Settings =
    { Platform: Config.Config
      Concurrency: int
      /// More is an error, never a truncation.
      MaxGroups: int
      Node: Node
      HealthAddr: string }

let private optional (key: string) : string option =
    match Environment.GetEnvironmentVariable key with
    | null
    | "" -> None
    | value -> Some value

/// A set value that is not a positive whole number stops the start rather
/// than falling back to the default.
let private positive (key: string) (fallback: int) : int =
    match optional key with
    | None -> fallback
    | Some raw ->
        match Int32.TryParse raw with
        | true, n when n > 0 -> n
        | _ -> failwith $"{key} must be a positive whole number, got \"{raw}\""

let private node () : Node =
    let host = optional "NINJACAT_MONITOR_HOST"
    let port = positive "NINJACAT_MONITOR_PORT" 2552

    let formation, advertised =
        match optional "NINJACAT_MONITOR_DISCOVERY", host with
        | None, _ ->
            let advertised = host |> Option.defaultValue "localhost"
            Seeds [ $"akka.tcp://{systemName}@{advertised}:{port}" ], advertised
        // Bound to localhost, the node would be listed by discovery and never
        // answer, and the cluster would not form.
        | Some "kubernetes", None -> failwith "NINJACAT_MONITOR_HOST must be the pod's IP with NINJACAT_MONITOR_DISCOVERY=kubernetes"
        | Some "kubernetes", Some advertised -> Kubernetes(optional "NINJACAT_MONITOR_POD_SELECTOR"), advertised
        | Some other, _ -> failwith $"NINJACAT_MONITOR_DISCOVERY must be \"kubernetes\" or unset, got \"{other}\""

    { Host = advertised
      Port = port
      Formation = formation
      ManagementPort = positive "NINJACAT_MONITOR_MANAGEMENT_PORT" 8558 }

let load () : Settings =
    { Platform = Config.load ()
      Concurrency = positive "NINJACAT_MONITOR_CONCURRENCY" 8
      MaxGroups = positive "NINJACAT_MONITOR_MAX_GROUPS" 1000
      Node = node ()
      HealthAddr = optional "NINJACAT_MONITOR_HEALTH_ADDR" |> Option.defaultValue ":8559" }
