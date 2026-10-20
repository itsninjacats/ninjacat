//! Reads the members of a JSON object into typed values. One set of rules for every intake that
//! reads JSON field by field:
//!
//!   - a member is found by its exact name, and of two with the same name the last one counts;
//!   - null reads the same as a missing member: the field stays empty;
//!   - an integer is read from the literal's digits: 1.0 and 1e2 are refused, and a 64-bit id
//!     never passes through a float;
//!   - a value of the wrong type does not stop the read. It is noted, the rest is still read, and
//!     the caller decides what to do with a document that has notes.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::Value;

use crate::json;

/// What did not fit while one document was read, in reading order.
pub type Mismatches = RefCell<Vec<String>>;

static EMPTY: Value = Value::Null;

/// A JSON object whose members are read one by one. `path` says where it sits in the document,
/// for the notes. A value that is not an object has no members.
pub struct Fields<'b, 'a> {
    bad: &'b Mismatches,
    path: String,
    value: &'a Value,
}

impl<'b, 'a> Fields<'b, 'a> {
    fn new(bad: &'b Mismatches, path: String, value: &'a Value) -> Self {
        Fields { bad, path, value }
    }

    fn at(&self, name: &str) -> String {
        if self.path.is_empty() { name.to_string() } else { format!("{}.{name}", self.path) }
    }

