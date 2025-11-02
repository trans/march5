use std::path::Path;

use anyhow::Result;

use super::util::{parse_cid_list, parse_effect_mask_flags, parse_type_tags};
use crate::cli::PrimCommand;
use march5::prim::{self, PrimCanon};
use march5::types::effect_mask;
use march5::{cid, open_store, put_name};

pub(crate) fn cmd_prim(store: &Path, command: PrimCommand) -> Result<()> {
    match command {
        PrimCommand::Add {
            name,
            params,
            results,
            effects,
            emask,
            no_register,
        } => {
            let conn = open_store(store)?;
            let param_tags = parse_type_tags(&params)?;
            let result_tags = parse_type_tags(&results)?;
            let effect_cids = parse_cid_list(effects.iter().map(|s| s.as_str()))?;
            let mut effect_mask_value = parse_effect_mask_flags(&emask)?;
            if effect_mask_value == effect_mask::NONE && !effect_cids.is_empty() {
                effect_mask_value = effect_mask::IO;
            }
            let spec = PrimCanon {
                params: &param_tags,
                results: &result_tags,
                effects: effect_cids.as_slice(),
                effect_mask: effect_mask_value,
            };
            let outcome = prim::store_prim(&conn, &spec)?;
            if !no_register {
                put_name(&conn, "prim", &name, &outcome.cid)?;
            }
            let cid_hex = cid::to_hex(&outcome.cid);
            if outcome.inserted {
                println!("stored prim `{name}` with cid {cid_hex}");
            } else {
                println!("prim `{name}` already present with cid {cid_hex}");
            }
        }
    }
    Ok(())
}
