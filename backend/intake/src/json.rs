//! JSON as every router reads and writes it: parsed into `serde_json::Value`, members in the
//! order they came, the last of a repeated name.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::wire::tags;

/// Senders do write strings holding bytes that are not UTF-8, or half of a surrogate pair
/// (Python writes `\udc80` for a byte it could not decode). The parser refuses both, which would
/// lose a whole body to one odd character. So a body is repaired before it is parsed: U+FFFD
/// takes their place.
pub fn repair(body: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    // A valid surrogate pair, half of one, or any other escape. Matching every escape is what
    // keeps `\\ud800` (a backslash, then text) apart.
    static ESCAPES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?s)\\u[dD][89abAB][0-9a-fA-F]{2}\\u[dD][c-fC-F][0-9a-fA-F]{2}|\\u[dD][89a-fA-F][0-9a-fA-F]{2}|\\.",
        )
        .unwrap()
    });
    let may_hold_half_pair = body.windows(3).any(|w| w == b"\\ud" || w == b"\\uD");
    match std::str::from_utf8(body) {
        Ok(_) if !may_hold_half_pair => std::borrow::Cow::Borrowed(body),
        _ => {
            let text = String::from_utf8_lossy(body);
            // Of the three alternatives only half a pair is six characters long.
            let repaired = ESCAPES.replace_all(&text, |found: &regex::Captures| {
                if found[0].len() == 6 { "\\ufffd".to_string() } else { found[0].to_string() }
            });
            std::borrow::Cow::Owned(repaired.into_owned().into_bytes())
        }
    }
}

/// The body as JSON, or the parser's message. The parser nests at most 128 deep (the F# server
/// allowed 512); a body that nests deeper is kept in raw_payloads.
pub fn try_parse(body: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(&repair(body)).map_err(|e| e.to_string())
}

/// The first JSON value of a body; what follows it is not looked at.
pub fn try_parse_first(body: &[u8]) -> Result<Value, String> {
    let repaired = repair(body);
    match serde_json::Deserializer::from_slice(&repaired).into_iter::<Value>().next() {
        Some(Ok(value)) => Ok(value),
        Some(Err(e)) => Err(e.to_string()),
        None => Err("the body is empty".into()),
    }
}

/// The body as a JSON object; None when it is anything else.
pub fn try_parse_object(body: &[u8]) -> Option<Map<String, Value>> {
    match try_parse(body) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// The name of a value's type, as it appears in a note.
pub fn kind(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
        Value::Null => "null",
    }
}

/// An object's members by name. Anything but an object has none.
pub fn members(value: &Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(object) => object.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        _ => BTreeMap::new(),
    }
}

/// A member's value; None when there is none or it is null.
pub fn field<'a>(name: &str, object: &'a Value) -> Option<&'a Value> {
    object.get(name).filter(|value| !value.is_null())
}

/// A number as it was written, so no digit of an integer is lost. A string that holds a number
/// is taken too.
pub fn number_text(value: &Value) -> Option<String> {
    static SYNTAX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\A-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?\z").unwrap());
    match value {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if SYNTAX.is_match(s) => Some(s.clone()),
        _ => None,
    }
}

pub fn string_of(value: &Value) -> Option<String> {
    value.as_str().map(String::from)
}

/// An integer as written: 1.0 and 1e2 are not one.
pub fn int64_of(value: &Value) -> Option<i64> {
    value.as_i64()
}

pub fn int32_of(value: &Value) -> Option<i32> {
    int64_of(value).and_then(|n| i32::try_from(n).ok())
}

pub fn uint64_of(value: &Value) -> Option<u64> {
    value.as_u64()
}

/// A number as a float; one too large for a float is refused.
pub fn float_of(value: &Value) -> Option<f64> {
    value.as_number().and_then(|n| n.as_f64()).filter(|f| f.is_finite())
}

/// A number as an integer where producers write whole quantities as floats: one with a fraction
/// or an exponent is truncated rather than lost.
pub fn truncated_int64_of(value: &Value) -> Option<i64> {
    int64_of(value).or_else(|| float_of(value).map(|f| f as i64))
}

pub fn bool_of(value: &Value) -> Option<bool> {
    value.as_bool()
}

/// The elements read with `read`, a null element as `zero`; None when the value is not an array
/// or an element does not fit.
pub fn list_of<T: Clone>(zero: T, read: impl Fn(&Value) -> Option<T>, value: &Value) -> Option<Vec<T>> {
    value.as_array()?.iter().map(|item| if item.is_null() { Some(zero.clone()) } else { read(item) }).collect()
}

pub fn strings_of(value: &Value) -> Option<Vec<String>> {
    list_of(String::new(), string_of, value)
}

pub fn floats_of(value: &Value) -> Option<Vec<f64>> {
    list_of(0.0, float_of, value)
}

