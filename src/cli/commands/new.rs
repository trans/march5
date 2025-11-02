use anyhow::Result;

use march5::{create_store, derive_db_path};

pub(crate) fn cmd_new(name: &str) -> Result<()> {
    let path = derive_db_path(name);
    let conn = create_store(&path)?;
    drop(conn);
    println!("created march database at {}", path.display());
    Ok(())
}
