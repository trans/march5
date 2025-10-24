use std::cmp::Ordering;

use anyhow::Result;
use rusqlite::Connection;

use crate::cbor::{push_array, push_map, push_text};
use crate::{cid, store};

pub struct PrimCanon<'a> {
    pub params: &'a [&'a str],
    pub results: &'a [&'a str],
    pub attrs: &'a [(&'a str, &'a str)],
}

pub struct PrimStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

pub fn encode(prim: &PrimCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    let has_attrs = !prim.attrs.is_empty();
    let map_len = if has_attrs { 3 } else { 2 };
    push_map(&mut buf, map_len);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "prim");

    push_text(&mut buf, "type");
    encode_type(&mut buf, prim.params, prim.results);

    if has_attrs {
        push_text(&mut buf, "attrs");
        encode_attrs(&mut buf, prim.attrs);
    }

    buf
}

pub fn store_prim(conn: &Connection, prim: &PrimCanon) -> Result<PrimStoreOutcome> {
    let cbor = encode(prim);
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "prim", &cbor)?;
    Ok(PrimStoreOutcome { cid, inserted })
}

fn encode_type(buf: &mut Vec<u8>, params: &[&str], results: &[&str]) {
    push_map(buf, 2);

    push_text(buf, "params");
    push_array(buf, params.len() as u64);
    for p in params {
        push_text(buf, p);
    }

    push_text(buf, "results");
    push_array(buf, results.len() as u64);
    for r in results {
        push_text(buf, r);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_without_attrs() {
        let params = ["i64", "i64"];
        let results = ["i64"];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            attrs: &[],
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
        let params = ["i64"];
        let results = ["i64"];
        let attrs = [("commutative", "true"), ("category", "arith")];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            attrs: &attrs,
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
}
