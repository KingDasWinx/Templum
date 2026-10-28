//! What the API's integration tests share: a server on an ephemeral loopback port whose "UI
//! thread" is a fake that records what it is sent and answers like Templum would, and a small
//! HTTP client. Not every test uses all of it.
#![allow(dead_code)]

use futures::StreamExt;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use templum_api::client::{KeySource, Keys, Limits, Mirror, Remote, Secrets, Update};
use templum_api::code::{Key, Trust};
use templum_api::model::*;
use templum_api::store::{Scope, Settings, Share, Store, Token, Way};
use templum_api::{Options, Server, TerminalRef, codes, registry, start};
use templum_mcp::{Incoming, Reply, rpc, tools};
use templum_terminal::{Palette, TermOptions, Terminal};

/// A token with the given scope and workspaces, and its text. Ids are unique in the process.
pub fn token(scope: Scope, workspaces: Option<Vec<&str>>) -> (String, Token) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let mut store = Store::default();
    let workspaces = workspaces.map(|w| w.into_iter().map(String::from).collect());
    let text = store.mint("phone", scope, None, workspaces, 0).unwrap();
    let mut token = store.tokens.remove(0);
    token.id = NEXT.fetch_add(1, Ordering::Relaxed);
    (text, token)
}

/// One of everything, in workspace w3: the block b7k2m (with an agent), a kanban k9x1d showing
/// board 3, the request r12 of b7k2m, card 40, automation 1 and a usage.
pub fn sample() -> State {
    let mut b7k2m = BlockView {
        id: "b7k2m".into(),
        workspace: "w3".into(),
        kind: "terminal".into(),
        title: "Claude Code".into(),
        rect: Rect {
            x: 0.0,
            y: 0.0,
            w: 720.0,
            h: 480.0,
        },
        ..Default::default()
    };
    b7k2m.agent = Some(AgentView {
        agent: "claude".into(),
        state: "needs_you".into(),
        ..Default::default()
    });
    let k9x1d = BlockView {
        id: "k9x1d".into(),
        workspace: "w3".into(),
        kind: "kanban".into(),
        title: "Sprint".into(),
        board: Some(3),
        z: 1,
        ..Default::default()
    };
    State {
        app: App {
            version: "0.1.5".into(),
            api: 1,
            shown: Some("w3".into()),
        },
        workspaces: vec![Arc::new(WorkspaceView {
            id: "w3".into(),
            name: "API".into(),
            shown: true,
            ..Default::default()
        })],
        blocks: vec![Arc::new(b7k2m), Arc::new(k9x1d)],
        wires: vec![Arc::new(WireView {
            a: "b7k2m".into(),
            b: "k9x1d".into(),
            label: None,
        })],
        requests: vec![Arc::new(RequestView {
            id: "r12".into(),
            kind: "question".into(),
            block: Some("b7k2m".into()),
            workspace: Some("w3".into()),
            summary: "Which database?".into(),
            ..Default::default()
        })],
        boards: vec![Arc::new(BoardView {
            id: 3,
            name: "Sprint".into(),
            columns: vec![ColumnView {
                id: 7,
                title: "Doing".into(),
                cards: vec![40],
            }],
        })],
        cards: vec![Arc::new(CardView {
            id: 40,
            board: 3,
            column: 7,
            title: "Login".into(),
            ..Default::default()
        })],
        notes: vec![],
        checklists: vec![],
        drawings: vec![],
        automations: vec![Arc::new(AutomationView {
            id: 1,
            name: "Morning".into(),
            workspace: Some("w3".into()),
            ..Default::default()
        })],
        schedules: vec![],
        shares: ["s1", "s2"]
            .map(|id| {
                Arc::new(ShareView {
                    id: id.into(),
                    workspace: "w3".into(),
                    name: "Another computer".into(),
                    scope: Scope::Full,
                    ..Default::default()
                })
            })
            .into(),
        usage: Some(UsageView::default()),
        view: Some(ViewView {
            workspace: "w3".into(),
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
            zoom: 1.0,
        }),
    }
}

