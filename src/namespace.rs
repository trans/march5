//! Canonical encoding for namespace descriptors tying interfaces to words.

use anyhow::Result;
use rusqlite::Connection;

use crate::cbor::{push_array, push_bytes, push_map, push_text};
use crate::{cid, store};

/// Structured namespace before encoding.
#[derive(Clone, Debug)]
pub struct NamespaceCanon {
    pub imports: Vec<[u8; 32]>,
    pub exports: Vec<NamespaceExport>,
    pub iface: [u8; 32],
}

#[derive(Clone, Debug)]
pub struct NamespaceExport {
    pub name: String,
    pub word: [u8; 32],
}

/// Result of persisting a namespace object.
pub struct NamespaceStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

/// Encode a namespace into canonical CBOR.
pub fn encode(ns: &NamespaceCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    push_map(&mut buf, 4);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "namespace");

    push_text(&mut buf, "imports");
    encode_cid_list(&mut buf, &ns.imports);

    push_text(&mut buf, "exports");
    encode_exports(&mut buf, &ns.exports);

    push_text(&mut buf, "iface");
    push_bytes(&mut buf, &ns.iface);

    buf
}

/// Persist a namespace in the object store.
pub fn store_namespace(conn: &Connection, ns: &NamespaceCanon) -> Result<NamespaceStoreOutcome> {
    let cbor = encode(ns);
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "namespace", &cbor)?;
    Ok(NamespaceStoreOutcome { cid, inserted })
}

fn encode_cid_list(buf: &mut Vec<u8>, cids: &[[u8; 32]]) {
    let mut sorted = cids.to_vec();
    sorted.sort();
    push_array(buf, sorted.len() as u64);
    for cid in sorted {
        push_bytes(buf, &cid);
    }
}

fn encode_exports(buf: &mut Vec<u8>, exports: &[NamespaceExport]) {
    let mut sorted = exports.to_vec();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    push_array(buf, sorted.len() as u64);
    for export in sorted {
        push_map(buf, 2);
        push_text(buf, "name");
        push_text(buf, &export.name);
        push_text(buf, "word");
        push_bytes(buf, &export.word);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_namespace_sorts_lists() {
        let ns = NamespaceCanon {
            imports: vec![[0x02; 32], [0x01; 32]],
            exports: vec![
                NamespaceExport {
                    name: "zeta".into(),
                    word: [0x22; 32],
                },
                NamespaceExport {
                    name: "alpha".into(),
                    word: [0x11; 32],
                },
            ],
            iface: [0xFF; 32],
        };
        let encoded = encode(&ns);
        // ensure sorted ordering by checking first occurrence of 0x01 before 0x02
        let first_import = encoded
            .windows(32)
            .position(|w| w.iter().all(|byte| *byte == 0x01))
            .unwrap();
        let second_import = encoded
            .windows(32)
            .position(|w| w.iter().all(|byte| *byte == 0x02))
            .unwrap();
        assert!(first_import < second_import);
        assert!(encoded.iter().filter(|&&b| b == 0xFF).count() >= 32);
        let alpha_pos = encoded
            .windows(b"alpha".len())
            .position(|w| w == b"alpha")
            .unwrap();
        let zeta_pos = encoded
            .windows(b"zeta".len())
            .position(|w| w == b"zeta")
            .unwrap();
        assert!(alpha_pos < zeta_pos);
    }
}
