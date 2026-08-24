//! Base64, because the wire carries file bytes that way.
//!
//! # No crate, and the refusal is recorded here
//!
//! `workspace.file-write` takes `{"encoding": "base64", "data": …}` and `workspace.file-read`
//! answers the same shape, so a client that speaks those two routes needs base64 and nothing else:
//! the standard alphabet, padded, no line breaks, no URL-safe variant, no streaming. That is forty
//! lines. The workspace rule is *prefer no dependency, and record the refusal*, and this is the
//! third time it has been applied in this crate — after the HTTP client and the digest.
//!
//! Decoding is strict. A byte outside the alphabet is an error rather than a skip: the input is a
//! file's contents as a daemon wrote them, and quietly dropping a character would hand a caller a
//! file that is not the one on disk while looking like a success.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Bytes as the wire spells them.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let block = u32::from(chunk[0]) << 16
            | u32::from(chunk.get(1).copied().unwrap_or(0)) << 8
            | u32::from(chunk.get(2).copied().unwrap_or(0));
        for index in 0..4 {
            if index <= chunk.len() {
                let shift = 18 - index * 6;
                out.push(ALPHABET[((block >> shift) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The wire's spelling, back to bytes.
///
/// # Errors
///
/// Returns the offending character when the input is not canonical base64. Strict on purpose: a
/// decoder that skipped what it did not recognise would answer a file that is not the one on disk
/// and look like it had succeeded.
pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let mut block = 0_u32;
    let mut held = 0_u32;
    for (position, character) in text.chars().enumerate() {
        if character == '=' {
            break;
        }
        let value = ALPHABET
            .iter()
            .position(|candidate| *candidate as char == character)
            .ok_or_else(|| format!("`{character}` at {position} is not base64"))?;
        // `position` in a 64-entry alphabet: the cast cannot lose anything.
        block = (block << 6) | u32::try_from(value).expect("an alphabet index fits");
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes.push(((block >> held) & 0xff) as u8);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_padding_length_round_trips() {
        // The three cases a hand-written encoder gets wrong, and the empty one that reads as a
        // success either way.
        for original in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let encoded = encode(original.as_bytes());
            assert_eq!(
                decode(&encoded).expect("decodes"),
                original.as_bytes(),
                "{original}"
            );
        }
    }

    #[test]
    fn the_spelling_is_the_one_the_wire_uses() {
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_character_outside_the_alphabet_is_an_error_and_never_a_skip() {
        // A file's contents with one byte quietly dropped is a file that is not the one on disk,
        // handed over as a success.
        let error = decode("Zm9v!mFy").expect_err("refused");
        assert!(error.contains('!'), "{error}");
        assert!(error.contains('4'), "and where it was: {error}");
    }

    #[test]
    fn bytes_that_are_not_text_survive_the_trip() {
        let original: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&original)).expect("decodes"), original);
    }
}
