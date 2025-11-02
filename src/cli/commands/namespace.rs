use std::path::Path;

use anyhow::Result;

use super::util::{list_scope, parse_cid_list, parse_exports, show_named_object};
use crate::cli::NamespaceCommand;
use march5::iface;
use march5::namespace::{self, NamespaceCanon, NamespaceExport};
use march5::{cid, open_store, put_name};

pub(crate) fn cmd_namespace(store: &Path, command: NamespaceCommand) -> Result<()> {
    match command {
        NamespaceCommand::Add {
            name,
            iface,
            imports,
            exports,
            no_register,
        } => {
            let conn = open_store(store)?;
            let imports = parse_cid_list(imports.iter().map(|s| s.as_str()))?;
            let export_pairs = parse_exports(&exports)?;
            let iface_cid = if let Some(iface_hex) = iface {
                cid::from_hex(&iface_hex)?
            } else {
                let iface_canon = iface::derive_from_exports(&conn, &export_pairs)?;
                iface::store_iface(&conn, &iface_canon)?.cid
            };
            let exports = export_pairs
                .iter()
                .map(|(name, word_cid)| NamespaceExport {
                    name: name.clone(),
                    word: *word_cid,
                })
                .collect();
            let ns = NamespaceCanon {
                imports,
                exports,
                iface: iface_cid,
            };
            let outcome = namespace::store_namespace(&conn, &ns)?;
            if !no_register {
                if let Some(name) = &name {
                    put_name(&conn, "namespace", name, &outcome.cid)?;
                }
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored namespace with cid {cid_hex}");
            } else {
                println!("namespace already present with cid {cid_hex}");
            }
        }
        NamespaceCommand::List { prefix } => {
            let conn = open_store(store)?;
            list_scope(
                &conn,
                "namespace",
                prefix.as_deref(),
                "no namespaces registered",
            )?;
        }
        NamespaceCommand::Show { name } => {
            let conn = open_store(store)?;
            show_named_object(&conn, "namespace", "namespace", &name)?;
        }
    }
    Ok(())
}