/// What the fake UI thread was sent.
#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub params: Value,
    pub terminal: String,
    pub client: String,
    pub conn: u64,
    pub notification: bool,
}

/// A request the fake UI thread holds unanswered: its id and where its answer goes.
type Parked = (Value, std::sync::mpsc::Sender<String>);

pub struct Fake {
    pub server: Arc<Server>,
    pub base: String,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    /// The terminals the fake UI thread lends when a terminal stream opens.
    pub terminals: Arc<Mutex<Vec<TerminalRef>>>,
    /// While set, the fake UI thread keeps the state requests it gets unanswered (`release_state`).
    pub hold_state: Arc<AtomicBool>,
    /// While set, the fake UI thread keeps the `terminal_input` calls it gets unanswered
    /// (`release_calls`), as a home that is slow to answer does.
    pub hold_calls: Arc<AtomicBool>,
    /// While set, the fake UI thread never answers `revoke_share`, as a home that is slow does.
    pub hold_revoke: Arc<AtomicBool>,
    /// The tool whose `tools/call`s the fake UI thread keeps unanswered (`hold_tool`).
    holding_tool: Arc<Mutex<Option<String>>>,
    parked: Arc<Mutex<Vec<Parked>>>,
    parked_calls: Arc<Mutex<Vec<Parked>>>,
    parked_tool: Arc<Mutex<Vec<Parked>>>,
    pub dir: tempfile::TempDir,
}

impl Fake {
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// What the UI thread was asked: every message that wanted an answer.
    pub fn asked(&self) -> Vec<Seen> {
        self.seen()
            .into_iter()
            .filter(|s| !s.notification)
            .collect()
    }

    /// What was sent to the UI thread that wasn't the state or a notification.
    pub fn calls(&self) -> Vec<Seen> {
        self.seen()
            .into_iter()
            .filter(|s| !s.notification && s.method == "tools/call")
            .collect()
    }

    pub fn notifications(&self, method: &str) -> Vec<Seen> {
        self.seen()
            .into_iter()
            .filter(|s| s.notification && s.method == method)
            .collect()
    }

    pub fn call(&self, request: Req) -> Answer {
        request.send(&self.base)
    }

    /// Answers the `terminal_input` calls held, and stops holding.
    pub fn release_calls(&self) {
        self.hold_calls.store(false, Ordering::SeqCst);
        for (id, reply) in self.parked_calls.lock().unwrap().drain(..) {
            let _ = reply.send(tools::tool_result(&id, &Reply::ok("did terminal_input")));
        }
    }

    /// Answers the oldest `terminal_input` call held and keeps holding the ones after it.
    pub fn release_one_call(&self) {
        let mut parked = self.parked_calls.lock().unwrap();
        if !parked.is_empty() {
            let (id, reply) = parked.remove(0);
            let _ = reply.send(tools::tool_result(&id, &Reply::ok("did terminal_input")));
        }
    }

    /// Parks the `tools/call`s of `name` unanswered, as a home that is slow to answer does.
    pub fn hold_tool(&self, name: &str) {
        *self.holding_tool.lock().unwrap() = Some(name.into());
    }

    /// How many calls of the tool held are parked.
    pub fn held(&self) -> usize {
        self.parked_tool.lock().unwrap().len()
    }

    /// Answers each call of the tool held "did {name}", and stops holding.
    pub fn release_tool(&self) {
        let name = self.holding_tool.lock().unwrap().take().unwrap_or_default();
        for (id, reply) in self.parked_tool.lock().unwrap().drain(..) {
            let _ = reply.send(tools::tool_result(&id, &Reply::ok(format!("did {name}"))));
        }
    }

