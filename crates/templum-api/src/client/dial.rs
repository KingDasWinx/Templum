//! Reaching home: TCP, through an `HTTP CONNECT` proxy when there is one, then TLS checked by
//! `trust`'s verifiers (docs/specs/templum-to-templum.md, The client).

use super::{Bad, Failure, Why, trust};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hyper::Uri;
use hyper_util::rt::TokioIo;
use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, Error as TlsError};

/// How long the TCP connection, the proxy's `CONNECT` and the TLS handshake may each take.
const STEP: Duration = Duration::from_secs(10);
/// The most of a proxy's answer that is read before giving up on it.
const HEAD_MOST: usize = 16 * 1024;

/// Where home is, as a connection code says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub https: bool,
    /// An IPv6 address without its brackets.
    pub host: String,
    pub port: u16,
    /// The code's path, without a trailing `/`: put before every `/v1/…`.
    pub prefix: String,
}

pub(crate) fn place(url: &str) -> Option<Place> {
    let uri: Uri = url.parse().ok()?;
    let https = match uri.scheme_str()? {
        "https" => true,
        "http" => false,
        _ => return None,
    };
    let host = uri.host()?.trim_start_matches('[').trim_end_matches(']');
    Some(Place {
        https,
        host: host.into(),
        port: uri.port_u16().unwrap_or(if https { 443 } else { 80 }),
        prefix: uri.path().trim_end_matches('/').into(),
    })
}

impl Place {
    /// Whether both are the same scheme, host and port: the paths may differ.
    pub(crate) fn same_origin(&self, other: &Place) -> bool {
        self.https == other.https
            && self.port == other.port
            && self.host.eq_ignore_ascii_case(&other.host)
    }

