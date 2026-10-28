//! The server (docs/specs/api.md): HTTP/1.1 on its own thread and a single-threaded runtime, in
//! plain text on loopback or over TLS anywhere. A call is handed to the UI thread as a bus
//! message (`client: "api"`, the token's id as its terminal), and its line is waited for on
//! tokio's blocking pool, so the UI thread never touches a socket and never waits on the network.

use crate::audit::{AUDIT_CAP, Audit};
use crate::events::{self, Hub, Ids, StreamBody};
use crate::model::{Caller, MeView, Notice, State};
use crate::registry::{self, Does, Resource};
use crate::store::{Listen, Settings, Tls, Token, Use, folder, hash, random_text, verify};
use crate::terminals::{self, FRAME_EVERY, Screens, TerminalRef};
use crate::{CLIENT, CLIENTS, STATE, TERMINAL, WATCH, codes, openapi, tls};
use bytes::Bytes;
use futures::channel::mpsc::UnboundedSender;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming as RequestBody;
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use templum_mcp::bus::{CLOSED, next_conn};
use templum_mcp::{Hello, Incoming, Message, rpc};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::AbortHandle;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;

/// How often a stream gets a `: ping`, under every proxy's idle timeout.
pub const HEARTBEAT: Duration = Duration::from_secs(15);
/// How long a ticket opens an event stream.
pub const TICKET_LIFE: Duration = Duration::from_secs(30);

/// The largest body, in bytes. A longer one is read no further than [`DRAIN`] past it.
const BODY_MAX: usize = 1024 * 1024;
const DRAIN: usize = 1024 * 1024;
const BODY_WAIT: Duration = Duration::from_secs(30);
const HEADER_WAIT: Duration = Duration::from_secs(10);
const HEADER_BYTES: usize = 16 * 1024;
const HANDSHAKE_WAIT: Duration = Duration::from_secs(10);
const CONNECTIONS: usize = 64;
/// Connections that showed no token yet, from one address off loopback.
const STRANGERS_PER_ADDR: usize = 8;
/// Connections past the limit that are still being told so.
const BUSY_AT_ONCE: usize = 64;
const FAILS: usize = 10;
const FAIL_WINDOW: Duration = Duration::from_secs(10 * 60);
const PENALTY_FIRST: Duration = Duration::from_secs(2);
const PENALTY_MOST: Duration = Duration::from_secs(15 * 60);
const BURST: f64 = 40.0;
const RATE: f64 = 20.0;
const TICKET_BYTES: usize = 24;
const TICKETS_MOST: usize = 1024;

type Body = BoxBody<Bytes, Infallible>;

/// How the server starts.
#[derive(Clone)]
pub struct Options {
    pub addr: SocketAddr,
    /// None: plain HTTP, which only loopback may have.
    pub tls: Option<Arc<ServerConfig>>,
    /// The web pages (whole origins) allowed to call.
    pub origins: Vec<String>,
    /// Where `api/audit.log` goes.
    pub data: PathBuf,
    pub heartbeat: Duration,
    pub ticket_life: Duration,
    /// How long a replaced mirror's streams have to flush `replaced` before they are cut.
    pub replaced_grace: Duration,
    /// This computer's name: what `GET /v1/me` says.
    pub computer: String,
}

const HTTPS_NEEDED: &str = "Off this computer, the API needs HTTPS.";

impl Options {
    /// The options the person's settings ask for, with the certificate loaded. Plain HTTP is
    /// refused off loopback, whatever the file says.
    pub fn from(settings: &Settings, data: &Path) -> Result<Options, String> {
        let ip = match &settings.listen {
            Listen::Loopback => IpAddr::V4(Ipv4Addr::LOCALHOST),
            Listen::All => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            Listen::Address(text) => text
                .trim()
                .parse::<IpAddr>()
                .map_err(|_| format!("{} isn't an IP address.", text.trim()))?,
        };
        let addr = SocketAddr::new(ip, settings.port);
        let tls = match &settings.tls {
            Tls::Off if !ip.to_canonical().is_loopback() => return Err(HTTPS_NEEDED.into()),
            Tls::Off => None,
            Tls::Own => Some(tls::server_config(tls::own(data)?)?),
            Tls::Files { cert, key } => Some(tls::server_config(tls::files(cert, key)?)?),
        };
        let origins = settings
            .origins
            .iter()
            .map(|origin| origin.trim().to_string())
            .filter(|origin| !origin.is_empty())
            .collect();
        Ok(Options {
            addr,
            tls,
            origins,
            data: data.to_path_buf(),
            heartbeat: HEARTBEAT,
            ticket_life: TICKET_LIFE,
            replaced_grace: REPLACED_GRACE,
            computer: crate::computer(),
        })
    }
}

/// A connection that showed a valid token.
#[derive(Clone, Debug, PartialEq)]
pub struct Client {
    pub id: u64,
    pub token: u64,
    pub name: String,
    pub addr: String,
    /// Unix seconds, when it first showed its token.
    pub since: u64,
    pub streaming: bool,
}

struct Conn {
    abort: Option<AbortHandle>,
    addr: String,
    token: Option<(u64, String)>,
    since: u64,
    streaming: bool,
    /// The `Templum-Mirror` value of its newest authenticated request.
    session: Option<String>,
    /// A share's `revoke_share` is being answered on it: a sweep leaves it to answer (its next
    /// request is refused, and its streams end through the hub).
    in_flight: bool,
    /// Another mirror session took its token's place: it no longer counts against the token's 16.
    replaced: bool,
}

#[derive(Default)]
struct Inner {
    tokens: Vec<Token>,
    conns: HashMap<u64, Conn>,
    uses: HashMap<u64, Use>,
    limiter: Limiter,
    buckets: HashMap<u64, Bucket>,
    /// Tickets by the hash of their text: the token they open for, and until when.
    tickets: HashMap<String, (u64, Instant)>,
    /// Over-limit connections being told so.
    busy: usize,
}

impl Inner {
    /// Whether a connection from `peer` may be kept, making room by closing strangers (connections
    /// that showed no token yet) rather than the people who have one. Off loopback an address holds
    /// at most [`STRANGERS_PER_ADDR`] strangers, its oldest making room for its newest; when the
    /// table is full the oldest stranger of the address holding the most goes. Nobody is refused
    /// here, not even a peer the limiter is refusing: authentication runs first, so a good token
    /// always passes (an address shared with someone who guesses must not be locked out). A refused
    /// address gets the same cap: its users open connections in parallel (a dashboard, a proxy, a
    /// CGNAT), and a cap of one would have them abort each other.
    fn make_room(&mut self, peer: IpAddr) -> bool {
        let key = limiter_key(&peer.to_string());
        if !peer.is_loopback() && self.strangers_of(&key).count() >= STRANGERS_PER_ADDR {
            self.close_oldest_stranger(Some(&key));
        }
        if self.conns.len() >= CONNECTIONS {
            let mut held: HashMap<String, usize> = HashMap::new();
            for conn in self.conns.values().filter(|c| c.token.is_none()) {
                *held.entry(limiter_key(&conn.addr)).or_default() += 1;
            }
            // Most strangers; of equals, the one with the older connection (smaller id).
            let heaviest = held
                .into_iter()
                .map(|(key, n)| {
                    let oldest = self.strangers_of(&key).map(|(id, _)| id).min();
                    (n, std::cmp::Reverse(oldest), key)
                })
                .max()
                .map(|(_, _, key)| key);
            self.close_oldest_stranger(heaviest.as_deref());
        }
        self.conns.len() < CONNECTIONS
    }

    /// How many connections `token` holds. The replaced ones are on their way out: they don't count.
    fn holds(&self, token: u64) -> usize {
        self.conns
            .values()
            .filter(|c| !c.replaced && c.token.as_ref().is_some_and(|(t, _)| *t == token))
            .count()
    }

    /// Aborts the oldest connections of `token` that were replaced and are still alive (a stream is
    /// let flush its last frame), down to [`CONNS_PER_TOKEN`]: a mirror in a reconnect loop, a new
    /// session each time, would hold the global table with them.
    fn trim_replaced(&mut self, token: u64) {
        let mut old: Vec<u64> = self
            .conns
            .iter()
            .filter(|(_, c)| c.replaced && c.token.as_ref().is_some_and(|(t, _)| *t == token))
            .map(|(id, _)| *id)
            .collect();
        old.sort_unstable();
        let excess = old.len().saturating_sub(CONNS_PER_TOKEN);
        for id in old.into_iter().take(excess) {
            if let Some(conn) = self.conns.remove(&id)
                && let Some(abort) = conn.abort
            {
                abort.abort();
            }
        }
    }

    /// The strangers (id, connection) whose address counts under `key`.
    fn strangers_of<'a>(&'a self, key: &'a str) -> impl Iterator<Item = (u64, &'a Conn)> {
        self.conns
            .iter()
            .filter(move |(_, c)| c.token.is_none() && limiter_key(&c.addr) == key)
            .map(|(id, c)| (*id, c))
    }

    fn close_oldest_stranger(&mut self, key: Option<&str>) {
        let oldest = key.and_then(|key| self.strangers_of(key).map(|(id, _)| id).min());
        if let Some(conn) = oldest.and_then(|id| self.conns.remove(&id))
            && let Some(abort) = conn.abort
        {
            abort.abort();
        }
    }
}

