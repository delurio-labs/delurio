//! What the revision diff is made of, for both kinds of graph: structural changes keyed by id, and changes in behaviour.
//! Nothing here knows what a flow or a page is; diff_flow.rs and diff_nav.rs do. The port of compare.ts.
//!
//! JavaScript's `undefined` (a key that is not there) and `null` (a key written with no value) differ here: `canon` of
//! the one is `~`, of the other `null`, and a field whose old value is undefined is "added". A value that may be
//! undefined is an `Option<&Value>`.

use std::collections::HashSet;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde::Serialize;

use crate::js::{self, Object, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Added,
    Removed,
    Changed,
    Moved,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Added => "added",
            Op::Removed => "removed",
            Op::Changed => "changed",
            Op::Moved => "moved",
        }
    }
}

// One structural change. `say` is the sentence for people (titles and `say` texts, no ids); None (null) when the change
// is only listed under the exact changes (a source citation, a key graph-viz does not read) and counted in the area's
// summary line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub area: String,
    pub key: String, // what changed, by id: "state consent", "consent on ALLOW", "page dersPlan"
    pub op: Op,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
    pub say: Option<String>,
}

impl Change {
    /// a change with no field and no values; `field`, `before` and `after` add them
    pub fn new(area: &str, key: impl Into<String>, op: Op, say: Option<String>) -> Change {
        Change { area: area.to_string(), key: key.into(), op, field: None, before: None, after: None, say }
    }
    pub fn field(mut self, field: impl Into<String>) -> Change {
        self.field = Some(field.into());
        self
    }
    pub fn before(mut self, v: Option<&Value>) -> Change {
        self.before = v.cloned();
        self
    }
    pub fn after(mut self, v: Option<&Value>) -> Change {
        self.after = v.cloned();
        self
    }
    /// the change as `JSON.stringify` writes it: its keys in the order the objects were written, undefined ones left out
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.insert("area", Value::from(self.area.as_str()));
        o.insert("key", Value::from(self.key.as_str()));
        o.insert("op", Value::from(self.op.as_str()));
        if let Some(f) = &self.field {
            o.insert("field", Value::from(f.as_str()));
        }
        if let Some(b) = &self.before {
            o.insert("before", b.clone());
        }
        if let Some(a) = &self.after {
            o.insert("after", a.clone());
        }
        o.insert("say", self.say.as_deref().map_or(Value::Null, Value::from));
        Value::Object(o)
    }
}

// One change in what the graph does, found by running both revisions.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Shift {
    pub topic: String, // exit, path, guarantee, situations, main, reach, taken; nav: landing, page, clicks, menu, goal, finding
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>, // nav: the kind of user it is about (title)
    pub before: Value,
    pub after: Value,
    pub say: String,
    pub detail: Vec<String>, // the steps of a path, in words
    pub worse: bool, // a regression: a guarantee that newly fails, a goal that newly fails, a problem that is new
}

impl Shift {
    /// the shift as `JSON.stringify` writes it
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.insert("topic", Value::from(self.topic.as_str()));
        o.insert("key", Value::from(self.key.as_str()));
        if let Some(p) = &self.persona {
            o.insert("persona", Value::from(p.as_str()));
        }
        o.insert("before", self.before.clone());
        o.insert("after", self.after.clone());
        o.insert("say", Value::from(self.say.as_str()));
        o.insert("detail", Value::Array(self.detail.iter().map(|d| Value::from(d.as_str())).collect()));
        o.insert("worse", Value::Bool(self.worse));
        Value::Object(o)
    }
}

/// `x ?? {}` read as a record: a value that is no map reads as one with no keys
pub fn raw(v: Option<&Value>) -> &Object {
    static EMPTY: LazyLock<Object> = LazyLock::new(Object::new);
    v.and_then(Value::as_object).unwrap_or(&EMPTY)
}

