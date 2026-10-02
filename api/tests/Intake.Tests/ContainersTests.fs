/// Tests of Routers/Containers.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.ContainersTests

open System
open Xunit
open Google.Protobuf
open Google.Protobuf.WellKnownTypes
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Datadog.Contimage
open Datadog.Contlcycle
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

/// A fixed conversion time, so a test can tell the sender's value from the
/// receive time: the two things an absent timestamp must never become.
let private now = DateTime(2026, 9, 21, 12, 0, 0, DateTimeKind.Utc)

let private at (seconds: int64) : DateTime = DateTime.UnixEpoch.AddSeconds(float seconds)

let private lifecycle (events: Event list) : EventsPayload =
    let payload = EventsPayload(Host = "node1")
    payload.Events.AddRange events
    payload

let private containerEvent (eventType: Event.Types.EventType) (container: ContainerEvent) : Event =
    Event(EventType = eventType, Container = container)

let private containerRow (container: ContainerEvent) : ContainerEventRow =
    Containers.eventRows (lifecycle [ containerEvent Event.Types.EventType.Delete container ]) "t1" now |> Assert.Single

let private images (list: ContainerImage list) : ContainerImagePayload =
    let payload = ContainerImagePayload(Host = "node1")
    payload.Images.AddRange list
    payload

let private layer (size: int64) : ContainerImage.Types.ContainerImageLayer =
    ContainerImage.Types.ContainerImageLayer(Size = size)

let private imageWithLayers (id: string) (sizes: int64 list) : ContainerImage =
    let image = ContainerImage(Id = id)
    image.Layers.AddRange(sizes |> List.map layer)
    image

/// Sends a body through the containers intake as the agent would.
let private post (path: string) (body: byte[]) : Response * CapturingSink =
    Requests.send [ "routeContainers" ] "POST" path Requests.withKey body

// ---- lifecycle events ----

[<Fact>]
let ``no exit code is None, exit 0 is Some 0, an OOM kill is Some 137`` () =
    Assert.Equal(None, (containerRow (ContainerEvent(ContainerID = "a"))).ExitCode)
    Assert.Equal(Some 0, (containerRow (ContainerEvent(ContainerID = "b", ExitCode = 0))).ExitCode)
    Assert.Equal(Some 137, (containerRow (ContainerEvent(ContainerID = "c", ExitCode = 137))).ExitCode)

[<Fact>]
let ``an explicit exit 0 is still there after the wire`` () =
    let sent =
        lifecycle
            [ containerEvent Event.Types.EventType.Delete (ContainerEvent(ContainerID = "clean", ExitCode = 0))
              containerEvent Event.Types.EventType.Delete (ContainerEvent(ContainerID = "unknown")) ]

    let rows = Containers.eventRows (EventsPayload.Parser.ParseFrom(sent.ToByteArray())) "t1" now

    Assert.Equal(2, rows.Length)
    Assert.Equal(Some 0, rows[0].ExitCode)
    Assert.Equal(None, rows[1].ExitCode)

[<Fact>]
let ``absent timestamps stay None: never 1970, never the receive time`` () =
    // A transition stamped 0 is "the agent did not say when", not the epoch.
    let row = containerRow (ContainerEvent(ContainerID = "a", Transition = ContainerStateTransition()))

    Assert.Equal(None, row.CreatedAt)
    Assert.Equal(None, row.ExitedAt)
    Assert.Equal(None, row.TransitionAt)
    Assert.Equal(now, row.Timestamp)

[<Fact>]
let ``timestamps the sender supplied are kept`` () =
    let row =
        containerRow (
            ContainerEvent(
                ContainerID = "a",
                CreationTimestamp = 1700000000L,
                ExitTimestamp = 1700000600L,
                Transition = ContainerStateTransition(TransitionTimestamp = 1700000300L)
            )
        )

    Assert.Equal(Some(at 1700000000L), row.CreatedAt)
    Assert.Equal(Some(at 1700000600L), row.ExitedAt)
    Assert.Equal(Some(at 1700000300L), row.TransitionAt)

