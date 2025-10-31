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
use crate::global_store;
use crate::prim::load_prim_info;
use crate::types::{self, EffectDomain, EffectMask, TypeTag, effect_mask};
use crate::word::load_word_info;
use crate::{cid, list_names_for_cid, load_object_cbor};
use smallvec::SmallVec;

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
    let token_domains = wrap_token_domains(&word_token_domains(&info));
    validate_output_tokens(&results, &token_domains)?;
    if !token_domains.is_empty() {
        results.drain(0..token_domains.len());
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

fn normalize_effect_mask(mask: EffectMask, has_effects: bool) -> EffectMask {
    if mask == effect_mask::NONE && has_effects {
        effect_mask::IO
    } else {
        mask
    }
}

fn mask_token_domains(mask: EffectMask, has_effects: bool) -> SmallVec<[EffectDomain; 4]> {
    let normalized = normalize_effect_mask(mask, has_effects);
    types::effect_domains(normalized)
}

fn word_token_domains(info: &crate::word::WordInfo) -> SmallVec<[EffectDomain; 4]> {
    mask_token_domains(info.effect_mask, !info.effects.is_empty())
}

fn token_values(domains: &[Option<EffectDomain>]) -> Vec<Value> {
    domains
        .iter()
        .map(|domain| Value::Token(*domain))
        .collect()
}

fn wrap_token_domains(domains: &[EffectDomain]) -> Vec<Option<EffectDomain>> {
    domains.iter().map(|d| Some(*d)).collect()
}

fn consume_token_inputs(
    inputs: &mut Vec<Value>,
    expected: &[Option<EffectDomain>],
) -> Result<()> {
    for domain in expected.iter().rev() {
        let value = inputs
            .pop()
            .ok_or_else(|| anyhow!("effectful node missing token input"))?;
        match (value, domain) {
            (Value::Token(Some(actual)), Some(expected_domain)) if actual == *expected_domain => {}
            (Value::Token(Some(actual)), Some(expected_domain)) => {
                bail!(
                    "effectful node received token for domain {:?}, expected {:?}",
                    actual,
                    *expected_domain
                );
            }
            (Value::Token(_), None) => {}
            (Value::Token(None), Some(expected_domain)) => {
                bail!(
                    "effectful node received generic token, expected {:?}",
                    *expected_domain
                );
            }
            (other, _) => bail!("effectful node missing token input, got {:?}", other.type_tag()),
        }
    }
    Ok(())
}

fn validate_output_tokens(outputs: &[Value], expected: &[Option<EffectDomain>]) -> Result<()> {
    if outputs.len() < expected.len() {
        bail!(
            "effectful node expected {} token output(s), found {}",
            expected.len(),
            outputs.len()
        );
    }
    for (idx, expected_domain) in expected.iter().enumerate() {
        match (outputs.get(idx), expected_domain) {
            (Some(Value::Token(Some(actual))), Some(expected)) if *actual == *expected => {}
            (Some(Value::Token(Some(actual))), Some(expected)) => {
                bail!(
                    "node returned token for domain {:?}, expected {:?}",
                    *actual,
                    *expected
                );
            }
            (Some(Value::Token(Some(_))), None) => {}
            (Some(Value::Token(None)), Some(expected)) => {
                bail!(
                    "node returned generic token, expected {:?}",
                    *expected
                );
            }
            (Some(Value::Token(None)), None) => {}
            (Some(other), _) => {
                bail!(
                    "effectful node missing token output at index {} (got {:?})",
                    idx,
                    other.type_tag()
                );
            }
            (None, _) => {
                bail!(
                    "effectful node missing token output at index {}",
                    idx
                );
            }
        }
    }
    Ok(())
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
    let outputs = eval_return(conn, &info.root, &mut cache, args, info)?;
    let token_domains = wrap_token_domains(&word_token_domains(info));
    let expected_len = info.results.len() + token_domains.len();
    if outputs.len() != expected_len {
        bail!(
            "result count mismatch: word declares {}, runner produced {}",
            info.results.len(),
            outputs.len()
        );
    }
    validate_output_tokens(&outputs, &token_domains)?;
    for (idx, (expected, actual)) in info
        .results
        .iter()
        .zip(outputs.iter().skip(token_domains.len()))
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
    Text(String),
    Unit,
    Tuple(Vec<Value>),
    Quote([u8; 32]),
    Token(Option<EffectDomain>),
}

impl Value {
    fn type_tag(&self) -> TypeTag {
        match self {
            Value::I64(_) => TypeTag::I64,
            Value::F64(_) => TypeTag::F64,
            Value::Ptr(_) => TypeTag::Ptr,
            Value::Text(_) => TypeTag::Text,
            Value::Unit => TypeTag::Unit,
            Value::Tuple(_) => TypeTag::Ptr,
            Value::Quote(_) => TypeTag::Ptr,
            Value::Token(domain) => match domain {
                Some(d) => types::token_tag_for_domain(*d),
                None => TypeTag::Token,
            },
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I64(n) => write!(f, "{n}"),
            Value::F64(x) => write!(f, "{x}"),
            Value::Ptr(ptr) => write!(f, "0x{ptr:016x}"),
            Value::Text(s) => write!(f, "\"{}\"", s.escape_default()),
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
            Value::Token(domain) => match domain {
                Some(d) => write!(f, "<{}>", types::token_tag_for_domain(*d).as_atom()),
                None => write!(f, "<token>"),
            },
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

    let out_tags: Vec<TypeTag> = out_types
        .iter()
        .map(|atom| TypeTag::from_atom(atom))
        .collect::<Result<Vec<_>>>()?;
    let token_domains: Vec<Option<EffectDomain>> = out_tags
        .iter()
        .take_while(|tag| tag.is_token())
        .map(|tag| tag.token_domain())
        .collect();

    let mut inputs = eval_inputs(conn, &inputs_raw, cache, args)?;

    let values = match kind_tag {
        0 => {
            let lit = cbor_to_i64(&payload_val, "LIT payload")?;
            vec![Value::I64(lit)]
        }
        1 => {
            let prim_cid = cbor_to_bytes32(&payload_val, "PRIM payload")?;
            consume_token_inputs(&mut inputs, &token_domains)?;
            let mut outputs = token_values(&token_domains);
            outputs.push(eval_primitive(conn, &prim_cid, inputs)?);
            validate_output_tokens(&outputs, &token_domains)?;
            outputs
        }
        2 => {
            let word_cid = cbor_to_bytes32(&payload_val, "CALL payload")?;
            consume_token_inputs(&mut inputs, &token_domains)?;
            let outputs = run_word(conn, &word_cid, &inputs)?;
            validate_output_tokens(&outputs, &token_domains)?;
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
            consume_token_inputs(&mut inputs, &token_domains)?;
            eval_apply(conn, &qid, type_key, &mut inputs, &token_domains)?
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
        11 => token_values(&token_domains),
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
    info: &crate::word::WordInfo,
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
    let token_domains = wrap_token_domains(&word_token_domains(info));
    let expected_len = token_domains.len() + info.results.len();
    if vals_raw.len() != expected_len {
        bail!(
            "RETURN node value count {} does not match declared arity {} (tokens + results)",
            vals_raw.len(),
            expected_len
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
    validate_output_tokens(&outputs, &token_domains)?;
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
    token_domains: &[Option<EffectDomain>],
) -> Result<Vec<Value>> {
    let args = std::mem::take(inputs);
    let outputs = run_word(conn, qid, &args)?;
    validate_output_tokens(&outputs, token_domains)?;
    Ok(outputs)
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

fn quote_key(value: &Value) -> Result<String> {
    match value {
        Value::Quote(cid_bytes) => Ok(cid::to_hex(cid_bytes)),
        other => bail!(
            "state primitive expects quote identifier key, got {:?}",
            other.type_tag()
        ),
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

    match name.as_deref() {
        Some("add_i64") => {
            require_sig(&info, &[TypeTag::I64, TypeTag::I64], &[TypeTag::I64])?;
            if inputs.len() != 2 {
                bail!("add_i64 expects 2 arguments, got {}", inputs.len());
            }
            let lhs = value_to_i64(&inputs[0])?;
            let rhs = value_to_i64(&inputs[1])?;
            let result = match compiled_add() {
                Ok(func) => unsafe { func(lhs, rhs) },
                Err(_) => lhs + rhs,
            };
            Ok(Value::I64(result))
        }
        Some("sub_i64") => {
            require_sig(&info, &[TypeTag::I64, TypeTag::I64], &[TypeTag::I64])?;
            if inputs.len() != 2 {
                bail!("sub_i64 expects 2 arguments, got {}", inputs.len());
            }
            let lhs = value_to_i64(&inputs[0])?;
            let rhs = value_to_i64(&inputs[1])?;
            let result = match compiled_sub() {
                Ok(func) => unsafe { func(lhs, rhs) },
                Err(_) => lhs - rhs,
            };
            Ok(Value::I64(result))
        }
        Some("state.read_i64") => {
            require_sig(&info, &[TypeTag::Ptr], &[TypeTag::I64])?;
            if inputs.len() != 1 {
                bail!("state.read_i64 expects 1 argument, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            match global_store::read(&key) {
                Some(Value::I64(n)) => Ok(Value::I64(n)),
                Some(other) => bail!(
                    "state entry `{key}` holds incompatible value {:?}",
                    other.type_tag()
                ),
                None => bail!("state entry `{key}` not found"),
            }
        }
        Some("state.read_f64") => {
            require_sig(&info, &[TypeTag::Ptr], &[TypeTag::F64])?;
            if inputs.len() != 1 {
                bail!("state.read_f64 expects 1 argument, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            match global_store::read(&key) {
                Some(Value::F64(x)) => Ok(Value::F64(x)),
                Some(other) => bail!(
                    "state entry `{key}` holds incompatible value {:?}",
                    other.type_tag()
                ),
                None => bail!("state entry `{key}` not found"),
            }
        }
        Some("state.read_ptr") => {
            require_sig(&info, &[TypeTag::Ptr], &[TypeTag::Ptr])?;
            if inputs.len() != 1 {
                bail!("state.read_ptr expects 1 argument, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            match global_store::read(&key) {
                Some(Value::Tuple(values)) => Ok(Value::Tuple(values)),
                Some(Value::Quote(cid)) => Ok(Value::Quote(cid)),
                Some(other) => bail!(
                    "state entry `{key}` holds incompatible value {:?}",
                    other.type_tag()
                ),
                None => bail!("state entry `{key}` not found"),
            }
        }
        Some("state.read_text") => {
            require_sig(&info, &[TypeTag::Ptr], &[TypeTag::Text])?;
            if inputs.len() != 1 {
                bail!("state.read_text expects 1 argument, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            match global_store::read(&key) {
                Some(Value::Text(s)) => Ok(Value::Text(s)),
                Some(other) => bail!(
                    "state entry `{key}` holds incompatible value {:?}",
                    other.type_tag()
                ),
                None => bail!("state entry `{key}` not found"),
            }
        }
        Some("state.write_i64") => {
            require_sig(&info, &[TypeTag::Ptr, TypeTag::I64], &[TypeTag::Unit])?;
            if inputs.len() != 2 {
                bail!("state.write_i64 expects 2 arguments, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            let value = value_to_i64(&inputs[1])?;
            global_store::write(key, Value::I64(value));
            Ok(Value::Unit)
        }
        Some("state.write_f64") => {
            require_sig(&info, &[TypeTag::Ptr, TypeTag::F64], &[TypeTag::Unit])?;
            if inputs.len() != 2 {
                bail!("state.write_f64 expects 2 arguments, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            let value = value_to_f64(&inputs[1])?;
            global_store::write(key, Value::F64(value));
            Ok(Value::Unit)
        }
        Some("state.write_ptr") => {
            require_sig(&info, &[TypeTag::Ptr, TypeTag::Ptr], &[TypeTag::Unit])?;
            if inputs.len() != 2 {
                bail!("state.write_ptr expects 2 arguments, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            let value = match &inputs[1] {
                Value::Tuple(_) | Value::Quote(_) => inputs[1].clone(),
                other => bail!(
                    "state.write_ptr expects tuple or quote value, got {:?}",
                    other.type_tag()
                ),
            };
            global_store::write(key, value);
            Ok(Value::Unit)
        }
        Some("state.write_text") => {
            require_sig(&info, &[TypeTag::Ptr, TypeTag::Text], &[TypeTag::Unit])?;
            if inputs.len() != 2 {
                bail!("state.write_text expects 2 arguments, got {}", inputs.len());
            }
            let key = quote_key(&inputs[0])?;
            let value = match &inputs[1] {
                Value::Text(s) => Value::Text(s.clone()),
                other => bail!(
                    "state.write_text expects text value, got {:?}",
                    other.type_tag()
                ),
            };
            global_store::write(key, value);
            Ok(Value::Unit)
        }
        Some(other) => bail!("primitive `{other}` not supported in runner"),
        None => bail!(
            "primitive {} not registered with a name (runner needs a symbolic name)",
            cid::to_hex(prim_cid)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::GraphBuilder;
    use crate::node::{NodeCanon, NodeInput, NodeKind, NodePayload};
    use crate::prim::{self, PrimCanon};
    use crate::store;
    use crate::types::{EffectDomain, TypeTag, effect_mask};
    use crate::global_store;

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
    fn run_word_with_multiple_tokens() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        // Create a primitive that claims IO + State effects so we emit both tokens.
        let params = [TypeTag::I64, TypeTag::I64];
        let results = [TypeTag::I64];
        let prim = PrimCanon {
            params: &params,
            results: &results,
            effects: &[],
            effect_mask: effect_mask::IO | effect_mask::STATE_READ,
        };
        let prim_outcome = prim::store_prim(&conn, &prim)?;
        store::put_name(&conn, "prim", "add_i64", &prim_outcome.cid)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&params)?;
        builder.apply_prim(prim_outcome.cid)?;
        let word_cid = builder.finish_word(&params, &results, Some("demo/add_multi"))?;

        let outputs = run_word(
            &conn,
            &word_cid,
            &[Value::I64(1), Value::I64(2)],
        )?;

        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], Value::Token(Some(EffectDomain::Io)));
        assert_eq!(outputs[1], Value::Token(Some(EffectDomain::State)));
        assert_eq!(outputs[2], Value::I64(3));
        Ok(())
    }

    #[test]
    fn state_read_write_roundtrip() -> Result<()> {
        global_store::reset();

        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let key = [0xAA; 32];

        let read_params = [TypeTag::Ptr];
        let read_results = [TypeTag::I64];
        let read_prim = PrimCanon {
            params: &read_params,
            results: &read_results,
            effects: &[],
            effect_mask: effect_mask::STATE_READ,
        };
        let read_outcome = prim::store_prim(&conn, &read_prim)?;
        store::put_name(&conn, "prim", "state.read_i64", &read_outcome.cid)?;

        let write_params = [TypeTag::Ptr, TypeTag::I64];
        let write_results = [TypeTag::Unit];
        let write_prim = PrimCanon {
            params: &write_params,
            results: &write_results,
            effects: &[],
            effect_mask: effect_mask::STATE_WRITE,
        };
        let write_outcome = prim::store_prim(&conn, &write_prim)?;
        store::put_name(&conn, "prim", "state.write_i64", &write_outcome.cid)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?;
        builder.quote(key)?;
        builder.push_lit_i64(99)?;
        builder.apply_prim(write_outcome.cid)?;
        builder.drop()?;
        builder.quote(key)?;
        builder.apply_prim(read_outcome.cid)?;
        let word_cid = builder.finish_word(&[], &[TypeTag::I64], Some("state/test"))?;

        let outputs = run_word(&conn, &word_cid, &[])?;
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0], Value::Token(Some(EffectDomain::State)));
        assert_eq!(outputs[1], Value::I64(99));

        let stored = global_store::read(&cid::to_hex(&key)).expect("value persisted");
        assert_eq!(stored, Value::I64(99));
        Ok(())
    }

    #[test]
    fn state_read_write_f64_roundtrip() -> Result<()> {
        global_store::reset();

        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let key = [0xBB; 32];

        let read_params = [TypeTag::Ptr];
        let read_results = [TypeTag::F64];
        let read_prim = PrimCanon {
            params: &read_params,
            results: &read_results,
            effects: &[],
            effect_mask: effect_mask::STATE_READ,
        };
        let read_outcome = prim::store_prim(&conn, &read_prim)?;
        store::put_name(&conn, "prim", "state.read_f64", &read_outcome.cid)?;

        let write_params = [TypeTag::Ptr, TypeTag::F64];
        let write_results = [TypeTag::Unit];
        let write_prim = PrimCanon {
            params: &write_params,
            results: &write_results,
            effects: &[],
            effect_mask: effect_mask::STATE_WRITE,
        };
        let write_outcome = prim::store_prim(&conn, &write_prim)?;
        store::put_name(&conn, "prim", "state.write_f64", &write_outcome.cid)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[TypeTag::F64])?;
        builder.quote(key)?;
        builder.swap()?;
        builder.apply_prim(write_outcome.cid)?;
        builder.drop()?;
        builder.quote(key)?;
        builder.apply_prim(read_outcome.cid)?;
        let word_cid = builder.finish_word(&[TypeTag::F64], &[TypeTag::F64], Some("state/f64"))?;

        let outputs = run_word(&conn, &word_cid, &[Value::F64(3.25)])?;
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0], Value::Token(Some(EffectDomain::State)));
        assert_eq!(outputs[1], Value::F64(3.25));

        let stored = global_store::read(&cid::to_hex(&key)).expect("value persisted");
        assert_eq!(stored, Value::F64(3.25));
        Ok(())
    }

    #[test]
    fn state_read_write_ptr_tuple() -> Result<()> {
        global_store::reset();

        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let key = [0xCC; 32];

        let read_params = [TypeTag::Ptr];
        let read_results = [TypeTag::Ptr];
        let read_prim = PrimCanon {
            params: &read_params,
            results: &read_results,
            effects: &[],
            effect_mask: effect_mask::STATE_READ,
        };
        let read_outcome = prim::store_prim(&conn, &read_prim)?;
        store::put_name(&conn, "prim", "state.read_ptr", &read_outcome.cid)?;

        let write_params = [TypeTag::Ptr, TypeTag::Ptr];
        let write_results = [TypeTag::Unit];
        let write_prim = PrimCanon {
            params: &write_params,
            results: &write_results,
            effects: &[],
            effect_mask: effect_mask::STATE_WRITE,
        };
        let write_outcome = prim::store_prim(&conn, &write_prim)?;
        store::put_name(&conn, "prim", "state.write_ptr", &write_outcome.cid)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?;
        builder.quote(key)?;
        builder.push_lit_i64(1)?;
        builder.push_lit_i64(2)?;
        builder.pair()?;
        builder.apply_prim(write_outcome.cid)?;
        builder.drop()?;
        builder.quote(key)?;
        builder.apply_prim(read_outcome.cid)?;
        builder.unpair(TypeTag::I64, TypeTag::I64)?;
        let word_cid = builder.finish_word(&[], &[TypeTag::I64, TypeTag::I64], Some("state/pair"))?;

        let outputs = run_word(&conn, &word_cid, &[])?;
        assert_eq!(outputs.len(), 3);
        assert_eq!(outputs[0], Value::Token(Some(EffectDomain::State)));
        assert_eq!(outputs[1], Value::I64(1));
        assert_eq!(outputs[2], Value::I64(2));

        let stored = global_store::read(&cid::to_hex(&key)).expect("value persisted");
        assert_eq!(stored, Value::Tuple(vec![Value::I64(1), Value::I64(2)]));
        Ok(())
    }

    #[test]
    fn state_read_write_text() -> Result<()> {
        global_store::reset();

        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let key = [0xDD; 32];

        let read_params = [TypeTag::Ptr];
        let read_results = [TypeTag::Text];
        let read_prim = PrimCanon {
            params: &read_params,
            results: &read_results,
            effects: &[],
            effect_mask: effect_mask::STATE_READ,
        };
        let read_outcome = prim::store_prim(&conn, &read_prim)?;
        store::put_name(&conn, "prim", "state.read_text", &read_outcome.cid)?;

        let write_params = [TypeTag::Ptr, TypeTag::Text];
        let write_results = [TypeTag::Unit];
        let write_prim = PrimCanon {
            params: &write_params,
            results: &write_results,
            effects: &[],
            effect_mask: effect_mask::STATE_WRITE,
        };
        let write_outcome = prim::store_prim(&conn, &write_prim)?;
        store::put_name(&conn, "prim", "state.write_text", &write_outcome.cid)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[TypeTag::Text])?;
        builder.quote(key)?;
        builder.swap()?;
        builder.apply_prim(write_outcome.cid)?;
        builder.drop()?;
        builder.quote(key)?;
        builder.apply_prim(read_outcome.cid)?;
        let word_cid = builder.finish_word(&[TypeTag::Text], &[TypeTag::Text], Some("state/text"))?;

        let outputs = run_word(&conn, &word_cid, &[Value::Text("hello".to_string())])?;
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0], Value::Token(Some(EffectDomain::State)));
        assert_eq!(outputs[1], Value::Text("hello".to_string()));

        let stored = global_store::read(&cid::to_hex(&key)).expect("value persisted");
        assert_eq!(stored, Value::Text("hello".to_string()));
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
