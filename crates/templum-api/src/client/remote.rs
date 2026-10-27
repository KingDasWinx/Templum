//! One remote's life (docs/specs/templum-to-templum.md, When it isn't live): a coordinator task
//! per remote that reacts to messages and never awaits the network itself. Its sub-tasks do:
//! the calls connection (`GET /v1/me`, moves, answers), the event stream, the typing connection
//! and one stream per terminal. They are aborted with the attempt that started them, so a
//! reconnect leaves no old connection behind.

use super::dial::{Place, Proxy, is_loopback, place};
use super::frames::Painter;
use super::http::{CALL, Conn, Headers, SLOW_CALL, Task, answer_most, stream};
use super::sse;
use super::trust::{self, Pinned};
use super::{
    About, Answered, Bad, Call, Command, Failure, Kept, KeySource, Keys, Limits, Order, Refusal,
    Remote, Shared, Status, Tools, Update, Why, notice_of,
};
use crate::code::Trust;
use crate::model::{ApplyError, FrameView, MeView};
use crate::registry::{INPUT_KEYS, INPUT_MOUSE, mouse_value};
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use templum_mcp::tools::{MAX_LINES, MAX_TEXT, tail};
use templum_terminal::{Feeder, MouseInput, MouseKind};
use tokio::runtime::Builder;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_rustls::rustls::ClientConfig;

/// At most this many terminals stream at once. With the events, the calls, the slow calls and the
/// typing connections, a remote holds 15 connections at most (home keeps 16 per token).
const STREAMS_MOST: usize = 11;
/// At most this many calls wait for a free connection, per remote; past it a new call is refused.
const CALLS_WAITING_MOST: usize = 64;
/// At most this many keys and mouse events wait, per terminal; past it the new ones are dropped.
const WAITING_MOST: usize = 1024;
/// At most this many events wait for the coordinator: past it the event stream isn't read, so a
/// home that writes faster than the state is kept is slowed by its own socket, not by memory.
const EVENTS_WAITING_MOST: usize = 64;
/// A terminal stream that ended is opened again after this long.
const REOPEN: Duration = Duration::from_secs(1);
/// A call of the typing connection starts no sooner than this after the previous one did, so
/// keys typed fast batch (about 16 calls a second, under what home's bucket refills).
const PACE: Duration = Duration::from_millis(60);
/// A call home answered "busy" this many times in a row is given up.
const BUSY_TRIES: u32 = 10;

/// `min(30 s, 1 s × 2^(attempt−1)) × (1 + jitter)`, `jitter` in `[-0.2, 0.2]`.
pub(crate) fn backoff(attempt: u32, jitter: f64) -> Duration {
    let base = 1u64 << attempt.saturating_sub(1).min(5);
    Duration::from_secs_f64(base.min(30) as f64 * (1.0 + jitter))
}

/// A random number in `[-0.2, 0.2]`.
fn jitter() -> f64 {
    let mut bytes = [0u8; 2];
    let _ = SystemRandom::new().fill(&mut bytes);
    f64::from(u16::from_le_bytes(bytes)) / 65535.0 * 0.4 - 0.2
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Whether the way in fits the address: plain HTTP only to this computer itself, TLS only to
/// anything else. The key never goes in clear off this computer, and no "TLS" nobody checks.
fn fits(trust: &Trust, place: &Place) -> bool {
    match trust {
        Trust::Loopback => !place.https && is_loopback(&place.host),
        Trust::Pinned(_) | Trust::Public => place.https,
    }
}

/// What the coordinator is told: by the UI thread, and by its sub-tasks (each with the attempt
/// it belongs to; those of an ended attempt are ignored).
pub(crate) enum Msg {
    Command(Command),
    Forget {
        revoke: bool,
    },
    Me {
        epoch: u64,
        result: Result<(MeView, Duration), Failure>,
    },
    /// What `GET /v1/tools` answered, once per attempt.
    Tools {
        epoch: u64,
        result: Result<Value, Failure>,
    },
    Opened {
        epoch: u64,
    },
    Event {
        epoch: u64,
        event: sse::Event,
        /// The event's place among those that wait: given back when it is handled or dropped.
        _room: OwnedSemaphorePermit,
    },
    /// The attempt failed. `quiet`: a live stream ended on home's side.
    Lost {
        epoch: u64,
        failure: Failure,
        quiet: bool,
    },
    Called {
        epoch: u64,
        lane: Lane,
        result: Result<Value, Failure>,
        took: Duration,
        /// False when the connection never opened: the call did not leave.
        sent: bool,
    },
    Typed {
        epoch: u64,
        block: String,
        result: Result<Value, Failure>,
        took: Duration,
    },
    /// A terminal's block is gone or isn't a terminal: no stream until the set changes.
    Stopped {
        epoch: u64,
        block: String,
    },
    /// The backoff ran out.
    Retry {
        epoch: u64,
    },
}

/// The thread: a runtime, and one coordinator task per remote.
pub(crate) fn run(
    keys: Keys,
    out: Arc<Shared>,
    limits: Limits,
    mut orders: UnboundedReceiver<Order>,
) {
    let Ok(runtime) = Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    else {
        return;
    };
    runtime.block_on(async {
        let mut remotes: HashMap<String, (UnboundedSender<Msg>, tokio::task::JoinHandle<()>)> =
            HashMap::new();
        while let Some(order) = orders.recv().await {
            match order {
                Order::Connect { id, round, remote } => {
                    if let Some((_, old)) = remotes.remove(&id) {
                        old.abort();
                    }
                    let (tx, rx) = unbounded_channel();
                    let life = Life::new(
                        &id,
                        round,
                        *remote,
                        keys.clone(),
                        out.clone(),
                        limits,
                        tx.clone(),
                    );
                    remotes.insert(id, (tx, tokio::spawn(life.run(rx))));
                }
                Order::Send(id, msg) => {
                    if let Some((tx, _)) = remotes.get(&id) {
                        let _ = tx.send(msg);
                    }
                }
                // The task finishes by itself (it revokes, deletes the key, ends).
                Order::Forget(id, revoke) => {
                    if let Some((tx, _)) = remotes.remove(&id) {
                        let _ = tx.send(Msg::Forget { revoke });
                    }
                }
                Order::DropKey(entry, file) => {
                    let keys = keys.clone();
                    tokio::task::spawn_blocking(move || keys.delete(&entry, file));
                }
                Order::Probe => {
                    let (keys, out) = (keys.clone(), out.clone());
                    tokio::task::spawn_blocking(move || {
                        out.push("", 0, Update::Keyring(keys.answers()));
                    });
                }
            }
        }
    });
    runtime.shutdown_background();
}

/// What a connection needs, new for each attempt (its session is).
struct Net {
    place: Place,
    tls: Option<Arc<ClientConfig>>,
    proxy: Option<Proxy>,
    headers: Headers,
}

/// What an attempt is made of: dropping it ends every task and connection.
struct Attempt {
    net: Arc<Net>,
    /// The calls connection, and the slow one, which starts with the first slow call.
    quick: Wire,
    slow: Option<Wire>,
    /// The calls that wait for a free connection, oldest first.
    jobs: VecDeque<Job>,
    typing: Option<UnboundedSender<Batch>>,
    events_started: bool,
    _tasks: Vec<Task>,
}

/// Which connection makes a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Quick,
    Slow,
}

/// A calls connection as the coordinator sees it: one call at a time goes to it, and who awaits
/// the answer waits here until it comes.
struct Wire {
    work: UnboundedSender<Work>,
    busy: Option<To>,
    /// The slow connection's task, ended with it (the quick one's is the attempt's).
    _task: Option<Task>,
}

/// What a calls connection is handed.
struct Work {
    tool: String,
    args: Value,
}

/// A call that waits for its connection, and who it answers to.
struct Job {
    call: Call,
    to: To,
}

enum To {
    /// A block moved: the answer is only heard when it is a refusal.
    Move(String),
    Answer(oneshot::Sender<Answered>),
    /// A terminal's history, and `fed` when it was asked: the answer is filled into its viewer
    /// here, never sent to the UI.
    History(String, u64),
}

struct Batch {
    block: String,
    args: Value,
}

/// What one remote is ready with once its key is loaded.
struct Ready {
    place: Place,
    tls: Option<Arc<ClientConfig>>,
    /// The public verifier, to forget its verdicts when the person presses Try now.
    checked: Option<Arc<trust::Checked>>,
    proxy: Option<Proxy>,
    secrets: super::Secrets,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Disconnected or cut: nothing runs.
    Idle,
    Opening,
    Live,
    /// Waiting out the backoff.
    Waiting,
}

/// One waiting piece of typing.
enum Item {
    Key(String),
    Paste(String),
    Mouse(MouseInput),
}

#[derive(Default)]
struct Queue {
    items: VecDeque<Item>,
    keys: usize,
    paste_bytes: usize,
    /// `Dropped` was said and the queue hasn't drained since: not said again.
    dropped: bool,
}

struct Life {
    id: String,
    round: u64,
    remote: Remote,
    /// Where the key was kept when this began: Forget deletes it there, whatever `remote.key`
    /// became since (memory, when a re-save was refused).
    kept_as: Option<(String, bool)>,
    keys: Keys,
    out: Arc<Shared>,
    limits: Limits,
    tx: UnboundedSender<Msg>,
    ready: Option<Ready>,
    phase: Phase,
    /// The last status pushed, so a repeat isn't.
    status: Option<Status>,
    /// Which attempt is current: messages of another are ignored.
    epoch: u64,
    failures: u32,
    rechecking: bool,
    last_id: Option<String>,
    me: Option<MeView>,
    last_live: Option<SystemTime>,
    live_since: Option<Instant>,
    attempt: Option<Attempt>,
    timer: Option<Task>,
    wanted: Vec<(String, Feeder)>,
    blocked: HashSet<String>,
    /// Each open stream, and the viewer it feeds.
    streams: HashMap<String, (Feeder, Task)>,
    queues: HashMap<String, Queue>,
    /// The blocks with something waiting, oldest first.
    turns: VecDeque<String>,
    typing_busy: bool,
    /// The blocks typed into while not live that were told so, until live again.
    refused: HashSet<String>,
    /// The one terminal whose history a viewer holds: its block, the viewer and what
    /// `Feeder::fed` was when it was asked for.
    history: Option<(String, Feeder, u64)>,
    /// The terminal whose history was asked for last: the only answer filled.
    history_asked: Option<String>,
}

impl Life {
    fn new(
        id: &str,
        round: u64,
        remote: Remote,
        keys: Keys,
        out: Arc<Shared>,
        limits: Limits,
        tx: UnboundedSender<Msg>,
    ) -> Life {
        let kept_as = match &remote.key {
            KeySource::Kept { entry, file } | KeySource::Save { entry, file, .. } => {
                Some((entry.clone(), *file))
            }
            KeySource::Memory(_) => None,
        };
        Life {
            id: id.into(),
            round,
            remote,
            kept_as,
            keys,
            out,
            limits,
            tx,
            ready: None,
            phase: Phase::Idle,
            status: None,
            epoch: 0,
            failures: 0,
            rechecking: false,
            last_id: None,
            me: None,
            last_live: None,
            live_since: None,
            attempt: None,
            timer: None,
            wanted: Vec::new(),
            blocked: HashSet::new(),
            streams: HashMap::new(),
            queues: HashMap::new(),
            turns: VecDeque::new(),
            typing_busy: false,
            refused: HashSet::new(),
            history: None,
            history_asked: None,
        }
    }

