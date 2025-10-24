//! Stack-based graph builder inspired by Forth evaluation order.
//!
//! The builder maintains a stack of `(cid, port, type)` pairs, seeds `ARG`
//! nodes for parameters, and materialises `node`/`word` objects on demand.
//! It uses compact `TypeTag` enums to avoid string drift and keeps track of
//! wiring-only stack operations (`dup`, `swap`, `over`). Declared effects from
//! primitives and words are accumulated so resulting nodes inherit effect sets.

use std::collections::{BTreeSet, HashMap};

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;

use crate::node::{self, NodeCanon, NodeInput, NodeKind, NodePayload};
use crate::prim::{self, PrimInfo};
use crate::store;
use crate::types::TypeTag;
use crate::word::{self, WordCanon, WordInfo};

/// Stack items track the producer CID, output port, and type.
#[derive(Clone, Debug)]
struct StackItem {
    cid: [u8; 32],
    port: u32,
    ty: TypeTag,
}

/// Incrementally builds graphs by pushing nodes and consuming stack items.
pub struct GraphBuilder<'conn> {
    conn: &'conn Connection,
    stack: Vec<StackItem>,
    param_types: Vec<TypeTag>,
    prim_cache: HashMap<[u8; 32], PrimInfo>,
    word_cache: HashMap<[u8; 32], WordInfo>,
    accumulated_effects: BTreeSet<[u8; 32]>,
}

impl<'conn> GraphBuilder<'conn> {
    /// Create a new builder backed by an already-initialised SQLite connection.
    pub fn new(conn: &'conn Connection) -> Self {
        Self {
            conn,
            stack: Vec::new(),
            param_types: Vec::new(),
            prim_cache: HashMap::new(),
            word_cache: HashMap::new(),
            accumulated_effects: BTreeSet::new(),
        }
    }

    /// Start assembling a word with the given parameter types, seeding ARG nodes.
    pub fn begin_word(&mut self, params: &[TypeTag]) -> Result<()> {
        self.stack.clear();
        self.param_types = params.to_vec();
        self.accumulated_effects.clear();

        for (idx, ty) in params.iter().enumerate() {
            let node = NodeCanon {
                kind: NodeKind::Arg,
                ty: ty.as_atom().to_string(),
                inputs: Vec::new(),
                effects: Vec::new(),
                payload: NodePayload::Arg(idx as u32),
            };
            let outcome = node::store_node(self.conn, &node)?;
            self.stack.push(StackItem {
                cid: outcome.cid,
                port: 0,
                ty: *ty,
            });
        }

        Ok(())
    }

    /// Current stack depth (number of available outputs).
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Push a literal i64 node on the stack.
    pub fn push_lit_i64(&mut self, value: i64) -> Result<[u8; 32]> {
        let node = NodeCanon {
            kind: NodeKind::Lit,
            ty: TypeTag::I64.as_atom().to_string(),
            inputs: Vec::new(),
            effects: Vec::new(),
            payload: NodePayload::LitI64(value),
        };
        let outcome = node::store_node(self.conn, &node)?;
        self.stack.push(StackItem {
            cid: outcome.cid,
            port: 0,
            ty: TypeTag::I64,
        });
        Ok(outcome.cid)
    }

    /// Wire-only DUP: (x -- x x)
    pub fn dup(&mut self) -> Result<()> {
        let top = self
            .stack
            .last()
            .cloned()
            .ok_or_else(|| anyhow!("stack underflow: dup"))?;
        self.stack.push(top);
        Ok(())
    }

    /// Wire-only SWAP: (x y -- y x)
    pub fn swap(&mut self) -> Result<()> {
        let n = self.stack.len();
        if n < 2 {
            bail!("stack underflow: swap");
        }
        self.stack.swap(n - 1, n - 2);
        Ok(())
    }