    /// `host` or `host:port`, as a `Host` header says it (a default port is left out).
    pub(crate) fn host_header(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        if self.port == if self.https { 443 } else { 80 } {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

/// An `HTTP CONNECT` proxy and, when it asks for a login, `user:password`.
#[derive(Clone)]
pub(crate) struct Proxy {
    pub host: String,
    pub port: u16,
    pub auth: Option<String>,
}

impl Proxy {
    /// From the address the index keeps (`http://host:port`) and the login the keyring keeps.
    pub(crate) fn new(address: &str, auth: Option<&str>) -> Option<Proxy> {
        let uri: Uri = address.parse().ok()?;
        Some(Proxy {
            host: uri
                .host()?
                .trim_start_matches('[')
                .trim_end_matches(']')
                .into(),
            port: uri.port_u16()?,
            auth: auth.map(Into::into),
        })
    }

    /// `host:port`, how a refusal names it.
    pub(crate) fn label(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl std::fmt::Debug for Proxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Proxy({})", self.label())
    }
}

/// `http://[user:pass@]host:port` as the address (what the index keeps) and the login (what the
/// keyring keeps).
pub fn proxy_of(text: &str) -> Result<(String, Option<String>), String> {
    const WRONG: &str = "The proxy must look like http://proxy.corp:8080.";
    let uri: Uri = text.trim().parse().map_err(|_| WRONG)?;
    let authority = uri.authority().ok_or(WRONG)?;
    if uri.scheme_str() != Some("http")
        || authority.port_u16().is_none()
        || !matches!(uri.path(), "" | "/")
        || uri.query().is_some()
    {
        return Err(WRONG.into());
    }
    let (login, address) = match authority.as_str().rsplit_once('@') {
        Some((login, address)) => (Some(percent_decoded(login)), address),
        None => (None, authority.as_str()),
    };
    Ok((format!("http://{address}"), login))
}

/// A URL's `%41` escapes; what isn't one stays.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%')
            .then(|| text.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `host` (an IPv6 address without its brackets) is this computer.
pub(crate) fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The proxy the environment names for `host`: `HTTPS_PROXY`, `https_proxy`, `ALL_PROXY`,
/// `all_proxy`; none when `NO_PROXY` / `no_proxy` names the host (`*`, the host, or a suffix such
/// as `.example.com` or `example.com`, comma-separated, ports ignored).
pub fn proxy_from_env(host: &str, var: impl Fn(&str) -> Option<String>) -> Option<String> {
    let set = |name: &str| {
        var(name)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    // Like curl: this computer is never reached through a proxy.
    if is_loopback(host) {
        return None;
    }
    let host = host.to_ascii_lowercase();
    if let Some(list) = set("NO_PROXY").or_else(|| set("no_proxy")) {
        let named = list.split(',').map(str::trim).any(|entry| {
            let entry = entry.to_ascii_lowercase();
            // Ports are ignored; an IPv6 entry has many colons, and keeps them.
            let entry = match entry.split_once(':') {
                Some((name, port)) if !port.contains(':') => name.to_string(),
                _ => entry,
            };
            let entry = entry.trim_start_matches('.');
            entry == "*"
                || (!entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}"))))
        });
        if named {
            return None;
        }
    }
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .into_iter()
        .find_map(set)
}

/// What a connection is made of; hyper reads and writes it through `TokioIo`.
pub(crate) trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
pub(crate) type Io = TokioIo<Box<dyn Duplex>>;

/// A connection to `place`: through `proxy` when given, with TLS when `tls` is.
pub(crate) async fn dial(
    place: &Place,
    tls: Option<Arc<ClientConfig>>,
    proxy: Option<&Proxy>,
) -> Result<Io, Failure> {
    let (host, port) = proxy.map_or((place.host.as_str(), place.port), |p| {
        (p.host.as_str(), p.port)
    });
    let mut tcp = within(TcpStream::connect((host, port))).await?;
    let _ = tcp.set_nodelay(true);
    if let Some(proxy) = proxy {
        connect_through(&mut tcp, place, proxy).await?;
    }
    let io: Box<dyn Duplex> = match tls {
        None => Box::new(tcp),
        Some(config) => {
            let name = ServerName::try_from(place.host.clone())
                .map_err(|_| Failure::Down("the address isn't a host name".into()))?;
            let stream = tokio::time::timeout(STEP, TlsConnector::from(config).connect(name, tcp))
                .await
                .map_err(|_| Failure::Down("the TLS handshake timed out".into()))?
                .map_err(|e| tls_failure(&e))?;
            Box::new(stream)
        }
    };
    Ok(TokioIo::new(io))
}

/// Waits for `step` at most, and names how it failed.
async fn within<T>(step: impl Future<Output = io::Result<T>>) -> Result<T, Failure> {
    match tokio::time::timeout(STEP, step).await {
        Ok(done) => done.map_err(|e| Failure::Down(e.to_string())),
        Err(_) => Err(Failure::Down("timed out".into())),
    }
}

/// `CONNECT host:port`; its answer's head must be a `2xx`.
async fn connect_through(tcp: &mut TcpStream, place: &Place, proxy: &Proxy) -> Result<(), Failure> {
    let target = if place.host.contains(':') {
        format!("[{}]:{}", place.host, place.port)
    } else {
        format!("{}:{}", place.host, place.port)
    };
    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if let Some(login) = &proxy.auth {
        request.push_str(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            STANDARD.encode(login)
        ));
    }
    request.push_str("\r\n");
    let head = within(async {
        tcp.write_all(request.as_bytes()).await?;
        // One byte at a time: whatever follows the head is the tunnel's, not ours.
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && head.len() < HEAD_MOST {
            if tcp.read(&mut byte).await? == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            head.push(byte[0]);
        }
        Ok(head)
    })
    .await
    .map_err(|failure| match failure {
        Failure::Down(why) => Failure::Down(format!("the proxy {}: {why}", proxy.label())),
        other => other,
    })?;
    let status = String::from_utf8_lossy(&head)
        .split_ascii_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            Failure::Down(format!(
                "the proxy {} didn't answer as a proxy",
                proxy.label()
            ))
        })?;
    match status {
        200..=299 => Ok(()),
        // A proxy that can't reach home right now: home is down, as it is without a proxy.
        502..=504 => Err(Failure::Down(format!(
            "the proxy {}: HTTP {status}",
            proxy.label()
        ))),
        _ => Err(Failure::Stop(Why::Proxy {
            proxy: proxy.label(),
            status,
        })),
    }
}

/// A handshake's refusal, as the mark `trust` put in it says.
fn tls_failure(error: &io::Error) -> Failure {
    let Some(error) = error.get_ref().and_then(|e| e.downcast_ref::<TlsError>()) else {
        return Failure::Down(error.to_string());
    };
    let certificate = |bad| Failure::Stop(Why::Certificate(bad));
    match error {
        TlsError::General(mark) => match mark.as_str() {
            trust::PIN => Failure::Stop(Why::PinChanged),
            trust::ADDED => Failure::Stop(Why::AddedAuthority),
            trust::EXPIRED => certificate(Bad::Expired),
            trust::NAME => certificate(Bad::Name),
            trust::ISSUER => certificate(Bad::Issuer),
            trust::ALONE => certificate(Bad::Alone),
            trust::OTHER => certificate(Bad::Other),
            _ => Failure::Down(error.to_string()),
        },
        // A signature that isn't the certificate's key's: someone shows a certificate they
        // can't answer for.
        TlsError::InvalidCertificate(_) => certificate(Bad::Other),
        _ => Failure::Down(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::block_on;
    use crate::client::http::{Conn, Headers};
    use crate::code::Key;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream as StdStream};

    #[test]
    fn places_and_proxies_are_read() {
        assert_eq!(
            place("https://templum.example.com/base/"),
            Some(Place {
                https: true,
                host: "templum.example.com".into(),
                port: 443,
                prefix: "/base".into()
            })
        );
        let v6 = place("https://[::1]:7411").unwrap();
        assert_eq!(
            (v6.host.as_str(), v6.port, v6.prefix.as_str()),
            ("::1", 7411, "")
        );
        assert_eq!(v6.host_header(), "[::1]:7411");
        assert_eq!(place("http://127.0.0.1:7411").unwrap().port, 7411);
        assert_eq!(
            place("https://t.example.com").unwrap().host_header(),
            "t.example.com"
        );
        assert_eq!(place("ftp://x"), None);
        assert_eq!(place("templum"), None);

        assert_eq!(
            proxy_of("http://u:p@proxy.corp:8080"),
            Ok(("http://proxy.corp:8080".into(), Some("u:p".into())))
        );
        assert_eq!(
            proxy_of("http://us%40er:p%3Ass@proxy.corp:8080/"),
            Ok(("http://proxy.corp:8080".into(), Some("us@er:p:ss".into())))
        );
        assert_eq!(
            proxy_of("http://proxy.corp:3128"),
            Ok(("http://proxy.corp:3128".into(), None))
        );
        for wrong in ["proxy", "https://proxy.corp:8080", "http://proxy.corp", ""] {
            assert_eq!(
                proxy_of(wrong),
                Err("The proxy must look like http://proxy.corp:8080.".into()),
                "{wrong}"
            );
        }

        let env = |pairs: &[(&str, &str)]| {
            let pairs: Vec<(String, String)> = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
            }
        };
        let host = "t.example.com";
        let proxy = Some("http://p:3128".to_string());
        assert_eq!(
            proxy_from_env(host, env(&[("HTTPS_PROXY", "http://p:3128")])),
            proxy
        );
        for named in [
            ".example.com",
            "example.com",
            "*",
            "other.com, t.example.com:443",
            "T.EXAMPLE.COM",
        ] {
            let set = [("HTTPS_PROXY", "http://p:3128"), ("no_proxy", named)];
            assert_eq!(proxy_from_env(host, env(&set)), None, "{named}");
        }
        let others = [
            ("HTTPS_PROXY", "http://p:3128"),
            ("NO_PROXY", "other.com,notexample.com"),
        ];
        assert_eq!(proxy_from_env(host, env(&others)), proxy);
        assert_eq!(
            proxy_from_env(host, env(&[("https_proxy", "http://p:3128")])),
            proxy
        );
        assert_eq!(
            proxy_from_env(
                host,
                env(&[("all_proxy", "http://q:1"), ("ALL_PROXY", "http://p:3128")])
            ),
            proxy
        );
        assert_eq!(proxy_from_env(host, env(&[("HTTPS_PROXY", " ")])), None);
        // This computer is not reached through a proxy, as curl has it.
        for local in ["127.0.0.1", "127.8.9.1", "::1", "localhost", "LOCALHOST"] {
            let set = [
                ("HTTPS_PROXY", "http://p:3128"),
                ("ALL_PROXY", "http://p:3128"),
            ];
            assert_eq!(proxy_from_env(local, env(&set)), None, "{local}");
            assert!(is_loopback(local), "{local}");
        }
        for far in [
            "192.168.1.5",
            "t.example.com",
            "::2",
            "localhost.example.com",
        ] {
            assert!(!is_loopback(far), "{far}");
        }
    }

    /// Answers every request on every connection with `200 {"ok":true}`.
    fn home() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || serve(stream));
            }
        });
        port
    }

    fn serve(mut stream: StdStream) {
        let mut seen = Vec::new();
        let mut byte = [0u8; 1];
        while stream.read(&mut byte).is_ok_and(|n| n == 1) {
            seen.push(byte[0]);
            if seen.ends_with(b"\r\n\r\n") {
                let body = r#"{"ok":true}"#;
                let answer = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                if stream.write_all(answer.as_bytes()).is_err() {
                    return;
                }
                seen.clear();
            }
        }
    }

    /// A proxy for one connection: it reads the head, hands it to `check`, answers `status` and,
    /// for a 200, pipes to `target`. Its port.
    fn proxy(status: u16, target: u16, check: impl FnOnce(&str) + Send + 'static) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut client, _) = listener.accept().unwrap();
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                client.read_exact(&mut byte).unwrap();
                head.push(byte[0]);
            }
            check(&String::from_utf8(head).unwrap());
            if status != 200 {
                let _ = write!(
                    client,
                    "HTTP/1.1 {status} Nope\r\nContent-Length: 0\r\n\r\n"
                );
                return;
            }
            client
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .unwrap();
            let server = StdStream::connect(("127.0.0.1", target)).unwrap();
            let (mut from_client, mut to_client) = (client.try_clone().unwrap(), client);
            let (mut from_server, mut to_server) = (server.try_clone().unwrap(), server);
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut from_client, &mut to_server);
            });
            let _ = std::io::copy(&mut from_server, &mut to_client);
        });
        port
    }

    fn headers() -> Headers {
        Headers {
            key: Key::new("tpl_x".into()),
            access: None,
            session: "s1".into(),
            host: "127.0.0.1".into(),
        }
    }

    fn at(port: u16) -> Place {
        place(&format!("http://127.0.0.1:{port}")).unwrap()
    }

    #[test]
    fn a_proxy_carries_the_connection_or_says_why_not() {
        let target = home();
        let carried = proxy(200, target, move |head| {
            assert!(
                head.starts_with(&format!("CONNECT 127.0.0.1:{target} HTTP/1.1\r\n")),
                "{head}"
            );
            assert!(
                head.contains("Proxy-Authorization: Basic dTpw\r\n"),
                "{head}"
            );
        });
        let through = Proxy::new(&format!("http://127.0.0.1:{carried}"), Some("u:p")).unwrap();
        block_on(async {
            let mut conn = Conn::open(&at(target), None, Some(&through)).await.unwrap();
            let answer = conn.call("GET", "/v1/me", None, &headers()).await;
            assert_eq!(answer, Ok(json!({"ok": true})));
        });

        let refusing = proxy(407, target, |_| {});
        let through = Proxy::new(&format!("http://127.0.0.1:{refusing}"), None).unwrap();
        let refused = block_on(Conn::open(&at(target), None, Some(&through)));
        assert_eq!(
            refused.err(),
            Some(Failure::Stop(Why::Proxy {
                proxy: format!("127.0.0.1:{refusing}"),
                status: 407
            }))
        );

        // A proxy that can't reach home is home being down; one that refuses is a stop.
        for status in [502, 503, 504] {
            let unreachable = proxy(status, target, |_| {});
            let through = Proxy::new(&format!("http://127.0.0.1:{unreachable}"), None).unwrap();
            let down = block_on(Conn::open(&at(target), None, Some(&through)));
            assert_eq!(
                down.err(),
                Some(Failure::Down(format!(
                    "the proxy 127.0.0.1:{unreachable}: HTTP {status}"
                )))
            );
        }
        let forbidden = proxy(403, target, |_| {});
        let through = Proxy::new(&format!("http://127.0.0.1:{forbidden}"), None).unwrap();
        assert!(matches!(
            block_on(Conn::open(&at(target), None, Some(&through))).err(),
            Some(Failure::Stop(Why::Proxy { status: 403, .. }))
        ));

        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let nothing = block_on(Conn::open(&at(closed), None, None));
        assert!(matches!(nothing.err(), Some(Failure::Down(_))));
    }
}
