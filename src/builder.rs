//! Stack-based graph builder inspired by Forth evaluation order.
//!
//! The builder maintains a stack of `(cid, port, type)` pairs, seeds `ARG`
//! nodes for parameters, and materialises `node`/`word` objects on demand.
//! It uses compact `TypeTag` enums to avoid string drift and keeps track of
//! wiring-only stack operations (`dup`, `swap`, `over`). Declared effects from
//! primitives and words are accumulated so resulting nodes inherit effect sets.

use std::collections::{BTreeMap, HashMap};

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
    effect_frontier: BTreeMap<[u8; 32], StackItem>,
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
            effect_frontier: BTreeMap::new(),
        }
    }

    /// Start assembling a word with the given parameter types, seeding ARG nodes.
    pub fn begin_word(&mut self, params: &[TypeTag]) -> Result<()> {
        self.stack.clear();
        self.param_types = params.to_vec();
        self.accumulated_effects.clear();
        self.effect_frontier.clear();

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

        let kind = match &payload {
            NodePayload::Prim(..) => NodeKind::Prim,
            NodePayload::Word(..) => NodeKind::Call,
            _ => unreachable!("apply_general: Prim/Word only"),
        };

        let out_atoms: Vec<String> = results.iter().map(|t| t.as_atom().to_string()).collect();
        let ty_field = if out_atoms.len() == 1 {
            Some(out_atoms[0].clone())
        } else {
            None
        };

        let node = NodeCanon {
            kind,
            ty: ty_field,
            out: out_atoms,
            inputs: inputs.into_vec(),
            vals: Vec::new(),
            deps: Vec::new(),
            effects: effects.to_vec(),
            payload,
        };

        let outcome = node::store_node(self.conn, &node)?;
        self.accumulated_effects.extend(effects.iter().copied());
        for (port, ty) in results.iter().copied().enumerate() {
            self.stack.push(StackItem {
                cid: outcome.cid,
                port: port as u32,
                ty,
            });
        }
        for effect in effects {
            self.effect_frontier.insert(
                *effect,
                StackItem {
                    cid: outcome.cid,
                    port: 0,
                    ty: TypeTag::Unit,
                },
            );
        }
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

        if self.stack.len() != results.len() {
            bail!(
                "word must leave exactly {} result(s); stack has {}",
                results.len(),
                self.stack.len()
            );
        }

        let mut vals = Vec::with_capacity(results.len());
        for (idx, expected) in results.iter().enumerate() {
            let item = self
                .stack
                .get(self.stack.len() - 1 - idx)
                .ok_or_else(|| anyhow!("stack underflow while collecting results"))?;
            if item.ty != *expected {
                bail!(
                    "result {} type mismatch: expected {:?}, got {:?}",
                    idx,
                    expected,
                    item.ty
                );
            }
            vals.push(NodeInput {
                cid: item.cid,
                port: item.port,
            });
        }
        let mut deps: Vec<NodeInput> = self
            .effect_frontier
            .values()
            .map(|item| NodeInput {
                cid: item.cid,
                port: item.port,
            })
            .collect();
        deps.sort_by(|a, b| match a.cid.cmp(&b.cid) {
            std::cmp::Ordering::Equal => a.port.cmp(&b.port),
            other => other,
        });
        deps.dedup_by(|a, b| a.cid == b.cid && a.port == b.port);

        self.accumulated_effects.sort_unstable();
        self.accumulated_effects.dedup();

        let out_types: Vec<String> = results.iter().map(|t| t.as_atom().to_string()).collect();

        let return_node = NodeCanon {
            kind: NodeKind::Return,
            ty: None,
            out: out_types.clone(),
            inputs: Vec::new(),
            vals,
            deps,
            effects: Vec::new(),
            payload: NodePayload::Return,
        };
        let return_outcome = node::store_node(self.conn, &return_node)?;

        let word = WordCanon {
            root: return_outcome.cid,
            params: params.iter().map(|t| t.as_atom().to_string()).collect(),
            results: out_types,
            effects: self.accumulated_effects.clone(),
        };
        let outcome = word::store_word(self.conn, &word)?;
        if let Some(name) = symbol {
            store::put_name(self.conn, "word", name, &outcome.cid)?;
        }

        // Leave the final results on the stack for inspection, but reset tracking.
        self.param_types.clear();
        self.accumulated_effects.clear();
        self.effect_frontier.clear();
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

    #[test]
    fn finish_void_word_creates_return() -> Result<()> {
        use serde_cbor::Value;

        let conn = Connection::open_in_memory()?;
        store::install_schema(&conn)?;

        let mut builder = GraphBuilder::new(&conn);
        builder.begin_word(&[])?;

        let word_cid = builder.finish_word(&[], &[], Some("demo/void"))?;

        let info = crate::word::load_word_info(&conn, &word_cid)?;
        assert!(info.results.is_empty());

        let (_kind, cbor) = crate::load_object_cbor(&conn, &info.root)?;
        let value: Value = serde_cbor::from_slice(&cbor)?;
        let mut map = std::collections::BTreeMap::new();
        if let Value::Map(entries) = value {
            for (k, v) in entries {
                if let Value::Text(key) = k {
                    map.insert(key, v);
                }
            }
        } else {
            bail!("RETURN node did not encode as map");
        }

        assert_eq!(map.get("nk"), Some(&Value::Text("RETURN".to_string())));
        assert_eq!(map.get("out"), Some(&Value::Array(Vec::new())));
        assert_eq!(map.get("vals"), Some(&Value::Array(Vec::new())));

        Ok(())
    }
}