    /// Answers the state requests held, and stops holding.
    pub fn release_state(&self) {
        self.hold_state.store(false, Ordering::SeqCst);
        for (id, reply) in self.parked.lock().unwrap().drain(..) {
            let _ = reply.send(rpc::result(&id, serde_json::to_value(sample()).unwrap()));
        }
    }
}

/// A server on `127.0.0.1:0`, plain, with a temporary data folder; `tweak` changes its options.
pub fn fake(tokens: Vec<Token>, tweak: impl FnOnce(&mut Options)) -> Fake {
    fake_with(
        Settings {
            port: 0,
            ..Default::default()
        },
        tokens,
        tweak,
    )
}

pub fn fake_with(settings: Settings, tokens: Vec<Token>, tweak: impl FnOnce(&mut Options)) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let mut options = Options::from(&settings, dir.path()).unwrap();
    tweak(&mut options);
    let tls = options.tls.is_some();
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let server = Arc::new(start(options, tokens, tx).unwrap());
    let base = format!(
        "{}://127.0.0.1:{}",
        if tls { "https" } else { "http" },
        server.addr().port()
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let terminals: Arc<Mutex<Vec<TerminalRef>>> = Arc::default();
    let lent = terminals.clone();
    let hold_state = Arc::new(AtomicBool::new(false));
    let holding = hold_state.clone();
    let parked: Arc<Mutex<Vec<Parked>>> = Arc::default();
    let park = parked.clone();
    let hold_calls = Arc::new(AtomicBool::new(false));
    let holding_calls = hold_calls.clone();
    let hold_revoke = Arc::new(AtomicBool::new(false));
    let holding_revoke = hold_revoke.clone();
    let parked_calls: Arc<Mutex<Vec<Parked>>> = Arc::default();
    let park_calls = parked_calls.clone();
    let holding_tool: Arc<Mutex<Option<String>>> = Arc::default();
    let held_tool = holding_tool.clone();
    let parked_tool: Arc<Mutex<Vec<Parked>>> = Arc::default();
    let park_tool = parked_tool.clone();
    // A weak hold, so that dropping the fake still drops the server.
    let lender = Arc::downgrade(&server);
    std::thread::spawn(move || {
        // Calls that never get an answer keep their repliers, as a waiting task does.
        let mut held = Vec::new();
        while let Some(message) = futures::executor::block_on(rx.next()) {
            let (method, params, id) = match &message.incoming {
                Incoming::Request { method, params, id } => {
                    (method.clone(), params.clone(), Some(id.clone()))
                }
                Incoming::Notification { method, params } => (method.clone(), params.clone(), None),
                Incoming::Response => continue,
            };
            record.lock().unwrap().push(Seen {
                method: method.clone(),
                params: params.clone(),
                terminal: message.hello.terminal.clone(),
                client: message.hello.client.clone(),
                conn: message.conn,
                notification: id.is_none(),
            });
            let Some(id) = id else { continue };
            let name = params["name"].as_str().unwrap_or_default();
            let line = match method.as_str() {
                templum_api::STATE if holding.load(Ordering::SeqCst) => {
                    park.lock().unwrap().push((id, message.reply));
                    continue;
                }
                templum_api::STATE => rpc::result(&id, serde_json::to_value(sample()).unwrap()),
                templum_api::TERMINAL => {
                    let block = params["block"].as_str().unwrap_or_default();
                    let all = lent.lock().unwrap().clone();
                    if block == "n0te" {
                        tools::tool_result(&id, &Reply::err("n0te isn't a terminal."))
                    } else if all.iter().any(|t| t.block == block) {
                        if let Some(server) = lender.upgrade() {
                            server.set_terminals(all);
                        }
                        tools::tool_result(&id, &Reply::ok("Streaming."))
                    } else {
                        rpc::error(&id, codes::NOT_FOUND, &registry::no("block", block))
                    }
                }
                "tools/call" if name == "close_block" => {
                    tools::tool_result(&id, &Reply::err("No."))
                }
                // A mirror's moves are refused for the block `locked`.
                "tools/call"
                    if name == "move_block" && params["arguments"]["block"] == "locked" =>
                {
                    tools::tool_result(&id, &Reply::err("No."))
                }
                "tools/call"
                    if name == "terminal_input" && holding_calls.load(Ordering::SeqCst) =>
                {
                    park_calls.lock().unwrap().push((id, message.reply));
                    continue;
                }
                "tools/call" if held_tool.lock().unwrap().as_deref() == Some(name) => {
                    park_tool.lock().unwrap().push((id, message.reply));
                    continue;
                }
                // A lent terminal is read as home's `read_terminal` does: its history and screen.
                "tools/call"
                    if name == "read_terminal"
                        && lent
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|t| params["arguments"]["block"] == t.block.as_str()) =>
                {
                    let lines = params["arguments"]["lines"].as_u64().unwrap_or(200) as usize;
                    let all = lent.lock().unwrap().clone();
                    let term = all
                        .iter()
                        .find(|t| params["arguments"]["block"] == t.block.as_str());
                    let text = term.map(|t| t.term.lines(lines).join("\n"));
                    tools::tool_result(&id, &Reply::ok(text.unwrap_or_default()))
                }
                "tools/call" if name == "delegate" && params["arguments"]["wait"] == true => {
                    held.push(message.reply);
                    continue;
                }
                "tools/call" if name == "revoke_share" && holding_revoke.load(Ordering::SeqCst) => {
                    held.push(message.reply);
                    continue;
                }
                // The UI thread drops the share's token before it answers, as the real one does.
                "tools/call" if name == "revoke_share" => {
                    if let Some(server) = lender.upgrade() {
                        server.set_tokens(Vec::new());
                    }
                    tools::tool_result(&id, &Reply::ok("Revoked s4."))
                }
                "tools/call" if name == "share_workspace" => rpc::result(
                    &id,
                    serde_json::json!({
                        "content": [{ "type": "text", "text": "Shared API as s4." }],
                        "code": "tplc1_x",
                    }),
                ),
                "tools/call" if name == "read_note" => {
                    tools::tool_result(&id, &Reply::ok("did read_note"))
                }
                _ => tools::tool_result(&id, &Reply::ok(format!("did {name}"))),
            };
            let _ = message.reply.send(line);
        }
    });
    Fake {
        server,
        base,
        seen,
        terminals,
        hold_state,
        hold_calls,
        hold_revoke,
        holding_tool,
        parked,
        parked_calls,
        parked_tool,
        dir,
    }
}

