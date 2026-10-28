//! Calls and streams over hyper 1's client connections, and what their failures mean
//! (docs/specs/templum-to-templum.md, The client and When it isn't live). No redirect is
//! followed: one is "didn't answer as a Templum".

use super::dial::{Place, Proxy, dial};
use super::{DETAILS_MOST, Failure, Why};
use crate::code::Key;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::client::conn::http1;
use hyper::http::header::{
    ACCEPT, AUTHORIZATION, CONTENT_TYPE, HOST, LOCATION, RETRY_AFTER, USER_AGENT,
};
use hyper::http::{HeaderMap, Request, request};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_rustls::rustls::ClientConfig;

/// The most a call's answer may hold.
const ANSWER_MOST: usize = 1024 * 1024;
/// The most the answer of `git` or `read_terminal` may hold.
const LARGE_ANSWER_MOST: usize = 8 * 1024 * 1024;
/// How long a call may take, from its request to the end of its answer.
pub(crate) const CALL: Duration = Duration::from_secs(30);
/// How long a call of the slow connection may take.
pub(crate) const SLOW_CALL: Duration = Duration::from_secs(600);

/// The most the answer of the tool `tool` may hold.
pub(crate) fn answer_most(tool: &str) -> usize {
    match tool {
        "git" | "read_terminal" => LARGE_ANSWER_MOST,
        _ => ANSWER_MOST,
    }
}

/// What every request of a mirror carries. No `Debug`: it holds the key.
#[derive(Clone)]
pub(crate) struct Headers {
    pub key: Key,
    /// A Cloudflare Access service token: the Client ID and its secret.
    pub access: Option<(String, String)>,
    /// New at each connect or reconnect.
    pub session: String,
    /// The `Host` header.
    pub host: String,
}

impl Headers {
    pub(crate) fn apply(&self, request: request::Builder) -> request::Builder {
        let mut request = request
            .header(HOST, &self.host)
            .header(AUTHORIZATION, format!("Bearer {}", self.key.expose()))
            .header(
                USER_AGENT,
                format!("Templum/{} (mirror)", env!("CARGO_PKG_VERSION")),
            )
            .header(crate::MIRROR_HEADER, &self.session);
        if let Some((id, secret)) = &self.access {
            request = request
                .header("cf-access-client-id", id)
                .header("cf-access-client-secret", secret);
        }
        request
    }
}

/// A spawned task that ends when this goes: nothing outlives the thing that owns it.
pub(crate) struct Task(JoinHandle<()>);

impl Task {
    pub(crate) fn spawn(future: impl std::future::Future<Output = ()> + Send + 'static) -> Task {
        Task(tokio::spawn(future))
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A keep-alive connection that makes one call at a time.
pub(crate) struct Conn {
    sender: http1::SendRequest<Full<Bytes>>,
    prefix: String,
    _driver: Task,
}

async fn connect(
    place: &Place,
    tls: Option<Arc<ClientConfig>>,
    proxy: Option<&Proxy>,
) -> Result<(http1::SendRequest<Full<Bytes>>, Task), Failure> {
    let io = dial(place, tls, proxy).await?;
    let (sender, connection) = http1::handshake::<_, Full<Bytes>>(io)
        .await
        .map_err(|e| Failure::Down(e.to_string()))?;
    let driver = Task::spawn(async move {
        let _ = connection.await;
    });
    Ok((sender, driver))
}

fn built(request: request::Builder, body: Bytes) -> Result<Request<Full<Bytes>>, Failure> {
    request
        .body(Full::new(body))
        .map_err(|_| Failure::Down("the request couldn't be made".into()))
}

impl Conn {
    pub(crate) async fn open(
        place: &Place,
        tls: Option<Arc<ClientConfig>>,
        proxy: Option<&Proxy>,
    ) -> Result<Conn, Failure> {
        let (sender, driver) = connect(place, tls, proxy).await?;
        Ok(Conn {
            sender,
            prefix: place.prefix.clone(),
            _driver: driver,
        })
    }

    /// Whether the connection can still make a call.
    pub(crate) fn open_still(&self) -> bool {
        !self.sender.is_closed()
    }

    /// `method path` (`path` gets the place's prefix in front) with a JSON body when given; the
    /// JSON the answer holds. A failed call leaves the connection unfit: open another.
    pub(crate) async fn call(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        headers: &Headers,
    ) -> Result<Value, Failure> {
        self.call_within(method, path, body, headers, CALL, ANSWER_MOST)
            .await
    }

    /// [`Conn::call`] with `limit` to answer in and an answer of at most `most` bytes.
    pub(crate) async fn call_within(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        headers: &Headers,
        limit: Duration,
        most: usize,
    ) -> Result<Value, Failure> {
        let mut request = headers.apply(
            Request::builder()
                .method(method)
                .uri(format!("{}{path}", self.prefix)),
        );
        let payload = match body {
            Some(value) => {
                request = request.header(CONTENT_TYPE, "application/json");
                Bytes::from(serde_json::to_vec(value).unwrap_or_default())
            }
            None => Bytes::new(),
        };
        let request = built(request, payload)?;
        let (status, answered, bytes) = tokio::time::timeout(limit, async {
            self.sender
                .ready()
                .await
                .map_err(|e| Failure::Down(e.to_string()))?;
            let response = self
                .sender
                .send_request(request)
                .await
                .map_err(|e| Failure::Down(e.to_string()))?;
            let (parts, body) = response.into_parts();
            Ok::<_, Failure>((
                parts.status.as_u16(),
                parts.headers,
                read(body, most).await?,
            ))
        })
        .await
        .map_err(|_| Failure::Down("home didn't answer in time".into()))??;
        if (200..300).contains(&status) {
            if bytes.is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_slice(&bytes).map_err(|_| not_templum(status, &answered));
        }
        Err(failure(status, &answered, &bytes, path))
    }
}

/// A body read whole, up to what the client keeps.
async fn read(body: Incoming, most: usize) -> Result<Bytes, Failure> {
    match Limited::new(body, most).collect().await {
        Ok(all) => Ok(all.to_bytes()),
        Err(e) if e.is::<http_body_util::LengthLimitError>() => Err(Failure::TooLarge),
        Err(e) => Err(Failure::Down(e.to_string())),
    }
}

/// Whether the connection ended where its body had more to say (the peer closed it), as against
/// a reset or bytes that aren't HTTP.
fn closed_by_home(error: &hyper::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::UnexpectedEof)
        {
            return true;
        }
        source = cause.source();
    }
    error.is_incomplete_message()
}

