//! Canonical encoding and storage of Mini-INet nodes.

use std::cmp::Ordering;

use anyhow::{Result, bail};
use rusqlite::Connection;

use crate::cbor::{push_array, push_bytes, push_i64, push_map, push_text, push_u32};
use crate::{cid, store};

/// Reference to another node's output.
#[derive(Clone, Copy, Debug)]
pub struct NodeInput {
    /// Producer node CID.
    pub cid: [u8; 32],
    /// Output port on the producer.
    pub port: u32,
}

/// Payload variants supported by the initial node set.
#[derive(Clone, Debug)]
pub enum NodePayload {
    LitI64(i64),
    Prim([u8; 32]),
    Word([u8; 32]),
    Arg(u32),
    Global([u8; 32]),
    Return,
    Quote([u8; 32]),
    Apply {
        qid: [u8; 32],
        type_key: Option<[u8; 32]>,
    },
    If {
        true_cont: NodeInput,
        false_cont: NodeInput,
    },
    Token,
    Guard {
        type_key: [u8; 32],
        match_cont: NodeInput,
        else_cont: NodeInput,
    },
    Deopt,
    Empty,
}

/// Minimal node kinds currently implemented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Lit,
    Prim,
    Call,
    Arg,
    LoadGlobal,
    Return,
    Pair,
    Unpair,
    Quote,
    Apply,
    If,
    Token,
    Guard,
    Deopt,
}

/// Fully described node ready for canonical encoding.
#[derive(Clone, Debug)]
pub struct NodeCanon {
    pub kind: NodeKind,
    pub ty: Option<String>,
    pub out: Vec<String>,
    pub inputs: Vec<NodeInput>,
    pub vals: Vec<NodeInput>,
    pub deps: Vec<NodeInput>,
    pub effects: Vec<[u8; 32]>,
    pub payload: NodePayload,
}

/// Result of persisting a node object.
pub struct NodeStoreOutcome {
    pub cid: [u8; 32],
    pub inserted: bool,
}

/// Encode a node into canonical CBOR.
pub fn encode(node: &NodeCanon) -> Result<Vec<u8>> {
    validate_node(node)?;

    let mut buf = Vec::new();
    let mut key_count = 2; // kind, nk
    // TODO: remove legacy `ty` emission once RETURN nodes are fully adopted.
    if node.ty.is_some() {
        key_count += 1;
    }
    if !node.out.is_empty() || node.kind == NodeKind::Return || node.ty.is_none() {
        key_count += 1;
    }
    match node.kind {
        NodeKind::Return => {
            key_count += 2; // vals, deps
        }
        _ => {
            key_count += 1; // in
        }
    }
    if !node.effects.is_empty() {
        key_count += 1;
    }
    key_count += 1; // pl
    push_map(&mut buf, key_count);

    push_text(&mut buf, "kind");
    push_text(&mut buf, "node");

    push_text(&mut buf, "nk");
    push_text(
        &mut buf,
        match node.kind {
            NodeKind::Lit => "LIT",
            NodeKind::Prim => "PRIM",
            NodeKind::Call => "CALL",
            NodeKind::Arg => "ARG",
            NodeKind::LoadGlobal => "LOAD_GLOBAL",
            NodeKind::Return => "RETURN",
            NodeKind::Pair => "PAIR",
            NodeKind::Unpair => "UNPAIR",
            NodeKind::Quote => "QUOTE",
            NodeKind::Apply => "APPLY",
            NodeKind::If => "IF",
            NodeKind::Token => "TOKEN",
            NodeKind::Guard => "GUARD",
            NodeKind::Deopt => "DEOPT",
        },
    );

    if let Some(ty) = &node.ty {
        push_text(&mut buf, "ty");
        push_text(&mut buf, ty);
    }

    if !node.out.is_empty() || node.kind == NodeKind::Return || node.ty.is_none() {
        push_text(&mut buf, "out");
        encode_outputs(&mut buf, &node.out);
    }

    match node.kind {
        NodeKind::Return => {
            push_text(&mut buf, "vals");
            encode_inputs_preserve(&mut buf, &node.vals);
            push_text(&mut buf, "deps");
            encode_inputs_sorted(&mut buf, &node.deps);
        }
        _ => {
            push_text(&mut buf, "in");
            encode_inputs_preserve(&mut buf, &node.inputs);
        }
    }

    if !node.effects.is_empty() {
        push_text(&mut buf, "eff");
        encode_effects(&mut buf, &node.effects);
    }

    push_text(&mut buf, "pl");
    encode_payload(&mut buf, &node.payload);

    Ok(buf)
}