/// A share of the workspace `workspace`, and its text.
pub fn share_token(scope: Scope, workspace: &str) -> (String, Token) {
    let (text, mut token) = token(scope, Some(vec![workspace]));
    token.name = "Another computer".into();
    token.share = Some(Share {
        workspace: workspace.into(),
        way: Way::Public,
        address: "https://templum.example.com".into(),
    });
    (text, token)
}

/// A mirror whose wake-ups are counted.
pub fn mirror(keys: Keys) -> (Mirror, Arc<std::sync::atomic::AtomicUsize>) {
    mirror_with(keys, Limits::default())
}

pub fn mirror_with(keys: Keys, limits: Limits) -> (Mirror, Arc<std::sync::atomic::AtomicUsize>) {
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = wakes.clone();
    let wake = Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    (Mirror::start_with(keys, wake, limits), wakes)
}

/// The remote for `fake`, with the share's text kept in memory only.
pub fn remote_for(fake: &Fake, text: &str) -> Remote {
    remote_at(&fake.base, text)
}

/// The remote for plain HTTP to `url` (this computer), the secrets bound to it.
pub fn remote_at(url: &str, text: &str) -> Remote {
    let mut remote = Remote {
        url: url.into(),
        trust: Trust::Loopback,
        trust_added: false,
        access_id: None,
        proxy: None,
        key: KeySource::Memory(Secrets {
            token: Key::new(text.into()),
            access_secret: None,
            proxy_auth: None,
            url: String::new(),
            trust: Trust::Loopback,
            trust_added: false,
            proxy: None,
        }),
    };
    rebind(&mut remote);
    remote
}

