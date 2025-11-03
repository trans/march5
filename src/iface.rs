//! Canonical encoding for interface descriptors (import/export surfaces).

use anyhow::Result;
use rusqlite::Connection;

use crate::cbor::{push_array, push_bytes, push_text};
use crate::{cid, db};

/// A single symbol exported by an interface.
#[derive(Clone, Debug)]
pub struct IfaceSymbol {
    pub name: String,
    pub params: Vec<String>,
    pub results: Vec<String>,
    pub effects: Vec<[u8; 32]>,
}

/// Canonical list of named exports that make up an interface.
#[derive(Clone, Debug)]
pub struct IfaceCanon {
    pub names: Vec<IfaceSymbol>,
}

/// Result of persisting an interface object.
pub struct IfaceStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

/// Encode an interface into canonical CBOR.
pub fn encode(iface: &IfaceCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    push_array(&mut buf, 2);
    crate::cbor::push_u32(&mut buf, 3); // object tag for "iface"
    encode_names(&mut buf, &iface.names);
    buf
}

/// Persist an interface object to the store.
pub fn store_iface(conn: &Connection, iface: &IfaceCanon) -> Result<IfaceStoreOutcome> {
    let cbor = encode(iface);
    let cid = cid::compute(&cbor);
    let inserted = db::put_object(conn, &cid, "iface", &cbor)?;
    Ok(IfaceStoreOutcome { cid, inserted })
}

/// Derive an interface from exported word CIDs (name, wordCID).
pub fn derive_from_exports(
    conn: &Connection,
    exports: &[(String, [u8; 32])],
) -> Result<IfaceCanon> {
    let mut names = Vec::with_capacity(exports.len());
    for (name, word_cid) in exports {
        let info = crate::word::load_word_info(conn, word_cid)?;
        let params = info
            .params
            .iter()
            .map(|t| t.as_atom().to_string())
            .collect();
        let results = info
            .results
            .iter()
            .map(|t| t.as_atom().to_string())
            .collect();
        names.push(IfaceSymbol {
            name: name.clone(),
            params,
            results,
            effects: info.effects.clone(),
        });
    }
    Ok(IfaceCanon { names })
}

fn encode_names(buf: &mut Vec<u8>, names: &[IfaceSymbol]) {
    let mut sorted = names.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    push_array(buf, sorted.len() as u64);
    for symbol in sorted {
        push_text(buf, &symbol.name);

        push_array(buf, symbol.params.len() as u64);
        for param in &symbol.params {
            push_text(buf, param);
        }

        push_array(buf, symbol.results.len() as u64);
        for result in &symbol.results {
            push_text(buf, result);
        }

        let mut effects = symbol.effects.clone();
        effects.sort();
        push_array(buf, effects.len() as u64);
        for effect in effects {
            push_bytes(buf, &effect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::types::{TypeTag, effect_mask};
    use crate::word::{WordCanon, store_word};

    #[test]
    fn encode_iface_names_sorted() {
        let iface = IfaceCanon {
            names: vec![
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

    #[test]
    fn derive_interface_from_words() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        db::install_schema(&conn)?;
        let effects = vec![[0xAA; 32]];
        let word = WordCanon {
            root: [0x44; 32],
            params: vec![TypeTag::I64.as_atom().to_string()],
            results: vec![TypeTag::I64.as_atom().to_string()],
            effects: effects.clone(),
            effect_mask: effect_mask::IO,
            guards: Vec::new(),
        };
        let outcome = store_word(&conn, &word)?;
        let iface = derive_from_exports(&conn, &[("add".into(), outcome.cid)])?;
        assert_eq!(iface.names.len(), 1);
        assert_eq!(iface.names[0].name, "add");
        assert_eq!(iface.names[0].effects, effects);
        Ok(())
    }
}
