use indexmap::IndexMap;

use super::value::Value;

/// An object's own string keys in JavaScript's order: the keys that are array indices first, in ascending numeric
/// order, then the others in the order they were added. A key added again keeps its place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Object {
    map: IndexMap<String, Value>,
    indices: usize, // how many of the first keys are array indices
}

/// whether a key is an array index: the canonical decimal form of an integer from 0 to 2^32 - 2
pub fn array_index(key: &str) -> Option<u32> {
    if key.is_empty()
        || key.len() > 10
        || !key.bytes().all(|b| b.is_ascii_digit())
        || (key.len() > 1 && key.starts_with('0'))
    {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    if n < u64::from(u32::MAX) { Some(n as u32) } else { None }
}

impl Object {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.map.get_mut(key)
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }
    /// `o[key] = value`
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        if let Some(slot) = self.map.get_mut(&key) {
            *slot = value;
            return;
        }
        match array_index(&key) {
            Some(n) => {
                let at = self
                    .map
                    .keys()
                    .take(self.indices)
                    .position(|k| array_index(k).is_some_and(|m| m > n))
                    .unwrap_or(self.indices);
                self.map.shift_insert(at, key, value);
                self.indices += 1;
            }
            None => {
                self.map.insert(key, value);
            }
        }
    }
    /// `delete o[key]`
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let at = self.map.get_index_of(key)?;
        if at < self.indices {
            self.indices -= 1;
        }
        self.map.shift_remove(key)
    }
    /// `Object.entries(o)`
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&String, &Value)> + ExactSizeIterator {
        self.map.iter()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut Value)> {
        self.map.iter_mut()
    }
    /// `Object.keys(o)`
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &String> + ExactSizeIterator {
        self.map.keys()
    }
    /// `Object.values(o)`
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Value> + ExactSizeIterator {
        self.map.values()
    }
    pub fn get_index(&self, i: usize) -> Option<(&String, &Value)> {
        self.map.get_index(i)
    }
    /// the key's place among the keys
    pub fn index_of(&self, key: &str) -> Option<usize> {
        self.map.get_index_of(key)
    }
}

impl<K: Into<String>> FromIterator<(K, Value)> for Object {
    fn from_iter<I: IntoIterator<Item = (K, Value)>>(iter: I) -> Self {
        let mut o = Object::new();
        for (k, v) in iter {
            o.insert(k, v);
        }
        o
    }
}

impl IntoIterator for Object {
    type Item = (String, Value);
    type IntoIter = indexmap::map::IntoIter<String, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a> IntoIterator for &'a Object {
    type Item = (&'a String, &'a Value);
    type IntoIter = indexmap::map::Iter<'a, String, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_keys_come_first_in_numeric_order() {
        let mut o = Object::new();
        for k in ["b", "10", "a", "2", "01", "4294967295", "0"] {
            o.insert(k, Value::Null);
        }
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        assert_eq!(keys, ["0", "2", "10", "b", "a", "01", "4294967295"]);
        o.remove("2");
        o.insert("1", Value::Null);
        let keys: Vec<&str> = o.keys().map(String::as_str).collect();
        assert_eq!(keys, ["0", "1", "10", "b", "a", "01", "4294967295"]);
    }
}