/// Makes the secrets of a remote kept in memory say where the remote goes, as the keyring does
/// for one that is kept: call after changing its address, way in or proxy on purpose.
pub fn rebind(remote: &mut Remote) {
    let (url, trust, trust_added, proxy) = (
        remote.url.clone(),
        remote.trust.clone(),
        remote.trust_added,
        remote.proxy.clone(),
    );
    if let KeySource::Memory(secrets) | KeySource::Save { secrets, .. } = &mut remote.key {
        (
            secrets.url,
            secrets.trust,
            secrets.trust_added,
            secrets.proxy,
        ) = (url, trust, trust_added, proxy);
    }
}

/// A TCP relay on a port of its own that sends every connection to another port: home can be
/// cut (`cut`), gone (`point(0)`) and back (`point(port)`) at an address that never changes. It
/// keeps what the clients sent, lowercased, to look at their requests.
pub struct Relay {
    pub port: u16,
    target: Arc<std::sync::atomic::AtomicU16>,
    open: Arc<Mutex<Vec<std::net::TcpStream>>>,
    log: Arc<Mutex<Vec<u8>>>,
}

impl Relay {
    pub fn to(port: u16) -> Relay {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let relay = Relay {
            port: listener.local_addr().unwrap().port(),
            target: Arc::new(std::sync::atomic::AtomicU16::new(port)),
            open: Arc::default(),
            log: Arc::default(),
        };
        let (target, open, log) = (relay.target.clone(), relay.open.clone(), relay.log.clone());
        std::thread::spawn(move || {
            for client in listener.incoming().flatten() {
                let to = target.load(Ordering::SeqCst);
                if to == 0 {
                    continue;
                }
                let Ok(server) = std::net::TcpStream::connect(("127.0.0.1", to)) else {
                    continue;
                };
                {
                    let mut open = open.lock().unwrap();
                    open.extend(client.try_clone().ok());
                    open.extend(server.try_clone().ok());
                }
                let (mut from_client, mut to_server) =
                    (client.try_clone().unwrap(), server.try_clone().unwrap());
                let log = log.clone();
                std::thread::spawn(move || {
                    let mut buffer = [0u8; 4096];
                    while let Ok(n) = std::io::Read::read(&mut from_client, &mut buffer) {
                        if n == 0
                            || std::io::Write::write_all(&mut to_server, &buffer[..n]).is_err()
                        {
                            break;
                        }
                        log.lock().unwrap().extend(buffer[..n].to_ascii_lowercase());
                    }
                    let _ = to_server.shutdown(std::net::Shutdown::Both);
                });
                let (mut from_server, mut to_client) = (server, client);
                std::thread::spawn(move || {
                    let _ = std::io::copy(&mut from_server, &mut to_client);
                    let _ = to_client.shutdown(std::net::Shutdown::Both);
                });
            }
        });
        relay
    }

    /// New connections go to `port`; 0: they are dropped.
    pub fn point(&self, port: u16) {
        self.target.store(port, Ordering::SeqCst);
    }