/// An object whose values are all strings; a null value is "".
pub fn string_map_of(value: &Value) -> Option<BTreeMap<String, String>> {
    value
        .as_object()?
        .iter()
        .map(
            |(k, v)| {
                if v.is_null() { Some((k.clone(), String::new())) } else { v.as_str().map(|s| (k.clone(), s.into())) }
            },
        )
        .collect()
}

/// The value written again without whitespace, its members in the order they came.
pub fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

/// The bytes of a value, compact.
pub fn raw_bytes(value: &Value) -> Vec<u8> {
    compact(value).into_bytes()
}

/// The text as a JSON string literal.
pub fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

/// The values as one JSON array.
pub fn compact_array<'a>(values: impl IntoIterator<Item = &'a Value>) -> String {
    format!("[{}]", values.into_iter().map(compact).collect::<Vec<_>>().join(","))
}

/// The members as one JSON object, sorted by name; "{}" when there are none.
pub fn compact_object(values: &BTreeMap<String, Value>) -> String {
    let object: Map<String, Value> = values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    compact(&Value::Object(object))
}

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let by_name: BTreeMap<&String, Value> = object.iter().map(|(k, v)| (k, sorted(v))).collect();
            Value::Object(by_name.into_iter().map(|(k, v)| (k.clone(), v)).collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// The same with the keys of every object sorted: one text for one value, however it was
/// written.
pub fn compact_sorted(value: &Value) -> String {
    compact(&sorted(value))
}

// --- reading without notes ---
//
// For documents with no schema to hold them to: a value is an `Option<&Value>`, None when the
// member is missing or null. What fits is kept, anything else reads as empty, and nothing is
// noted.

/// A member of a value that may itself be missing.
pub fn child<'a>(name: &str, parent: Option<&'a Value>) -> Option<&'a Value> {
    parent.and_then(|p| field(name, p))
}

/// The value at the end of a path of member names.
pub fn at<'a>(path: &[&str], root: &'a Value) -> Option<&'a Value> {
    path.iter().try_fold(root, |current, name| field(name, current)).filter(|v| !v.is_null())
}

pub fn lenient_string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or_default().to_string()
}

pub fn lenient_int64(value: Option<&Value>) -> i64 {
    value.and_then(int64_of).unwrap_or(0)
}

/// A list of strings, an element that is not a string as "". None when the value is absent,
/// null or not a list.
pub fn lenient_strings(value: Option<&Value>) -> Option<Vec<String>> {
    value?.as_array().map(|items| items.iter().map(|item| lenient_string(Some(item))).collect())
}

/// An object of string lists: a key whose value is not a list keeps the key, with no values.
pub fn lenient_string_lists(value: Option<&Value>) -> BTreeMap<String, Vec<String>> {
    match value {
        Some(v) => {
            members(v).into_iter().map(|(k, list)| (k, lenient_strings(Some(&list)).unwrap_or_default())).collect()
        }
        None => BTreeMap::new(),
    }
}

/// The members of an object, else none.
pub fn lenient_members(value: Option<&Value>) -> BTreeMap<String, Value> {
    value.map(members).unwrap_or_default()
}

/// The elements of a list, else none.
pub fn lenient_items(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// A JSON bool as 1 or 0; None for anything else: "never said" is not "false".
pub fn flag(value: Option<&Value>) -> Option<u8> {
    value.and_then(bool_of).map(u8::from)
}

/// A value as the text a column holds: a string as it is, a number digit for digit, true or
/// false, an object or a list as JSON with sorted keys; "" when there is none.
pub fn text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v @ (Value::Object(_) | Value::Array(_))) => compact_sorted(v),
        Some(v) => v.to_string(),
    }
}

/// A value as JSON text for a column, compact and in the order it was sent; "" when there is
/// none.
pub fn compact_or_empty(value: Option<&Value>) -> String {
    value.map(compact).unwrap_or_default()
}

/// The same with sorted keys.
pub fn compact_sorted_or_empty(value: Option<&Value>) -> String {
    value.map(compact_sorted).unwrap_or_default()
}

/// A list as texts. A lone string that is not empty is a list of one: several producers send a
/// single tag as a bare string.
pub fn text_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().map(|item| text(Some(item))).collect(),
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// A tag list in either spelling: a list of strings, or one comma-separated string, as some
/// producers send `ddtags`. Anything else is no tags at all.
pub fn tag_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => tags::split_ddtags(s),
        Some(v) => strings_of(v).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// The JSON text of a value, compact, `null` included; "" when the member was not sent at all.
pub fn raw_or_empty(value: Option<&Value>) -> String {
    value.map(compact).unwrap_or_default()
}

/// Members as the texts of a Map(String, String) column, each value as the JSON `write` gives
/// it. A null member is the text `null`: an empty string is a value too, and not the one that
/// was sent.
pub fn member_texts(write: impl Fn(&Value) -> String, members: &BTreeMap<String, Value>) -> BTreeMap<String, String> {
    members.iter().map(|(k, v)| (k.clone(), write(v))).collect()
}