struct Shared {
    options: Options,
    inner: Mutex<Inner>,
    hub: Mutex<Hub>,
    /// The terminal streams, and the terminals lent for them.
    screens: Mutex<Screens>,
    /// A terminal stream opened: the reader wakes (it sleeps while there are none).
    looking: Notify,
    ids: Ids,
    tx: UnboundedSender<Message>,
    audit: Audit,
    stop: Notify,
    closed: Arc<AtomicBool>,
    published: Notify,
    pending: Mutex<Option<State>>,
    /// The web pages allowed to call: Settings changes them while the server runs.
    origins: Mutex<Vec<String>>,
    next_client: AtomicU64,
}

impl Shared {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn origins(&self) -> Vec<String> {
        self.origins
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn hub(&self) -> MutexGuard<'_, Hub> {
        self.hub.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn screens(&self) -> MutexGuard<'_, Screens> {
        self.screens.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether `token` is still among the tokens and alive, whatever workspaces it shows now.
    fn live(&self, token: &Token) -> bool {
        let now = unix();
        self.inner()
            .tokens
            .iter()
            .any(|t| t.id == token.id && t.live(now))
    }

    /// Whether `token` is live and still shows the workspaces it showed.
    fn current(&self, token: &Token) -> bool {
        let now = unix();
        self.inner()
            .tokens
            .iter()
            .any(|t| t.id == token.id && t.live(now) && t.workspaces == token.workspaces)
    }

    /// Whether a connection streams anything, events or a terminal.
    fn streams_on(&self, conn: u64) -> bool {
        let events = self.hub().on_conn(conn);
        events || self.screens().on_conn(conn)
    }

    /// Ends what belongs to a token that is gone or expired: its connections, its streams, its
    /// tickets, its call bucket. A stream also ends when its token's workspaces are no longer the
    /// ones it opened with (the client opens it again and sees the new list). Called when the
    /// tokens change and at each heartbeat.
    fn sweep(&self, now: u64) {
        let lists = {
            let mut inner = self.inner();
            let lists: Vec<(u64, Option<Vec<String>>)> = inner
                .tokens
                .iter()
                .filter(|t| t.live(now))
                .map(|t| (t.id, t.workspaces.clone()))
                .collect();
            let live: Vec<u64> = lists.iter().map(|(id, _)| *id).collect();
            inner.buckets.retain(|id, _| live.contains(id));
            inner.tickets.retain(|_, (id, _)| live.contains(id));
            for conn in inner.conns.values() {
                if !conn.in_flight
                    && conn
                        .token
                        .as_ref()
                        .is_some_and(|(id, _)| !live.contains(id))
                    && let Some(abort) = &conn.abort
                {
                    abort.abort();
                }
            }
            lists
        };
        let dead = |caller: &Caller| {
            !lists
                .iter()
                .any(|(id, workspaces)| *id == caller.token && *workspaces == caller.workspaces)
        };
        self.hub().cut(dead);
        self.screens().cut(dead);
    }
}

/// The server, while it runs. Dropping it closes the listener and every connection.
pub struct Server {
    shared: Arc<Shared>,
    addr: SocketAddr,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Starts serving. An address in use is an Err that says so.
pub fn start(
    options: Options,
    tokens: Vec<Token>,
    tx: UnboundedSender<Message>,
) -> Result<Server, String> {
    if options.tls.is_none() && !options.addr.ip().to_canonical().is_loopback() {
        return Err(HTTPS_NEEDED.into());
    }
    let listener = std::net::TcpListener::bind(options.addr).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            format!(
                "Port {} is in use: choose another port.",
                options.addr.port()
            )
        } else {
            format!("The API can't listen on {}: {error}.", options.addr)
        }
    })?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .max_blocking_threads(64)
        .build()
        .map_err(|e| e.to_string())?;
    let options_origins = options.origins.clone();
    let shared = Arc::new(Shared {
        audit: Audit::new(&folder(&options.data), AUDIT_CAP),
        options,
        inner: Mutex::new(Inner {
            tokens,
            ..Default::default()
        }),
        hub: Mutex::new(Hub::default()),
        screens: Mutex::new(Screens::default()),
        looking: Notify::new(),
        ids: Ids::new(),
        tx,
        stop: Notify::new(),
        closed: Arc::new(AtomicBool::new(false)),
        published: Notify::new(),
        pending: Mutex::new(None),
        origins: Mutex::new(options_origins),
        next_client: AtomicU64::new(1),
    });
    let served = shared.clone();
    let thread = std::thread::Builder::new()
        .name("templum-api".into())
        .spawn(move || {
            runtime.block_on(async {
                let Ok(listener) = TcpListener::from_std(listener) else {
                    return;
                };
                tokio::spawn(publisher(served.clone()));
                tokio::spawn(heartbeat(served.clone()));
                tokio::spawn(reader(served.clone()));
                let accepting = accept(listener, served.clone());
                let stopped = served.stop.notified();
                futures::pin_mut!(accepting, stopped);
                futures::future::select(accepting, stopped).await;
            });
            // Blocking waits end by themselves within a tick (the closed flag); don't wait for them.
            runtime.shutdown_background();
        })
        .map_err(|e| e.to_string())?;
    Ok(Server {
        shared,
        addr,
        thread: Some(thread),
    })
}

impl Server {
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The tokens now: a connection of one gone or expired ends, with its streams and tickets.
    pub fn set_tokens(&self, tokens: Vec<Token>) {
        self.shared.inner().tokens = tokens;
        self.shared.sweep(unix());
    }

    pub fn clients(&self) -> Vec<Client> {
        let inner = self.shared.inner();
        let mut clients: Vec<Client> = inner
            .conns
            .iter()
            .filter_map(|(id, conn)| {
                let (token, name) = conn.token.clone()?;
                Some(Client {
                    id: *id,
                    token,
                    name,
                    addr: conn.addr.clone(),
                    since: conn.since,
                    streaming: conn.streaming,
                })
            })
            .collect();
        clients.sort_by_key(|c| c.id);
        clients
    }

    /// How many clients are connected: what the title bar's pill says, without a list to copy.
    pub fn client_count(&self) -> usize {
        self.shared
            .inner()
            .conns
            .values()
            .filter(|c| c.token.is_some())
            .count()
    }

    /// Ends a client's connection; its token keeps working.
    pub fn disconnect(&self, client: u64) {
        if let Some(abort) = self
            .shared
            .inner()
            .conns
            .get(&client)
            .and_then(|c| c.abort.clone())
        {
            abort.abort();
        }
    }

    /// Each token's newest use since the last call, then forgotten.
    pub fn uses(&self) -> Vec<(u64, Use)> {
        let mut uses: Vec<(u64, Use)> = self.shared.inner().uses.drain().collect();
        uses.sort_by_key(|(id, _)| *id);
        uses
    }

    /// The state now: each open stream compares it with what it last saw. The comparison is the
    /// server thread's; this only hands the state over.
    pub fn publish(&self, state: State) {
        *self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(state);
        self.shared.published.notify_one();
    }

    /// The web pages allowed to call, from now on.
    pub fn set_origins(&self, origins: Vec<String>) {
        *self
            .shared
            .origins
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = origins;
    }

    /// A happening that isn't state, for the streams whose token sees its workspace.
    pub fn notice(&self, notice: Notice) {
        self.shared
            .hub()
            .notice(&self.shared.ids, notice, Instant::now());
    }

    /// Each token that holds an open event stream now, with the address of that stream's
    /// connection, by token id: who mirrors a shared workspace. (A token with several streams is
    /// there once, with its oldest connection's address.)
    pub fn mirrored(&self) -> Vec<(u64, String)> {
        let open = self.shared.hub().streams_by_conn();
        let inner = self.shared.inner();
        let mut by: Vec<(u64, u64)> = Vec::new();
        for (token, conn) in open {
            match by.iter_mut().find(|(t, _)| *t == token) {
                Some(found) => found.1 = found.1.min(conn),
                None => by.push((token, conn)),
            }
        }
        by.sort();
        by.into_iter()
            .map(|(token, conn)| {
                let addr = inner.conns.get(&conn).map(|c| c.addr.clone());
                (token, addr.unwrap_or_default())
            })
            .collect()
    }

    /// How many event streams are open.
    pub fn streams(&self) -> usize {
        self.shared.hub().len()
    }

    /// The terminals now, as the UI thread lends them (see [`TerminalRef`]). Ignored while no
    /// terminal stream is open: the server keeps no terminal then. A stream whose block is not
    /// among them, or whose token no longer sees its workspace, ends.
    pub fn set_terminals(&self, terminals: Vec<TerminalRef>) {
        self.shared.screens().set(terminals);
    }