    /// Ends every connection open now.
    pub fn cut(&self) {
        for stream in self.open.lock().unwrap().drain(..) {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Everything the clients sent so far, lowercased.
    pub fn log(&self) -> String {
        String::from_utf8_lossy(&self.log.lock().unwrap()).into_owned()
    }
}

/// Drains what the remote `id` says until one update matches `pick`; every update drained on the
/// way, the matching one and any that came with it included.
pub fn until(
    mirror: &Mirror,
    id: &str,
    what: &str,
    mut pick: impl FnMut(&Update) -> bool,
) -> Vec<Update> {
    let mut seen = Vec::new();
    wait_for(what, || {
        let mut hit = false;
        for (of, update) in mirror.updates() {
            if of == id {
                hit |= pick(&update);
                seen.push(update);
            }
        }
        hit
    });
    seen
}

/// The calls' log and the noise's, one after the other.
pub fn logs(dir: &std::path::Path) -> String {
    let folder = templum_api::store::folder(dir);
    ["audit.log", "noise.log"]
        .iter()
        .map(|name| std::fs::read_to_string(folder.join(name)).unwrap_or_default())
        .collect()
}

/// A terminal that runs nothing: feed it bytes (`Terminal::feed`) and it shows them.
pub fn viewer(cols: u16, rows: u16) -> Terminal {
    Terminal::viewer(
        "v",
        (cols, rows),
        Palette::default(),
        TermOptions::default(),
        Arc::new(|| {}),
    )
}

/// Lends `term` to the fake's next terminal streams, as the block `block` of `workspace`.
pub fn lend(fake: &Fake, block: &str, workspace: &str, term: &Terminal) {
    fake.terminals.lock().unwrap().push(TerminalRef {
        block: block.into(),
        workspace: workspace.into(),
        term: term.shared().clone(),
    });
}

/// How a stream is opened: with a token, or with a ticket (`?ticket=` after the path).
pub enum Auth<'a> {
    Bearer(&'a str),
    Ticket(&'a str),
}

/// An open stream: its status and headers, and its lines as they arrive.
pub struct Stream {
    pub status: u16,
    pub headers: ureq::http::HeaderMap,
    lines: Receiver<String>,
}

#[derive(Debug)]
pub struct Event {
    pub id: String,
    pub event: String,
    pub data: Value,
}

/// `GET path` (with its query, if any) as a stream.
pub fn open(fake: &Fake, path: &str, auth: Auth, last_event_id: Option<&str>) -> Stream {
    open_with(fake, path, auth, last_event_id, &[])
}

/// `open` with more headers.
pub fn open_with(
    fake: &Fake,
    path: &str,
    auth: Auth,
    last_event_id: Option<&str>,
    headers: &[(&str, &str)],
) -> Stream {
    let request = match auth {
        Auth::Bearer(token) => req("GET", path).token(token),
        Auth::Ticket(ticket) => {
            let joint = if path.contains('?') { '&' } else { '?' };
            req("GET", &format!("{path}{joint}ticket={ticket}"))
        }
    };
    let request = match last_event_id {
        Some(id) => request.header("Last-Event-ID", id),
        None => request,
    };
    let request = headers.iter().fold(request, |request, (name, value)| {
        request.header(name, value)
    });
    let built = request.build(&fake.base);
    let (lines, receiver) = channel();
    let (head, heads) = channel();
    std::thread::spawn(move || {
        // No overall limit: a stream lasts. The test's reads are what time out.
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(None)
            .build();
        let agent: ureq::Agent = config.into();
        let response = agent.run(built).unwrap();
        let _ = head.send((response.status().as_u16(), response.headers().clone()));
        let mut reader = BufReader::new(response.into_body().into_reader());
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if lines.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let (status, headers) = heads
        .recv_timeout(Duration::from_secs(10))
        .expect("response headers");
    Stream {
        status,
        headers,
        lines: receiver,
    }
}

impl Stream {
    fn line(&self) -> Option<String> {
        match self.lines.recv_timeout(Duration::from_secs(10)) {
            Ok(line) => Some(line.trim_end_matches('\n').to_string()),
            Err(RecvTimeoutError::Disconnected) => None,
            Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for a line"),
        }
    }

    /// The next event, heartbeats skipped; None when the stream has ended.
    pub fn next(&self) -> Option<Event> {
        let (mut id, mut event, mut data) = (String::new(), String::new(), Value::Null);
        loop {
            let line = self.line()?;
            if line.is_empty() {
                if !event.is_empty() {
                    return Some(Event { id, event, data });
                }
            } else if line.starts_with(':') {
                // a comment
            } else if let Some(v) = line.strip_prefix("id: ") {
                id = v.into();
            } else if let Some(v) = line.strip_prefix("event: ") {
                event = v.into();
            } else if let Some(v) = line.strip_prefix("data: ") {
                data = serde_json::from_str(v).unwrap();
            }
        }
    }

    pub fn event(&self) -> Event {
        self.next().expect("the stream ended")
    }

    /// The next heartbeat: any line that is a comment, before an event.
    pub fn ping(&self) -> bool {
        loop {
            match self.line() {
                Some(line) if line == ": ping" => return true,
                Some(line) if line.is_empty() => {}
                Some(_) => return false,
                None => return false,
            }
        }
    }

    /// The stream ended (its body is over).
    pub fn ended(&self) -> bool {
        loop {
            match self.lines.recv_timeout(Duration::from_secs(10)) {
                Ok(_) => {}
                Err(RecvTimeoutError::Disconnected) => return true,
                Err(RecvTimeoutError::Timeout) => return false,
            }
        }
    }
}

/// Waits (up to 10 s) until `condition` holds.
pub fn wait_for(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A client that returns statuses as answers, not errors.
pub fn http() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    config.into()
}

/// A client for a server whose certificate it doesn't check (Templum's own is self-signed).
pub fn https_unverified() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .disable_verification(true)
        .build();
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .tls_config(tls)
        .build();
    config.into()
}

pub struct Req {
    method: &'static str,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
    agent: Option<ureq::Agent>,
}

pub fn req(method: &'static str, path: &str) -> Req {
    Req {
        method,
        path: path.into(),
        headers: Vec::new(),
        body: String::new(),
        agent: None,
    }
}

impl Req {
    pub fn token(self, text: &str) -> Req {
        self.header("Authorization", &format!("Bearer {text}"))
    }

    pub fn header(mut self, name: &str, value: &str) -> Req {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn json(self, body: &Value) -> Req {
        self.raw("application/json", &body.to_string())
    }

    pub fn raw(mut self, content_type: &str, body: &str) -> Req {
        self.headers
            .push(("Content-Type".into(), content_type.into()));
        self.body = body.into();
        self
    }

    /// A body that says nothing of what it is: no Content-Type.
    pub fn untyped(mut self, body: &str) -> Req {
        self.body = body.into();
        self
    }

    pub fn agent(mut self, agent: ureq::Agent) -> Req {
        self.agent = Some(agent);
        self
    }

    /// The request, unsent, for a client that wants the body to stream.
    pub fn build(&self, base: &str) -> ureq::http::Request<String> {
        let mut request = ureq::http::Request::builder()
            .method(self.method)
            .uri(format!("{base}{}", self.path));
        for (name, value) in &self.headers {
            request = request.header(name, value);
        }
        request.body(self.body.clone()).unwrap()
    }

    pub fn send(&self, base: &str) -> Answer {
        let agent = self.agent.clone().unwrap_or_else(http);
        let response = agent
            .run(self.build(base))
            .unwrap_or_else(|e| panic!("{} {}: {e}", self.method, self.path));
        Answer::of(response)
    }
}

#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    pub headers: ureq::http::HeaderMap,
    pub text: String,
    pub json: Value,
}

impl Answer {
    pub fn of(mut response: ureq::http::Response<ureq::Body>) -> Answer {
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let text = response.body_mut().read_to_string().unwrap_or_default();
        let json = serde_json::from_str(&text).unwrap_or(Value::Null);
        Answer {
            status,
            headers,
            text,
            json,
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// The `error.code` of an error answer.
    pub fn code(&self) -> &str {
        self.json["error"]["code"].as_str().unwrap_or_default()
    }

    pub fn message(&self) -> &str {
        self.json["error"]["message"].as_str().unwrap_or_default()
    }
}
