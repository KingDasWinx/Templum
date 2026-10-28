//! TLS for the API (docs/specs/api.md, Encryption): Templum's own certificate, made once and kept
//! beside the settings, or the person's. rustls with ring's crypto, HTTP/1.1 only.

use crate::store::{folder, private_write};
use ring::digest::{SHA256, digest};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::pem::{Error as PemError, PemObject};
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// A certificate chain and the key that signs for it. No `Debug`: it holds the key.
pub struct Certificate {
    pub chain: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
}

/// How long Templum's certificate lasts, in days.
const LIFE_DAYS: i64 = 5 * 365;

/// Templum's certificate: `<data>/api/cert.pem` and `key.pem`, made when either is missing, then
/// read.
pub fn own(data: &Path) -> Result<Certificate, String> {
    let (cert, key) = (folder(data).join("cert.pem"), folder(data).join("key.pem"));
    if !(cert.exists() && key.exists()) {
        make(&cert, &key)?;
    }
    files(&cert, &key)
}

/// New files for Templum's certificate, then `own`: clients that pinned the old one stop connecting.
pub fn renew(data: &Path) -> Result<Certificate, String> {
    let folder = folder(data);
    make(&folder.join("cert.pem"), &folder.join("key.pem"))?;
    own(data)
}

/// Writes a self-signed ECDSA P-256 certificate for `localhost`, `127.0.0.1` and `::1`, valid
/// from today for five years, and its key (private to the person).
fn make(cert: &Path, key: &Path) -> Result<(), String> {
    let failed =
        |error: &dyn std::fmt::Display| format!("Templum's certificate couldn't be made: {error}");
    let pair =
        rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).map_err(|e| failed(&e))?;
    let mut params = rcgen::CertificateParams::new(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ])
    .map_err(|e| failed(&e))?;
    let today = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() / 86_400) as i64;
    let date = |days| {
        let (year, month, day) = civil(days);
        rcgen::date_time_ymd(year, month, day)
    };
    params.not_before = date(today);
    params.not_after = date(today + LIFE_DAYS);
    let certificate = params.self_signed(&pair).map_err(|e| failed(&e))?;
    private_write(key, pair.serialize_pem().as_bytes()).map_err(|e| failed(&e))?;
    private_write(cert, certificate.pem().as_bytes()).map_err(|e| failed(&e))
}

/// The calendar date of a day counted from 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted + 2) / 5 + 1) as u8;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    } as u8;
    let year = (year_of_era + era * 400) as i32 + i32::from(month <= 2);
    (year, month, day)
}