    async fn run(mut self, mut inbox: UnboundedReceiver<Msg>) {
        self.start().await;
        while let Some(msg) = inbox.recv().await {
            if !self.handle(msg).await {
                return;
            }
        }
    }

    fn say(&self, update: Update) {
        self.out.push(&self.id, self.round, update);
    }

    fn status(&mut self, status: Status) {
        if self.status.as_ref() != Some(&status) {
            self.status = Some(status.clone());
            self.say(Update::Status(status));
        }
    }

    /// Loads the key and builds the verifier, then makes the first attempt.
    async fn start(&mut self) {
        self.status(Status::Connecting);
        match self.prepare().await {
            Ok(()) => self.begin(),
            Err(why) => self.cut(why),
        }
    }

    async fn prepare(&mut self) -> Result<(), Why> {
        if self.ready.is_some() {
            return Ok(());
        }
        let secrets = match self.remote.key.clone() {
            KeySource::Memory(secrets) => secrets,
            KeySource::Save {
                entry,
                mut secrets,
                file,
                from,
            } => {
                if let Some(from) = from {
                    let keys = self.keys.clone();
                    let old = blocking(move || keys.load(&from.entry, from.file))
                        .await
                        .and_then(Result::ok);
                    let Some(old) = old else {
                        // It may be the only copy of the Access token: nothing is saved, so
                        // nothing is dropped, and Try again reads it again.
                        self.say(Update::Refused {
                            about: About::Key,
                            sentence: "Templum couldn't read the old key for the Access token or the proxy login: try again, or connect again with a new code.".into(),
                        });
                        return Err(Why::Keyring);
                    };
                    if from.access {
                        // The secret was bound to its address (1b): it goes only to the same
                        // origin, whatever the old index entry says.
                        let same = place(&old.url)
                            .zip(place(&secrets.url))
                            .is_some_and(|(old, new)| old.same_origin(&new));
                        if same {
                            secrets.access_secret = old.access_secret;
                        } else {
                            self.say(Update::Refused {
                                about: About::Key,
                                sentence:
                                    "The address changed: type the Client Secret again under Network."
                                        .into(),
                            });
                        }
                    }
                    if from.login {
                        secrets.proxy_auth = old.proxy_auth;
                    }
                }
                let (keys, kept) = (self.keys.clone(), secrets.clone());
                let saved = blocking(move || keys.save(&entry, &kept, file))
                    .await
                    .unwrap_or_else(|| Err("Templum couldn't keep this connection's key.".into()));
                match saved {
                    Ok(kept) => self.say(Update::Kept(kept)),
                    Err(sentence) => {
                        self.say(Update::Refused {
                            about: About::Key,
                            sentence,
                        });
                        return Err(Why::Keyring);
                    }
                }
                secrets
            }
            KeySource::Kept { entry, file } => {
                let keys = self.keys.clone();
                blocking(move || keys.load(&entry, file))
                    .await
                    .and_then(Result::ok)
                    .ok_or(Why::Keyring)?
            }
        };
        if !self.remote.binds(&secrets) {
            self.say(Update::Refused {
                about: About::Key,
                sentence:
                    "This connection's address or way in changed outside Templum, so its key \
                           isn't sent. Connect again with a new code."
                        .into(),
            });
            return Err(Why::Keyring);
        }
        let place = place(&self.remote.url).ok_or_else(|| Why::NotTemplum {
            status: 0,
            kind: "no address".into(),
        })?;
        if !fits(&self.remote.trust, &place) {
            self.say(Update::Refused {
                about: About::Key,
                sentence: "The address and the way it is reached don't agree, so the key isn't \
                           sent."
                    .into(),
            });
            return Err(Why::NotTemplum {
                status: 0,
                kind: "an address that doesn't fit how it is reached".into(),
            });
        }
        let mut checked = None;
        let tls = match &self.remote.trust {
            Trust::Loopback => None,
            Trust::Pinned(pin) => {
                let pinned = Pinned::new(pin).ok_or(Why::Certificate(Bad::Other))?;
                Some(trust::config(Arc::new(pinned)))
            }
            Trust::Public => {
                let (system, public) = blocking(trust::web)
                    .await
                    .and_then(Result::ok)
                    .ok_or(Why::Certificate(Bad::Other))?;
                let verifier =
                    Arc::new(trust::Checked::new(system, public, self.remote.trust_added));
                checked = Some(verifier.clone());
                Some(trust::config(verifier))
            }
        };
        let proxy = match &self.remote.proxy {
            // This computer is never reached through a proxy.
            _ if self.remote.trust == Trust::Loopback => None,
            None => None,
            Some(address) => Some(
                Proxy::new(address, secrets.proxy_auth.as_deref()).ok_or_else(|| Why::Proxy {
                    proxy: address.clone(),
                    status: 0,
                })?,
            ),
        };
        self.ready = Some(Ready {
            place,
            tls,
            checked,
            proxy,
            secrets,
        });
        Ok(())
    }

    /// Everything of the old attempt dropped: its tasks, connections, streams and waiting keys.
    /// What its tasks already said is ignored from now on: the epoch moves.
    fn drop_attempt(&mut self) {
        self.epoch += 1;
        if let Some(attempt) = self.attempt.take() {
            let busy = [Some(attempt.quick), attempt.slow]
                .into_iter()
                .flatten()
                .filter_map(|wire| wire.busy);
            let waiting = attempt.jobs.into_iter().map(|job| job.to);
            // A call in flight was sent: home may have done it.
            let may_have_done_it = format!(
                "{} may have done it: the connection dropped before it answered.",
                self.computer()
            );
            for to in busy {
                self.refuse_with(to, Refusal::maybe_done(may_have_done_it.clone()));
            }
            for to in waiting {
                self.refuse(to, "Couldn't reach the other computer.");
            }
        }
        self.timer = None;
        self.streams.clear();
        self.drop_stale_history();
        self.queues.clear();
        self.turns.clear();
        self.typing_busy = false;
    }

    /// A new attempt, with a session of its own: the calls task opens its connection and asks
    /// `GET /v1/me`.
    fn begin(&mut self) {
        self.drop_attempt();
        self.phase = Phase::Opening;
        self.live_since = None;
        let Some(ready) = &self.ready else { return };
        let net = Arc::new(Net {
            place: ready.place.clone(),
            tls: ready.tls.clone(),
            proxy: ready.proxy.clone(),
            headers: Headers {
                key: ready.secrets.token.clone(),
                access: self
                    .remote
                    .access_id
                    .clone()
                    .zip(ready.secrets.access_secret.clone()),
                session: crate::store::random_text("", 12),
                host: ready.place.host_header(),
            },
        });
        let (work, inbox) = unbounded_channel();
        let task = Task::spawn(calls_task(
            net.clone(),
            self.epoch,
            Lane::Quick,
            inbox,
            self.tx.clone(),
            self.limits.me_every,
        ));
        self.attempt = Some(Attempt {
            net,
            quick: Wire {
                work,
                busy: None,
                _task: None,
            },
            slow: None,
            jobs: VecDeque::new(),
            typing: None,
            events_started: false,
            _tasks: vec![task],
        });
    }

    /// Stops for good, until the person tries again.
    fn cut(&mut self, why: Why) {
        self.drop_attempt();
        self.phase = Phase::Idle;
        self.live_since = None;
        self.status(Status::Cut(why));
    }

    fn lost(&mut self, failure: Failure, quiet: bool) {
        match failure {
            Failure::Auth => {
                let ended = self
                    .me
                    .as_ref()
                    .and_then(|me| me.expires)
                    .is_some_and(|end| unix_now() >= end);
                self.cut(if ended { Why::Expired } else { Why::Revoked });
            }
            Failure::Stop(why) => self.cut(why),
            Failure::Busy(wait) => self.retry(wait, false),
            Failure::Down(_) | Failure::Missing(_) | Failure::Refused(..) | Failure::TooLarge => {
                self.retry(Duration::ZERO, quiet)
            }
        }
    }

    fn retry(&mut self, floor: Duration, quiet: bool) {
        let was_live = self.phase == Phase::Live;
        let lived = self.live_since.take().map(|since| since.elapsed());
        if lived.is_some() {
            self.last_live = Some(SystemTime::now());
        }
        if lived.is_some_and(|lived| lived >= self.limits.live_resets) {
            self.failures = 0;
        }
        // A live stream that home ended may mean a revoke or a deleted workspace: look again at
        // once, so the person reads that, not "reconnecting". (Not twice in a row if the look
        // came back live and ended within a second.)
        let looks = quiet
            && was_live
            && (!self.rechecking || lived.is_some_and(|l| l >= Duration::from_secs(1)));
        if looks {
            self.rechecking = true;
            self.begin();
            return;
        }
        self.rechecking = false;
        self.failures += 1;
        let next = backoff(self.failures, jitter()).max(floor);
        self.drop_attempt();
        self.phase = Phase::Waiting;
        self.status(Status::Reconnecting {
            attempt: self.failures,
            next,
            live_at: self.last_live,
        });
        let (tx, epoch) = (self.tx.clone(), self.epoch);
        self.timer = Some(Task::spawn(async move {
            tokio::time::sleep(next).await;
            let _ = tx.send(Msg::Retry { epoch });
        }));
    }

