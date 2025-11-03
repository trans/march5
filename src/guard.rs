use anyhow::{Result, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;

use crate::cbor::{push_array, push_bytes, push_text};
use crate::types::{EffectMask, TypeTag, effect_mask};
use crate::{cid, db};

#[derive(Clone, Debug)]
pub struct GuardCanon {
    pub root: [u8; 32],
    pub params: Vec<String>,
    pub results: Vec<String>,
    pub effects: Vec<[u8; 32]>,
    pub effect_mask: EffectMask,
}

pub struct GuardStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

pub fn encode(guard: &GuardCanon) -> Vec<u8> {
    let mut buf = Vec::new();
    push_array(&mut buf, 6);
    crate::cbor::push_u32(&mut buf, 7); // object tag for "guard"
    push_bytes(&mut buf, &guard.root);

    push_array(&mut buf, guard.params.len() as u64);
    for param in &guard.params {
        push_text(&mut buf, param);
    }

    push_array(&mut buf, guard.results.len() as u64);
    for result in &guard.results {
        push_text(&mut buf, result);
    }

    let mut effects = guard.effects.clone();
    effects.sort();
    push_array(&mut buf, effects.len() as u64);
    for effect in effects {
        push_bytes(&mut buf, &effect);
    }

    crate::cbor::push_u32(&mut buf, guard.effect_mask);

    buf
}

pub fn store_guard(conn: &Connection, guard: &GuardCanon) -> Result<GuardStoreOutcome> {
    let cbor = encode(guard);
    let cid = cid::compute(&cbor);
    let inserted = db::put_object(conn, &cid, "guard", &cbor)?;
    Ok(GuardStoreOutcome { cid, inserted })
}

#[derive(Clone, Debug)]
pub struct GuardInfo {
    pub root: [u8; 32],
    pub params: Vec<TypeTag>,
    pub results: Vec<TypeTag>,
    pub effects: Vec<[u8; 32]>,
    pub effect_mask: EffectMask,
}

pub fn load_guard_info(conn: &Connection, cid_bytes: &[u8; 32]) -> Result<GuardInfo> {
    let cbor = db::load_cbor_for_kind(conn, cid_bytes, "guard")?;
    let GuardRecord(tag, root_buf, params_raw, results_raw, effects_raw, mask_opt) =
        serde_cbor::from_slice(&cbor)?;
    if tag != 7 {
        bail!("object tag mismatch while loading guard: {tag}");
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
                bail!("invalid effect CID length in guard object: {}", slice.len());
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(slice);
            Ok(arr)
        })
        .collect::<Result<Vec<_>>>()?;
    let effect_mask_value = mask_opt.unwrap_or(if effects.is_empty() {
        effect_mask::NONE
    } else {
        effect_mask::IO
    });
    Ok(GuardInfo {
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
struct GuardRecord(
    u64,
    ByteBuf,
    Vec<String>,
    Vec<String>,
    Vec<ByteBuf>,
    #[serde(default)] Option<u32>,
);
