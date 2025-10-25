use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;

use crate::prim::load_prim_info;
use crate::word::load_word_info;
use crate::{TypeTag, cid, list_names_for_cid, load_object_cbor};

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

    let mut inputs = Vec::new();
    for input in &record.inputs {
        let input_cid = bytebuf_to_array(&input.cid)?;
        inputs.push(eval_node(conn, &input_cid, cache)?);
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
        "PRIM" => {
            let prim_cid_buf = record
                .payload
                .prim
                .ok_or_else(|| anyhow!("PRIM node missing primitive payload"))?;
            let prim_cid = bytebuf_to_array(&prim_cid_buf)?;
            eval_primitive(conn, &prim_cid, inputs)?
        }
        "CALL" => {
            let word_buf = record
                .payload
                .word
                .ok_or_else(|| anyhow!("CALL node missing word payload"))?;
            let word_cid = bytebuf_to_array(&word_buf)?;
            Value::I64(run_word_i64(conn, &word_cid)?)
        }
        kind => bail!("unsupported node kind `{kind}` in runner"),
    };

    cache.insert(*node_cid, value.clone());
    Ok(value)
}

fn eval_primitive(conn: &Connection, prim_cid: &[u8; 32], inputs: Vec<Value>) -> Result<Value> {
    let info = load_prim_info(conn, prim_cid)?;
    let name = list_names_for_cid(conn, "prim", prim_cid)?
        .into_iter()
        .next();

    let ints: Vec<i64> = inputs
        .into_iter()
        .map(|v| match v {
            Value::I64(n) => Ok(n),
        })
        .collect::<Result<_>>()?;

    let value = match name.as_deref() {
        Some("add_i64") => {
            require_sig(&info, &[TypeTag::I64, TypeTag::I64], &[TypeTag::I64])?;
            if ints.len() != 2 {
                bail!("add_i64 expects 2 arguments, got {}", ints.len());
            }
            Value::I64(ints[0] + ints[1])
        }
        Some("sub_i64") => {
            require_sig(&info, &[TypeTag::I64, TypeTag::I64], &[TypeTag::I64])?;
            if ints.len() != 2 {
                bail!("sub_i64 expects 2 arguments, got {}", ints.len());
            }
            Value::I64(ints[0] - ints[1])
        }
        Some(other) => bail!("primitive `{other}` not supported in runner"),
        None => bail!(
            "primitive {} not registered with a name (runner needs a symbolic name)",
            cid::to_hex(prim_cid)
        ),
    };

    Ok(value)
}

fn require_sig(
    info: &crate::prim::PrimInfo,
    params: &[TypeTag],
    results: &[TypeTag],
) -> Result<()> {
    if info.params != params || info.results != results {
        bail!(
            "primitive signature mismatch: params {:?} -> {:?}, expected {:?} -> {:?}",
            info.params,
            info.results,
            params,
            results
        );
    }
    Ok(())
}

fn bytebuf_to_array(buf: &ByteBuf) -> Result<[u8; 32]> {
    let slice = buf.as_slice();
    if slice.len() != 32 {
        bail!("expected 32-byte CID, found {} bytes", slice.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(slice);
    Ok(arr)
}
