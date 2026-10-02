/// contlcycle-intake.<site>, contimage-intake.<site> — containers.
///
///   agent config: none. Event platform pipelines have no dd_url, only
///   container_lifecycle.enabled / container_image.enabled, and their
///   additional_endpoints ADD a recipient rather than replace one. DD_SITE
///   is the only way to point them here.
///
/// Both carry protobuf. Lifecycle events land in container_events, the image
/// inventory in container_images. A body that does not decode, or decodes to
/// nothing, goes to raw_payloads.
module NinjaCat.Api.Intake.Routers.Containers

open System
open System.Threading.Tasks
open Google.Protobuf
open Google.Protobuf.WellKnownTypes
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open Datadog.Contimage
open Datadog.Contlcycle
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

let private tryParse (parser: MessageParser<'message>) (body: byte[]) : Result<'message, string> =
    try
        Ok(parser.ParseFrom body)
    with :? InvalidProtocolBufferException as e ->
        Error e.Message

// ---- lifecycle events ----

/// A container state as one label: the kind, then reason, exit code and
/// signal when the value carries them.
let private containerStateLabel (state: ContainerStateValue) : string =
    if isNull state then
        "-"
    else
        let reason = if state.HasReason then "/" + state.Reason else ""
        let exitCode = if state.HasExitCode then $" exit={state.ExitCode}" else ""
        let signal = if state.HasSignal then $" signal={state.Signal}" else ""
        ProtoEnum.name state.Kind + reason + exitCode + signal

/// A pod status as one label: a phase, or a condition as type=status/reason.
let private podStatusLabel (status: PodStatusValue) : string =
    if isNull status then
        "-"
    elif status.ValueCase = PodStatusValue.ValueOneofCase.Condition then
        let condition = status.Condition
        let reason = if condition.HasReason then "/" + condition.Reason else ""
        condition.Type + "=" + condition.Status + reason
    elif status.Phase = "" then
        "-"
    else
        status.Phase

/// A pod status spread over its columns. A status is a phase OR a condition;
/// Variant says which, so a reader knows which columns mean something.
type private PodStatusColumns =
    { Variant: string
      Phase: string option
      ConditionType: string option
      ConditionStatus: string option
      ConditionReason: string option
      ConditionMessage: string option }

let private noPodStatus: PodStatusColumns =
    { Variant = ""
      Phase = None
      ConditionType = None
      ConditionStatus = None
      ConditionReason = None
      ConditionMessage = None }

let private podStatusColumns (status: PodStatusValue) : PodStatusColumns =
    if isNull status then
        noPodStatus
    elif status.ValueCase = PodStatusValue.ValueOneofCase.Condition then
        let condition = status.Condition

        { noPodStatus with
            Variant = "condition"
            ConditionType = Some condition.Type
            ConditionStatus = Some condition.Status
            ConditionReason = (if condition.HasReason then Some condition.Reason else None)
            ConditionMessage = (if condition.HasMessage then Some condition.Message else None) }
    elif status.ValueCase = PodStatusValue.ValueOneofCase.Phase then
        { noPodStatus with Variant = "phase"; Phase = Some status.Phase }
    else
        noPodStatus

let private withContainerTransition (transition: ContainerStateTransition) (row: ContainerEventRow) : ContainerEventRow =
    if isNull transition then
        row
    else
        let was = transition.LastObservedState
        let became = transition.NewState

        { row with
            OldState = containerStateLabel was
            NewState = containerStateLabel became
            TransitionAt = Time.optionalSeconds true transition.TransitionTimestamp
            ContainerKind = ProtoEnum.name transition.ContainerKind
            Precision = ProtoEnum.name transition.Precision
            MissedIntermediate = ProtoEnum.name transition.MissedIntermediate
            OldStateKind = (if isNull was then "" else ProtoEnum.name was.Kind)
            OldReason = (if not (isNull was) && was.HasReason then Some was.Reason else None)
            OldExitCode = (if not (isNull was) && was.HasExitCode then Some was.ExitCode else None)
            OldSignal = (if not (isNull was) && was.HasSignal then Some was.Signal else None)
            NewStateKind = (if isNull became then "" else ProtoEnum.name became.Kind)
            NewReason = (if not (isNull became) && became.HasReason then Some became.Reason else None)
            NewExitCode = (if not (isNull became) && became.HasExitCode then Some became.ExitCode else None)
            NewSignal = (if not (isNull became) && became.HasSignal then Some became.Signal else None) }

let private withPodTransition (transition: PodStateTransition) (row: ContainerEventRow) : ContainerEventRow =
    if isNull transition then
        row
    else
        let was = podStatusColumns transition.LastObservedState
        let became = podStatusColumns transition.NewState

        { row with
            OldState = podStatusLabel transition.LastObservedState
            NewState = podStatusLabel transition.NewState
            TransitionAt = Time.optionalSeconds true transition.TransitionTimestamp
            PodStatusField = ProtoEnum.name transition.Field
            Precision = ProtoEnum.name transition.Precision
            MissedIntermediate = ProtoEnum.name transition.MissedIntermediate
            OldStateVariant = was.Variant
            OldPhase = was.Phase
            OldConditionType = was.ConditionType
            OldConditionStatus = was.ConditionStatus
            OldConditionReason = was.ConditionReason
            OldConditionMessage = was.ConditionMessage
            NewStateVariant = became.Variant
            NewPhase = became.Phase
            NewConditionType = became.ConditionType
            NewConditionStatus = became.ConditionStatus
            NewConditionReason = became.ConditionReason
            NewConditionMessage = became.ConditionMessage }

/// One row per event. The envelope (host, cluster, object kind) repeats on
/// every row; each kind of event fills only its own identity column.
///
/// `now` is the row's timestamp: the payload does not say when it was sent,
/// only when things were created, exited or changed, and those have columns
/// of their own.
let eventRows (payload: EventsPayload) (tenant: string) (now: DateTime) : ContainerEventRow[] =
    let envelope: ContainerEventRow =
        { TenantID = tenant
          Timestamp = now
          Host = payload.Host
          ClusterID = payload.ClusterId
          ObjectKind = ProtoEnum.name payload.ObjectKind
          EventType = ""
          ContainerID = ""
          ContainerName = ""
          PodUID = ""
          TaskARN = ""
          Source = ""
          ExitCode = None
          CreatedAt = None
          ExitedAt = None
          OwnerType = ""
          OwnerUID = ""
          OldState = ""
          NewState = ""
          TransitionAt = None
          PayloadVersion = payload.Version
          EventVariant = "unknown"
          ContainerKind = ""
          Precision = ""
          MissedIntermediate = ""
          OldStateKind = ""
          OldReason = None
          OldExitCode = None
          OldSignal = None
          NewStateKind = ""
          NewReason = None
          NewExitCode = None
          NewSignal = None
          PodStatusField = ""
          OldStateVariant = ""
          OldPhase = None
          OldConditionType = None
          OldConditionStatus = None
          OldConditionReason = None
          OldConditionMessage = None
          NewStateVariant = ""
          NewPhase = None
          NewConditionType = None
          NewConditionStatus = None
          NewConditionReason = None
          NewConditionMessage = None
          ContainerNamePresent = 0uy }

    let toRow (event: Event) : ContainerEventRow =
        let row = { envelope with EventType = ProtoEnum.name event.EventType }

        match event.TypedEventCase with
        | Event.TypedEventOneofCase.Container ->
            let container = event.Container
            let owner = container.Owner

            { row with
                EventVariant = "container"
                ContainerID = container.ContainerID
                ContainerName = container.ContainerName
                ContainerNamePresent = Text.flag container.HasContainerName
                Source = container.Source
                // Presence, not value: no exit code is NULL, distinct from an
                // honest exit 0. OOM kill is 137.
                ExitCode = (if container.HasExitCode then Some container.ExitCode else None)
                CreatedAt = Time.optionalSeconds container.HasCreationTimestamp container.CreationTimestamp
                ExitedAt = Time.optionalSeconds container.HasExitTimestamp container.ExitTimestamp
                OwnerType = (if isNull owner then "" else ProtoEnum.name owner.OwnerType)
                OwnerUID = (if isNull owner then "" else owner.OwnerUID) }
            |> withContainerTransition container.Transition
        | Event.TypedEventOneofCase.Pod ->
            let pod = event.Pod

            { row with
                EventVariant = "pod"
                PodUID = pod.PodUID
                Source = pod.Source
                CreatedAt = Time.optionalSeconds pod.HasCreationTimestamp pod.CreationTimestamp
                ExitedAt = Time.optionalSeconds pod.HasExitTimestamp pod.ExitTimestamp }
            |> withPodTransition pod.Transition
        | Event.TypedEventOneofCase.Task ->
            let task = event.Task

            { row with
                EventVariant = "task"
                TaskARN = task.TaskARN
                Source = task.Source
                ExitedAt = Time.optionalSeconds task.HasExitTimestamp task.ExitTimestamp }
        // No typed detail, or a kind newer than this schema. A Delete with no
        // detail is still a Delete, so the row stands on the envelope alone.
        | _ -> row

    payload.Events |> Seq.map toRow |> Array.ofSeq

// ---- image inventory ----

/// A protobuf Timestamp for a nullable column. Not after 1970 is None, like
/// an absent one: a missing build date must not become a real one.
let private timestamp (value: Timestamp) : DateTime option =
    if isNull value || value.Seconds <= 0L then None else Time.ofTimestamp value

/// The rows of one inventory, and how many images were skipped for having
/// neither a digest nor an id.
///
/// The table replaces on the image key. A missing digest is normal (built
/// locally, loaded from a tarball, pulled moments ago), so the image id
/// stands in. An image with neither is skipped: keyed on "" they would all
/// collapse into one row, which loses more.
let imageRows (payload: ContainerImagePayload) (tenant: string) (now: DateTime) : ContainerImageRow[] * int =
    let toRow (image: ContainerImage) : ContainerImageRow option =
        let key, source =
            if image.Digest <> "" then image.Digest, "digest" else image.Id, "image_id"

        if key = "" then
            None
        else
            let layers = List.ofSeq image.Layers
            let histories = layers |> List.map (fun layer -> Option.ofObj layer.History)

            let historyText (read: ContainerImage.Types.ContainerImageLayer.Types.History -> string) =
                histories |> List.map (fun h -> h |> Option.map read |> Option.defaultValue "") |> Array.ofList

            // Both size columns are unsigned and the wire is signed. A
            // negative size cast blindly becomes ~18 exabytes and poisons
            // every SUM over the table, so it is clamped and the clamp is
            // recorded. For layers the flag rises on ANY negative one: inside
            // a still-positive sum it is just as wrong and far quieter.
            let layerBytes = layers |> List.sumBy _.Size
            let os = image.Os

            Some
                { TenantID = tenant
                  CollectedAt = now
                  Host = payload.Host
                  ImageKey = key
                  IdentitySource = source
                  ImageID = image.Id
                  Digest = image.Digest
                  Name = image.Name
                  ShortName = image.ShortName
                  Registry = image.Registry
                  RepoTags = Array.ofSeq image.RepoTags
                  RepoDigests = Array.ofSeq image.RepoDigests
                  SizeBytes = uint64 (max image.Size 0L)
                  OSName = (if isNull os then "" else os.Name)
                  OSVersion = (if isNull os then "" else os.Version)
                  Architecture = (if isNull os then "" else os.Architecture)
                  LayerCount = uint32 layers.Length
                  LayerBytes = uint64 (max layerBytes 0L)
                  BuiltAt = timestamp image.BuiltAt
                  PublishedAt = timestamp image.PublishedAt
                  DDTags = Tags.toMultiMap image.DdTags
                  PayloadVersion = payload.Version
                  Source = (if payload.HasSource then Some payload.Source else None)
                  LayerMediaTypes = layers |> List.map _.MediaType |> Array.ofList
                  LayerDigests = layers |> List.map _.Digest |> Array.ofList
                  LayerSizes = layers |> List.map _.Size |> Array.ofList
                  LayerURLs = layers |> List.map (fun layer -> Array.ofSeq layer.Urls) |> Array.ofList
                  LayerHistoryCreated =
                    histories |> List.map (fun h -> h |> Option.bind (fun h -> timestamp h.Created)) |> Array.ofList
                  LayerHistoryCreatedBy = historyText _.CreatedBy
                  LayerHistoryAuthor = historyText _.Author
                  LayerHistoryComment = historyText _.Comment
                  LayerHistoryEmptyLayer =
                    histories
                    |> List.map (fun h -> Text.flag (h |> Option.map _.EmptyLayer |> Option.defaultValue false))
                    |> Array.ofList
                  SizeNegative = Text.flag (image.Size < 0L)
                  LayerSizeNegative = Text.flag (layers |> List.exists (fun layer -> layer.Size < 0L)) }

    let rows = payload.Images |> Seq.choose toRow |> Array.ofSeq
    rows, payload.Images.Count - rows.Length

// ---- handlers ----

// Protobuf carries no type marker and skips fields it does not know, so a
// payload meant for another endpoint decodes "successfully" into nothing. An
// empty decode is the one hint that the wrong message arrived; both handlers
// keep such a body raw.

/// Container, pod and task start/stop/OOM events. One payload covers one
/// kind of object, named by ObjectKind.
let handleLifecycle (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match tryParse EventsPayload.Parser body with
    | Error e ->
        Raw.store ctx "contlcycle" "decode_error" e body
    | Ok payload when payload.Events.Count = 0 ->
        Raw.store ctx "contlcycle" "unexpected_shape" "EventsPayload decoded with no events" body
    | Ok payload ->
        Ctx.write ctx ContainerEvents.table (eventRows payload tenant DateTime.UtcNow)

    accepted ctx

/// The container image inventory.
let handleImages (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx

    match tryParse ContainerImagePayload.Parser body with
    | Error e ->
        Raw.store ctx "contimage" "decode_error" e body
    | Ok payload when payload.Images.Count = 0 ->
        Raw.store ctx "contimage" "unexpected_shape" "ContainerImagePayload decoded with no images" body
    | Ok payload ->
        let rows, skipped = imageRows payload tenant DateTime.UtcNow

        if skipped > 0 then
            log.LogWarning(
                "[contimage] {Skipped} of {Total} images skipped: neither digest nor image id, so nothing to key on",
                skipped,
                payload.Images.Count
            )

            // The metric is what makes an incomplete inventory noticeable
            // without going looking for a log line.
            SelfMetrics.count ctx payload.Host SelfMetrics.imagesSkipped (float skipped) (Map [ "reason", [| "no_identity" |] ])

        Ctx.write ctx ContainerImages.table rows

    accepted ctx
