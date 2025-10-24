pub mod cid;
pub mod effect;
pub mod store;

pub type Result<T> = anyhow::Result<T>;

pub use effect::{EffectCanon, EffectStoreOutcome};
pub use store::{create_store, derive_db_path, ensure_parent_dirs, open_store};
