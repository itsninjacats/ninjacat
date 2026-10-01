/// API keys as the intake sees them: an in-memory snapshot, replaced whole.
///
/// The lookup is on the path of every agent request, so it takes no lock and
/// asks no database: one reference read and one dictionary lookup. Whoever
/// refreshes the keys builds a new snapshot and publishes it.
module NinjaCat.Api.Intake.ApiKeys

open System
open System.Collections.Generic
open System.Security.Cryptography
open System.Text
open System.Threading

type Key =
    { ID: string
      Name: string
      TenantID: string }

/// SHA-256 as lowercase hex: the only form of a key that is ever stored.
let hash (key: string) : string =
    Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes key))

type Snapshot =
    { ByHash: IReadOnlyDictionary<string, Key>
      UpdatedAt: DateTime }

type Store() =
    let mutable current =
        { ByHash = Dictionary<string, Key>()
          UpdatedAt = DateTime.MinValue }

    /// Replaces every key at once. `keys` pairs each key with its hash.
    member _.Publish(keys: (string * Key) seq) =
        let byHash = Dictionary<string, Key>()

        for keyHash, key in keys do
            byHash[keyHash] <- key

        Volatile.Write(&current, { ByHash = byHash; UpdatedAt = DateTime.UtcNow })

    member _.Current: Snapshot = Volatile.Read &current

    member _.Lookup(key: string) : Key option =
        if String.IsNullOrEmpty key then
            None
        else
            match (Volatile.Read &current).ByHash.TryGetValue(hash key) with
            | true, found -> Some found
            | false, _ -> None
