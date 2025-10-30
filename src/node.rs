//! Canonical encoding and storage of Mini-INet nodes.

use std::cmp::Ordering;

use anyhow::{Result, bail};
use rusqlite::Connection;

use crate::cbor::{push_array, push_bytes, push_i64, push_text, push_u32};
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
    push_array(&mut buf, 6);
    crate::cbor::push_u32(&mut buf, 6); // object tag for "node"
    crate::cbor::push_u32(&mut buf, node_kind_tag(node.kind) as u32);
    encode_inputs(&mut buf, &node.inputs);
    encode_outputs(&mut buf, &node.out);
    encode_effects(&mut buf, &node.effects);
    encode_payload(&mut buf, node)?;

    Ok(buf)
}

/// Persist a node in the object store.
pub fn store_node(conn: &Connection, node: &NodeCanon) -> Result<NodeStoreOutcome> {
    let cbor = encode(node)?;
    let cid = cid::compute(&cbor);
    let inserted = store::put_object(conn, &cid, "node", &cbor)?;
    Ok(NodeStoreOutcome { cid, inserted })
}

fn node_kind_tag(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Lit => 0,
        NodeKind::Prim => 1,
        NodeKind::Call => 2,
        NodeKind::Arg => 3,
        NodeKind::LoadGlobal => 4,
        NodeKind::Return => 5,
        NodeKind::Pair => 6,
        NodeKind::Unpair => 7,
        NodeKind::Quote => 8,
        NodeKind::Apply => 9,
        NodeKind::If => 10,
        NodeKind::Token => 11,
        NodeKind::Guard => 12,
        NodeKind::Deopt => 13,
    }
}

fn encode_inputs(buf: &mut Vec<u8>, inputs: &[NodeInput]) {
    push_array(buf, inputs.len() as u64);
    for input in inputs {
        encode_input(buf, input);
    }
}

fn encode_input(buf: &mut Vec<u8>, input: &NodeInput) {
    push_array(buf, 2);
    push_bytes(buf, &input.cid);
    push_u32(buf, input.port);
}

fn encode_input_list(buf: &mut Vec<u8>, inputs: &[NodeInput]) {
    push_array(buf, inputs.len() as u64);
    for input in inputs {
        encode_input(buf, input);
    }
}

fn encode_input_list_sorted(buf: &mut Vec<u8>, inputs: &[NodeInput]) {
    let mut sorted = inputs.to_vec();
    sorted.sort_by(|a, b| match a.cid.cmp(&b.cid) {
        Ordering::Equal => a.port.cmp(&b.port),
        other => other,
    });
    sorted.dedup_by(|a, b| a.cid == b.cid && a.port == b.port);
    encode_input_list(buf, &sorted);
}

fn encode_outputs(buf: &mut Vec<u8>, outs: &[String]) {
    push_array(buf, outs.len() as u64);
    for out in outs {
        push_text(buf, out);
    }
}

fn encode_effects(buf: &mut Vec<u8>, effects: &[[u8; 32]]) {
    let mut sorted = effects.to_vec();
    sorted.sort();
    push_array(buf, sorted.len() as u64);
    for effect in sorted {
        push_bytes(buf, &effect);
    }
}

