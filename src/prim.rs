//! Canonical encoding, storage, and metadata helpers for primitive descriptors.

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;

use crate::cbor::{push_array, push_bytes, push_text};
use crate::types::{EffectMask, TypeTag, effect_mask};
use crate::{cid, store};

/// Structured representation of a primitive prior to encoding.
pub struct PrimCanon<'a> {
    /// Parameter type tags in call order.
    pub params: &'a [TypeTag],
    /// Result type tags in return order.
    pub results: &'a [TypeTag],
    /// Declared effect CIDs (unordered; will be sorted during encoding).
    pub effects: &'a [[u8; 32]],
    /// Bitmask describing which effect domains this primitive touches.
    pub effect_mask: EffectMask,
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
    pub effect_mask: EffectMask,
}

/// Encode a primitive into canonical CBOR.
pub fn encode(prim: &PrimCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    // [tag, rootCID, params[], results[], effects[], mask]
    push_array(&mut buf, 6);
    crate::cbor::push_u32(&mut buf, 0); // object tag for "prim"
    push_bytes(&mut buf, &[0u8; 32]); // reserved root slot (always zero for prims)

    push_array(&mut buf, prim.params.len() as u64);
    for tag in prim.params {
        push_text(&mut buf, tag.as_atom());
    }

    push_array(&mut buf, prim.results.len() as u64);
    for tag in prim.results {
        push_text(&mut buf, tag.as_atom());
    }

    encode_effects(&mut buf, prim.effects);
    crate::cbor::push_u32(&mut buf, prim.effect_mask);
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
    let PrimRecord(tag, root, params_raw, results_raw, effects_raw, mask_opt) =
        serde_cbor::from_slice(&cbor).with_context(|| "failed to decode primitive CBOR payload")?;
    if tag != 0 {
        bail!("object tag mismatch while loading prim: {}", tag);
    }
    let root_slice = root.as_ref();
    if !root_slice.is_empty() && root_slice.len() != 32 {
        bail!(
            "invalid prim root length {}; expected 0 or 32",
            root_slice.len()
        );
    }
    let params = params_raw
        .iter()
        .map(|s| TypeTag::from_atom(s))
        .collect::<Result<Vec<_>>>()?;
    let results = results_raw
        .iter()
        .map(|s| TypeTag::from_atom(s))
        .collect::<Result<Vec<_>>>()?;
    let effects = effects_raw
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
    let effect_mask_value = mask_opt.unwrap_or(if effects.is_empty() {
        effect_mask::NONE
    } else {
        effect_mask::IO
    });

    Ok(PrimInfo {
        params,
        results,
        effects,
        effect_mask: effect_mask_value,
    })
}

fn encode_effects(buf: &mut Vec<u8>, effects: &[[u8; 32]]) {
    let mut sorted = effects.to_vec();
    sorted.sort();
    push_array(buf, sorted.len() as u64);
    for effect in sorted {
        push_bytes(buf, &effect);
    }
}

#[derive(Deserialize)]
struct PrimRecord(
    u64,
    ByteBuf,
    Vec<String>,
    Vec<String>,
    Vec<ByteBuf>,
    #[serde(default)] Option<u32>,
);

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn encode_layout() {
        let params = [TypeTag::I64, TypeTag::I64];
        let results = [TypeTag::I64];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            effects: &[],
            effect_mask: effect_mask::NONE,
        };
        let encoded = encode(&prim);
        let value: serde_cbor::Value =
            serde_cbor::from_slice(&encoded).expect("valid CBOR encoding");
        match value {
            serde_cbor::Value::Array(items) => {
                assert_eq!(items.len(), 6);
                assert_eq!(items[0], serde_cbor::Value::Integer(0));
                match &items[1] {
                    serde_cbor::Value::Bytes(bytes) => {
                        assert_eq!(bytes.len(), 32);
                        assert!(bytes.iter().all(|b| *b == 0));
                    }
                    other => panic!("expected bytes for root slot, got {other:?}"),
                }
                assert!(matches!(items[4], serde_cbor::Value::Array(ref arr) if arr.is_empty()));
                assert_eq!(items[5], serde_cbor::Value::Integer(0));
            }
            other => panic!("expected array encoding, got {other:?}"),
        }
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
            effects: &[],
            effect_mask: effect_mask::NONE,
        };
        let outcome = store_prim(&conn, &prim)?;
        let info = load_prim_info(&conn, &outcome.cid)?;
        assert_eq!(info.params, params);
        assert_eq!(info.results, results);
        assert!(info.effects.is_empty());
        assert_eq!(info.effect_mask, effect_mask::NONE);
        Ok(())
    }

    #[test]
    fn roundtrip_effects() -> Result<()> {
        let params = [TypeTag::I64];
        let results = [TypeTag::I64];
        let effects = [[0x11; 32], [0x22; 32]];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            effects: &effects,
            effect_mask: effect_mask::STATE_WRITE,
        };
        let conn = Connection::open_in_memory()?;
        crate::store::install_schema(&conn)?;
        let outcome = store_prim(&conn, &prim)?;
        let info = load_prim_info(&conn, &outcome.cid)?;
        assert_eq!(info.effects, effects);
        assert_eq!(info.effect_mask, effect_mask::STATE_WRITE);
        Ok(())
    }
}
