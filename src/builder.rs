//! Stack-based graph builder inspired by Forth evaluation order.
//!
//! The builder maintains a stack of `(cid, port, type)` pairs, seeds `ARG`
//! nodes for parameters, and materialises `node`/`word` objects on demand.
//! It uses compact `TypeTag` enums to avoid string drift and keeps track of
//! wiring-only stack operations (`dup`, `swap`, `over`). Declared effects from
//! primitives and words are accumulated so resulting nodes inherit effect sets.

use std::collections::HashMap;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use smallvec::SmallVec;

use crate::node::{self, NodeCanon, NodeInput, NodeKind, NodePayload};
use crate::prim::{self, PrimInfo};
use crate::store;
use crate::types::TypeTag;
use crate::word::{self, WordCanon, WordInfo};

/// Stack items track the producer CID, output port, and type.
#[derive(Copy, Clone, Debug)]
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
    accumulated_effects: Vec<[u8; 32]>,
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
            accumulated_effects: Vec::new(),
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
                ty: Some(ty.as_atom().to_string()),
                out: vec![ty.as_atom().to_string()],
                inputs: Vec::new(),
                vals: Vec::new(),
                deps: Vec::new(),
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
            ty: Some(TypeTag::I64.as_atom().to_string()),
            out: vec![TypeTag::I64.as_atom().to_string()],
            inputs: Vec::new(),
            vals: Vec::new(),
            deps: Vec::new(),
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

    /// Wire-only DROP: (x -- )
    pub fn drop(&mut self) -> Result<()> {
        self.stack
            .pop()
            .ok_or_else(|| anyhow!("stack underflow: drop"))?;
        Ok(())
    }

    /// Wire-only NIP: (x y -- y)
    pub fn nip(&mut self) -> Result<()> {
        let n = self.stack.len();
        if n < 2 {
            bail!("stack underflow: nip");
        }
        self.stack.remove(n - 2);
        Ok(())
    }

    /// Wire-only TUCK: (x y -- y x y)
    pub fn tuck(&mut self) -> Result<()> {
        let n = self.stack.len();
        if n < 2 {
            bail!("stack underflow: tuck");
        }
        let item = self.stack[n - 2];
        self.stack.push(item);
        Ok(())
    }

    /// Wire-only ROT: (x y z -- y z x)
    pub fn rot(&mut self) -> Result<()> {
        let n = self.stack.len();
        if n < 3 {
            bail!("stack underflow: rot");
        }
        self.stack[n - 3..].rotate_left(1);
        Ok(())
    }

    /// Wire-only -ROT: (x y z -- z x y)
    pub fn rot_minus(&mut self) -> Result<()> {
        let n = self.stack.len();
        if n < 3 {
            bail!("stack underflow: -rot");
        }
        self.stack[n - 3..].rotate_right(1);
        Ok(())
    }

    #[inline]
    fn apply_general(
        &mut self,
        arity: usize,
        params: &[TypeTag],
        results: &[TypeTag],
        effects: &[[u8; 32]],
        payload: NodePayload,
    ) -> Result<[u8; 32]> {
        let popped = self.pop_n(arity)?;

        // Fast-path type check over the hot loop.
        for (i, (expected, actual)) in params.iter().zip(popped.iter()).enumerate() {
            if expected != &actual.ty {
                bail!(
                    "argument {i} type mismatch: expected {:?}, got {:?}",
                    expected,
                    actual.ty
                );
            }
        }

        let inputs: SmallVec<[NodeInput; 8]> = popped
            .iter()
            .map(|item| NodeInput {
                cid: item.cid,
                port: item.port,
            })
            .collect();

        let out_ty = *results
            .get(0)
            .ok_or_else(|| anyhow!("callee must have \u{2265}1 result"))?;

        let kind = match &payload {
            NodePayload::Prim(..) => NodeKind::Prim,
            NodePayload::Word(..) => NodeKind::Call,
            _ => unreachable!("apply_general: Prim/Word only"),
        };

        let node = NodeCanon {
            kind,
            ty: Some(out_ty.as_atom().to_string()),
            out: vec![out_ty.as_atom().to_string()],
            inputs: inputs.into_vec(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: effects.to_vec(),
            payload,
        };

        let outcome = node::store_node(self.conn, &node)?;
        self.accumulated_effects.extend(effects.iter().copied());
        self.stack.push(StackItem {
            cid: outcome.cid,
            port: 0,
            ty: out_ty,
        });
        Ok(outcome.cid)
    }

    /// Apply a primitive by CID, deriving arity/result/effects from storage.
    pub fn apply_prim(&mut self, prim_cid: [u8; 32]) -> Result<[u8; 32]> {
        let info = self.prim_info(&prim_cid)?;
        self.apply_general(
            info.params.len(),
            &info.params,
            &info.results,
            &info.effects,
            NodePayload::Prim(prim_cid),
        )
    }

    /// Apply a word call using metadata loaded from storage.
    pub fn apply_word(&mut self, word_cid: [u8; 32]) -> Result<[u8; 32]> {
        let info = self.word_info(&word_cid)?;
        self.apply_general(
            info.params.len(),
            &info.params,
            &info.results,
            &info.effects,
            NodePayload::Word(word_cid),
        )
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

        self.accumulated_effects.sort_unstable();
        self.accumulated_effects.dedup();

        let word = WordCanon {
            root: root.cid,
            params: params.iter().map(|t| t.as_atom().to_string()).collect(),
            results: results.iter().map(|t| t.as_atom().to_string()).collect(),
            effects: self.accumulated_effects.clone(),
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

    #[inline]
    fn pop_n(&mut self, count: usize) -> Result<SmallVec<[StackItem; 8]>> {
        if self.stack.len() < count {
            bail!(
                "stack underflow: need {count} value(s), have {}",
                self.stack.len()
            );
        }
        let mut out = SmallVec::<[StackItem; 8]>::with_capacity(count);
        for _ in 0..count {
            let item = *self.stack.last().expect("checked length");
            self.stack.pop();
            out.push(item);
        }
        out.reverse();
        Ok(out)
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

    #[test]
    fn drop_nip_tuck_rot_variants() -> Result<()> {
        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?;

        let cid1 = builder.push_lit_i64(1)?;
        let cid2 = builder.push_lit_i64(2)?;
        builder.push_lit_i64(3)?;

        builder.drop()?;
        assert_eq!(builder.stack.len(), 2);
        assert_eq!(builder.stack[0].cid, cid1);
        assert_eq!(builder.stack[1].cid, cid2);

        let cid4 = builder.push_lit_i64(4)?;
        builder.nip()?;
        assert_eq!(builder.stack.len(), 2);
        assert_eq!(builder.stack[0].cid, cid1);
        assert_eq!(builder.stack[1].cid, cid4);

        builder.tuck()?;
        assert_eq!(builder.stack.len(), 3);
        assert_eq!(
            builder
                .stack
                .iter()
                .map(|item| item.cid)
                .collect::<Vec<_>>(),
            vec![cid1, cid4, cid1]
        );

        builder.rot()?;
        assert_eq!(
            builder
                .stack
                .iter()
                .map(|item| item.cid)
                .collect::<Vec<_>>(),
            vec![cid4, cid1, cid1]
        );

        builder.rot_minus()?;
        assert_eq!(
            builder
                .stack
                .iter()
                .map(|item| item.cid)
                .collect::<Vec<_>>(),
            vec![cid1, cid4, cid1]
        );
        Ok(())
    }
}
