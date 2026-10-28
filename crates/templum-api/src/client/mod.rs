//! The mirror's client (docs/specs/templum-to-templum.md, The client): one thread follows every
//! remote workspace of another computer, UI-free. Plan 1c wires it to the canvas.

mod dial;
mod frames;
mod http;
mod keys;
mod remote;
mod sse;
mod trust;

use crate::code::Trust;
use crate::model::{ApplyError, MeView, Notice, Rect, State};
use remote::Msg;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use templum_terminal::{Feeder, MouseInput};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

pub use dial::{proxy_from_env, proxy_of};
pub use keys::{Kept, Keys, SERVICE, SERVICE_DEV, Secrets, entry_name};

/// What is wrong with a public certificate, when it is simply not valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bad {
    Expired,
    /// Made for another name.
    Name,
    /// No authority this computer trusts signed it.
    Issuer,
    /// The same, and the server sent its certificate alone: maybe it is Templum's own.
    Alone,
    Other,
}

/// Why a remote stopped and won't try again by itself (the spec's table, rows 4 to 15).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    Expired,
    Revoked,
    Deleted,
    Replaced,
    PinChanged,
    Certificate(Bad),
    AddedAuthority,
    /// Cloudflare Access stands in front.
    Access,
    NotTemplum {
        status: u16,
        kind: String,
    },
    TooOld,
    Proxy {
        proxy: String,
        status: u16,
    },
    Keyring,
}

/// How a call or a connection went wrong, and what that asks of the remote.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Failure {
    /// Nobody answered right: try again with backoff.
    Down(String),
    /// Try again after this long.
    Busy(Duration),
    /// A `401`: the remote decides whether the share expired or was revoked.
    Auth,
    /// Templum's own `404`, with its words.
    Missing(String),
    /// Templum's own `403` or `422`, with its words and, when it has them, its details.
    Refused(String, Option<String>),
    /// Stop for good.
    Stop(Why),
    /// More than the client keeps in memory.
    TooLarge,
}

/// The updates a `Mirror` keeps for the UI thread: more than this and the oldest are dropped.
const UPDATES_MOST: usize = 1024;

/// One connection to a home: what the person pasted and chose.
#[derive(Clone, Debug)]
pub struct Remote {
    /// The code's address, without a trailing `/`.
    pub url: String,
    pub trust: Trust,
    /// The person accepted a certificate signed by an authority this computer added.
    pub trust_added: bool,
    /// A Cloudflare Access service token's Client ID (its secret is in the secrets).
    pub access_id: Option<String>,
    /// `http://host:port`, without credentials (they are in the secrets).
    pub proxy: Option<String>,
    pub key: KeySource,
}

impl Remote {
    /// Whether `secrets` were kept for this very address, way in and proxy. The index these come
    /// from can be rewritten by anything that writes the data folder; the secrets can't be.
    pub(crate) fn binds(&self, secrets: &Secrets) -> bool {
        secrets.url == self.url
            && secrets.trust == self.trust
            && secrets.trust_added == self.trust_added
            && secrets.proxy == self.proxy
    }
}

/// Where a remote's secrets come from.
#[derive(Clone, Debug)]
pub enum KeySource {
    /// Load them: `Keys::load(entry, file)`.
    Kept { entry: String, file: bool },
    /// Save them first (then `Update::Kept` says where), then use them. `from`: a connection
    /// this one replaces, whose secrets the new ones lack are taken from it here, so they never
    /// pass through the UI.
    Save {
        entry: String,
        secrets: Secrets,
        file: bool,
        from: Option<Inherit>,
    },
    /// Use them as given: kept nowhere.
    Memory(Secrets),
}

/// What a new code for a connection keeps of the secrets the old one was kept with.
#[derive(Clone, Debug)]
pub struct Inherit {
    /// Where the old secrets were kept: `Keys::load(entry, file)`.
    pub entry: String,
    pub file: bool,
    /// The Cloudflare Access Client Secret.
    pub access: bool,
    /// The proxy's login.
    pub login: bool,
}

/// How a remote is doing.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Connecting,
    Live,
    /// `attempt` tries have failed; the next is in `next`. `live_at`: the last time it was live.
    Reconnecting {
        attempt: u32,
        next: Duration,
        live_at: Option<SystemTime>,
    },
    /// Stopped for good, until the person tries again.
    Cut(Why),
    Disconnected,
}