[<Fact>]
let ``each kind of event fills only its own identity column`` () =
    let payload =
        lifecycle
            [ Event(
                  EventType = Event.Types.EventType.Delete,
                  Pod = PodEvent(PodUID = "pod-1", Source = "kubelet", ExitTimestamp = 1700000600L)
              )
              Event(
                  EventType = Event.Types.EventType.Delete,
                  Task = TaskEvent(TaskARN = "arn:aws:ecs:task/1", Source = "ecs", ExitTimestamp = 1700000600L)
              ) ]

    payload.ClusterId <- "cl-1"
    payload.ObjectKind <- ObjectKind.Pod
    let rows = Containers.eventRows payload "t1" now

    Assert.Equal(2, rows.Length)
    let pod = rows[0]
    Assert.Equal("pod", pod.EventVariant)
    Assert.Equal("pod-1", pod.PodUID)
    Assert.Equal("kubelet", pod.Source)
    Assert.Equal("", pod.ContainerID)
    Assert.Equal("", pod.ContainerName)
    Assert.Equal("", pod.TaskARN)
    Assert.Equal("node1", pod.Host)
    Assert.Equal("cl-1", pod.ClusterID)
    Assert.Equal("Pod", pod.ObjectKind)
    Assert.Equal("Delete", pod.EventType)
    Assert.Equal(Some(at 1700000600L), pod.ExitedAt)

    let task = rows[1]
    Assert.Equal("task", task.EventVariant)
    Assert.Equal("arn:aws:ecs:task/1", task.TaskARN)
    Assert.Equal("ecs", task.Source)
    Assert.Equal("", task.ContainerID)
    Assert.Equal("", task.PodUID)

[<Fact>]
let ``an event with no typed detail still yields a row, marked unknown`` () =
    let row = Containers.eventRows (lifecycle [ Event(EventType = Event.Types.EventType.Create) ]) "t1" now |> Assert.Single

    Assert.Equal("Create", row.EventType)
    Assert.Equal("unknown", row.EventVariant)
    Assert.Equal("node1", row.Host)
    Assert.Equal("t1", row.TenantID)
    Assert.Equal("", row.ContainerID)
    Assert.Equal("", row.PodUID)
    Assert.Equal("", row.TaskARN)
    Assert.Equal(None, row.ExitCode)

[<Fact>]
let ``empty sub-messages convert without failing`` () =
    let payload =
        lifecycle
            [ Event(Container = ContainerEvent(Transition = ContainerStateTransition()))
              Event(Pod = PodEvent(Transition = PodStateTransition()))
              Event(Task = TaskEvent())
              Event() ]

    let rows = Containers.eventRows payload "t1" now

    Assert.Equal<string list>([ "container"; "pod"; "task"; "unknown" ], rows |> Array.map _.EventVariant |> List.ofArray)
    // A transition with no states: the label says so, the columns stay empty.
    Assert.Equal("-", rows[0].OldState)
    Assert.Equal("", rows[0].OldStateKind)
    Assert.Equal("-", rows[1].NewState)
    Assert.Equal("", rows[1].NewStateVariant)

[<Fact>]
let ``a container event carries its owner and its transition`` () =
    let row =
        containerRow (
            ContainerEvent(
                ContainerID = "a",
                Owner = ContainerEvent.Types.Owner(OwnerType = ObjectKind.Pod, OwnerUID = "pod-9"),
                Transition =
                    ContainerStateTransition(
                        LastObservedState = ContainerStateValue(Kind = ContainerStateKind.Running),
                        NewState = ContainerStateValue(Kind = ContainerStateKind.Terminated, Reason = "OOMKilled"),
                        TransitionTimestamp = 1700000300L
                    )
            )
        )

    Assert.Equal("Pod", row.OwnerType)
    Assert.Equal("pod-9", row.OwnerUID)
    Assert.Equal("CONTAINER_STATE_KIND_RUNNING", row.OldState)
    Assert.Equal("CONTAINER_STATE_KIND_TERMINATED/OOMKilled", row.NewState)
    Assert.Equal(Some(at 1700000300L), row.TransitionAt)