/// The members of an object that have no column of their own, those not in `known`, as such
/// texts.
pub fn other_members(
    known: &BTreeSet<&str>,
    write: impl Fn(&Value) -> String,
    object: Option<&Value>,
) -> BTreeMap<String, String> {
    let others = lenient_members(object).into_iter().filter(|(name, _)| !known.contains(name.as_str())).collect();
    member_texts(write, &others)
}

/// A body as its items: the elements of a list, or the one object of a sender that skipped the
/// batch. `null` is no items.
pub fn try_parse_list(body: &[u8]) -> Result<Vec<Value>, String> {
    match try_parse(body)? {
        Value::Array(items) => Ok(items),
        object @ Value::Object(_) => Ok(vec![object]),
        Value::Null => Ok(Vec::new()),
        other => Err(format!("expected a list or an object, got {}", kind(&other))),
    }
}

/// A body that has to be a list, as its elements. `null` is no elements; one bare object is
/// refused, which is what keeps the agent's `{}` probe from becoming an empty row.
pub fn try_parse_array(body: &[u8]) -> Result<Vec<Value>, String> {
    match try_parse(body)? {
        Value::Array(items) => Ok(items),
        Value::Null => Ok(Vec::new()),
        other => Err(format!("expected a list, got {}", kind(&other))),
    }
}

/// The elements of a JSON list, each as the bytes it had on the wire: for bodies whose elements
/// are stored as they were sent. `null` is no elements.
///
/// The body is not repaired, so bytes that are not UTF-8 are kept. They are read past as the
/// parser reads a repaired body: a copy of the body of the same length, where each such byte is
/// a `?` and half a surrogate pair is `�`, marks where each element starts and ends.
pub fn try_parse_element_bytes(body: &[u8]) -> Result<Vec<&[u8]>, String> {
    let marked: Vec<u8> = body
        .utf8_chunks()
        .flat_map(|chunk| chunk.valid().bytes().chain(std::iter::repeat_n(b'?', chunk.invalid().len())))
        .collect();
    let readable = repair(&marked);
    let elements: Option<Vec<&serde_json::value::RawValue>> = match serde_json::from_slice(&readable) {
        Ok(elements) => elements,
        Err(e) => {
            return Err(match try_parse(&readable) {
                Ok(other) if !other.is_array() => format!("expected a list, got {}", kind(&other)),
                _ => e.to_string(),
            });
        }
    };
    let start = readable.as_ptr() as usize;
    Ok(elements
        .unwrap_or_default()
        .iter()
        .map(|element| {
            let at = element.get().as_ptr() as usize - start;
            &body[at..at + element.get().len()]
        })
        .collect())
}

/// The object's property names, sorted and comma-separated, for a log line or a note that says
/// what was there.
pub fn keys(object: &Value) -> String {
    let mut names: Vec<&String> = match object {
        Value::Object(o) => o.keys().collect(),
        _ => return "(none)".into(),
    };
    names.sort();
    if names.is_empty() { "(none)".into() } else { names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ") }
}

/// Protobuf messages as JSON text, written and read in proto3's JSON mapping: lowerCamelCase
/// names, 64-bit integers as strings, enums by name, fields at their default left out.
pub mod proto {
    use super::*;

    /// A JSON body as the message of its shape. A field is taken under the name the schema gives
    /// it or in lowerCamelCase, an enum by name or by number, and keys the schema does not have
    /// are passed over.
    pub fn try_parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, String> {
        serde_json::from_slice(&repair(body)).map_err(|e| e.to_string())
    }

    /// "" for a message that is absent.
    pub fn message<T: Serialize>(value: Option<&T>) -> String {
        value.map(|m| serde_json::to_string(m).unwrap()).unwrap_or_default()
    }

    /// A list of messages as a JSON array; "" for an empty list.
    pub fn messages<T: Serialize>(values: &[T]) -> String {
        if values.is_empty() { String::new() } else { serde_json::to_string(values).unwrap() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_a_surrogate_pair_is_repaired_and_a_whole_one_kept() {
        let body = br#"{"a":"x\udc80y","b":"\ud83d\ude00","c":"\\ud800"}"#;
        let value = try_parse(body).unwrap();
        assert_eq!(value["a"], "x\u{fffd}y");
        assert_eq!(value["b"], "😀");
        assert_eq!(value["c"], "\\ud800");
    }

    #[test]
    fn bytes_that_are_not_utf8_become_the_replacement_character() {
        let value = try_parse(b"{\"a\":\"\xff\"}").unwrap();
        assert_eq!(value["a"], "\u{fffd}");
    }

    #[test]
    fn an_integer_is_read_from_its_digits() {
        assert_eq!(int64_of(&serde_json::json!(9007199254740993i64)), Some(9007199254740993));
        assert_eq!(int64_of(&try_parse(b"1.0").unwrap()), None);
        assert_eq!(truncated_int64_of(&try_parse(b"1.5").unwrap()), Some(1));
    }
}
