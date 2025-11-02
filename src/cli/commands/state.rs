use std::path::Path;

use anyhow::{Result, anyhow};

use super::util::{list_scope, require_store_path};
use crate::cli::StateCommand;
use march5::global_store::{self, store_snapshot};
use march5::{cid, get_name, open_store, put_name};

pub(crate) fn cmd_state(store: Option<&Path>, command: StateCommand) -> Result<()> {
    match command {
        StateCommand::Snapshot => {
            let snapshot = global_store::snapshot();
            if snapshot.is_empty() {
                println!("(empty)");
            } else {
                for (key, value) in snapshot.iter() {
                    println!("{key} = {value}");
                }
            }
        }
        StateCommand::Reset => {
            global_store::reset();
            println!("global store cleared");
        }
        StateCommand::Save { name } => {
            let store_path = require_store_path(store)?;
            let conn = open_store(store_path)?;
            let snapshot = global_store::snapshot();
            let outcome = store_snapshot(&conn, &snapshot)?;
            let cid_hex = cid::to_hex(&outcome.cid);
            if let Some(name) = name {
                put_name(&conn, "gstate", &name, &outcome.cid)?;
                println!("stored global snapshot `{name}` with cid {cid_hex}");
            } else {
                println!("stored global snapshot with cid {cid_hex}");
            }
        }
        StateCommand::Load { name } => {
            let store_path = require_store_path(store)?;
            let conn = open_store(store_path)?;
            let cid = get_name(&conn, "gstate", &name)?
                .ok_or_else(|| anyhow!("snapshot `{name}` not found"))?;
            let snapshot = global_store::load_snapshot(&conn, &cid)?;
            global_store::restore(snapshot);
            println!("restored global snapshot `{name}`");
        }
        StateCommand::List { prefix } => {
            let store_path = require_store_path(store)?;
            let conn = open_store(store_path)?;
            list_scope(&conn, "gstate", prefix.as_deref(), "no saved snapshots")?;
        }
    }
    Ok(())
}
