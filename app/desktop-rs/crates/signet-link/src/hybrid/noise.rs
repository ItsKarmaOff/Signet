//! Noise handshake and transport encryption of the caBLE v2 ("hybrid") tunnel, initiator side (the browser).
//! Checked against Chromium (device/fido/cable/): noise.cc (Noise_KNpsk0_P256_AESGCM_SHA256, zero-padded name,
//! HKDF2 / 96-byte HKDF in MixKeyAndHash, nonce, traffic keys), v2_handshake.cc, v2_handshake.h.
//! Keys, psk and nonces are never logged.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rand_core::OsRng;
use sha2::{Digest, Sha256};

/// DerivedValueType::kTunnelID and kPSK.
const TUNNEL_ID_TYPE: u32 = 2;
const PSK_TYPE: u32 = 3;

/// Uncompressed X9.62 P-256 point (kP256X962Length).
pub const P256_POINT_SIZE: usize = 65;
/// Handshake response: ephemeral point + empty AES-GCM ciphertext (HandshakeInitiator::kResponseSize).
pub const RESPONSE_SIZE: usize = P256_POINT_SIZE + 16;
const TAG_SIZE: usize = 16;
/// Sequence numbers use 24 bits (kMaxSequence).
const MAX_SEQUENCE: u32 = (1 << 24) - 1;
/// Messages are padded to a multiple of this size.
const PADDING_GRANULARITY: usize = 32;

/// Derive(): HKDF-SHA256(secret, salt, info = uint32 little-endian of the type).
fn derive(secret: &[u8], salt: &[u8], ty: u32, out: &mut [u8]) {
    // Sizes used are tiny: expand cannot fail.
    let _ = Hkdf::<Sha256>::new(Some(salt), secret).expand(&ty.to_le_bytes(), out);
}

/// Tunnel id (16 bytes) from the QR secret: it names the tunnel next to the routing id.
pub fn tunnel_id(secret: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    derive(secret, &[], TUNNEL_ID_TYPE, &mut out);
    out
}

/// Pre-shared key (32 bytes) of a QR handshake: the QR secret salted with the decrypted advert (16 bytes).
pub fn qr_psk(secret: &[u8], eid_plaintext: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    derive(secret, eid_plaintext, PSK_TYPE, &mut out);
    out
}

/// Connect URL of the tunnel (tunnelserver::GetConnectURL: base::HexEncode is uppercase).
pub fn tunnel_url(domain: &str, routing_id: &[u8], id: &[u8]) -> String {
    format!("wss://{domain}/cable/connect/{}/{}", hex::encode_upper(routing_id), hex::encode_upper(id))
}

/// HKDF-SHA256 with the chaining key as salt and no info (the Noise HKDF of Chromium's noise.cc).
fn hkdf(ck: &[u8], ikm: &[u8], out: &mut [u8]) {
    let _ = Hkdf::<Sha256>::new(Some(ck), ikm).expand(&[], out);
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn gcm(key: &[u8]) -> Aes256Gcm {
    // Keys are always 32 bytes here.
    Aes256Gcm::new_from_slice(key).expect("32-byte key")
}

/// Noise SymmetricState for the caBLE handshake (Chromium's device::Noise).
pub struct Noise {
    ck: [u8; 32],
    h: [u8; 32],
    key: [u8; 32],
    counter: u32,
}

impl Default for Noise {
    fn default() -> Self {
        Self::new("Noise_KNpsk0_P256_AESGCM_SHA256")
    }
}

impl Noise {
    /// The protocol name is shorter than 32 bytes: zero-padded, not hashed.
    pub fn new(name: &str) -> Self {
        let mut ck = [0u8; 32];
        ck[..name.len()].copy_from_slice(name.as_bytes());
        Self { ck, h: ck, key: [0; 32], counter: 0 }
    }

    pub fn handshake_hash(&self) -> [u8; 32] {
        self.h
    }

    pub fn mix_hash(&mut self, data: &[u8]) {
        self.h = sha256(&[&self.h, data]);
    }

    fn set_key(&mut self, key: &[u8]) {
        self.key.copy_from_slice(key);
        self.counter = 0;
    }

    pub fn mix_key(&mut self, ikm: &[u8]) {
        let mut out = [0u8; 64];
        hkdf(&self.ck, ikm, &mut out);
        self.ck.copy_from_slice(&out[..32]);
        self.set_key(&out[32..]);
    }

    pub fn mix_key_and_hash(&mut self, ikm: &[u8]) {
        let mut out = [0u8; 96];
        hkdf(&self.ck, ikm, &mut out);
        self.ck.copy_from_slice(&out[..32]);
        self.mix_hash(&out[32..64]);
        self.set_key(&out[64..]);
    }

    /// 12-byte nonce: the counter as a big-endian uint32 in the first 4 bytes.
    fn nonce(&mut self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[..4].copy_from_slice(&self.counter.to_be_bytes());
        self.counter += 1;
        n
    }

    pub fn encrypt_and_hash(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = self.nonce();
        let out = gcm(&self.key).encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad: &self.h }).unwrap_or_default();
        self.mix_hash(&out);
        out
    }

    /// The plaintext, or None if the tag does not match (the hash is only updated on success).
    pub fn decrypt_and_hash(&mut self, ciphertext: &[u8]) -> Option<Vec<u8>> {
        let nonce = self.nonce();
        if ciphertext.len() < TAG_SIZE {
            return None;
        }
        let out = gcm(&self.key).decrypt(Nonce::from_slice(&nonce), Payload { msg: ciphertext, aad: &self.h }).ok()?;
        self.mix_hash(ciphertext);
        Some(out)
    }

    /// Keys for the two directions: the first is the initiator's write key (and the responder's read key).
    pub fn traffic_keys(&self) -> ([u8; 32], [u8; 32]) {
        let mut out = [0u8; 64];
        hkdf(&self.ck, &[], &mut out);
        (out[..32].try_into().unwrap_or([0; 32]), out[32..].try_into().unwrap_or([0; 32]))
    }
}