    /// Whether the coordinator goes on.
    async fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Forget { revoke } => {
                self.forget(revoke).await;
                return false;
            }
            Msg::Command(command) => self.command(command).await,
            Msg::Retry { epoch } if epoch == self.epoch && self.phase == Phase::Waiting => {
                self.begin();
            }
            Msg::Me { epoch, result } if epoch == self.epoch => self.me_arrived(result),
            // A failure says nothing: the tools stay unknown, and nothing is dimmed as older.
            Msg::Tools {
                epoch,
                result: Ok(list),
            } if epoch == self.epoch => self.say(Update::Tools(Tools::read(&list))),
            Msg::Opened { epoch } if epoch == self.epoch => self.live(),
            Msg::Event { epoch, event, .. } if epoch == self.epoch => self.event(event),
            Msg::Lost {
                epoch,
                failure,
                quiet,
            } if epoch == self.epoch => self.lost(failure, quiet),
            Msg::Called {
                epoch,
                lane,
                result,
                took,
                sent,
            } if epoch == self.epoch => self.called(lane, result, took, sent),
            Msg::Typed {
                epoch,
                block,
                result,
                took,
            } if epoch == self.epoch => self.typed(block, result, took),
            Msg::Stopped { epoch, block } if epoch == self.epoch => {
                self.streams.remove(&block);
                self.blocked.insert(block);
            }
            _ => {}
        }
        true
    }

    fn me_arrived(&mut self, result: Result<(MeView, Duration), Failure>) {
        let (me, took) = match result {
            Ok(done) => done,
            // A refresh that couldn't reach home: the stream tells soon enough.
            Err(
                Failure::Down(_)
                | Failure::Busy(_)
                | Failure::TooLarge
                | Failure::Refused(..)
                | Failure::Missing(_),
            ) if self.phase == Phase::Live => {
                return;
            }
            Err(failure) => return self.lost(failure, false),
        };
        self.say(Update::Me(me.clone()));
        self.say(Update::Latency(took));
        let deleted = me.workspaces.as_ref().is_some_and(Vec::is_empty);
        self.me = Some(me);
        if deleted {
            return self.cut(Why::Deleted);
        }
        let Some(attempt) = &mut self.attempt else {
            return;
        };
        if attempt.events_started {
            return;
        }
        attempt.events_started = true;
        let task = Task::spawn(events_task(
            attempt.net.clone(),
            self.epoch,
            self.last_id.clone(),
            self.tx.clone(),
            self.limits.silence,
        ));
        attempt._tasks.push(task);
    }

    /// The event stream is open.
    fn live(&mut self) {
        let Some(attempt) = &mut self.attempt else {
            return;
        };
        self.phase = Phase::Live;
        self.refused.clear();
        self.live_since = Some(Instant::now());
        let (typing, inbox) = unbounded_channel();
        attempt.typing = Some(typing);
        attempt._tasks.push(Task::spawn(typing_task(
            attempt.net.clone(),
            self.epoch,
            inbox,
            self.tx.clone(),
            self.limits.typing_patience,
        )));
        self.status(Status::Live);
        self.sync_terminals();
    }

    fn event(&mut self, event: sse::Event) {
        if event.event == crate::REPLACED {
            return self.cut(Why::Replaced);
        }
        if let Some(id) = event.id {
            self.last_id = Some(id);
        }
        if event.event == "notice" {
            // Not state: one that is malformed is dropped, and the stream goes on.
            if let Some(notice) = notice_of(&event.data) {
                self.say(Update::Notice(notice));
            }
            return;
        }
        let applied = serde_json::from_str::<Value>(&event.data)
            .map_err(|e| ApplyError::Malformed(format!("a malformed {} event: {e}", event.event)))
            .and_then(|data| {
                self.out
                    .apply(&self.id, self.round, &event.event, &data, event.data.len())
            });
        match applied {
            Ok(_) => {}
            Err(ApplyError::TooLarge) => self.lost(Failure::TooLarge, false),
            Err(why) => self.lost(Failure::Down(why.to_string()), false),
        }
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::Disconnect => {
                self.drop_attempt();
                self.phase = Phase::Idle;
                self.live_since = None;
                self.status(Status::Disconnected);
            }
            Command::TryNow => match self.phase {
                Phase::Waiting | Phase::Opening => self.begin(),
                Phase::Idle => {
                    self.failures = 0;
                    self.rechecking = false;
                    // The person may have fixed what the last verdict named.
                    if let Some(checked) = self.ready.as_ref().and_then(|r| r.checked.as_ref()) {
                        checked.forget();
                    }
                    self.start().await;
                }
                Phase::Live => {}
            },
            Command::TrustAdded => self.trust_added().await,
            Command::Terminals(set) => {
                let set: Vec<_> = set.into_iter().take(STREAMS_MOST).collect();
                // Home refused a stream: it is asked again when the terminals shown change, not
                // when they are only put in another order.
                let same = set.len() == self.wanted.len()
                    && set
                        .iter()
                        .all(|(b, _)| self.wanted.iter().any(|(w, _)| w == b));
                if !same {
                    self.blocked.clear();
                }
                self.wanted = set;
                self.sync_terminals();
            }
            Command::Keys { block, keys } => self.type_keys(block, keys),
            Command::Paste { block, text } => self.type_paste(block, text),
            Command::Mouse { block, event } => self.type_mouse(block, event),
            Command::History { block } => self.history(block),
            Command::Move { block, rect } => {
                let args = json!({
                    "block": block, "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h,
                });
                let call = Call {
                    tool: "move_block".into(),
                    args,
                    slow: false,
                };
                self.call(Job {
                    call,
                    to: To::Move(block),
                });
            }
            Command::Call { call, answer } => self.call(Job {
                call,
                to: To::Answer(answer),
            }),
        }
    }

    /// The person accepted the authority this computer added: the secrets the remote was cut with
    /// are kept again with that choice (where they were, else in memory), and it starts again.
    async fn trust_added(&mut self) {
        let Some(mut secrets) = self.ready.as_ref().map(|r| r.secrets.clone()) else {
            return;
        };
        secrets.trust_added = true;
        self.remote.trust_added = true;
        let place = match &self.remote.key {
            KeySource::Kept { entry, file } | KeySource::Save { entry, file, .. } => {
                Some((entry.clone(), *file))
            }
            KeySource::Memory(_) => None,
        };
        let kept = match place {
            Some((entry, file)) => {
                let (keys, again) = (self.keys.clone(), secrets.clone());
                let saved = blocking(move || keys.save(&entry, &again, file)).await;
                saved
                    .and_then(Result::ok)
                    .filter(|kept| *kept != Kept::Memory)
            }
            None => None,
        };
        if kept.is_none() {
            self.remote.key = KeySource::Memory(secrets);
        } else if let KeySource::Save { entry, file, .. } = self.remote.key.clone() {
            self.remote.key = KeySource::Kept { entry, file };
        }
        self.ready = None;
        self.failures = 0;
        self.rechecking = false;
        self.start().await;
    }

    /// The computer's name for the sentences that name it.
    fn computer(&self) -> String {
        self.me
            .as_ref()
            .map(|me| me.computer.clone())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "The other computer".into())
    }

    /// Answers a call's `to` with a refusal. A move is told as an update, as it always was.
    fn refuse(&self, to: To, sentence: impl Into<String>) {
        self.refuse_with(to, Refusal::of(sentence));
    }

    fn refuse_with(&self, to: To, refusal: Refusal) {
        match to {
            To::Move(block) => self.say(Update::Refused {
                about: About::Move(block),
                sentence: refusal.sentence,
            }),
            To::Answer(answer) => {
                let _ = answer.send(Err(refusal));
            }
            // Scrolling back says nothing when it can't read: the person keeps the live screen.
            To::History(..) => {}
        }
    }

    /// Queues a call for a free connection, or refuses it now: not live, or 64 already wait. A
    /// move of a block whose earlier move still waits takes its place, so a drag sends its newest.
    fn call(&mut self, job: Job) {
        let live = self.phase == Phase::Live;
        let Some(attempt) = self.attempt.as_mut().filter(|_| live) else {
            return self.refuse(job.to, "Not connected.");
        };
        let twin = match &job.to {
            To::Move(block) => attempt
                .jobs
                .iter_mut()
                .find(|waiting| matches!(&waiting.to, To::Move(b) if b == block)),
            To::Answer(_) | To::History(..) => None,
        };
        if let Some(waiting) = twin {
            *waiting = job;
        } else if attempt.jobs.len() >= CALLS_WAITING_MOST {
            let sentence = format!("{} is slow to answer: try again.", self.computer());
            return self.refuse(job.to, sentence);
        } else {
            attempt.jobs.push_back(job);
        }
        self.pump_calls();
    }

    /// Hands the oldest waiting call to each connection that is free: the quick one, and the slow
    /// one (which starts with the first call that needs it).
    fn pump_calls(&mut self) {
        let Some(attempt) = self.attempt.as_mut().filter(|_| self.phase == Phase::Live) else {
            return;
        };
        let (net, epoch, tx) = (attempt.net.clone(), self.epoch, self.tx.clone());
        let mut gone = Vec::new();
        for lane in [Lane::Quick, Lane::Slow] {
            let slow = lane == Lane::Slow;
            let free = match lane {
                Lane::Quick => attempt.quick.busy.is_none(),
                Lane::Slow => attempt.slow.as_ref().is_none_or(|wire| wire.busy.is_none()),
            };
            let Some(at) = attempt.jobs.iter().position(|job| job.call.slow == slow) else {
                continue;
            };
            if !free {
                continue;
            }
            let Some(Job { call, to }) = attempt.jobs.remove(at) else {
                continue;
            };
            let wire = match lane {
                Lane::Quick => &mut attempt.quick,
                Lane::Slow => attempt.slow.get_or_insert_with(|| {
                    let (work, inbox) = unbounded_channel();
                    let task = Task::spawn(calls_task(
                        net.clone(),
                        epoch,
                        Lane::Slow,
                        inbox,
                        tx.clone(),
                        Duration::MAX,
                    ));
                    Wire {
                        work,
                        busy: None,
                        _task: Some(task),
                    }
                }),
            };
            let work = Work {
                tool: call.tool,
                args: call.args,
            };
            if wire.work.send(work).is_ok() {
                wire.busy = Some(to);
            } else {
                gone.push(to);
            }
        }
        for to in gone {
            self.refuse(to, "Couldn't reach the other computer.");
        }
    }

    fn called(&mut self, lane: Lane, result: Result<Value, Failure>, took: Duration, sent: bool) {
        // A failure that ends the attempt leaves its call to `drop_attempt`, which refuses it.
        // (A slow call that stops after it was sent is told as one that didn't answer: it may still be
        // running.)
        if let Err(failure) = &result
            && (matches!(failure, Failure::Auth)
                || (lane == Lane::Quick && matches!(failure, Failure::Stop(_))))
        {
            return self.lost(failure.clone(), false);
        }
        let Some(attempt) = self.attempt.as_mut() else {
            return;
        };
        let to = match lane {
            Lane::Quick => attempt.quick.busy.take(),
            Lane::Slow => attempt.slow.as_mut().and_then(|wire| wire.busy.take()),
        };
        let Some(to) = to else { return };
        let answer = match result {
            Ok(json) => {
                if lane == Lane::Quick {
                    self.say(Update::Latency(took));
                }
                Ok(json)
            }
            Err(Failure::Refused(sentence, details)) => Err(Refusal {
                sentence,
                details,
                maybe_done: false,
            }),
            Err(Failure::Missing(sentence)) => Err(Refusal::of(sentence)),
            Err(Failure::TooLarge) => Err(Refusal::of(format!(
                "{}'s answer was too large to show.",
                self.computer()
            ))),
            // A slow call cut by a timeout or a tunnel may still be running there; the slow
            // connection is dropped, the remote is not.
            Err(_) if lane == Lane::Slow && sent => {
                if let Some(attempt) = self.attempt.as_mut() {
                    attempt.slow = None;
                }
                Err(Refusal::maybe_done(format!(
                    "{} didn't answer in time: it may still be running there.",
                    self.computer()
                )))
            }
            Err(_) => {
                if lane == Lane::Slow
                    && let Some(attempt) = self.attempt.as_mut()
                {
                    attempt.slow = None;
                }
                Err(Refusal {
                    maybe_done: sent,
                    ..Refusal::of("Couldn't reach the other computer.")
                })
            }
        };
        match (to, answer) {
            (To::Answer(sender), answer) => {
                let _ = sender.send(answer);
            }
            (To::Move(_), Ok(_)) => {}
            (to @ To::Move(_), Err(refusal)) => self.refuse(to, refusal.sentence),
            (To::History(block, fed), Ok(json)) => self.fill_history(block, fed, &json),
            (To::History(..), Err(_)) => {}
        }
        self.pump_calls();
    }

    /// Opens a stream for each wanted terminal that has none, and ends those no longer wanted.
    fn sync_terminals(&mut self) {
        if self.phase != Phase::Live || self.attempt.is_none() {
            return;
        }
        let wanted = &self.wanted;
        // A stream feeds the viewer it was opened for: another viewer for the block, another stream.
        self.streams.retain(|block, (feeder, _)| {
            wanted
                .iter()
                .any(|(w, f)| w == block && f.same_viewer(feeder))
        });
        self.drop_stale_history();
        let Some(attempt) = &self.attempt else { return };
        for (block, feeder) in &self.wanted {
            if self.streams.contains_key(block) || self.blocked.contains(block) {
                continue;
            }
            self.streams.insert(
                block.clone(),
                (
                    feeder.clone(),
                    Task::spawn(terminal_task(
                        attempt.net.clone(),
                        self.epoch,
                        block.clone(),
                        feeder.clone(),
                        self.tx.clone(),
                        self.limits.silence,
                    )),
                ),
            );
        }
    }

    /// Reads home's history of the terminal `block` into its viewer (`Command::History`): not when
    /// the viewer holds it already and no frame was fed since, nor while a read of it is under
    /// way. The answer is cut and filled here, on this thread; the UI only hears it is there.
    fn history(&mut self, block: String) {
        let Some(feeder) = self.streams.get(&block).map(|(feeder, _)| feeder.clone()) else {
            return;
        };
        let Some(attempt) = self.attempt.as_ref().filter(|_| self.phase == Phase::Live) else {
            return;
        };
        let fed = feeder.fed();
        if self.history.as_ref().is_some_and(|(held, viewer, was)| {
            *held == block && viewer.same_viewer(&feeder) && *was == fed
        }) {
            return;
        }
        self.history_asked = Some(block.clone());
        let reading = |to: &To| matches!(to, To::History(b, _) if *b == block);
        if attempt.quick.busy.iter().any(reading) || attempt.jobs.iter().any(|job| reading(&job.to))
        {
            return;
        }
        let call = Call {
            tool: "read_terminal".into(),
            args: json!({ "block": block, "lines": MAX_LINES }),
            slow: false,
        };
        self.call(Job {
            call,
            to: To::History(block, fed),
        });
    }

    /// Home's answer to a read of a terminal's history: filled above the live screen of its
    /// viewer when it is still the one asked for and still streams. Home is not trusted: the text
    /// is cut to its end, and `Feeder::fill_history` draws it and runs nothing.
    /// `fed` is the viewer's when the read was asked, so a frame fed since makes the next ask read
    /// again.
    fn fill_history(&mut self, block: String, fed: u64, answer: &Value) {
        if self.history_asked.as_ref() != Some(&block) {
            return;
        }
        let Some(feeder) = self.streams.get(&block).map(|(feeder, _)| feeder.clone()) else {
            return;
        };
        let Some(text) = answer.get("text").and_then(Value::as_str) else {
            return;
        };
        if !feeder.fill_history(tail(text, MAX_TEXT)) {
            // Unchanged: a held history stays held, so it is dropped when its stream ends.
            return;
        }
        if let Some((held, viewer, _)) = self.history.take()
            && (held != block || !viewer.same_viewer(&feeder))
        {
            viewer.drop_history();
        }
        self.history = Some((block.clone(), feeder, fed));
        self.say(Update::History(block));
    }

    /// Forgets the history of a terminal that no longer streams to its viewer: a viewer's
    /// scrollback is 0 again.
    fn drop_stale_history(&mut self) {
        let streaming = |block: &str, viewer: &Feeder| {
            self.streams
                .get(block)
                .is_some_and(|(feeder, _)| feeder.same_viewer(viewer))
        };
        if let Some((block, viewer, _)) = &self.history
            && !streaming(block, viewer)
        {
            viewer.drop_history();
            self.history = None;
        }
        if let Some(block) = &self.history_asked
            && !self.streams.contains_key(block)
        {
            self.history_asked = None;
        }
    }

    fn queue_of(&mut self, block: &str) -> &mut Queue {
        if !self.turns.iter().any(|b| b == block) && !self.queues.contains_key(block) {
            self.turns.push_back(block.into());
        }
        self.queues.entry(block.into()).or_default()
    }

    /// Typing while not live is never sent late; the person is told once per block, until live
    /// again (the status may still say Live while the look after a stream's end is made).
    fn not_live(&mut self, block: String) {
        if self.refused.insert(block.clone()) {
            self.say(Update::Refused {
                about: About::Keys(block),
                sentence: "Not connected: what you type isn't sent.".into(),
            });
        }
    }

    fn type_keys(&mut self, block: String, keys: Vec<String>) {
        if self.phase != Phase::Live {
            return self.not_live(block);
        }
        let queue = self.queue_of(&block);
        let room = WAITING_MOST.saturating_sub(queue.keys);
        let dropped = keys.len() > room;
        for key in keys.into_iter().take(room) {
            queue.items.push_back(Item::Key(key));
            queue.keys += 1;
        }
        if dropped {
            self.said_dropped(block);
        }
        self.pump();
    }

    fn type_paste(&mut self, block: String, text: String) {
        if self.phase != Phase::Live {
            return self.not_live(block);
        }
        if text.len() > MAX_TEXT {
            return self.say(Update::Refused {
                about: About::Keys(block),
                sentence: "Paste at most 256 KiB into a mirrored terminal.".into(),
            });
        }
        let queue = self.queue_of(&block);
        if queue.paste_bytes + text.len() > MAX_TEXT {
            return self.said_dropped(block);
        }
        queue.paste_bytes += text.len();
        queue.items.push_back(Item::Paste(text));
        self.pump();
    }

    /// A mouse event waits with the keys, in order. A move takes the place of the move waiting
    /// last (a drag or a hover storm is one call, not hundreds), so a click's press and release
    /// are never merged.
    fn type_mouse(&mut self, block: String, event: MouseInput) {
        if self.phase != Phase::Live {
            return self.not_live(block);
        }
        let queue = self.queue_of(&block);
        if event.kind == MouseKind::Move
            && let Some(Item::Mouse(last)) = queue.items.back_mut()
            && last.kind == MouseKind::Move
            && last.button == event.button
        {
            *last = event;
        } else if queue.keys < WAITING_MOST {
            queue.items.push_back(Item::Mouse(event));
            queue.keys += 1;
        } else {
            return self.said_dropped(block);
        }
        self.pump();
    }

    /// `Dropped`, once while the block's queue stays full.
    fn said_dropped(&mut self, block: String) {
        let queue = self.queue_of(&block);
        if !std::mem::replace(&mut queue.dropped, true) {
            self.say(Update::Dropped(block));
        }
    }

    /// Sends the next piece of typing if the typing connection is free: the waiting keys of the
    /// block that has waited longest (at most `INPUT_KEYS`), or its paste, in order.
    fn pump(&mut self) {
        if self.typing_busy {
            return;
        }
        let Some(sender) = self.attempt.as_ref().and_then(|a| a.typing.clone()) else {
            return;
        };
        while let Some(block) = self.turns.pop_front() {
            let Some(queue) = self.queues.get_mut(&block) else {
                continue;
            };
            let args = match queue.items.pop_front() {
                None => {
                    self.queues.remove(&block);
                    continue;
                }
                Some(Item::Paste(text)) => {
                    queue.paste_bytes -= text.len();
                    json!({ "block": block, "text": text })
                }
                Some(Item::Key(first)) => {
                    let mut keys = vec![first];
                    while keys.len() < INPUT_KEYS
                        && matches!(queue.items.front(), Some(Item::Key(_)))
                    {
                        if let Some(Item::Key(key)) = queue.items.pop_front() {
                            keys.push(key);
                        }
                    }
                    queue.keys -= keys.len();
                    json!({ "block": block, "keys": keys })
                }
                Some(Item::Mouse(first)) => {
                    let mut events = vec![mouse_value(&first)];
                    while events.len() < INPUT_MOUSE
                        && matches!(queue.items.front(), Some(Item::Mouse(_)))
                    {
                        if let Some(Item::Mouse(event)) = queue.items.pop_front() {
                            events.push(mouse_value(&event));
                        }
                    }
                    queue.keys -= events.len();
                    json!({ "block": block, "mouse": events })
                }
            };
            if queue.keys < WAITING_MOST && queue.paste_bytes == 0 {
                queue.dropped = false;
            }
            if queue.items.is_empty() {
                self.queues.remove(&block);
            } else {
                self.turns.push_back(block.clone());
            }
            self.typing_busy = sender.send(Batch { block, args }).is_ok();
            return;
        }
    }

    fn typed(&mut self, block: String, result: Result<Value, Failure>, took: Duration) {
        self.typing_busy = false;
        match result {
            Ok(_) => self.say(Update::Latency(took)),
            Err(failure) => {
                // What waits behind a failed call is not sent late.
                self.queues.remove(&block);
                self.turns.retain(|b| *b != block);
                match failure {
                    Failure::Refused(sentence, _) | Failure::Missing(sentence) => {
                        self.say(Update::Refused {
                            about: About::Keys(block),
                            sentence,
                        })
                    }
                    // Home stayed busy through every try, or the call failed (it is not made
                    // again: `terminal_input` is not idempotent): what was waiting is gone.
                    Failure::Busy(_) | Failure::Down(_) | Failure::TooLarge => {
                        self.say(Update::Dropped(block))
                    }
                    failure => return self.lost(failure, false),
                }
            }
        }
        self.pump();
    }

    /// Revokes the share with its own token when asked and live, deletes the key, ends.
    async fn forget(&mut self, revoke: bool) {
        let net = match (revoke, &self.attempt, self.phase) {
            (true, Some(attempt), Phase::Live) => Some(attempt.net.clone()),
            _ => None,
        };
        // The streams go first: the revoke may wait on a slow home for a minute, and a hostile
        // one would fill that minute with events nobody reads.
        self.drop_attempt();
        if let Some(net) = net {
            // Its result is ignored: whether or not home heard, the key goes.
            if let Ok(mut conn) = Conn::open(&net.place, net.tls.clone(), net.proxy.as_ref()).await
            {
                let _ = conn
                    .call(
                        "POST",
                        "/v1/tools/revoke_share",
                        Some(&json!({})),
                        &net.headers,
                    )
                    .await;
            }
        }
        if let Some((entry, file)) = self.kept_as.clone() {
            let keys = self.keys.clone();
            let _ = blocking(move || keys.delete(&entry, file)).await;
        }
    }
}

