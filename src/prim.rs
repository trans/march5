//! Canonical encoding, storage, and metadata helpers for primitive descriptors.

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;
use std::cmp::Ordering;

use crate::cbor::{push_map, push_text};
use crate::types::{TypeTag, encode_type_signature};
use crate::{cid, store};

/// Structured representation of a primitive prior to encoding.
pub struct PrimCanon<'a> {
    /// Parameter type tags in call order.
    pub params: &'a [TypeTag],
    /// Result type tags in return order.
    pub results: &'a [TypeTag],
    /// Optional key/value attribute pairs (sorted before encoding).
    pub attrs: &'a [(&'a str, &'a str)],
    /// Declared effect CIDs (unordered; will be sorted during encoding).
    pub effects: &'a [[u8; 32]],
}

/// Result of persisting a primitive descriptor.
pub struct PrimStoreOutcome {
    /// CID of the canonical primitive object.
    pub cid: [u8; 32],
    /// True when a new row was inserted, false if it already existed.
    pub inserted: bool,
}

/// Convenience metadata used by the graph builder.
#[derive(Clone, Debug)]
pub struct PrimInfo {
    pub params: Vec<TypeTag>,
    pub results: Vec<TypeTag>,
    pub effects: Vec<[u8; 32]>,
}

/// Encode a primitive into canonical CBOR.
pub fn encode(prim: &PrimCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    let has_attrs = !prim.attrs.is_empty();
    let has_effects = !prim.effects.is_empty();
    let mut map_len = 2;
    if has_attrs {
        map_len += 1;
    }
    if has_effects {
        map_len += 1;
    }
    push_map(&mut buf, map_len);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "prim");

    push_text(&mut buf, "type");
    let params: Vec<&str> = prim.params.iter().map(|tag| tag.as_atom()).collect();
    let results: Vec<&str> = prim.results.iter().map(|tag| tag.as_atom()).collect();
    encode_type_signature(&mut buf, &params, &results);

    if has_attrs {
        push_text(&mut buf, "attrs");
        encode_attrs(&mut buf, prim.attrs);
    }

    if has_effects {
        push_text(&mut buf, "effects");
        encode_effects(&mut buf, prim.effects);
    }

    buf
}

/// Persist a primitive into the object store.
pub fn store_prim(conn: &Connection, prim: &PrimCanon) -> Result<PrimStoreOutcome> {
    let cbor = encode(prim);
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "prim", &cbor)?;
    Ok(PrimStoreOutcome { cid, inserted })
}

/// Load primitive metadata required by the graph builder.
pub fn load_prim_info(conn: &Connection, cid_bytes: &[u8; 32]) -> Result<PrimInfo> {
    let cbor: Vec<u8> = conn.query_row(
        "SELECT cbor FROM object WHERE cid = ?1 AND kind = 'prim'",
        [cid_bytes.as_slice()],
        |row| row.get(0),
    )?;
    let record: PrimRecord =
        serde_cbor::from_slice(&cbor).with_context(|| "failed to decode primitive CBOR payload")?;
    if record.kind != "prim" {
        bail!("object kind mismatch while loading prim: {}", record.kind);
    }
    let params = record
        .ty
        .params
        .iter()
        .map(|s| TypeTag::from_atom(s))
        .collect::<Result<Vec<_>>>()?;
    let results = record
        .ty
        .results
        .iter()
        .map(|s| TypeTag::from_atom(s))
        .collect::<Result<Vec<_>>>()?;
    // TODO: surface declared effects once encoded.
    let effects = record
        .effects
        .unwrap_or_default()
        .into_iter()
        .map(|bytes| {
            let slice = bytes.as_ref();
            if slice.len() != 32 {
                bail!("invalid effect CID length in prim object: {}", slice.len());
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(slice);
            Ok(arr)
        })
        .collect::<Result<Vec<_>>>()
        .with_context(|| "failed to parse prim effects")?;

    Ok(PrimInfo {
        params,
        results,
        effects,
    })
}

fn encode_attrs(buf: &mut Vec<u8>, attrs: &[(&str, &str)]) {
    let mut sorted: Vec<_> = attrs.iter().collect();
    sorted.sort_by(|a, b| match a.0.cmp(b.0) {
        Ordering::Equal => a.1.cmp(b.1),
        other => other,
    });

    push_map(buf, sorted.len() as u64);
    for (key, value) in sorted.into_iter().map(|pair| (pair.0, pair.1)) {
        push_text(buf, key);
        push_text(buf, value);
    }
}

fn encode_effects(buf: &mut Vec<u8>, effects: &[[u8; 32]]) {
    let mut sorted = effects.to_vec();
    sorted.sort();
    crate::cbor::push_array(buf, sorted.len() as u64);
    for effect in sorted {
        crate::cbor::push_bytes(buf, &effect);
    }
}

#[derive(Deserialize)]
struct PrimRecord {
    kind: String,
    #[serde(rename = "type")]
    ty: PrimTypeRecord,
    #[serde(default)]
    effects: Option<Vec<ByteBuf>>,
}

#[derive(Deserialize)]
struct PrimTypeRecord {
    params: Vec<String>,
    results: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn encode_without_attrs() {
        let params = [TypeTag::I64, TypeTag::I64];
        let results = [TypeTag::I64];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            attrs: &[],
            effects: &[],
        };
        let encoded = encode(&prim);
        assert_eq!(
            encoded,
            vec![
                0xA2, // map(2)
                0x64, b'k', b'i', b'n', b'd', 0x64, b'p', b'r', b'i', b'm', 0x64, b't', b'y', b'p',
                b'e', 0xA2, // map(2) for type
                0x66, b'p', b'a', b'r', b'a', b'm', b's', 0x82, // array(2)
                0x63, b'i', b'6', b'4', 0x63, b'i', b'6', b'4', 0x67, b'r', b'e', b's', b'u', b'l',
                b't', b's', 0x81, // array(1)
                0x63, b'i', b'6', b'4',
            ]
        );
    }

    #[test]
    fn encode_with_attrs_sorted() {
        let params = [TypeTag::I64];
        let results = [TypeTag::I64];
        let attrs = [("commutative", "true"), ("category", "arith")];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            attrs: &attrs,
            effects: &[],
        };
        let encoded = encode(&prim);
        // The attrs map must be sorted lexicographically by key, then value.
        let category_pos = encoded
            .windows(b"category".len())
            .position(|w| w == b"category")
            .expect("category attr present");
        let commutative_pos = encoded
            .windows(b"commutative".len())
            .position(|w| w == b"commutative")
            .expect("commutative attr present");
        assert!(
            category_pos < commutative_pos,
            "attrs not sorted: {encoded:?}"
        );
    }

    #[test]
    fn roundtrip_prim_info() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        crate::store::install_schema(&conn)?;

        let params = [TypeTag::I64, TypeTag::I64];
        let results = [TypeTag::I64];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            attrs: &[],
            effects: &[],
        };
        let outcome = store_prim(&conn, &prim)?;
        let info = load_prim_info(&conn, &outcome.cid)?;
        assert_eq!(info.params, params);
        assert_eq!(info.results, results);
        Ok(())
    }
}