/// Transport encryption after the handshake: AES-256-GCM without additional data, one counter per direction (big-endian in
/// the last 4 bytes of the nonce), messages padded to a multiple of 32 bytes (zeros, the last byte counts them).
pub struct Crypter {
    read_key: [u8; 32],
    write_key: [u8; 32],
    read_seq: u32,
    write_seq: u32,
}

impl Crypter {
    pub fn new(read_key: [u8; 32], write_key: [u8; 32]) -> Self {
        Self { read_key, write_key, read_seq: 0, write_seq: 0 }
    }

    fn nonce(seq: u32) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[8..].copy_from_slice(&seq.to_be_bytes());
        n
    }

    /// None when the sequence space is exhausted.
    pub fn encrypt(&mut self, message: &[u8]) -> Option<Vec<u8>> {
        if self.write_seq > MAX_SEQUENCE {
            return None;
        }
        let padded = (message.len() + 1).div_ceil(PADDING_GRANULARITY) * PADDING_GRANULARITY;
        let mut plain = vec![0u8; padded];
        plain[..message.len()].copy_from_slice(message);
        plain[padded - 1] = (padded - message.len() - 1) as u8;
        let nonce = Self::nonce(self.write_seq);
        self.write_seq += 1;
        gcm(&self.write_key).encrypt(Nonce::from_slice(&nonce), plain.as_slice()).ok()
    }

    /// The unpadded plaintext, or None (bad tag, bad padding, empty); the read counter only moves on success.
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Option<Vec<u8>> {
        if self.read_seq > MAX_SEQUENCE || ciphertext.len() < TAG_SIZE {
            return None;
        }
        let nonce = Self::nonce(self.read_seq);
        let mut plain = gcm(&self.read_key).decrypt(Nonce::from_slice(&nonce), ciphertext).ok()?;
        self.read_seq += 1;
        let padding = *plain.last()? as usize;
        if padding + 1 > plain.len() {
            return None;
        }
        plain.truncate(plain.len() - padding - 1);
        Some(plain)
    }
}

pub struct HandshakeResult {
    pub crypter: Crypter,
    pub handshake_hash: [u8; 32],
}

fn ecdh(secret: &SecretKey, peer: &PublicKey) -> [u8; 32] {
    let shared = p256::ecdh::diffie_hellman(secret.to_nonzero_scalar(), peer.as_affine());
    let mut out = [0u8; 32];
    out.copy_from_slice(shared.raw_secret_bytes());
    out
}

/// Uncompressed point (what both sides mix into the hash).
pub fn uncompressed(pk: &PublicKey) -> Vec<u8> {
    pk.to_encoded_point(false).as_bytes().to_vec()
}

/// Initiator of the QR handshake, Noise KNpsk0: the phone knows our identity key from the QR code, both share the psk.
/// Message 1: ephemeral key (65 bytes) + AEAD of nothing. Message 2 (81 bytes): the phone's ephemeral key + AEAD of nothing.
pub struct HandshakeInitiator {
    noise: Noise,
    ephemeral: SecretKey,
    psk: [u8; 32],
    identity: SecretKey,
}

