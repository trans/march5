use std::path::Path;

use anyhow::{Result, bail};

use super::util::{list_scope, parse_cid_list, parse_effect_mask_flags, show_named_object};
use crate::cli::WordCommand;
use march5::types::effect_mask;
use march5::word::{self, WordCanon};
use march5::{cid, get_name, open_store, put_name};

pub(crate) fn cmd_word(store: &Path, command: WordCommand) -> Result<()> {
    match command {
        WordCommand::Add {
            name,
            root,
            params,
            results,
            effects,
            emask,
            guards,
            no_register,
        } => {
            let conn = open_store(store)?;
            let root_cid = cid::from_hex(&root)?;
            let effect_cids = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let mut effect_mask_value = parse_effect_mask_flags(&emask)?;
            if effect_mask_value == effect_mask::NONE && !effect_cids.is_empty() {
                effect_mask_value = effect_mask::IO;
            }
            let mut guard_cids = Vec::new();
            for g in guards {
                if g.len() == 64 && g.chars().all(|c| c.is_ascii_hexdigit()) {
                    guard_cids.push(cid::from_hex(&g)?);
                } else if let Some(cid) = get_name(&conn, "guard", &g)? {
                    guard_cids.push(cid);
                } else {
                    bail!("guard `{g}` not found in name index; use hex CID or register first");
                }
            }
            let word = WordCanon {
                root: root_cid,
                params,
                results,
                effects: effect_cids,
                effect_mask: effect_mask_value,
                guards: guard_cids,
            };
            let outcome = word::store_word(&conn, &word)?;
            if !no_register {
                if let Some(name) = &name {
                    put_name(&conn, "word", name, &outcome.cid)?;
                }
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored word with cid {cid_hex}");
            } else {
                println!("word already present with cid {cid_hex}");
            }
        }
        WordCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(&conn, "word", prefix.as_deref(), "no words registered")?;
        }
        WordCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "word", "word", &name)?;
        }
    }
    Ok(())
}
