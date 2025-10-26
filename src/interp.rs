use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;
use std::fmt;

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
    let mut cache: HashMap<[u8; 32], Vec<Value>> = HashMap::new();
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
    #[serde(default)]
    quote: Option<ByteBuf>,
    #[serde(default, rename = "qid")]
    apply_qid: Option<ByteBuf>,
    #[serde(default, rename = "type_key")]
    apply_type_key: Option<ByteBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    I64(i64),
    F64(f64),
    Ptr(u64),
    Unit,
    Tuple(Vec<Value>),
    Quote([u8; 32]),
}

impl Value {
    fn type_tag(&self) -> TypeTag {
        match self {
            Value::I64(_) => TypeTag::I64,
            Value::F64(_) => TypeTag::F64,
            Value::Ptr(_) => TypeTag::Ptr,
            Value::Unit => TypeTag::Unit,
            Value::Tuple(_) => TypeTag::Ptr,
            Value::Quote(_) => TypeTag::Ptr,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I64(n) => write!(f, "{n}"),
            Value::F64(x) => write!(f, "{x}"),
            Value::Ptr(ptr) => write!(f, "0x{ptr:016x}"),
            Value::Unit => write!(f, "()"),
            Value::Tuple(values) => {
                let body = values
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({body})")
            }
            Value::Quote(qid) => write!(f, "<quote:{}>", cid::to_hex(qid)),
        }
    }
}

fn eval_node(
    conn: &Connection,
    node_cid: &[u8; 32],
    cache: &mut HashMap<[u8; 32], Vec<Value>>,
    args: &[Value],
) -> Result<Vec<Value>> {
    if let Some(values) = cache.get(node_cid) {
        return Ok(values.clone());
    }

    let (_, cbor) = load_object_cbor(conn, node_cid)?;
    let record: NodeRecord = serde_cbor::from_slice(&cbor)?;
    if record.kind != "node" {
        bail!("object {} is not a node", cid::to_hex(node_cid));
    }

    let values = match record.nk.as_str() {
        "LIT" => {
            let lit = record
                .payload
                .lit
                .ok_or_else(|| anyhow!("LIT node missing literal payload"))?;
            vec![Value::I64(lit)]
        }
        "ARG" => {
            let index = record
                .payload
                .arg
                .ok_or_else(|| anyhow!("ARG node missing index payload"))?
                as usize;
            let value = args
                .get(index)
                .cloned()
                .ok_or_else(|| anyhow!("argument {index} not supplied"))?;
            vec![value]
        }
        "PRIM" => {
            let inputs = eval_inputs(conn, &record.inputs, cache, args)?;
            let prim_cid_buf = record
                .payload
                .prim
                .ok_or_else(|| anyhow!("PRIM node missing primitive payload"))?;
            let prim_cid = bytebuf_to_array(&prim_cid_buf)?;
            vec![eval_primitive(conn, &prim_cid, inputs)?]
        }
        "CALL" => {
            let inputs = eval_inputs(conn, &record.inputs, cache, args)?;
            let word_buf = record
                .payload
                .word
                .ok_or_else(|| anyhow!("CALL node missing word payload"))?;
            let word_cid = bytebuf_to_array(&word_buf)?;
            // TODO: support stack-passed quotations by dispatching without assuming
            // eagerly inlined bodies.
            run_word(conn, &word_cid, &inputs)?
        }
        "PAIR" => {
            let inputs = eval_inputs(conn, &record.inputs, cache, args)?;
            if inputs.len() != 2 {
                bail!(
                    "PAIR node requires exactly two inputs, found {}",
                    inputs.len()
                );
            }
            vec![Value::Tuple(inputs)]
        }
        "UNPAIR" => {
            if record.inputs.len() != 1 {
                bail!(
                    "UNPAIR node requires exactly one input, found {}",
                    record.inputs.len()
                );
            }
            let input_value = eval_input(conn, &record.inputs[0], cache, args)?;
            match input_value {
                Value::Tuple(values) => values,
                other => bail!("UNPAIR expected tuple value, got {:?}", other),
            }
        }
        "QUOTE" => {
            let quote_buf = record
                .payload
                .quote
                .ok_or_else(|| anyhow!("QUOTE node missing quotation payload"))?;
            let qid = bytebuf_to_array(&quote_buf)?;
            vec![Value::Quote(qid)]
        }
        "APPLY" => {
            let qid_buf = record
                .payload
                .apply_qid
                .ok_or_else(|| anyhow!("APPLY node missing qid payload"))?;
            let qid = bytebuf_to_array(&qid_buf)?;
            let inputs = eval_inputs(conn, &record.inputs, cache, args)?;
            run_word(conn, &qid, &inputs)?
        }
        "RETURN" => bail!("RETURN node should be handled at word entry"),
        "LOAD_GLOBAL" => bail!("LOAD_GLOBAL not supported by runner (yet)"),
        kind => bail!("unsupported node kind `{kind}` in runner"),
    };

    cache.insert(*node_cid, values.clone());
    Ok(values)
}

