//! What a profiles row keeps of a pprof attachment.
//!
//! The bytes are read by the messages generated from pprof's own profile.proto. What follows the
//! parse are the checks github.com/google/pprof/profile runs on a profile before it trusts one: the
//! string table, the ids, and that samples and lines point at things the profile has.

use std::collections::HashSet;

use ninjapaws_proto::perftools::profiles::{Label, Profile, ValueType};
use prost::Message;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub sample_types: Vec<String>,
    pub sample_units: Vec<String>,
    pub sample_count: u64,
    pub time_nanos: i64,
    pub duration_nanos: i64,
    /// "type/unit" of the sampling period.
    pub period_type: String,
    pub period: i64,
    pub mapping_count: u32,
    pub location_count: u32,
    pub function_count: u32,
}

/// The ids as a set; None unless they are all different and none is 0, which pprof reserves.
fn distinct_and_non_zero(ids: impl Iterator<Item = u64>, count: usize) -> Option<HashSet<u64>> {
    let seen: HashSet<u64> = ids.collect();
    (seen.len() == count && !seen.contains(&0)).then_some(seen)
}

fn summarise(profile: &Profile) -> Option<Summary> {
    let strings = &profile.string_table;
    let resolves = |index: i64| index >= 0 && index < strings.len() as i64;

    // An absent value type is the one whose two names are both "".
    let period_type = profile.period_type.unwrap_or_default();
    let value_type_resolves = |t: &ValueType| resolves(t.r#type) && resolves(t.unit);

    // A label's unit is looked up only when it has no string value.
    let label_resolves = |l: &Label| {
        resolves(l.key)
            && if l.str != 0 {
                resolves(l.str)
            } else if l.num_unit != 0 {
                resolves(l.num_unit)
            } else {
                true
            }
    };

    // Even an empty profile looks up index 0, so the table cannot be empty, and its first string
    // must be "".
    let strings_resolve = strings.first().is_some_and(String::is_empty)
        && resolves(profile.drop_frames)
        && resolves(profile.keep_frames)
        && resolves(profile.default_sample_type)
        && resolves(profile.doc_url)
        && profile.comment.iter().all(|c| resolves(*c))
        && value_type_resolves(&period_type)
        && profile.sample_type.iter().all(value_type_resolves)
        && profile.mapping.iter().all(|m| resolves(m.filename) && resolves(m.build_id))
        && profile.function.iter().all(|f| resolves(f.name) && resolves(f.system_name) && resolves(f.filename))
        && profile.sample.iter().all(|s| s.label.iter().all(label_resolves));

    let mapping_ids = distinct_and_non_zero(profile.mapping.iter().map(|m| m.id), profile.mapping.len());
    let function_ids = distinct_and_non_zero(profile.function.iter().map(|f| f.id), profile.function.len());
    let location_ids = distinct_and_non_zero(profile.location.iter().map(|l| l.id), profile.location.len());

    let (true, Some(_), Some(known_functions), Some(known_locations)) =
        (strings_resolve, mapping_ids, function_ids, location_ids)
    else {
        return None;
    };
    let samples_are_sound = (!profile.sample_type.is_empty() || profile.sample.is_empty())
        && profile.sample.iter().all(|s| {
            s.value.len() == profile.sample_type.len() && s.location_id.iter().all(|id| known_locations.contains(id))
        });
    // A line must name a function, and one the profile has.
    let lines_are_sound =
        profile.location.iter().all(|l| l.line.iter().all(|line| known_functions.contains(&line.function_id)));
    if !(samples_are_sound && lines_are_sound) {
        return None;
    }
    let text = |index: i64| strings[index as usize].clone();
    Some(Summary {
        sample_types: profile.sample_type.iter().map(|t| text(t.r#type)).collect(),
        sample_units: profile.sample_type.iter().map(|t| text(t.unit)).collect(),
        sample_count: profile.sample.len() as u64,
        time_nanos: profile.time_nanos,
        duration_nanos: profile.duration_nanos,
        // Without a period type both names are "", so this reads "/".
        period_type: format!("{}/{}", text(period_type.r#type), text(period_type.unit)),
        period: profile.period,
        mapping_count: profile.mapping.len() as u32,
        location_count: profile.location.len() as u32,
        function_count: profile.function.len() as u32,
    })
}

/// The envelope of an uncompressed pprof profile; None when the bytes are not one.
pub fn read(data: &[u8]) -> Option<Summary> {
    if data.is_empty() {
        return None;
    }
    summarise(&Profile::decode(data).ok()?)
}
