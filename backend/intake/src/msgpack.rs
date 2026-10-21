//! MessagePack, read. Tracers send APM stats, Data Streams and CI events in it. The bytes are read
//! by the `rmp` crate; this module only says what a value is to us and how a decoder takes fields
//! out of one.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use rmp::Marker;
use rmp::decode::{RmpRead, read_marker};

#[derive(Clone, Debug, PartialEq)]
pub enum MsgValue {
    Nil,
    Bool(bool),
    /// A negative integer, or any integer written in a signed format.
    Int(i64),
    UInt(u64),
    Float32(f32),
    Float(f64),
    Str(String),
    Bin(Vec<u8>),
    Array(Vec<MsgValue>),
    /// Entries in wire order. Keys are strings.
    Map(Vec<(String, MsgValue)>),
    Ext(i8, Vec<u8>),
}

/// How deep a value may nest. The walk below is recursive and a stack overflow ends the process,
/// so a body of nothing but nested array headers has to end in an error.
pub const MAX_DEPTH: usize = 512;

pub const TRUNCATED: &str = "the data ends before the value does";
pub const FORGED: &str = "a length in the data is larger than any value could be";

/// A value being read: the bytes it starts in, and where the reading is.
struct Reader<'a> {
    start: &'a [u8],
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn position(&self) -> usize {
        self.start.len() - self.rest.len()
    }

    /// A length from a header. A count no value could hold is refused before anything is
    /// allocated for it: every element takes at least a byte.
    fn count(&self, count: u32) -> Result<usize, String> {
        if count > i32::MAX as u32 {
            Err(FORGED.into())
        } else if count as usize > self.rest.len() {
            Err(TRUNCATED.into())
        } else {
            Ok(count as usize)
        }
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], String> {
        if length > self.rest.len() {
            return Err(TRUNCATED.into());
        }
        let (taken, rest) = self.rest.split_at(length);
        self.rest = rest;
        Ok(taken)
    }

    fn text(&mut self, length: usize) -> Result<String, String> {
        // Not UTF-8 is read with replacement characters, as MessagePack-CSharp's reader does.
        Ok(String::from_utf8_lossy(self.bytes(length)?).into_owned())
    }

    /// The length a sized marker announces: of a string, bytes, an extension, or the elements of
    /// a container. 0 for a marker with none.
    fn length(&mut self, marker: Marker) -> Result<u32, String> {
        let truncated = |_| TRUNCATED.to_string();
        Ok(match marker {
            Marker::FixStr(n) | Marker::FixArray(n) | Marker::FixMap(n) => u32::from(n),
            Marker::FixExt1 => 1,
            Marker::FixExt2 => 2,
            Marker::FixExt4 => 4,
            Marker::FixExt8 => 8,
            Marker::FixExt16 => 16,
            Marker::Str8 | Marker::Bin8 | Marker::Ext8 => u32::from(self.rest.read_data_u8().map_err(truncated)?),
            Marker::Str16 | Marker::Bin16 | Marker::Ext16 | Marker::Array16 | Marker::Map16 => {
                u32::from(self.rest.read_data_u16().map_err(truncated)?)
            }
            Marker::Str32 | Marker::Bin32 | Marker::Ext32 | Marker::Array32 | Marker::Map32 => {
                self.rest.read_data_u32().map_err(truncated)?
            }
            _ => 0,
        })
    }

    /// The value at the reader's position. Kept small, with every value that holds no other in
    /// `scalar`: this is the frame each level of nesting adds to the stack.
    fn value(&mut self, depth: usize) -> Result<MsgValue, String> {
        if depth > MAX_DEPTH {
            return Err(format!("nesting deeper than {MAX_DEPTH} levels"));
        }
        let at = self.position();
        let marker = read_marker(&mut self.rest).map_err(|_| TRUNCATED.to_string())?;
        match marker {
            Marker::FixArray(_) | Marker::Array16 | Marker::Array32 => {
                let count = self.length(marker)?;
                self.array(count, depth)
            }
            Marker::FixMap(_) | Marker::Map16 | Marker::Map32 => {
                let count = self.length(marker)?;
                self.map(count, depth)
            }
            _ => self.scalar(marker, at),
        }
    }

    fn scalar(&mut self, marker: Marker, at: usize) -> Result<MsgValue, String> {
        let truncated = |_| TRUNCATED.to_string();
        Ok(match marker {
            Marker::Null => MsgValue::Nil,
            Marker::True => MsgValue::Bool(true),
            Marker::False => MsgValue::Bool(false),
            Marker::FixPos(n) => MsgValue::UInt(u64::from(n)),
            Marker::U8 => MsgValue::UInt(u64::from(self.rest.read_data_u8().map_err(truncated)?)),
            Marker::U16 => MsgValue::UInt(u64::from(self.rest.read_data_u16().map_err(truncated)?)),
            Marker::U32 => MsgValue::UInt(u64::from(self.rest.read_data_u32().map_err(truncated)?)),
            Marker::U64 => MsgValue::UInt(self.rest.read_data_u64().map_err(truncated)?),
            Marker::FixNeg(n) => MsgValue::Int(i64::from(n)),
            Marker::I8 => MsgValue::Int(i64::from(self.rest.read_data_i8().map_err(truncated)?)),
            Marker::I16 => MsgValue::Int(i64::from(self.rest.read_data_i16().map_err(truncated)?)),
            Marker::I32 => MsgValue::Int(i64::from(self.rest.read_data_i32().map_err(truncated)?)),
            Marker::I64 => MsgValue::Int(self.rest.read_data_i64().map_err(truncated)?),
            Marker::F32 => MsgValue::Float32(self.rest.read_data_f32().map_err(truncated)?),
            Marker::F64 => MsgValue::Float(self.rest.read_data_f64().map_err(truncated)?),
            Marker::FixStr(_) | Marker::Str8 | Marker::Str16 | Marker::Str32 => {
                let length = self.length(marker)?;
                MsgValue::Str(self.text(length as usize)?)
            }
            Marker::Bin8 | Marker::Bin16 | Marker::Bin32 => {
                let length = self.length(marker)?;
                MsgValue::Bin(self.bytes(length as usize)?.to_vec())
            }
            Marker::Reserved => return Err(format!("byte 0xc1 at position {at} starts no MessagePack value")),
            // The rest are the extensions.
            _ => {
                let length = self.length(marker)?;
                let kind = self.rest.read_data_i8().map_err(truncated)?;
                MsgValue::Ext(kind, self.bytes(length as usize)?.to_vec())
            }
        })
    }

    fn array(&mut self, count: u32, depth: usize) -> Result<MsgValue, String> {
        let count = self.count(count)?;
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(self.value(depth + 1)?);
        }
        Ok(MsgValue::Array(items))
    }

    fn map(&mut self, count: u32, depth: usize) -> Result<MsgValue, String> {
        let count = self.count(count)?;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let at = self.position();
            // Looked at before it is read: a key that is not a string is refused as it is, not
            // after a walk through whatever it holds.
            let key = match self.rest.first() {
                None => return Err(TRUNCATED.into()),
                Some(0xa0..=0xbf | 0xd9..=0xdb) => self.value(depth + 1)?,
                Some(_) => MsgValue::Nil,
            };
            let MsgValue::Str(key) = key else { return Err(format!("map key at byte {at} is not a string")) };
            entries.push((key, self.value(depth + 1)?));
        }
        Ok(MsgValue::Map(entries))
    }
}