[<Fact>]
let ``an OOM kill is stored as columns beside the label`` () =
    let payload =
        lifecycle
            [ containerEvent
                  Event.Types.EventType.Transition
                  (ContainerEvent(
                      ContainerID = "containerd://abc",
                      Transition =
                          ContainerStateTransition(
                              ContainerKind = ContainerKind.Regular,
                              Precision = Precision.Exact,
                              MissedIntermediate = MissedIntermediate.Proven,
                              TransitionTimestamp = 1700000000L,
                              LastObservedState = ContainerStateValue(Kind = ContainerStateKind.Running),
                              NewState =
                                  ContainerStateValue(
                                      Kind = ContainerStateKind.Terminated,
                                      Reason = "OOMKilled",
                                      ExitCode = 137,
                                      Signal = 9
                                  )
                          )
                  )) ]

    payload.Version <- "v1"
    let row = Containers.eventRows payload "t" now |> Assert.Single

    Assert.Equal("v1", row.PayloadVersion)
    Assert.Equal("container", row.EventVariant)
    Assert.Equal("CONTAINER_KIND_REGULAR", row.ContainerKind)
    Assert.Equal("PRECISION_EXACT", row.Precision)
    Assert.Equal("MISSED_INTERMEDIATE_PROVEN", row.MissedIntermediate)
    Assert.Equal("CONTAINER_STATE_KIND_RUNNING", row.OldStateKind)
    Assert.Equal("CONTAINER_STATE_KIND_TERMINATED", row.NewStateKind)
    Assert.Equal(Some "OOMKilled", row.NewReason)
    Assert.Equal(Some 137, row.NewExitCode)
    Assert.Equal(Some 9, row.NewSignal)
    // RUNNING carried no reason, code or signal, and absent must stay absent
    // rather than read as "terminated cleanly with no reason".
    Assert.Equal(None, row.OldReason)
    Assert.Equal(None, row.OldExitCode)
    Assert.Equal(None, row.OldSignal)
    Assert.Equal("CONTAINER_STATE_KIND_TERMINATED/OOMKilled exit=137 signal=9", row.NewState)

[<Fact>]
let ``a pod condition keeps its message, and the row says which kind of status it holds`` () =
    let message = "0/3 nodes are available: insufficient memory"

    let payload =
        lifecycle
            [ Event(
                  EventType = Event.Types.EventType.Transition,
                  Pod =
                      PodEvent(
                          PodUID = "pod-uid-1",
                          Transition =
                              PodStateTransition(
                                  Field = PodStatusField.Condition,
                                  LastObservedState = PodStatusValue(Phase = "Pending"),
                                  NewState =
                                      PodStatusValue(
                                          Condition =
                                              ConditionValue(
                                                  Type = "PodScheduled",
                                                  Status = "False",
                                                  Reason = "Unschedulable",
                                                  Message = message
                                              )
                                      )
                              )
                      )
              ) ]

    let row = Containers.eventRows payload "t" now |> Assert.Single

    Assert.Equal("pod", row.EventVariant)
    Assert.Equal("pod-uid-1", row.PodUID)
    Assert.Equal("POD_STATUS_FIELD_CONDITION", row.PodStatusField)
    Assert.Equal("phase", row.OldStateVariant)
    Assert.Equal("condition", row.NewStateVariant)
    Assert.Equal(Some "Pending", row.OldPhase)
    Assert.Equal(None, row.OldConditionType)
    Assert.Equal(None, row.NewPhase)
    Assert.Equal(Some "PodScheduled", row.NewConditionType)
    Assert.Equal(Some "False", row.NewConditionStatus)
    Assert.Equal(Some "Unschedulable", row.NewConditionReason)
    Assert.Equal(Some message, row.NewConditionMessage)
    Assert.Equal("Pending", row.OldState)
    Assert.Equal("PodScheduled=False/Unschedulable", row.NewState)

[<Fact>]
let ``a container name set to empty is not the same as one never set`` () =
    Assert.Equal(0uy, (containerRow (ContainerEvent(ContainerID = "a"))).ContainerNamePresent)
    Assert.Equal(1uy, (containerRow (ContainerEvent(ContainerID = "b", ContainerName = ""))).ContainerNamePresent)
    Assert.Equal(1uy, (containerRow (ContainerEvent(ContainerID = "c", ContainerName = "app"))).ContainerNamePresent)

[<Fact>]
let ``an enum value this schema has no name for is stored as its number`` () =
    let payload = lifecycle [ Event(EventType = enum<Event.Types.EventType> 42) ]
    payload.ObjectKind <- enum<ObjectKind> 7
    let row = Containers.eventRows payload "t" now |> Assert.Single

    Assert.Equal("42", row.EventType)
    Assert.Equal("7", row.ObjectKind)

// ---- image inventory ----

