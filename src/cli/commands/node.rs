use std::path::Path;

use anyhow::Result;

use super::util::{parse_cid_list, parse_inputs};
use crate::cli::NodeCommand;
use march5::node::{self, NodeCanon, NodeKind, NodePayload};
use march5::{cid, open_store};

pub(crate) fn cmd_node(store: &Path, command: NodeCommand) -> Result<()> {
    let conn = open_store(store)?;
    let outcome = match command {
        NodeCommand::Lit { ty, value, effects } => {
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Lit,
                out: vec![ty],
                inputs: Vec::new(),
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::LitI64(value),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::Prim {
            ty,
            prim,
            inputs,
            effects,
        } => {
            let prim_cid = cid::from_hex(&prim)?;
            let inputs = parse_inputs(&inputs)?;
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Prim,
                out: vec![ty],
                inputs,
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Prim(prim_cid),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::Call {
            ty,
            word,
            inputs,
            effects,
        } => {
            let word_cid = cid::from_hex(&word)?;
            let inputs = parse_inputs(&inputs)?;
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Call,
                out: vec![ty],
                inputs,
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Word(word_cid),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::Arg { ty, index, effects } => {
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::Arg,
                out: vec![ty],
                inputs: Vec::new(),
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Arg(index),
            };
            node::store_node(&conn, &node)?
        }
        NodeCommand::LoadGlobal {
            ty,
            global,
            effects,
        } => {
            let global_cid = cid::from_hex(&global)?;
            let effects = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let node = NodeCanon {
                kind: NodeKind::LoadGlobal,
                out: vec![ty],
                inputs: Vec::new(),
                vals: Vec::new(),
                deps: Vec::new(),
                effects,
                payload: NodePayload::Global(global_cid),
            };
            node::store_node(&conn, &node)?
        }
    };
    let cid_hex = march5::cid::to_hex(&outcome.cid);
    if outcome.inserted {
        println!("stored node with cid {cid_hex}");
    } else {
        println!("node already present with cid {cid_hex}");
    }
    Ok(())
}