fn encode_payload(buf: &mut Vec<u8>, node: &NodeCanon) -> Result<()> {
    match node.kind {
        NodeKind::Return => {
            push_array(buf, 2);
            encode_input_list(buf, &node.vals);
            encode_input_list_sorted(buf, &node.deps);
            return Ok(());
        }
        NodeKind::Lit => match node.payload {
            NodePayload::LitI64(value) => {
                push_i64(buf, value);
                return Ok(());
            }
            _ => bail!("LIT node requires literal payload"),
        },
        NodeKind::Prim => match node.payload {
            NodePayload::Prim(cid) => {
                push_bytes(buf, &cid);
                return Ok(());
            }
            _ => bail!("PRIM node requires prim payload"),
        },
        NodeKind::Call => match node.payload {
            NodePayload::Word(cid) => {
                push_bytes(buf, &cid);
                return Ok(());
            }
            _ => bail!("CALL node requires word payload"),
        },
        NodeKind::Arg => match node.payload {
            NodePayload::Arg(index) => {
                push_u32(buf, index);
                return Ok(());
            }
            _ => bail!("ARG node requires index payload"),
        },
        NodeKind::LoadGlobal => match node.payload {
            NodePayload::Global(cid) => {
                push_bytes(buf, &cid);
                return Ok(());
            }
            _ => bail!("LOAD_GLOBAL node requires global payload"),
        },
        NodeKind::Pair | NodeKind::Unpair | NodeKind::Token | NodeKind::Deopt => {
            push_array(buf, 0);
            return Ok(());
        }
        NodeKind::Quote => match node.payload {
            NodePayload::Quote(cid) => {
                push_bytes(buf, &cid);
                return Ok(());
            }
            _ => bail!("QUOTE node requires quote payload"),
        },
        NodeKind::Apply => match node.payload {
            NodePayload::Apply { qid, type_key } => {
                if let Some(key) = type_key {
                    push_array(buf, 2);
                    push_bytes(buf, &qid);
                    push_bytes(buf, &key);
                } else {
                    push_array(buf, 1);
                    push_bytes(buf, &qid);
                }
                return Ok(());
            }
            _ => bail!("APPLY node requires apply payload"),
        },
        NodeKind::If => match node.payload {
            NodePayload::If {
                ref true_cont,
                ref false_cont,
            } => {
                push_array(buf, 2);
                encode_input(buf, true_cont);
                encode_input(buf, false_cont);
                return Ok(());
            }
            _ => bail!("IF node requires branch payload"),
        },
        NodeKind::Guard => match node.payload {
            NodePayload::Guard {
                type_key,
                ref match_cont,
                ref else_cont,
            } => {
                push_array(buf, 3);
                push_bytes(buf, &type_key);
                encode_input(buf, match_cont);
                encode_input(buf, else_cont);
                return Ok(());
            }
            _ => bail!("GUARD node requires guard payload"),
        },
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

    fn decode(encoded: &[u8]) -> Vec<Value> {
        match serde_cbor::from_slice::<Value>(encoded).expect("valid CBOR encoding") {
            Value::Array(items) => items,
            other => panic!("expected node to encode as array, got {other:?}"),
        }
    }

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
        let items = decode(&encode(&node).unwrap());
        assert_eq!(items[0], Value::Integer(6));
        assert_eq!(items[1], Value::Integer(0));
        assert_eq!(items[2], Value::Array(Vec::new()));
        assert_eq!(items[3], Value::Array(vec![Value::Text("i64".to_string())]));
        assert_eq!(items[4], Value::Array(Vec::new()));
        assert_eq!(items[5], Value::Integer(9));
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
        let items = decode(&encode(&node).unwrap());
        assert_eq!(items[1], Value::Integer(1));
        let inputs = match &items[2] {
            Value::Array(values) => values,
            other => panic!("inputs should be array, got {other:?}"),
        };
        assert_eq!(inputs.len(), 2);
        let ports: Vec<u32> = inputs
            .iter()
            .map(|entry| match entry {
                Value::Array(elements) if elements.len() == 2 => match &elements[1] {
                    Value::Integer(port) => *port as u32,
                    other => panic!("expected port integer, got {other:?}"),
                },
                other => panic!("input entry must be array, got {other:?}"),
            })
            .collect();
        assert_eq!(ports, vec![1, 0]);
        assert!(matches!(items[4], Value::Array(ref v) if v.len() == 1));
        assert!(matches!(items[5], Value::Bytes(_)));
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
        let items = decode(&encode(&node).unwrap());
        assert_eq!(items[1], Value::Integer(8));
        assert_eq!(items[3], Value::Array(vec![Value::Text("ptr".to_string())]));
        match &items[5] {
            Value::Bytes(bytes) => assert_eq!(bytes.len(), 32),
            other => panic!("payload must be bytes, got {other:?}"),
        }
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
        let items = decode(&encode(&node).unwrap());
        assert_eq!(items[1], Value::Integer(10));
        match &items[2] {
            Value::Array(values) => assert_eq!(values.len(), 1),
            other => panic!("inputs should be array, got {other:?}"),
        }
        match &items[5] {
            Value::Array(values) => {
                assert_eq!(values.len(), 2);
                for entry in values {
                    match entry {
                        Value::Array(parts) if parts.len() == 2 => {
                            assert!(matches!(parts[0], Value::Bytes(_)));
                            assert!(matches!(parts[1], Value::Integer(_)));
                        }
                        other => panic!("unexpected IF payload entry: {other:?}"),
                    }
                }
            }
            other => panic!("payload must be array, got {other:?}"),
        }
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
        let items = decode(&encode(&node).unwrap());
        assert_eq!(items[1], Value::Integer(11));
        assert!(matches!(items[5], Value::Array(ref arr) if arr.is_empty()));
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
        let items = decode(&encode(&node).unwrap());
        assert_eq!(items[1], Value::Integer(12));
        match &items[5] {
            Value::Array(values) => {
                assert_eq!(values.len(), 3);
                assert!(matches!(values[0], Value::Bytes(ref bytes) if bytes.len() == 32));
            }
            other => panic!("payload must be array, got {other:?}"),
        }
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
        let items = decode(&encode(&node).unwrap());
        assert_eq!(items[1], Value::Integer(13));
        assert!(matches!(items[5], Value::Array(ref arr) if arr.is_empty()));
    }

    fn guard_key(tag: TypeTag) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        let atom = tag.as_atom().as_bytes();
        bytes[..atom.len()].copy_from_slice(atom);
        bytes
    }
}
