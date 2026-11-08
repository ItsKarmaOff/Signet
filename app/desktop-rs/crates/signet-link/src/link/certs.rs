//! Self-signed ECDSA P-256 certificates (one per link, plus a default one).
//! The phone only pins the SHA-256 of the DER certificate.

use std::time::{SystemTime, UNIX_EPOCH};

use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, date_time_ymd};
use rustls_pki_types::{CertificateDer, pem::PemObject};
use sha2::{Digest, Sha256};

use super::protocol::server_name_for_link;
use crate::b64;

#[derive(Debug, Clone)]
pub struct CertPair {
    pub cert_pem: String,
    pub key_pem: String,
}

/// Civil date from days since the Unix epoch (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((y + i64::from(m <= 2)) as i32, m as u8, d as u8)
}

fn make(common_name: &str) -> Result<CertPair, rcgen::Error> {
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
    let mut params = CertificateParams::new(vec![common_name.to_string()])?;
    params.distinguished_name.push(DnType::CommonName, common_name);
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let now_days = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| (d.as_secs() / 86_400) as i64).unwrap_or(0);
    let (y, m, d) = civil(now_days - 1);
    params.not_before = date_time_ymd(y, m, d);
    let (y, m, d) = civil(now_days + 3650);
    params.not_after = date_time_ymd(y, m, d);
    let cert = params.self_signed(&key)?;
    Ok(CertPair { cert_pem: cert.pem(), key_pem: key.serialize_pem() })
}

pub fn make_link_cert(link_id: &str) -> Result<CertPair, rcgen::Error> {
    make(&server_name_for_link(link_id))
}

pub fn make_default_cert() -> Result<CertPair, rcgen::Error> {
    make("default.link.signet.invalid")
}

/// SHA-256 of the DER certificate in base64url (43 characters). None if the PEM is unreadable.
pub fn cert_sha256(cert_pem: &str) -> Option<String> {
    let der = CertificateDer::from_pem_slice(cert_pem.as_bytes()).ok()?;
    Some(b64::encode(&Sha256::digest(der.as_ref())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_a_certificate_with_a_43_char_fingerprint() {
        let c = make_link_cert("0123456789abcdef").unwrap();
        let f = cert_sha256(&c.cert_pem).unwrap();
        assert_eq!(f.len(), 43);
        assert_ne!(f, cert_sha256(&make_default_cert().unwrap().cert_pem).unwrap());
        assert!(cert_sha256("nope").is_none());
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_723), (2024, 1, 1));
    }
}
