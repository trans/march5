//! Core March α₅ library primitives shared by the CLI and, eventually, the Forth surface.

pub mod builder;
pub mod cbor;
pub mod cid;
pub mod effect;
pub mod exec;
pub mod iface;
pub mod interp;
pub mod namespace;
pub mod node;
pub mod prim;
pub mod store;
pub mod types;
pub mod word;

pub type Result<T> = anyhow::Result<T>;

pub use builder::GraphBuilder;
pub use effect::{EffectCanon, EffectStoreOutcome};
pub use iface::{IfaceCanon, IfaceStoreOutcome, IfaceSymbol};
pub use interp::run_word_i64;
pub use namespace::{NamespaceCanon, NamespaceExport, NamespaceStoreOutcome};
pub use node::{NodeCanon, NodeInput, NodeKind, NodePayload, NodeStoreOutcome};
pub use prim::{PrimCanon, PrimInfo, PrimStoreOutcome};
pub use store::{
    create_store, derive_db_path, ensure_parent_dirs, get_name, list_names_for_cid,
    load_object_cbor, open_store, put_name,
};
pub use types::TypeTag;
pub use word::{WordCanon, WordStoreOutcome};
