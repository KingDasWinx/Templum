//! Who answers: the verifiers the mirror's TLS asks (docs/specs/templum-to-templum.md, Trust).
//! A direct share pins Templum's certificate; a public one is checked by the system and then by
//! Mozilla's list, so an authority the computer added (a company proxy's) is told apart. A refusal
//! is a `rustls::Error::General` holding one of the marks below, so the dialer can say which after
//! the handshake.

use crate::store::same;
use ring::digest::{Context, SHA256, digest};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio_rustls::rustls::client::WebPkiServerVerifier;
use tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use tokio_rustls::rustls::crypto::{
    CryptoProvider, verify_tls12_signature, verify_tls13_signature,
};
use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use tokio_rustls::rustls::{
    self, CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore,
    SignatureScheme,
};

/// The pinned certificate is not the one that answered.
pub(crate) const PIN: &str = "templum:pin";
/// Valid only through an authority the computer added.
pub(crate) const ADDED: &str = "templum:added";
pub(crate) const EXPIRED: &str = "templum:expired";
pub(crate) const NAME: &str = "templum:name";
/// No authority the computer knows signed it, though the server sent more than its certificate.
pub(crate) const ISSUER: &str = "templum:issuer";
/// No authority known, and the server sent its certificate alone: maybe Templum's own.
pub(crate) const ALONE: &str = "templum:issuer-alone";
pub(crate) const OTHER: &str = "templum:other";

/// ring's crypto, as the server's.
pub(crate) fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn refuse(mark: &str) -> Error {
    Error::General(mark.into())
}

/// The pinned certificate shown by someone who can't sign for it: it is not the pinned one's
/// owner, so the person reads "the pin changed", not a public certificate's fault.
fn not_the_pinned(error: Error) -> Error {
    match error {
        Error::InvalidCertificate(_) => refuse(PIN),
        other => other,
    }
}

/// Templum's own certificate, by identity: its SHA-256. Names and dates are not looked at
/// (Templum's is made for `localhost` and lasts five years); the handshake's signatures are.
#[derive(Debug)]
pub(crate) struct Pinned {
    pin: [u8; 32],
    provider: Arc<CryptoProvider>,
}

