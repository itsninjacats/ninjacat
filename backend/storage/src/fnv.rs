//! FNV-1a, 64 bit: the hash behind the migration checksums, the ids derived from a tenant's name
//! and the monitors' shards. None can change: the checksums are in the ledger, the ids are in
//! agents' hands, and a monitor's shard moves only with every replica stopped.

pub fn hash64(data: &[u8]) -> u64 {
    data.iter().fold(14695981039346656037u64, |hash, b| (hash ^ u64::from(*b)).wrapping_mul(1099511628211))
}