    /// How many terminal streams are open (those still being opened included).
    pub fn terminal_streams(&self) -> usize {
        self.shared.screens().len()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        self.shared.stop.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Hands the latest state to the streams, as `Server::publish` gives it.
async fn publisher(shared: Arc<Shared>) {
    let mut publisher = events::Publisher::default();
    loop {
        shared.published.notified().await;
        let state = shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        // What belongs to an expired token ends now, not at the next heartbeat.
        shared.sweep(unix());
        if let Some(state) = state
            && shared.hub().len() > 0
        {
            publisher.publish(&shared.hub, &shared.ids, state);
        }
    }
}

/// Every `heartbeat`: a ping on each stream, and what belongs to an expired token ended.
async fn heartbeat(shared: Arc<Shared>) {
    let mut tick = tokio::time::interval(shared.options.heartbeat);
    tick.tick().await;
    loop {
        tick.tick().await;
        shared.sweep(unix());
        shared.hub().ping();
        shared.screens().ping();
    }
}

/// Reads the terminals that changed, [`FRAME_EVERY`] apart, while a terminal stream is open;
/// asleep while there is none.
async fn reader(shared: Arc<Shared>) {
    let mut tick = tokio::time::interval(FRAME_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if shared.screens().len() == 0 {
            shared.looking.notified().await;
        }
        tick.tick().await;
        terminals::look(&shared.screens, &shared.ids);
    }
}

async fn accept(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            // Out of descriptors, or the like: wait rather than spin.
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let _ = stream.set_nodelay(true);
        let id = shared.next_client.fetch_add(1, Ordering::Relaxed);
        enum Admit {
            Yes,
            /// Past the limit: told so, in plain HTTP only.
            Busy,
            No,
        }
        let admit = {
            let mut inner = shared.inner();
            if inner.make_room(peer.ip().to_canonical()) {
                let conn = Conn {
                    abort: None,
                    addr: peer.ip().to_canonical().to_string(),
                    token: None,
                    since: 0,
                    streaming: false,
                    session: None,
                    in_flight: false,
                    replaced: false,
                };
                inner.conns.insert(id, conn);
                Admit::Yes
            } else if shared.options.tls.is_none() && inner.busy < BUSY_AT_ONCE {
                inner.busy += 1;
                Admit::Busy
            } else {
                Admit::No
            }
        };
        match admit {
            Admit::Yes => {
                let task = tokio::spawn(connection(shared.clone(), stream, peer, id));
                if let Some(conn) = shared.inner().conns.get_mut(&id) {
                    conn.abort = Some(task.abort_handle());
                }
            }
            Admit::Busy => {
                tokio::spawn(busy(shared.clone(), stream));
            }
            Admit::No => {}
        }
    }
}

/// Tells a connection past the limit so, once its request head is in (an answer written over an
/// unread request can be lost to a reset), and closes it.
async fn busy(shared: Arc<Shared>, mut stream: TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    struct Slot(Arc<Shared>);
    impl Drop for Slot {
        fn drop(&mut self) {
            self.0.inner().busy -= 1;
        }
    }
    let _slot = Slot(shared.clone());
    let mut head = Vec::new();
    let read = async {
        let mut buf = [0u8; 1024];
        while head.len() < HEADER_BYTES && !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => head.extend_from_slice(&buf[..n]),
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), read).await;
    let body =
        r#"{"error":{"code":"busy","message":"Too many connections: try again in a moment."}}"#;
    let mut answer = format!(
        "HTTP/1.1 503 Service Unavailable\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (name, value) in security_headers() {
        answer.push_str(&format!("{name}: {value}\r\n"));
    }
    answer.push_str("\r\n");
    answer.push_str(body);
    let _ = stream.write_all(answer.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Takes a connection out of the table, however its task ends.
struct ConnGuard {
    shared: Arc<Shared>,
    id: u64,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        let had_token = self
            .shared
            .inner()
            .conns
            .remove(&self.id)
            .is_some_and(|conn| conn.token.is_some());
        if had_token {
            tell_ui(&self.shared, CLIENTS);
        }
    }
}

/// Who a request comes from.
#[derive(Clone, Copy)]
struct Peer {
    conn: u64,
    addr: SocketAddr,
}

async fn connection(shared: Arc<Shared>, stream: TcpStream, addr: SocketAddr, conn: u64) {
    let _guard = ConnGuard {
        shared: shared.clone(),
        id: conn,
    };
    shared
        .audit
        .write(&addr.ip().to_string(), None, "connect", 0);
    let peer = Peer { conn, addr };
    match shared.options.tls.clone() {
        None => serve(shared, stream, peer).await,
        Some(config) => {
            match tokio::time::timeout(HANDSHAKE_WAIT, TlsAcceptor::from(config).accept(stream))
                .await
            {
                Ok(Ok(secured)) => serve(shared, secured, peer).await,
                _ => shared
                    .audit
                    .write(&addr.ip().to_string(), None, "tls_failed", 0),
            }
        }
    }
}

async fn serve<I>(shared: Arc<Shared>, io: I, peer: Peer)
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |request| handle(shared.clone(), peer, request));
    let mut http = http1::Builder::new();
    http.timer(TokioTimer::new())
        .header_read_timeout(HEADER_WAIT)
        .max_buf_size(HEADER_BYTES);
    let _ = http.serve_connection(TokioIo::new(io), service).await;
}

/// The headers every answer carries (docs/specs/api.md, Every answer). No HSTS: a browser would
/// pin the host for a year, and Templum's own certificate and a tunnel's change.
fn security_headers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("x-content-type-options", "nosniff"),
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-frame-options", "DENY"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
        ("cross-origin-resource-policy", "same-origin"),
    ]
}

/// Whether `origin` is one the person listed, whole.
fn allowed_origin(origin: &str, origins: &[String]) -> bool {
    origins.iter().any(|allowed| allowed == origin)
}

/// The address a request counts against: the peer's, unless the peer is loopback (a tunnel or a
/// proxy on this computer): then the last entry of `X-Forwarded-For`, the one the proxy added
/// (the entries before it are the client's to write), else the peer. No other header is believed:
/// one a client can send could be anyone's address.
fn client_addr(peer: SocketAddr, headers: &HeaderMap) -> String {
    let ip = peer.ip().to_canonical();
    if ip.is_loopback() {
        let forwarded = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .next_back()
            .and_then(|last| last.trim().parse::<IpAddr>().ok());
        if let Some(ip) = forwarded {
            return ip.to_canonical().to_string();
        }
    }
    ip.to_string()
}

/// The addresses the limiter tells apart; past it the rest share one entry.
const CONNS_PER_TOKEN: usize = 16;
/// How long a replaced mirror's streams have to flush `replaced` before they are cut.
const REPLACED_GRACE: Duration = Duration::from_secs(2);
const LIMITER_MOST: usize = 4096;
/// The entry the addresses past [`LIMITER_MOST`] share.
const OVERFLOW: &str = "overflow";
/// The table is swept at most this often.
const PRUNE_EVERY: Duration = Duration::from_secs(1);

/// Failed authentications by address.
#[derive(Default)]
struct Limiter {
    by: HashMap<String, Tries>,
    pruned: Option<Instant>,
}

#[derive(Default)]
struct Tries {
    fails: VecDeque<Instant>,
    until: Option<Instant>,
    penalty: Duration,
}

/// What an address counts under: an IPv6 /64 is one household's, so it is one address.
pub(crate) fn limiter_key(addr: &str) -> String {
    match addr.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        _ => addr.to_string(),
    }
}

impl Limiter {
    /// Where `key` is kept: its own entry, or the shared one when the table is full.
    fn slot(&self, key: String) -> String {
        if self.by.contains_key(&key) || self.by.len() < LIMITER_MOST {
            key
        } else {
            OVERFLOW.to_string()
        }
    }

    /// How long `addr` is refused for, if it is.
    fn refused(&mut self, addr: &str, now: Instant) -> Option<Duration> {
        let slot = self.slot(limiter_key(addr));
        let until = self.by.get(&slot)?.until?;
        (until > now).then(|| until - now)
    }

    /// Forgets what has gone quiet. At most once a second: a flood of new addresses must not make
    /// each of its requests sweep the table.
    fn prune(&mut self, now: Instant) {
        if self
            .pruned
            .is_some_and(|at| now.duration_since(at) < PRUNE_EVERY)
        {
            return;
        }
        self.pruned = Some(now);
        self.by.retain(|_, t| {
            t.until.is_some_and(|until| now < until + FAIL_WINDOW)
                || t.fails
                    .back()
                    .is_some_and(|at| now.duration_since(*at) <= FAIL_WINDOW)
        });
    }

    /// A failed authentication: at the tenth within ten minutes the address is refused for 2
    /// seconds, and for twice as long at each further failure (one made once the refusal ended,
    /// while the address was still in trouble: within ten minutes of it), up to 15 minutes.
    fn failed(&mut self, addr: &str, now: Instant) {
        let key = limiter_key(addr);
        if !self.by.contains_key(&key) && self.by.len() >= LIMITER_MOST {
            self.prune(now);
        }
        let slot = self.slot(key);
        let tries = self.by.entry(slot).or_default();
        while tries
            .fails
            .front()
            .is_some_and(|at| now.duration_since(*at) > FAIL_WINDOW)
        {
            tries.fails.pop_front();
        }
        tries.fails.push_back(now);
        let in_trouble = tries.until.is_some_and(|until| now < until + FAIL_WINDOW);
        if in_trouble || tries.fails.len() >= FAILS {
            tries.penalty = if in_trouble && !tries.penalty.is_zero() {
                (tries.penalty * 2).min(PENALTY_MOST)
            } else {
                PENALTY_FIRST
            };
            tries.until = Some(now + tries.penalty);
        }
    }

    /// A good authentication clears the address.
    fn passed(&mut self, addr: &str) {
        self.by.remove(&limiter_key(addr));
    }
}

/// A token's calls: bursts of 40, 20 a second.
struct Bucket {
    level: f64,
    at: Instant,
}

impl Bucket {
    fn new(now: Instant) -> Bucket {
        Bucket {
            level: BURST,
            at: now,
        }
    }

    /// None: the call may go. Some: how long until it may.
    fn take(&mut self, now: Instant) -> Option<Duration> {
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        self.level = (self.level + elapsed * RATE).min(BURST);
        self.at = now;
        if self.level >= 1.0 {
            self.level -= 1.0;
            None
        } else {
            Some(Duration::from_secs_f64((1.0 - self.level) / RATE))
        }
    }
}

fn full(bytes: impl Into<Bytes>) -> Body {
    Full::new(bytes.into()).boxed()
}

