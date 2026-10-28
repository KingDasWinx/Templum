//! The server over TLS (docs/specs/api.md, Encryption).

mod common;

use common::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use templum_api::store::{Listen, Scope, Settings, Tls};
use templum_api::{Options, tls};
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::crypto::{self, CryptoProvider};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{
    self, ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme,
};

fn own() -> Settings {
    Settings {
        tls: Tls::Own,
        port: 0,
        ..Default::default()
    }
}

/// Records the certificate a server presents, and checks its signatures, but trusts any.
#[derive(Debug)]
struct Recording {
    provider: Arc<CryptoProvider>,
    seen: Mutex<Option<CertificateDer<'static>>>,
}

impl ServerCertVerifier for Recording {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        *self.seen.lock().unwrap() = Some(end_entity.clone().into_owned());
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// The certificate the server on `port` shows, from a real handshake.
fn presented(port: u16) -> CertificateDer<'static> {
    let provider = Arc::new(crypto::ring::default_provider());
    let verifier = Arc::new(Recording {
        provider: provider.clone(),
        seen: Mutex::new(None),
    });
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let mut connection =
        ClientConnection::new(Arc::new(config), ServerName::try_from("localhost").unwrap())
            .unwrap();
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let mut stream = rustls::Stream::new(&mut connection, &mut tcp);
    stream
        .write_all(b"GET /v1/nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    assert!(
        String::from_utf8_lossy(&answer).starts_with("HTTP/1.1 401"),
        "{}",
        String::from_utf8_lossy(&answer)
    );
    assert_eq!(connection.alpn_protocol(), Some(&b"http/1.1"[..]));
    let seen = verifier.seen.lock().unwrap().clone();
    seen.expect("a certificate was shown")
}

#[test]
fn https_serves_with_templums_certificate() {
    let (text, token) = token(Scope::Read, None);
    let fake = fake_with(own(), vec![token], |_| {});
    assert!(fake.base.starts_with("https://"));
    let answer = fake.call(
        req("GET", "/v1/state")
            .token(&text)
            .agent(https_unverified()),
    );
    assert_eq!(answer.status, 200);
    // No HSTS: a browser would pin the host for a year, and the certificate changes.
    assert_eq!(answer.header("strict-transport-security"), None);
    assert_eq!(answer.header("x-content-type-options"), Some("nosniff"));
    let refused = fake.call(req("GET", "/v1/state").agent(https_unverified()));
    assert_eq!(
        (refused.status, refused.header("strict-transport-security")),
        (401, None)
    );
    // Plain HTTP to the same port is no HTTP at all.
    let plain = format!("http://127.0.0.1:{}", fake.server.addr().port());
    let attempt = http().run(req("GET", "/v1/state").token(&text).build(&plain));
    assert!(attempt.is_err() || attempt.is_ok_and(|r| r.status() != 200));
    // And the handshake that failed was audited.
    let log = || {
        std::fs::read_to_string(templum_api::store::folder(fake.dir.path()).join("noise.log"))
            .unwrap_or_default()
    };
    wait_for("tls_failed in the audit log", || {
        log().contains("\"tls_failed\"")
    });
}

#[test]
fn the_served_certificate_is_the_fingerprint_shown() {
    let fake = fake_with(own(), vec![], |_| {});
    let shown = tls::fingerprint(&tls::own(fake.dir.path()).unwrap().chain[0]);
    let got = tls::fingerprint(&presented(fake.server.addr().port()));
    assert_eq!(got, shown);
}

#[test]
fn the_persons_certificate_is_served() {
    let dir = tempfile::tempdir().unwrap();
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();
    let (cert_file, key_file) = (dir.path().join("mine.pem"), dir.path().join("mine.key"));
    std::fs::write(&cert_file, cert.pem()).unwrap();
    std::fs::write(&key_file, key.serialize_pem()).unwrap();
    let settings = Settings {
        tls: Tls::Files {
            cert: cert_file.clone(),
            key: key_file.clone(),
        },
        port: 0,
        ..Default::default()
    };
    let fake = fake_with(settings.clone(), vec![], |_| {});
    let got = tls::fingerprint(&presented(fake.server.addr().port()));
    assert_eq!(got, tls::fingerprint(cert.der()));
    // A key that doesn't load keeps it from starting, and the error names the file.
    std::fs::write(&key_file, "not a key").unwrap();
    let error = Options::from(&settings, dir.path()).err().unwrap();
    assert!(error.contains(key_file.to_str().unwrap()), "{error}");
    // Nor does a key that isn't this certificate's.
    let other = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
    std::fs::write(&key_file, other.serialize_pem()).unwrap();
    assert!(Options::from(&settings, dir.path()).is_err());
}

#[test]
fn every_network_needs_https() {
    let (text, token) = token(Scope::Read, None);
    let settings = Settings {
        listen: Listen::All,
        tls: Tls::Own,
        port: 0,
        ..Default::default()
    };
    let fake = fake_with(settings, vec![token], |_| {});
    assert!(
        fake.server.addr().ip().is_unspecified(),
        "bound to every interface"
    );
    let answer = fake.call(
        req("GET", "/v1/state")
            .token(&text)
            .agent(https_unverified()),
    );
    assert_eq!(answer.status, 200);
}

#[test]
fn a_silent_client_does_not_hold_a_handshake_forever() {
    let fake = fake_with(own(), vec![], |_| {});
    // It connects and says nothing: the server keeps answering others meanwhile.
    let _quiet = TcpStream::connect(("127.0.0.1", fake.server.addr().port())).unwrap();
    let answer = fake.call(req("GET", "/v1/state").agent(https_unverified()));
    assert_eq!(answer.status, 401);
}
