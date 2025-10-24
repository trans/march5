//! Canonical encoding and persistence for word entrypoints.

use anyhow::Result;
use rusqlite::Connection;

use crate::cbor::{push_bytes, push_map, push_text};
use crate::types::encode_type_signature;
use crate::{cid, store};

/// Structured word definition before encoding.
#[derive(Clone, Debug)]
pub struct WordCanon {
    pub root: [u8; 32],
    pub params: Vec<String>,
    pub results: Vec<String>,
}

/// Result of persisting a word object.
pub struct WordStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

/// Encode a word into canonical CBOR.
pub fn encode(word: &WordCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    push_map(&mut buf, 3);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "word");

    push_text(&mut buf, "root");
    push_bytes(&mut buf, &word.root);

    push_text(&mut buf, "type");
    let param_refs: Vec<&str> = word.params.iter().map(|s| s.as_str()).collect();
    let result_refs: Vec<&str> = word.results.iter().map(|s| s.as_str()).collect();
    encode_type_signature(&mut buf, &param_refs, &result_refs);

    buf
}

/// Persist a word in the object store.
pub fn store_word(conn: &Connection, word: &WordCanon) -> Result<WordStoreOutcome> {
    let cbor = encode(word);
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "word", &cbor)?;
    Ok(WordStoreOutcome { cid, inserted })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_word_empty_type() {
        let word = WordCanon {
            root: [0x11; 32],
            params: Vec::new(),
            results: vec!["i64".to_string()],
        };
        let encoded = encode(&word);
        // Ensure the kind and root fields are present.
        assert!(encoded.windows(4).any(|w| w == b"word"));
        assert!(encoded.iter().filter(|&&b| b == 0x11).count() >= 32);
    }
}
