//! Canonical encoding and storage of Mini-INet nodes.

use std::cmp::Ordering;

use anyhow::{Result, bail};
use rusqlite::Connection;

use crate::cbor::{push_array, push_bytes, push_i64, push_map, push_text, push_u32};
use crate::{cid, store};

/// Reference to another node's output.
#[derive(Clone, Debug)]
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
}

/// Minimal node kinds currently implemented.
#[derive(Clone, Copy, Debug)]
pub enum NodeKind {
    Lit,
    Prim,
    Call,
    Arg,
    LoadGlobal,
}

/// Fully described node ready for canonical encoding.
#[derive(Clone, Debug)]
pub struct NodeCanon {
    pub kind: NodeKind,
    pub ty: String,
    pub inputs: Vec<NodeInput>,
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
    let key_count = 5 + if node.effects.is_empty() { 0 } else { 1 };
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
        },
    );

    push_text(&mut buf, "ty");
    push_text(&mut buf, &node.ty);

    push_text(&mut buf, "in");
    encode_inputs(&mut buf, &node.inputs);

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

fn encode_inputs(buf: &mut Vec<u8>, inputs: &[NodeInput]) {
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

fn encode_effects(buf: &mut Vec<u8>, effects: &[[u8; 32]]) {
    let mut sorted = effects.to_vec();
    sorted.sort();
    push_array(buf, sorted.len() as u64);
    for effect in sorted {
        push_bytes(buf, &effect);
    }
}

fn encode_payload(buf: &mut Vec<u8>, payload: &NodePayload) {
    push_map(buf, 1);
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
    }
}

fn validate_node(node: &NodeCanon) -> Result<()> {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_lit_node() {
        let node = NodeCanon {
            kind: NodeKind::Lit,
            ty: "i64".to_string(),
            inputs: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(9),
        };
        let encoded = encode(&node).unwrap();
        assert_eq!(
            encoded,
            vec![
                0xA5, // map(5)
                0x64, b'k', b'i', b'n', b'd', 0x64, b'n', b'o', b'd', b'e', 0x62, b'n', b'k', 0x63,
                b'L', b'I', b'T', 0x62, b't', b'y', 0x63, b'i', b'6', b'4', 0x62, b'i', b'n',
                0x80, // array(0)
                0x62, b'p', b'l', 0xA1, // map(1)
                0x63, b'l', b'i', b't', 0x09, // positive integer 9
            ]
        );
    }

    #[test]
    fn encode_prim_node_with_inputs_and_effects() {
        let node = NodeCanon {
            kind: NodeKind::Prim,
            ty: "i64".to_string(),
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
            effects: vec![[0xAA; 32]],
            payload: NodePayload::Prim([0xFF; 32]),
        };
        let encoded = encode(&node).unwrap();
        // The first port value in the payload should be 0 (corresponding to cid 0x10...).
        let mut port_values = Vec::new();
        let mut search_start = 0;
        while let Some(rel_pos) = encoded[search_start..]
            .windows(5)
            .position(|w| w == [0x64, b'p', b'o', b'r', b't'])
        {
            let port_index = search_start + rel_pos + 5;
            port_values.push(encoded[port_index]);
            search_start = port_index + 1;
        }
        assert_eq!(port_values, vec![0, 1]);
        // Ensure the effect CID bytes are present and the prim payload CID is present.
        assert_eq!(encoded.iter().filter(|&&b| b == 0xAA).count(), 32);
        assert_eq!(encoded.iter().filter(|&&b| b == 0xFF).count(), 32);
    }
}
