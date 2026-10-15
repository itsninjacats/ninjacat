/// `agent flare` uploads: POST /support/flare, with or without a case id in
/// the path. The HEAD that precedes one is answered in Routes.fs.
///
/// Registered on every intake host, because the agent sends a flare to
/// whichever host its dd_url resolves to, not to a dedicated one.
module NinjaCat.Api.Intake.Routers.Flare

open System
open System.Buffers.Binary
open System.Threading.Tasks
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The label of flare uploads in raw_payloads.
let private intake = "flare"

/// Form fields with a column of their own; the rest go to `fields`.
let private knownFields = set [ "case_id"; "email"; "source"; "agent_version"; "hostname" ]

/// What the agent's uploader parses the answer into. It needs all three keys
/// and a 200, or it reports "could not deserialize response body" and retries.
let private answer (caseId: int64) (error: string) : EndpointHandler =
    json
        {| case_id = caseId
           error = error
           request_uuid = Guid.NewGuid().ToString() |}

/// The number shown to the operator: the case id the agent sent, or for a new
/// flare one minted from a UUID (63 bits, so it fits an int64).
let private caseNumber (caseId: string) : int64 =
    match Int64.TryParse caseId with
    | true, n -> n
    | false, _ -> int64 (BinaryPrimitives.ReadUInt64BigEndian(ReadOnlySpan(Guid.NewGuid().ToByteArray(true))) >>> 1)

/// `pathCaseId` is the case id from the path, "" for a new flare.
let handle (pathCaseId: string) (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match Multipart.boundary (Ctx.header ctx "Content-Type") with
    | None ->
        // Refusing would only make the agent retry the same upload, so
        // the answer keeps its shape and carries the error.
        Raw.store ctx intake "decode_error" "content-type is not multipart/form-data" body
        answer 0L "content-type is not multipart/form-data" ctx
    | Some boundary ->
        let parts = Multipart.parts boundary body

        // The agent writes agent_version and hostname AFTER the archive,
        // so nothing here may depend on the order of parts.
        match parts |> List.tryFindBack (fun p -> p.Name = "flare_file") with
        | None ->
            Raw.store ctx intake "decode_error" "no flare_file part in the multipart body" body
            answer 0L "no flare archive in the request" ctx
        | Some archive ->
            let field (name: string) =
                parts
                |> List.tryFindBack (fun p -> p.Name = name)
                |> Option.map (fun p -> Text.utf8 p.Data)
                |> Option.defaultValue ""

            let otherFields =
                parts
                |> List.filter (fun p -> p.Name <> "" && p.Name <> "flare_file" && not (knownFields.Contains p.Name))
                |> List.map (fun p -> p.Name, Text.utf8 p.Data)
                |> Map.ofList

            // A flare for an existing case carries its id in the path and
            // in the form; the form wins if they ever disagree.
            let caseId = Text.firstNonEmpty [ field "case_id"; pathCaseId ]

            Ctx.write
                ctx
                AgentFlares.table
                [| { TenantID = tenant
                     ReceivedAt = DateTime.UtcNow
                     Hostname = field "hostname"
                     CaseID = caseId
                     Email = field "email"
                     Source = field "source"
                     AgentVersion = field "agent_version"
                     Filename = archive.FileName
                     SizeBytes = uint64 archive.Data.Length
                     Fields = otherFields
                     Archive = archive.Data } |]

            answer (caseNumber caseId) "" ctx