fn json_response(status: u16, value: &Value) -> Response<Body> {
    let mut response = Response::new(full(value.to_string()));
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

/// An error answer: `{"error":{"code":…,"message":…}}`.
fn error(status: u16, code: &str, message: &str) -> Response<Body> {
    error_with(status, code, message, None)
}

/// An error answer with `details` beside its message (git's stderr).
fn error_with(status: u16, code: &str, message: &str, details: Option<&str>) -> Response<Body> {
    let mut body = json!({ "error": { "code": code, "message": message } });
    if let Some(details) = details {
        body["error"]["details"] = json!(details);
    }
    let mut response = json_response(status, &body);
    if status == 401 {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
    }
    response
}

/// The same body whatever was wrong with the credentials.
fn unauthorized() -> Response<Body> {
    error(
        401,
        "auth",
        "A valid token is needed: send Authorization: Bearer <token>.",
    )
}

fn slow_down(wait: Duration, message: &str) -> Response<Body> {
    let mut response = error(429, "slow_down", message);
    let seconds = wait.as_secs_f64().ceil().max(1.0) as u64;
    if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

/// Ends the connection after this answer.
fn closing(mut response: Response<Body>) -> Response<Body> {
    response
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
    response
}

/// Tells the UI thread something about the server (a client came or went), as a bus notification.
fn tell_ui(shared: &Shared, method: &str) {
    let hello = Hello {
        terminal: String::new(),
        client: CLIENT.into(),
        control: None,
    };
    let (reply, _) = std::sync::mpsc::channel();
    let incoming = Incoming::Notification {
        method: method.into(),
        params: Value::Null,
    };
    let _ = shared.tx.unbounded_send(Message {
        hello,
        conn: next_conn(),
        incoming,
        reply,
    });
}

fn quitting() -> Response<Body> {
    error(503, "busy", "Templum is quitting.")
}

/// Puts the headers of every answer on `response`, and the CORS ones when `origin` is allowed.
fn decorate(
    mut response: Response<Body>,
    shared: &Shared,
    origin: Option<&str>,
    preflight: bool,
) -> Response<Body> {
    let headers = response.headers_mut();
    for (name, value) in security_headers() {
        headers.insert(
            header::HeaderName::from_static(name),
            HeaderValue::from_static(value),
        );
    }
    if !shared.origins().is_empty() {
        headers.append(header::VARY, HeaderValue::from_static("Origin"));
    }
    if let Some(origin) = origin
        && let Ok(value) = HeaderValue::from_str(origin)
    {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
        headers.insert(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            HeaderValue::from_static("Retry-After"),
        );
        if preflight {
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static("Authorization, Content-Type, Last-Event-ID"),
            );
            headers.insert(
                header::ACCESS_CONTROL_MAX_AGE,
                HeaderValue::from_static("600"),
            );
        }
    }
    response
}

async fn handle(
    shared: Arc<Shared>,
    peer: Peer,
    request: Request<RequestBody>,
) -> Result<Response<Body>, Infallible> {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .filter(|origin| allowed_origin(origin, &shared.origins()))
        .map(str::to_string);
    if request.method() == hyper::Method::OPTIONS {
        // A preflight carries no credentials: it is answered for any path, to an allowed origin only.
        let mut response = Response::new(full(Bytes::new()));
        *response.status_mut() = StatusCode::NO_CONTENT;
        return Ok(decorate(response, &shared, origin.as_deref(), true));
    }
    let response = route(&shared, peer, request).await;
    Ok(decorate(response, &shared, origin.as_deref(), false))
}

/// The credentials a request shows: a bearer token, or (on the event stream only) a ticket.
enum Credentials {
    Bearer(String),
    Ticket(String),
    None,
}

fn credentials(request: &Request<RequestBody>, query: &[(String, String)]) -> Credentials {
    let bearer = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim().to_string());
    if let Some(token) = bearer {
        return Credentials::Bearer(token);
    }
    // A ticket opens a stream, the event stream or a terminal's, and nothing else.
    if request.method() == hyper::Method::GET
        && registry::find("GET", request.uri().path())
            .is_some_and(|(found, _)| matches!(found.does, Does::Events | Does::Frames))
        && let Some((_, ticket)) = query.iter().find(|(key, _)| key == "ticket")
    {
        return Credentials::Ticket(ticket.clone());
    }
    Credentials::None
}

/// The token a request is for, if its credentials are good. The caller keeps the lock until it has
/// marked the connection as authenticated: between the two an accept could evict it as a stranger.
fn authenticate(
    inner: &mut Inner,
    credentials: Credentials,
    now: u64,
    clock: Instant,
) -> Option<Token> {
    match credentials {
        Credentials::Bearer(text) => verify(&inner.tokens, &text, now).cloned(),
        // A ticket opens once, within its life, for a token that still lives.
        Credentials::Ticket(text) => inner
            .tickets
            .remove(&hash(&text))
            .filter(|(_, until)| clock < *until)
            .and_then(|(id, _)| {
                inner
                    .tokens
                    .iter()
                    .find(|t| t.id == id && t.live(now))
                    .cloned()
            }),
        Credentials::None => None,
    }
}

/// The `Templum-Mirror` header: a mirror's session, at most 64 characters (a longer one is none).
fn mirror_session(headers: &HeaderMap) -> Option<String> {
    headers
        .get(crate::MIRROR_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty() && v.chars().count() <= 64)
        .map(str::to_string)
}

/// `conn` of `token` opens its event stream in `session`: every other connection of the token
/// that carries another session is cut. Its event streams are sent `replaced` and end (the
/// connection closes with the body, once the frame is written), so are its terminal streams, and
/// the connections with nothing streaming are aborted. The connections cut. The caller holds
/// `inner`.
fn replace_session(
    shared: &Arc<Shared>,
    inner: &mut Inner,
    token: &Token,
    conn: u64,
    session: &str,
) -> Vec<u64> {
    if let Some(mine) = inner.conns.get_mut(&conn) {
        mine.session = Some(session.to_string());
    }
    let old: Vec<u64> = inner
        .conns
        .iter()
        .filter(|(id, c)| {
            **id != conn
                && c.token.as_ref().is_some_and(|(t, _)| *t == token.id)
                && c.session.as_deref() != Some(session)
        })
        .map(|(id, _)| *id)
        .collect();
    if old.is_empty() {
        return old;
    }
    let streaming: Vec<u64> = old
        .iter()
        .copied()
        .filter(|id| shared.streams_on(*id))
        .collect();
    shared.hub().replace(token.id, &old, &shared.ids);
    shared.screens().end_on(token.id, &old);
    for id in &old {
        let Some(conn) = inner.conns.get_mut(id) else {
            continue;
        };
        conn.replaced = true;
        if !streaming.contains(id)
            && let Some(abort) = &conn.abort
        {
            abort.abort();
        }
    }
    inner.trim_replaced(token.id);
    // A streaming one is let flush its last frame, and then cut: a dead peer's full send buffer
    // would hold it for minutes.
    if !streaming.is_empty() {
        let shared = shared.clone();
        tokio::spawn(async move {
            tokio::time::sleep(shared.options.replaced_grace).await;
            for id in streaming {
                if let Some(abort) = shared.inner().conns.get(&id).and_then(|c| c.abort.clone()) {
                    abort.abort();
                }
            }
        });
    }
    old
}

