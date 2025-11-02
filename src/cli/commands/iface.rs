use std::path::Path;

use anyhow::Result;

use super::util::{list_scope, parse_iface_spec, show_named_object};
use crate::cli::IfaceCommand;
use march5::iface::{self, IfaceCanon};
use march5::{cid, open_store, put_name};

pub(crate) fn cmd_iface(store: &Path, command: IfaceCommand) -> Result<()> {
    match command {
        IfaceCommand::Add {
            register,
            names,
            no_register,
        } => {
            let conn = open_store(store)?;
            let mut parsed = Vec::with_capacity(names.len());
            for spec in names {
                parsed.push(parse_iface_spec(&spec)?);
            }
            let iface = IfaceCanon { names: parsed };
            let outcome = iface::store_iface(&conn, &iface)?;
            if !no_register {
                if let Some(name) = &register {
                    put_name(&conn, "iface", name, &outcome.cid)?;
                }
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored iface with cid {cid_hex}");
            } else {
                println!("iface already present with cid {cid_hex}");
            }
        }
        IfaceCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(
                &conn,
                "iface",
                prefix.as_deref(),
                "no interfaces registered",
            )?;
        }
        IfaceCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "iface", "interface", &name)?;
        }
    }
    Ok(())
}