/// The person's certificate chain and private key, both PEM. An Err names the file and what is
/// wrong with it.
pub fn files(cert: &Path, key: &Path) -> Result<Certificate, String> {
    let wrong = |file: &Path, why: &dyn std::fmt::Display| format!("{}: {why}", file.display());
    let chain = CertificateDer::pem_file_iter(cert)
        .map_err(|e| wrong(cert, &e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| wrong(cert, &e))?;
    if chain.is_empty() {
        return Err(format!("{} has no certificate", cert.display()));
    }
    let key = PrivateKeyDer::from_pem_file(key).map_err(|e| match e {
        PemError::NoItemsFound => format!("{} has no private key", key.display()),
        e => wrong(key, &e),
    })?;
    Ok(Certificate { chain, key })
}

/// The SHA-256 of a certificate's DER as 32 uppercase hex pairs joined by `:`, as Node's
/// `fingerprint256` gives it: what a client pins.
pub fn fingerprint(cert: &CertificateDer) -> String {
    let pairs: Vec<String> = digest(&SHA256, cert.as_ref())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    pairs.join(":")
}

/// What a connection code pins: the fingerprint as 64 lowercase hex characters, no colons.
pub fn pin(cert: &CertificateDer) -> String {
    fingerprint(cert).replace(':', "").to_lowercase()
}

/// The server's TLS settings: safe protocol versions (1.2 and 1.3), no client certificate, HTTP/1.1.
pub fn server_config(cert: Certificate) -> Result<Arc<rustls::ServerConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(cert.chain, cert.key)
        .map_err(|e| format!("The certificate and the key don't go together: {e}"))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templums_certificate_is_made_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let a = fingerprint(&own(dir.path()).unwrap().chain[0]);
        assert_eq!(fingerprint(&own(dir.path()).unwrap().chain[0]), a);
        assert!(
            folder(dir.path()).join("cert.pem").exists()
                && folder(dir.path()).join("key.pem").exists()
        );
        assert_ne!(fingerprint(&renew(dir.path()).unwrap().chain[0]), a);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(folder(dir.path()).join("key.pem"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // One file missing: both are made again.
        std::fs::remove_file(folder(dir.path()).join("key.pem")).unwrap();
        assert!(own(dir.path()).is_ok() && folder(dir.path()).join("key.pem").exists());
    }

    #[test]
    fn the_fingerprint_is_the_sha256_of_the_der() {
        let der = CertificateDer::from(b"templum".to_vec());
        let sum = digest(&SHA256, b"templum");
        let want: Vec<String> = sum.as_ref().iter().map(|b| format!("{b:02X}")).collect();
        let got = fingerprint(&der);
        assert_eq!(got, want.join(":"));
        assert_eq!((got.len(), got.matches(':').count()), (95, 31));
        assert!(
            got.chars()
                .all(|c| c == ':' || c.is_ascii_digit() || ('A'..='F').contains(&c))
        );
    }

    #[test]
    fn the_pin_is_the_fingerprint_in_lowercase_hex() {
        let der = CertificateDer::from(b"templum".to_vec());
        let pinned = pin(&der);
        assert_eq!(pinned, fingerprint(&der).replace(':', "").to_lowercase());
        assert!(
            pinned.len() == 64
                && pinned
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }

    #[test]
    fn dates_are_counted_from_the_epoch() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_723), (2024, 1, 1));
        assert_eq!(civil(19_782), (2024, 2, 29));
        assert_eq!(civil(-1), (1969, 12, 31));
    }

    #[test]
    fn the_persons_files_load_or_say_what_is_wrong() {
        let dir = tempfile::tempdir().unwrap();
        own(dir.path()).unwrap();
        let other = tempfile::tempdir().unwrap();
        let (cert, key) = (other.path().join("c.pem"), other.path().join("k.pem"));
        std::fs::copy(folder(dir.path()).join("cert.pem"), &cert).unwrap();
        std::fs::copy(folder(dir.path()).join("key.pem"), &key).unwrap();
        let loaded = files(&cert, &key).unwrap();
        assert_eq!(
            fingerprint(&loaded.chain[0]),
            fingerprint(&own(dir.path()).unwrap().chain[0])
        );
        let missing = other.path().join("nope.pem");
        assert!(
            files(&missing, &key)
                .err()
                .unwrap()
                .contains(missing.to_str().unwrap())
        );
        assert!(
            files(&cert, &missing)
                .err()
                .unwrap()
                .contains(missing.to_str().unwrap())
        );
        let junk = other.path().join("junk.pem");
        std::fs::write(&junk, "junk").unwrap();
        assert!(
            files(&junk, &key)
                .err()
                .unwrap()
                .contains("has no certificate")
        );
        assert!(
            files(&cert, &cert)
                .err()
                .unwrap()
                .contains("has no private key")
        );
        assert!(
            files(&cert, &junk)
                .err()
                .unwrap()
                .contains("has no private key")
        );
    }

    #[test]
    fn a_server_config_speaks_http_1_1() {
        let dir = tempfile::tempdir().unwrap();
        let config = server_config(own(dir.path()).unwrap()).unwrap();
        assert_eq!(config.alpn_protocols, vec![b"http/1.1".to_vec()]);
        // A key that isn't this certificate's is refused.
        let (a, b) = (own(dir.path()).unwrap(), tempfile::tempdir().unwrap());
        let stranger = own(b.path()).unwrap();
        let error = server_config(Certificate {
            chain: a.chain,
            key: stranger.key,
        })
        .err()
        .unwrap();
        assert!(error.contains("don't go together"), "{error}");
    }
}
