//! A tag filter as a SQL condition on a table with a `host` column and a `tags` map of value
//! lists: `metrics`, `metrics_1m` and `logs` alike. Raw `metrics` also keeps some tags as array
//! columns of their own (`metrics::LIFTED_TAGS`), which it passes as `columns`; the others pass
//! none.

use crate::metric_query::ast::TagFilter;
use crate::sql::{SqlParams, SqlValue};

/// Datadog's `*` wildcard as a LIKE pattern. `_` and `%` are LIKE's own wildcards and common in
/// tag values (`kube_namespace`), so they are escaped.
fn like_pattern(value: &str) -> SqlValue {
    SqlValue::String(value.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_").replace('*', "%"))
}

fn is_wildcard(value: &str) -> bool {
    value.contains('*')
}

fn string(value: &str) -> SqlValue {
    SqlValue::String(value.to_string())
}

fn each(columns: &[&str], p: &mut SqlParams, filters: &[TagFilter], separator: &str) -> String {
    filters.iter().map(|f| format!("({})", sql(columns, p, f))).collect::<Vec<_>>().join(separator)
}

/// `host` lives in its own column, not in the tag map: the agent sends it apart from the tags, and
/// the table's ORDER BY uses it.
///
/// Every value of the filter is bound; the text holds only what this function wrote.
pub fn sql(columns: &[&str], p: &mut SqlParams, filter: &TagFilter) -> String {
    match filter {
        TagFilter::All => "1".to_string(),
        TagFilter::Tag(k, v) if k == "host" && is_wildcard(v) => format!("host LIKE {}", p.add(like_pattern(v))),
        TagFilter::Tag(k, v) if k == "host" => format!("host = {}", p.add(string(v))),
        // `k` is one of the caller's own column names, so it can be written as is.
        TagFilter::Tag(k, v) if columns.contains(&k.as_str()) && is_wildcard(v) => {
            format!("arrayExists(x -> x LIKE {}, {k})", p.add(like_pattern(v)))
        }
        TagFilter::Tag(k, v) if columns.contains(&k.as_str()) => format!("has({k}, {})", p.add(string(v))),
        TagFilter::Tag(k, v) if is_wildcard(v) => {
            let pattern = p.add(like_pattern(v));
            format!("arrayExists(x -> x LIKE {pattern}, tags[{}])", p.add_as("k", string(k)))
        }
        TagFilter::Tag(k, v) => {
            let key = p.add_as("k", string(k));
            format!("has(tags[{key}], {})", p.add(string(v)))
        }
        // A key-less tag is stored as a key with an empty value (Tags.split in the intake), so
        // `{prod}` asks whether the key exists.
        TagFilter::Bare(v) if is_wildcard(v) => {
            format!("arrayExists(x -> x LIKE {}, mapKeys(tags))", p.add(like_pattern(v)))
        }
        TagFilter::Bare(v) => format!("mapContains(tags, {})", p.add_as("k", string(v))),
        // `IN` with a wildcard among its values (`name IN (web-*, db)`) is the OR of its parts.
        //
        // WARNING(undocumented): wildcards inside IN. Datadog's tag rules forbid `*` in a tag
        // value, so a `*` in a filter can only ever mean one.
        TagFilter::In(k, vs) if vs.iter().any(|v| is_wildcard(v)) => {
            let exact: Vec<String> = vs.iter().filter(|v| !is_wildcard(v)).cloned().collect();
            let mut parts: Vec<TagFilter> =
                vs.iter().filter(|v| is_wildcard(v)).map(|v| TagFilter::Tag(k.clone(), v.clone())).collect();
            if !exact.is_empty() {
                parts.push(TagFilter::In(k.clone(), exact));
            }
            sql(columns, p, &TagFilter::Or(parts))
        }
        TagFilter::In(k, vs) if k == "host" => format!("has({}, host)", p.add(SqlValue::StringArray(vs.clone()))),
        TagFilter::In(k, vs) if columns.contains(&k.as_str()) => {
            format!("hasAny({k}, {})", p.add(SqlValue::StringArray(vs.clone())))
        }
        TagFilter::In(k, vs) => {
            let key = p.add_as("k", string(k));
            format!("hasAny(tags[{key}], {})", p.add(SqlValue::StringArray(vs.clone())))
        }
        TagFilter::Not(f) => format!("NOT ({})", sql(columns, p, f)),
        TagFilter::And(fs) => each(columns, p, fs, " AND "),
        TagFilter::Or(fs) => each(columns, p, fs, " OR "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compiled(columns: &[&str], filter: &TagFilter) -> (String, Vec<(String, SqlValue)>) {
        let mut p = SqlParams::new();
        let text = sql(columns, &mut p, filter);
        (text, p.into_parameters())
    }

    fn tag(k: &str, v: &str) -> TagFilter {
        TagFilter::Tag(k.into(), v.into())
    }

    #[test]
    fn values_are_bound_in_the_order_written() {
        let (text, params) = compiled(&[], &tag("env", "prod"));
        assert_eq!(text, "has(tags[{k0:String}], {p1:String})");
        assert_eq!(params, vec![("k0".into(), string("env")), ("p1".into(), string("prod"))]);

        let (text, params) = compiled(&[], &tag("env", "pr_od*"));
        assert_eq!(text, "arrayExists(x -> x LIKE {p0:String}, tags[{k1:String}])");
        assert_eq!(params[0].1, string("pr\\_od%"));
    }

    #[test]
    fn host_and_lifted_columns_are_their_own() {
        assert_eq!(compiled(&[], &tag("host", "a")).0, "host = {p0:String}");
        assert_eq!(compiled(&[], &tag("host", "web-*")).0, "host LIKE {p0:String}");
        assert_eq!(compiled(&["env"], &tag("env", "prod")).0, "has(env, {p0:String})");
        assert_eq!(compiled(&["env"], &tag("env", "pr*")).0, "arrayExists(x -> x LIKE {p0:String}, env)");
        assert_eq!(
            compiled(&["env"], &TagFilter::In("env".into(), vec!["a".into()])).0,
            "hasAny(env, {p0:Array(String)})"
        );
        assert_eq!(compiled(&[], &TagFilter::In("host".into(), vec!["a".into()])).0, "has({p0:Array(String)}, host)");
    }

    #[test]
    fn bare_tags_ask_for_the_key() {
        assert_eq!(compiled(&[], &TagFilter::Bare("canary".into())).0, "mapContains(tags, {k0:String})");
        assert_eq!(
            compiled(&[], &TagFilter::Bare("web-*".into())).0,
            "arrayExists(x -> x LIKE {p0:String}, mapKeys(tags))"
        );
    }

    #[test]
    fn in_with_wildcards_is_the_or_of_its_parts() {
        let (text, params) =
            compiled(&[], &TagFilter::In("name".into(), vec!["web-*".into(), "db".into(), "x".into()]));
        assert_eq!(
            text,
            "(arrayExists(x -> x LIKE {p0:String}, tags[{k1:String}])) OR (hasAny(tags[{k2:String}], {p3:Array(String)}))"
        );
        assert_eq!(params[3].1, SqlValue::StringArray(vec!["db".into(), "x".into()]));
    }

    #[test]
    fn not_and_or_nest_in_parentheses() {
        let filter = TagFilter::And(vec![
            TagFilter::All,
            TagFilter::Not(Box::new(TagFilter::Or(vec![tag("host", "a"), tag("host", "b")]))),
        ]);
        assert_eq!(compiled(&[], &filter).0, "(1) AND (NOT ((host = {p0:String}) OR (host = {p1:String})))");
    }
}
