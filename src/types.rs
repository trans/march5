//! Shared helpers for encoding type signatures across objects.

//! Shared helpers and enums for encoding type signatures across March objects.

use anyhow::{Result, bail};

use crate::cbor::{push_array, push_map, push_text};

/// Compact representation of common Mini-INet type atoms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeTag {
    I64,
    F64,
    Ptr,
    Unit,
}

impl TypeTag {
    /// Return the canonical string atom for this type.
    pub fn as_atom(self) -> &'static str {
        match self {
            TypeTag::I64 => "i64",
            TypeTag::F64 => "f64",
            TypeTag::Ptr => "ptr",
            TypeTag::Unit => "unit",
        }
    }

    /// Parse a canonical atom into a `TypeTag`.
    pub fn from_atom(atom: &str) -> Result<TypeTag> {
        match atom {
            "i64" => Ok(TypeTag::I64),
            "f64" => Ok(TypeTag::F64),
            "ptr" => Ok(TypeTag::Ptr),
            "unit" => Ok(TypeTag::Unit),
            other => bail!("unknown type atom `{other}`"),
        }
    }
}

/// Encode a type signature object `{params: [...], results: [...]}`.
pub fn encode_type_signature(buf: &mut Vec<u8>, params: &[&str], results: &[&str]) {
    push_map(buf, 2);

    push_text(buf, "params");
    push_array(buf, params.len() as u64);
    for param in params {
        push_text(buf, param);
    }

    push_text(buf, "results");
    push_array(buf, results.len() as u64);
    for result in results {
        push_text(buf, result);
    }
}