/// Runs `work` on the runtime's blocking pool: the keyring may wait for the person. None: it
/// panicked.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tokio::task::spawn_blocking(work).await.ok()
}

fn down(error: impl std::fmt::Display) -> Failure {
    Failure::Down(error.to_string())
}

/// The calls connection: on the quick lane `GET /v1/me` first, then the calls it is given one
/// at a time, and `GET /v1/me` again when none came for `every`. The slow lane starts with its
/// first call, waits for the next as long as it takes, and gives each call `SLOW_CALL` to answer.
async fn calls_task(
    net: Arc<Net>,
    epoch: u64,
    lane: Lane,
    mut inbox: UnboundedReceiver<Work>,
    tx: UnboundedSender<Msg>,
    every: Duration,
) {
    let (limit, mut conn, mut job) = (
        if lane == Lane::Slow { SLOW_CALL } else { CALL },
        None::<Conn>,
        None,
    );
    let mut tools_asked = lane == Lane::Slow;
    if lane == Lane::Slow {
        job = match inbox.recv().await {
            Some(first) => Some(first),
            None => return,
        };
    }
    loop {
        if conn.as_ref().is_none_or(|c| !c.open_still()) {
            match Conn::open(&net.place, net.tls.clone(), net.proxy.as_ref()).await {
                Ok(open) => conn = Some(open),
                Err(failure) => {
                    let _ = match job {
                        None => tx.send(Msg::Me {
                            epoch,
                            result: Err(failure),
                        }),
                        Some(_) => tx.send(Msg::Called {
                            epoch,
                            lane,
                            result: Err(failure),
                            took: Duration::ZERO,
                            sent: false,
                        }),
                    };
                    job = match next_call(&mut inbox, every).await {
                        Some(next) => next,
                        None => return,
                    };
                    continue;
                }
            }
        }
        let Some(open) = conn.as_mut() else { continue };
        let started = Instant::now();
        match job.take() {
            None => {
                let result = open
                    .call("GET", "/v1/me", None, &net.headers)
                    .await
                    .and_then(|value| {
                        serde_json::from_value::<MeView>(value).map_err(|_| {
                            Failure::Stop(Why::NotTemplum {
                                status: 200,
                                kind: "application/json".into(),
                            })
                        })
                    });
                let took = started.elapsed();
                let failed = result.is_err();
                let _ = tx.send(Msg::Me {
                    epoch,
                    result: result.map(|me| (me, took)),
                });
                // Home's tools, once per attempt, after its first answer.
                if !failed && !tools_asked {
                    tools_asked = true;
                    let result = open.call("GET", "/v1/tools", None, &net.headers).await;
                    if result.is_err() {
                        conn = None;
                    }
                    let _ = tx.send(Msg::Tools { epoch, result });
                } else if failed {
                    conn = None;
                }
            }
            Some(Work { tool, args }) => {
                let path = format!("/v1/tools/{tool}");
                let most = answer_most(&tool);
                let mut tries = 0;
                // A call home answered "busy" is made again after it says to wait.
                let result = loop {
                    match open
                        .call_within("POST", &path, Some(&args), &net.headers, limit, most)
                        .await
                    {
                        Err(Failure::Busy(wait)) if tries + 1 < BUSY_TRIES => {
                            tries += 1;
                            tokio::time::sleep(wait.max(PACE)).await;
                        }
                        done => break done,
                    }
                };
                if result.is_err() {
                    conn = None;
                }
                let _ = tx.send(Msg::Called {
                    epoch,
                    lane,
                    result,
                    took: started.elapsed(),
                    sent: true,
                });
            }
        }
        job = match next_call(&mut inbox, every).await {
            Some(next) => next,
            None => return,
        };
    }
}