/// `o[k]` of a value that may be undefined: a map's key; anything else has none
pub fn member<'a>(v: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    v.and_then(|x| x.as_object()).and_then(|o| o.get(key))
}

/// a value as a template literal writes it: `undefined` when it is not there
pub fn text(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), js::to_js_string)
}

/// `b === undefined ? 'added' : a === undefined ? 'removed' : 'changed'`
pub fn op_of(b: Option<&Value>, a: Option<&Value>) -> Op {
    if b.is_none() {
        Op::Added
    } else if a.is_none() {
        Op::Removed
    } else {
        Op::Changed
    }
}

/// `p === q` of two values that may be undefined: scalars by value, a list or a map only as the same object, which a
/// value of another file never is
pub fn strict_eq(p: Option<&Value>, q: Option<&Value>) -> bool {
    match (p, q) {
        (None, None) => true,
        (Some(x), Some(y)) => x.strict_eq(y),
        _ => false,
    }
}

/// SameValueZero, the equality of `includes`: `===`, and NaN is NaN
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a.strict_eq(b),
    }
}

// a value as text with sorted keys, so two values written in a different key order compare equal
pub fn canon(v: Option<&Value>) -> String {
    let Some(v) = v else { return "~".to_string() };
    let mut out = String::new();
    canon_into(v, &mut out);
    out
}

fn canon_into(v: &Value, out: &mut String) {
    match v {
        Value::Array(items) => {
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                canon_into(x, out);
            }
            out.push(']');
        }
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort_by(|a, b| js::cmp_utf16(a, b));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&js::quote_string(k));
                out.push(':');
                if let Some(x) = o.get(k) {
                    canon_into(x, out);
                }
            }
            out.push('}');
        }
        scalar => out.push_str(&js::stringify(scalar)),
    }
}

pub fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    canon(a) == canon(b)
}

// the keys of both maps: the new order first, then the keys only the old one has, in its order
pub fn union(before: Option<&Value>, after: Option<&Value>) -> Vec<String> {
    union_keys(&object_keys(before), &object_keys(after))
}

/// `Object.keys(v ?? {})` of a map; a value that is no map has none
pub fn object_keys(v: Option<&Value>) -> Vec<&str> {
    v.and_then(Value::as_object).map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default()
}

/// `union` of the keys of two maps, given as their keys in order
pub fn union_keys(before: &[&str], after: &[&str]) -> Vec<String> {
    let own: HashSet<&str> = after.iter().copied().collect();
    let mut out: Vec<String> = after.iter().map(|k| k.to_string()).collect();
    for k in before {
        if !own.contains(k) {
            out.push(k.to_string());
        }
    }
    out
}

pub fn quote(s: &str) -> String {
    format!("“{s}”")
}

