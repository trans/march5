//! Canonical encoding for interface descriptors (import/export surfaces).

use anyhow::Result;
use rusqlite::Connection;

use crate::cbor::{push_array, push_bytes, push_map, push_text};
use crate::types::encode_type_signature;
use crate::{cid, store};

/// A single symbol exported by an interface.
#[derive(Clone, Debug)]
pub struct IfaceSymbol {
    pub name: String,
    pub params: Vec<String>,
    pub results: Vec<String>,
    pub effects: Vec<[u8; 32]>,
}

/// Collection of symbols that make up an interface.
#[derive(Clone, Debug)]
pub struct IfaceCanon {
    pub symbols: Vec<IfaceSymbol>,
}

/// Result of persisting an interface object.
pub struct IfaceStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

/// Encode an interface into canonical CBOR.
pub fn encode(iface: &IfaceCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    push_map(&mut buf, 2);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "iface");

    push_text(&mut buf, "names");
    encode_symbols(&mut buf, &iface.symbols);

    buf
}

/// Persist an interface object to the store.
pub fn store_iface(conn: &Connection, iface: &IfaceCanon) -> Result<IfaceStoreOutcome> {
    let cbor = encode(iface);
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "iface", &cbor)?;
    Ok(IfaceStoreOutcome { cid, inserted })
}

fn encode_symbols(buf: &mut Vec<u8>, symbols: &[IfaceSymbol]) {
    let mut sorted = symbols.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    push_array(buf, sorted.len() as u64);
    for symbol in sorted {
        let has_effects = !symbol.effects.is_empty();
        push_map(buf, if has_effects { 3 } else { 2 });

        push_text(buf, "name");
        push_text(buf, &symbol.name);

        push_text(buf, "type");
        let param_refs: Vec<&str> = symbol.params.iter().map(|s| s.as_str()).collect();
        let result_refs: Vec<&str> = symbol.results.iter().map(|s| s.as_str()).collect();
        encode_type_signature(buf, &param_refs, &result_refs);

        if has_effects {
            push_text(buf, "effects");
            let mut effects = symbol.effects.clone();
            effects.sort_by(|a, b| a.cmp(b));
            push_array(buf, effects.len() as u64);
            for effect in effects {
                push_bytes(buf, &effect);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_iface_symbols_sorted() {
        let iface = IfaceCanon {
            symbols: vec![
                IfaceSymbol {
                    name: "world".to_string(),
                    params: vec![],
                    results: vec!["unit".to_string()],
                    effects: vec![],
                },
                IfaceSymbol {
                    name: "hello".to_string(),
                    params: vec![],
                    results: vec!["unit".to_string()],
                    effects: vec![[0x11; 32]],
                },
            ],
        };
        let encoded = encode(&iface);
        // Ensure "hello" appears before "world" in encoded bytes
        let hello_pos = encoded
            .windows(b"hello".len())
            .position(|w| w == b"hello")
            .unwrap();
        let world_pos = encoded
            .windows(b"world".len())
            .position(|w| w == b"world")
            .unwrap();
        assert!(hello_pos < world_pos);
    }
}