[<Fact>]
let ``an image row is keyed on its digest, and an image with no identity is skipped`` () =
    let image =
        ContainerImage(
            Id = "img-1",
            Name = "registry.example.com/app",
            ShortName = "app",
            Registry = "registry.example.com",
            Digest = "sha256:abc",
            Size = 1234L,
            Os = ContainerImage.Types.OperatingSystem(Name = "linux", Architecture = "amd64"),
            BuiltAt = Timestamp(Seconds = 1700000000L)
        )

    image.RepoTags.Add "app:1.0"
    image.Layers.AddRange [ layer 100L; layer 250L ]
    let rows, skipped = Containers.imageRows (images [ ContainerImage(Name = "no-digest"); image ]) "t1" now

    Assert.Equal(1, skipped)
    let row = Assert.Single rows
    Assert.Equal("sha256:abc", row.Digest)
    Assert.Equal("node1", row.Host)
    Assert.Equal("t1", row.TenantID)
    Assert.Equal(now, row.CollectedAt)
    Assert.Equal(2u, row.LayerCount)
    Assert.Equal(350UL, row.LayerBytes)
    Assert.Equal(1234UL, row.SizeBytes)
    Assert.Equal("linux", row.OSName)
    Assert.Equal("amd64", row.Architecture)
    Assert.Equal<string[]>([| "app:1.0" |], row.RepoTags)
    Assert.Equal(Some(at 1700000000L), row.BuiltAt)
    Assert.Equal(None, row.PublishedAt)
    Assert.Equal(None, row.Source)

[<Fact>]
let ``empty payloads convert to no rows`` () =
    Assert.Empty(Containers.eventRows (EventsPayload()) "t1" now)
    let rows, skipped = Containers.imageRows (ContainerImagePayload()) "t1" now
    Assert.Empty rows
    Assert.Equal(0, skipped)

[<Fact>]
let ``an image without a digest is keyed on its id, and the row says so`` () =
    let payload =
        images
            [ ContainerImage(Id = "img-a", Name = "pushed", Digest = "sha256:abc")
              ContainerImage(Id = "img-b", Name = "built-locally")
              ContainerImage(Name = "nothing-at-all") ]

    let rows, skipped = Containers.imageRows payload "t1" now

    Assert.Equal(1, skipped)
    Assert.Equal(2, rows.Length)
    Assert.Equal(("sha256:abc", "digest", "sha256:abc", "img-a"), (rows[0].ImageKey, rows[0].IdentitySource, rows[0].Digest, rows[0].ImageID))
    // The fallback fills the key; it does not forge a digest.
    Assert.Equal(("img-b", "image_id", "", "img-b"), (rows[1].ImageKey, rows[1].IdentitySource, rows[1].Digest, rows[1].ImageID))

[<Fact>]
let ``layers and their build history are parallel arrays in wire order`` () =
    let created = Timestamp(Seconds = 1767323045L, Nanos = 500_000_000)

    let first =
        ContainerImage.Types.ContainerImageLayer(
            MediaType = "application/vnd.oci.image.layer.v1.tar+gzip",
            Digest = "sha256:layer1",
            Size = 100L,
            History =
                ContainerImage.Types.ContainerImageLayer.Types.History(
                    Created = created,
                    CreatedBy = "RUN apk add curl",
                    Author = "ci",
                    Comment = "base"
                )
        )

    first.Urls.Add "https://example.invalid/blob"

    let second =
        ContainerImage.Types.ContainerImageLayer(
            Digest = "sha256:layer2",
            Size = 200L,
            History = ContainerImage.Types.ContainerImageLayer.Types.History(CreatedBy = "COPY . /app", EmptyLayer = true)
        )

    let image = ContainerImage(Id = "sha256:img", Name = "shop/app", Digest = "sha256:dig", Size = 300L)
    image.Layers.AddRange [ first; second; layer 5L ]
    let payload = images [ image ]
    payload.Version <- "v1"
    payload.Source <- "agent"

    let rows, skipped = Containers.imageRows payload "t" now

    Assert.Equal(0, skipped)
    let row = Assert.Single rows
    Assert.Equal("v1", row.PayloadVersion)
    Assert.Equal(Some "agent", row.Source)
    Assert.Equal<string[]>([| "sha256:layer1"; "sha256:layer2"; "" |], row.LayerDigests)
    Assert.Equal<int64[]>([| 100L; 200L; 5L |], row.LayerSizes)
    Assert.Equal("application/vnd.oci.image.layer.v1.tar+gzip", row.LayerMediaTypes[0])
    Assert.Equal<string[][]>([| [| "https://example.invalid/blob" |]; [||]; [||] |], row.LayerURLs)
    // The third layer has no history at all: the arrays stay aligned.
    Assert.Equal<string[]>([| "RUN apk add curl"; "COPY . /app"; "" |], row.LayerHistoryCreatedBy)
    Assert.Equal<string[]>([| "ci"; ""; "" |], row.LayerHistoryAuthor)
    Assert.Equal<string[]>([| "base"; ""; "" |], row.LayerHistoryComment)
    Assert.Equal<uint8[]>([| 0uy; 1uy; 0uy |], row.LayerHistoryEmptyLayer)
    // A missing date is None in its slot, not 1970.
    Assert.Equal<DateTime option[]>([| Some((at 1767323045L).AddMilliseconds 500.0); None; None |], row.LayerHistoryCreated)
    Assert.Equal(3u, row.LayerCount)
    Assert.Equal(305UL, row.LayerBytes)

