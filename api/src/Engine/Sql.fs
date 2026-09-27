namespace NinjaCat.Api.Engine

/// A tenant, as carried in the first ORDER BY column of every ClickHouse table.
///
/// A distinct type rather than a string so no query can be built without one:
/// forgetting the tenant filter is the one mistake multi-tenancy cannot survive.
[<Struct>]
type TenantId = TenantId of string

/// A value bound to a ClickHouse query parameter.
///
/// Closed on purpose. Every case maps to one ClickHouse type in the `{name:Type}`
/// placeholder, so the SQL text and the value can never disagree about it.
type SqlValue =
    | String of string
    | Int64 of int64
    | Float64 of float
    | StringArray of string list

    member v.ClickHouseType =
        match v with
        | String _ -> "String"
        | Int64 _ -> "Int64"
        | Float64 _ -> "Float64"
        | StringArray _ -> "Array(String)"

/// A compiled query: text with `{name:Type}` placeholders plus their values.
///
/// This is the whole boundary between the engine and the database. The engine
/// never concatenates a user value into `Text`; the server hands `Parameters`
/// to the driver, which ClickHouse binds on its side.
type Sql =
    { Text: string
      Parameters: (string * SqlValue) list }

module Sql =
    /// The placeholder for a parameter, typed from its value.
    let param (name: string) (value: SqlValue) = $"{{{name}:{value.ClickHouseType}}}"
