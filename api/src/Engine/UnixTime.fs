/// Unix times and spans a client sends, as dates and steps.
module NinjaCat.Api.Engine.UnixTime

open System

// Every stored time falls between the epoch and this (ClickHouse's DateTime),
// so a range outside them finds nothing, which is Datadog's answer too, not
// an error. Clamped this far inside what a date holds, a range can be shifted
// by a step or a season without overflowing.
let private latestMs = DateTimeOffset(2105, 12, 31, 23, 59, 59, TimeSpan.Zero).ToUnixTimeMilliseconds()

let ofSeconds (seconds: int64) =
    DateTimeOffset.FromUnixTimeSeconds(Math.Clamp(seconds, 0L, latestMs / 1000L))

let ofMillis (ms: int64) =
    DateTimeOffset.FromUnixTimeMilliseconds(Math.Clamp(ms, 0L, latestMs))

/// A step or a shift a client asks for, kept to the span of stored times:
/// a longer one finds what that one does, or nothing.
let spanOfMillis (ms: float) =
    TimeSpan.FromMilliseconds(Math.Clamp(ms, float -latestMs, float latestMs))
