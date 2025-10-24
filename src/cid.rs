//! SHA-256 based content identifiers used throughout March.

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

/// Compute the 32-byte content ID (CID) as SHA-256 of the given bytes.
pub fn compute(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Render a CID as lowercase hexadecimal for human output.
pub fn to_hex(cid: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut buf = Vec::with_capacity(64);
    for byte in cid {
        buf.push(HEX[(byte >> 4) as usize]);
        buf.push(HEX[(byte & 0x0f) as usize]);
    }
    String::from_utf8(buf).expect("hex encoding is valid UTF-8")
}

/// Parse a lowercase hexadecimal string into a 32-byte CID.
pub fn from_hex(s: &str) -> Result<[u8; 32]> {
    if s.len() != 64 {
        bail!("CID must be 64 hex characters, got length {}", s.len());
    }
    let mut out = [0u8; 32];
    let bytes = s.as_bytes();
    for i in 0..32 {
        let hi = hex_value(bytes[2 * i])
            .ok_or_else(|| anyhow::anyhow!("invalid hex digit `{}`", bytes[2 * i] as char))?;
        let lo = hex_value(bytes[2 * i + 1])
            .ok_or_else(|| anyhow::anyhow!("invalid hex digit `{}`", bytes[2 * i + 1] as char))?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

/// Convert a raw slice into a CID array, validating the length.
pub fn from_slice(data: &[u8]) -> Result<[u8; 32]> {
    if data.len() != 32 {
        bail!("CID blob must be exactly 32 bytes, got {}", data.len());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(data);
    Ok(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_hex() {
        let cid = [0xabu8; 32];
        let hex = to_hex(&cid);
        assert_eq!(hex.len(), 64);
        let parsed = from_hex(&hex).unwrap();
        assert_eq!(parsed, cid);
    }

    #[test]
    fn from_slice_checks_length() {
        assert!(from_slice(&[0u8; 31]).is_err());
        assert!(from_slice(&[0u8; 32]).is_ok());
    }
}