/// The next value of bytes a decoder is walking by hand; `rest` is left after it. A position in
/// an error counts from where the value starts.
pub fn read(rest: &mut &[u8]) -> Result<MsgValue, String> {
    let mut reader = Reader { start: rest, rest };
    let value = reader.value(0)?;
    *rest = reader.rest;
    Ok(value)
}

/// A whole body as one value. What follows the value is not looked at.
pub fn decode(body: &[u8]) -> Result<MsgValue, String> {
    if body.is_empty() {
        return Err("empty body".into());
    }
    read(&mut &body[..])
}

/// What a value is, for a note about one that does not fit.
pub fn kind(value: &MsgValue) -> String {
    match value {
        MsgValue::Nil => "nil".into(),
        MsgValue::Bool(_) => "a bool".into(),
        MsgValue::Int(n) => format!("the integer {n}"),
        MsgValue::UInt(n) => format!("the integer {n}"),
        MsgValue::Float32(_) | MsgValue::Float(_) => "a float".into(),
        MsgValue::Str(_) => "a string".into(),
        MsgValue::Bin(_) => "bytes".into(),
        MsgValue::Array(_) => "an array".into(),
        MsgValue::Map(_) => "a map".into(),
        MsgValue::Ext(..) => "an extension value".into(),
    }
}