impl Status {
    /// The word the mirror's API state says.
    pub fn word(&self) -> &'static str {
        match self {
            Status::Connecting => "connecting",
            Status::Live => "live",
            Status::Reconnecting { .. } => "reconnecting",
            Status::Cut(Why::Expired | Why::Revoked | Why::Deleted | Why::Replaced) => "cut",
            Status::Cut(_) => "refused",
            Status::Disconnected => "disconnected",
        }
    }
}

/// What a refusal was about.
#[derive(Clone, Debug, PartialEq)]
pub enum About {
    /// Typing into this block.
    Keys(String),
    Move(String),
    /// The secrets that couldn't be kept.
    Key,
}

/// A tool of home's, called from the mirror (docs/specs/templum-to-templum.md, The mirror's side).
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub tool: String,
    pub args: Value,
    /// On the slow connection: a call that may take minutes, which the quick ones don't wait for.
    pub slow: bool,
}

/// Home's words for why it did not do a call, or the mirror's for why it could not ask.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Refusal {
    pub sentence: String,
    /// What home says besides (git's whole stderr for a failed `git` operation).
    pub details: Option<String>,
    /// Home may have done the call anyway: it was sent and no answer came (the connection dropped,
    /// or it timed out). False for home's own refusals and for a call that never left.
    pub maybe_done: bool,
}

/// The most of a refusal's details the client keeps: home's own cap, again where it enters.
pub const DETAILS_MOST: usize = 65_536;

impl Refusal {
    pub fn of(sentence: impl Into<String>) -> Refusal {
        Refusal {
            sentence: sentence.into(),
            details: None,
            maybe_done: false,
        }
    }

    /// `of`, for a call that was sent and not answered.
    pub fn maybe_done(sentence: impl Into<String>) -> Refusal {
        Refusal {
            maybe_done: true,
            ..Refusal::of(sentence)
        }
    }
}

/// What a call came to: home's JSON (`{"text": …}` and what it touched), or a refusal.
pub type Answered = Result<Value, Refusal>;

/// What a remote tells the UI thread, besides its newest state.
#[derive(Clone, Debug)]
pub enum Update {
    Status(Status),
    /// Who the share is, from `GET /v1/me`.
    Me(MeView),
    /// The last round trip.
    Latency(Duration),
    /// Where the secrets were put.
    Kept(Kept),
    /// Home refused something, in its words.
    Refused {
        about: About,
        sentence: String,
    },
    /// Keys typed faster than home answers were dropped: the block.
    Dropped(String),
    /// Whether the system's keyring answers (for the empty id: the answer to `probe`).
    Keyring(bool),
    /// The tools home has, read at each connect (`GET /v1/tools`).
    Tools(Tools),
    /// The viewer of this block holds home's history now (above its live screen).
    History(String),
    /// Something happened at home that isn't state: a task ended, an agent needs its person, a
    /// watch matched. Cleaned and short.
    Notice(Notice),
}

/// The most tools of home's the mirror keeps.
const TOOLS_MOST: usize = 500;
/// The longest name of a tool or of an argument the mirror keeps, in bytes.
const NAME_MOST: usize = 64;
/// The most characters of a notice's text the mirror keeps.
pub const NOTICE_CHARS: usize = 300;

/// The tools home has, by name, each with the names of its arguments: what the mirror asks before
/// it offers something home's Templum may not do (docs/specs/templum-to-templum.md, One rule).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tools {
    by_name: HashMap<String, std::collections::HashSet<String>>,
}

impl Tools {
    /// Reads `GET /v1/tools`' array: each item's `name` and the keys of its
    /// `inputSchema.properties`. Home is not trusted: at most 500 tools, a name that is long or
    /// has a control character is left out, and anything that is not an array is none.
    pub fn read(list: &Value) -> Tools {
        let fit = |name: &str| name.len() <= NAME_MOST && !name.chars().any(char::is_control);
        let by_name = list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str().filter(|n| fit(n))?;
                let arguments = tool
                    .pointer("/inputSchema/properties")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(|properties| properties.keys())
                    .filter(|key| fit(key))
                    .cloned()
                    .collect();
                Some((name.to_string(), arguments))
            })
            .take(TOOLS_MOST)
            .collect();
        Tools { by_name }
    }

    pub fn has(&self, tool: &str) -> bool {
        self.by_name.contains_key(tool)
    }

    /// Whether `tool` takes `argument`.
    pub fn takes(&self, tool: &str, argument: &str) -> bool {
        self.by_name
            .get(tool)
            .is_some_and(|arguments| arguments.contains(argument))
    }
}

