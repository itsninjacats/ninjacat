/// A tag filter as a SQL condition on a table with a `host` column and a
/// `tags` map of value lists: `metrics` and `logs` alike.
module NinjaCat.Api.Engine.MetricQuery.Filter

open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery.Ast

/// Datadog's `*` wildcard as a LIKE pattern. `_` and `%` are LIKE's own
/// wildcards and common in tag values (`kube_namespace`), so they are escaped.
let private likePattern (value: string) =
    value.Replace("\\", "\\\\").Replace("%", "\\%").Replace("_", "\\_").Replace("*", "%")

let private isWildcard (value: string) = value.Contains '*'

/// `host` lives in its own column, not in the tag map — the agent sends it
/// apart from the tags, and the table's ORDER BY uses it.
///
/// Every value of the filter is bound; the text holds only what this
/// function wrote.
let rec sql (p: SqlParams) (filter: TagFilter) : string =
    match filter with
    | All -> "1"
    | Tag("host", v) when isWildcard v -> $"""host LIKE {p.Add("p", String(likePattern v))}"""
    | Tag("host", v) -> $"""host = {p.Add("p", String v)}"""
    | Tag(k, v) when isWildcard v ->
        $"""arrayExists(x -> x LIKE {p.Add("p", String(likePattern v))}, tags[{p.Add("k", String k)}])"""
    | Tag(k, v) -> $"""has(tags[{p.Add("k", String k)}], {p.Add("p", String v)})"""
    // A key-less tag is stored as a key with an empty value (Tags.split in
    // the intake), so `{prod}` asks whether the key exists.
    | Bare v when isWildcard v -> $"""arrayExists(x -> x LIKE {p.Add("p", String(likePattern v))}, mapKeys(tags))"""
    | Bare v -> $"""mapContains(tags, {p.Add("k", String v)})"""
    // `IN` with a wildcard among its values (`name IN (web-*, db)`) is the OR
    // of its parts.
    //
    // WARNING(undocumented): wildcards inside IN. Datadog's tag rules forbid
    // `*` in a tag value, so a `*` in a filter can only ever mean one.
    | In(k, vs) when List.exists isWildcard vs ->
        let exact = vs |> List.filter (isWildcard >> not)
        let parts = [ for v in vs |> List.filter isWildcard -> Tag(k, v) ] @ (if exact.IsEmpty then [] else [ In(k, exact) ])
        sql p (Or parts)
    | In("host", vs) -> $"""has({p.Add("p", StringArray vs)}, host)"""
    | In(k, vs) -> $"""hasAny(tags[{p.Add("k", String k)}], {p.Add("p", StringArray vs)})"""
    | Not f -> $"NOT ({sql p f})"
    | And fs -> fs |> List.map (fun f -> $"({sql p f})") |> String.concat " AND "
    | Or fs -> fs |> List.map (fun f -> $"({sql p f})") |> String.concat " OR "
