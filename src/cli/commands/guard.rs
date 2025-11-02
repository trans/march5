use std::path::Path;

use anyhow::Result;

use super::util::{list_scope, show_named_object};
use crate::cli::GuardCommand;
use march5::types::effect_mask;
use march5::{cid, open_store, put_name};

pub(crate) fn cmd_guard(store: &Path, command: GuardCommand) -> Result<()> {
    match command {
        GuardCommand::Add {
            name,
            root,
            params,
            results,
            no_register,
        } => {
            let conn = open_store(store)?;
            let root_cid = cid::from_hex(&root)?;
            if !(results.len() == 1 && results[0].trim() == "i64") {
                eprintln!(
                    "warning: guards should return exactly one i64; got {:?}",
                    results
                );
            }
            let guard = march5::GuardCanon {
                root: root_cid,
                params,
                results,
                effects: Vec::new(),
                effect_mask: effect_mask::NONE,
            };
            let outcome = march5::guard::store_guard(&conn, &guard)?;
            if !no_register {
                if let Some(name) = &name {
                    put_name(&conn, "guard", name, &outcome.cid)?;
                }
            }
            let cid_hex = march5::cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored guard with cid {cid_hex}");
            } else {
                println!("guard already present with cid {cid_hex}");
            }
        }
        GuardCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(&conn, "guard", prefix.as_deref(), "no guards registered")?;
        }
        GuardCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "guard", "guard", &name)?;
        }
    }
    Ok(())
}