/// A `notice` event as the UI may take it: the first line of its text, at most [`NOTICE_CHARS`]
/// characters, and a block and workspace only when short and plain. None for one that is not.
pub(crate) fn notice_of(data: &str) -> Option<Notice> {
    let notice: Notice = serde_json::from_str(data).ok()?;
    let plain = |id: Option<String>| {
        id.filter(|id| id.len() <= NAME_MOST && !id.chars().any(char::is_control))
    };
    Some(Notice {
        text: notice
            .text
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(NOTICE_CHARS)
            .collect(),
        block: plain(notice.block),
        workspace: plain(notice.workspace),
        automation: notice.automation,
    })
}

/// What the UI thread asks of a remote.
pub enum Command {
    Disconnect,
    /// Retry at once, or start again a remote that was cut or disconnected.
    TryNow,
    /// The terminals to stream, nearest the center first: the first 11 are.
    Terminals(Vec<(String, Feeder)>),
    /// Named keys typed into a block.
    Keys {
        block: String,
        keys: Vec<String>,
    },
    Paste {
        block: String,
        text: String,
    },
    /// A mouse event for the program in a block, typed like keys: in order, merged while waiting
    /// when it is a move.
    Mouse {
        block: String,
        event: MouseInput,
    },
    /// Scrolling back in a block: read home's history of that streamed terminal into its viewer,
    /// unless the viewer holds it already and no frame was fed since. `Update::History` says it
    /// is there.
    History {
        block: String,
    },
    Move {
        block: String,
        rect: Rect,
    },
    /// Calls a tool of home's: `Mirror::call` sends it and awaits the answer.
    Call {
        call: Call,
        answer: tokio::sync::oneshot::Sender<Answered>,
    },
    /// The person accepted a certificate signed by an authority this computer added
    /// (`Why::AddedAuthority`): the key is kept that way from now on, and the remote starts again.
    /// A click, never the index, can say so: the index could be rewritten by anything that writes
    /// the data folder.
    TrustAdded,
}

/// The limits that time a remote; `Default` is the spec's. The tests shorten them.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// A stream that sends nothing this long is dead.
    pub silence: Duration,
    /// With no call this long, `GET /v1/me`.
    pub me_every: Duration,
    /// A connection live this long makes the next failure the first again.
    pub live_resets: Duration,
    /// Typing waits this long in all over the "busy" answers of one call, then its keys are
    /// dropped: keys typed a while ago are never sent late, into whatever the terminal shows by
    /// then.
    pub typing_patience: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            silence: Duration::from_secs(45),
            me_every: Duration::from_secs(15),
            live_resets: Duration::from_secs(60),
            typing_patience: Duration::from_secs(5),
        }
    }
}

/// The most event bytes the items of one remote's state may weigh: a few of the largest event the
/// stream carries (`sse::CAP`). A home that sends past it is "too large", as for an event.
const WEIGHT_MOST: usize = 4 * sse::CAP;

/// What each item a remote's state keeps costs besides its data (its keys, its place in the
/// collection and in the weights), so a hostile home can't fill memory with a great many tiny ones.
const ITEM_OVERHEAD: usize = 64;

/// The size, in bytes of event data and [`ITEM_OVERHEAD`], of each item a remote's state keeps, so
/// a hostile home can't fill memory with a few thousand big items or a million tiny ones.
#[derive(Default)]
struct Weight {
    items: HashMap<(String, String), usize>,
    total: usize,
}

