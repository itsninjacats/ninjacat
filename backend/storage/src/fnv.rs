//! FNV-1a, 64 bit: the hash behind the migration checksums and the ids derived from a tenant's
//! name. Neither can change: the checksums are in the ledger and the ids are in agents' hands.

pub fn hash64(data: &[u8]) -> u64 {
    data.iter().fold(14695981039346656037u64, |hash, b| (hash ^ u64::from(*b)).wrapping_mul(1099511628211))
}
