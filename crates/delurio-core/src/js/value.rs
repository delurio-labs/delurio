use super::number::number_to_string;
use super::object::Object;

/// A value of a graph file as JavaScript holds it once `JSON.parse` or the yaml library has read it. There is no
/// `undefined`: a key that is not there is `None` where it is looked up.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Object),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }
    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }
    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
    }
    /// `typeof v === 'object' && v !== null`: an object or an array
    pub fn is_collection(&self) -> bool {
        matches!(self, Value::Object(_) | Value::Array(_))
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }
    /// `v[key]` on an object; None on anything else or a key it does not have
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|o| o.get(key))
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.as_object_mut().and_then(|o| o.get_mut(key))
    }
    /// `v[key]` when it is a string
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }
    /// `v[key]` when it is an object
    pub fn get_object(&self, key: &str) -> Option<&Object> {
        self.get(key).and_then(Value::as_object)
    }
    /// `v[key]` when it is an array
    pub fn get_array(&self, key: &str) -> Option<&Vec<Value>> {
        self.get(key).and_then(Value::as_array)
    }
    /// `v[i]` on an array
    pub fn index(&self, i: usize) -> Option<&Value> {
        self.as_array().and_then(|a| a.get(i))
    }
    /// JavaScript truthiness: false for null, false, 0, NaN and ""
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) => true,
        }
    }
    /// `String(v)`
    pub fn to_js_string(&self) -> String {
        to_js_string(self)
    }
    /// strict equality (`===`) of two scalars; collections are equal only as the same object, which a copy never is
    pub fn strict_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            _ => false,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}
impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Number(n as f64)
    }
}
impl From<usize> for Value {
    fn from(n: usize) -> Self {
        Value::Number(n as f64)
    }
}
impl From<Object> for Value {
    fn from(o: Object) -> Self {
        Value::Object(o)
    }
}
impl From<Vec<Value>> for Value {
    fn from(a: Vec<Value>) -> Self {
        Value::Array(a)
    }
}

/// `String(v)`: a list is its items joined by commas (a null item is empty), an object is `[object Object]`
pub fn to_js_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_to_string(*n),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|x| if x.is_null() { String::new() } else { to_js_string(x) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_string(),
    }
}

/// `typeof v`
pub fn typeof_name(v: &Value) -> &'static str {
    match v {
        Value::Null | Value::Array(_) | Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
    }
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            Value::Null => s.serialize_unit(),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 && !(*n == 0.0 && n.is_sign_negative()) {
                    s.serialize_i64(*n as i64)
                } else {
                    s.serialize_f64(*n)
                }
            }
            Value::String(x) => s.serialize_str(x),
            Value::Array(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for x in items {
                    seq.serialize_element(x)?;
                }
                seq.end()
            }
            Value::Object(o) => {
                let mut map = s.serialize_map(Some(o.len()))?;
                for (k, x) in o.iter() {
                    map.serialize_entry(k, x)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Value;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_unit<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_none<E>(self) -> Result<Value, E> {
                Ok(Value::Null)
            }
            fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> Result<Value, D2::Error> {
                serde::Deserialize::deserialize(d)
            }
            fn visit_bool<E>(self, b: bool) -> Result<Value, E> {
                Ok(Value::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<Value, E> {
                Ok(Value::Number(n as f64))
            }
            fn visit_u64<E>(self, n: u64) -> Result<Value, E> {
                Ok(Value::Number(n as f64))
            }
            fn visit_f64<E>(self, n: f64) -> Result<Value, E> {
                Ok(Value::Number(n))
            }
            fn visit_str<E>(self, s: &str) -> Result<Value, E> {
                Ok(Value::String(s.to_string()))
            }
            fn visit_string<E>(self, s: String) -> Result<Value, E> {
                Ok(Value::String(s))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
                let mut out = Vec::new();
                while let Some(x) = a.next_element()? {
                    out.push(x);
                }
                Ok(Value::Array(out))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
                let mut out = Object::new();
                while let Some((k, x)) = a.next_entry::<String, Value>()? {
                    out.insert(k, x);
                }
                Ok(Value::Object(out))
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_of_values() {
        assert_eq!(to_js_string(&Value::Number(1.0)), "1");
        assert_eq!(to_js_string(&Value::Number(0.5)), "0.5");
        assert_eq!(to_js_string(&Value::Array(vec![Value::Null, "a".into(), Value::Number(2.0)])), ",a,2");
        assert_eq!(to_js_string(&Value::Object(Object::new())), "[object Object]");
    }
}