impl Weight {
    /// Counts an event of `size` bytes: a snapshot starts over, a put weighs the item it puts in
    /// place of the old one, a delete frees it; the events of a kind the state doesn't keep weigh
    /// nothing. False, and nothing counted, when the total would pass [`WEIGHT_MOST`].
    fn add(&mut self, event: &str, data: &Value, size: usize) -> bool {
        if event == "snapshot" {
            self.items.clear();
            self.total = size;
            return size <= WEIGHT_MOST;
        }
        if !State::keeps_items(event) {
            return true;
        }
        let size = size + ITEM_OVERHEAD;
        let Some(id) = data.get("id").and_then(Value::as_str) else {
            return true;
        };
        let key = (event.to_string(), id.to_string());
        match data.get("op").and_then(Value::as_str) {
            Some("put") => {
                let old = self.items.get(&key).copied().unwrap_or(0);
                if self.total - old + size > WEIGHT_MOST {
                    return false;
                }
                self.total = self.total - old + size;
                self.items.insert(key, size);
            }
            Some("delete") => self.total -= self.items.remove(&key).unwrap_or(0),
            _ => {}
        }
        true
    }
}

/// What the UI thread and the client's thread share: the updates and the newest states.
pub(crate) struct Shared {
    inner: Mutex<Inner>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct Inner {
    updates: VecDeque<(String, Update)>,
    /// The newest state of each remote, and whether the UI thread has yet to take it.
    states: HashMap<String, (State, bool)>,
    /// What each remote's state weighs, in event bytes.
    weights: HashMap<String, Weight>,
    /// The generation each remote's task must carry: a replaced or forgotten one is stale, and
    /// what it still says is dropped.
    current: HashMap<String, u64>,
}

impl Shared {
    #[cfg(test)]
    pub(crate) fn for_tests() -> Shared {
        Shared {
            inner: Mutex::default(),
            wake: Arc::new(|| {}),
        }
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// An update from the task of generation `round` of remote `id`.
    pub(crate) fn push(&self, id: &str, round: u64, update: Update) {
        {
            let mut inner = self.inner();
            if id.is_empty() || inner.current.get(id) == Some(&round) {
                if inner.updates.len() >= UPDATES_MOST {
                    inner.updates.pop_front();
                }
                inner.updates.push_back((id.into(), update));
            } else {
                return;
            }
        }
        (self.wake)();
    }

    /// Applies an event (`size` bytes of data) to the remote's state in place (nothing is copied
    /// per event). Whether the state changed. The event is parsed before the lock is taken: the
    /// UI thread waits on it.
    pub(crate) fn apply(
        &self,
        id: &str,
        round: u64,
        event: &str,
        data: &Value,
        size: usize,
    ) -> Result<bool, ApplyError> {
        let change = State::parse(event, data)?;
        let changed = {
            let mut inner = self.inner();
            if inner.current.get(id) != Some(&round) {
                return Ok(false);
            }
            if !inner
                .weights
                .entry(id.into())
                .or_default()
                .add(event, data, size)
            {
                return Err(ApplyError::TooLarge);
            }
            let (state, fresh) = inner.states.entry(id.into()).or_default();
            let changed = change(state)?;
            *fresh |= changed;
            changed
        };
        if changed {
            (self.wake)();
        }
        Ok(changed)
    }

    /// The newest state of the remote `id` if it is newer than the one last taken. A clone copies
    /// pointers, not items: every item that didn't change is the very `Arc` of the last take.
    pub(crate) fn take(&self, id: &str) -> Option<State> {
        let mut inner = self.inner();
        let (state, fresh) = inner.states.get_mut(id)?;
        std::mem::take(fresh).then(|| state.clone())
    }
}

/// One thread, `templum-mirror`, with a single-threaded runtime, follows every remote. The UI
/// thread never waits on it: its methods push orders to a channel or copy what a lock holds.
/// Dropping the `Mirror` ends the thread and every connection.
pub struct Mirror {
    orders: UnboundedSender<Order>,
    shared: Arc<Shared>,
    next: Mutex<u64>,
}

enum Order {
    Connect {
        id: String,
        round: u64,
        remote: Box<Remote>,
    },
    Send(String, Msg),
    Forget(String, bool),
    DropKey(String, bool),
    Probe,
}

impl Mirror {
    pub fn start(keys: Keys, wake: Arc<dyn Fn() + Send + Sync>) -> Mirror {
        Mirror::start_with(keys, wake, Limits::default())
    }

    #[doc(hidden)]
    pub fn start_with(keys: Keys, wake: Arc<dyn Fn() + Send + Sync>, limits: Limits) -> Mirror {
        let shared = Arc::new(Shared {
            inner: Mutex::default(),
            wake,
        });
        let (orders, inbox) = unbounded_channel();
        let ours = shared.clone();
        // A thread that can't start leaves a Mirror whose orders go nowhere: remotes stay as
        // they are, and nothing else breaks.
        let _ = std::thread::Builder::new()
            .name("templum-mirror".into())
            .spawn(move || remote::run(keys, ours, limits, inbox));
        Mirror {
            orders,
            shared,
            next: Mutex::new(0),
        }
    }

