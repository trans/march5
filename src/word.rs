//! Canonical encoding and persistence for word entrypoints.

use anyhow::{Result, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;

use crate::cbor::{push_array, push_bytes, push_text};
use crate::types::{EffectMask, TypeTag, effect_mask};
use crate::{cid, store};

/// Structured word definition before encoding.
#[derive(Clone, Debug)]
pub struct WordCanon {
    pub root: [u8; 32],
    pub params: Vec<String>,
    pub results: Vec<String>,
    pub effects: Vec<[u8; 32]>,
    pub effect_mask: EffectMask,
}

/// Result of persisting a word object.
pub struct WordStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

/// Encode a word into canonical CBOR.
pub fn encode(word: &WordCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    push_array(&mut buf, 5);
    crate::cbor::push_u32(&mut buf, 1); // object tag for "word"
    push_bytes(&mut buf, &word.root);

    push_array(&mut buf, word.params.len() as u64);
    for param in &word.params {
        push_text(&mut buf, param);
    }

    push_array(&mut buf, word.results.len() as u64);
    for result in &word.results {
        push_text(&mut buf, result);
    }

    let mut sorted_effects = word.effects.clone();
    sorted_effects.sort();
    push_array(&mut buf, sorted_effects.len() as u64);
    for effect in sorted_effects {
        push_bytes(&mut buf, &effect);
    }

    buf
}

/// Persist a word in the object store.
pub fn store_word(conn: &Connection, word: &WordCanon) -> Result<WordStoreOutcome> {
    let cbor = encode(word);
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "word", &cbor)?;
    Ok(WordStoreOutcome { cid, inserted })
}

/// Convenience metadata for word invocations.
#[derive(Clone, Debug)]
pub struct WordInfo {
    pub root: [u8; 32],
    pub params: Vec<TypeTag>,
    pub results: Vec<TypeTag>,
    pub effects: Vec<[u8; 32]>,
    pub effect_mask: EffectMask,
}

/// Load word metadata from storage.
pub fn load_word_info(conn: &Connection, cid_bytes: &[u8; 32]) -> Result<WordInfo> {
    let cbor: Vec<u8> = conn.query_row(
        "SELECT cbor FROM object WHERE cid = ?1 AND kind = 'word'",
        [cid_bytes.as_slice()],
        |row| row.get(0),
    )?;
    let WordRecord(tag, root_buf, params_raw, results_raw, effects_raw) =
        serde_cbor::from_slice(&cbor)?;
    if tag != 1 {
        bail!("object tag mismatch while loading word: {tag}");
    }
    let root = bytebuf_to_array(&root_buf)?;
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
                bail!("invalid effect CID length in word object: {}", slice.len());
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(slice);
            Ok(arr)
        })
        .collect::<Result<Vec<_>>>()?;
    let effect_mask_value = if effects.is_empty() {
        effect_mask::NONE
    } else {
        effect_mask::IO
    };
    Ok(WordInfo {
        root,
        params,
        results,
        effects,
        effect_mask: effect_mask_value,
    })
}

fn bytebuf_to_array(buf: &ByteBuf) -> Result<[u8; 32]> {
    let slice = buf.as_slice();
    if slice.len() != 32 {
        bail!("invalid CID length {}; expected 32", slice.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(slice);
    Ok(out)
}

#[derive(Deserialize)]
struct WordRecord(u64, ByteBuf, Vec<String>, Vec<String>, Vec<ByteBuf>);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TypeTag, effect_mask};

    #[test]
    fn encode_word_empty_type() {
        let word = WordCanon {
            root: [0x11; 32],
            params: Vec::new(),
            results: vec!["i64".to_string()],
            effects: Vec::new(),
            effect_mask: effect_mask::NONE,
        };
        let encoded = encode(&word);
        let value: serde_cbor::Value =
            serde_cbor::from_slice(&encoded).expect("valid CBOR encoding");
        match value {
            serde_cbor::Value::Array(items) => {
                assert_eq!(items.len(), 5);
                assert_eq!(items[0], serde_cbor::Value::Integer(1));
                assert!(matches!(items[1], serde_cbor::Value::Bytes(_)));
            }
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn roundtrip_word_info() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        crate::store::install_schema(&conn)?;

        let word = WordCanon {
            root: [0x22; 32],
            params: vec!["i64".to_string()],
            results: vec!["i64".to_string()],
            effects: vec![[0xAA; 32]],
            effect_mask: effect_mask::IO,
        };
        let outcome = store_word(&conn, &word)?;
        let info = load_word_info(&conn, &outcome.cid)?;
        assert_eq!(info.root, [0x22; 32]);
        assert_eq!(info.params, vec![TypeTag::I64]);
        assert_eq!(info.results, vec![TypeTag::I64]);
        assert_eq!(info.effects.len(), 1);
        assert_eq!(info.effects[0], [0xAA; 32]);
        assert_eq!(info.effect_mask, effect_mask::IO);
        Ok(())
    }
}
