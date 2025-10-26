use std::collections::HashMap;
use std::str;

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
    let info = load_word_info(conn, word_cid)?;
    if info.params.len() != args.len() {
        bail!(
            "argument mismatch: word expects {} params, got {}",
            info.params.len(),
            args.len()
        );
    }
    for (idx, expected) in info.params.iter().enumerate() {
        if *expected != TypeTag::I64 {
            bail!(
                "runner only supports i64 parameters (param {} has type {:?})",
                idx,
                expected
            );
        }
    }
    let arg_values: Vec<Value> = args.iter().copied().map(Value::I64).collect();
    let mut results = run_word_with_info(conn, &info, &arg_values)?;
    if !info.effects.is_empty() {
        match results.first() {
            Some(Value::Token) => {
                results.remove(0);
            }
            _ => bail!("effectful word missing token output"),
        }
    }
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
    run_word_with_info(conn, &info, args)
}

fn run_word_with_info(
    conn: &Connection,
    info: &crate::word::WordInfo,
    args: &[Value],
) -> Result<Vec<Value>> {
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
    let has_token = !info.effects.is_empty();
    if has_token {
        match outputs.first() {
            Some(Value::Token) => {}
            _ => bail!("effectful word missing token output"),
        }
    }
    let expected_len = info.results.len() + if has_token { 1 } else { 0 };
    if outputs.len() != expected_len {
        bail!(
            "result count mismatch: word declares {}, runner produced {}",
            info.results.len(),
            outputs.len()
        );
    }
    for (idx, (expected, actual)) in info
        .results
        .iter()
        .zip(outputs.iter().skip(if has_token { 1 } else { 0 }))
        .enumerate()
    {
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
    #[serde(default, rename = "true")]
    if_true: Option<NodeInputRecord>,
    #[serde(default, rename = "false")]
    if_false: Option<NodeInputRecord>,
    #[serde(default, rename = "guard_type")]
    guard_type: Option<ByteBuf>,
    #[serde(default, rename = "match")]
    guard_match: Option<NodeInputRecord>,
    #[serde(default, rename = "else")]
    guard_else: Option<NodeInputRecord>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    I64(i64),
    F64(f64),
    Ptr(u64),
    Unit,
    Tuple(Vec<Value>),
    Quote([u8; 32]),
    Token,
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
            Value::Token => TypeTag::Token,
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
            Value::Token => write!(f, "<token>"),
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
            let mut inputs = eval_inputs(conn, &record.inputs, cache, args)?;
            let prim_cid_buf = record
                .payload
                .prim
                .ok_or_else(|| anyhow!("PRIM node missing primitive payload"))?;
            let prim_cid = bytebuf_to_array(&prim_cid_buf)?;
            let has_token_out = record
                .out
                .as_ref()
                .and_then(|outs| outs.first())
                .map(|s| s == "token")
                .unwrap_or(false);
            if has_token_out {
                match inputs.pop() {
                    Some(Value::Token) => {}
                    _ => bail!("PRIM node missing token input"),
                }
            }
            let mut outputs = Vec::new();
            if has_token_out {
                outputs.push(Value::Token);
            }
            outputs.push(eval_primitive(conn, &prim_cid, inputs)?);
            outputs
        }
        "CALL" => {
            let mut inputs = eval_inputs(conn, &record.inputs, cache, args)?;
            let word_buf = record
                .payload
                .word
                .ok_or_else(|| anyhow!("CALL node missing word payload"))?;
            let word_cid = bytebuf_to_array(&word_buf)?;
            let has_token_out = record
                .out
                .as_ref()
                .and_then(|outs| outs.first())
                .map(|s| s == "token")
                .unwrap_or(false);
            if has_token_out {
                match inputs.pop() {
                    Some(Value::Token) => {}
                    _ => bail!("CALL node missing token input"),
                }
            }
            let mut outputs = run_word(conn, &word_cid, &inputs)?;
            if has_token_out {
                outputs.insert(0, Value::Token);
            }
            outputs
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
            let mut inputs = eval_inputs(conn, &record.inputs, cache, args)?;
            let qid_buf = record
                .payload
                .apply_qid
                .ok_or_else(|| anyhow!("APPLY node missing qid payload"))?;
            let qid = bytebuf_to_array(&qid_buf)?;
            let has_token_out = record
                .out
                .as_ref()
                .and_then(|outs| outs.first())
                .map(|s| s == "token")
                .unwrap_or(false);
            if has_token_out {
                match inputs.pop() {
                    Some(Value::Token) => {}
                    _ => bail!("APPLY node missing token input"),
                }
            }
            let mut outputs = run_word(conn, &qid, &inputs)?;
            if has_token_out {
                outputs.insert(0, Value::Token);
            }
            outputs
        }
        "TOKEN" => vec![Value::Token],
        "GUARD" => {
            if record.inputs.len() != 1 {
                bail!(
                    "GUARD node requires exactly one input, found {}",
                    record.inputs.len()
                );
            }
            let guard_type_bytes = record
                .payload
                .guard_type
                .as_ref()
                .ok_or_else(|| anyhow!("GUARD node missing type key payload"))?;
            let expected_tag = decode_guard_type_key(guard_type_bytes.as_ref())?;

            let input_value = eval_input(conn, &record.inputs[0], cache, args)?;
            let matches = input_value.type_tag() == expected_tag;
            let branch = if matches {
                record
                    .payload
                    .guard_match
                    .as_ref()
                    .ok_or_else(|| anyhow!("GUARD missing match continuation"))?
            } else {
                record
                    .payload
                    .guard_else
                    .as_ref()
                    .ok_or_else(|| anyhow!("GUARD missing else continuation"))?
            };
            vec![eval_input(conn, branch, cache, args)?]
        }
        "DEOPT" => bail!("deopt triggered"),
        "IF" => {
            if record.inputs.len() != 1 {
                bail!(
                    "IF node requires exactly one condition input, found {}",
                    record.inputs.len()
                );
            }
            let cond_value = eval_input(conn, &record.inputs[0], cache, args)?;
            let cond_truth = match cond_value {
                Value::I64(n) => n != 0,
                _ => bail!("IF condition must be i64 (0/!=0)"),
            };
            let branch = if cond_truth {
                record
                    .payload
                    .if_true
                    .as_ref()
                    .ok_or_else(|| anyhow!("IF missing true continuation"))?
            } else {
                record
                    .payload
                    .if_false
                    .as_ref()
                    .ok_or_else(|| anyhow!("IF missing false continuation"))?
            };
            let result = eval_input(conn, branch, cache, args)?;
            vec![result]
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

fn decode_guard_type_key(bytes: &[u8]) -> Result<TypeTag> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let slice = &bytes[..end];
    let atom = str::from_utf8(slice)?;
    TypeTag::from_atom(atom)
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
    use crate::node::{NodeCanon, NodeInput, NodeKind, NodePayload};
    use crate::store;
    use crate::types::TypeTag;

    fn guard_key(tag: TypeTag) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        let atom = tag.as_atom().as_bytes();
        bytes[..atom.len()].copy_from_slice(atom);
        bytes
    }

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

    #[test]
    fn run_word_guard_match_branch() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let value_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(5),
        };
        let value_cid = crate::node::store_node(&conn, &value_node)?.cid;

        let match_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(1),
        };
        let match_cid = crate::node::store_node(&conn, &match_node)?.cid;

        let else_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(99),
        };
        let else_cid = crate::node::store_node(&conn, &else_node)?.cid;

        let guard_node = NodeCanon {
            kind: NodeKind::Guard,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: vec![NodeInput {
                cid: value_cid,
                port: 0,
            }],
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Guard {
                type_key: guard_key(TypeTag::I64),
                match_cont: NodeInput {
                    cid: match_cid,
                    port: 0,
                },
                else_cont: NodeInput {
                    cid: else_cid,
                    port: 0,
                },
            },
        };
        let guard_cid = crate::node::store_node(&conn, &guard_node)?.cid;

        let return_node = NodeCanon {
            kind: NodeKind::Return,
            ty: None,
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: vec![NodeInput {
                cid: guard_cid,
                port: 0,
            }],
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Return,
        };
        let return_cid = crate::node::store_node(&conn, &return_node)?.cid;

        let word = crate::word::WordCanon {
            root: return_cid,
            params: Vec::new(),
            results: vec![TypeTag::I64.as_atom().to_string()],
            effects: Vec::new(),
        };
        let word_cid = crate::word::store_word(&conn, &word)?.cid;

        let outputs = run_word(&conn, &word_cid, &[])?;
        assert_eq!(outputs, vec![Value::I64(1)]);
        Ok(())
    }

    #[test]
    fn run_word_guard_else_branch() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let value_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(5),
        };
        let value_cid = crate::node::store_node(&conn, &value_node)?.cid;

        let match_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(1),
        };
        let match_cid = crate::node::store_node(&conn, &match_node)?.cid;

        let else_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(99),
        };
        let else_cid = crate::node::store_node(&conn, &else_node)?.cid;

        let guard_node = NodeCanon {
            kind: NodeKind::Guard,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: vec![NodeInput {
                cid: value_cid,
                port: 0,
            }],
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Guard {
                type_key: guard_key(TypeTag::F64),
                match_cont: NodeInput {
                    cid: match_cid,
                    port: 0,
                },
                else_cont: NodeInput {
                    cid: else_cid,
                    port: 0,
                },
            },
        };
        let guard_cid = crate::node::store_node(&conn, &guard_node)?.cid;

        let return_node = NodeCanon {
            kind: NodeKind::Return,
            ty: None,
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: vec![NodeInput {
                cid: guard_cid,
                port: 0,
            }],
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Return,
        };
        let return_cid = crate::node::store_node(&conn, &return_node)?.cid;

        let word = crate::word::WordCanon {
            root: return_cid,
            params: Vec::new(),
            results: vec![TypeTag::I64.as_atom().to_string()],
            effects: Vec::new(),
        };
        let word_cid = crate::word::store_word(&conn, &word)?.cid;

        let outputs = run_word(&conn, &word_cid, &[])?;
        assert_eq!(outputs, vec![Value::I64(99)]);
        Ok(())
    }

    #[test]
    fn run_word_guard_else_deopt() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let value_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(5),
        };
        let value_cid = crate::node::store_node(&conn, &value_node)?.cid;

        let match_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(1),
        };
        let match_cid = crate::node::store_node(&conn, &match_node)?.cid;

        let deopt_node = NodeCanon {
            kind: NodeKind::Deopt,
            ty: None,
            out: vec![TypeTag::Unit.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Deopt,
        };
        let deopt_cid = crate::node::store_node(&conn, &deopt_node)?.cid;

        let guard_node = NodeCanon {
            kind: NodeKind::Guard,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: vec![NodeInput {
                cid: value_cid,
                port: 0,
            }],
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Guard {
                type_key: guard_key(TypeTag::F64),
                match_cont: NodeInput {
                    cid: match_cid,
                    port: 0,
                },
                else_cont: NodeInput {
                    cid: deopt_cid,
                    port: 0,
                },
            },
        };
        let guard_cid = crate::node::store_node(&conn, &guard_node)?.cid;

        let return_node = NodeCanon {
            kind: NodeKind::Return,
            ty: None,
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: vec![NodeInput {
                cid: guard_cid,
                port: 0,
            }],
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Return,
        };
        let return_cid = crate::node::store_node(&conn, &return_node)?.cid;

        let word = crate::word::WordCanon {
            root: return_cid,
            params: Vec::new(),
            results: vec![TypeTag::I64.as_atom().to_string()],
            effects: Vec::new(),
        };
        let word_cid = crate::word::store_word(&conn, &word)?.cid;

        let err = run_word(&conn, &word_cid, &[]).unwrap_err();
        assert!(err.to_string().contains("deopt"));
        Ok(())
    }

    #[test]
    fn run_word_handles_if_true_branch() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let cond_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(1),
        };
        let cond_cid = crate::node::store_node(&conn, &cond_node)?.cid;

        let true_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(42),
        };
        let true_cid = crate::node::store_node(&conn, &true_node)?.cid;

        let false_node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(7),
        };
        let false_cid = crate::node::store_node(&conn, &false_node)?.cid;

        let if_node = NodeCanon {
            kind: NodeKind::If,
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: vec![NodeInput {
                cid: cond_cid,
                port: 0,
            }],
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::If {
                true_cont: NodeInput {
                    cid: true_cid,
                    port: 0,
                },
                false_cont: NodeInput {
                    cid: false_cid,
                    port: 0,
                },
            },
        };
        let if_cid = crate::node::store_node(&conn, &if_node)?.cid;

        let return_node = NodeCanon {
            kind: NodeKind::Return,
            ty: None,
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: vec![NodeInput {
                cid: if_cid,
                port: 0,
            }],
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Return,
        };
        let return_cid = crate::node::store_node(&conn, &return_node)?.cid;

        let word = crate::word::WordCanon {
            root: return_cid,
            params: Vec::new(),
            results: vec![TypeTag::I64.as_atom().to_string()],
            effects: Vec::new(),
        };
        let word_cid = crate::word::store_word(&conn, &word)?.cid;

        let outputs = run_word(&conn, &word_cid, &[])?;
        assert_eq!(outputs, vec![Value::I64(42)]);
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