impl HandshakeInitiator {
    pub fn new(psk: [u8; 32], identity: SecretKey) -> Self {
        Self { noise: Noise::default(), ephemeral: SecretKey::random(&mut OsRng), psk, identity }
    }

    pub fn build_initial_message(&mut self) -> Vec<u8> {
        self.noise.mix_hash(&[1]); // prologue: 1 for a QR (KN) handshake
        self.noise.mix_hash(&uncompressed(&self.identity.public_key()));
        self.noise.mix_key_and_hash(&self.psk);
        self.ephemeral = SecretKey::random(&mut OsRng);
        let e = uncompressed(&self.ephemeral.public_key());
        self.noise.mix_hash(&e);
        self.noise.mix_key(&e);
        let mut out = e;
        out.extend_from_slice(&self.noise.encrypt_and_hash(&[]));
        out
    }

    /// None for anything that is not a valid response (size, point, tag, non-empty payload).
    pub fn process_response(&mut self, response: &[u8]) -> Option<HandshakeResult> {
        if response.len() != RESPONSE_SIZE {
            return None;
        }
        let peer_bytes = &response[..P256_POINT_SIZE];
        let ciphertext = &response[P256_POINT_SIZE..];
        let peer = PublicKey::from_sec1_bytes(peer_bytes).ok()?;
        let ee = ecdh(&self.ephemeral, &peer);
        let se = ecdh(&self.identity, &peer);
        self.noise.mix_hash(peer_bytes);
        self.noise.mix_key(peer_bytes);
        self.noise.mix_key(&ee);
        self.noise.mix_key(&se);
        let payload = self.noise.decrypt_and_hash(ciphertext)?;
        if !payload.is_empty() {
            return None;
        }
        let (write, read) = self.noise.traffic_keys();
        Some(HandshakeResult { crypter: Crypter::new(read, write), handshake_hash: self.noise.handshake_hash() })
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub struct Responded {
        pub response: Vec<u8>,
        pub crypter: Crypter,
        pub handshake_hash: [u8; 32],
    }

    /// The phone's half of a QR (KNpsk0) handshake (Chromium's RespondToHandshake); `peer_identity` is the uncompressed key from the QR code.
    pub fn respond_to_handshake(psk: &[u8], peer_identity: &[u8], input: &[u8]) -> Option<Responded> {
        if input.len() < P256_POINT_SIZE {
            return None;
        }
        let peer_bytes = &input[..P256_POINT_SIZE];
        let ciphertext = &input[P256_POINT_SIZE..];
        let mut noise = Noise::default();
        noise.mix_hash(&[1]);
        noise.mix_hash(peer_identity);
        noise.mix_key_and_hash(psk);
        noise.mix_hash(peer_bytes);
        noise.mix_key(peer_bytes);
        let peer = PublicKey::from_sec1_bytes(peer_bytes).ok()?;
        let identity_pub = PublicKey::from_sec1_bytes(peer_identity).ok()?;
        let ephemeral = SecretKey::random(&mut OsRng);
        let ee = ecdh(&ephemeral, &peer);
        let se = ecdh(&ephemeral, &identity_pub);
        let payload = noise.decrypt_and_hash(ciphertext)?;
        if !payload.is_empty() {
            return None;
        }
        let e = uncompressed(&ephemeral.public_key());
        noise.mix_hash(&e);
        noise.mix_key(&e);
        noise.mix_key(&ee);
        noise.mix_key(&se);
        let mut response = e;
        response.extend_from_slice(&noise.encrypt_and_hash(&[]));
        let (read, write) = noise.traffic_keys();
        Some(Responded { response, crypter: Crypter::new(read, write), handshake_hash: noise.handshake_hash() })
    }
}

#[cfg(test)]
mod tests {
    use super::testing::respond_to_handshake;
    use super::*;
    use hmac::{Hmac, Mac};

    fn seq(start: u8, n: usize) -> Vec<u8> {
        (0..n).map(|i| start.wrapping_add(i as u8)).collect()
    }

    /// HKDF-SHA256 from RFC 5869 with plain HMAC, independent of the hkdf crate.
    fn rfc_hkdf(ikm: &[u8], salt: &[u8], info: &[u8], len: usize) -> Vec<u8> {
        let salt = if salt.is_empty() { vec![0u8; 32] } else { salt.to_vec() };
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(&salt).unwrap();
        m.update(ikm);
        let prk = m.finalize().into_bytes();
        let mut out = vec![];
        let mut t: Vec<u8> = vec![];
        let mut i = 1u8;
        while out.len() < len {
            let mut m = <Hmac<Sha256> as Mac>::new_from_slice(&prk).unwrap();
            m.update(&t);
            m.update(info);
            m.update(&[i]);
            t = m.finalize().into_bytes().to_vec();
            out.extend_from_slice(&t);
            i += 1;
        }
        out.truncate(len);
        out
    }