pub fn join_words(xs: &[String]) -> String {
    match xs {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// a key of `match_ordered`: None pairs nothing (`null`)
pub type Key<'a, T> = &'a dyn Fn(&T) -> Option<String>;

// Pairs the items of two ordered lists (the rules of an `always`, the alternatives of one event, the entries of a nav graph).
// The first key pairs items in the order they occur (the n-th with the n-th); a later key pairs what is left only when it is
// unique on both sides, so nothing is paired by guesswork. Returns [before index, after index] pairs, sorted by before index.
pub fn match_ordered<T>(before: &[T], after: &[T], keys: &[Key<'_, T>]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut used_b: HashSet<usize> = HashSet::new();
    let mut used_a: HashSet<usize> = HashSet::new();
    for (round, key) in keys.iter().enumerate() {
        let group = |xs: &[T], used: &HashSet<usize>| -> IndexMap<String, Vec<usize>> {
            let mut m: IndexMap<String, Vec<usize>> = IndexMap::new();
            for (i, x) in xs.iter().enumerate() {
                if used.contains(&i) {
                    continue;
                }
                if let Some(k) = key(x) {
                    m.entry(k).or_default().push(i);
                }
            }
            m
        };
        let gb = group(before, &used_b);
        let ga = group(after, &used_a);
        for (k, bs) in &gb {
            let Some(as_) = ga.get(k) else { continue };
            if round > 0 && (bs.len() > 1 || as_.len() > 1) {
                continue;
            }
            for (&b, &a) in bs.iter().zip(as_) {
                pairs.push((b, a));
                used_b.insert(b);
                used_a.insert(a);
            }
        }
    }
    pairs.sort_by_key(|p| p.0);
    pairs
}

// The pairs that changed place relative to the others: the pairs outside the longest run that keeps its order.
// An item that only shifted because something was added or removed before it is not moved.
pub fn moved(pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut sorted = pairs.to_vec();
    sorted.sort_by_key(|p| p.0);
    let n = sorted.len();
    let mut len = vec![1usize; n];
    let mut prev: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        for j in 0..i {
            if sorted[j].1 < sorted[i].1 && len[j] + 1 > len[i] {
                len[i] = len[j] + 1;
                prev[i] = Some(j);
            }
        }
    }
    let mut best: Option<usize> = None;
    for i in 0..n {
        if best.is_none_or(|b| len[i] > len[b]) {
            best = Some(i);
        }
    }
    let mut keep: HashSet<usize> = HashSet::new();
    let mut i = best;
    while let Some(k) = i {
        keep.insert(k);
        i = prev[k];
    }
    sorted.into_iter().enumerate().filter(|(i, _)| !keep.contains(i)).map(|(_, p)| p).collect()
}

// A field-by-field comparison for data graph-viz does not interpret (domain.yaml, unknown keys): maps by key, lists whose
// items all carry an `id` by id, anything else as one value. A field called `source` or `evidence` is a citation.
pub fn deep_diff(
    area: &str,
    key: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    out: &mut Vec<Change>,
    path: &[String],
) {
    if same(before, after) {
        return;
    }
    if let (Some(Value::Object(b)), Some(Value::Object(a))) = (before, after) {
        for k in union(before, after) {
            let mut p = path.to_vec();
            p.push(k.clone());
            deep_diff(area, key, b.get(&k), a.get(&k), out, &p);
        }
        return;
    }
    // a list whose items are all maps with a text `id`, as a map by those ids (`Object.fromEntries`)
    let by_id = |v: Option<&Value>| -> Option<(usize, Object)> {
        let items = v?.as_array()?;
        let mut m = Object::new();
        for x in items {
            let id = x.as_object()?.get("id")?.as_str()?;
            m.insert(id, x.clone());
        }
        Some((items.len(), m))
    };
    if let (Some((nb, mb)), Some((na, ma))) = (by_id(before), by_id(after))
        && nb + na > 0
        && mb.len() == nb
        && ma.len() == na
    {
        let (vb, va) = (Value::Object(mb), Value::Object(ma));
        for id in union(Some(&vb), Some(&va)) {
            let mut p = path.to_vec();
            p.push(id.clone());
            deep_diff(area, key, member(Some(&vb), &id), member(Some(&va), &id), out, &p);
        }
        return;
    }
    let last = path.last().map(String::as_str);
    let mut c = Change::new(
        if last == Some("source") || last == Some("evidence") { "citations" } else { area },
        key,
        op_of(before, after),
        None,
    )
    .before(before)
    .after(after);
    if !path.is_empty() {
        c.field = Some(path.join("."));
    }
    out.push(c);
}

/// the words of a changed field, or None: `say(field, before, after)` of `field_changes`
pub type SayField<'a> = &'a dyn Fn(&str, Option<&Value>, Option<&Value>) -> Option<String>;

// the plain old → new comparison of named fields of one item; `say` words the change for people, or gives None
pub fn field_changes(
    area: &str,
    key: &str,
    before: &Object,
    after: &Object,
    fields: &[&str],
    say: SayField<'_>,
) -> Vec<Change> {
    let mut out = Vec::new();
    for &f in fields {
        let (b, a) = (before.get(f), after.get(f));
        if same(b, a) {
            continue;
        }
        let citation = f == "source" || f == "evidence";
        let words = if citation { None } else { say(f, b, a) };
        out.push(
            Change::new(if citation { "citations" } else { area }, key, Op::Changed, words).field(f).before(b).after(a),
        );
    }
    out
}

// the keys of an item that are not in `known`, compared as data
pub fn other_keys(key: &str, before: &Object, after: &Object, known: &[&str], out: &mut Vec<Change>) {
    let b: Vec<&str> = before.keys().map(String::as_str).collect();
    let a: Vec<&str> = after.keys().map(String::as_str).collect();
    for k in union_keys(&b, &a) {
        if known.contains(&k.as_str()) {
            continue;
        }
        deep_diff("other", key, before.get(&k), after.get(&k), out, std::slice::from_ref(&k));
    }
}

/// what `list_changes` finds
#[derive(Debug, Clone, PartialEq)]
pub struct ListChanges<T> {
    pub added: Vec<T>,
    pub removed: Vec<T>,
}

// added and removed items of two lists of plain words
pub fn list_changes<T: Clone>(before: &[T], after: &[T], eq: impl Fn(&T, &T) -> bool) -> ListChanges<T> {
    ListChanges {
        added: after.iter().filter(|x| !before.iter().any(|y| eq(y, x))).cloned().collect(),
        removed: before.iter().filter(|x| !after.iter().any(|y| eq(y, x))).cloned().collect(),
    }
}

/// `list_changes` of two lists of words
pub fn word_changes(before: &[String], after: &[String]) -> ListChanges<String> {
    list_changes(before, after, |a, b| a == b)
}

/// `list_changes` of two lists of values that may be undefined (`x ?? []`): a value that is no list has no items
pub fn value_changes(before: Option<&Value>, after: Option<&Value>) -> ListChanges<Value> {
    let items = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    list_changes(&items(before), &items(after), same_value_zero)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        Value::Object(pairs.iter().map(|(k, v)| (*k, v.clone())).collect())
    }

    // expected values: node work-diff/compare-cases.mjs against ts-ref/dist/compare.js
    #[test]
    fn canonical_text() {
        assert_eq!(canon(None), "~");
        assert_eq!(canon(Some(&Value::Null)), "null");
        assert_eq!(canon(Some(&Value::from(1.0))), "1");
        assert_eq!(canon(Some(&Value::from(0.5))), "0.5");
        assert_eq!(canon(Some(&Value::from(1e21))), "1e+21");
        assert_eq!(canon(Some(&Value::from("a\"b"))), "\"a\\\"b\"");
        let nested = Value::Array(vec![
            Value::from(1.0),
            Value::Array(vec![Value::from(2.0), obj(&[("b", Value::from(1.0)), ("a", Value::from(2.0))])]),
        ]);
        assert_eq!(canon(Some(&nested)), "[1,[2,{\"a\":2,\"b\":1}]]");
        let keys = obj(&[
            ("b", Value::from(1.0)),
            ("a", Value::Array(vec![Value::Null])),
            ("10", Value::from(3.0)),
            ("2", Value::from(4.0)),
            ("Z", Value::from(0.0)),
        ]);
        assert_eq!(canon(Some(&keys)), "{\"10\":3,\"2\":4,\"Z\":0,\"a\":[null],\"b\":1}");
        assert_eq!(canon(Some(&obj(&[]))), "{}");
        assert_eq!(canon(Some(&Value::Array(vec![]))), "[]");
        assert!(same(
            Some(&obj(&[("x", Value::from(1.0)), ("y", Value::Null)])),
            Some(&obj(&[("y", Value::Null), ("x", Value::from(1.0))]))
        ));
        assert!(!same(None, Some(&Value::Null)));
    }

    #[test]
    fn ordered_matching() {
        let s = |x: &&str| Some(x.to_string());
        let b = ["a", "b", "c", "a"];
        let a = ["b", "a", "c", "x", "a"];
        assert_eq!(match_ordered(&b, &a, &[&s]), vec![(0, 1), (1, 0), (2, 2), (3, 4)]);

        struct T(&'static str, &'static str, &'static str);
        let bt = [T("x", "p", "1"), T("y", "q", "2"), T("z", "q", "3"), T("w", "r", "4")];
        let at = [T("x2", "p", "1"), T("y2", "q", "2"), T("z2", "q", "3"), T("w", "r", "4")];
        let g = |t: &T| Some(t.0.to_string());
        let st = |t: &T| Some(t.1.to_string());
        let tt = |t: &T| Some(t.2.to_string());
        let none = |_: &T| None;
        // the second key pairs only what is unique on both sides (p); q is left to the third
        assert_eq!(match_ordered(&bt, &at, &[&g, &st, &tt]), vec![(0, 0), (1, 1), (2, 2), (3, 3)]);
        assert_eq!(match_ordered(&bt, &at, &[&g, &none, &tt]), vec![(0, 0), (1, 1), (2, 2), (3, 3)]);
    }

    #[test]
    fn moved_pairs() {
        assert_eq!(moved(&[(0, 0), (1, 1), (2, 2)]), vec![]);
        assert_eq!(moved(&[(0, 2), (1, 0), (2, 1)]), vec![(0, 2)]);
        assert_eq!(moved(&[(0, 1), (1, 0)]), vec![(1, 0)]);
        assert_eq!(moved(&[(3, 0), (0, 3), (1, 1), (2, 2)]), vec![(0, 3), (3, 0)]);
        assert_eq!(moved(&[]), vec![]);
    }

    #[test]
    fn data_compared_field_by_field() {
        let n = |x: f64| Value::from(x);
        let b = obj(&[
            (
                "entities",
                Value::Array(vec![
                    obj(&[("id", "b".into()), ("x", n(1.0))]),
                    obj(&[("id", "a".into()), ("source", "s1".into())]),
                ]),
            ),
            ("k", n(1.0)),
        ]);
        let a = obj(&[
            (
                "entities",
                Value::Array(vec![
                    obj(&[("id", "a".into()), ("source", "s2".into())]),
                    obj(&[("id", "b".into()), ("x", n(2.0))]),
                    obj(&[("id", "1".into())]),
                ]),
            ),
            ("j", n(2.0)),
        ]);
        let mut out = Vec::new();
        deep_diff("domain", "domain", Some(&b), Some(&a), &mut out, &[]);
        let json = js::stringify(&Value::Array(out.iter().map(Change::to_value).collect()));
        assert_eq!(
            json,
            r#"[{"area":"domain","key":"domain","op":"added","field":"entities.1","after":{"id":"1"},"say":null},{"area":"citations","key":"domain","op":"changed","field":"entities.a.source","before":"s1","after":"s2","say":null},{"area":"domain","key":"domain","op":"changed","field":"entities.b.x","before":1,"after":2,"say":null},{"area":"domain","key":"domain","op":"added","field":"j","after":2,"say":null},{"area":"domain","key":"domain","op":"removed","field":"k","before":1,"say":null}]"#
        );
    }

    #[test]
    fn keys_of_both_and_words() {
        let b = obj(&[("a", Value::from(1.0)), ("b", Value::from(2.0)), ("c", Value::from(3.0))]);
        let a = obj(&[("c", Value::from(1.0)), ("d", Value::from(2.0)), ("a", Value::from(0.0))]);
        assert_eq!(union(Some(&b), Some(&a)), vec!["c", "d", "a", "b"]);
        let w = |xs: &[&str]| join_words(&xs.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!([w(&[]), w(&["a"]), w(&["a", "b"]), w(&["a", "b", "c"])], ["", "a", "a and b", "a, b and c"]);
    }
}
