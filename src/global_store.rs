use std::collections::BTreeMap;
use std::sync::RwLock;

use anyhow::{Result, anyhow, bail};
use std::convert::TryFrom;
use once_cell::sync::Lazy;
use rusqlite::Connection;
use serde_cbor::Value as CborValue;

use crate::cbor::{push_array, push_bytes, push_f64, push_i64, push_text, push_u32};
use crate::interp::Value;
use crate::{cid, store};

/// Global process-local store holding immutable values keyed by namespace-qualified names.
#[derive(Clone, Debug)]
pub struct GlobalStore {
    entries: BTreeMap<String, Value>,
}

impl GlobalStore {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<Value> {
        self.entries.get(key).cloned()
    }

    pub fn put(&mut self, key: impl Into<String>, value: Value) -> Option<Value> {
        self.entries.insert(key.into(), value)
    }

    pub fn replace(&mut self, snapshot: GlobalStoreSnapshot) {
        self.entries = snapshot.entries;
    }
}

impl Default for GlobalStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable snapshot of the global store.
#[derive(Clone, Debug, Default)]
pub struct GlobalStoreSnapshot {
    entries: BTreeMap<String, Value>,
}

impl GlobalStoreSnapshot {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter()
    }

    pub fn into_vec(self) -> Vec<(String, Value)> {
        self.entries.into_iter().collect()
    }

    pub fn from_entries(entries: BTreeMap<String, Value>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &BTreeMap<String, Value> {
        &self.entries
    }
}

static STORE: Lazy<RwLock<GlobalStore>> = Lazy::new(|| RwLock::new(GlobalStore::new()));

/// Reset the global store to an empty map.
pub fn reset() {
    let mut guard = STORE.write().expect("global store poisoned");
    *guard = GlobalStore::new();
}

/// Retrieve the current value for a key, if any.
pub fn read(key: &str) -> Option<Value> {
    let guard = STORE.read().expect("global store poisoned");
    guard.get(key)
}

/// Insert or update a value for the given key, returning the previous value if present.
pub fn write(key: impl Into<String>, value: Value) -> Option<Value> {
    let mut guard = STORE.write().expect("global store poisoned");
    guard.put(key, value)
}

/// Acquire a snapshot of the current global store.
pub fn snapshot() -> GlobalStoreSnapshot {
    let guard = STORE.read().expect("global store poisoned");
    GlobalStoreSnapshot {
        entries: guard.entries.clone(),
    }
}

/// Replace the in-memory store with a provided snapshot.
pub fn restore(snapshot: GlobalStoreSnapshot) {
    let mut guard = STORE.write().expect("global store poisoned");
    guard.replace(snapshot);
}

/// Canonically encode the snapshot for persistence.
pub fn encode_snapshot(snapshot: &GlobalStoreSnapshot) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    push_array(&mut buf, 2);
    push_u32(&mut buf, 8); // object tag for "gstate"
    push_array(&mut buf, snapshot.len() as u64);
    for (key, value) in snapshot.iter() {
        push_array(&mut buf, 2);
        push_text(&mut buf, key);
        encode_store_value(&mut buf, value)?;
    }
    Ok(buf)
}

/// Persist the snapshot in the canonical object store.
pub fn store_snapshot(conn: &Connection, snapshot: &GlobalStoreSnapshot) -> Result<GlobalStoreStoreOutcome> {
    let cbor = encode_snapshot(snapshot)?;
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "gstate", &cbor)?;
    Ok(GlobalStoreStoreOutcome { cid, inserted })
}

/// Load a persisted snapshot from the object store.
pub fn load_snapshot(conn: &Connection, cid_bytes: &[u8; 32]) -> Result<GlobalStoreSnapshot> {
    let (kind, cbor) = store::load_object_cbor(conn, cid_bytes)?;
    if kind != "gstate" {
        bail!("object {} is not a global store snapshot", cid::to_hex(cid_bytes));
    }
    let value: CborValue = serde_cbor::from_slice(&cbor)?;
    decode_snapshot(&value)
}

/// Result of persisting a snapshot object.
pub struct GlobalStoreStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

fn encode_store_value(buf: &mut Vec<u8>, value: &Value) -> Result<()> {
    match value {
        Value::I64(n) => {
            push_array(buf, 2);
            push_text(buf, "i64");
            push_i64(buf, *n);
            Ok(())
        }
        Value::F64(x) => {
            push_array(buf, 2);
            push_text(buf, "f64");
            push_f64(buf, *x);
            Ok(())
        }
        Value::Unit => {
            push_array(buf, 1);
            push_text(buf, "unit");
            Ok(())
        }
        Value::Quote(cid_bytes) => {
            push_array(buf, 2);
            push_text(buf, "quote");
            push_bytes(buf, cid_bytes);
            Ok(())
        }
        Value::Tuple(items) => {
            push_array(buf, 2);
            push_text(buf, "tuple");
            push_array(buf, items.len() as u64);
            for item in items {
                encode_store_value(buf, item)?;
            }
            Ok(())
        }
        Value::Text(s) => {
            push_array(buf, 2);
            push_text(buf, "text");
            push_text(buf, s);
            Ok(())
        }
        other => bail!("unsupported value type in global store snapshot: {:?}", other),
    }
}

