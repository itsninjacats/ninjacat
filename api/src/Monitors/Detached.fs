/// Slow work of an actor, off its thread. A handler that awaited would hold
/// the mailbox for the whole await, so the work runs as a task and its outcome
/// comes back as a message.
module NinjaCat.Api.Monitors.Detached

open System
open System.Threading
open System.Threading.Tasks
open Akka.Actor

/// Runs `work` and pipes its outcome to `self`; a thrown exception becomes
/// `failed`'s message. An outcome that arrives after the actor stopped goes to
/// dead letters.
let run (ct: CancellationToken) (self: IActorRef) (failed: exn -> 'M) (work: CancellationToken -> Task<'M>) : unit =
    Task
        .Run<'M>(Func<Task<'M>>(fun () -> work ct))
        .PipeTo(self, ActorRefs.NoSender, (fun outcome -> box outcome), (fun e -> box (failed e)))
    |> ignore