/// An open event or frame stream: its body, read chunk by chunk, and the connection under it.
pub(crate) struct Events {
    body: Incoming,
    _driver: Task,
}

impl Events {
    /// The next bytes the stream sends; None when it ended: finished, or closed by home (a
    /// connection that ends before its body does, as home's does when it cuts a share). Any
    /// other fault of the connection is an error.
    pub(crate) async fn chunk(&mut self) -> Option<Result<Bytes, Failure>> {
        loop {
            match self.body.frame().await? {
                Ok(frame) => {
                    if let Ok(data) = frame.into_data() {
                        return Some(Ok(data));
                    }
                }
                Err(e) if closed_by_home(&e) => return None,
                Err(e) => return Some(Err(Failure::Down(e.to_string()))),
            }
        }
    }
}

/// `GET path` as an event stream, on a connection of its own; `last_id` resumes after an event.
pub(crate) async fn stream(
    place: &Place,
    tls: Option<Arc<ClientConfig>>,
    proxy: Option<&Proxy>,
    path: &str,
    headers: &Headers,
    last_id: Option<&str>,
) -> Result<Events, Failure> {
    stream_within(CALL, place, tls, proxy, path, headers, last_id).await
}

/// [`stream`], with `within` as the time its head, and the body of an answer that isn't a
/// stream, may take.
pub(crate) async fn stream_within(
    within: Duration,
    place: &Place,
    tls: Option<Arc<ClientConfig>>,
    proxy: Option<&Proxy>,
    path: &str,
    headers: &Headers,
    last_id: Option<&str>,
) -> Result<Events, Failure> {
    let (mut sender, driver) = connect(place, tls, proxy).await?;
    let request = stream_request(&place.prefix, path, headers, last_id)?;
    let late = || Failure::Down("home didn't answer in time".into());
    let response = tokio::time::timeout(within, async {
        sender
            .ready()
            .await
            .map_err(|e| Failure::Down(e.to_string()))?;
        sender
            .send_request(request)
            .await
            .map_err(|e| Failure::Down(e.to_string()))
    })
    .await
    .map_err(|_| late())??;
    let (parts, body) = response.into_parts();
    if parts.status.as_u16() != 200 {
        let bytes = tokio::time::timeout(within, read(body, ANSWER_MOST))
            .await
            .map_err(|_| late())??;
        return Err(failure(parts.status.as_u16(), &parts.headers, &bytes, path));
    }
    Ok(Events {
        body,
        _driver: driver,
    })
}

/// The request that opens an event stream; `last_id` resumes after an event.
pub(crate) fn stream_request(
    prefix: &str,
    path: &str,
    headers: &Headers,
    last_id: Option<&str>,
) -> Result<Request<Full<Bytes>>, Failure> {
    let mut request = headers
        .apply(
            Request::builder()
                .method("GET")
                .uri(format!("{prefix}{path}")),
        )
        .header(ACCEPT, "text/event-stream");
    if let Some(id) = last_id {
        request = request.header("last-event-id", id);
    }
    built(request, Bytes::new())
}

