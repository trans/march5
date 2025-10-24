/// CBOR utility helpers for canonical encoders.
pub fn push_map(buf: &mut Vec<u8>, len: u64) {
    push_header(buf, 5, len);
}

pub fn push_array(buf: &mut Vec<u8>, len: u64) {
    push_header(buf, 4, len);
}

pub fn push_text(buf: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    push_header(buf, 3, bytes.len() as u64);
    buf.extend_from_slice(bytes);
}

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
