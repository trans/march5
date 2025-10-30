use std::collections::HashMap;
use std::convert::TryFrom;
use std::str;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde_bytes::ByteBuf;
use serde_cbor::Value as CborValue;
use std::fmt;

use crate::exec::{compiled_add, compiled_sub};
use crate::prim::load_prim_info;
use crate::types::{TypeTag, effect_mask, mask_has};
use crate::word::load_word_info;
use crate::{cid, list_names_for_cid, load_object_cbor};

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
    let expects_token = if info.effect_mask == effect_mask::NONE {
        !info.effects.is_empty()
    } else {
        mask_has(info.effect_mask, effect_mask::IO)
    };
    if expects_token {
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
    let has_token = if info.effect_mask == effect_mask::NONE {
        !info.effects.is_empty()
    } else {
        mask_has(info.effect_mask, effect_mask::IO)
    };
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
struct NodeRecord(
    u64,
    u64,
    Vec<NodeInputRecord>,
    Vec<String>,
    Vec<ByteBuf>,
    CborValue,
);

#[derive(Deserialize)]
struct NodeInputRecord(ByteBuf, u32);

impl NodeInputRecord {
    fn cid_array(&self) -> Result<[u8; 32]> {
        bytebuf_to_array(&self.0)
    }

    fn port(&self) -> u32 {
        self.1
    }
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
    let NodeRecord(tag, kind_tag, inputs_raw, out_types, _effects_raw, payload_val) =
        serde_cbor::from_slice(&cbor)?;
    if tag != 6 {
        bail!("object {} is not a node", cid::to_hex(node_cid));
    }

    let has_token_out = out_types.first().map(|s| s == "token").unwrap_or(false);

    let mut inputs = eval_inputs(conn, &inputs_raw, cache, args)?;

    let values = match kind_tag {
        0 => {
            let lit = cbor_to_i64(&payload_val, "LIT payload")?;
            vec![Value::I64(lit)]
        }
        1 => {
            let prim_cid = cbor_to_bytes32(&payload_val, "PRIM payload")?;
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
        2 => {
            let word_cid = cbor_to_bytes32(&payload_val, "CALL payload")?;
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
        3 => {
            let index = cbor_to_u32(&payload_val, "ARG payload")? as usize;
            let value = args
                .get(index)
                .cloned()
                .ok_or_else(|| anyhow!("argument {index} not supplied"))?;
            vec![value]
        }
        4 => bail!("LOAD_GLOBAL not supported by runner (yet)"),
        5 => bail!("RETURN node should be handled at word entry"),
        6 => {
            if inputs.len() != 2 {
                bail!("PAIR node expects two inputs, found {}", inputs.len());
            }
            vec![Value::Tuple(inputs)]
        }
        7 => {
            if inputs.len() != 1 {
                bail!("UNPAIR node expects one input, found {}", inputs.len());
            }
            match inputs.pop().unwrap() {
                Value::Tuple(values) => values,
                other => bail!("UNPAIR expected tuple input, got {:?}", other.type_tag()),
            }
        }
        8 => {
            let quote_cid = cbor_to_bytes32(&payload_val, "QUOTE payload")?;
            vec![Value::Quote(quote_cid)]
        }
        9 => {
            let (qid, type_key) = cbor_to_apply_payload(&payload_val)?;
            eval_apply(conn, &qid, type_key, &mut inputs)?
        }
        10 => {
            if inputs_raw.len() != 1 {
                bail!(
                    "IF node requires exactly one condition input, found {}",
                    inputs_raw.len()
                );
            }
            let cond_value = inputs
                .drain(..1)
                .next()
                .ok_or_else(|| anyhow!("IF missing evaluated condition"))?;
            let cond_truth = match cond_value {
                Value::I64(n) => n != 0,
                _ => bail!("IF condition must be i64 (0/!=0)"),
            };
            let branches = cbor_to_inputs(&payload_val, "IF payload")?;
            if branches.len() != 2 {
                bail!("IF payload must contain exactly two continuations");
            }
            let branch = if cond_truth {
                &branches[0]
            } else {
                &branches[1]
            };
            let result = eval_input(conn, branch, cache, args)?;
            vec![result]
        }
        11 => vec![Value::Token],
        12 => {
            let (type_key, match_input, else_input) = cbor_to_guard_payload(&payload_val)?;
            let expected_tag = decode_guard_type_key(&type_key)?;
            let input_value = inputs
                .drain(..1)
                .next()
                .ok_or_else(|| anyhow!("GUARD missing evaluated input"))?;
            let matches = input_value.type_tag() == expected_tag;
            let branch = if matches { match_input } else { else_input };
            vec![eval_input(conn, &branch, cache, args)?]
        }
        13 => bail!("deopt triggered"),
        other => bail!("unsupported node kind tag `{other}` in runner"),
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
    let NodeRecord(tag, kind_tag, _inputs_raw, _out_types, _effects_raw, payload_val) =
        serde_cbor::from_slice(&cbor)?;
    if tag != 6 {
        bail!("root object {} is not a node", cid::to_hex(root));
    }

    if kind_tag != 5 {
        // Legacy root without RETURN.
        let values = eval_node(conn, root, cache, args)?;
        return Ok(values);
    }

    let (vals_raw, deps_raw) = cbor_to_return_payload(&payload_val)?;
    if vals_raw.len() != expected_results.len() {
        bail!(
            "RETURN node value count {} does not match declared results {}",
            vals_raw.len(),
            expected_results.len()
        );
    }

    for dep in &deps_raw {
        let _ = eval_input(conn, dep, cache, args)?;
    }

    let mut outputs = Vec::with_capacity(vals_raw.len());
    for input in &vals_raw {
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
    let input_cid = record.cid_array()?;
    let outputs = eval_node(conn, &input_cid, cache, args)?;
    let port = record.port() as usize;
    outputs
        .get(port)
        .cloned()
        .ok_or_else(|| anyhow!("node {} missing port {port}", cid::to_hex(&input_cid)))
}

fn cbor_to_i64(value: &CborValue, context: &str) -> Result<i64> {
    match value {
        CborValue::Integer(n) => match i64::try_from(*n) {
            Ok(value) => Ok(value),
            Err(_) => bail!("{context} integer out of range for i64"),
        },
        other => bail!("{context} expected integer, found {other:?}"),
    }
}

fn cbor_to_u32(value: &CborValue, context: &str) -> Result<u32> {
    let n = cbor_to_i64(value, context)?;
    if n < 0 {
        bail!("{context} must be non-negative");
    }
    Ok(n as u32)
}

fn cbor_to_bytes32(value: &CborValue, context: &str) -> Result<[u8; 32]> {
    match value {
        CborValue::Bytes(bytes) => {
            if bytes.len() != 32 {
                bail!("{context} must be 32 bytes, found {}", bytes.len());
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(bytes);
            Ok(arr)
        }
        other => bail!("{context} expected bytes, found {other:?}"),
    }
}

fn cbor_to_input_record(value: &CborValue, context: &str) -> Result<NodeInputRecord> {
    match value {
        CborValue::Array(items) if items.len() == 2 => {
            let cid = match &items[0] {
                CborValue::Bytes(bytes) => ByteBuf::from(bytes.clone()),
                other => bail!("{context} entry expected bytes, found {other:?}"),
            };
            let port = cbor_to_u32(&items[1], context)?;
            Ok(NodeInputRecord(cid, port))
        }
        other => bail!("{context} expected [cid, port] array, found {other:?}"),
    }
}

fn cbor_to_inputs(value: &CborValue, context: &str) -> Result<Vec<NodeInputRecord>> {
    match value {
        CborValue::Array(items) => items
            .iter()
            .map(|entry| cbor_to_input_record(entry, context))
            .collect(),
        other => bail!("{context} expected array, found {other:?}"),
    }
}

fn cbor_to_apply_payload(value: &CborValue) -> Result<([u8; 32], Option<[u8; 32]>)> {
    match value {
        CborValue::Array(items) if items.len() == 1 => {
            let qid = cbor_to_bytes32(&items[0], "APPLY qid")?;
            Ok((qid, None))
        }
        CborValue::Array(items) if items.len() == 2 => {
            let qid = cbor_to_bytes32(&items[0], "APPLY qid")?;
            let type_key = cbor_to_bytes32(&items[1], "APPLY type key")?;
            Ok((qid, Some(type_key)))
        }
        other => bail!("APPLY payload expected [qid] or [qid, type_key], found {other:?}"),
    }
}

fn cbor_to_guard_payload(
    value: &CborValue,
) -> Result<([u8; 32], NodeInputRecord, NodeInputRecord)> {
    match value {
        CborValue::Array(items) if items.len() == 3 => {
            let type_key = cbor_to_bytes32(&items[0], "GUARD type key")?;
            let match_input = cbor_to_input_record(&items[1], "GUARD match continuation")?;
            let else_input = cbor_to_input_record(&items[2], "GUARD else continuation")?;
            Ok((type_key, match_input, else_input))
        }
        other => bail!("GUARD payload expected [type_key, match, else], found {other:?}"),
    }
}

fn cbor_to_return_payload(
    value: &CborValue,
) -> Result<(Vec<NodeInputRecord>, Vec<NodeInputRecord>)> {
    match value {
        CborValue::Array(items) if items.len() == 2 => {
            let vals = cbor_to_inputs(&items[0], "RETURN vals")?;
            let deps = cbor_to_inputs(&items[1], "RETURN deps")?;
            Ok((vals, deps))
        }
        other => bail!("RETURN payload expected [vals, deps], found {other:?}"),
    }
}

fn eval_apply(
    conn: &Connection,
    qid: &[u8; 32],
    _type_key: Option<[u8; 32]>,
    inputs: &mut Vec<Value>,
) -> Result<Vec<Value>> {
    let args = std::mem::take(inputs);
    run_word(conn, qid, &args)
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
    use crate::types::{TypeTag, effect_mask};

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
            effect_mask: effect_mask::NONE,
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
            effect_mask: effect_mask::NONE,
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
            effect_mask: effect_mask::NONE,
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
            effect_mask: effect_mask::NONE,
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