/// Persist a node in the object store.
pub fn store_node(conn: &Connection, node: &NodeCanon) -> Result<NodeStoreOutcome> {
    let cbor = encode(node)?;
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "node", &cbor)?;
    Ok(NodeStoreOutcome { cid, inserted })
}

fn encode_outputs(buf: &mut Vec<u8>, outs: &[String]) {
    push_array(buf, outs.len() as u64);
    for out in outs {
        push_text(buf, out);
    }
}

fn encode_inputs_sorted(buf: &mut Vec<u8>, inputs: &[NodeInput]) {
    let mut sorted = inputs.to_vec();
    sorted.sort_by(|a, b| match a.port.cmp(&b.port) {
        Ordering::Equal => a.cid.cmp(&b.cid),
        other => other,
    });

    push_array(buf, sorted.len() as u64);
    for input in sorted {
        push_map(buf, 2);
        push_text(buf, "cid");
        push_bytes(buf, &input.cid);
        push_text(buf, "port");
        push_u32(buf, input.port);
    }
}

fn encode_inputs_preserve(buf: &mut Vec<u8>, inputs: &[NodeInput]) {
    push_array(buf, inputs.len() as u64);
    for input in inputs {
        push_map(buf, 2);
        push_text(buf, "cid");
        push_bytes(buf, &input.cid);
        push_text(buf, "port");
        push_u32(buf, input.port);
    }
}

fn encode_single_input(buf: &mut Vec<u8>, input: &NodeInput) {
    push_map(buf, 2);
    push_text(buf, "cid");
    push_bytes(buf, &input.cid);
    push_text(buf, "port");
    push_u32(buf, input.port);
}

fn encode_effects(buf: &mut Vec<u8>, effects: &[[u8; 32]]) {
    let mut sorted = effects.to_vec();
    sorted.sort();
    push_array(buf, sorted.len() as u64);
    for effect in sorted {
        push_bytes(buf, &effect);
    }
}

fn encode_payload(buf: &mut Vec<u8>, payload: &NodePayload) {
    match payload {
        NodePayload::Return => {
            push_map(buf, 0);
            return;
        }
        NodePayload::Apply { qid, type_key } => {
            let mut map_len = 1;
            if type_key.is_some() {
                map_len += 1;
            }
            push_map(buf, map_len);
            push_text(buf, "qid");
            push_bytes(buf, qid);
            if let Some(key) = type_key {
                push_text(buf, "type_key");
                push_bytes(buf, key);
            }
            return;
        }
        NodePayload::If {
            true_cont,
            false_cont,
        } => {
            push_map(buf, 2);
            push_text(buf, "true");
            encode_single_input(buf, true_cont);
            push_text(buf, "false");
            encode_single_input(buf, false_cont);
            return;
        }
        NodePayload::Token => {
            push_map(buf, 0);
            return;
        }
        NodePayload::Guard {
            type_key,
            match_cont,
            else_cont,
        } => {
            push_map(buf, 3);
            push_text(buf, "guard_type");
            push_bytes(buf, type_key);
            push_text(buf, "match");
            encode_single_input(buf, match_cont);
            push_text(buf, "else");
            encode_single_input(buf, else_cont);
            return;
        }
        NodePayload::Deopt => {
            push_map(buf, 0);
            return;
        }
        NodePayload::Empty => {
            push_map(buf, 0);
            return;
        }
        _ => push_map(buf, 1),
    }
    match payload {
        NodePayload::LitI64(value) => {
            push_text(buf, "lit");
            push_i64(buf, *value);
        }
        NodePayload::Prim(cid) => {
            push_text(buf, "prim");
            push_bytes(buf, cid);
        }
        NodePayload::Word(cid) => {
            push_text(buf, "word");
            push_bytes(buf, cid);
        }
        NodePayload::Arg(index) => {
            push_text(buf, "arg");
            push_u32(buf, *index);
        }
        NodePayload::Global(cid) => {
            push_text(buf, "glob");
            push_bytes(buf, cid);
        }
        NodePayload::Return => {}
        NodePayload::Quote(cid) => {
            push_text(buf, "quote");
            push_bytes(buf, cid);
        }
        NodePayload::Apply { .. } => unreachable!(),
        NodePayload::If { .. } => unreachable!(),
        NodePayload::Token => unreachable!(),
        NodePayload::Guard { .. } => unreachable!(),
        NodePayload::Deopt => unreachable!(),
        NodePayload::Empty => unreachable!(),
    }
}

