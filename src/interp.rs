use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;

use crate::word::load_word_info;
use crate::{TypeTag, cid, load_object_cbor};

/// Evaluates a word that returns a single `i64` result.
pub fn run_word_i64(conn: &Connection, word_cid: &[u8; 32]) -> Result<i64> {
    let info = load_word_info(conn, word_cid)?;
    if info.params.len() != 0 {
        bail!(
            "runner currently supports zero-argument words ({} params)",
            info.params.len()
        );
    }
    if info.results.len() != 1 || info.results[0] != TypeTag::I64 {
        bail!(
            "runner expects a single i64 result, found {:?}",
            info.results
        );
    }

    let mut cache = HashMap::new();
    let value = eval_node(conn, &info.root, &mut cache)?;
    match value {
        Value::I64(n) => Ok(n),
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct NodeRecord {
    kind: String,
    #[serde(rename = "nk")]
    nk: String,
    #[serde(default)]
    #[allow(dead_code)]
    ty: String,
    #[serde(default, rename = "in")]
    inputs: Vec<NodeInputRecord>,
    #[serde(default)]
    eff: Vec<ByteBuf>,
    #[serde(rename = "pl")]
    payload: NodePayloadRecord,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct NodeInputRecord {
    cid: ByteBuf,
    port: u32,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)]
struct NodePayloadRecord {
    #[serde(default)]
    lit: Option<i64>,
    #[serde(default)]
    prim: Option<ByteBuf>,
    #[serde(default)]
    arg: Option<u32>,
    #[serde(default)]
    word: Option<ByteBuf>,
}

#[derive(Clone, Debug)]
enum Value {
    I64(i64),
}

fn eval_node(
    conn: &Connection,
    node_cid: &[u8; 32],
    cache: &mut HashMap<[u8; 32], Value>,
) -> Result<Value> {
    if let Some(value) = cache.get(node_cid) {
        return Ok(value.clone());
    }

    let (_, cbor) = load_object_cbor(conn, node_cid)?;
    let record: NodeRecord = serde_cbor::from_slice(&cbor)?;
    if record.kind != "node" {
        bail!("object {} is not a node", cid::to_hex(node_cid));
    }

    let value = match record.nk.as_str() {
        "LIT" => {
            let lit = record
                .payload
                .lit
                .ok_or_else(|| anyhow!("LIT node missing literal payload"))?;
            Value::I64(lit)
        }
        "ARG" => bail!("ARG nodes not supported in runner"),
        "PRIM" => bail!("PRIM nodes not yet supported in runner"),
        "CALL" => bail!("CALL nodes not yet supported in runner"),
        kind => bail!("unsupported node kind `{kind}` in runner"),
    };

    cache.insert(*node_cid, value.clone());
    Ok(value)
}
