//! The packed buffers of a connections payload: tag sets and DNS names.
//!
//! Ported from github.com/DataDog/agent-payload v5.0.207, process/tags.go, tags_v2.go, dns_v1.go
//! and dns_v2.go: the readers only, and only what the handler uses. A buffer the Go reader would
//! run off the end of gives no tags, or an Err for DNS names, instead of failing the request.
//!
//! Copyright (c) 2017, Datadog, Inc. Licensed under the BSD 3-Clause License; see
//! THIRD_PARTY_NOTICES.md.

use crate::wire::{text, varint};

fn u16_at(buffer: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([buffer[at], buffer[at + 1]]))
}

fn u32_at(buffer: &[u8], at: usize) -> usize {
    u32::from_le_bytes(buffer[at..at + 4].try_into().unwrap()) as usize
}

/// `encodedTags` and `encodedConnectionsTags`: many tag sets in one buffer, each found by the
/// index a connection, host or resource carries. The first byte is the format's version.
pub mod tag_buffer {
    use super::*;

    /// Version 1: at the index, a count, then each tag behind its length. All numbers are 2
    /// bytes, little-endian.
    fn read_v1(buffer: &[u8], tag_index: usize) -> Vec<String> {
        if tag_index >= buffer.len() || buffer.len() - tag_index < 2 {
            return Vec::new();
        }
        let count = u16_at(buffer, tag_index);
        let mut tags = Vec::new();
        let mut at = tag_index + 2;
        for _ in 0..count {
            if buffer.len() - at >= 2 {
                let length = u16_at(buffer, at);
                at += 2;
                // A tag that runs past the end is dropped, and the reader goes on from where it
                // stands, as the Go one does.
                if at + length <= buffer.len() {
                    tags.push(text::utf8(&buffer[at..at + length]));
                    at += length;
                }
            }
        }
        tags
    }

    /// Versions 2 and 3: every distinct tag is stored once, behind a 2-byte length. Bytes 1-4 say
    /// where the footer starts; a tag set is a position in the footer: a 2-byte count, then the
    /// 4-byte position of each tag.
    fn read_v2(buffer: &[u8], tag_index: usize) -> Vec<String> {
        // The tag whose position is written at `at` in the footer.
        let tag_at = |at: usize| -> Option<String> {
            if buffer.len() < at + 4 {
                return None;
            }
            let position = u32_at(buffer, at);
            if position >= buffer.len() || buffer.len() - position < 2 {
                return None;
            }
            let length = u16_at(buffer, position);
            let first = position + 2;
            (first + length <= buffer.len()).then(|| text::utf8(&buffer[first..first + length]))
        };

        if buffer.len() < 5 {
            return Vec::new();
        }
        let start = u32_at(buffer, 1) + tag_index;
        if start >= buffer.len() || buffer.len() - start < 2 {
            return Vec::new();
        }
        let count = u16_at(buffer, start);
        let mut tags = Vec::new();
        let mut at = start + 2;
        // A tag that cannot be read ends the set: the Go reader never moves past it.
        while tags.len() < count {
            match tag_at(at) {
                Some(tag) => {
                    tags.push(tag);
                    at += 4;
                }
                None => break,
            }
        }
        tags
    }

    /// The tags of one set. Nothing for an empty buffer, a negative index (the encoders' "no
    /// tags") or a version this reader does not know.
    pub fn tags(buffer: &[u8], tag_index: i32) -> Vec<String> {
        let Ok(index) = usize::try_from(tag_index) else { return Vec::new() };
        match buffer.first() {
            // Byte 0 of a version 1 buffer is the version, never a tag set: an index of 0 there is
            // one that was not sent.
            Some(1) if index == 0 => Vec::new(),
            Some(1) => read_v1(buffer, index),
            Some(2 | 3) => read_v2(buffer, index),
            _ => Vec::new(),
        }
    }
}

/// The DNS names of a connections payload. Two encodings exist: version 1 keeps the names inside
/// `encodedDNS`, version 2 in a buffer of their own, `encodedDomainDatabase`.
pub mod dns_buffer {
    use super::*;

