//! Utilities for emitting canonical CBOR fragments used across March objects.

/// Append a map header with `len` entries to `buf`.
pub fn push_map(buf: &mut Vec<u8>, len: u64) {
    push_header(buf, 5, len);
}

/// Append an array header with `len` elements to `buf`.
pub fn push_array(buf: &mut Vec<u8>, len: u64) {
    push_header(buf, 4, len);
}

/// Append a text item to `buf`.
pub fn push_text(buf: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    push_header(buf, 3, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

/// Append a byte-string item to `buf`.
pub fn push_bytes(buf: &mut Vec<u8>, bytes: &[u8]) {
    push_header(buf, 2, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

/// Append an unsigned 32-bit integer.
pub fn push_u32(buf: &mut Vec<u8>, value: u32) {
    push_unsigned(buf, value as u64);
}

/// Append a signed 64-bit integer.
pub fn push_i64(buf: &mut Vec<u8>, value: i64) {
    if value >= 0 {
        push_unsigned(buf, value as u64);
    } else {
        let magnitude = (-1 - value) as u64;
        push_header(buf, 1, magnitude);
    }
}

fn push_unsigned(buf: &mut Vec<u8>, value: u64) {
    push_header(buf, 0, value);
}

/// Append a CBOR header for the given major type and length.
pub fn push_header(buf: &mut Vec<u8>, major: u8, len: u64) {
    assert!(major < 8);
    match len {
        0..=23 => buf.push((major << 5) | (len as u8)),
        24..=0xff => {
            buf.push((major << 5) | 24);
            buf.push(len as u8);
        }
        0x100..=0xffff => {
            buf.push((major << 5) | 25);
            buf.extend_from_slice(&(len as u16).to_be_bytes());
        }
        0x1_0000..=0xffff_ffff => {
            buf.push((major << 5) | 26);
            buf.extend_from_slice(&(len as u32).to_be_bytes());
        }
        _ => {
            buf.push((major << 5) | 27);
            buf.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
}
