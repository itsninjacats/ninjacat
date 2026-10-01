/// FNV-1a, 64 bit: the hash behind the migration checksums and the ids
/// derived from a tenant's name. Neither can change: the checksums are in
/// the ledger and the ids are in agents' hands.
module NinjaCat.Api.Storage.Fnv

let hash64 (data: byte[]) : uint64 =
    let mutable hash = 14695981039346656037UL

    for b in data do
        hash <- (hash ^^^ uint64 b) * 1099511628211UL

    hash