/// What the audit log says a request was.
fn what_of(route: &registry::Route, params: &[(&'static str, String)], path: &str) -> String {
    match route.does {
        Does::Tool("") => params
            .iter()
            .find(|(k, _)| *k == "name")
            .map(|(_, v)| v.clone())
            .unwrap_or_default(),
        Does::Tool(name) => name.to_string(),
        _ => path.chars().take(200).collect(),
    }
}

async fn route(shared: &Arc<Shared>, peer: Peer, request: Request<RequestBody>) -> Response<Body> {
    let (clock, now) = (Instant::now(), unix());
    let addr = client_addr(peer.addr, request.headers());
    let query: Vec<(String, String)> = request
        .uri()
        .query()
        .map(|q| form_urlencoded::parse(q.as_bytes()).into_owned().collect())
        .unwrap_or_default();
    let credentials = credentials(&request, &query);
    let shown = !matches!(credentials, Credentials::None);
    let session = mirror_session(request.headers());
    let opens_events = request.method() == hyper::Method::GET
        && registry::find("GET", request.uri().path())
            .is_some_and(|(found, _)| matches!(found.does, Does::Events));
    // Good credentials always pass; the limiter slows only the failures, so a stranger who forges
    // an address can't lock the person out. A connection that fails isn't kept: it would hold one
    // of the 64 for nothing.
    // One lock from the credentials to the connection being marked: between the two an accept could
    // evict it as a stranger. (The audit never waits, so writing under it is fine.)
    let admitted = 'admit: {
        let mut inner = shared.inner();
        let Some(token) = authenticate(&mut inner, credentials, now, clock) else {
            let wait = inner.limiter.refused(&addr, clock);
            // Showing no credentials at all isn't a failed attempt.
            if wait.is_none() && shown {
                inner.limiter.failed(&addr, clock);
            }
            break 'admit Err(closing(match wait {
                Some(wait) => {
                    shared.audit.write(&addr, None, "refused", 429);
                    slow_down(wait, "Too many failed attempts from this address: wait.")
                }
                None => {
                    shared.audit.write(&addr, None, "auth_failed", 401);
                    unauthorized()
                }
            }));
        };
        inner.limiter.passed(&addr);
        // A share's token opening its event stream from a new session ends the old session's
        // connections first: a laptop that slept leaves them open, and they would hold the
        // token's 16.
        let replaced = match &session {
            Some(session) if token.share.is_some() && opens_events => {
                replace_session(shared, &mut inner, &token, peer.conn, session)
            }
            _ => Vec::new(),
        };
        if !replaced.is_empty() {
            shared
                .audit
                .write(&addr, Some(&token.name), "mirror_replaced", 200);
        }
        let mine = inner
            .conns
            .get(&peer.conn)
            .is_some_and(|c| c.token.as_ref().is_some_and(|(t, _)| *t == token.id));
        if !mine && inner.holds(token.id) >= CONNS_PER_TOKEN {
            shared
                .audit
                .write(&addr, Some(&token.name), "slow_down", 429);
            break 'admit Err(closing(slow_down(
                Duration::from_secs(1),
                "This token holds as many connections as it may.",
            )));
        }
        inner.uses.insert(
            token.id,
            Use {
                at: now,
                addr: addr.clone(),
            },
        );
        let mut joined = false;
        if let Some(conn) = inner.conns.get_mut(&peer.conn) {
            if conn.token.as_ref().map(|(id, _)| *id) != Some(token.id) {
                joined = conn.token.is_none();
                conn.token = Some((token.id, token.name.clone()));
                conn.since = now;
            }
            conn.addr = addr.clone();
            conn.session = session.clone();
        }
        let wait = inner
            .buckets
            .entry(token.id)
            .or_insert_with(|| Bucket::new(clock))
            .take(clock);
        if let Some(wait) = wait {
            shared
                .audit
                .write(&addr, Some(&token.name), "slow_down", 429);
            break 'admit Err(slow_down(
                wait,
                "Too many calls from this token: slow down.",
            ));
        }
        Ok((token, joined))
    };
    let (token, joined) = match admitted {
        Ok(admitted) => admitted,
        Err(response) => return response,
    };
    if joined {
        tell_ui(shared, CLIENTS);
    }
    let path = request.uri().path().to_string();
    let method = request.method().as_str().to_string();
    let Some((found, params)) = registry::find(&method, &path) else {
        shared.audit.write(
            &addr,
            Some(&token.name),
            &path.chars().take(200).collect::<String>(),
            404,
        );
        return error(
            404,
            "not_found",
            "There's no such route: GET /v1/openapi.json lists them.",
        );
    };
    let what = what_of(found, &params, &path);
    // While a share's revoke is answered a sweep must not cut its connection: the share gets its
    // answer. Every other call of a token that goes is cut with it, a waiting one included.
    struct InFlight<'a>(&'a Shared, u64);
    impl InFlight<'_> {
        fn set(&self, on: bool) {
            if let Some(conn) = self.0.inner().conns.get_mut(&self.1) {
                conn.in_flight = on;
            }
        }
    }
    impl Drop for InFlight<'_> {
        fn drop(&mut self) {
            self.set(false);
        }
    }
    let flight = InFlight(shared, peer.conn);
    flight.set(what == "revoke_share");
    let response = dispatch(shared, peer, &token, found, params, query, request).await;
    drop(flight);
    let what = audited(&response, what);
    shared
        .audit
        .write(&addr, Some(&token.name), &what, response.status().as_u16());
    response
}

/// A body method's JSON, at most [`BODY_MAX`]: Err is the answer to give.
#[allow(clippy::result_large_err)] // the Err is an answer to give, once: boxing buys nothing
async fn json_body(request: Request<RequestBody>) -> Result<Option<Value>, Response<Body>> {
    let json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"));
    let typed = request.headers().contains_key(header::CONTENT_TYPE);
    let length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok());
    let too_large = || {
        let mut response = error(413, "too_large", "A body is at most 1 MiB.");
        response
            .headers_mut()
            .insert(header::CONNECTION, HeaderValue::from_static("close"));
        response
    };
    if length.is_some_and(|n| n > BODY_MAX + DRAIN) {
        return Err(too_large());
    }
    let mut body = request.into_body();
    let mut bytes: Vec<u8> = Vec::new();
    let mut total = 0usize;
    let read = async {
        // An oversize body is read on to its end (up to DRAIN more) and dropped, so the client
        // that wrote it can read the answer instead of meeting a reset.
        while let Some(frame) = body.frame().await {
            let Ok(frame) = frame else { return false };
            if let Some(data) = frame.data_ref() {
                total += data.len();
                if total <= BODY_MAX {
                    bytes.extend_from_slice(data);
                } else if total > BODY_MAX + DRAIN {
                    break;
                }
            }
        }
        true
    };
    match tokio::time::timeout(BODY_WAIT, read).await {
        Ok(true) => {}
        Ok(false) => return Err(error(400, "invalid", "The body couldn't be read.")),
        Err(_) => return Err(error(400, "invalid", "The body took too long.")),
    }
    if total > BODY_MAX {
        return Err(too_large());
    }
    // Refused once read, as an oversize body: answered before, a client still writing it met a
    // reset (Windows), and one that kept the connection found it closed.
    if typed && !json {
        return Err(error(
            415,
            "json",
            "A POST, PUT or PATCH must be application/json.",
        ));
    }
    // No type is fine for no body (a button pressed); a body says what it is.
    if !typed && total > 0 {
        return Err(error(
            415,
            "json",
            "A POST, PUT or PATCH must be application/json.",
        ));
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| error(400, "invalid", &format!("The body isn't JSON: {e}.")))
}

