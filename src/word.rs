//! Canonical encoding and persistence for word entrypoints.

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;

use crate::cbor::{push_bytes, push_map, push_text};
use crate::types::{TypeTag, encode_type_signature};
use crate::{cid, store};

/// Structured word definition before encoding.
#[derive(Clone, Debug)]
pub struct WordCanon {
    pub root: [u8; 32],
    pub params: Vec<String>,
    pub results: Vec<String>,
    pub effects: Vec<[u8; 32]>,
}

/// Result of persisting a word object.
pub struct WordStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

/// Encode a word into canonical CBOR.
pub fn encode(word: &WordCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    let has_effects = !word.effects.is_empty();
    let map_len = if has_effects { 4 } else { 3 };
    push_map(&mut buf, map_len);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "word");

    push_text(&mut buf, "root");
    push_bytes(&mut buf, &word.root);

    push_text(&mut buf, "type");
    let param_refs: Vec<&str> = word.params.iter().map(|s| s.as_str()).collect();
    let result_refs: Vec<&str> = word.results.iter().map(|s| s.as_str()).collect();
    encode_type_signature(&mut buf, &param_refs, &result_refs);

    if has_effects {
        push_text(&mut buf, "effects");
        let mut sorted = word.effects.clone();
        sorted.sort();
        crate::cbor::push_array(&mut buf, sorted.len() as u64);
        for effect in sorted {
            crate::cbor::push_bytes(&mut buf, &effect);
        }
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
    pub params: Vec<TypeTag>,
    pub results: Vec<TypeTag>,
    pub effects: Vec<[u8; 32]>,
}

/// Load word metadata from storage.
pub fn load_word_info(conn: &Connection, cid_bytes: &[u8; 32]) -> Result<WordInfo> {
    let cbor: Vec<u8> = conn.query_row(
        "SELECT cbor FROM object WHERE cid = ?1 AND kind = 'word'",
        [cid_bytes.as_slice()],
        |row| row.get(0),
    )?;
    let record: WordRecord = serde_cbor::from_slice(&cbor)?;
    if record.kind != "word" {
        return Err(anyhow!(
            "object kind mismatch while loading word: {}",
            record.kind
        ));
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
    let effects = record
        .effects
        .unwrap_or_default()
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
    Ok(WordInfo {
        params,
        results,
        effects,
    })
}

#[derive(Deserialize)]
struct WordRecord {
    kind: String,
    #[serde(default)]
    _root: Option<ByteBuf>,
    #[serde(rename = "type")]
    ty: WordTypeRecord,
    #[serde(default)]
    effects: Option<Vec<ByteBuf>>,
}

#[derive(Deserialize)]
struct WordTypeRecord {
    params: Vec<String>,
    results: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TypeTag;

    #[test]
    fn encode_word_empty_type() {
        let word = WordCanon {
            root: [0x11; 32],
            params: Vec::new(),
            results: vec!["i64".to_string()],
            effects: Vec::new(),
        };
        let encoded = encode(&word);
        // Ensure the kind and root fields are present.
        assert!(encoded.windows(4).any(|w| w == b"word"));
        assert!(encoded.iter().filter(|&&b| b == 0x11).count() >= 32);
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
        };
        let outcome = store_word(&conn, &word)?;
        let info = load_word_info(&conn, &outcome.cid)?;
        assert_eq!(info.params, vec![TypeTag::I64]);
        assert_eq!(info.results, vec![TypeTag::I64]);
        assert_eq!(info.effects.len(), 1);
        assert_eq!(info.effects[0], [0xAA; 32]);
        Ok(())
    }
}
