use anyhow::Result;
use rusqlite::Connection;

use crate::cid;
use crate::store;

pub struct EffectCanon<'a> {
    pub name: &'a str,
    pub doc: Option<&'a str>,
}

pub struct EffectStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

pub fn encode(effect: &EffectCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    let map_len = if effect.doc.is_some() { 3 } else { 2 };
    push_header(&mut buf, 5, map_len);

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

pub fn store_effect(conn: &Connection, effect: &EffectCanon) -> Result<EffectStoreOutcome> {
    let cbor = encode(effect);
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "effect", &cbor)?;
    Ok(EffectStoreOutcome { cid, inserted })
}

fn push_text(buf: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    push_header(buf, 3, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

fn push_header(buf: &mut Vec<u8>, major: u8, len: u64) {
    assert!(major < 8);
    match len {
        0..=23 => buf.push((major << 5) | (len as u8)),
        24..=0xff => {
            buf.push((major << 5) | 24);
            buf.push(len as u8);
        }
        0x100..=0xffff => {
            buf.push((major << 5) | 25);
            buf.extend_from_slice(&(len as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            buf.push((major << 5) | 26);
            buf.extend_from_slice(&(len as u32).to_be_bytes());
        }
        _ => {
            buf.push((major << 5) | 27);
            buf.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
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