async fn dispatch(
    shared: &Arc<Shared>,
    peer: Peer,
    token: &Token,
    route: &'static registry::Route,
    params: Vec<(&'static str, String)>,
    query: Vec<(String, String)>,
    request: Request<RequestBody>,
) -> Response<Body> {
    let body = if matches!(route.method, "POST" | "PUT" | "PATCH") {
        match json_body(request).await {
            Ok(body) => body,
            Err(response) => return response,
        }
    } else {
        let last_event = request
            .headers()
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        return match route.does {
            Does::Me => json_response(200, &val(&MeView::of(token, &shared.options.computer))),
            Does::Tools => json_response(200, &json!(registry::listed())),
            Does::OpenApi => json_response(200, &openapi::document()),
            Does::Events => {
                let view = match query.iter().find(|(key, _)| key == "view") {
                    None => false,
                    Some((_, value)) if value == "true" => true,
                    Some((_, value)) if value == "false" => false,
                    Some(_) => return error(400, "invalid", "\"view\" is true or false."),
                };
                stream(shared, peer, token, last_event, view).await
            }
            Does::Frames => {
                let block = params
                    .iter()
                    .find(|(k, _)| *k == "block")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or_default();
                terminal_stream(shared, peer, token, block).await
            }
            Does::Read(resource) => read(shared, token, resource, &params, &query).await,
            Does::Tool(name) => tool(shared, token, name, &params, &query, None).await,
            Does::Ticket => unreachable!("a ticket is a POST"),
        };
    };
    match route.does {
        Does::Ticket => ticket(shared, token),
        Does::Tool("") => {
            let name = params
                .iter()
                .find(|(k, _)| *k == "name")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            tool(shared, token, &name, &[], &[], body).await
        }
        Does::Tool(name) => tool(shared, token, name, &params, &query, body).await,
        _ => error(
            404,
            "not_found",
            "There's no such route: GET /v1/openapi.json lists them.",
        ),
    }
}

/// `POST /v1/tickets`: a key for one event stream.
fn ticket(shared: &Shared, token: &Token) -> Response<Body> {
    let (clock, life) = (Instant::now(), shared.options.ticket_life);
    let text = random_text("tkt_", TICKET_BYTES);
    {
        let mut inner = shared.inner();
        inner.tickets.retain(|_, (_, until)| clock < *until);
        if inner.tickets.len() >= TICKETS_MOST {
            drop(inner);
            return slow_down(
                Duration::from_secs(1),
                "Too many tickets are waiting: use one.",
            );
        }
        inner.tickets.insert(hash(&text), (token.id, clock + life));
    }
    json_response(
        200,
        &json!({ "ticket": text, "expires_in": life.as_secs() }),
    )
}

/// The line the UI thread answers with, or the answer to give instead.
#[allow(clippy::result_large_err)] // the Err is an answer to give, once: boxing buys nothing
async fn ask(
    shared: &Arc<Shared>,
    token: &Token,
    method: &str,
    params: Value,
) -> Result<Value, Response<Body>> {
    /// Tells the UI thread nobody waits for the answer anymore, if the request is dropped first.
    struct Guard {
        tx: UnboundedSender<Message>,
        hello: Hello,
        conn: u64,
        cancel: Arc<AtomicBool>,
        armed: bool,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            if self.armed {
                self.cancel.store(true, Ordering::Release);
                let (reply, _) = std::sync::mpsc::channel();
                let closed = Incoming::Notification {
                    method: CLOSED.into(),
                    params: Value::Null,
                };
                let _ = self.tx.unbounded_send(Message {
                    hello: self.hello.clone(),
                    conn: self.conn,
                    incoming: closed,
                    reply,
                });
            }
        }
    }
    let hello = Hello {
        terminal: token.id.to_string(),
        client: CLIENT.into(),
        control: None,
    };
    let conn = next_conn();
    let (reply, lines) = std::sync::mpsc::channel::<String>();
    let incoming = Incoming::Request {
        id: json!(1),
        method: method.into(),
        params,
    };
    if shared
        .tx
        .unbounded_send(Message {
            hello: hello.clone(),
            conn,
            incoming,
            reply,
        })
        .is_err()
    {
        return Err(quitting());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let mut guard = Guard {
        tx: shared.tx.clone(),
        hello,
        conn,
        cancel: cancel.clone(),
        armed: true,
    };
    let (sender, answer) = tokio::sync::oneshot::channel();
    let closed = shared.closed.clone();
    tokio::task::spawn_blocking(move || {
        let line = loop {
            match lines.recv_timeout(Duration::from_millis(100)) {
                Ok(line) => break Some(line),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if cancel.load(Ordering::Acquire) || closed.load(Ordering::Acquire) {
                        break None;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break None,
            }
        };
        let _ = sender.send(line);
    });
    let line = answer.await.ok().flatten();
    guard.armed = false;
    let Some(line) = line else {
        return Err(error(
            500,
            "internal",
            "Templum dropped the call without an answer.",
        ));
    };
    serde_json::from_str(&line)
        .map_err(|_| error(500, "internal", "Templum's answer wasn't understood."))
}

/// The state, as the token sees it.
#[allow(clippy::result_large_err)] // the Err is an answer to give, once: boxing buys nothing
async fn state_of(shared: &Arc<Shared>, token: &Token) -> Result<State, Response<Body>> {
    let answer = ask(shared, token, STATE, Value::Null).await?;
    if let Some(failed) = answer.get("error") {
        return Err(failed_answer(failed));
    }
    let state: State = serde_json::from_value(answer["result"].clone())
        .map_err(|_| error(500, "internal", "Templum's state wasn't understood."))?;
    Ok(state.visible(&Caller::of(token)))
}

/// A JSON-RPC error from the UI thread as an answer.
fn failed_answer(failed: &Value) -> Response<Body> {
    let message = failed["message"].as_str().unwrap_or("Failed.");
    match failed["code"].as_i64().and_then(|c| i32::try_from(c).ok()) {
        Some(codes::SCOPE) => error(403, "scope", message),
        Some(codes::AUTH) => error(401, "auth", message),
        Some(codes::NOT_FOUND) => error(404, "not_found", message),
        Some(rpc::PARAMS) => error(400, "invalid", message),
        _ => error(500, "internal", message),
    }
}

/// A tool's reply line as an answer: its text (200), the tool's refusal (422) or an error.
fn tool_answer(answer: &Value) -> Response<Body> {
    if let Some(failed) = answer.get("error") {
        return failed_answer(failed);
    }
    let result = &answer["result"];
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    if result["isError"] == true {
        return error_with(422, "refused", text, result["details"].as_str());
    }
    let mut out = json!({ "text": text });
    // The git tool's result, whatever its shape (null included).
    if let Some(git) = result.get("git") {
        out["git"] = git.clone();
    }
    // A share's connection code: given once, in the answer.
    if let Some(code) = result["code"].as_str() {
        out["code"] = json!(code);
    }
    for key in ["workspace", "block"] {
        if let Some(value) = result["target"][key].as_str() {
            out[key] = json!(value);
        }
    }
    json_response(200, &out)
}

async fn tool(
    shared: &Arc<Shared>,
    token: &Token,
    name: &str,
    params: &[(&'static str, String)],
    query: &[(String, String)],
    body: Option<Value>,
) -> Response<Body> {
    if !registry::known(name) {
        return error(
            404,
            "not_found",
            &format!("Unknown tool: {name}. GET /v1/tools lists them."),
        );
    }
    let args = match registry::arguments(name, params, query, body) {
        Ok(args) => args,
        Err(why) => return error(400, "invalid", &why),
    };
    match registry::scope(name, &args) {
        Err(why) => return error(400, "invalid", &why),
        Ok(needs) if needs > token.scope => {
            return audited_as(
                error(403, "scope", &registry::needs_words(name, &args, needs)),
                name,
                &args,
            );
        }
        Ok(_) => {}
    }
    // The API's own tools are checked here too, so a call the UI thread would refuse for its
    // arguments never reaches it.
    if registry::own(name)
        && let Err(why) = registry::parse_own(name, &args)
    {
        return error(400, "invalid", &why);
    }
    match ask(
        shared,
        token,
        "tools/call",
        json!({ "name": name, "arguments": args }),
    )
    .await
    {
        Ok(answer) => audited_as(tool_answer(&answer), name, &args),
        Err(response) => audited_as(response, name, &args),
    }
}

/// What the audit log writes for a call: `git:<op>` for the git tool (its operation's scope and
/// noise differ), else the tool's name.
#[derive(Clone)]
struct Audited(String);

/// A response of the git tool marked with its operation (known by now: its scope was found).
fn audited_as(mut response: Response<Body>, name: &str, args: &Value) -> Response<Body> {
    if name == "git"
        && let Some(op) = args["op"].as_str()
    {
        response
            .extensions_mut()
            .insert(Audited(format!("git:{op}")));
    }
    response
}

/// The audit line's `what`: the response's own when the call marked one.
fn audited(response: &Response<Body>, what: String) -> String {
    response
        .extensions()
        .get::<Audited>()
        .map_or(what, |a| a.0.clone())
}

/// The words of a target that isn't there.
fn missing(word: &str, id: &str) -> Response<Body> {
    error(404, "not_found", &registry::no(word, id))
}

fn val<T: Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A GET of a resource: the state, or a slice of it.
async fn read(
    shared: &Arc<Shared>,
    token: &Token,
    resource: Resource,
    params: &[(&'static str, String)],
    query: &[(String, String)],
) -> Response<Body> {
    let state = match state_of(shared, token).await {
        Ok(state) => state,
        Err(response) => return response,
    };
    let param = |key: &str| {
        params
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    };
    let number = |key: &str| param(key).parse::<u64>().ok();
    let one = |found: Option<Value>, word: &str, key: &str| match found {
        Some(value) => json_response(200, &value),
        None => missing(word, param(key)),
    };
    match resource {
        Resource::State => json_response(200, &val(&state)),
        Resource::Workspaces => json_response(200, &val(&state.workspaces)),
        Resource::Workspace => one(
            state
                .workspaces
                .iter()
                .find(|w| w.id == param("workspace") || w.name == param("workspace"))
                .map(val),
            "workspace",
            "workspace",
        ),
        Resource::Blocks => {
            let filter = query
                .iter()
                .find(|(k, _)| k == "workspace")
                .map(|(_, v)| v.as_str());
            let blocks: Vec<_> = state
                .blocks
                .iter()
                .filter(|b| filter.is_none_or(|w| b.workspace == w))
                .collect();
            json_response(200, &val(&blocks))
        }
        Resource::Block => one(
            state
                .blocks
                .iter()
                .find(|b| b.id == param("block"))
                .map(val),
            "block",
            "block",
        ),
        Resource::Agents => {
            let agents: Vec<_> = state.blocks.iter().filter(|b| b.agent.is_some()).collect();
            json_response(200, &val(&agents))
        }
        Resource::Requests => json_response(200, &val(&state.requests)),
        Resource::Request => one(
            state
                .requests
                .iter()
                .find(|r| r.id == param("request"))
                .map(val),
            "request",
            "request",
        ),
        Resource::Boards => json_response(200, &val(&state.boards)),
        Resource::Board => one(
            number("board")
                .and_then(|id| state.boards.iter().find(|b| b.id == id))
                .map(val),
            "board",
            "board",
        ),
        Resource::Card => one(
            number("card")
                .and_then(|id| state.cards.iter().find(|c| c.id == id))
                .map(val),
            "card",
            "card",
        ),
        Resource::Automations => json_response(200, &val(&state.automations)),
        Resource::Automation => one(
            number("automation")
                .and_then(|id| state.automations.iter().find(|a| a.id == id))
                .map(val),
            "automation",
            "automation",
        ),
        Resource::Shares => json_response(200, &val(&state.shares)),
        Resource::Usage => json_response(200, &val(&state.usage.unwrap_or_default())),
    }
}

/// `GET /v1/events`: the snapshot, then the changes.
async fn stream(
    shared: &Arc<Shared>,
    peer: Peer,
    token: &Token,
    last_event: Option<String>,
    view: bool,
) -> Response<Body> {
    if shared.hub().of_token(token.id) >= events::STREAMS_PER_TOKEN {
        return slow_down(
            Duration::from_secs(1),
            "This token holds as many streams as it may.",
        );
    }
    let state = match state_of(shared, token).await {
        Ok(state) => state,
        Err(response) => return response,
    };
    let after = last_event.as_deref().and_then(|id| shared.ids.parse(id));
    let snapshot = events::snapshot(&shared.ids, &state);
    let caller = Caller::of(token);
    let opened = shared.hub().open(
        caller,
        state,
        peer.conn,
        snapshot,
        after,
        Instant::now(),
        view,
    );
    let Ok(opened) = opened else {
        return slow_down(
            Duration::from_secs(1),
            "This token holds as many streams as it may.",
        );
    };
    // The token may have been revoked, or given other workspaces, while the state was asked for:
    // a sweep that ran before the stream was in the hub didn't cut it. A revoked token is a 401;
    // a changed list is retryable, and the client opens again and sees the new list.
    if !shared.live(token) {
        shared.hub().close(opened.id);
        return unauthorized();
    }
    if !shared.current(token) {
        shared.hub().close(opened.id);
        return slow_down(
            Duration::from_secs(1),
            "This token's workspaces changed; open the stream again.",
        );
    }
    if let Some(conn) = shared.inner().conns.get_mut(&peer.conn) {
        conn.streaming = true;
    }
    // The sidebar's marks and the Edit… dialog's rows show who mirrors now.
    tell_ui(shared, CLIENTS);
    if opened.first {
        let hello = Hello {
            terminal: token.id.to_string(),
            client: CLIENT.into(),
            control: None,
        };
        let (reply, _) = std::sync::mpsc::channel();
        let incoming = Incoming::Notification {
            method: WATCH.into(),
            params: Value::Null,
        };
        let _ = shared.tx.unbounded_send(Message {
            hello,
            conn: next_conn(),
            incoming,
            reply,
        });
    }
    let (id, conn) = (opened.id, peer.conn);
    let ended = shared.clone();
    let body = StreamBody::new(opened.cell, move || {
        ended.hub().close(id);
        let more = ended.streams_on(conn);
        if let Some(conn) = ended.inner().conns.get_mut(&conn) {
            conn.streaming = more;
        }
        tell_ui(&ended, CLIENTS);
    });
    event_stream(body)
}

/// A response that streams `body` as server-sent events.
fn event_stream(body: StreamBody) -> Response<Body> {
    let mut response = Response::new(BodyExt::boxed(body));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

/// `GET /v1/terminals/{block}/stream`: a terminal's screen, whole and then the rows that change.
/// The stream is opened first, so the terminals are kept while the UI thread checks the block
/// and lends them.
async fn terminal_stream(
    shared: &Arc<Shared>,
    peer: Peer,
    token: &Token,
    block: &str,
) -> Response<Body> {
    /// Takes the stream out of the table however the request ends: refused, dropped by the
    /// client while the UI thread answers, or the body's end.
    struct Opened {
        shared: Arc<Shared>,
        id: u64,
        conn: u64,
    }
    impl Drop for Opened {
        fn drop(&mut self) {
            self.shared.screens().close(self.id);
            let more = self.shared.streams_on(self.conn);
            if let Some(conn) = self.shared.inner().conns.get_mut(&self.conn) {
                conn.streaming = more;
            }
        }
    }
    let (id, cell) = shared.screens().open(Caller::of(token), block, peer.conn);
    let opened = Opened {
        shared: shared.clone(),
        id,
        conn: peer.conn,
    };
    shared.looking.notify_one();
    match ask(shared, token, TERMINAL, json!({ "block": block })).await {
        Ok(answer) => {
            if answer.get("error").is_some() || answer["result"]["isError"] == true {
                return tool_answer(&answer);
            }
        }
        Err(response) => return response,
    }
    // The UI thread lent the terminals; the stream may have ended since (its block closed).
    if !shared.screens().admits(id) {
        return missing("block", block);
    }
    if let Some(conn) = shared.inner().conns.get_mut(&peer.conn) {
        conn.streaming = true;
    }
    event_stream(StreamBody::new(cell, move || drop(opened)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderName;
    use templum_mcp::tools;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn peer(ip: &str) -> SocketAddr {
        SocketAddr::new(ip.parse().unwrap(), 5000)
    }

    #[test]
    fn client_address_trusts_forwarding_only_from_loopback() {
        let xff = headers(&[("x-forwarded-for", "1.1.1.1, 203.0.113.9")]);
        assert_eq!(client_addr(peer("127.0.0.1"), &xff), "203.0.113.9");
        let two_lines = headers(&[
            ("x-forwarded-for", "1.1.1.1"),
            ("x-forwarded-for", "203.0.113.9"),
        ]);
        assert_eq!(client_addr(peer("::1"), &two_lines), "203.0.113.9");
        assert_eq!(client_addr(peer("192.168.1.5"), &xff), "192.168.1.5");
        assert_eq!(
            client_addr(peer("127.0.0.1"), &HeaderMap::new()),
            "127.0.0.1"
        );
        // A header a client can send is believed by no one: Cloudflare's, or a forged first entry.
        let cf = headers(&[("cf-connecting-ip", "203.0.113.7")]);
        assert_eq!(client_addr(peer("127.0.0.1"), &cf), "127.0.0.1");
        let both = headers(&[
            ("cf-connecting-ip", "203.0.113.7"),
            ("x-forwarded-for", "9.9.9.9"),
        ]);
        assert_eq!(client_addr(peer("127.0.0.1"), &both), "9.9.9.9");
        let forged = headers(&[("x-forwarded-for", "203.0.113.7, 10.0.0.1")]);
        assert_eq!(
            client_addr(peer("127.0.0.1"), &forged),
            "10.0.0.1",
            "the proxy's entry is the last"
        );
        // Nonsense is no address.
        let junk = headers(&[("x-forwarded-for", "not an ip")]);
        assert_eq!(client_addr(peer("127.0.0.1"), &junk), "127.0.0.1");
        assert_eq!(client_addr(peer("::ffff:127.0.0.1"), &xff), "203.0.113.9");
    }

    fn stranger(addr: &str) -> Conn {
        Conn {
            abort: None,
            addr: addr.into(),
            token: None,
            since: 0,
            streaming: false,
            session: None,
            in_flight: false,
            replaced: false,
        }
    }

    #[test]
    fn replaced_connections_do_not_count_against_the_tokens_sixteen() {
        let mut inner = Inner::default();
        for n in 0..20u64 {
            let mut conn = stranger("203.0.113.9");
            conn.token = Some((7, "phone".into()));
            // A mirror that slept: streaming, replaced, and not yet closed.
            conn.streaming = true;
            conn.replaced = n < 15;
            inner.conns.insert(n, conn);
        }
        inner.conns.insert(99, stranger("203.0.113.9"));
        assert_eq!(inner.holds(7), 5);
        assert_eq!(inner.holds(8), 0);
    }

    #[test]
    fn a_tokens_replaced_connections_are_trimmed_to_sixteen_oldest_first() {
        let mut inner = Inner::default();
        for n in 0..40u64 {
            let mut conn = stranger("203.0.113.9");
            // Token 7's: 0..38 replaced, 38 live; token 8's: 39, replaced.
            conn.token = Some((if n < 39 { 7 } else { 8 }, "phone".into()));
            conn.replaced = n < 38 || n == 39;
            inner.conns.insert(n, conn);
        }
        inner.trim_replaced(7);
        let kept: Vec<u64> = (0..40).filter(|n| inner.conns.contains_key(n)).collect();
        // 38 replaced of token 7 (0..38): the oldest 22 go; 22..38, the live 38 and token 8's stay.
        assert_eq!(kept, (22..40).collect::<Vec<_>>());
    }

    #[test]
    fn strangers_from_one_address_cant_crowd_out_the_rest() {
        let mut inner = Inner::default();
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        // One address (an IPv6 /64) holds at most eight: its oldest makes room for its newest.
        for n in 0..STRANGERS_PER_ADDR as u64 {
            inner
                .conns
                .insert(n, stranger(&format!("2001:db8:1:2::{n:x}")));
        }
        assert!(inner.make_room(ip("2001:db8:1:2::ff")));
        assert!(
            !inner.conns.contains_key(&0),
            "the oldest of that address went"
        );
        assert_eq!(inner.conns.len(), STRANGERS_PER_ADDR - 1);
        // Full: the oldest stranger of the address holding the most goes, not the oldest overall.
        let mut inner = Inner::default();
        inner.conns.insert(0, stranger("198.51.100.1"));
        for n in 1..CONNECTIONS as u64 {
            inner.conns.insert(n, stranger("203.0.113.9"));
        }
        assert!(inner.make_room(ip("192.0.2.1")));
        assert!(inner.conns.contains_key(&0), "the lone stranger stays");
        assert!(!inner.conns.contains_key(&1), "the flood's oldest went");
        // Who showed a token is never closed for a stranger.
        let mut inner = Inner::default();
        for n in 0..CONNECTIONS as u64 {
            let mut conn = stranger("203.0.113.9");
            conn.token = Some((1, "phone".into()));
            inner.conns.insert(n, conn);
        }
        assert!(!inner.make_room(ip("192.0.2.1")));
        assert_eq!(inner.conns.len(), CONNECTIONS);
        // Loopback is a proxy or a tunnel for many: no cap of eight.
        let mut inner = Inner::default();
        for n in 0..20 {
            inner.conns.insert(n, stranger("127.0.0.1"));
        }
        assert!(inner.make_room(ip("127.0.0.1")));
        assert_eq!(inner.conns.len(), 20);
    }

    #[test]
    fn a_good_token_from_a_refused_address_is_let_in() {
        let (mut inner, t) = (Inner::default(), Instant::now());
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        for _ in 0..FAILS {
            inner.limiter.failed("192.0.2.7", t);
        }
        assert!(inner.limiter.refused("192.0.2.7", t).is_some());
        // Never refused at accept: its token is only looked at once it speaks.
        assert!(inner.make_room(ip("192.0.2.7")));
        // Its strangers keep the normal cap: users behind one address open connections in parallel,
        // and must not abort each other.
        for n in 1..STRANGERS_PER_ADDR as u64 {
            inner.conns.insert(n, stranger("192.0.2.7"));
        }
        assert!(inner.make_room(ip("192.0.2.7")));
        assert_eq!(
            inner.conns.len(),
            STRANGERS_PER_ADDR - 1,
            "none went: room for one more"
        );
        inner
            .conns
            .insert(STRANGERS_PER_ADDR as u64, stranger("192.0.2.7"));
        assert!(inner.make_room(ip("192.0.2.7")));
        assert!(
            !inner.conns.contains_key(&1),
            "only the cap's oldest makes room"
        );
        assert_eq!(inner.conns.len(), STRANGERS_PER_ADDR - 1);
        inner.conns.clear();
        // A refused address holding a token is not touched by its strangers.
        let mut holder = stranger("192.0.2.7");
        holder.token = Some((1, "phone".into()));
        inner.conns.insert(2, holder);
        assert!(inner.make_room(ip("192.0.2.7")));
        assert!(inner.conns.contains_key(&2));
        // The shared entry refusing doesn't lock out the addresses it holds.
        let mut full = Inner::default();
        for n in 0..LIMITER_MOST {
            full.limiter
                .failed(&format!("172.16.{}.{}", n / 256, n % 256), t);
        }
        for _ in 0..FAILS {
            full.limiter.failed("203.0.113.1", t);
        }
        assert!(full.limiter.refused("203.0.113.99", t).is_some());
        assert!(full.make_room(ip("203.0.113.99")));
        for n in 0..3 {
            full.conns.insert(n, stranger("203.0.113.99"));
        }
        assert!(full.make_room(ip("203.0.113.99")));
        assert_eq!(full.conns.len(), 3);
    }

    #[test]
    fn the_limiter_keeps_a_bounded_table_and_sweeps_it_once_a_second() {
        let (mut limiter, t) = (Limiter::default(), Instant::now());
        for n in 0..LIMITER_MOST + 500 {
            limiter.failed(
                &format!("10.{}.{}.{}", n / 65_536, (n / 256) % 256, n % 256),
                t,
            );
        }
        assert!(limiter.by.len() <= LIMITER_MOST + 1, "{}", limiter.by.len());
        assert!(
            limiter.by.contains_key(OVERFLOW),
            "the rest share one entry"
        );
        // Every address has one failure, and they are all recent: nothing is swept, and the table
        // doesn't grow either.
        let before = limiter.by.len();
        limiter.failed("10.200.0.1", t);
        assert_eq!(limiter.by.len(), before);
        // Ten minutes on they are old: one sweep clears them, and the next within the second doesn't
        // run again.
        let later = t + FAIL_WINDOW + Duration::from_secs(2);
        limiter.failed("10.201.0.1", later);
        assert!(limiter.by.len() < 10, "{}", limiter.by.len());
        let swept = limiter.pruned;
        limiter.failed("10.201.0.2", later + Duration::from_millis(10));
        assert_eq!(limiter.pruned, swept, "not again within the second");
        // The shared entry refuses the addresses it holds, and only while the table is full.
        let (mut full, now) = (Limiter::default(), Instant::now());
        for n in 0..LIMITER_MOST {
            full.failed(&format!("172.16.{}.{}", n / 256, n % 256), now);
        }
        for _ in 0..FAILS {
            full.failed("203.0.113.1", now);
        }
        assert!(
            full.refused("203.0.113.99", now).is_some(),
            "the shared entry refuses failures from addresses it holds"
        );
        assert!(
            full.refused("172.16.0.0", now).is_none(),
            "a tracked address has its own"
        );
    }

    #[test]
    fn an_ipv6_slash_64_is_one_address() {
        let (mut limiter, t) = (Limiter::default(), Instant::now());
        for n in 0..FAILS {
            limiter.failed(&format!("2001:db8:1:2::{n:x}"), t);
        }
        assert!(
            limiter
                .refused("2001:db8:1:2:aaaa:bbbb:cccc:dddd", t)
                .is_some()
        );
        assert!(
            limiter.refused("2001:db8:1:3::1", t).is_none(),
            "another /64"
        );
        limiter.passed("2001:db8:1:2::77");
        assert!(
            limiter.refused("2001:db8:1:2::1", t).is_none(),
            "a pass clears the /64"
        );
    }

    #[test]
    fn the_limiter_doubles_up_to_fifteen_minutes_and_a_pass_clears_it() {
        let (mut limiter, t) = (Limiter::default(), Instant::now());
        for _ in 0..FAILS - 1 {
            limiter.failed("a", t);
        }
        assert_eq!(limiter.refused("a", t), None, "nine failures");
        limiter.failed("a", t);
        assert_eq!(limiter.refused("a", t), Some(Duration::from_secs(2)));
        assert_eq!(limiter.refused("a", t + Duration::from_secs(2)), None);
        let mut at = t + Duration::from_secs(2);
        for want in [4, 8, 16, 32, 64, 128, 256, 512, 900, 900] {
            limiter.failed("a", at);
            let wait = Duration::from_secs(want);
            assert_eq!(limiter.refused("a", at), Some(wait));
            at += wait;
            assert_eq!(limiter.refused("a", at), None);
        }
        assert_eq!(
            limiter.refused("b", t),
            None,
            "another address is unaffected"
        );
        limiter.passed("a");
        for _ in 0..FAILS - 1 {
            limiter.failed("a", at);
        }
        assert_eq!(limiter.refused("a", at), None, "a pass cleared the count");
    }

    #[test]
    fn old_failures_dont_count_and_the_penalty_is_forgotten() {
        let (mut limiter, t) = (Limiter::default(), Instant::now());
        for _ in 0..FAILS - 1 {
            limiter.failed("a", t);
        }
        let later = t + FAIL_WINDOW + Duration::from_secs(1);
        limiter.failed("a", later);
        assert_eq!(
            limiter.refused("a", later),
            None,
            "the nine are older than ten minutes"
        );
        for _ in 0..FAILS - 1 {
            limiter.failed("a", later);
        }
        assert_eq!(
            limiter.refused("a", later),
            Some(PENALTY_FIRST),
            "and the penalty starts over"
        );
    }

    #[test]
    fn the_bucket_gives_forty_then_twenty_a_second() {
        let (mut bucket, t) = (Bucket::new(Instant::now()), Instant::now());
        let t = t + Duration::from_secs(1);
        for _ in 0..40 {
            assert_eq!(bucket.take(t), None);
        }
        let wait = bucket.take(t).unwrap();
        assert!(
            wait > Duration::ZERO && wait <= Duration::from_millis(51),
            "{wait:?}"
        );
        let later = t + Duration::from_secs(1);
        for _ in 0..20 {
            assert_eq!(bucket.take(later), None);
        }
        assert!(bucket.take(later).is_some());
        // Idle time never gives more than the burst.
        let much_later = later + Duration::from_secs(3600);
        for _ in 0..40 {
            assert_eq!(bucket.take(much_later), None);
        }
        assert!(bucket.take(much_later).is_some());
    }

    #[test]
    fn an_origin_matches_whole_or_not_at_all() {
        let listed = vec!["https://dash.example".to_string()];
        assert!(allowed_origin("https://dash.example", &listed));
        for other in [
            "https://dash.example.evil",
            "http://dash.example",
            "https://dash.example:8443",
            "null",
            "",
            "https://dash.example/",
        ] {
            assert!(!allowed_origin(other, &listed), "{other}");
        }
        assert!(!allowed_origin("https://dash.example", &[]));
    }

    #[test]
    fn a_tools_reply_line_becomes_its_answer() {
        let status = |line: Value| tool_answer(&line).status().as_u16();
        assert_eq!(
            status(json!({"result": {"content": [{"type": "text", "text": "ok"}]}})),
            200
        );
        assert_eq!(
            status(
                json!({"result": {"isError": true, "content": [{"type": "text", "text": "no"}]}})
            ),
            422
        );
        assert_eq!(
            status(json!({"error": {"code": codes::SCOPE, "message": "m"}})),
            403
        );
        assert_eq!(
            status(json!({"error": {"code": codes::AUTH, "message": "m"}})),
            401
        );
        assert_eq!(
            status(json!({"error": {"code": codes::NOT_FOUND, "message": "m"}})),
            404
        );
        assert_eq!(
            status(json!({"error": {"code": rpc::PARAMS, "message": "m"}})),
            400
        );
        assert_eq!(status(json!({"error": {"code": -1, "message": "m"}})), 500);
        let reply = tools::tool_result(&json!(1), &templum_mcp::Reply::ok("hi"));
        assert_eq!(status(serde_json::from_str(&reply).unwrap()), 200);
    }

    /// A response's JSON body, read to the end.
    fn json_of(response: Response<Body>) -> Value {
        let bytes = futures::executor::block_on(response.into_body().collect())
            .unwrap()
            .to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn a_git_answer_carries_its_result_and_a_refusal_its_details() {
        let ok = tool_answer(&json!({"result": {
            "content": [{"type": "text", "text": "Committed a1b2c3d."}],
            "git": {"short": "a1b2c3d", "replaced": null},
        }}));
        assert_eq!(ok.status().as_u16(), 200);
        assert_eq!(
            json_of(ok),
            json!({"text": "Committed a1b2c3d.", "git": {"short": "a1b2c3d", "replaced": null}})
        );
        // A result that is nothing is still there, as null.
        let nothing = tool_answer(&json!({"result": {
            "content": [{"type": "text", "text": "Fetched."}],
            "git": null,
        }}));
        assert_eq!(json_of(nothing), json!({"text": "Fetched.", "git": null}));
        let refused = tool_answer(&json!({"result": {
            "isError": true,
            "content": [{"type": "text", "text": "fatal: x"}],
            "details": "fatal: x\nhint: y",
        }}));
        assert_eq!(refused.status().as_u16(), 422);
        assert_eq!(
            json_of(refused),
            json!({"error": {"code": "refused", "message": "fatal: x", "details": "fatal: x\nhint: y"}})
        );
        // Any other tool's refusal has no details.
        let plain = tool_answer(&json!({"result": {
            "isError": true,
            "content": [{"type": "text", "text": "no"}],
        }}));
        assert_eq!(
            json_of(plain),
            json!({"error": {"code": "refused", "message": "no"}})
        );
    }

    #[test]
    fn a_git_call_is_audited_by_its_operation() {
        let marked = audited_as(error(403, "scope", "m"), "git", &json!({"op": "push"}));
        assert_eq!(audited(&marked, "git".into()), "git:push");
        // A call without a known operation, and any other tool, keep the route's word.
        let unmarked = audited_as(error(400, "invalid", "m"), "git", &json!({}));
        assert_eq!(audited(&unmarked, "git".into()), "git");
        let other = audited_as(
            error(403, "scope", "m"),
            "send_keys",
            &json!({"op": "push"}),
        );
        assert_eq!(audited(&other, "send_keys".into()), "send_keys");
    }
}
