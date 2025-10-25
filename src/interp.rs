use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;

use crate::exec::{compiled_add, compiled_sub};
use crate::prim::load_prim_info;
use crate::word::load_word_info;
use crate::{TypeTag, cid, list_names_for_cid, load_object_cbor};

/// Evaluates a word and returns its single `i64` result.
/// Currently supports words whose parameters and results are all `i64`.
pub fn run_word_i64(conn: &Connection, word_cid: &[u8; 32], args: &[i64]) -> Result<i64> {
    let arg_values: Vec<Value> = args.iter().copied().map(Value::I64).collect();
    let mut results = run_word(conn, word_cid, &arg_values)?;
    let value = results
        .pop()
        .ok_or_else(|| anyhow!("runner expected a single result"))?;
    match value {
        Value::I64(n) => Ok(n),
        other => bail!("expected i64 result, got {:?}", other.type_tag()),
    }
}

/// Evaluate a word and return its result values.
pub fn run_word(conn: &Connection, word_cid: &[u8; 32], args: &[Value]) -> Result<Vec<Value>> {
    let info = load_word_info(conn, word_cid)?;
    if info.params.len() != args.len() {
        bail!(
            "argument mismatch: word expects {} params, got {}",
            info.params.len(),
            args.len()
        );
    }
    for (idx, (expected, actual)) in info.params.iter().zip(args.iter()).enumerate() {
        let actual_tag = actual.type_tag();
        if *expected != actual_tag {
            bail!(
                "argument {idx} type mismatch: expected {:?}, got {:?}",
                expected,
                actual_tag
            );
        }
    }
    if info.results.len() != 1 {
        bail!(
            "runner currently requires exactly one result, found {:?}",
            info.results
        );
    }

    let mut cache = HashMap::new();
    let outputs = eval_return(conn, &info.root, &mut cache, args, &info.results)?;
    if outputs.len() != info.results.len() {
        bail!(
            "result count mismatch: word declares {}, runner produced {}",
            info.results.len(),
            outputs.len()
        );
    }
    for (idx, (expected, actual)) in info.results.iter().zip(outputs.iter()).enumerate() {
        if actual.type_tag() != *expected {
            bail!(
                "result {} type mismatch: expected {:?}, got {:?}",
                idx,
                expected,
                actual.type_tag()
            );
        }
    }
    Ok(outputs)
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct NodeRecord {
    kind: String,
    #[serde(rename = "nk")]
    nk: String,
    #[serde(default)]
    #[allow(dead_code)]
    ty: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    out: Option<Vec<String>>,
    #[serde(default, rename = "in")]
    inputs: Vec<NodeInputRecord>,
    #[serde(default)]
    vals: Vec<NodeInputRecord>,
    #[serde(default)]
    deps: Vec<NodeInputRecord>,
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
    #[serde(default)]
    glob: Option<ByteBuf>,
}

#[derive(Clone, Debug)]
pub enum Value {
    I64(i64),
    F64(f64),
    Ptr(u64),
    Unit,
}

impl Value {
    fn type_tag(&self) -> TypeTag {
        match self {
            Value::I64(_) => TypeTag::I64,
            Value::F64(_) => TypeTag::F64,
            Value::Ptr(_) => TypeTag::Ptr,
            Value::Unit => TypeTag::Unit,
        }
    }
}

fn eval_node(
    conn: &Connection,
    node_cid: &[u8; 32],
    cache: &mut HashMap<[u8; 32], Value>,
    args: &[Value],
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
        "ARG" => {
            let index = record
                .payload
                .arg
                .ok_or_else(|| anyhow!("ARG node missing index payload"))?
                as usize;
            args.get(index)
                .cloned()
                .ok_or_else(|| anyhow!("argument {index} not supplied"))?
        }
        "PRIM" => {
            let mut inputs = Vec::new();
            for input in &record.inputs {
                let input_cid = bytebuf_to_array(&input.cid)?;
                inputs.push(eval_node(conn, &input_cid, cache, args)?);
            }
            let prim_cid_buf = record
                .payload
                .prim
                .ok_or_else(|| anyhow!("PRIM node missing primitive payload"))?;
            let prim_cid = bytebuf_to_array(&prim_cid_buf)?;
            eval_primitive(conn, &prim_cid, inputs)?
        }
        "CALL" => {
            let mut inputs = Vec::new();
            for input in &record.inputs {
                let input_cid = bytebuf_to_array(&input.cid)?;
                inputs.push(eval_node(conn, &input_cid, cache, args)?);
            }
            let word_buf = record
                .payload
                .word
                .ok_or_else(|| anyhow!("CALL node missing word payload"))?;
            let word_cid = bytebuf_to_array(&word_buf)?;
            // TODO: support stack-passed quotations by dispatching without assuming
            // eagerly inlined bodies.
            let mut results = run_word(conn, &word_cid, &inputs)?;
            if results.len() != 1 {
                bail!("runner CALL support limited to single-result words");
            }
            results.pop().unwrap()
        }
        "RETURN" => bail!("RETURN node should be handled at word entry"),
        "LOAD_GLOBAL" => bail!("LOAD_GLOBAL not supported by runner (yet)"),
        kind => bail!("unsupported node kind `{kind}` in runner"),
    };

    cache.insert(*node_cid, value.clone());
    Ok(value)
}

fn eval_return(
    conn: &Connection,
    root: &[u8; 32],
    cache: &mut HashMap<[u8; 32], Value>,
    args: &[Value],
    expected_results: &[TypeTag],
) -> Result<Vec<Value>> {
    let (_, cbor) = load_object_cbor(conn, root)?;
    let record: NodeRecord = serde_cbor::from_slice(&cbor)?;
    if record.kind != "node" {
        bail!("root object {} is not a node", cid::to_hex(root));
    }

    if record.nk != "RETURN" {
        // Legacy single-result root without RETURN.
        let value = eval_node(conn, root, cache, args)?;
        return Ok(vec![value]);
    }

    if record.vals.len() != expected_results.len() {
        bail!(
            "RETURN node value count {} does not match declared results {}",
            record.vals.len(),
            expected_results.len()
        );
    }

    for dep in &record.deps {
        let dep_cid = bytebuf_to_array(&dep.cid)?;
        let _ = eval_node(conn, &dep_cid, cache, args)?;
    }

    let mut outputs = Vec::with_capacity(record.vals.len());
    for input in &record.vals {
        let input_cid = bytebuf_to_array(&input.cid)?;
        let value = eval_node(conn, &input_cid, cache, args)?;
        outputs.push(value);
    }
    Ok(outputs)
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
            other => bail!("primitive expects i64 inputs, got {:?}", other.type_tag()),
        })
        .collect::<Result<_>>()?;

    let value = match name.as_deref() {
        Some("add_i64") => {
            require_sig(&info, &[TypeTag::I64, TypeTag::I64], &[TypeTag::I64])?;
            if ints.len() != 2 {
                bail!("add_i64 expects 2 arguments, got {}", ints.len());
            }
            match compiled_add() {
                Ok(func) => unsafe { Value::I64(func(ints[0], ints[1])) },
                Err(_) => Value::I64(ints[0] + ints[1]),
            }
        }
        Some("sub_i64") => {
            require_sig(&info, &[TypeTag::I64, TypeTag::I64], &[TypeTag::I64])?;
            if ints.len() != 2 {
                bail!("sub_i64 expects 2 arguments, got {}", ints.len());
            }
            match compiled_sub() {
                Ok(func) => unsafe { Value::I64(func(ints[0], ints[1])) },
                Err(_) => Value::I64(ints[0] - ints[1]),
            }
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