    /// `count` names from `at`, each behind a varint length, stopping at `stop`.
    fn read_names(buffer: &[u8], at: usize, stop: usize, count: u64) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        let mut position = at;
        while position < stop && (names.len() as u64) < count {
            let Some((length, read)) = varint::read(buffer, position) else {
                return Err(format!("unreadable name length at byte {position}"));
            };
            let first = position + read;
            if length > stop.saturating_sub(first) as u64 {
                return Err(format!("name at byte {position} runs past the end of the buffer"));
            }
            let end = first + length as usize;
            names.push(text::utf8(&buffer[first..end]));
            position = end;
        }
        Ok(names)
    }

    /// Version 1: a 3-byte preamble, then three varints, of which the second is the length of the
    /// name block. The names are the last thing in the buffer.
    fn names_v1(buffer: &[u8]) -> Result<Vec<String>, String> {
        if buffer.len() < 3 {
            return Err("encodedDNS is shorter than its preamble".into());
        }
        let name_block_length = varint::read(buffer, 3).and_then(|(_, read)| varint::read(buffer, 3 + read));
        match name_block_length {
            // A buffer that ends inside its header has no name block.
            None => Ok(Vec::new()),
            Some((length, _)) if length > buffer.len() as u64 => {
                Err("encodedDNS: the name block is longer than the buffer".into())
            }
            Some((length, _)) => read_names(buffer, buffer.len() - length as usize, buffer.len(), u64::MAX),
        }
    }

    /// Version 2: the number of names, a varint that is never used, then the names.
    fn names_v2(buffer: &[u8]) -> Result<Vec<String>, String> {
        let Some((count, read)) = varint::read(buffer, 0) else { return Ok(Vec::new()) };
        match varint::read(buffer, read) {
            None if read < buffer.len() => Err("encodedDomainDatabase: unreadable header".into()),
            None => Ok(Vec::new()),
            Some((_, read_for_middle)) => read_names(buffer, read + read_for_middle, buffer.len(), count),
        }
    }

    /// Every DNS name of the payload. `encodedDNS` wins when both are there; one in a version
    /// other than 1 gives no names, as does a payload with neither buffer. Err means a buffer that
    /// could not be read through.
    pub fn names(encoded_dns: &[u8], domain_database: &[u8]) -> Result<Vec<String>, String> {
        match encoded_dns.first() {
            Some(1) => names_v1(encoded_dns),
            Some(_) => Ok(Vec::new()),
            None if !domain_database.is_empty() => names_v2(domain_database),
            None => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use base64::Engine;

    use super::*;

    fn bytes(base64: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD.decode(base64).unwrap()
    }

    // Buffers made by the encoders of agent-payload v5.0.207, each holding three tag sets:
    // [env:prod kube_service:a kube_service:b], [env:prod role:db] and [zeta:1 alpha:2 alpha:2].
    pub fn tags_v1() -> Vec<u8> {
        bytes(
            "AQMACABlbnY6cHJvZA4Aa3ViZV9zZXJ2aWNlOmEOAGt1YmVfc2VydmljZTpiAgAIAGVudjpwcm9kBwByb2xlOmRiAwAGAHpldGE6MQcAYWxwaGE6MgcAYWxwaGE6MgAA",
        )
    }
    pub fn tags_v2() -> Vec<u8> {
        bytes(
            "AkkAAAAIAGVudjpwcm9kDgBrdWJlX3NlcnZpY2U6YQ4Aa3ViZV9zZXJ2aWNlOmIHAHJvbGU6ZGIGAHpldGE6MQcAYWxwaGE6MgMABQAAAA8AAAAfAAAAAgAFAAAALwAAAAMAOAAAAEAAAABAAAAA",
        )
    }
    pub fn tags_v3() -> Vec<u8> {
        bytes(
            "A0kAAAAIAGVudjpwcm9kDgBrdWJlX3NlcnZpY2U6YQ4Aa3ViZV9zZXJ2aWNlOmIHAHJvbGU6ZGIHAGFscGhhOjIGAHpldGE6MQMABQAAAA8AAAAfAAAAAgAFAAAALwAAAAIAOAAAAEEAAAA=",
        )
    }

    const FIRST_SET: [&str; 3] = ["env:prod", "kube_service:a", "kube_service:b"];
    const SECOND_SET: [&str; 2] = ["env:prod", "role:db"];

    #[test]
    fn tag_sets_come_out_of_a_version_1_buffer() {
        let buffer = tags_v1();
        assert_eq!(tag_buffer::tags(&buffer, 1), FIRST_SET);
        assert_eq!(tag_buffer::tags(&buffer, 45), SECOND_SET);
        assert_eq!(tag_buffer::tags(&buffer, 66), ["zeta:1", "alpha:2", "alpha:2"]);
        // What the encoder returned for an empty set.
        assert!(tag_buffer::tags(&buffer, 94).is_empty());
        // Byte 0 is the version: an index of 0 is one that was not sent.
        assert!(tag_buffer::tags(&buffer, 0).is_empty());
    }

    #[test]
    fn tag_sets_come_out_of_a_version_2_buffer() {
        let buffer = tags_v2();
        assert_eq!(tag_buffer::tags(&buffer, 0), FIRST_SET);
        assert_eq!(tag_buffer::tags(&buffer, 14), SECOND_SET);
        assert_eq!(tag_buffer::tags(&buffer, 24), ["zeta:1", "alpha:2", "alpha:2"]);
    }

    /// Version 3 is version 2's layout with sorted, deduplicated sets.
    #[test]
    fn tag_sets_come_out_of_a_version_3_buffer() {
        let buffer = tags_v3();
        assert_eq!(tag_buffer::tags(&buffer, 0), FIRST_SET);
        assert_eq!(tag_buffer::tags(&buffer, 14), SECOND_SET);
        assert_eq!(tag_buffer::tags(&buffer, 24), ["alpha:2", "zeta:1"]);
    }

    #[test]
    fn no_tags_for_a_negative_index_one_past_the_end_an_empty_buffer_or_an_unknown_version() {
        for buffer in [tags_v1(), tags_v2(), tags_v3()] {
            // -1 is what the encoders return for "no tags".
            assert!(tag_buffer::tags(&buffer, -1).is_empty());
            assert!(tag_buffer::tags(&buffer, 9999).is_empty());
        }
        assert!(tag_buffer::tags(&[], 0).is_empty());
        let unknown = [&[9u8][..], &tags_v2()[1..]].concat();
        assert!(tag_buffer::tags(&unknown, 0).is_empty());
    }

    #[test]
    fn a_tag_buffer_cut_short_gives_the_tags_before_the_break_never_a_panic() {
        for buffer in [tags_v1(), tags_v2(), tags_v3()] {
            for length in 0..buffer.len() {
                for index in [0, 1, 14, 24, 45, 66] {
                    tag_buffer::tags(&buffer[..length], index);
                }
            }
        }
        // The second set of the version 2 buffer, cut inside its second tag's position.
        assert_eq!(tag_buffer::tags(&tags_v2()[..95], 14), ["env:prod"]);
        // The first set of the version 1 buffer, cut inside its third tag.
        assert_eq!(tag_buffer::tags(&tags_v1()[..36], 1), ["env:prod", "kube_service:a"]);
    }

    // Made by V1DNSEncoder.Encode and V2DNSEncoder.EncodeDomainDatabase of agent-payload
    // v5.0.207, from the same three names.
    pub fn dns_v1() -> Vec<u8> {
        bytes(
            "AQIAAjMBAAEAAwgxMC4wLjAuMQIADAgxMC4wLjAuMgEACDEwLjAuMC4zARgLZXhhbXBsZS5jb20LZXhhbXBsZS5vcmcaaW50ZXJuYWwuc3ZjLmNsdXN0ZXIubG9jYWw=",
        )
    }
    fn domain_database() -> Vec<u8> {
        bytes("AwALZXhhbXBsZS5jb20LZXhhbXBsZS5vcmcaaW50ZXJuYWwuc3ZjLmNsdXN0ZXIubG9jYWw=")
    }
    pub const DNS_NAMES: [&str; 3] = ["example.com", "example.org", "internal.svc.cluster.local"];

    #[test]
    fn dns_names_come_out_of_a_version_1_buffer() {
        assert_eq!(dns_buffer::names(&dns_v1(), &[]).unwrap(), DNS_NAMES);
    }

    #[test]
    fn dns_names_come_out_of_a_version_2_domain_database() {
        assert_eq!(dns_buffer::names(&[], &domain_database()).unwrap(), DNS_NAMES);
    }

    /// As the agent's own reader: it looks at encodedDNS first and does not fall back to the
    /// domain database.
    #[test]
    fn encoded_dns_in_an_unknown_version_gives_no_names_even_beside_a_domain_database() {
        assert_eq!(dns_buffer::names(&[9, 0, 0], &domain_database()), Ok(Vec::new()));
    }

    #[test]
    fn a_payload_with_neither_dns_buffer_has_no_names() {
        assert_eq!(dns_buffer::names(&[], &[]), Ok(Vec::new()));
    }

    #[test]
    fn a_dns_buffer_cut_short_is_an_error_or_fewer_names_never_a_panic() {
        let v1 = dns_v1();
        for length in 1..v1.len() {
            let _ = dns_buffer::names(&v1[..length], &[]);
        }
        let database = domain_database();
        for length in 1..database.len() {
            let _ = dns_buffer::names(&[], &database[..length]);
        }
        // The domain database cut inside its second name.
        assert!(dns_buffer::names(&[], &database[..20]).is_err());
        // A name longer than what is left of the buffer.
        assert!(dns_buffer::names(&[], &[1, 0, 50, 97]).is_err());
        // A name block longer than the buffer.
        assert!(dns_buffer::names(&[1, 0, 0, 0, 100, 0], &[]).is_err());
    }
}
