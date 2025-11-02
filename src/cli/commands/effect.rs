use std::path::Path;

use anyhow::Result;

use crate::cli::EffectCommand;
use march5::effect::{self, EffectCanon};
use march5::{cid, open_store};

pub(crate) fn cmd_effect(store: &Path, command: EffectCommand) -> Result<()> {
    match command {
        EffectCommand::Add { name, doc } => {
            let conn = open_store(store)?;
            let spec = EffectCanon {
                name: &name,
                doc: doc.as_deref(),
            };
            let outcome = effect::store_effect(&conn, &spec)?;
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored effect `{name}` with cid {cid_hex}");
            } else {
                println!("effect `{name}` already present with cid {cid_hex}");
            }
        }
    }
    Ok(())
}