    /// Connects `remote` as `id`, replacing the one with that id.
    pub fn connect(&self, id: &str, remote: Remote) {
        let round = {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            *next += 1;
            *next
        };
        {
            let mut inner = self.shared.inner();
            inner.current.insert(id.into(), round);
            inner.updates.retain(|(of, _)| of != id);
            inner.states.remove(id);
            inner.weights.remove(id);
        }
        let _ = self.orders.send(Order::Connect {
            id: id.into(),
            round,
            remote: Box::new(remote),
        });
    }

    pub fn send(&self, id: &str, command: Command) {
        let _ = self
            .orders
            .send(Order::Send(id.into(), Msg::Command(command)));
    }

    /// Calls `call` at home. The order is sent at once; the future is the answer, which the UI
    /// thread awaits in a task (never on the thread itself). A remote that is unknown, forgotten
    /// or gone answers "Couldn't reach the other computer.".
    pub fn call(&self, id: &str, call: Call) -> impl Future<Output = Answered> + Send + 'static {
        let (answer, answered) = tokio::sync::oneshot::channel();
        self.send(id, Command::Call { call, answer });
        async move {
            answered
                .await
                .unwrap_or_else(|_| Err(Refusal::of("Couldn't reach the other computer.")))
        }
    }

    /// Ends the remote `id`: when `revoke` and it is live, its share is revoked at home first;
    /// then its key is deleted and its connections dropped. No update follows.
    pub fn forget(&self, id: &str, revoke: bool) {
        {
            let mut inner = self.shared.inner();
            inner.current.remove(id);
            inner.updates.retain(|(of, _)| of != id);
            inner.states.remove(id);
            inner.weights.remove(id);
        }
        let _ = self.orders.send(Order::Forget(id.into(), revoke));
    }

    /// Deletes the key kept under `entry` (`file`: in a file), and nothing else: a connection that
    /// a new code replaced no longer needs its old key.
    pub fn drop_key(&self, entry: &str, file: bool) {
        let _ = self.orders.send(Order::DropKey(entry.into(), file));
    }

    /// Asks whether the system's keyring answers: the answer comes as `("", Update::Keyring)`.
    pub fn probe(&self) {
        let _ = self.orders.send(Order::Probe);
    }

    /// What the remotes said since the last call, oldest first.
    pub fn updates(&self) -> Vec<(String, Update)> {
        self.shared.inner().updates.drain(..).collect()
    }

    /// The newest state of the remote `id` if it is newer than the one last taken.
    pub fn state(&self, id: &str) -> Option<State> {
        self.shared.take(id)
    }
}

/// Runs a future to its end on a runtime like the client's, for the tests.
#[cfg(test)]
pub(crate) fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{self, BlockView, CardView};
    use serde_json::json;

    #[test]
    fn home_s_tools_are_capped_and_read_by_name_and_argument() {
        let mut list: Vec<Value> = (0..600)
            .map(|i| {
                json!({ "name": format!("t{i}"), "inputSchema": { "properties": { "a": {}, "x".repeat(65): {} } } })
            })
            .collect();
        list.insert(0, json!({ "name": "y".repeat(65) }));
        let tools = Tools::read(&Value::Array(list));
        assert!(
            tools.has("t0")
                && tools.has("t498")
                && !tools.has("t500")
                && !tools.has(&"y".repeat(65))
        );
        assert!(
            tools.takes("t0", "a")
                && !tools.takes("t0", "b")
                && !tools.takes("t0", &"x".repeat(65))
        );
        assert_eq!(Tools::read(&json!({"tools": []})), Tools::default());
    }

    #[test]
    fn a_notice_is_its_first_line_and_a_short_plain_place() {
        let data = json!({
            "text": format!("{}\nsecond", "é".repeat(400)),
            "block": "b7k2m",
            "workspace": "w\u{1b}3",
        })
        .to_string();
        let notice = notice_of(&data).unwrap();
        assert_eq!(notice.text, "é".repeat(300));
        assert_eq!(notice.block.as_deref(), Some("b7k2m"));
        assert_eq!(notice.workspace, None);
        assert!(notice_of("[]").is_none());
    }