/// The fields of a MessagePack map, read by name, the way a decoder generated for a struct reads
/// them: the last of two equal keys wins, and nil is the same as absent. A value of the wrong type
/// is noted in `problems` with its path and the field keeps its default; the caller looks at
/// `problems` once the whole document has been read.
pub struct MsgFields<'a> {
    problems: &'a RefCell<Vec<String>>,
    path: String,
    entries: &'a [(String, MsgValue)],
}

impl<'a> MsgFields<'a> {
    pub fn new(problems: &'a RefCell<Vec<String>>, path: &str, value: &'a MsgValue) -> Self {
        let entries: &[(String, MsgValue)] = match value {
            MsgValue::Map(entries) => entries,
            MsgValue::Nil => &[],
            other => {
                let note = if path.is_empty() {
                    format!("expected a map, got {}", kind(other))
                } else {
                    format!("{path}: expected a map, got {}", kind(other))
                };
                problems.borrow_mut().push(note);
                &[]
            }
        };
        MsgFields { problems, path: path.into(), entries }
    }

    fn at(&self, name: &str) -> String {
        if self.path.is_empty() { name.into() } else { format!("{}/{name}", self.path) }
    }

    fn find(&self, name: &str) -> Option<&'a MsgValue> {
        self.entries.iter().rev().find(|(key, _)| key == name).map(|(_, found)| found).filter(|f| **f != MsgValue::Nil)
    }

    fn refuse(&self, name: &str, wanted: &str, found: &MsgValue) {
        self.problems.borrow_mut().push(format!("{}: expected {wanted}, got {}", self.at(name), kind(found)));
    }

    /// The field read with `read`; `zero` when it is absent or does not fit.
    fn field<T>(&self, name: &str, wanted: &str, zero: T, read: impl Fn(&'a MsgValue) -> Option<T>) -> T {
        match self.find(name) {
            None => zero,
            Some(found) => read(found).unwrap_or_else(|| {
                self.refuse(name, wanted, found);
                zero
            }),
        }
    }

    pub fn string(&self, name: &str) -> String {
        self.field(name, "a string", String::new(), |found| match found {
            MsgValue::Str(text) => Some(text.clone()),
            _ => None,
        })
    }

    pub fn bool(&self, name: &str) -> bool {
        self.field(name, "a bool", false, |found| match found {
            MsgValue::Bool(flag) => Some(*flag),
            _ => None,
        })
    }

    pub fn uint64(&self, name: &str) -> u64 {
        self.field(name, "an unsigned integer", 0, unsigned)
    }

    pub fn uint32(&self, name: &str) -> u32 {
        self.field(name, "an unsigned 32-bit integer", 0, |found| unsigned(found).and_then(|n| u32::try_from(n).ok()))
    }

    pub fn int64(&self, name: &str) -> i64 {
        self.field(name, "an integer", 0, signed)
    }

    pub fn int32(&self, name: &str) -> i32 {
        self.field(name, "a 32-bit integer", 0, |found| signed(found).and_then(|n| i32::try_from(n).ok()))
    }

    pub fn bytes(&self, name: &str) -> Vec<u8> {
        self.field(name, "bytes", Vec::new(), |found| match found {
            MsgValue::Bin(bytes) => Some(bytes.clone()),
            _ => None,
        })
    }

    pub fn strings(&self, name: &str) -> Vec<String> {
        self.field(name, "an array of strings", Vec::new(), |found| match found {
            MsgValue::Array(items) => items
                .iter()
                .map(|item| match item {
                    MsgValue::Str(text) => Some(text.clone()),
                    _ => None,
                })
                .collect(),
            _ => None,
        })
    }

    /// An array of maps, each as its fields. A nil element is left out: an encoder writes one for
    /// a nil pointer. Unlike a scalar, a list that appears under the same key twice is the two
    /// lists one after the other.
    pub fn maps(&self, name: &str) -> Vec<MsgFields<'a>> {
        let mut found = Vec::new();
        for (key, value) in self.entries.iter().filter(|(key, _)| key == name) {
            match value {
                MsgValue::Nil => {}
                MsgValue::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        if *item != MsgValue::Nil {
                            found.push(MsgFields::new(self.problems, &format!("{}/{i}", self.at(key)), item));
                        }
                    }
                }
                other => self.refuse(name, "an array", other),
            }
        }
        found
    }

    /// The fields whose names are not in `known`, with their values.
    pub fn unknown(&self, known: &BTreeSet<&str>) -> BTreeMap<String, MsgValue> {
        self.entries.iter().filter(|(key, _)| !known.contains(key.as_str())).cloned().collect()
    }
}

