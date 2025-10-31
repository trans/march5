//! Shared helpers for encoding type signatures across objects.

//! Shared helpers and enums for encoding type signatures across March objects.

use anyhow::{Result, bail};
use smallvec::SmallVec;

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

/// Logical domains that drive token threading in the builder/interpreter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EffectDomain {
    Io,
    State,
    Test,
    Metric,
}

/// Translate a bitmask into the ordered set of effect domains it touches.
pub fn effect_domains(mask: EffectMask) -> SmallVec<[EffectDomain; 4]> {
    let mut domains = SmallVec::<[EffectDomain; 4]>::new();
    if mask_has(mask, effect_mask::IO) {
        domains.push(EffectDomain::Io);
    }
    if mask_has(mask, effect_mask::STATE_READ) || mask_has(mask, effect_mask::STATE_WRITE) {
        domains.push(EffectDomain::State);
    }
    if mask_has(mask, effect_mask::TEST) {
        domains.push(EffectDomain::Test);
    }
    if mask_has(mask, effect_mask::METRIC) {
        domains.push(EffectDomain::Metric);
    }
    domains
}

/// Convenience helper to map a domain back to its bit flag.
pub fn effect_mask_for_domain(domain: EffectDomain) -> EffectMask {
    match domain {
        EffectDomain::Io => effect_mask::IO,
        EffectDomain::State => effect_mask::STATE_READ | effect_mask::STATE_WRITE,
        EffectDomain::Test => effect_mask::TEST,
        EffectDomain::Metric => effect_mask::METRIC,
    }
}

/// Helper to map an effect domain into its dedicated token type tag.
pub fn token_tag_for_domain(domain: EffectDomain) -> TypeTag {
    match domain {
        EffectDomain::Io => TypeTag::IoToken,
        EffectDomain::State => TypeTag::StateToken,
        EffectDomain::Test => TypeTag::TestToken,
        EffectDomain::Metric => TypeTag::MetricToken,
    }
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
    TestToken,
    MetricToken,
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
            TypeTag::TestToken => "test.token",
            TypeTag::MetricToken => "metric.token",
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
            "test.token" => Ok(TypeTag::TestToken),
            "metric.token" => Ok(TypeTag::MetricToken),
            other => bail!("unknown type atom `{other}`"),
        }
    }

    /// Return the effect domain encoded by this token type, if any.
    pub fn token_domain(self) -> Option<EffectDomain> {
        match self {
            TypeTag::IoToken => Some(EffectDomain::Io),
            TypeTag::StateToken => Some(EffectDomain::State),
            TypeTag::TestToken => Some(EffectDomain::Test),
            TypeTag::MetricToken => Some(EffectDomain::Metric),
            _ => None,
        }
    }

    /// True when this tag represents any token type (including domainless).
    pub fn is_token(self) -> bool {
        matches!(
            self,
            TypeTag::Token
                | TypeTag::IoToken
                | TypeTag::StateToken
                | TypeTag::TestToken
                | TypeTag::MetricToken
        )
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