    /// Wire-only OVER: (x y -- x y x)
    pub fn over(&mut self) -> Result<()> {
        let n = self.stack.len();
        if n < 2 {
            bail!("stack underflow: over");
        }
        let item = self.stack[n - 2].clone();
        self.stack.push(item);
        Ok(())
    }

    /// Apply a primitive by CID, deriving arity/result/effects from storage.
    pub fn apply_prim(&mut self, prim_cid: [u8; 32]) -> Result<[u8; 32]> {
        let info = self.prim_info(&prim_cid)?;
        let arity = info.params.len();

        if self.stack.len() < arity {
            bail!(
                "stack underflow: prim needs {arity} values, have {}",
                self.stack.len()
            );
        }

        // Pop inputs (right-to-left), then reverse for left-to-right wiring.
        let mut popped: Vec<StackItem> = Vec::with_capacity(arity);
        for _ in 0..arity {
            popped.push(self.stack.pop().expect("checked stack depth"));
        }
        popped.reverse();

        // Basic type check to avoid wiring mistakes.
        for (i, (expected, actual)) in info.params.iter().zip(popped.iter()).enumerate() {
            if expected != &actual.ty {
                bail!(
                    "prim argument {i} type mismatch: expected {:?}, got {:?}",
                    expected,
                    actual.ty
                );
            }
        }

        let inputs: Vec<NodeInput> = popped
            .iter()
            .map(|item| NodeInput {
                cid: item.cid,
                port: item.port,
            })
            .collect();

        let out_ty = info
            .results
            .get(0)
            .copied()
            .ok_or_else(|| anyhow!("primitive must have at least one result"))?;

        let node = NodeCanon {
            kind: NodeKind::Prim,
            ty: out_ty.as_atom().to_string(),
            inputs,
            effects: info.effects.clone(),
            payload: NodePayload::Prim(prim_cid),
        };
        let outcome = node::store_node(self.conn, &node)?;
        for effect in &info.effects {
            self.accumulated_effects.insert(*effect);
        }
        self.stack.push(StackItem {
            cid: outcome.cid,
            port: 0,
            ty: out_ty,
        });
        Ok(outcome.cid)
    }

    /// Apply a word call using metadata loaded from storage.
    pub fn apply_word(&mut self, word_cid: [u8; 32]) -> Result<[u8; 32]> {
        let info = self.word_info(&word_cid)?;
        let arity = info.params.len();
        if self.stack.len() < arity {
            bail!(
                "stack underflow: call needs {arity} values, have {}",
                self.stack.len()
            );
        }

        let mut popped: Vec<StackItem> = Vec::with_capacity(arity);
        for _ in 0..arity {
            popped.push(self.stack.pop().expect("checked stack depth"));
        }
        popped.reverse();

        for (i, (expected, actual)) in info.params.iter().zip(popped.iter()).enumerate() {
            if expected != &actual.ty {
                bail!(
                    "call argument {i} type mismatch: expected {:?}, got {:?}",
                    expected,
                    actual.ty
                );
            }
        }

        let inputs: Vec<NodeInput> = popped
            .iter()
            .map(|item| NodeInput {
                cid: item.cid,
                port: item.port,
            })
            .collect();

        let out_ty = info
            .results
            .get(0)
            .copied()
            .ok_or_else(|| anyhow!("callee must produce at least one result"))?;

        let node = NodeCanon {
            kind: NodeKind::Call,
            ty: out_ty.as_atom().to_string(),
            inputs,
            effects: info.effects.clone(),
            payload: NodePayload::Word(word_cid),
        };
        let outcome = node::store_node(self.conn, &node)?;
        for effect in &info.effects {
            self.accumulated_effects.insert(*effect);
        }
        self.stack.push(StackItem {
            cid: outcome.cid,
            port: 0,
            ty: out_ty,
        });
        Ok(outcome.cid)
    }

    /// Expose the top CID without consuming it.
    pub fn peek_cid(&self) -> Result<[u8; 32]> {
        self.stack
            .last()
            .map(|item| item.cid)
            .ok_or_else(|| anyhow!("stack is empty"))
    }