impl Pinned {
    /// `hex` is 64 hex digits, as a connection code carries it.
    pub(crate) fn new(hex: &str) -> Option<Pinned> {
        if hex.len() != 64 || !hex.is_ascii() {
            return None;
        }
        let mut pin = [0u8; 32];
        for (byte, pair) in pin.iter_mut().zip(hex.as_bytes().chunks(2)) {
            *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        Some(Pinned {
            pin,
            provider: provider(),
        })
    }
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        if same(digest(&SHA256, end_entity.as_ref()).as_ref(), &self.pin) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(refuse(PIN))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
        .map_err(not_the_pinned)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
        .map_err(not_the_pinned)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// How many verdicts a `Checked` remembers.
const VERDICTS_MOST: usize = 8;

/// What a chain was judged to be: fine, or the mark of what is wrong with it.
type Verdict = Result<(), &'static str>;

/// A chain judged fine: what it is about (see `chain_key`) and the second its certificate ends.
type Remembered = ([u8; 32], u64);

/// A public certificate: the system's verdict first, then Mozilla's list on the same chain, so
/// one only the system accepts (an authority the computer added) is `ADDED` unless the person
/// accepted it for this remote.
///
/// The system's verifier may go to the network (Windows, macOS fetch missing intermediates and
/// revocations) while the one thread of the client waits, and every connection of a remote
/// shakes hands: so a chain judged fine is remembered, for a chain and a name, for the hour or
/// until its certificate ends, whichever is first. A refusal is never remembered: the person
/// fixing what it names (an authority installed, a clock set right) is heard on the next try.
/// What stays: for up to that hour a certificate revoked, or an authority taken away, since the
/// verdict is still trusted, unless the person presses Try now, which forgets every verdict.
#[derive(Debug)]
pub(crate) struct Checked {
    system: Arc<dyn ServerCertVerifier>,
    public: Arc<dyn ServerCertVerifier>,
    trust_added: bool,
    verdicts: Mutex<VecDeque<Remembered>>,
}

impl Checked {
    pub(crate) fn new(
        system: Arc<dyn ServerCertVerifier>,
        public: Arc<dyn ServerCertVerifier>,
        trust_added: bool,
    ) -> Checked {
        Checked {
            system,
            public,
            trust_added,
            verdicts: Mutex::default(),
        }
    }

    fn verdicts(&self) -> std::sync::MutexGuard<'_, VecDeque<Remembered>> {
        self.verdicts.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Every verdict forgotten, so the next handshake is judged anew.
    pub(crate) fn forget(&self) {
        self.verdicts().clear();
    }

    fn judge(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Verdict {
        if let Err(error) =
            self.system
                .verify_server_cert(end_entity, intermediates, name, ocsp, now)
        {
            let mark = mark_of(&error, intermediates);
            if mark != OTHER {
                return Err(mark);
            }
            // The platform's own errors are not told apart (macOS reports most as "other"):
            // Mozilla's list reads the same chain, so the person still hears why.
            return Err(self
                .public
                .verify_server_cert(end_entity, intermediates, name, ocsp, now)
                .err()
                .map_or(OTHER, |error| mark_of(&error, intermediates)));
        }
        if !self.trust_added
            && self
                .public
                .verify_server_cert(end_entity, intermediates, name, ocsp, now)
                .is_err()
        {
            return Err(ADDED);
        }
        Ok(())
    }
}

/// What a verifier's refusal says about the certificate.
fn mark_of(error: &Error, intermediates: &[CertificateDer<'_>]) -> &'static str {
    match error {
        Error::InvalidCertificate(CertificateError::Expired)
        | Error::InvalidCertificate(CertificateError::ExpiredContext { .. }) => EXPIRED,
        Error::InvalidCertificate(CertificateError::NotValidForName)
        | Error::InvalidCertificate(CertificateError::NotValidForNameContext { .. }) => NAME,
        Error::InvalidCertificate(CertificateError::UnknownIssuer) => {
            if intermediates.is_empty() {
                ALONE
            } else {
                ISSUER
            }
        }
        _ => OTHER,
    }
}

/// One DER element: its tag, its content and what follows it. None when it doesn't fit.
fn der(bytes: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, bytes) = bytes.split_first()?;
    let (&first, bytes) = bytes.split_first()?;
    let (len, bytes) = if first < 0x80 {
        (usize::from(first), bytes)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || bytes.len() < count {
            return None;
        }
        let len = bytes[..count]
            .iter()
            .fold(0usize, |len, b| len << 8 | usize::from(*b));
        (len, &bytes[count..])
    };
    (len <= bytes.len()).then(|| (tag, &bytes[..len], &bytes[len..]))
}

/// The second a certificate ends (its `notAfter`), read from its DER. None when it can't be read,
/// and then nothing is remembered about it.
fn not_after(cert: &[u8]) -> Option<u64> {
    let (_, cert, _) = der(cert)?;
    let (_, tbs, _) = der(cert)?;
    // [0] version (optional), serial, signature, issuer, then validity: notBefore, notAfter.
    let (tag, _, mut rest) = der(tbs)?;
    if tag == 0xa0 {
        rest = der(rest)?.2;
    }
    let rest = der(rest)?.2;
    let rest = der(rest)?.2;
    let (_, validity, _) = der(rest)?;
    let (tag, time, _) = der(der(validity)?.2)?;
    let digits = |text: &[u8]| {
        text.iter().try_fold(0i64, |n, b| {
            b.is_ascii_digit().then(|| n * 10 + i64::from(b - b'0'))
        })
    };
    // UTCTime has two digits of year (50 and up is 19xx), GeneralizedTime four.
    let (year, rest) = match (tag, time.len()) {
        (0x17, 13) => {
            let year = digits(&time[..2])?;
            (year + if year >= 50 { 1900 } else { 2000 }, &time[2..])
        }
        (0x18, 15) => (digits(&time[..4])?, &time[4..]),
        _ => return None,
    };
    let [month, day, hour, minute, second] = [0, 2, 4, 6, 8].map(|at| digits(&rest[at..at + 2]));
    let (month, day) = (month?, day?);
    let (hour, minute, second) = (hour?, minute?, second?);
    if rest[10] != b'Z'
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Howard Hinnant's `days_from_civil`.
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second).ok()
}

/// A chain, a name and the hour: what a remembered verdict is about.
fn chain_key(
    end_entity: &CertificateDer<'_>,
    intermediates: &[CertificateDer<'_>],
    name: &ServerName<'_>,
    now: UnixTime,
) -> [u8; 32] {
    let mut hash = Context::new(&SHA256);
    let mut part = |bytes: &[u8]| {
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    part(end_entity.as_ref());
    for der in intermediates {
        part(der.as_ref());
    }
    part(name.to_str().as_bytes());
    part(&(now.as_secs() / 3600).to_le_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(hash.finish().as_ref());
    key
}

impl ServerCertVerifier for Checked {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let key = chain_key(end_entity, intermediates, name, now);
        let fine = self
            .verdicts()
            .iter()
            .any(|(of, ends)| *of == key && now.as_secs() < *ends);
        if fine {
            return Ok(ServerCertVerified::assertion());
        }
        let verdict = self.judge(end_entity, intermediates, name, ocsp, now);
        if let (Ok(()), Some(ends)) = (verdict, not_after(end_entity.as_ref())) {
            let mut verdicts = self.verdicts();
            verdicts.retain(|(of, _)| *of != key);
            if verdicts.len() >= VERDICTS_MOST {
                verdicts.pop_front();
            }
            verdicts.push_back((key, ends));
        }
        verdict
            .map(|()| ServerCertVerified::assertion())
            .map_err(refuse)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.system.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.system.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.system.supported_verify_schemes()
    }
}

/// The system's verifier and Mozilla's roots', in that order.
pub(crate) type Verifiers = (Arc<dyn ServerCertVerifier>, Arc<dyn ServerCertVerifier>);

/// The two verifiers a public share is checked by: the system's own and Mozilla's roots.
pub(crate) fn web() -> Result<Verifiers, String> {
    let failed = |error: &dyn std::fmt::Display| {
        format!("Templum couldn't load the certificate authorities: {error}")
    };
    let system = rustls_platform_verifier::Verifier::new(provider()).map_err(|e| failed(&e))?;
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let public = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider())
        .build()
        .map_err(|e| failed(&e))?;
    Ok((Arc::new(system), public))
}

/// The client's TLS settings: 1.2 and 1.3, the given verifier, no client certificate, HTTP/1.1.
pub(crate) fn config(verifier: Arc<dyn ServerCertVerifier>) -> Arc<ClientConfig> {
    let mut config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("ring supports TLS 1.2 and 1.3")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Arc::new(config)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tls;
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
    use tokio_rustls::rustls::ServerConfig;
    use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio_rustls::rustls::server::{ClientHello, ResolvesServerCert};
    use tokio_rustls::rustls::sign::CertifiedKey;
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    /// An authority: its parameters, key and certificate.
    pub(crate) struct Authority {
        params: CertificateParams,
        key: KeyPair,
        der: CertificateDer<'static>,
    }

    /// A certificate and the key that signs for it.
    pub(crate) struct Leaf {
        pub(crate) der: CertificateDer<'static>,
        key: KeyPair,
    }

    fn pair() -> KeyPair {
        KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap()
    }

    pub(crate) fn ca(name: &str) -> Authority {
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.distinguished_name.push(DnType::CommonName, name);
        let key = pair();
        let der = params.self_signed(&key).unwrap().der().clone();
        Authority { params, key, der }
    }

    /// A certificate for `name` that `ca` signs; valid through 2100, or, when `expired`, ended in
    /// 2021.
    pub(crate) fn leaf(ca: &Authority, name: &str, expired: bool) -> Leaf {
        let ends = if expired { (2021, 1, 1) } else { (2100, 1, 1) };
        leaf_ending(ca, name, ends, 0)
    }

    /// A certificate for `name` that `ca` signs, valid from 2020 until `ends` (a date) and `seconds`
    /// after its midnight.
    fn leaf_ending(ca: &Authority, name: &str, ends: (i32, u8, u8), seconds: u64) -> Leaf {
        let mut params = CertificateParams::new(vec![name.to_string()]).unwrap();
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after =
            rcgen::date_time_ymd(ends.0, ends.1, ends.2) + std::time::Duration::from_secs(seconds);
        let key = pair();
        let issuer = rcgen::Issuer::from_params(&ca.params, &ca.key);
        let der = params.signed_by(&key, &issuer).unwrap().der().clone();
        Leaf { der, key }
    }

    fn self_signed(name: &str) -> Leaf {
        let key = pair();
        let der = CertificateParams::new(vec![name.to_string()])
            .unwrap()
            .self_signed(&key)
            .unwrap()
            .der()
            .clone();
        Leaf { der, key }
    }

    /// A verifier that trusts exactly these authorities.
    pub(crate) fn roots(authorities: &[&Authority]) -> Arc<dyn ServerCertVerifier> {
        let mut store = RootCertStore::empty();
        for authority in authorities {
            store.add(authority.der.clone()).unwrap();
        }
        WebPkiServerVerifier::builder_with_provider(Arc::new(store), provider())
            .build()
            .unwrap()
    }

    fn check(
        verifier: &dyn ServerCertVerifier,
        leaf: &Leaf,
        intermediates: &[CertificateDer<'static>],
    ) -> Result<(), String> {
        let name = ServerName::try_from("localhost").unwrap();
        verifier
            .verify_server_cert(&leaf.der, intermediates, &name, &[], UnixTime::now())
            .map(|_| ())
            .map_err(|e| match e {
                Error::General(mark) => mark,
                other => format!("{other:?}"),
            })
    }

    /// Always answers with one chain and one key, which need not belong together.
    #[derive(Debug)]
    struct Fixed(Arc<CertifiedKey>);

    impl ResolvesServerCert for Fixed {
        fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
            Some(self.0.clone())
        }
    }

    /// A TLS handshake in memory: a server that shows `chain` and signs with `signer`, and a
    /// client that asks `verifier` about it; the client's refusal as its mark.
    fn handshake(
        chain: &Leaf,
        signer: &KeyPair,
        verifier: Arc<dyn ServerCertVerifier>,
        name: &str,
    ) -> Result<(), String> {
        let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(signer.serialize_der()));
        let key = provider().key_provider.load_private_key(key).unwrap();
        let certified = CertifiedKey {
            cert: vec![chain.der.clone()],
            key,
            ocsp: None,
        };
        let server = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(Fixed(Arc::new(certified))));
        let (to_server, to_client) = tokio::io::duplex(64 * 1024);
        let acceptor = TlsAcceptor::from(Arc::new(server));
        let connector = TlsConnector::from(config(verifier));
        let name = ServerName::try_from(name.to_string()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let (_, client) = runtime.block_on(async {
            futures::join!(
                async { acceptor.accept(to_client).await.map(|_| ()) },
                async { connector.connect(name, to_server).await.map(|_| ()) }
            )
        });
        client.map_err(
            |e| match e.get_ref().and_then(|inner| inner.downcast_ref::<Error>()) {
                Some(Error::General(mark)) => mark.clone(),
                _ => e.to_string(),
            },
        )
    }

    #[test]
    fn the_pin_is_the_certificates_identity() {
        let cert = self_signed("localhost");
        let pinned = || Pinned::new(&tls::pin(&cert.der)).unwrap();
        // Names and dates aren't looked at: an IP the certificate isn't for is the same host.
        assert_eq!(
            handshake(&cert, &cert.key, Arc::new(pinned()), "192.168.1.5"),
            Ok(())
        );
        assert_eq!(
            handshake(&cert, &cert.key, Arc::new(pinned()), "localhost"),
            Ok(())
        );

        let other = self_signed("localhost");
        assert_eq!(
            handshake(&other, &other.key, Arc::new(pinned()), "localhost"),
            Err(PIN.to_string())
        );
        for wrong in ["zz", "", &"0".repeat(63), &"g".repeat(64), &"é".repeat(32)] {
            assert!(Pinned::new(wrong).is_none(), "{wrong}");
        }
        assert!(Pinned::new(&tls::pin(&cert.der).to_uppercase()).is_some());

        // The right certificate shown by someone who doesn't hold its key is not its owner: the
        // person reads "the pin changed", not a public certificate's fault.
        let forged = handshake(&cert, &other.key, Arc::new(pinned()), "localhost");
        assert_eq!(forged, Err(PIN.to_string()));
    }

    #[test]
    fn a_public_certificate_is_checked_twice() {
        let (a, b) = (ca("a"), ca("b"));
        let both = || Checked::new(roots(&[&a]), roots(&[&a]), false);
        assert_eq!(check(&both(), &leaf(&a, "localhost", false), &[]), Ok(()));
        assert_eq!(
            check(&both(), &leaf(&a, "localhost", true), &[]),
            Err(EXPIRED.into())
        );
        assert_eq!(
            check(&both(), &leaf(&a, "other.example", false), &[]),
            Err(NAME.into())
        );
        let stranger = leaf(&b, "localhost", false);
        assert_eq!(check(&both(), &stranger, &[]), Err(ALONE.into()));
        assert_eq!(
            check(&both(), &stranger, std::slice::from_ref(&a.der)),
            Err(ISSUER.into())
        );
        let shown = leaf(&a, "localhost", false);
        let signed_by_a_stranger = handshake(&shown, &stranger.key, Arc::new(both()), "localhost");
        assert!(
            signed_by_a_stranger.is_err(),
            "another key's signature is refused"
        );
    }

    #[test]
    fn an_added_authority_is_refused_unless_trusted() {
        let (a, c) = (ca("public"), ca("a company's"));
        let behind = leaf(&c, "localhost", false);
        let refused = Checked::new(roots(&[&a, &c]), roots(&[&a]), false);
        assert_eq!(check(&refused, &behind, &[]), Err(ADDED.into()));
        let accepted = Checked::new(roots(&[&a, &c]), roots(&[&a]), true);
        assert_eq!(check(&accepted, &behind, &[]), Ok(()));
        // A public authority needs no acceptance.
        let publicly = leaf(&a, "localhost", false);
        assert_eq!(check(&refused, &publicly, &[]), Ok(()));
    }

    /// A verifier that refuses every chain with an error of its own, as a platform's may; the
    /// times it was asked.
    #[derive(Debug)]
    pub(crate) struct Platform {
        asked: std::sync::atomic::AtomicUsize,
        says: Mutex<Option<Arc<dyn ServerCertVerifier>>>,
    }

    impl Platform {
        fn refusing() -> Arc<Platform> {
            Arc::new(Platform {
                asked: Default::default(),
                says: Mutex::default(),
            })
        }

        /// The authority is installed: from now on the platform answers as `inner` does.
        fn install(&self, inner: Arc<dyn ServerCertVerifier>) {
            *self.says.lock().unwrap() = Some(inner);
        }

        pub(crate) fn counting(inner: Arc<dyn ServerCertVerifier>) -> Arc<Platform> {
            Arc::new(Platform {
                asked: Default::default(),
                says: Mutex::new(Some(inner)),
            })
        }

        pub(crate) fn asked(&self) -> usize {
            self.asked.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl ServerCertVerifier for Platform {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            intermediates: &[CertificateDer<'_>],
            name: &ServerName<'_>,
            ocsp: &[u8],
            now: UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            match &*self.says.lock().unwrap() {
                Some(inner) => inner.verify_server_cert(end_entity, intermediates, name, ocsp, now),
                None => Err(Error::General("the platform's own words".into())),
            }
        }

        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    #[test]
    fn a_platform_that_only_says_other_is_read_by_mozillas_list() {
        let (a, b) = (ca("a"), ca("b"));
        let checked = || Checked::new(Platform::refusing(), roots(&[&a]), false);
        assert_eq!(
            check(&checked(), &leaf(&a, "localhost", true), &[]),
            Err(EXPIRED.into())
        );
        assert_eq!(
            check(&checked(), &leaf(&a, "other.example", false), &[]),
            Err(NAME.into())
        );
        let stranger = leaf(&b, "localhost", false);
        assert_eq!(check(&checked(), &stranger, &[]), Err(ALONE.into()));
        assert_eq!(
            check(&checked(), &stranger, std::slice::from_ref(&a.der)),
            Err(ISSUER.into())
        );
        // Nothing wrong the list can find: the platform's refusal stays "other".
        assert_eq!(
            check(&checked(), &leaf(&a, "localhost", false), &[]),
            Err(OTHER.into())
        );
    }

    #[test]
    fn a_chain_judged_fine_is_asked_once_and_a_refusal_every_time() {
        let a = ca("a");
        let system = Platform::counting(roots(&[&a]));
        let checked = Checked::new(system.clone(), roots(&[&a]), false);
        let (good, expired) = (leaf(&a, "localhost", false), leaf(&a, "localhost", true));
        for _ in 0..3 {
            assert_eq!(check(&checked, &good, &[]), Ok(()));
            assert_eq!(check(&checked, &expired, &[]), Err(EXPIRED.into()));
        }
        assert_eq!(
            system.asked(),
            1 + 3,
            "the good chain once, the refused one at every handshake"
        );

        // Another name is another question; another hour too.
        let other = ServerName::try_from("other.example").unwrap();
        let _ = checked.verify_server_cert(&good.der, &[], &other, &[], UnixTime::now());
        assert_eq!(system.asked(), 5);
        let later = UnixTime::since_unix_epoch(std::time::Duration::from_secs(
            UnixTime::now().as_secs() + 2 * 3600,
        ));
        let name = ServerName::try_from("localhost").unwrap();
        let _ = checked.verify_server_cert(&good.der, &[], &name, &[], later);
        assert_eq!(system.asked(), 6);

        // Only a few are kept, the oldest forgotten first.
        for _ in 0..VERDICTS_MOST {
            let _ = check(&checked, &leaf(&a, "localhost", false), &[]);
        }
        let before = system.asked();
        assert_eq!(check(&checked, &good, &[]), Ok(()));
        assert_eq!(system.asked(), before + 1, "forgotten, so asked again");
        let remembered = leaf(&a, "localhost", false);
        let _ = check(&checked, &remembered, &[]);
        let after = system.asked();
        let _ = check(&checked, &remembered, &[]);
        assert_eq!(system.asked(), after, "and remembered again");

        // A refusal whose certificate has not ended is no more remembered than one that has: the
        // platform trusts `c` (an authority the computer added), Mozilla's list does not.
        let c = ca("c");
        let system = Platform::counting(roots(&[&a, &c]));
        let checked = Checked::new(system.clone(), roots(&[&a]), false);
        let added = leaf(&c, "localhost", false);
        let wrong_name = leaf(&a, "other.example", false);
        for _ in 0..3 {
            assert_eq!(check(&checked, &added, &[]), Err(ADDED.into()));
            assert_eq!(check(&checked, &wrong_name, &[]), Err(NAME.into()));
        }
        assert_eq!(system.asked(), 6, "refused, so asked at every handshake");
    }

    #[test]
    fn a_refusal_is_forgotten_at_once_and_a_fine_chain_when_asked_to() {
        let a = ca("a");
        let system = Platform::refusing();
        let checked = Checked::new(system.clone(), roots(&[&a]), false);
        let good = leaf(&a, "localhost", false);
        let name = ServerName::try_from("localhost").unwrap();
        let now = UnixTime::now();
        let ask = || checked.verify_server_cert(&good.der, &[], &name, &[], now);
        assert!(ask().is_err());
        // The person installs the authority: the next try, in the same hour, is heard.
        system.install(roots(&[&a]));
        assert!(ask().is_ok(), "the refusal was not kept");
        let asked = system.asked();
        assert!(ask().is_ok());
        assert_eq!(system.asked(), asked, "a fine chain is kept");
        // ...until Try now: the person may have taken the authority away again.
        checked.forget();
        assert!(ask().is_ok());
        assert_eq!(system.asked(), asked + 1);
    }

    #[test]
    fn a_fine_chain_is_not_trusted_past_its_certificates_end() {
        const MIDNIGHT: u64 = 1_748_736_000; // 2025-06-01 00:00:00 UTC
        let a = ca("a");
        let system = Platform::counting(roots(&[&a]));
        let checked = Checked::new(system.clone(), roots(&[&a]), false);
        // Valid until 00:30, in the hour that began at midnight.
        let short = leaf_ending(&a, "localhost", (2025, 6, 1), 30 * 60);
        let name = ServerName::try_from("localhost").unwrap();
        let at = |minutes: u64| {
            let now =
                UnixTime::since_unix_epoch(std::time::Duration::from_secs(MIDNIGHT + minutes * 60));
            checked.verify_server_cert(&short.der, &[], &name, &[], now)
        };
        assert!(at(10).is_ok());
        assert!(at(20).is_ok());
        assert_eq!(system.asked(), 1);
        // Same hour, but past the end: asked again, and refused for its date.
        assert!(matches!(
            at(40),
            Err(Error::General(mark)) if mark == EXPIRED
        ));
        assert_eq!(system.asked(), 2);
    }

    #[test]
    fn a_certificates_end_is_read_from_its_der() {
        let a = ca("a");
        let ends = |l: &Leaf| not_after(l.der.as_ref());
        // 2100-01-01 is a GeneralizedTime, 2021-01-01 a UTCTime.
        assert_eq!(ends(&leaf(&a, "x", false)), Some(4_102_444_800));
        assert_eq!(ends(&leaf(&a, "x", true)), Some(1_609_459_200));
        assert_eq!(
            ends(&leaf_ending(&a, "x", (2025, 6, 1), 1_845)),
            Some(1_748_736_000 + 1_845)
        );
        // UTCTime reads 99 as 1999 and 49 as 2049; 2050 is where GeneralizedTime takes over.
        for (date, second) in [
            ((1999, 12, 31), 946_598_400),
            ((2049, 12, 31), 2_524_521_600),
            ((2050, 1, 1), 2_524_608_000),
        ] {
            assert_eq!(
                ends(&leaf_ending(&a, "x", date, 0)),
                Some(second),
                "{date:?}"
            );
        }
        for broken in [
            &[][..],
            &[0x30],
            &[0x30, 0x84, 0xff, 0xff, 0xff, 0xff],
            b"junk",
        ] {
            assert_eq!(not_after(broken), None);
        }
        let whole = leaf(&a, "x", false).der;
        for cut in (0..whole.len()).step_by(7) {
            let _ = not_after(&whole[..cut]);
        }
    }

    #[test]
    fn the_web_verifiers_build_on_this_computer() {
        assert!(web().is_ok());
    }
}
