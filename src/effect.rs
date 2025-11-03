//! Canonical encoding and storage helpers for effect descriptors.

use anyhow::Result;
use rusqlite::Connection;

use crate::cbor::{push_map, push_text};
use crate::{cid, db};

/// Shape of an effect descriptor before encoding.
pub struct EffectCanon<'a> {
    /// Human readable effect name (e.g. `"io"`).
    pub name: &'a str,
    /// Optional documentation string; excluded from the CID if omitted.
    pub doc: Option<&'a str>,
}

/// Result of attempting to persist an effect.
pub struct EffectStoreOutcome {
    /// CID of the encoded effect.
    pub cid: [u8; 32],
    /// True when a new row was inserted, false when it already existed.
    pub inserted: bool,
}

/// Encode an effect descriptor into canonical CBOR.
pub fn encode(effect: &EffectCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    let map_len = if effect.doc.is_some() { 3 } else { 2 };
    push_map(&mut buf, map_len);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "effect");

    push_text(&mut buf, "name");
    push_text(&mut buf, effect.name);

    if let Some(doc) = effect.doc {
        push_text(&mut buf, "doc");
        push_text(&mut buf, doc);
    }

    buf
}

/// Persist an effect descriptor into the object store.
pub fn store_effect(conn: &Connection, effect: &EffectCanon) -> Result<EffectStoreOutcome> {
    let cbor = encode(effect);
    let cid = cid::compute(&cbor);
    let inserted = db::put_object(conn, &cid, "effect", &cbor)?;
    Ok(EffectStoreOutcome { cid, inserted })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_without_doc() {
        let effect = EffectCanon {
            name: "io",
            doc: None,
        };
        let encoded = encode(&effect);
        assert_eq!(
            encoded,
            vec![
                0xA2, // map(2)
                0x64, b'k', b'i', b'n', b'd', 0x66, b'e', b'f', b'f', b'e', b'c', b't', 0x64, b'n',
                b'a', b'm', b'e', 0x62, b'i', b'o',
            ]
        );
    }

    #[test]
    fn encode_with_doc() {
        let effect = EffectCanon {
            name: "io",
            doc: Some("side effects"),
        };
        let encoded = encode(&effect);
        assert_eq!(
            encoded,
            vec![
                0xA3, // map(3)
                0x64, b'k', b'i', b'n', b'd', 0x66, b'e', b'f', b'f', b'e', b'c', b't', 0x64, b'n',
                b'a', b'm', b'e', 0x62, b'i', b'o', 0x63, b'd', b'o', b'c', 0x6C, b's', b'i', b'd',
                b'e', b' ', b'e', b'f', b'f', b'e', b'c', b't', b's',
            ]
        );
    }

    #[test]
    fn encode_with_long_doc() {
        let long_doc = "long documentation string beyond twenty three bytes";
        let effect = EffectCanon {
            name: "io",
            doc: Some(long_doc),
        };
        let encoded = encode(&effect);
        assert_eq!(encoded[0], 0xA3); // map(3)
        let doc_key = [0x63, b'd', b'o', b'c'];
        let pos = encoded
            .windows(doc_key.len())
            .position(|window| window == doc_key)
            .expect("doc key present");
        assert_eq!(encoded[pos + doc_key.len()], 0x78);
        assert_eq!(encoded[pos + doc_key.len() + 1] as usize, long_doc.len());
    }
}
