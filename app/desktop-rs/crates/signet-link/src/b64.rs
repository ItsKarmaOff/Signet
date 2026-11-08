//! base64url (RFC 4648 section 5, no padding) and UTF-8 helpers, strict like the TS ones.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

pub fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// None if the text is not valid base64url (no padding, no other alphabet).
pub fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).ok()
}

pub fn encode_text(text: &str) -> String {
    encode(text.as_bytes())
}

pub fn decode_text(text: &str) -> Option<String> {
    String::from_utf8(decode(text)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_and_decodes_bytes() {
        for len in [0usize, 1, 2, 3, 4, 5, 32] {
            let bytes: Vec<u8> = (0..len).map(|i| ((i * 37 + 250) & 0xff) as u8).collect();
            assert_eq!(decode(&encode(&bytes)).unwrap(), bytes);
        }
        assert_eq!(encode(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn keeps_utf8_text() {
        let t = "Ordinateur de Zoé 💻 澪";
        assert_eq!(decode_text(&encode_text(t)).unwrap(), t);
    }

    #[test]
    fn rejects_characters_outside_the_alphabet() {
        assert!(decode("ab+/").is_none());
        assert!(decode("a").is_none());
    }
}
