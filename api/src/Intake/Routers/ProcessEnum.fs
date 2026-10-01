/// Enum values of the process-agent's schema, as text.
module NinjaCat.Api.Intake.Routers.ProcessEnum

open System
open System.Collections.Generic
open Google.Protobuf.Reflection
open Datadog.ProcessAgent

let private byType: Dictionary<Type, EnumDescriptor> =
    let found = Dictionary<Type, EnumDescriptor>()

    for file in [ AgentReflection.Descriptor; ConnectionsReflection.Descriptor ] do
        for enum in file.EnumTypes do
            found[enum.ClrType] <- enum

    found

/// An enum value under the name the .proto gives it, as Go's String() does.
/// Names, never numbers: a renumbering upstream must not rewrite stored
/// history. A value newer than these definitions has only its number.
let name (value: 'T when 'T :> Enum) : string =
    let number = Convert.ToInt32(box value)

    match byType[typeof<'T>].FindValueByNumber number with
    | null -> string number
    | known -> known.Name
