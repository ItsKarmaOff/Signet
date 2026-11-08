//! Phone's BLE advert of the caBLE v2 ("hybrid") transport: decryption with the QR secret.
//! Checked against Chromium (device/fido/cable/): v2_handshake.cc (eid::Decrypt, tunnelserver::DecodeDomain),
//! v2_discovery.cc (KeysFromQRGeneratorKey, advert size), fido_ble_uuids.cc.

use aes::Aes256;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, KeyInit};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::link::store::constant_time_eq;

/// The two 16-bit service UUIDs the phone may use: FIDO (0xFFF9) and Google (0xFDE2), in 128-bit form.
pub const CABLE_UUIDS: [&str; 2] = ["0000fff9-0000-1000-8000-00805f9b34fb", "0000fde2-0000-1000-8000-00805f9b34fb"];

/// 16-byte encrypted block + 4-byte tag.
pub const ADVERT_SIZE: usize = 20;
const BLOCK: usize = 16;
const TAG: usize = 4;
/// DerivedValueType::kEIDKey.
const EID_KEY_TYPE: u32 = 1;

const ASSIGNED_DOMAINS: [&str; 2] = ["cable.ua5v.com", "cable.auth.com"];
const DOMAIN_PREFIX: &str = "caBLEv2 tunnel server domain";
const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const TLDS: [&str; 4] = ["com", "org", "net", "info"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eid {
    /// The 16 decrypted bytes.
    pub plaintext: [u8; 16],
    /// Connection nonce (10 bytes).
    pub nonce: [u8; 10],
    /// Routing id (3 bytes), sent to the tunnel server.
    pub routing_id: [u8; 3],
    pub tunnel_domain_id: u16,
    pub tunnel_domain: String,
}

pub struct EidKeys {
    pub aes: [u8; 32],
    pub hmac: [u8; 32],
}

/// AES-256 and HMAC-SHA256 keys protecting the advert: HKDF-SHA256(secret, no salt, info = uint32 LE 1), 64 bytes.
pub fn eid_keys(secret: &[u8]) -> EidKeys {
    let mut okm = [0u8; 64];
    // 64 bytes is far below the HKDF limit: expand cannot fail.
    let _ = Hkdf::<Sha256>::new(Some(&[]), secret).expand(&EID_KEY_TYPE.to_le_bytes(), &mut okm);
    let mut aes = [0u8; 32];
    let mut hmac = [0u8; 32];
    aes.copy_from_slice(&okm[..32]);
    hmac.copy_from_slice(&okm[32..]);
    EidKeys { aes, hmac }
}

/// Domain of a tunnel server id: 0 and 1 are assigned, 256 and above are hashed into a generated name, others are invalid.
pub fn tunnel_domain(id: i64) -> Option<String> {
    if !(0..=0xffff).contains(&id) {
        return None;
    }
    if (id as usize) < ASSIGNED_DOMAINS.len() {
        return Some(ASSIGNED_DOMAINS[id as usize].to_string());
    }
    if id < 256 {
        return None;
    }
    // Prefix, the id as 2 bytes little-endian, and a zero byte (Chromium hashes 3 bytes).
    let mut input = DOMAIN_PREFIX.as_bytes().to_vec();
    input.extend_from_slice(&(id as u16).to_le_bytes());
    input.push(0);
    let digest = Sha256::digest(&input);
    // First 8 bytes of the digest as a little-endian integer: 2 bits pick the TLD, the rest is written in base 32.
    let mut v = u64::from_le_bytes(digest[..8].try_into().ok()?);
    let tld = TLDS[(v & 3) as usize];
    v >>= 2;
    let mut label = String::new();
    while v != 0 {
        label.push(BASE32[(v & 31) as usize] as char);
        v >>= 5;
    }
    Some(format!("cable.{label}.{tld}"))
}

/// Decrypts the service data of the phone's advert; None if it is not ours (size, tag, reserved byte or domain).
pub fn decrypt_advert(secret: &[u8], service_data: &[u8]) -> Option<Eid> {
    if service_data.len() != ADVERT_SIZE {
        return None;
    }
    let block = &service_data[..BLOCK];
    let keys = eid_keys(secret);
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&keys.hmac).ok()?;
    mac.update(block);
    let tag = mac.finalize().into_bytes();
    if !constant_time_eq(&tag[..TAG], &service_data[BLOCK..]) {
        return None;
    }
    let cipher = Aes256::new(GenericArray::from_slice(&keys.aes));
    let mut b = GenericArray::clone_from_slice(block);
    cipher.decrypt_block(&mut b);
    let plaintext: [u8; 16] = b.into();
    if plaintext[0] != 0 {
        return None;
    }
    let id = u16::from_le_bytes([plaintext[14], plaintext[15]]);
    let domain = tunnel_domain(id as i64)?;
    Some(Eid {
        plaintext,
        nonce: plaintext[1..11].try_into().ok()?,
        routing_id: plaintext[11..14].try_into().ok()?,
        tunnel_domain_id: id,
        tunnel_domain: domain,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use aes::cipher::BlockEncrypt;

    /// An advert as the phone builds it.
    pub fn advert(secret: &[u8], plaintext: &[u8; 16]) -> Vec<u8> {
        let keys = eid_keys(secret);
        let cipher = Aes256::new(GenericArray::from_slice(&keys.aes));
        let mut b = GenericArray::clone_from_slice(plaintext);
        cipher.encrypt_block(&mut b);
        let block: [u8; 16] = b.into();
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&keys.hmac).unwrap();
        mac.update(&block);
        let mut out = block.to_vec();
        out.extend_from_slice(&mac.finalize().into_bytes()[..4]);
        out
    }

    pub fn plain(domain: u16, reserved: u8) -> [u8; 16] {
        let mut p = [0u8; 16];
        p[0] = reserved;
        for i in 0..10 {
            p[1 + i] = 0x10 + i as u8;
        }
        p[11..14].copy_from_slice(&[0xa1, 0xa2, 0xa3]);
        p[14..16].copy_from_slice(&domain.to_le_bytes());
        p
    }

    fn secret() -> Vec<u8> {
        hex::decode("000102030405060708090a0b0c0d0e0f").unwrap()
    }

    #[test]
    fn eid_keys_are_deterministic_and_distinct() {
        let a = eid_keys(&secret());
        assert_ne!(a.aes, a.hmac);
        assert_eq!(eid_keys(&secret()).aes, a.aes);
        assert_ne!(eid_keys(&[9u8; 16]).aes, a.aes);
    }

    #[test]
    fn decrypts_what_the_phone_sent() {
        let eid = decrypt_advert(&secret(), &advert(&secret(), &plain(1, 0))).unwrap();
        assert_eq!(hex::encode(eid.nonce), "10111213141516171819");
        assert_eq!(hex::encode(eid.routing_id), "a1a2a3");
        assert_eq!(eid.tunnel_domain_id, 1);
        assert_eq!(eid.tunnel_domain, "cable.auth.com");
        assert_eq!(eid.plaintext, plain(1, 0));
    }

    #[test]
    fn accepts_a_hashed_domain() {
        assert_eq!(decrypt_advert(&secret(), &advert(&secret(), &plain(266, 0))).unwrap().tunnel_domain, "cable.wufkweyy3uaxb.com");
    }

    #[test]
    fn rejects_wrong_secret_altered_data_and_wrong_size() {
        let ok = advert(&secret(), &plain(0, 0));
        assert!(decrypt_advert(&secret(), &ok).is_some());
        assert!(decrypt_advert(&[7u8; 16], &ok).is_none());
        let mut tag = ok.clone();
        tag[19] ^= 1;
        assert!(decrypt_advert(&secret(), &tag).is_none());
        let mut block = ok.clone();
        block[0] ^= 1;
        assert!(decrypt_advert(&secret(), &block).is_none());
        assert!(decrypt_advert(&secret(), &ok[..ADVERT_SIZE - 1]).is_none());
        let mut longer = ok.clone();
        longer.push(0);
        assert!(decrypt_advert(&secret(), &longer).is_none());
        assert!(decrypt_advert(&secret(), &[]).is_none());
    }

    #[test]
    fn rejects_reserved_byte_or_invalid_domain_even_with_a_good_tag() {
        assert!(decrypt_advert(&secret(), &advert(&secret(), &plain(0, 1))).is_none());
        assert!(decrypt_advert(&secret(), &advert(&secret(), &plain(2, 0))).is_none());
        assert!(decrypt_advert(&secret(), &advert(&secret(), &plain(255, 0))).is_none());
    }

    #[test]
    fn tunnel_domains() {
        assert_eq!(tunnel_domain(0).as_deref(), Some("cable.ua5v.com"));
        assert_eq!(tunnel_domain(1).as_deref(), Some("cable.auth.com"));
        for id in [2, 100, 255, -1, 65536] {
            assert_eq!(tunnel_domain(id), None, "{id}");
        }
        assert_eq!(tunnel_domain(266).as_deref(), Some("cable.wufkweyy3uaxb.com"));
        for id in [256, 65535] {
            let d = tunnel_domain(id).unwrap();
            assert!(d.starts_with("cable.") && ["com", "org", "net", "info"].iter().any(|t| d.ends_with(&format!(".{t}"))), "{d}");
        }
    }
}
