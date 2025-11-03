use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};
use rusqlite::Connection;

use crate::cli::commands::util::{lookup_named_cid, parse_effect_mask_flags, require_store_path};
use march5::effect::{self, EffectCanon};
use march5::global_store::{GlobalStoreSnapshot, store_snapshot};
use march5::prim::{self, PrimCanon};
use march5::types::EffectMask;
use march5::yaml::{self, CatalogItem, WordOp};
use march5::{TypeTag, Value, cid, get_name, open_store, put_name};

pub(crate) fn cmd_catalog(store: Option<&Path>, file: &Path, dry_run: bool) -> Result<()> {
    let catalog = yaml::parse_catalog_from_file(file)?;
    if dry_run {
        for (namespace, entries) in &catalog {
            for (symbol, item) in entries {
                println!(
                    "[dry-run] {namespace}/{symbol}: {}",
                    describe_catalog_item(item)
                );
            }
        }
        return Ok(());
    }

    let store_path = require_store_path(store)?;
    let conn = open_store(store_path)?;
    for (namespace, entries) in catalog {
        let mut guard_items = Vec::new();
        let mut word_items = Vec::new();
        let mut overload_items: Vec<(String, String, Vec<yaml::OverloadSpec>)> = Vec::new();
        let mut snapshot_items = Vec::new();
        for (symbol, item) in entries {
            let full_name = format!("{namespace}/{symbol}");
            match item {
                CatalogItem::Effect { doc } => {
                    let spec = EffectCanon {
                        name: &full_name,
                        doc: doc.as_deref(),
                    };
                    let outcome = effect::store_effect(&conn, &spec)?;
                    put_name(&conn, "effect", &full_name, &outcome.cid)?;
                    println!(
                        "stored effect `{full_name}` with cid {}",
                        cid::to_hex(&outcome.cid)
                    );
                }
                CatalogItem::Prim {
                    params,
                    results,
                    effects,
                    emask,
                } => {
                    let effect_mask = parse_effect_mask_flags(&emask)?;
                    let spec = PrimCanon {
                        params: &params,
                        results: &results,
                        effects: effects.as_slice(),
                        effect_mask,
                    };
                    let outcome = prim::store_prim(&conn, &spec)?;
                    put_name(&conn, "prim", &full_name, &outcome.cid)?;
                    if get_name(&conn, "prim", &symbol)?.is_none() {
                        put_name(&conn, "prim", &symbol, &outcome.cid)?;
                    }
                    println!(
                        "stored prim `{full_name}` with cid {}",
                        cid::to_hex(&outcome.cid)
                    );
                }
                CatalogItem::Guard {
                    params,
                    results,
                    stack,
                } => {
                    guard_items.push((symbol, full_name, params, results, stack));
                }
                CatalogItem::Word {
                    params,
                    results,
                    stack,
                    guards,
                } => {
                    word_items.push((symbol, full_name, params, results, stack, guards));
                }
                CatalogItem::Overloads { entries } => {
                    overload_items.push((symbol, full_name, entries));
                }
                CatalogItem::Snapshot { values } => {
                    snapshot_items.push((symbol, full_name, values));
                }
            }
        }

        for (symbol, full_name, params, results, stack) in guard_items {
            apply_guard_catalog(&conn, &full_name, &params, &results, &stack)?;
            if get_name(&conn, "guard", &symbol)?.is_none() {
                let cid = lookup_named_cid(&conn, "guard", &full_name)?;
                put_name(&conn, "guard", &symbol, &cid)?;
            }
        }

        for (_symbol, full_name, entries) in overload_items {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for entry in &entries {
                let sig = format_signature(&entry.params, &entry.results);
                let counter = counts.entry(sig.clone()).or_insert(0);
                *counter += 1;
                let derived = if *counter == 1 {
                    format!("{full_name}#{}", sig)
                } else {
                    format!("{full_name}#{}${}", sig, *counter)
                };
                apply_word_catalog(
                    &conn,
                    &derived,
                    &entry.params,
                    &entry.results,
                    &entry.stack,
                    &entry.guards,
                )?;
            }
            println!(
                "registered overload set `{full_name}` ({} entries)",
                entries.len()
            );
        }

        for (symbol, full_name, params, results, stack, guards) in word_items {
            apply_word_catalog(&conn, &full_name, &params, &results, &stack, &guards)?;
            if get_name(&conn, "word", &symbol)?.is_none() {
                let cid = lookup_named_cid(&conn, "word", &full_name)?;
                put_name(&conn, "word", &symbol, &cid)?;
            }
        }

        for (_symbol, full_name, values) in snapshot_items {
            let snapshot = GlobalStoreSnapshot::from_entries(values);
            let outcome = store_snapshot(&conn, &snapshot)?;
            put_name(&conn, "gstate", &full_name, &outcome.cid)?;
            println!(
                "stored snapshot `{full_name}` with cid {}",
                cid::to_hex(&outcome.cid)
            );
        }
    }

    Ok(())
}