/// The words of Templum's error body `{"error":{"code","message"}}`, if that is what it is.
fn templum_error(body: &[u8]) -> Option<(String, String, Option<String>)> {
    let error = serde_json::from_slice::<Value>(body).ok()?;
    let error = error.get("error")?;
    let details = error.get("details").and_then(Value::as_str).map(|details| {
        let mut end = details.len().min(DETAILS_MOST);
        while !details.is_char_boundary(end) {
            end -= 1;
        }
        details[..end].to_string()
    });
    Some((
        error.get("code")?.as_str()?.to_string(),
        error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        details,
    ))
}

/// How long `Retry-After` says to wait, else a second, and never more than 30 seconds: a hostile
/// or broken home must not stall a call for hours or overflow an `Instant`.
fn retry_after(headers: &HeaderMap) -> Duration {
    let seconds = headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(1)
        .min(30);
    Duration::from_secs(seconds)
}

fn not_templum(status: u16, headers: &HeaderMap) -> Failure {
    let kind = if (300..400).contains(&status) {
        "a redirect".to_string()
    } else {
        headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or("").trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "no type".into())
    };
    Failure::Stop(Why::NotTemplum { status, kind })
}

/// What an answer that wasn't a success means. `path` is the API path, without the place's
/// prefix.
pub(crate) fn failure(status: u16, headers: &HeaderMap, body: &[u8], path: &str) -> Failure {
    let down = || Failure::Down(format!("HTTP {status}"));
    let ours = templum_error(body);
    let access = headers.contains_key("cf-access-domain")
        || String::from_utf8_lossy(body).contains("cloudflareaccess.com");
    match status {
        401 => Failure::Auth,
        429 => Failure::Busy(retry_after(headers)),
        503 => match ours {
            Some((code, ..)) if code == "busy" => Failure::Busy(retry_after(headers)),
            _ => down(),
        },
        // 524 is Cloudflare's: a tunnel cut a call that took more than 100 s.
        502 | 504 | 524 | 530 => down(),
        300..=399 => {
            let to_access = headers
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|l| l.parse::<hyper::Uri>().ok())
                .and_then(|uri| uri.host().map(str::to_ascii_lowercase))
                .is_some_and(|host| host.ends_with("cloudflareaccess.com"));
            if to_access {
                Failure::Stop(Why::Access)
            } else {
                not_templum(status, headers)
            }
        }
        403 if access => Failure::Stop(Why::Access),
        _ => match ours {
            Some(_) if status == 404 && path == "/v1/me" => Failure::Stop(Why::TooOld),
            Some((_, message, _)) if status == 404 => Failure::Missing(message),
            Some((_, message, _)) if status >= 500 => Failure::Down(message),
            Some((_, message, details)) if status >= 400 => Failure::Refused(message, details),
            _ => not_templum(status, headers),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::http::HeaderValue;

    fn with(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(*k, HeaderValue::from_str(v).unwrap());
        }
        map
    }

    fn templum(code: &str, message: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"error": {"code": code, "message": message}}))
            .unwrap()
    }

    #[test]
    fn a_refusal_keeps_home_s_details_cut_to_64_kib() {
        let none = HeaderMap::new();
        let body = |details: &str| {
            serde_json::to_vec(&serde_json::json!({"error": {
                "code": "refused", "message": "fatal: x", "details": details
            }}))
            .unwrap()
        };
        assert_eq!(
            failure(422, &none, &body("hint: y\nfatal: x"), "/v1/tools/git"),
            Failure::Refused("fatal: x".into(), Some("hint: y\nfatal: x".into()))
        );
        let Failure::Refused(_, Some(cut)) =
            failure(422, &none, &body(&"é".repeat(70_000)), "/v1/tools/git")
        else {
            panic!("details kept");
        };
        assert!(
            cut.len() <= DETAILS_MOST && cut.chars().all(|c| c == 'é'),
            "{}",
            cut.len()
        );
    }

    #[test]
    fn failures_say_what_happened() {
        let none = HeaderMap::new();
        let down = |s: u16| Failure::Down(format!("HTTP {s}"));
        assert_eq!(failure(401, &none, b"", "/v1/me"), Failure::Auth);
        assert_eq!(
            failure(429, &with(&[("retry-after", "7")]), b"", "/v1/me"),
            Failure::Busy(Duration::from_secs(7))
        );
        assert_eq!(
            failure(503, &none, &templum("busy", "Busy."), "/v1/me"),
            Failure::Busy(Duration::from_secs(1))
        );
        // A hostile or broken home cannot make a call wait for hours, or overflow a clock.
        for huge in ["600", "18446744073709551615"] {
            assert_eq!(
                failure(429, &with(&[("retry-after", huge)]), b"", "/v1/me"),
                Failure::Busy(Duration::from_secs(30)),
                "{huge}"
            );
        }
        assert_eq!(
            failure(
                503,
                &with(&[("content-type", "text/html")]),
                b"<html>",
                "/v1/me"
            ),
            down(503)
        );
        assert_eq!(failure(530, &none, b"", "/v1/me"), down(530));
        assert_eq!(failure(502, &none, b"", "/v1/me"), down(502));
        assert!(matches!(
            failure(524, &with(&[]), b"", "/v1/tools/git"),
            Failure::Down(_)
        ));
        assert_eq!(
            failure(
                302,
                &with(&[(
                    "location",
                    "https://x.cloudflareaccess.com/cdn-cgi/access/login"
                )]),
                b"",
                "/v1/me"
            ),
            Failure::Stop(Why::Access)
        );
        assert_eq!(
            failure(
                302,
                &with(&[("location", "https://elsewhere")]),
                b"",
                "/v1/me"
            ),
            Failure::Stop(Why::NotTemplum {
                status: 302,
                kind: "a redirect".into()
            })
        );
        assert_eq!(
            failure(
                404,
                &with(&[("content-type", "text/html; charset=utf-8")]),
                b"<html>",
                "/v1/blocks"
            ),
            Failure::Stop(Why::NotTemplum {
                status: 404,
                kind: "text/html".into()
            })
        );
        assert_eq!(
            failure(
                404,
                &none,
                &templum("not_found", "No such route."),
                "/v1/me"
            ),
            Failure::Stop(Why::TooOld)
        );
        assert_eq!(
            failure(
                404,
                &none,
                &templum("not_found", "No block b7."),
                "/v1/terminals/x/stream"
            ),
            Failure::Missing("No block b7.".into())
        );
        assert_eq!(
            failure(
                422,
                &none,
                &templum("refused", "No."),
                "/v1/tools/move_block"
            ),
            Failure::Refused("No.".into(), None)
        );
        assert_eq!(
            failure(
                403,
                &with(&[("cf-access-domain", "x.cloudflareaccess.com")]),
                b"",
                "/v1/me"
            ),
            Failure::Stop(Why::Access)
        );
        assert_eq!(
            failure(403, &none, &templum("scope", "Needs Full."), "/v1/tools/x"),
            Failure::Refused("Needs Full.".into(), None)
        );
        assert_eq!(
            failure(200, &none, b"", "/v1/me"),
            Failure::Stop(Why::NotTemplum {
                status: 200,
                kind: "no type".into()
            })
        );
    }

    #[test]
    fn every_request_carries_the_mirrors_headers() {
        let headers = Headers {
            key: Key::new("tpl_x".into()),
            access: Some(("id".into(), "sec".into())),
            session: "s1".into(),
            host: "t.example.com".into(),
        };
        let request = headers
            .apply(Request::builder().uri("/v1/me"))
            .body(())
            .unwrap();
        let get = |name: &str| {
            request
                .headers()
                .get(name)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(get("authorization"), "Bearer tpl_x");
        assert_eq!(
            (get("cf-access-client-id"), get("cf-access-client-secret")),
            ("id".into(), "sec".into())
        );
        assert_eq!(get("templum-mirror"), "s1");
        assert_eq!(get("host"), "t.example.com");
        let agent = get("user-agent");
        assert!(
            agent.starts_with("Templum/") && agent.ends_with(" (mirror)"),
            "{agent}"
        );
        let plain = Headers {
            access: None,
            ..headers
        };
        let request = plain
            .apply(Request::builder().uri("/v1/me"))
            .body(())
            .unwrap();
        assert!(request.headers().get("cf-access-client-id").is_none());
    }

    #[test]
    fn a_stream_answer_that_stalls_in_its_body_times_out() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let (mut head, mut byte) = (Vec::new(), [0u8; 1]);
                    while !head.ends_with(b"\r\n\r\n") && stream.read_exact(&mut byte).is_ok() {
                        head.push(byte[0]);
                    }
                    // A head that promises a body, and a body that never comes.
                    let _ = stream.write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 100\r\n\r\n",
                    );
                    std::thread::sleep(Duration::from_secs(20));
                });
            }
        });
        let place = crate::client::dial::place(&format!("http://127.0.0.1:{port}")).unwrap();
        let headers = Headers {
            key: Key::new("tpl_x".into()),
            access: None,
            session: "s1".into(),
            host: place.host_header(),
        };
        let started = std::time::Instant::now();
        let opened = crate::client::block_on(stream_within(
            Duration::from_millis(300),
            &place,
            None,
            None,
            "/v1/events?view=true",
            &headers,
            None,
        ));
        assert_eq!(
            opened.err(),
            Some(Failure::Down("home didn't answer in time".into()))
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
