//! Base64 (RFC 4648, standard alphabet, padded) for the images the format keeps inline as
//! `data:` URIs. No dependency for twenty lines.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

fn value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

/// Decodes, ignoring whitespace and padding; `None` on a character outside the alphabet.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for &c in text.as_bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The bytes and MIME type of a `data:…;base64,…` URI.
pub fn data_uri(src: &str) -> Option<(String, Vec<u8>)> {
    let rest = src.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let mime = meta.split(';').next().unwrap_or("").to_string();
    if meta.contains(";base64") {
        Some((mime, decode(payload)?))
    } else {
        Some((mime, payload.as_bytes().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for s in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0xff, 0x00, 0x7f]] {
            assert_eq!(decode(&encode(s)).as_deref(), Some(s));
        }
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(encode(b"fo"), "Zm8=");
        let (mime, bytes) = data_uri("data:image/png;base64,Zm9v").expect("a data uri");
        assert_eq!((mime.as_str(), bytes.as_slice()), ("image/png", &b"foo"[..]));
    }
}
