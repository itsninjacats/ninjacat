module NinjaCat.Api.Monitors.Detached

open System
open System.Threading
open System.Threading.Tasks
open Akka.Actor

/// Runs slow work off the actor's thread and pipes its outcome back as a message; a throw becomes `failed`.
let run (ct: CancellationToken) (self: IActorRef) (failed: exn -> 'M) (work: CancellationToken -> Task<'M>) : unit =
    Task
        .Run<'M>(Func<Task<'M>>(fun () -> work ct))
        .PipeTo(self, ActorRefs.NoSender, (fun outcome -> box outcome), (fun e -> box (failed e)))
    |> ignore