/// The next call, `Some(None)` when none came in `every` (time to ask `GET /v1/me`), None when
/// the coordinator is gone.
async fn next_call(inbox: &mut UnboundedReceiver<Work>, every: Duration) -> Option<Option<Work>> {
    match tokio::time::timeout(every, inbox.recv()).await {
        Ok(Some(work)) => Some(Some(work)),
        Ok(None) => None,
        Err(_) => Some(None),
    }
}

/// The event stream: opens it, says so, and reads it until it ends, goes silent or fails.
async fn events_task(
    net: Arc<Net>,
    epoch: u64,
    last_id: Option<String>,
    tx: UnboundedSender<Msg>,
    silence: Duration,
) {
    let lost = |failure, quiet| {
        let _ = tx.send(Msg::Lost {
            epoch,
            failure,
            quiet,
        });
    };
    let mut events = match stream(
        &net.place,
        net.tls.clone(),
        net.proxy.as_ref(),
        "/v1/events?view=true",
        &net.headers,
        last_id.as_deref(),
    )
    .await
    {
        Ok(events) => events,
        Err(failure) => return lost(failure, false),
    };
    let _ = tx.send(Msg::Opened { epoch });
    let mut reader = sse::Reader::default();
    let room = Arc::new(Semaphore::new(EVENTS_WAITING_MOST));
    loop {
        match tokio::time::timeout(silence, events.chunk()).await {
            Err(_) => return lost(Failure::Down("the stream went silent".into()), false),
            Ok(None) => return lost(Failure::Down("the stream ended".into()), true),
            // Home ending it (it finished, or closed the connection) is `quiet`; a fault of the
            // connection (a reset, bytes that aren't HTTP) is not.
            Ok(Some(Err(failure))) => return lost(failure, false),
            Ok(Some(Ok(bytes))) => match reader.push(&bytes) {
                Err(sse::TooLarge) => return lost(Failure::TooLarge, false),
                Ok(found) => {
                    for event in found {
                        // Never closed: it only waits while the coordinator catches up.
                        let Ok(_room) = room.clone().acquire_owned().await else {
                            return;
                        };
                        let _ = tx.send(Msg::Event {
                            epoch,
                            event,
                            _room,
                        });
                    }
                }
            },
        }
    }
}

/// The typing connection: one call at a time, in the order it is given, each starting at least
/// `PACE` after the previous one did, so the keys typed meanwhile go together. A call home answers
/// "busy" is made again after it says to wait, for `patience` at most (the coordinator keeps what
/// waits behind it).
async fn typing_task(
    net: Arc<Net>,
    epoch: u64,
    mut inbox: UnboundedReceiver<Batch>,
    tx: UnboundedSender<Msg>,
    patience: Duration,
) {
    let mut conn: Option<Conn> = None;
    let mut last: Option<Instant> = None;
    while let Some(Batch { block, args }) = inbox.recv().await {
        let started = Instant::now();
        let mut tries = 0;
        let mut waited = Duration::ZERO;
        let result = loop {
            if let Some(last) = last {
                tokio::time::sleep_until((last + PACE).into()).await;
            }
            last = Some(Instant::now());
            let result = async {
                if conn.as_ref().is_none_or(|c| !c.open_still()) {
                    conn = Some(Conn::open(&net.place, net.tls.clone(), net.proxy.as_ref()).await?);
                }
                let open = conn.as_mut().ok_or_else(|| down("no connection"))?;
                open.call(
                    "POST",
                    "/v1/tools/terminal_input",
                    Some(&args),
                    &net.headers,
                )
                .await
            }
            .await;
            match result {
                Err(Failure::Busy(wait))
                    if tries + 1 < BUSY_TRIES && waited + wait.max(PACE) <= patience =>
                {
                    tries += 1;
                    waited += wait.max(PACE);
                    // `wait` is at most 30 s (`retry_after`), so this cannot overflow.
                    last = Some(Instant::now() + wait.saturating_sub(PACE));
                }
                done => break done,
            }
        };
        if result.is_err() {
            conn = None;
        }
        let _ = tx.send(Msg::Typed {
            epoch,
            block,
            result,
            took: started.elapsed(),
        });
    }
}