fn decode_snapshot(value: &CborValue) -> Result<GlobalStoreSnapshot> {
    match value {
        CborValue::Array(items) if items.len() == 2 => {
            let tag = match &items[0] {
                CborValue::Integer(n) => *n,
                other => bail!("global store snapshot tag must be integer, found {other:?}"),
            };
            if tag != 8 {
                bail!("unexpected global store object tag {}", tag);
            }
            let entries = match &items[1] {
                CborValue::Array(entries) => entries,
                other => bail!("global store entries must be array, found {other:?}"),
            };
            let mut map = BTreeMap::new();
            for entry in entries {
                match entry {
                    CborValue::Array(pair) if pair.len() == 2 => {
                        let key = match &pair[0] {
                            CborValue::Text(s) => s.clone(),
                            other => bail!("global store key must be text, found {other:?}"),
                        };
                        let value = decode_store_value(&pair[1])?;
                        map.insert(key, value);
                    }
                    other => bail!("global store entry must be [key, value], found {other:?}"),
                }
            }
            Ok(GlobalStoreSnapshot { entries: map })
        }
        other => bail!("invalid global store snapshot object {other:?}"),
    }
}

fn decode_store_value(value: &CborValue) -> Result<Value> {
    match value {
        CborValue::Array(items) if !items.is_empty() => {
            let type_atom = match &items[0] {
                CborValue::Text(s) => s.as_str(),
                other => bail!("global store value type must be text, found {other:?}"),
            };
            match type_atom {
                "i64" => {
                    if items.len() != 2 {
                        bail!("i64 value must have payload");
                    }
                    match &items[1] {
                        CborValue::Integer(n) => {
                            let value = i64::try_from(*n).map_err(|_| anyhow!("i64 payload out of range"))?;
                            Ok(Value::I64(value))
                        }
                        other => bail!("i64 payload must be integer, found {other:?}"),
                    }
                }
                "f64" => {
                    if items.len() != 2 {
                        bail!("f64 value must have payload");
                    }
                    match &items[1] {
                        CborValue::Float(f) => Ok(Value::F64(*f)),
                        other => bail!("f64 payload must be float, found {other:?}"),
                    }
                }
                "unit" => {
                    if items.len() != 1 {
                        bail!("unit value must not have payload");
                    }
                    Ok(Value::Unit)
                }
                "quote" => {
                    if items.len() != 2 {
                        bail!("quote value must include payload");
                    }
                    match &items[1] {
                        CborValue::Bytes(bytes) => {
                            if bytes.len() != 32 {
                                bail!("quote payload must be 32 bytes, found {}", bytes.len());
                            }
                            let mut cid_bytes = [0u8; 32];
                            cid_bytes.copy_from_slice(bytes);
                            Ok(Value::Quote(cid_bytes))
                        }
                        other => bail!("quote payload must be bytes, found {other:?}"),
                    }
                }
                "tuple" => {
                    if items.len() != 2 {
                        bail!("tuple value must include payload");
                    }
                    let elements = match &items[1] {
                        CborValue::Array(values) => values,
                        other => bail!("tuple payload must be array, found {other:?}"),
                    };
                    let mut decoded = Vec::with_capacity(elements.len());
                    for element in elements {
                        decoded.push(decode_store_value(element)?);
                    }
                    Ok(Value::Tuple(decoded))
                }
                "text" => {
                    if items.len() != 2 {
                        bail!("text value must include payload");
                    }
                    match &items[1] {
                        CborValue::Text(s) => Ok(Value::Text(s.clone())),
                        other => bail!("text payload must be UTF-8 string, found {other:?}"),
                    }
                }
                other => bail!("unsupported global store value type `{other}`"),
            }
        }
        other => bail!("global store value must be array, found {other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store;
    use std::collections::BTreeMap;

    #[test]
    fn snapshot_roundtrip() -> Result<()> {
        reset();
        write("demo/item", Value::I64(123));
        write("demo/float", Value::F64(1.5));
        write(
            "demo/tuple",
            Value::Tuple(vec![Value::I64(1), Value::F64(2.5)]),
        );
        let quote_cid = [0xAB; 32];
        write("demo/quote", Value::Quote(quote_cid));
        write("demo/text", Value::Text("hello".to_string()));
        let snapshot = snapshot();
        let cbor = encode_snapshot(&snapshot)?;
        let value: CborValue = serde_cbor::from_slice(&cbor)?;
        let decoded = decode_snapshot(&value)?;
        assert_eq!(decoded.len(), 5);
        let map: BTreeMap<_, _> = decoded.into_vec().into_iter().collect();
        assert_eq!(map.get("demo/item"), Some(&Value::I64(123)));
        assert_eq!(map.get("demo/float"), Some(&Value::F64(1.5)));
        assert_eq!(
            map.get("demo/tuple"),
            Some(&Value::Tuple(vec![Value::I64(1), Value::F64(2.5)]))
        );
        assert_eq!(map.get("demo/quote"), Some(&Value::Quote(quote_cid)));
        assert_eq!(map.get("demo/text"), Some(&Value::Text("hello".to_string())));
        Ok(())
    }

    #[test]
    fn store_and_load_snapshot_from_db() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;
        reset();
        write("demo.item", Value::I64(7));
        write("demo.float", Value::F64(2.5));
        write("demo.tuple", Value::Tuple(vec![Value::I64(4), Value::I64(5)]));
        let quote_cid = [0x11; 32];
        write("demo.quote", Value::Quote(quote_cid));
        write("demo.text", Value::Text("store".to_string()));
        let snapshot = snapshot();
        let outcome = store_snapshot(&conn, &snapshot)?;
        let loaded = load_snapshot(&conn, &outcome.cid)?;
        restore(loaded);
        let value = read("demo.item").expect("loaded value");
        assert_eq!(value, Value::I64(7));
        assert_eq!(read("demo.float"), Some(Value::F64(2.5)));
        assert_eq!(
            read("demo.tuple"),
            Some(Value::Tuple(vec![Value::I64(4), Value::I64(5)]))
        );
        assert_eq!(read("demo.quote"), Some(Value::Quote(quote_cid)));
        assert_eq!(read("demo.text"), Some(Value::Text("store".to_string())));
        Ok(())
    }
}
