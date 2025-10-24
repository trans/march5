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