/// One terminal's frames, painted into its viewer as they are read; opened again after a second
/// when it ends or fails, while it is wanted.
async fn terminal_task(
    net: Arc<Net>,
    epoch: u64,
    block: String,
    feeder: Feeder,
    tx: UnboundedSender<Msg>,
    silence: Duration,
) {
    if !block
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return;
    }
    let path = format!("/v1/terminals/{block}/stream");
    loop {
        let mut wait = REOPEN;
        match stream(
            &net.place,
            net.tls.clone(),
            net.proxy.as_ref(),
            &path,
            &net.headers,
            None,
        )
        .await
        {
            Ok(mut events) => {
                let (mut reader, mut painter) = (sse::Reader::default(), Painter::default());
                'read: loop {
                    let bytes = match tokio::time::timeout(silence, events.chunk()).await {
                        Ok(Some(Ok(bytes))) => bytes,
                        _ => break,
                    };
                    let Ok(found) = reader.push(&bytes) else {
                        break;
                    };
                    for event in found.iter().filter(|e| e.event == "frame") {
                        let Ok(frame) = serde_json::from_str::<FrameView>(&event.data) else {
                            break 'read;
                        };
                        if painter.paint(&frame, &feeder).is_err() {
                            break 'read;
                        }
                    }
                }
            }
            Err(Failure::Missing(_) | Failure::Refused(..)) => {
                let _ = tx.send(Msg::Stopped { epoch, block });
                return;
            }
            Err(failure @ (Failure::Auth | Failure::Stop(_))) => {
                let _ = tx.send(Msg::Lost {
                    epoch,
                    failure,
                    quiet: false,
                });
                return;
            }
            // Home said "busy": not before it says.
            Err(Failure::Busy(busy)) => wait = wait.max(busy),
            Err(_) => {}
        }
        tokio::time::sleep(wait).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::block_on;
    use crate::client::http::stream_request;
    use crate::code::Key;

    fn near(a: Duration, seconds: f64) -> bool {
        (a.as_secs_f64() - seconds).abs() < 1e-9
    }

    #[test]
    fn backoff_doubles_to_thirty_seconds_with_a_fifth_either_way() {
        for (attempt, seconds) in [
            (1, 1.0),
            (2, 2.0),
            (3, 4.0),
            (5, 16.0),
            (6, 30.0),
            (40, 30.0),
        ] {
            assert!(near(backoff(attempt, 0.0), seconds), "{attempt}");
        }
        assert!(near(backoff(3, 0.2), 4.8) && near(backoff(3, -0.2), 3.2));
        for _ in 0..100 {
            let j = jitter();
            assert!((-0.2..=0.2).contains(&j), "{j}");
        }
    }

    #[test]
    fn the_event_stream_resumes_with_the_last_id() {
        let remote = Remote {
            url: "http://127.0.0.1:9".into(),
            trust: Trust::Loopback,
            trust_added: false,
            access_id: None,
            proxy: None,
            key: KeySource::Memory(super::super::Secrets {
                token: Key::new("tpl_x".into()),
                access_secret: None,
                proxy_auth: None,
                url: "http://127.0.0.1:9".into(),
                trust: Trust::Loopback,
                trust_added: false,
                proxy: None,
            }),
        };
        let (tx, _inbox) = unbounded_channel();
        let out = Arc::new(Shared::for_tests());
        let mut life = Life::new(
            "r1",
            1,
            remote,
            Keys::without_keyring(None),
            out,
            Limits::default(),
            tx,
        );
        let session = |life: &Life| {
            life.attempt
                .as_ref()
                .map(|a| a.net.headers.clone())
                .unwrap()
        };
        let (first, second) = block_on(async {
            life.prepare().await.unwrap();
            life.begin();
            let first = session(&life);
            life.begin();
            (first, session(&life))
        });
        assert_ne!(
            first.session, second.session,
            "each attempt has a session of its own"
        );

        let request = stream_request("", "/v1/events?view=true", &second, Some("r-7")).unwrap();
        assert_eq!(request.uri().to_string(), "/v1/events?view=true");
        assert_eq!(request.headers()["last-event-id"], "r-7");
        assert_eq!(
            request.headers()["templum-mirror"].to_str().unwrap(),
            second.session
        );
        assert_eq!(request.headers()["accept"], "text/event-stream");
        let fresh = stream_request("/base", "/v1/events?view=true", &second, None).unwrap();
        assert_eq!(fresh.uri().to_string(), "/base/v1/events?view=true");
        assert!(fresh.headers().get("last-event-id").is_none());
    }

    // -- A life, and servers that say what a test needs --

    fn life_at(url: &str) -> Life {
        life_with_inbox(url).0
    }

    fn life_with_inbox(url: &str) -> (Life, UnboundedReceiver<Msg>) {
        let remote = Remote {
            url: url.into(),
            trust: Trust::Loopback,
            trust_added: false,
            access_id: None,
            proxy: None,
            key: KeySource::Memory(super::super::Secrets {
                token: Key::new("tpl_x".into()),
                access_secret: None,
                proxy_auth: None,
                url: url.into(),
                trust: Trust::Loopback,
                trust_added: false,
                proxy: None,
            }),
        };
        let (tx, inbox) = unbounded_channel();
        let life = Life::new(
            "r1",
            1,
            remote,
            Keys::without_keyring(None),
            Arc::new(Shared::for_tests()),
            Limits::default(),
            tx,
        );
        (life, inbox)
    }

    /// A job that waits for its connection, and the answer it will get.
    fn answered(tool: &str, slow: bool) -> (Job, oneshot::Receiver<Answered>) {
        let (answer, answered) = oneshot::channel();
        let call = Call {
            tool: tool.into(),
            args: json!({}),
            slow,
        };
        let job = Job {
            call,
            to: To::Answer(answer),
        };
        (job, answered)
    }

    /// Hands each connection to `serve` on a thread of its own. The port.
    fn server(serve: impl Fn(std::net::TcpStream) + Send + Sync + 'static) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let serve = Arc::new(serve);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let serve = serve.clone();
                std::thread::spawn(move || serve(stream));
            }
        });
        port
    }

    /// The next request's head and body; None when the client is gone.
    fn request(stream: &mut std::net::TcpStream) -> Option<(String, String)> {
        use std::io::Read;
        let (mut head, mut byte) = (Vec::new(), [0u8; 1]);
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).ok()? == 0 {
                return None;
            }
            head.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&head).into_owned();
        let length = head
            .to_ascii_lowercase()
            .lines()
            .find_map(|l| l.strip_prefix("content-length: ").map(str::to_string))
            .and_then(|n| n.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        stream.read_exact(&mut body).ok()?;
        Some((head, String::from_utf8_lossy(&body).into_owned()))
    }

    fn net_at(port: u16) -> Arc<Net> {
        let place = place(&format!("http://127.0.0.1:{port}")).unwrap();
        Arc::new(Net {
            headers: Headers {
                key: Key::new("tpl_x".into()),
                access: None,
                session: "s1".into(),
                host: place.host_header(),
            },
            place,
            tls: None,
            proxy: None,
        })
    }

    /// Counts a move asked of the fake; whether it is the first.
    fn counted_move(asked: &std::sync::Mutex<usize>) -> bool {
        let mut asked = asked.lock().unwrap();
        *asked += 1;
        *asked == 1
    }

    fn viewer() -> templum_terminal::Terminal {
        templum_terminal::Terminal::viewer(
            "v",
            (10, 2),
            templum_terminal::Palette::default(),
            templum_terminal::TermOptions::default(),
            Arc::new(|| {}),
        )
    }

    // -- What an attempt that fails asks of the remote --

    #[test]
    fn a_401_after_the_share_ends_is_expired_and_before_it_is_revoked() {
        let ends = |expires| {
            let mut life = life_at("http://127.0.0.1:9");
            life.me = Some(MeView {
                expires,
                ..Default::default()
            });
            life.lost(Failure::Auth, false);
            life.status
        };
        assert_eq!(ends(Some(unix_now() - 5)), Some(Status::Cut(Why::Expired)));
        assert_eq!(
            ends(Some(unix_now() + 3600)),
            Some(Status::Cut(Why::Revoked))
        );
        assert_eq!(ends(None), Some(Status::Cut(Why::Revoked)));
    }

    #[test]
    fn home_saying_busy_sets_the_floor_of_the_wait() {
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.lost(Failure::Busy(Duration::from_secs(40)), false);
            let Some(Status::Reconnecting { next, attempt, .. }) = life.status else {
                panic!("{:?}", life.status);
            };
            assert_eq!(attempt, 1);
            assert!(next >= Duration::from_secs(40), "{next:?}");
            life.lost(Failure::Down("x".into()), false);
            let Some(Status::Reconnecting { next, .. }) = life.status else {
                panic!()
            };
            assert!(
                next < Duration::from_secs(4),
                "a plain failure has no floor: {next:?}"
            );
        });
    }

    #[test]
    fn trusting_an_added_authority_keeps_the_key_that_way_and_starts_again() {
        block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let keys = Keys::without_keyring(Some(dir.path()));
            let secrets = super::super::Secrets {
                token: Key::new("tpl_x".into()),
                access_secret: None,
                proxy_auth: None,
                url: "http://127.0.0.1:9".into(),
                trust: Trust::Loopback,
                trust_added: false,
                proxy: None,
            };
            assert_eq!(keys.save("remote-aa", &secrets, true), Ok(Kept::File));
            let mut life = life_at("http://127.0.0.1:9");
            life.keys = keys.clone();
            life.remote.key = KeySource::Kept {
                entry: "remote-aa".into(),
                file: true,
            };
            life.prepare().await.unwrap();
            life.begin();
            life.cut(Why::AddedAuthority);
            let epoch = life.epoch;
            life.command(Command::TrustAdded).await;
            assert!(life.remote.trust_added);
            assert!(keys.load("remote-aa", true).unwrap().trust_added);
            assert!(life.remote.binds(&keys.load("remote-aa", true).unwrap()));
            assert!(life.epoch > epoch && life.phase == Phase::Opening);
        });
    }

    #[test]
    fn forget_deletes_the_key_where_it_was_kept_even_after_a_refused_re_save() {
        block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let keys = Keys::without_keyring(Some(dir.path()));
            let secrets = super::super::Secrets {
                token: Key::new("tpl_x".into()),
                access_secret: None,
                proxy_auth: None,
                url: "http://127.0.0.1:9".into(),
                trust: Trust::Loopback,
                trust_added: false,
                proxy: None,
            };
            assert_eq!(keys.save("remote-aa", &secrets, true), Ok(Kept::File));
            let mut remote = life_at("http://127.0.0.1:9").remote;
            remote.key = KeySource::Kept {
                entry: "remote-aa".into(),
                file: true,
            };
            let (tx, _inbox) = unbounded_channel();
            let mut life = Life::new(
                "r1",
                1,
                remote,
                keys.clone(),
                Arc::new(Shared::for_tests()),
                Limits::default(),
                tx,
            );
            // What `trust_added` leaves when the keyring refuses the second save.
            life.remote.key = KeySource::Memory(secrets);
            life.forget(false).await;
            assert!(keys.load("remote-aa", true).is_err(), "the key is gone");
        });
    }

    #[test]
    fn a_new_code_keeps_the_access_secret_and_the_proxy_login_it_leaves_empty() {
        block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let keys = Keys::without_keyring(Some(dir.path()));
            let secrets =
                |token: &str, access: Option<&str>, login: Option<&str>| super::super::Secrets {
                    token: Key::new(token.into()),
                    access_secret: access.map(Into::into),
                    proxy_auth: login.map(Into::into),
                    url: "http://127.0.0.1:9".into(),
                    trust: Trust::Loopback,
                    trust_added: false,
                    proxy: None,
                };
            let old = secrets("tpl_old", Some("the-secret"), Some("me:pw"));
            assert_eq!(keys.save("remote-old", &old, true), Ok(Kept::File));
            for (access, login, want) in [
                (true, true, (Some("the-secret"), Some("me:pw"))),
                (true, false, (Some("the-secret"), None)),
                (false, false, (None, None)),
            ] {
                let mut life = life_at("http://127.0.0.1:9");
                life.keys = keys.clone();
                life.remote.key = KeySource::Save {
                    entry: "remote-new".into(),
                    secrets: secrets("tpl_new", None, None),
                    file: true,
                    from: Some(super::super::Inherit {
                        entry: "remote-old".into(),
                        file: true,
                        access,
                        login,
                    }),
                };
                life.prepare().await.unwrap();
                let kept = keys.load("remote-new", true).unwrap();
                assert_eq!(
                    (kept.access_secret.as_deref(), kept.proxy_auth.as_deref()),
                    want
                );
                assert_eq!(kept.token.expose(), "tpl_new");
            }
        });
    }

    /// A life that saves a new key for `url`, inheriting from the old one kept at "remote-old".
    fn inheriting(keys: &Keys, url: &str, access: bool, login: bool) -> Life {
        let mut life = life_at(url);
        life.keys = keys.clone();
        life.out.inner().current.insert("r1".into(), 1);
        life.remote.key = KeySource::Save {
            entry: "remote-new".into(),
            secrets: super::super::Secrets {
                token: Key::new("tpl_new".into()),
                access_secret: None,
                proxy_auth: None,
                url: url.into(),
                trust: Trust::Loopback,
                trust_added: false,
                proxy: None,
            },
            file: true,
            from: Some(super::super::Inherit {
                entry: "remote-old".into(),
                file: true,
                access,
                login,
            }),
        };
        life
    }

    #[test]
    fn the_access_secret_of_an_old_key_is_inherited_only_by_the_same_origin() {
        block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let keys = Keys::without_keyring(Some(dir.path()));
            let old = super::super::Secrets {
                token: Key::new("tpl_old".into()),
                access_secret: Some("the-secret".into()),
                proxy_auth: Some("me:pw".into()),
                url: "http://127.0.0.1:9/a".into(),
                trust: Trust::Loopback,
                trust_added: false,
                proxy: None,
            };
            assert_eq!(keys.save("remote-old", &old, true), Ok(Kept::File));
            // Another path of the same origin keeps it; another host or port never gets it.
            for (url, want) in [
                ("http://127.0.0.1:9/b", Some("the-secret")),
                ("http://127.0.0.1:10", None),
                ("http://localhost:9", None),
            ] {
                let mut life = inheriting(&keys, url, true, true);
                life.prepare().await.unwrap();
                let kept = keys.load("remote-new", true).unwrap();
                assert_eq!(kept.access_secret.as_deref(), want, "{url}");
                assert_eq!(kept.proxy_auth.as_deref(), Some("me:pw"), "the same proxy");
                let said = life.out.inner().updates.clone();
                let told = said.iter().any(|(_, u)| {
                    matches!(u, Update::Refused { sentence, .. } if sentence.starts_with("The address changed"))
                });
                assert_eq!(told, want.is_none(), "{url}");
            }
        });
    }

    #[test]
    fn an_old_key_that_cannot_be_read_stops_before_anything_is_saved() {
        block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let keys = Keys::without_keyring(Some(dir.path()));
            let mut life = inheriting(&keys, "http://127.0.0.1:9", true, true);
            assert!(matches!(life.prepare().await, Err(Why::Keyring)));
            assert!(keys.load("remote-new", true).is_err(), "nothing was saved");
            let said = life.out.inner().updates.clone();
            assert!(
                said.iter().all(|(_, u)| !matches!(u, Update::Kept(_))),
                "no Kept, so nothing is dropped"
            );
            assert!(
                said.iter()
                    .any(|(_, u)| matches!(u, Update::Refused { .. }))
            );
        });
    }

    #[test]
    fn try_now_restarts_an_opening_attempt_too() {
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.prepare().await.unwrap();
            life.begin();
            let epoch = life.epoch;
            assert_eq!(life.phase, Phase::Opening);
            life.command(Command::TryNow).await;
            assert!(life.epoch > epoch, "a new attempt");
            assert_eq!(life.phase, Phase::Opening);
        });
    }

    #[test]
    fn a_call_in_flight_when_the_attempt_is_dropped_is_refused() {
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.out.inner().current.insert("r1".into(), 1);
            life.prepare().await.unwrap();
            life.begin();
            life.phase = Phase::Live;
            let ((first, mut first_answer), (second, mut second_answer)) = (
                answered("answer_request", false),
                answered("answer_request", false),
            );
            life.call(first);
            life.call(second);
            // One was answered before: it is not refused too, and the next goes out.
            life.called(Lane::Quick, Ok(json!({})), Duration::ZERO, true);
            // The stream ended and home was looked at again: no status changes.
            life.begin();
            assert_eq!(first_answer.try_recv().unwrap(), Ok(json!({})));
            assert_eq!(
                second_answer.try_recv().unwrap(),
                Err(Refusal::maybe_done(
                    "The other computer may have done it: the connection dropped before it answered."
                ))
            );
            let said = life.out.inner().updates.clone();
            assert!(
                said.iter()
                    .all(|(_, u)| !matches!(u, Update::Refused { .. })),
                "{said:?}"
            );
        });
    }

    #[test]
    fn a_refused_call_hands_its_details_to_the_ui() {
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.out.inner().current.insert("r1".into(), 1);
            life.prepare().await.unwrap();
            life.begin();
            life.phase = Phase::Live;
            let (job, mut answer) = answered("git", false);
            life.call(job);
            life.called(
                Lane::Quick,
                Err(Failure::Refused("fatal: x".into(), Some("d".into()))),
                Duration::ZERO,
                true,
            );
            assert_eq!(
                answer.try_recv().unwrap(),
                Err(Refusal {
                    sentence: "fatal: x".into(),
                    details: Some("d".into()),
                    maybe_done: false,
                })
            );
        });
    }

    #[test]
    fn calls_waiting_when_the_attempt_drops_are_each_refused_once() {
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.out.inner().current.insert("r1".into(), 1);
            life.prepare().await.unwrap();
            life.begin();
            life.phase = Phase::Live;
            let mut answers = Vec::new();
            for _ in 0..3 {
                let (job, answer) = answered("show", false);
                life.call(job);
                answers.push(answer);
            }
            life.drop_attempt();
            life.begin();
            for (n, mut answer) in answers.into_iter().enumerate() {
                // The first was sent: home may have done it. The others never left.
                let expected = if n == 0 {
                    Refusal::maybe_done(
                        "The other computer may have done it: the connection dropped before it answered.",
                    )
                } else {
                    Refusal::of("Couldn't reach the other computer.")
                };
                assert_eq!(answer.try_recv().unwrap(), Err(expected));
            }
            assert!(
                life.attempt.as_ref().unwrap().jobs.is_empty(),
                "none is sent again"
            );
        });
    }

    #[test]
    fn a_slow_call_a_tunnel_cuts_is_told_and_never_cuts_the_remote() {
        let me = serde_json::to_string(&MeView {
            computer: "winx-desktop".into(),
            ..Default::default()
        })
        .unwrap();
        let port = server(move |mut stream| {
            use std::io::Write;
            while let Some((head, _)) = request(&mut stream) {
                let answer = if head.starts_with("GET /v1/me ") {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{me}",
                        me.len()
                    )
                } else {
                    // Cloudflare's: a tunnel cut the call at 100 s, with no Templum body.
                    "HTTP/1.1 524 A Timeout Occurred\r\nContent-Length: 0\r\n\r\n".to_string()
                };
                if stream.write_all(answer.as_bytes()).is_err() {
                    return;
                }
            }
        });
        block_on(async {
            let (mut life, mut inbox) = life_with_inbox(&format!("http://127.0.0.1:{port}"));
            life.out.inner().current.insert("r1".into(), 1);
            life.prepare().await.unwrap();
            life.begin();
            life.me = Some(MeView {
                computer: "winx-desktop".into(),
                ..Default::default()
            });
            life.phase = Phase::Live;
            life.status(Status::Live);
            let (job, answer) = answered("git", true);
            life.call(job);
            loop {
                match inbox.recv().await {
                    Some(Msg::Called {
                        lane,
                        result,
                        took,
                        sent,
                        ..
                    }) => {
                        life.called(lane, result, took, sent);
                        break;
                    }
                    Some(_) => {}
                    None => panic!("the calls task ended"),
                }
            }
            assert_eq!(
                answer.await.unwrap(),
                Err(Refusal::maybe_done(
                    "winx-desktop didn't answer in time: it may still be running there."
                ))
            );
            assert_eq!(life.phase, Phase::Live);
            let said = life.out.inner().updates.clone();
            assert!(
                said.iter()
                    .all(|(_, u)| matches!(u, Update::Status(Status::Live))),
                "only Live was said: {said:?}"
            );
            assert!(
                life.attempt.as_ref().unwrap().slow.is_none(),
                "the slow connection is dropped"
            );
        });
    }

    #[test]
    fn a_slow_call_that_never_left_or_came_back_too_large_is_not_told_as_one_still_running() {
        for (failure, sent, words) in [
            (
                Failure::Down("refused".into()),
                false,
                "Couldn't reach the other computer.",
            ),
            (
                Failure::TooLarge,
                true,
                "winx-desktop's answer was too large to show.",
            ),
        ] {
            block_on(async {
                let (mut life, _inbox) = life_with_inbox("http://127.0.0.1:9");
                life.prepare().await.unwrap();
                life.begin();
                life.me = Some(MeView {
                    computer: "winx-desktop".into(),
                    ..Default::default()
                });
                life.phase = Phase::Live;
                let (job, answer) = answered("git", true);
                life.call(job);
                life.called(Lane::Slow, Err(failure), Duration::ZERO, sent);
                assert_eq!(answer.await.unwrap(), Err(Refusal::of(words)));
                assert_eq!(life.phase, Phase::Live);
            });
        }
    }

    #[test]
    fn try_now_forgets_the_verdicts_a_public_certificate_was_trusted_by() {
        use crate::client::trust::tests as certs;
        use tokio_rustls::rustls::client::danger::ServerCertVerifier;
        use tokio_rustls::rustls::pki_types::{ServerName, UnixTime};
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.prepare().await.unwrap();
            let a = certs::ca("a");
            let platform = certs::Platform::counting(certs::roots(&[&a]));
            let checked = Arc::new(trust::Checked::new(
                platform.clone(),
                certs::roots(&[&a]),
                false,
            ));
            life.ready.as_mut().unwrap().checked = Some(checked.clone());
            let leaf = certs::leaf(&a, "localhost", false);
            let (name, now) = (ServerName::try_from("localhost").unwrap(), UnixTime::now());
            let ask = || checked.verify_server_cert(&leaf.der, &[], &name, &[], now);
            assert!(ask().is_ok() && ask().is_ok());
            assert_eq!(platform.asked(), 1, "a fine chain is kept");
            // Cut, then Try now: the person may have taken the authority away since.
            life.cut(Why::Revoked);
            life.command(Command::TryNow).await;
            assert!(ask().is_ok());
            assert_eq!(platform.asked(), 2, "asked anew");
        });
    }

    #[test]
    fn a_snapshot_past_the_caps_makes_the_remote_try_again() {
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.out.inner().current.insert("r1".into(), 1);
            let big = crate::model::State {
                blocks: vec![Default::default(); 10_001],
                ..Default::default()
            };
            life.event(sse::Event {
                event: "snapshot".into(),
                data: serde_json::to_string(&big).unwrap(),
                id: None,
            });
            assert!(matches!(life.status, Some(Status::Reconnecting { .. })));
            assert_eq!(life.out.inner().states.len(), 0, "nothing of it was kept");
        });
    }

    // -- Typing --

    #[test]
    fn typing_calls_are_spaced_and_a_busy_one_is_made_again() {
        let seen: Arc<std::sync::Mutex<Vec<(Instant, String)>>> = Arc::default();
        let log = seen.clone();
        let port = server(move |mut stream| {
            use std::io::Write;
            while let Some((_, body)) = request(&mut stream) {
                let first = {
                    let mut log = log.lock().unwrap();
                    log.push((Instant::now(), body));
                    log.len() == 1
                };
                let answer = if first {
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}"
                };
                if stream.write_all(answer.as_bytes()).is_err() {
                    return;
                }
            }
        });
        let (tx, mut results) = unbounded_channel();
        let (batches, inbox) = unbounded_channel();
        let batch = |keys: &[&str]| Batch {
            block: "b".into(),
            args: json!({"block": "b", "keys": keys}),
        };
        block_on(async {
            let _task = Task::spawn(typing_task(
                net_at(port),
                1,
                inbox,
                tx,
                Duration::from_secs(5),
            ));
            batches.send(batch(&["a", "b"])).unwrap();
            batches.send(batch(&["c"])).unwrap();
            for _ in 0..2 {
                let Some(Msg::Typed { result, .. }) = results.recv().await else {
                    panic!("typed")
                };
                assert!(result.is_ok(), "{result:?}");
            }
        });
        let seen = seen.lock().unwrap();
        let bodies: Vec<Value> = seen
            .iter()
            .map(|(_, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(
            bodies,
            [
                json!({"block": "b", "keys": ["a", "b"]}),
                json!({"block": "b", "keys": ["a", "b"]}),
                json!({"block": "b", "keys": ["c"]}),
            ],
            "the busy call again, then the next"
        );
        for pair in seen.windows(2) {
            let gap = pair[1].0 - pair[0].0;
            assert!(gap >= PACE / 2, "{gap:?}");
        }
    }

    /// A typing task, patient for `patience`, whose home answers every call with 429 and
    /// `retry_after`; the answers it got and what the first batch came to.
    fn typing_against_busy_home(
        retry_after: &'static str,
        patience: Duration,
    ) -> (usize, Result<Value, Failure>) {
        let asked: Arc<std::sync::Mutex<usize>> = Arc::default();
        let counted = asked.clone();
        let port = server(move |mut stream| {
            use std::io::Write;
            while request(&mut stream).is_some() {
                *counted.lock().unwrap() += 1;
                let answer = format!(
                    "HTTP/1.1 429 Too Many Requests\r\nRetry-After: {retry_after}\r\nContent-Length: 0\r\n\r\n"
                );
                if stream.write_all(answer.as_bytes()).is_err() {
                    return;
                }
            }
        });
        let (tx, mut results) = unbounded_channel();
        let (batches, inbox) = unbounded_channel();
        let result = block_on(async {
            let _task = Task::spawn(typing_task(net_at(port), 1, inbox, tx, patience));
            batches
                .send(Batch {
                    block: "b".into(),
                    args: json!({"block": "b", "keys": ["a"]}),
                })
                .unwrap();
            let Some(Msg::Typed { result, .. }) = results.recv().await else {
                panic!("typed")
            };
            result
        });
        let asked = *asked.lock().unwrap();
        (asked, result)
    }

    #[test]
    fn typing_is_given_up_after_ten_busy_answers() {
        let (asked, result) = typing_against_busy_home("0", Duration::from_secs(5));
        assert!(matches!(result, Err(Failure::Busy(_))), "{result:?}");
        assert_eq!(asked, usize::try_from(BUSY_TRIES).unwrap());
    }

    #[test]
    fn typing_told_to_wait_long_drops_its_keys_instead_of_sending_them_late() {
        // 600 s is held to 30 s, past what typing waits: one answer, no second call, no sleep.
        let (asked, result) = typing_against_busy_home("600", Duration::from_secs(5));
        assert!(matches!(result, Err(Failure::Busy(_))), "{result:?}");
        assert_eq!(asked, 1);
        // A number that would overflow an `Instant` is held the same way, and does not panic.
        let (asked, result) =
            typing_against_busy_home("18446744073709551615", Duration::from_secs(5));
        assert!(matches!(result, Err(Failure::Busy(_))), "{result:?}");
        assert_eq!(asked, 1);
    }

    #[test]
    fn typing_waits_its_patience_in_all_over_the_busy_answers() {
        // Each answer is waited out for a `PACE` at least; 150 ms hold two such waits (60 + 60),
        // not a third: the call is made three times, then given up.
        let (asked, result) = typing_against_busy_home("0", Duration::from_millis(150));
        assert!(matches!(result, Err(Failure::Busy(_))), "{result:?}");
        assert_eq!(asked, 3);
    }

    #[test]
    fn a_typing_call_that_fails_drops_what_waited_and_says_so() {
        // Home takes the call and closes the connection without an answer.
        let port = server(|mut stream| {
            let _ = request(&mut stream);
        });
        let (tx, mut results) = unbounded_channel();
        let (batches, inbox) = unbounded_channel();
        let result = block_on(async {
            let _task = Task::spawn(typing_task(
                net_at(port),
                1,
                inbox,
                tx,
                Duration::from_secs(5),
            ));
            batches
                .send(Batch {
                    block: "b".into(),
                    args: json!({"block": "b", "keys": ["a"]}),
                })
                .unwrap();
            let Some(Msg::Typed { result, .. }) = results.recv().await else {
                panic!("typed")
            };
            result
        });
        assert!(matches!(result, Err(Failure::Down(_))), "{result:?}");

        // The coordinator keeps no key of it, and tells the person.
        let mut life = life_at("http://127.0.0.1:9");
        life.out.inner().current.insert("r1".into(), 1);
        life.queues.insert(
            "b".into(),
            Queue {
                items: VecDeque::from([Item::Key("b".into())]),
                keys: 1,
                ..Default::default()
            },
        );
        life.turns.push_back("b".into());
        // (A failure it didn't know would reconnect, which needs the runtime.)
        block_on(async { life.typed("b".into(), result, Duration::ZERO) });
        assert!(life.queues.is_empty() && life.turns.is_empty());
        let said = life.out.inner().updates.clone();
        assert!(
            said.iter()
                .any(|(_, u)| matches!(u, Update::Dropped(b) if b == "b")),
            "{said:?}"
        );
    }

    #[test]
    fn a_move_home_answers_busy_to_is_made_again() {
        let moves: Arc<std::sync::Mutex<usize>> = Arc::default();
        let counted = moves.clone();
        let me = serde_json::to_string(&MeView::default()).unwrap();
        let port = server(move |mut stream| {
            use std::io::Write;
            while let Some((head, _)) = request(&mut stream) {
                let answer = if head.starts_with("GET /v1/me ") {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{me}",
                        me.len()
                    )
                } else if head.starts_with("GET /v1/tools ") {
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n[]".to_string()
                } else if counted_move(&counted) {
                    "HTTP/1.1 503 Service Unavailable\r\nContent-Type: application/json\r\nRetry-After: 0\r\nContent-Length: 82\r\n\r\n{\"error\":{\"code\":\"busy\",\"message\":\"Too many connections: try again in a moment.\"}}".to_string()
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}".to_string()
                };
                if stream.write_all(answer.as_bytes()).is_err() {
                    return;
                }
            }
        });
        let (tx, mut results) = unbounded_channel();
        let (calls, inbox) = unbounded_channel();
        block_on(async {
            let _task = Task::spawn(calls_task(
                net_at(port),
                1,
                Lane::Quick,
                inbox,
                tx,
                Duration::from_secs(60),
            ));
            assert!(matches!(
                results.recv().await,
                Some(Msg::Me { result: Ok(_), .. })
            ));
            calls
                .send(Work {
                    tool: "move_block".into(),
                    args: json!({"block": "b"}),
                })
                .unwrap();
            let result = loop {
                match results.recv().await {
                    Some(Msg::Called { result, .. }) => break result,
                    Some(Msg::Tools { .. }) => {}
                    _ => panic!("called"),
                }
            };
            assert!(result.is_ok(), "{result:?}");
        });
        assert_eq!(*moves.lock().unwrap(), 2, "once refused, once made");
    }

    // -- The event stream --

    #[test]
    fn only_a_clean_end_of_the_stream_is_home_ending_it() {
        let ends = |tail: &'static str| {
            let port = server(move |mut stream| {
                use std::io::Write;
                if request(&mut stream).is_some() {
                    let _ = stream.write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{tail}"
                        )
                        .as_bytes(),
                    );
                }
            });
            let (tx, mut inbox) = unbounded_channel();
            block_on(async {
                let _task = Task::spawn(events_task(
                    net_at(port),
                    1,
                    None,
                    tx,
                    Duration::from_secs(5),
                ));
                loop {
                    match inbox.recv().await {
                        Some(Msg::Lost { quiet, .. }) => return quiet,
                        Some(_) => {}
                        None => panic!("no end"),
                    }
                }
            })
        };
        assert!(ends("0\r\n\r\n"), "a stream that finished");
        assert!(
            ends("5\r\nhello"),
            "a connection home closed before the body ended"
        );
        assert!(!ends("zz\r\n"), "bytes that aren't HTTP");
    }

    #[test]
    fn a_home_that_floods_events_is_held_by_its_socket_not_kept_in_memory() {
        use std::io::Write;
        // Far more than a socket holds, so a mirror that reads everything is seen reading it.
        const MOST: usize = 32 << 20;
        let (done, written) = std::sync::mpsc::channel();
        let port = server(move |mut stream| {
            if request(&mut stream).is_none() {
                return;
            }
            // A write that stays blocked this long is a client that stopped reading.
            let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
            let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n";
            let events = "event: block\ndata: {\"op\":\"delete\",\"id\":\"x\"}\n\n".repeat(300);
            let chunk = format!("{:x}\r\n{events}\r\n", events.len());
            let mut sent = 0;
            if stream.write_all(head.as_bytes()).is_ok() {
                while sent < MOST && stream.write_all(chunk.as_bytes()).is_ok() {
                    sent += chunk.len();
                }
            }
            let _ = done.send(sent);
        });
        let (tx, mut inbox) = unbounded_channel();
        let sent = block_on(async {
            let _task = Task::spawn(events_task(
                net_at(port),
                1,
                None,
                tx,
                Duration::from_secs(30),
            ));
            // Nothing takes the events: the coordinator is as slow as can be. A deadline, not a
            // fixed wait: the server is done when its writes block.
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                if let Ok(sent) = written.try_recv() {
                    break sent;
                }
                assert!(
                    Instant::now() < deadline,
                    "the server's writes never blocked"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        assert!(
            sent < MOST / 2,
            "the server wrote {sent} bytes before it was held"
        );
        let mut waiting = 0;
        while let Ok(msg) = inbox.try_recv() {
            waiting += usize::from(matches!(msg, Msg::Event { .. }));
        }
        assert!(waiting <= EVENTS_WAITING_MOST, "{waiting} events wait");
    }

    // -- Terminal streams --

    #[test]
    fn a_stream_follows_the_viewer_of_its_block() {
        block_on(async {
            let mut life = life_at("http://127.0.0.1:9");
            life.prepare().await.unwrap();
            life.begin();
            life.phase = Phase::Live;
            let (first, second) = (viewer(), viewer());
            let on = |life: &Life, viewer: &templum_terminal::Terminal| {
                life.streams["b7k2m"].0.same_viewer(&viewer.feeder())
            };
            life.command(Command::Terminals(vec![("b7k2m".into(), first.feeder())]))
                .await;
            assert!(on(&life, &first));
            // The same viewer again (another order, a new handle): the stream goes on.
            life.command(Command::Terminals(vec![("b7k2m".into(), first.feeder())]))
                .await;
            assert!(on(&life, &first) && life.streams.len() == 1);
            // Another viewer for the block: a stream of its own, not the old one feeding the old.
            life.command(Command::Terminals(vec![("b7k2m".into(), second.feeder())]))
                .await;
            assert!(on(&life, &second) && !on(&life, &first));
        });
    }

    #[test]
    fn a_terminal_stream_told_busy_waits_as_long_as_home_says() {
        let (tx, mut asked) = unbounded_channel();
        let port = server(move |mut stream| {
            use std::io::Write;
            if request(&mut stream).is_some() {
                let _ = tx.send(Instant::now());
                let _ = stream.write_all(
                    b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 2\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        let (tx, _inbox) = unbounded_channel();
        let gap = block_on(async {
            let _task = Task::spawn(terminal_task(
                net_at(port),
                1,
                "b7k2m".into(),
                viewer().feeder(),
                tx,
                Duration::from_secs(5),
            ));
            let first = asked.recv().await.unwrap();
            let second = tokio::time::timeout(Duration::from_secs(10), asked.recv())
                .await
                .expect("it opens again")
                .unwrap();
            second - first
        });
        assert!(
            gap >= Duration::from_millis(1900),
            "not before Retry-After: {gap:?}"
        );
        assert!(gap < Duration::from_secs(5), "{gap:?}");
    }
}
