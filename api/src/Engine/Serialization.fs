/// The one set of JSON options, for requests read and responses written.
module NinjaCat.Api.Engine.Serialization

open System.Text.Json
open System.Text.Json.Serialization

/// Datadog's wire format is snake_case (`data_source`, `query_index`), so F#
/// field names map across without an attribute on every one. The F# converter
/// adds what System.Text.Json lacks: options (absent or null ↔ None), lists
/// and unions.
let options =
    let o = JsonSerializerOptions(JsonSerializerDefaults.Web)
    o.PropertyNamingPolicy <- JsonNamingPolicy.SnakeCaseLower
    // Option fields: absent or null reads as None, and None is left out when
    // writing. This is the only combination that gets every option read
    // right. What it gives up — tested, FSharp.SystemTextJson 1.4.36:
    //
    //   - a missing required field is not reliably an error: a missing string
    //     reads as null, a missing int64 throws NullReferenceException (only a
    //     missing list reports "Missing field"). So required scalars are
    //     declared as options and checked by the reader of each type, and
    //     required strings are null-checked there.
    //   - `[<JsonRequired>]` is not honoured, and a per-field
    //     `[<JsonIgnore(Condition = ...)>]` makes the field always read None.
    //   - Where Datadog writes an explicit null in a response (a series'
    //     `"unit": null`), None would be left out; that needs its own answer
    //     when responses are written.
    o.DefaultIgnoreCondition <- JsonIgnoreCondition.WhenWritingNull

    o.Converters.Add(
        JsonFSharpConverter(
            JsonFSharpOptions.Default().WithSkippableOptionFields(SkippableOptionFields.FromJsonSerializerOptions)
        )
    )

    o
