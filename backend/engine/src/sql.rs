/// A tenant, as carried in the first ORDER BY column of every ClickHouse table.
///
/// A type of its own rather than a string so no query can be built without one: forgetting the
/// tenant filter is the one mistake multi-tenancy cannot survive.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TenantId(pub String);

/// A value bound to a ClickHouse query parameter.
///
/// Closed on purpose. Every case maps to one ClickHouse type in the `{name:Type}` placeholder, so
/// the SQL text and the value can never disagree about it.
#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    String(String),
    Int64(i64),
    Float64(f64),
    StringArray(Vec<String>),
    Int64Array(Vec<i64>),
}

impl SqlValue {
    pub fn clickhouse_type(&self) -> &'static str {
        match self {
            SqlValue::String(_) => "String",
            SqlValue::Int64(_) => "Int64",
            SqlValue::Float64(_) => "Float64",
            SqlValue::StringArray(_) => "Array(String)",
            SqlValue::Int64Array(_) => "Array(Int64)",
        }
    }
}

/// A compiled query: text with `{name:Type}` placeholders plus their values.
///
/// This is the whole boundary between the engine and the database. The engine never puts a
/// user's value into `text`; the server hands `parameters` to the driver, which ClickHouse binds
/// on its side.
#[derive(Clone, Debug, PartialEq)]
pub struct Sql {
    pub text: String,
    pub parameters: Vec<(String, SqlValue)>,
}

/// The placeholder for a parameter, typed from its value.
pub fn param(name: &str, value: &SqlValue) -> String {
    format!("{{{name}:{}}}", value.clickhouse_type())
}

/// Collects the values a query binds and hands out their placeholders, each under a name of its
/// own: the prefix, then its place in the list.
#[derive(Debug, Default)]
pub struct SqlParams {
    values: Vec<(String, SqlValue)>,
}

impl SqlParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_as(&mut self, prefix: &str, value: SqlValue) -> String {
        let name = format!("{prefix}{}", self.values.len());
        let placeholder = param(&name, &value);
        self.values.push((name, value));
        placeholder
    }

    /// Under the prefix "p".
    pub fn add(&mut self, value: SqlValue) -> String {
        self.add_as("p", value)
    }

    pub fn into_parameters(self) -> Vec<(String, SqlValue)> {
        self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_value_gets_a_name_of_its_own() {
        let mut p = SqlParams::new();
        let text = format!("{} {}", p.add(SqlValue::String("a".into())), p.add_as("k", SqlValue::Int64(1)));
        assert_eq!(text, "{p0:String} {k1:Int64}");
        assert_eq!(
            p.into_parameters(),
            vec![("p0".into(), SqlValue::String("a".into())), ("k1".into(), SqlValue::Int64(1))]
        );
    }
}