    #[test]
    fn a_notice_keeps_its_automation() {
        let notice = notice_of(r#"{"text":"a\nb","workspace":"w3","automation":4}"#).unwrap();
        assert_eq!(
            notice,
            Notice {
                text: "a".into(),
                workspace: Some("w3".into()),
                automation: Some(4),
                block: None
            }
        );
    }

    #[test]
    fn a_take_shares_every_item_that_did_not_change() {
        let shared = Shared::for_tests();
        shared.inner().current.insert("r1".into(), 1);
        let block = |id: &str, title: &str| {
            Arc::new(BlockView {
                id: id.into(),
                workspace: "w3".into(),
                kind: "terminal".into(),
                title: title.into(),
                ..Default::default()
            })
        };
        let snapshot = serde_json::to_value(State {
            blocks: vec![block("b1", "one"), block("b2", "two")],
            cards: vec![Arc::new(CardView {
                id: 40,
                board: 3,
                title: "Ship".into(),
                ..Default::default()
            })],
            ..Default::default()
        })
        .unwrap();
        shared.apply("r1", 1, "snapshot", &snapshot, 100).unwrap();
        let first = shared.take("r1").unwrap();
        let put = |b: Arc<BlockView>| json!({ "op": "put", "id": b.id.clone(), "value": &*b });
        shared
            .apply("r1", 1, "block", &put(block("b1", "new")), 50)
            .unwrap();
        // The same item again.
        shared
            .apply("r1", 1, "block", &put(block("b2", "two")), 50)
            .unwrap();
        let second = shared.take("r1").unwrap();
        assert_eq!(second.blocks[0].title, "new");
        assert!(!Arc::ptr_eq(&first.blocks[0], &second.blocks[0]));
        assert!(
            Arc::ptr_eq(&first.blocks[1], &second.blocks[1]),
            "an equal put keeps the item"
        );
        assert!(Arc::ptr_eq(&first.cards[0], &second.cards[0]));
        assert!(
            model::same(&first.cards, &second.cards) && !model::same(&first.blocks, &second.blocks)
        );
        assert!(shared.take("r1").is_none(), "nothing newer");
    }

    #[test]
    fn what_a_state_weighs_is_capped_in_bytes_and_given_back_when_items_go() {
        let shared = Shared::for_tests();
        shared.inner().current.insert("r".into(), 1);
        let put = |id: &str, size: usize| {
            let block = crate::model::BlockView {
                id: id.into(),
                ..Default::default()
            };
            let item = json!({"op": "put", "id": id, "value": block});
            shared.apply("r", 1, "block", &item, size)
        };
        let delete = |id: &str| {
            let item = json!({"op": "delete", "id": id});
            shared.apply("r", 1, "block", &item, 20)
        };
        let big = WEIGHT_MOST / 5 * 3;
        assert_eq!(put("a", big), Ok(true));
        assert_eq!(
            put("b", big),
            Err(ApplyError::TooLarge),
            "a second one is past it"
        );
        // A put in place of an item weighs the new one, not both.
        assert_eq!(
            put("a", big - 1),
            Ok(false),
            "the same item: no change, new weight"
        );
        assert_eq!(put("a", 100), Ok(false));
        assert_eq!(put("b", big), Ok(true));
        // A delete gives the weight back.
        assert_eq!(put("c", big), Err(ApplyError::TooLarge));
        assert_eq!(delete("b"), Ok(true));
        assert_eq!(put("c", big), Ok(true));
        // A snapshot starts over.
        let empty = serde_json::to_value(State::default()).unwrap();
        assert_eq!(shared.apply("r", 1, "snapshot", &empty, 10), Ok(true));
        assert_eq!(put("d", big), Ok(true));
        assert_eq!(put("e", big), Err(ApplyError::TooLarge));
        // A snapshot that is itself too big.
        assert_eq!(
            shared.apply("r", 1, "snapshot", &empty, WEIGHT_MOST + 1),
            Err(ApplyError::TooLarge)
        );
    }

    /// The weights go by the event's id: an item with another id would sit in the state without
    /// being counted, so it is refused. (The loop is bounded: a regression fails the test.)
    #[test]
    fn puts_with_mismatched_ids_cannot_get_past_the_weight_cap() {
        let shared = Shared::for_tests();
        shared.inner().current.insert("r".into(), 1);
        let mut kept = 0;
        for i in 0..200 {
            let block = crate::model::BlockView {
                id: format!("y{i}"),
                ..Default::default()
            };
            let put = json!({"op": "put", "id": "x", "value": block});
            if shared.apply("r", 1, "block", &put, WEIGHT_MOST / 2).is_ok() {
                kept += 1;
            }
        }
        assert_eq!(kept, 0);
        assert!(
            shared
                .inner()
                .states
                .get("r")
                .is_none_or(|(state, _)| state.blocks.is_empty())
        );
    }

    #[test]
    fn items_of_a_kind_the_state_does_not_keep_weigh_nothing() {
        let shared = Shared::for_tests();
        shared.inner().current.insert("r".into(), 1);
        let item = |i: usize| json!({"op": "put", "id": format!("g{i}"), "value": {}});
        for i in 0..100 {
            assert_eq!(
                shared.apply("r", 1, "gadget", &item(i), WEIGHT_MOST / 2),
                Ok(false)
            );
        }
        let inner = shared.inner();
        let weight = &inner.weights["r"];
        assert_eq!((weight.items.len(), weight.total), (0, 0));
    }

    #[test]
    fn every_item_weighs_its_data_and_its_own_overhead() {
        let mut weight = Weight::default();
        let mut item = json!({"op": "put", "id": ""});
        // One byte of data each: without the overhead, a million and more would fit. (The loop
        // is bounded, so a missing overhead fails the test instead of filling memory.)
        let fit = WEIGHT_MOST / 65;
        let kept = (0..=fit)
            .take_while(|i| {
                item["id"] = i.to_string().into();
                weight.add("block", &item, 1)
            })
            .count();
        assert_eq!(kept, fit);
        // A delete gives back the data and the overhead.
        weight.add("block", &json!({"op": "delete", "id": "0"}), 20);
        assert!(weight.add("block", &item, 1));
    }

    /// A `Debug` of anything the client holds or says never has a token in it.
    #[test]
    fn what_the_client_says_never_shows_a_key() {
        let key = crate::code::Key::new("tpl_SECRETSECRETSECRET".into());
        let secrets = Secrets {
            token: key.clone(),
            access_secret: Some("acc-secret".into()),
            proxy_auth: Some("user:proxy-password".into()),
            url: "https://templum.example.com".into(),
            trust: Trust::Pinned("ab".repeat(32)),
            trust_added: true,
            proxy: Some("http://proxy.corp:8080".into()),
        };
        let failures = [
            Failure::Down("HTTP 502".into()),
            Failure::Busy(Duration::from_secs(3)),
            Failure::Auth,
            Failure::Missing("No such block.".into()),
            Failure::Refused("No.".into(), None),
            Failure::Stop(Why::Access),
            Failure::TooLarge,
        ];
        let updates = [
            Update::Status(Status::Cut(Why::Keyring)),
            Update::Me(MeView::default()),
            Update::Latency(Duration::from_millis(3)),
            Update::Kept(Kept::File),
            Update::Refused {
                about: About::Keys("b7k2m".into()),
                sentence: "No.".into(),
            },
            Update::Dropped("b7k2m".into()),
            Update::Keyring(true),
        ];
        let remote = Remote {
            url: secrets.url.clone(),
            trust: secrets.trust.clone(),
            trust_added: true,
            access_id: Some("id".into()),
            proxy: secrets.proxy.clone(),
            key: KeySource::Save {
                entry: "remote-ab12cd34".into(),
                secrets: secrets.clone(),
                file: true,
                from: None,
            },
        };
        let shown = format!(
            "{failures:?} {updates:?} {secrets:?} {remote:?} {:#?}",
            remote.key
        );
        assert!(
            !shown.contains("tpl_S") && !shown.contains("SECRET"),
            "{shown}"
        );
        assert!(
            !shown.contains("proxy-password") && !shown.contains("acc-secret"),
            "{shown}"
        );
        assert!(shown.contains("tpl_…"), "the token shows as tpl_…");
        assert!(remote.binds(&secrets));
        assert!(
            !Remote {
                trust_added: false,
                ..remote.clone()
            }
            .binds(&secrets)
        );
    }
}
