//! IMAP's modified UTF-7 for mailbox names (RFC 3501 §5.1.3).
//!
//! Printable ASCII stands for itself, except `&`, which starts a run of
//! UTF-16BE in base64 (with `,` instead of `/`, no padding) ended by `-`.
//! `&-` is a literal `&`.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";

/// Decode a mailbox name as the server sent it. Names that are not valid
/// modified UTF-7 (some servers send raw UTF-8) are read as UTF-8, lossily.
pub fn decode(raw: &[u8]) -> String {
    try_decode(raw).unwrap_or_else(|| String::from_utf8_lossy(raw).into_owned())
}

fn try_decode(raw: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'&' => {
                let end = i + 1 + raw[i + 1..].iter().position(|&b| b == b'-')?;
                let run = &raw[i + 1..end];
                if run.is_empty() {
                    out.push('&');
                } else {
                    let bytes = base64_decode(run)?;
                    if bytes.len() % 2 != 0 {
                        return None;
                    }
                    let units = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_be_bytes(*pair));
                    for c in char::decode_utf16(units) {
                        out.push(c.ok()?);
                    }
                }
                i = end + 1;
            }
            byte @ 0x20..=0x7e => {
                out.push(byte as char);
                i += 1;
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Encode a name for the server.
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut pending: Vec<u16> = Vec::new();
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if pending.is_empty() {
            return;
        }
        let bytes: Vec<u8> = pending.iter().flat_map(|unit| unit.to_be_bytes()).collect();
        out.push('&');
        out.push_str(&base64_encode(&bytes));
        out.push('-');
        pending.clear();
    };
    for c in name.chars() {
        match c {
            '&' => {
                flush(&mut pending, &mut out);
                out.push_str("&-");
            }
            ' '..='~' => {
                flush(&mut pending, &mut out);
                out.push(c);
            }
            other => {
                let mut units = [0u16; 2];
                pending.extend_from_slice(other.encode_utf16(&mut units));
            }
        }
    }
    flush(&mut pending, &mut out);
    out
}

fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0);
    for &byte in input {
        let value = ALPHABET.iter().position(|&a| a == byte)? as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    // Leftover bits must be zero padding.
    (buffer & ((1 << bits) - 1) == 0).then_some(out)
}

fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let (mut buffer, mut bits) = (0u32, 0);
    for &byte in input {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 6 {
            bits -= 6;
            out.push(ALPHABET[((buffer >> bits) & 0x3f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (6 - bits)) & 0x3f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_3501_example() {
        let wire = "~peter/mail/&U,BTFw-/&ZeVnLIqe-";
        assert_eq!(decode(wire.as_bytes()), "~peter/mail/台北/日本語");
        assert_eq!(encode("~peter/mail/台北/日本語"), wire);
    }

    #[test]
    fn ampersands_accents_and_emoji() {
        for (name, wire) in [
            ("Tom & Jerry", "Tom &- Jerry"),
            ("Envoyés", "Envoy&AOk-s"),
            ("Éléments supprimés", "&AMk-l&AOk-ments supprim&AOk-s"),
            ("Ideas 💡", "Ideas &2D3coQ-"),
        ] {
            assert_eq!(encode(name), wire, "encoding {name:?}");
            assert_eq!(decode(wire.as_bytes()), name, "decoding {wire:?}");
        }
    }

    #[test]
    fn invalid_input_falls_back_to_utf8() {
        assert_eq!(decode("Envoyés".as_bytes()), "Envoyés", "raw UTF-8 from a lax server");
        assert_eq!(decode(b"&unterminated"), "&unterminated");
        assert_eq!(decode(b"&*-"), "&*-", "not base64");
    }

    #[test]
    fn round_trips() {
        for name in ["INBOX", "", "a&b&&c", "日本語 and English", "Ünïcödé/Ordner", "🙂🙃"] {
            assert_eq!(decode(encode(name).as_bytes()), name);
        }
    }
}