fn eval_return(
    conn: &Connection,
    root: &[u8; 32],
    cache: &mut HashMap<[u8; 32], Vec<Value>>,
    args: &[Value],
    expected_results: &[TypeTag],
) -> Result<Vec<Value>> {
    let (_, cbor) = load_object_cbor(conn, root)?;
    let record: NodeRecord = serde_cbor::from_slice(&cbor)?;
    if record.kind != "node" {
        bail!("root object {} is not a node", cid::to_hex(root));
    }

    if record.nk != "RETURN" {
        // Legacy root without RETURN.
        let values = eval_node(conn, root, cache, args)?;
        return Ok(values);
    }

    if record.vals.len() != expected_results.len() {
        bail!(
            "RETURN node value count {} does not match declared results {}",
            record.vals.len(),
            expected_results.len()
        );
    }

    for dep in &record.deps {
        let _ = eval_input(conn, dep, cache, args)?;
    }

    let mut outputs = Vec::with_capacity(record.vals.len());
    for input in &record.vals {
        let value = eval_input(conn, input, cache, args)?;
        outputs.push(value);
    }
    Ok(outputs)
}

fn eval_inputs(
    conn: &Connection,
    records: &[NodeInputRecord],
    cache: &mut HashMap<[u8; 32], Vec<Value>>,
    args: &[Value],
) -> Result<Vec<Value>> {
    let mut values = Vec::with_capacity(records.len());
    for input in records {
        values.push(eval_input(conn, input, cache, args)?);
    }
    Ok(values)
}

fn eval_input(
    conn: &Connection,
    record: &NodeInputRecord,
    cache: &mut HashMap<[u8; 32], Vec<Value>>,
    args: &[Value],
) -> Result<Value> {
    let input_cid = bytebuf_to_array(&record.cid)?;
    let outputs = eval_node(conn, &input_cid, cache, args)?;
    let port = record.port as usize;
    outputs
        .get(port)
        .cloned()
        .ok_or_else(|| anyhow!("node {} missing port {port}", cid::to_hex(&input_cid)))
}

fn value_to_i64(value: &Value) -> Result<i64> {
    match value {
        Value::I64(n) => Ok(*n),
        other => bail!("expected i64 value, got {:?}", other.type_tag()),
    }
}

#[allow(dead_code)]
fn value_to_f64(value: &Value) -> Result<f64> {
    match value {
        Value::F64(x) => Ok(*x),
        other => bail!("expected f64 value, got {:?}", other.type_tag()),
    }
}

#[allow(dead_code)]
fn value_to_ptr(value: &Value) -> Result<u64> {
    match value {
        Value::Ptr(p) => Ok(*p),
        other => bail!("expected ptr value, got {:?}", other.type_tag()),
    }
}

fn eval_primitive(conn: &Connection, prim_cid: &[u8; 32], inputs: Vec<Value>) -> Result<Value> {
    let info = load_prim_info(conn, prim_cid)?;
    let name = list_names_for_cid(conn, "prim", prim_cid)?
        .into_iter()
        .next();

    let ints: Vec<i64> = inputs.iter().map(value_to_i64).collect::<Result<_>>()?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::GraphBuilder;
    use crate::store;
    use crate::types::TypeTag;

    #[test]
    fn run_word_supports_multi_result_literals() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?;
        builder.push_lit_i64(1)?;
        builder.push_lit_i64(2)?;
        let word_cid =
            builder.finish_word(&[], &[TypeTag::I64, TypeTag::I64], Some("demo/multi"))?;

        let outputs = run_word(&conn, &word_cid, &[])?;
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0], Value::I64(1));
        assert_eq!(outputs[1], Value::I64(2));
        Ok(())
    }

    #[test]
    fn run_word_supports_void_result() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?;
        let word_cid = builder.finish_word(&[], &[], Some("demo/void"))?;

        let outputs = run_word(&conn, &word_cid, &[])?;
        assert!(outputs.is_empty());
        Ok(())
    }
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
