//! Contents of the caBLE v2 QR code ("hybrid" transport), as Chromium's device/fido/cable/v2_handshake.cc builds it.

use p256::SecretKey;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use rand_core::OsRng;

use super::cbor::{Cbor, encode_cbor};
use crate::rand_bytes;

/// Number of tunnel server domains this client knows (cable.ua5v.com and cable.auth.com).
const KNOWN_DOMAINS: i128 = 2;
pub const SECRET_SIZE: usize = 16;

#[derive(Clone)]
pub struct QrKeys {
    /// Identity key pair of the session; the private key never leaves this process.
    pub identity: SecretKey,
    /// Compressed public key (33 bytes).
    pub public_key: [u8; 33],
    pub secret: [u8; SECRET_SIZE],
}

impl std::fmt::Debug for QrKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QrKeys(..)")
    }
}

/// Fresh identity key (P-256) and QR secret.
pub fn generate_qr_keys() -> QrKeys {
    let identity = SecretKey::random(&mut OsRng);
    let mut public_key = [0u8; 33];
    public_key.copy_from_slice(identity.public_key().to_encoded_point(true).as_bytes());
    QrKeys { identity, public_key, secret: rand_bytes::<SECRET_SIZE>() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    Get,
    Create,
}

impl RequestType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Create => "create",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "get" => Some(Self::Get),
            "create" => Some(Self::Create),
            _ => None,
        }
    }
}

/// The CBOR map carried by the code. Key 4: supports linking (not supported). Key 5: request type hint.
pub fn qr_cbor(public_key: &[u8; 33], secret: &[u8; 16], ty: RequestType, now_ms: u64) -> Vec<u8> {
    let map = Cbor::map(vec![
        (Cbor::int(0), Cbor::bytes(public_key)),
        (Cbor::int(1), Cbor::bytes(secret)),
        (Cbor::int(2), Cbor::Int(KNOWN_DOMAINS)),
        (Cbor::int(3), Cbor::int(now_ms / 1000)),
        (Cbor::int(4), Cbor::Bool(false)),
        (Cbor::int(5), Cbor::text(if ty == RequestType::Get { "ga" } else { "mc" })),
    ]);
    encode_cbor(&map).unwrap_or_default()
}

/// Digits per partial chunk of 0 to 6 bytes.
const PARTIAL_DIGITS: [usize; 7] = [0, 3, 5, 8, 10, 13, 15];

/// 7-byte chunks read as little-endian integers, 17 decimal digits each; the last partial chunk gets fewer digits.
pub fn encode_qr_digits(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(7) {
        let n = chunk.iter().rev().fold(0u64, |acc, b| (acc << 8) | *b as u64);
        let width = if chunk.len() == 7 { 17 } else { PARTIAL_DIGITS[chunk.len()] };
        out.push_str(&format!("{n:0width$}"));
    }
    out
}

/// Text to encode in the QR code.
pub fn qr_text(keys: &QrKeys, ty: RequestType, now_ms: u64) -> String {
    format!("FIDO:/{}", encode_qr_digits(&qr_cbor(&keys.public_key, &keys.secret, ty, now_ms)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inverse of encode_qr_digits, written independently.
    fn decode_digits(digits: &str) -> Vec<u8> {
        let partial = |n: usize| match n {
            3 => 1,
            5 => 2,
            8 => 3,
            10 => 4,
            13 => 5,
            15 => 6,
            _ => panic!("bad width"),
        };
        let mut out = vec![];
        let mut i = 0;
        while i < digits.len() {
            let size = 17.min(digits.len() - i);
            let bytes = if size == 17 { 7 } else { partial(size) };
            let mut n: u64 = digits[i..i + size].parse().unwrap();
            for _ in 0..bytes {
                out.push((n & 0xff) as u8);
                n >>= 8;
            }
            i += size;
        }
        out
    }

    #[test]
    fn known_vectors() {
        assert_eq!(encode_qr_digits(&[1]), "001");
        assert_eq!(encode_qr_digits(&[1, 2, 3, 4, 5, 6, 7]), "01976943448883713");
        assert_eq!(encode_qr_digits(&[]), "");
    }

    #[test]
    fn digit_counts_for_1_to_14_bytes() {
        let expected = [3, 5, 8, 10, 13, 15, 17, 20, 22, 25, 27, 30, 32, 34];
        for n in 1..=14usize {
            let bytes: Vec<u8> = (0..n).map(|i| 255 - i as u8).collect();
            let digits = encode_qr_digits(&bytes);
            assert_eq!(digits.len(), expected[n - 1], "{n} bytes");
            assert_eq!(decode_digits(&digits), bytes);
        }
    }

    #[test]
    fn keys_are_compressed_p256_and_fresh() {
        let a = generate_qr_keys();
        assert!(a.public_key[0] == 2 || a.public_key[0] == 3);
        let b = generate_qr_keys();
        assert_ne!(a.public_key, b.public_key);
        assert_ne!(a.secret, b.secret);
    }

    #[test]
    fn text_decodes_to_the_expected_cbor_map() {
        let keys = generate_qr_keys();
        let now = 1_700_000_123_456u64;
        for (ty, hint) in [(RequestType::Get, "ga"), (RequestType::Create, "mc")] {
            let text = qr_text(&keys, ty, now);
            assert!(text.starts_with("FIDO:/"));
            assert!(text[6..].bytes().all(|b| b.is_ascii_digit()));
            let bytes = decode_digits(&text[6..]);
            assert_eq!(bytes, qr_cbor(&keys.public_key, &keys.secret, ty, now));
            let mut expected = hex::decode("a6005821").unwrap();
            expected.extend_from_slice(&keys.public_key);
            expected.extend_from_slice(&hex::decode("0150").unwrap());
            expected.extend_from_slice(&keys.secret);
            expected.extend_from_slice(&hex::decode("0202").unwrap());
            expected.extend_from_slice(&hex::decode(format!("031a{:08x}04f40562", 1_700_000_123u32)).unwrap());
            expected.extend_from_slice(hint.as_bytes());
            assert_eq!(hex::encode(bytes), hex::encode(expected));
        }
    }
}