fn unsigned(found: &MsgValue) -> Option<u64> {
    match found {
        MsgValue::UInt(n) => Some(*n),
        MsgValue::Int(n) => u64::try_from(*n).ok(),
        _ => None,
    }
}

fn signed(found: &MsgValue) -> Option<i64> {
    match found {
        MsgValue::Int(n) => Some(*n),
        MsgValue::UInt(n) => i64::try_from(*n).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn str(text: &str) -> MsgValue {
        MsgValue::Str(text.into())
    }

    #[test]
    fn scalars() {
        assert_eq!(decode(&[0xc0]), Ok(MsgValue::Nil));
        assert_eq!(decode(&[0xc3]), Ok(MsgValue::Bool(true)));
        assert_eq!(decode(&[0x07]), Ok(MsgValue::UInt(7)));
        assert_eq!(decode(&[0xff]), Ok(MsgValue::Int(-1)));
        assert_eq!(decode(&[0xcd, 0x01, 0x2c]), Ok(MsgValue::UInt(300)));
        assert_eq!(decode(&[0xd1, 0xfe, 0xd4]), Ok(MsgValue::Int(-300)));
        assert_eq!(decode(&[0xcb, 0x3f, 0xf8, 0, 0, 0, 0, 0, 0]), Ok(MsgValue::Float(1.5)));
        assert_eq!(decode(&[0xca, 0x3f, 0xc0, 0, 0]), Ok(MsgValue::Float32(1.5)));
        assert_eq!(decode(&[0xa2, b'h', b'i']), Ok(str("hi")));
    }

    #[test]
    fn containers_keep_their_order() {
        // {"a": [1, nil], "b": "x"}
        let body = [0x82, 0xa1, b'a', 0x92, 0x01, 0xc0, 0xa1, b'b', 0xa1, b'x'];
        let expected = MsgValue::Map(vec![
            ("a".into(), MsgValue::Array(vec![MsgValue::UInt(1), MsgValue::Nil])),
            ("b".into(), str("x")),
        ]);
        assert_eq!(decode(&body), Ok(expected));
    }

    #[test]
    fn a_forged_length_is_refused_before_anything_is_allocated() {
        // array32 claiming four billion elements, with no elements behind it.
        assert_eq!(decode(&[0xdd, 0xff, 0xff, 0xff, 0xff]), Err(FORGED.into()));
        assert_eq!(decode(&[0xdd, 0x00, 0x00, 0xff, 0xff]), Err(TRUNCATED.into()));
    }

    #[test]
    fn an_empty_or_truncated_body_is_an_error() {
        assert!(decode(&[]).is_err());
        assert_eq!(decode(&[0xa5, b'h']), Err(TRUNCATED.into()));
    }

    #[test]
    fn the_reserved_byte_starts_no_value() {
        assert_eq!(decode(&[0x91, 0xc1]), Err("byte 0xc1 at position 1 starts no MessagePack value".into()));
    }

    #[test]
    fn fields_are_read_by_name_the_last_of_two_equal_keys_wins_nil_is_absent() {
        // {"n": 1, "s": "a", "n": 7, "gone": nil}
        let body =
            [0x84, 0xa1, b'n', 0x01, 0xa1, b's', 0xa1, b'a', 0xa1, b'n', 0x07, 0xa4, b'g', b'o', b'n', b'e', 0xc0];
        let root = decode(&body).unwrap();
        let problems = RefCell::new(Vec::new());
        let fields = MsgFields::new(&problems, "", &root);
        assert_eq!(
            (fields.uint64("n"), fields.string("s"), fields.string("gone"), fields.int64("missing")),
            (7, "a".to_string(), String::new(), 0)
        );
        assert!(problems.borrow().is_empty());
        let unknown = fields.unknown(&BTreeSet::from(["n"]));
        assert_eq!(unknown, BTreeMap::from([("gone".to_string(), MsgValue::Nil), ("s".to_string(), str("a"))]));
    }

    #[test]
    fn a_field_of_the_wrong_type_keeps_its_default_and_is_noted_with_its_path() {
        // {"items": [{"count": "three"}, nil, {"count": -1}]}
        let item = |value: &[u8]| [&[0x81, 0xa5, b'c', b'o', b'u', b'n', b't'][..], value].concat();
        let body = [
            &[0x81, 0xa5, b'i', b't', b'e', b'm', b's', 0x93][..],
            &item(&[0xa5, b't', b'h', b'r', b'e', b'e']),
            &[0xc0],
            &item(&[0xff]),
        ]
        .concat();
        let root = decode(&body).unwrap();
        let problems = RefCell::new(Vec::new());
        let items = MsgFields::new(&problems, "", &root).maps("items");
        // The nil element is left out, and the others keep their place in the path.
        assert_eq!(items.iter().map(|item| item.uint64("count")).collect::<Vec<_>>(), vec![0, 0]);
        assert_eq!(
            *problems.borrow(),
            vec![
                "items/0/count: expected an unsigned integer, got a string",
                "items/2/count: expected an unsigned integer, got the integer -1"
            ]
        );
    }

    #[test]
    fn a_map_key_that_is_not_a_string_is_refused() {
        // {bin"a": 1}
        assert_eq!(decode(&[0x81, 0xc4, 0x01, b'a', 0x01]), Err("map key at byte 1 is not a string".into()));
    }

    #[test]
    fn a_body_nested_without_end_is_an_error_not_a_stack_overflow() {
        // A hundred thousand one-element array headers, one inside the other.
        let error = decode(&vec![0x91; 100_000]).unwrap_err();
        assert!(error.contains("nesting"), "{error}");
    }

    #[test]
    fn a_value_read_by_hand_leaves_the_rest() {
        let data = [0x01, 0xa1, b'x'];
        let mut rest = &data[..];
        assert_eq!(read(&mut rest), Ok(MsgValue::UInt(1)));
        assert_eq!(rest, &[0xa1, b'x']);
    }
}