[<Fact>]
let ``a timestamp that is not after 1970, or not valid, is None`` () =
    let builtAt (value: Timestamp) : DateTime option =
        let rows, _ = Containers.imageRows (images [ ContainerImage(Id = "a", BuiltAt = value) ]) "t" now
        rows[0].BuiltAt

    Assert.Equal(None, builtAt (Timestamp(Seconds = 0L, Nanos = 5)))
    Assert.Equal(None, builtAt (Timestamp(Seconds = -1L)))
    Assert.Equal(None, builtAt (Timestamp(Seconds = 1600000000L, Nanos = -1)))
    Assert.Equal(None, builtAt (Timestamp(Seconds = 253402300800L)))
    Assert.Equal(Some(at 1600000000L), builtAt (Timestamp(Seconds = 1600000000L)))

[<Fact>]
let ``a negative image size is clamped to zero and flagged`` () =
    let rows, _ = Containers.imageRows (images [ ContainerImage(Id = "a", Size = -1L); ContainerImage(Id = "b", Size = 42L) ]) "t" now

    Assert.Equal((0UL, 1uy), (rows[0].SizeBytes, rows[0].SizeNegative))
    Assert.Equal((42UL, 0uy), (rows[1].SizeBytes, rows[1].SizeNegative))

[<Fact>]
let ``one negative layer size flags the total, and a negative total is clamped`` () =
    let payload =
        images
            [ // The sum stays positive, but one layer lied: only the flag says so.
              imageWithLayers "a" [ 500L; -1L ]
              // The sum goes negative: cast as is it would be ~18 exabytes.
              imageWithLayers "b" [ -700L; 100L ]
              imageWithLayers "c" [ 10L; 20L ] ]

    let rows, _ = Containers.imageRows payload "t" now

    Assert.Equal((499UL, 1uy), (rows[0].LayerBytes, rows[0].LayerSizeNegative))
    Assert.Equal((0UL, 1uy), (rows[1].LayerBytes, rows[1].LayerSizeNegative))
    Assert.Equal((30UL, 0uy), (rows[2].LayerBytes, rows[2].LayerSizeNegative))
    // The clamp hides nothing: the signed values arrive as they were sent.
    Assert.Equal(-700L, rows[1].LayerSizes[0])

// ---- through the intake ----

[<Fact>]
let ``skipped images are counted as a metric of the intake itself, then the rest are stored`` () =
    let payload = images [ ContainerImage(Id = "img-a"); ContainerImage(Name = "x"); ContainerImage(Name = "y") ]
    payload.Host <- "node-9"

    let response, sink = post "/api/v2/contimage" (payload.ToByteArray())

    Assert.Equal(202, response.Status)
    Assert.Equal<string list>([ "storage_metrics"; "storage_container_images" ], sink.Writes |> List.map _.Writer)

    // An event-driven COUNT: no sampling interval to divide by later.
    let point = sink.Rows<MetricPoint>() |> Assert.Single
    Assert.Equal("ninjacat.intake.images.skipped", point.Metric)
    Assert.Equal("COUNT", point.MetricType)
    Assert.Equal(0u, point.Interval)
    Assert.Equal(Replay.testTenant, point.TenantID)
    Assert.Equal("node-9", point.Host)
    Assert.Equal(2.0, point.Value)
    Assert.Equal("ninjacat", point.SourceType)
    Assert.Equal<Map<string, string[]>>(Map [ "reason", [| "no_identity" |] ], point.Tags)

    let row = sink.Rows<ContainerImageRow>() |> Assert.Single
    Assert.Equal("img-a", row.ImageKey)
    Assert.Equal(Replay.testTenant, row.TenantID)

[<Fact>]
let ``an inventory of only unidentifiable images stores the metric and no rows`` () =
    let _, sink = post "/api/v2/contimage" ((images [ ContainerImage(Name = "x") ]).ToByteArray())

    Assert.Equal<string list>([ "storage_metrics" ], sink.Writes |> List.map _.Writer)