    /// Finish the current word, validating result arity exactly.
    pub fn finish_word(
        &mut self,
        params: &[TypeTag],
        results: &[TypeTag],
        symbol: Option<&str>,
    ) -> Result<[u8; 32]> {
        if params != self.param_types {
            bail!(
                "parameter types changed mid-build: began with {:?}, finishing with {:?}",
                self.param_types,
                params
            );
        }

        match results.len() {
            0 => {
                if !self.stack.is_empty() {
                    bail!(
                        "word declared Unit result but stack has {} value(s)",
                        self.stack.len()
                    );
                }
                bail!("unit-returning words not yet supported");
            }
            1 => {
                if self.stack.len() != 1 {
                    bail!(
                        "word must leave exactly one result; stack has {}",
                        self.stack.len()
                    );
                }
                let top = self.stack.last().unwrap();
                if top.ty != results[0] {
                    bail!(
                        "result type mismatch: expected {:?}, got {:?}",
                        results[0],
                        top.ty
                    );
                }
            }
            n => {
                bail!("multi-result words (arity {n}) not supported yet");
            }
        }

        let root = self
            .stack
            .last()
            .ok_or_else(|| anyhow!("cannot finish word without a root node"))?;

        let word = WordCanon {
            root: root.cid,
            params: params.iter().map(|t| t.as_atom().to_string()).collect(),
            results: results.iter().map(|t| t.as_atom().to_string()).collect(),
            effects: self.accumulated_effects.iter().copied().collect(),
        };
        let outcome = word::store_word(self.conn, &word)?;
        if let Some(name) = symbol {
            store::put_name(self.conn, "word", name, &outcome.cid)?;
        }

        // Leave the final result on the stack for inspection, but reset param tracking.
        self.param_types.clear();
        self.accumulated_effects.clear();
        Ok(outcome.cid)
    }

    fn prim_info(&mut self, cid: &[u8; 32]) -> Result<PrimInfo> {
        if let Some(info) = self.prim_cache.get(cid) {
            return Ok(info.clone());
        }
        let info = prim::load_prim_info(self.conn, cid)?;
        self.prim_cache.insert(*cid, info.clone());
        Ok(info)
    }

    fn word_info(&mut self, cid: &[u8; 32]) -> Result<WordInfo> {
        if let Some(info) = self.word_cache.get(cid) {
            return Ok(info.clone());
        }
        let info = word::load_word_info(self.conn, cid)?;
        self.word_cache.insert(*cid, info.clone());
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prim::{self, PrimCanon};
    use crate::store;

    #[test]
    fn build_add_word() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let param_tags = [TypeTag::I64, TypeTag::I64];
        let result_tags = [TypeTag::I64];
        let prim = PrimCanon {
            params: &param_tags,
            results: &result_tags,
            attrs: &[],
            effects: &[],
        };
        let prim_outcome = prim::store_prim(&conn, &prim)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?; // no params when using literals directly

        builder.push_lit_i64(4)?;
        builder.push_lit_i64(5)?;
        builder.apply_prim(prim_outcome.cid)?;

        let word_cid = builder.finish_word(&[], &[TypeTag::I64], Some("demo/add"))?;

        let stored_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM object WHERE kind = 'word'",
            [],
            |row| row.get(0),
        )?;
        assert!(stored_count >= 1);

        let registered: Vec<u8> = conn.query_row(
            "SELECT cid FROM name_index WHERE scope = 'word' AND name = 'demo/add'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(registered.as_slice(), &word_cid);
        Ok(())
    }

    #[test]
    fn dup_swap_over_are_wiring_only() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?;

        builder.push_lit_i64(1)?;
        builder.push_lit_i64(2)?;
        builder.dup()?;
        builder.swap()?;
        builder.over()?;

        assert_eq!(builder.depth(), 4);
        Ok(())
    }
}
