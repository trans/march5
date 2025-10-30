//! Shared helpers for encoding type signatures across objects.

//! Shared helpers and enums for encoding type signatures across March objects.

use anyhow::{Result, bail};

use crate::cbor::{push_array, push_map, push_text};

pub type EffectMask = u32;

pub mod effect_mask {
    use super::EffectMask;

    pub const NONE: EffectMask = 0;
    pub const IO: EffectMask = 1 << 0;
    pub const STATE_READ: EffectMask = 1 << 1;
    pub const STATE_WRITE: EffectMask = 1 << 2;
    pub const TEST: EffectMask = 1 << 3;
    pub const METRIC: EffectMask = 1 << 4;
}

#[inline]
pub fn mask_has(mask: EffectMask, flag: EffectMask) -> bool {
    (mask & flag) != 0
}

/// Compact representation of common Mini-INet type atoms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeTag {
    I64,
    F64,
    Ptr,
    Unit,
    Token,
    StateToken,
    IoToken,
}

impl TypeTag {
    /// Return the canonical string atom for this type.
    pub fn as_atom(self) -> &'static str {
        match self {
            TypeTag::I64 => "i64",
            TypeTag::F64 => "f64",
            TypeTag::Ptr => "ptr",
            TypeTag::Unit => "unit",
            TypeTag::Token => "token",
            TypeTag::StateToken => "state.token",
            TypeTag::IoToken => "io.token",
        }
    }

    /// Parse a canonical atom into a `TypeTag`.
    pub fn from_atom(atom: &str) -> Result<TypeTag> {
        match atom {
            "i64" => Ok(TypeTag::I64),
            "f64" => Ok(TypeTag::F64),
            "ptr" => Ok(TypeTag::Ptr),
            "unit" => Ok(TypeTag::Unit),
            "token" => Ok(TypeTag::Token),
            "state.token" => Ok(TypeTag::StateToken),
            "io.token" => Ok(TypeTag::IoToken),
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