    fn identity() -> SecretKey {
        SecretKey::random(&mut OsRng)
    }

    #[test]
    fn derivations() {
        let secret = seq(0, 16);
        let eid = seq(16, 16);
        assert_eq!(hex::encode(tunnel_id(&secret)), "de1d8150b45f03349b2b00a52e2e893e");
        assert_eq!(tunnel_id(&secret).to_vec(), rfc_hkdf(&secret, &[], &[2, 0, 0, 0], 16));
        assert_eq!(hex::encode(qr_psk(&secret, &eid)), "2a1db389ed56313efb6b4278d02b62d3ca0e95c434b1c83188d550dd62e3afeb");
        assert_eq!(qr_psk(&secret, &eid).to_vec(), rfc_hkdf(&secret, &eid, &[3, 0, 0, 0], 32));
    }

    #[test]
    fn connect_url_uses_uppercase_hex() {
        assert_eq!(tunnel_url("cable.wufkweyy3uaxb.com", &[0xaa, 0xbb, 0x0c], &[0; 16]), format!("wss://cable.wufkweyy3uaxb.com/cable/connect/AABB0C/{}", "00".repeat(16)));
        assert_eq!(
            tunnel_url("cable.ua5v.com", &[0xab, 0xcd, 0xef], &seq(0xa0, 16)),
            "wss://cable.ua5v.com/cable/connect/ABCDEF/A0A1A2A3A4A5A6A7A8A9AAABACADAEAF"
        );
    }

    #[test]
    fn noise_starts_from_the_zero_padded_name() {
        let mut name = [0u8; 32];
        name[..31].copy_from_slice(b"Noise_KNpsk0_P256_AESGCM_SHA256");
        assert_eq!(Noise::default().handshake_hash(), name);
    }

    #[test]
    fn noise_states_agree_and_refuse_tampering() {
        let (mut a, mut b) = (Noise::default(), Noise::default());
        for n in [&mut a, &mut b] {
            n.mix_hash(&[1, 2, 3]);
            n.mix_key_and_hash(&seq(0, 32));
            n.mix_key(&seq(9, 32));
        }
        let c = a.encrypt_and_hash(b"hello");
        let mut bad = c.clone();
        bad[0] ^= 1;
        assert!(Noise::default().decrypt_and_hash(&c).is_none());
        assert!(b.decrypt_and_hash(&bad).is_none());
    }

    #[test]
    fn noise_decrypts_and_keeps_hashes_equal() {
        let (mut a, mut b) = (Noise::default(), Noise::default());
        for n in [&mut a, &mut b] {
            n.mix_key(&seq(1, 32));
        }
        let c = a.encrypt_and_hash(b"payload");
        assert_eq!(b.decrypt_and_hash(&c).unwrap(), b"payload");
        assert_eq!(a.handshake_hash(), b.handshake_hash());
        assert_eq!(a.traffic_keys().0, b.traffic_keys().0);
    }

    fn pair() -> (Crypter, Crypter) {
        let (k1, k2): ([u8; 32], [u8; 32]) = (seq(1, 32).try_into().unwrap(), seq(100, 32).try_into().unwrap());
        (Crypter::new(k1, k2), Crypter::new(k2, k1))
    }

    fn raw_seal(key: &[u8], seq_num: u32, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; 12];
        nonce[8..].copy_from_slice(&seq_num.to_be_bytes());
        gcm(key).encrypt(Nonce::from_slice(&nonce), plain).unwrap()
    }

    #[test]
    fn crypter_pads_to_a_multiple_of_32() {
        let (mut a, mut b) = pair();
        for (len, total) in [(0usize, 32usize), (1, 32), (30, 32), (31, 32), (32, 64), (100, 128)] {
            let m = vec![7u8; len];
            let sealed = a.encrypt(&m).unwrap();
            assert_eq!(sealed.len(), total + 16);
            assert_eq!(b.decrypt(&sealed).unwrap(), m);
        }
    }

    #[test]
    fn crypter_uses_a_counter_per_direction() {
        let (mut a, mut b) = pair();
        let one = a.encrypt(b"x").unwrap();
        let two = a.encrypt(b"x").unwrap();
        assert_ne!(one, two);
        assert!(b.decrypt(&two).is_none()); // out of order
        assert_eq!(b.decrypt(&one).unwrap(), b"x");
        assert_eq!(b.decrypt(&two).unwrap(), b"x");
        let y = b.encrypt(b"y").unwrap();
        assert_eq!(a.decrypt(&y).unwrap(), b"y");
    }