fn validate_node(node: &NodeCanon) -> Result<()> {
    match node.kind {
        NodeKind::Return => {
            if !matches!(node.payload, NodePayload::Return) {
                bail!("RETURN node requires a return payload");
            }
            if node.ty.is_some() {
                bail!("RETURN node must not set `ty`");
            }
            if !node.inputs.is_empty() {
                bail!("RETURN node must not have regular inputs");
            }
            if node.out.len() != node.vals.len() {
                bail!(
                    "RETURN node out/vals length mismatch: {} vs {}",
                    node.out.len(),
                    node.vals.len()
                );
            }
            if !node.effects.is_empty() {
                bail!("RETURN node must not declare effects");
            }
        }
        _ => {
            if !node.vals.is_empty() || !node.deps.is_empty() {
                bail!("non-RETURN node cannot specify vals/deps");
            }
            if node.out.is_empty() {
                bail!("non-RETURN node must declare at least one output type");
            }
            if let Some(ty) = &node.ty {
                if node.out.len() == 1 && ty != &node.out[0] {
                    bail!("node ty `{ty}` does not match out[0] `{}`", node.out[0]);
                }
            }
        }
    }

    match node.kind {
        NodeKind::Lit => match node.payload {
            NodePayload::LitI64(_) => Ok(()),
            _ => bail!("LIT node requires a lit payload"),
        },
        NodeKind::Prim => match node.payload {
            NodePayload::Prim(_) => Ok(()),
            _ => bail!("PRIM node requires a prim payload"),
        },
        NodeKind::Call => match node.payload {
            NodePayload::Word(_) => Ok(()),
            _ => bail!("CALL node requires a word payload"),
        },
        NodeKind::Arg => match node.payload {
            NodePayload::Arg(_) => Ok(()),
            _ => bail!("ARG node requires an arg payload"),
        },
        NodeKind::LoadGlobal => match node.payload {
            NodePayload::Global(_) => Ok(()),
            _ => bail!("LOAD_GLOBAL node requires a global payload"),
        },
        NodeKind::Return => Ok(()),
        NodeKind::Pair | NodeKind::Unpair => match node.payload {
            NodePayload::Empty => Ok(()),
            _ => bail!("PAIR/UNPAIR nodes must not carry payload"),
        },
        NodeKind::Quote => match node.payload {
            NodePayload::Quote(_) => Ok(()),
            _ => bail!("QUOTE node requires a quote payload"),
        },
        NodeKind::Apply => match node.payload {
            NodePayload::Apply { .. } => Ok(()),
            _ => bail!("APPLY node requires an apply payload"),
        },
        NodeKind::If => match node.payload {
            NodePayload::If { .. } => Ok(()),
            _ => bail!("IF node requires branch payload"),
        },
        NodeKind::Token => match node.payload {
            NodePayload::Token => Ok(()),
            _ => bail!("TOKEN node must not carry payload"),
        },
        NodeKind::Guard => match node.payload {
            NodePayload::Guard { .. } => Ok(()),
            _ => bail!("GUARD node requires guard payload"),
        },
        NodeKind::Deopt => match node.payload {
            NodePayload::Deopt => Ok(()),
            _ => bail!("DEOPT node must not carry payload"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TypeTag;
    use serde_cbor::Value;

    #[test]
    fn encode_lit_node() {
        let node = NodeCanon {
            kind: NodeKind::Lit,
            ty: Some("i64".to_string()),
            out: vec!["i64".to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(9),
        };
        let encoded = encode(&node).unwrap();
        let value: Value = serde_cbor::from_slice(&encoded).unwrap();
        let map = match value {
            Value::Map(entries) => entries,
            _ => panic!("node should encode as map"),
        };
        let mut fields = std::collections::BTreeMap::new();
        for (k, v) in map {
            if let Value::Text(key) = k {
                fields.insert(key, v);
            }
        }
        assert_eq!(fields.get("nk"), Some(&Value::Text("LIT".to_string())));
        assert_eq!(fields.get("ty"), Some(&Value::Text("i64".to_string())));
        assert_eq!(
            fields.get("out"),
            Some(&Value::Array(vec![Value::Text("i64".to_string())]))
        );
        assert_eq!(fields.get("in"), Some(&Value::Array(Vec::new())));
        assert!(fields.contains_key("pl"));
    }

    #[test]
    fn encode_prim_node_with_inputs_and_effects() {
        let node = NodeCanon {
            kind: NodeKind::Prim,
            ty: Some("i64".to_string()),
            out: vec!["i64".to_string()],
            inputs: vec![
                NodeInput {
                    cid: [0x11; 32],
                    port: 1,
                },
                NodeInput {
                    cid: [0x10; 32],
                    port: 0,
                },
            ],
            vals: Vec::new(),
            deps: Vec::new(),
            effects: vec![[0xAA; 32]],
            payload: NodePayload::Prim([0xFF; 32]),
        };
        let encoded = encode(&node).unwrap();
        let value: Value = serde_cbor::from_slice(&encoded).unwrap();
        let map = match value {
            Value::Map(entries) => entries,
            _ => panic!("node should encode as map"),
        };
        let mut fields = std::collections::BTreeMap::new();
        for (k, v) in map {
            if let Value::Text(key) = k {
                fields.insert(key, v);
            }
        }
        let inputs = match fields.get("in").unwrap() {
            Value::Array(values) => values,
            _ => panic!("inputs should be array"),
        };
        let ports: Vec<u32> = inputs
            .iter()
            .map(|entry| match entry {
                Value::Map(m) => m
                    .iter()
                    .find_map(|(k, v)| {
                        if let (Value::Text(key), Value::Integer(port)) = (k, v) {
                            if key == "port" {
                                Some(*port as u32)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .expect("port field present"),
                _ => panic!("input entry must be map"),
            })
            .collect();
        assert_eq!(ports, vec![1, 0]);
        assert_eq!(
            fields.get("out"),
            Some(&Value::Array(vec![Value::Text("i64".to_string())]))
        );
        // Effects array should contain the expected CID bytes.
        let effects = match fields.get("eff").unwrap() {
            Value::Array(values) => values,
            _ => panic!("effects should be array"),
        };
        assert_eq!(effects.len(), 1);
        let effect_bytes = match &effects[0] {
            Value::Bytes(bytes) => bytes,
            _ => panic!("effect entry should be bytes"),
        };
        assert_eq!(effect_bytes.len(), 32);
    }

    #[test]
    fn encode_quote_node() {
        let node = NodeCanon {
            kind: NodeKind::Quote,
            ty: None,
            out: vec!["ptr".to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Quote([0x42; 32]),
        };
        let encoded = encode(&node).unwrap();
        let value: Value = serde_cbor::from_slice(&encoded).unwrap();
        let entries = match value {
            Value::Map(entries) => entries,
            _ => panic!("quote node should encode to map"),
        };
        let mut map = std::collections::BTreeMap::new();
        for (k, v) in entries {
            if let Value::Text(key) = k {
                map.insert(key, v);
            }
        }
        assert_eq!(map.get("nk"), Some(&Value::Text("QUOTE".to_string())));
        assert_eq!(
            map.get("out"),
            Some(&Value::Array(vec![Value::Text("ptr".to_string())]))
        );
        let payload = map.get("pl").expect("payload present");
        let payload_map = match payload {
            Value::Map(entries) => entries,
            _ => panic!("payload must be map"),
        };
        let quote_bytes = payload_map
            .iter()
            .find_map(|(k, v)| match (k, v) {
                (Value::Text(key), Value::Bytes(bytes)) if key == "quote" => Some(bytes.clone()),
                _ => None,
            })
            .expect("quote bytes present");
        assert_eq!(quote_bytes.len(), 32);
    }

    #[test]
    fn encode_if_node() {
        let node = NodeCanon {
            kind: NodeKind::If,
            ty: None,
            out: vec!["i64".to_string()],
            inputs: vec![NodeInput {
                cid: [0xAA; 32],
                port: 0,
            }],
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::If {
                true_cont: NodeInput {
                    cid: [0xBB; 32],
                    port: 1,
                },
                false_cont: NodeInput {
                    cid: [0xCC; 32],
                    port: 0,
                },
            },
        };
        let encoded = encode(&node).unwrap();
        let value: Value = serde_cbor::from_slice(&encoded).unwrap();
        let map = match value {
            Value::Map(entries) => entries,
            _ => panic!("IF node should encode as map"),
        };
        let mut fields = std::collections::BTreeMap::new();
        for (k, v) in map {
            if let Value::Text(key) = k {
                fields.insert(key, v);
            }
        }
        assert_eq!(fields.get("nk"), Some(&Value::Text("IF".to_string())));
        let inputs = match fields.get("in").expect("inputs present") {
            Value::Array(values) => values,
            _ => panic!("inputs should be array"),
        };
        assert_eq!(inputs.len(), 1);
        let payload = fields.get("pl").expect("payload present");
        let payload_map = match payload {
            Value::Map(entries) => entries,
            _ => panic!("payload must be map"),
        };
        assert!(
            payload_map
                .iter()
                .any(|(k, _)| matches!(k, Value::Text(s) if s == "true"))
        );
        assert!(
            payload_map
                .iter()
                .any(|(k, _)| matches!(k, Value::Text(s) if s == "false"))
        );
    }

    #[test]
    fn encode_token_node() {
        let node = NodeCanon {
            kind: NodeKind::Token,
            ty: Some(TypeTag::Ptr.as_atom().to_string()),
            out: vec![TypeTag::Ptr.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Token,
        };
        let encoded = encode(&node).unwrap();
        let value: Value = serde_cbor::from_slice(&encoded).unwrap();
        let map = match value {
            Value::Map(entries) => entries,
            _ => panic!("TOKEN node should encode as map"),
        };
        let mut fields = std::collections::BTreeMap::new();
        for (k, v) in map {
            if let Value::Text(key) = k {
                fields.insert(key, v);
            }
        }
        assert_eq!(fields.get("nk"), Some(&Value::Text("TOKEN".to_string())));
        assert_eq!(
            fields.get("out"),
            Some(&Value::Array(vec![Value::Text("ptr".to_string())]))
        );
    }

    #[test]
    fn encode_guard_node() {
        let key = guard_key(TypeTag::I64);
        let node = NodeCanon {
            kind: NodeKind::Guard,
            ty: None,
            out: vec!["i64".to_string()],
            inputs: vec![NodeInput {
                cid: [0xAA; 32],
                port: 0,
            }],
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Guard {
                type_key: key,
                match_cont: NodeInput {
                    cid: [0xBB; 32],
                    port: 0,
                },
                else_cont: NodeInput {
                    cid: [0xCC; 32],
                    port: 0,
                },
            },
        };
        let encoded = encode(&node).unwrap();
        let value: Value = serde_cbor::from_slice(&encoded).unwrap();
        let map = match value {
            Value::Map(entries) => entries,
            _ => panic!("GUARD node should encode as map"),
        };
        let mut fields = std::collections::BTreeMap::new();
        for (k, v) in map {
            if let Value::Text(key) = k {
                fields.insert(key, v);
            }
        }
        assert_eq!(fields.get("nk"), Some(&Value::Text("GUARD".to_string())));
        let payload = fields.get("pl").expect("payload present");
        let payload_map = match payload {
            Value::Map(entries) => entries,
            _ => panic!("payload must be map"),
        };
        assert!(
            payload_map
                .iter()
                .any(|(k, _)| matches!(k, Value::Text(s) if s == "guard_type"))
        );
        assert!(
            payload_map
                .iter()
                .any(|(k, _)| matches!(k, Value::Text(s) if s == "match"))
        );
        assert!(
            payload_map
                .iter()
                .any(|(k, _)| matches!(k, Value::Text(s) if s == "else"))
        );
    }

    #[test]
    fn encode_deopt_node() {
        let node = NodeCanon {
            kind: NodeKind::Deopt,
            ty: None,
            out: vec!["unit".to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::Deopt,
        };
        let encoded = encode(&node).unwrap();
        let value: Value = serde_cbor::from_slice(&encoded).unwrap();
        let map = match value {
            Value::Map(entries) => entries,
            _ => panic!("DEOPT node should encode as map"),
        };
        let mut fields = std::collections::BTreeMap::new();
        for (k, v) in map {
            if let Value::Text(key) = k {
                fields.insert(key, v);
            }
        }
        assert_eq!(fields.get("nk"), Some(&Value::Text("DEOPT".to_string())));
    }

    fn guard_key(tag: TypeTag) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        let atom = tag.as_atom().as_bytes();
        bytes[..atom.len()].copy_from_slice(atom);
        bytes
    }
}