    /// The member's value; None when there is none or it is null.
    pub fn find(&self, name: &str) -> Option<&'a Value> {
        json::field(name, self.value)
    }

    /// Every member by name, the null ones included.
    pub fn members(&self) -> BTreeMap<String, Value> {
        json::members(self.value)
    }

    /// Notes a member whose value does not fit.
    pub fn refuse(&self, name: &str, expected: &str, got: &Value) {
        self.bad.borrow_mut().push(format!("{}: expected {expected}, got {}", self.at(name), json::kind(got)));
    }

    /// Notes a member that was read and is still not acceptable.
    pub fn invalid(&self, name: &str, problem: &str) {
        self.bad.borrow_mut().push(format!("{}: {problem}", self.at(name)));
    }

    /// Notes a member that has to be there and is not.
    pub fn require(&self, name: &str) {
        if self.find(name).is_none() {
            self.invalid(name, "missing");
        }
    }

    /// The member read with `read`; None when it is missing or null, and when its value does not
    /// fit, which is also noted.
    pub fn try_read<T>(&self, name: &str, expected: &str, read: impl Fn(&'a Value) -> Option<T>) -> Option<T> {
        let found = self.find(name)?;
        let result = read(found);
        if result.is_none() {
            self.refuse(name, expected, found);
        }
        result
    }

    /// The same, with `zero` in place of None.
    pub fn read<T>(&self, name: &str, expected: &str, zero: T, read: impl Fn(&'a Value) -> Option<T>) -> T {
        self.try_read(name, expected, read).unwrap_or(zero)
    }

    pub fn string(&self, name: &str) -> String {
        self.read(name, "a string", String::new(), json::string_of)
    }

    pub fn optional_string(&self, name: &str) -> Option<String> {
        self.try_read(name, "a string", json::string_of)
    }

    /// The digits of a number as written, or "".
    pub fn number(&self, name: &str) -> String {
        self.read(name, "a number", String::new(), json::number_text)
    }

    pub fn int64(&self, name: &str) -> i64 {
        self.read(name, "an integer", 0, json::int64_of)
    }

    pub fn optional_int64(&self, name: &str) -> Option<i64> {
        self.try_read(name, "an integer", json::int64_of)
    }

    pub fn optional_int32(&self, name: &str) -> Option<i32> {
        self.try_read(name, "a 32-bit integer", json::int32_of)
    }

    pub fn int32(&self, name: &str) -> i32 {
        self.optional_int32(name).unwrap_or(0)
    }

    pub fn uint64(&self, name: &str) -> u64 {
        self.read(name, "an unsigned integer", 0, json::uint64_of)
    }

    pub fn uint32(&self, name: &str) -> u32 {
        self.read(name, "an unsigned 32-bit integer", 0, |v| json::uint64_of(v).and_then(|n| u32::try_from(n).ok()))
    }

    pub fn uint16(&self, name: &str) -> u16 {
        self.read(name, "an unsigned 16-bit integer", 0, |v| json::uint64_of(v).and_then(|n| u16::try_from(n).ok()))
    }

    pub fn float(&self, name: &str) -> f64 {
        self.read(name, "a number", 0.0, json::float_of)
    }

    pub fn float32(&self, name: &str) -> f32 {
        self.read(name, "a 32-bit float", 0.0, |v| json::float_of(v).map(|f| f as f32).filter(|f| f.is_finite()))
    }

    pub fn bool(&self, name: &str) -> bool {
        self.read(name, "a bool", false, json::bool_of)
    }

    pub fn optional_bool(&self, name: &str) -> Option<bool> {
        self.try_read(name, "a bool", json::bool_of)
    }

    pub fn strings(&self, name: &str) -> Vec<String> {
        self.read(name, "a list of strings", Vec::new(), json::strings_of)
    }

    pub fn optional_strings(&self, name: &str) -> Option<Vec<String>> {
        self.try_read(name, "a list of strings", json::strings_of)
    }

    pub fn floats(&self, name: &str) -> Vec<f64> {
        self.read(name, "a list of numbers", Vec::new(), json::floats_of)
    }

    /// The member's text, `null` included; "" when it was not sent.
    pub fn raw(&self, name: &str) -> String {
        json::raw_or_empty(self.value.get(name))
    }

    /// The elements of a list member, each still to be read.
    pub fn items(&self, name: &str) -> Vec<&'a Value> {
        self.read(name, "a list", Vec::new(), |v| v.as_array().map(|items| items.iter().collect()))
    }

    /// A value as an object to read: null reads as an empty one, anything but an object is noted
    /// and reads as an empty one too.
    fn as_object(&self, at: String, found: &'a Value) -> Fields<'b, 'a> {
        match found {
            Value::Object(_) => Fields::new(self.bad, at, found),
            Value::Null => Fields::new(self.bad, at, &EMPTY),
            other => {
                self.bad.borrow_mut().push(format!("{at}: expected an object, got {}", json::kind(other)));
                Fields::new(self.bad, at, &EMPTY)
            }
        }
    }

    pub fn object(&self, name: &str) -> Fields<'b, 'a> {
        match self.find(name) {
            Some(found) => self.as_object(self.at(name), found),
            None => Fields::new(self.bad, self.at(name), &EMPTY),
        }
    }

    /// None when the member is missing or null.
    pub fn optional_object(&self, name: &str) -> Option<Fields<'b, 'a>> {
        self.find(name).map(|found| self.as_object(self.at(name), found))
    }

    pub fn objects(&self, name: &str) -> Vec<Fields<'b, 'a>> {
        self.items(name)
            .into_iter()
            .enumerate()
            .map(|(i, item)| self.as_object(format!("{}[{i}]", self.at(name)), item))
            .collect()
    }
}

/// A document's root, or one element of a root list, as an object to read: null is an empty one,
/// anything else is noted.
pub fn fields<'b, 'a>(bad: &'b Mismatches, path: &str, value: &'a Value) -> Fields<'b, 'a> {
    match value {
        Value::Object(_) => Fields::new(bad, path.into(), value),
        Value::Null => Fields::new(bad, path.into(), &EMPTY),
        other => {
            let note = if path.is_empty() {
                format!("expected an object, got {}", json::kind(other))
            } else {
                format!("{path}: expected an object, got {}", json::kind(other))
            };
            bad.borrow_mut().push(note);
            Fields::new(bad, path.into(), &EMPTY)
        }
    }
}