    #[test]
    fn crypter_nonce_is_like_chromium() {
        let (mut a, _) = pair();
        let mut padded = vec![0u8; 32];
        padded[..3].copy_from_slice(b"abc");
        padded[31] = 28;
        let k = seq(100, 32);
        assert_eq!(a.encrypt(b"abc").unwrap(), raw_seal(&k, 0, &padded));
        assert_eq!(a.encrypt(b"abc").unwrap(), raw_seal(&k, 1, &padded));
    }

    #[test]
    fn crypter_rejects_tampered_truncated_and_badly_padded() {
        let (mut a, mut b) = pair();
        let sealed = a.encrypt(b"data").unwrap();
        let mut bad = sealed.clone();
        bad[3] ^= 0x80;
        assert!(b.decrypt(&bad).is_none());
        assert!(b.decrypt(&sealed[..10]).is_none());
        assert!(b.decrypt(&[]).is_none());
        assert_eq!(b.decrypt(&sealed).unwrap(), b"data");
        let (mut evil, _) = pair();
        let mut plain = vec![0u8; 32];
        plain[31] = 32;
        let mut e2 = Crypter::new(seq(1, 32).try_into().unwrap(), seq(100, 32).try_into().unwrap());
        assert!(e2.decrypt(&raw_seal(&seq(100, 32), 0, &plain)).is_none());
        let _ = &mut evil;
    }

    fn psk() -> [u8; 32] {
        qr_psk(&seq(0, 16), &seq(16, 16))
    }

    struct Run {
        initiator: HandshakeInitiator,
        message: Vec<u8>,
        answered: Option<testing::Responded>,
    }

    fn run(phone_psk: [u8; 32]) -> Run {
        let id = identity();
        let mut initiator = HandshakeInitiator::new(psk(), id.clone());
        let message = initiator.build_initial_message();
        let answered = respond_to_handshake(&phone_psk, &uncompressed(&id.public_key()), &message);
        Run { initiator, message, answered }
    }

    #[test]
    fn first_message_is_ephemeral_key_and_tag() {
        let r = run(psk());
        assert_eq!(r.message.len(), 65 + 16);
        assert_eq!(r.message[0], 4);
    }

    #[test]
    fn both_sides_agree_and_talk_both_ways() {
        let mut r = run(psk());
        let mut answered = r.answered.take().unwrap();
        assert_eq!(answered.response.len(), RESPONSE_SIZE);
        let mut result = r.initiator.process_response(&answered.response).unwrap();
        assert_eq!(result.handshake_hash, answered.handshake_hash);
        assert_eq!(answered.crypter.decrypt(&result.crypter.encrypt(b"to phone").unwrap()).unwrap(), b"to phone");
        assert_eq!(result.crypter.decrypt(&answered.crypter.encrypt(b"to browser").unwrap()).unwrap(), b"to browser");
    }

    #[test]
    fn another_psk_cannot_answer() {
        assert!(run(seq(1, 32).try_into().unwrap()).answered.is_none());
    }

    #[test]
    fn another_identity_cannot_answer() {
        let id = identity();
        let msg = HandshakeInitiator::new(psk(), id).build_initial_message();
        assert!(respond_to_handshake(&psk(), &uncompressed(&identity().public_key()), &msg).is_none());
    }

    #[test]
    fn tampered_response_is_refused() {
        for at in [0usize, 10, 64, 65, 80] {
            let mut r = run(psk());
            let mut bad = r.answered.take().unwrap().response;
            bad[at] ^= 1;
            assert!(r.initiator.process_response(&bad).is_none(), "byte {at}");
        }
    }

    #[test]
    fn wrong_size_or_off_curve_point_is_refused() {
        let mut r = run(psk());
        let resp = r.answered.take().unwrap().response;
        assert!(r.initiator.process_response(&resp[1..]).is_none());
        let mut longer = resp.clone();
        longer.push(0);
        assert!(r.initiator.process_response(&longer).is_none());
        let mut off = resp.clone();
        off[1..65].fill(1);
        assert!(r.initiator.process_response(&off).is_none());
    }

    #[test]
    fn replayed_response_from_another_handshake_is_refused() {
        let mut first = run(psk());
        let mut second = run(psk());
        assert!(first.initiator.process_response(&second.answered.take().unwrap().response).is_none());
    }
}