fn describe_catalog_item(item: &CatalogItem) -> &'static str {
    match item {
        CatalogItem::Effect { .. } => "effect",
        CatalogItem::Prim { .. } => "prim",
        CatalogItem::Guard { .. } => "guard",
        CatalogItem::Word { .. } => "word",
        CatalogItem::Overloads { .. } => "overloads",
        CatalogItem::Snapshot { .. } => "snapshot",
    }
}

fn apply_word_catalog(
    conn: &Connection,
    full_name: &str,
    params: &[TypeTag],
    results: &[TypeTag],
    stack: &[WordOp],
    guards: &[String],
) -> Result<()> {
    let mut builder = march5::GraphBuilder::new(conn);
    builder.begin_word(params)?;
    for guard_name in guards {
        let cid = lookup_named_cid(conn, "guard", guard_name)?;
        builder.attach_guard(cid);
    }
    apply_stack_ops(&mut builder, conn, full_name, stack)?;
    let word_cid = builder.finish_word(params, results, Some(full_name))?;
    put_name(conn, "word", full_name, &word_cid)?;
    println!(
        "stored word `{full_name}` with cid {}",
        cid::to_hex(&word_cid)
    );
    Ok(())
}

fn format_signature(params: &[TypeTag], results: &[TypeTag]) -> String {
    let left = params
        .iter()
        .map(|t| t.as_atom())
        .collect::<Vec<_>>()
        .join(",");
    let right = results
        .iter()
        .map(|t| t.as_atom())
        .collect::<Vec<_>>()
        .join(",");
    format!("{left}->{right}")
}

fn apply_guard_catalog(
    conn: &Connection,
    full_name: &str,
    params: &[TypeTag],
    results: &[TypeTag],
    stack: &[WordOp],
) -> Result<()> {
    let mut builder = march5::GraphBuilder::new(conn);
    builder.begin_guard(params)?;
    apply_stack_ops(&mut builder, conn, full_name, stack)?;
    let guard_cid = builder.finish_guard(params, results, Some(full_name))?;
    put_name(conn, "guard", full_name, &guard_cid)?;
    println!(
        "stored guard `{full_name}` with cid {}",
        cid::to_hex(&guard_cid)
    );
    Ok(())
}

fn apply_stack_ops(
    builder: &mut march5::GraphBuilder<'_>,
    conn: &Connection,
    full_name: &str,
    ops: &[WordOp],
) -> Result<()> {
    for op in ops {
        match op {
            WordOp::Prim(name) => {
                let cid = lookup_named_cid(conn, "prim", name)?;
                builder.apply_prim(cid)?;
            }
            WordOp::Word(name) => match lookup_named_cid(conn, "word", name) {
                Ok(cid) => {
                    builder.apply_word(cid)?;
                }
                Err(_) => {
                    apply_overloaded_symbol(builder, conn, name)?;
                }
            },
            WordOp::Dup => builder.dup()?,
            WordOp::Swap => builder.swap()?,
            WordOp::Over => builder.over()?,
            WordOp::Lit(value) => match value {
                Value::I64(n) => {
                    builder.push_lit_i64(*n)?;
                }
                other => bail!("unsupported literal in `{full_name}`: {:?}", other),
            },
            WordOp::Quote(cid_bytes) => {
                builder.quote(*cid_bytes)?;
            }
        }
    }
    Ok(())
}

fn apply_overloaded_symbol(
    builder: &mut march5::GraphBuilder<'_>,
    conn: &Connection,
    base_name: &str,
) -> Result<()> {
    let mut candidates: Vec<([u8; 32], Vec<TypeTag>, Vec<TypeTag>)> = Vec::new();
    let prefix = format!("{base_name}#");
    for entry in march5::db::list_names(conn, "word", Some(&prefix))? {
        let cid = entry.cid;
        let info = march5::word::load_word_info(conn, &cid)?;
        candidates.push((cid, info.params.clone(), info.results.clone()));
    }
    if candidates.is_empty() {
        bail!("word `{base_name}` not found and no overloads registered");
    }

    let mut matches: Vec<(
        [u8; 32],
        Vec<TypeTag>,
        Vec<TypeTag>,
        Vec<[u8; 32]>,
        EffectMask,
    )> = Vec::new();
    for (cid, params, results) in &candidates {
        let arity = params.len();
        let top_types = builder.peek_top_types(arity)?;
        if *params == top_types {
            let info = march5::word::load_word_info(conn, cid)?;
            matches.push((
                *cid,
                params.clone(),
                results.clone(),
                info.guards.clone(),
                info.effect_mask,
            ));
        }
    }
    if matches.is_empty() {
        bail!("no overload of `{base_name}` matches top-of-stack types");
    }
    if matches.len() == 1 && matches[0].3.is_empty() {
        builder.apply_word(matches[0].0)?;
        return Ok(());
    }
    let mut specs: Vec<march5::builder::DispatchSpec<'_>> = Vec::with_capacity(matches.len());
    for (word, params, results, guards, effect_mask) in &matches {
        specs.push(march5::builder::DispatchSpec {
            word: *word,
            params,
            results,
            guards,
            effect_mask: *effect_mask,
        });
    }
    builder.apply_dispatch(&specs)?;
    Ok(())
}
