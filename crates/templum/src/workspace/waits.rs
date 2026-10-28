//! Tool calls that wait (docs/specs/connections.md, Architecture): an agent's question for the
//! person. Each keeps its reply channel in `Workspace::waits` until it is answered, its deadline
//! passes, its call is cancelled or its connection ends, or its block closes.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::InputState;
use gpui_kit::{App, Context, Entity, Focusable as _, Subscription, Window};
use serde_json::Value;
use templum_api::store::Scope;
use templum_canvas::BlockId;
use templum_mcp::ask::{Ask, Form, Row};
use templum_mcp::tools::Event;
use templum_mcp::{Replier, Reply, tools};
use templum_terminal::Pattern;

use super::bus::{Outcome, target};
use super::{AgentState, Pane, Workspace};
use crate::alert::Alert;

/// How long after the dialog first shows a question its keys and clicks are ignored: someone
/// typing or clicking in the terminal as it appears doesn't answer it.
pub(crate) const TOO_SOON: Duration = Duration::from_millis(500);
/// The lines of an agent's screen a turn's wait that times out gives.
pub(super) const LAST_LINES: usize = 40;
/// The lines of its terminal a `wait_for` with a pattern gives after the line that matched.
pub(super) const PRINTED_LINES: usize = 20;
/// How often a `wait_for` a command or an alert looks at its terminal's counters.
pub(super) const EVENT_LOOK: Duration = Duration::from_millis(250);
/// How long after a hook says its agent waits for its person its screen is read: the hook comes
/// before its dialog is drawn (it may answer the dialog; Codex's drew 25 ms after).
const DRAWN: Duration = Duration::from_millis(300);

/// A tool call waiting for its answer.
pub(crate) struct Wait {
    /// Names it for its deadline and the inbox's buttons; counts up, so the oldest is first.
    pub key: u64,
    /// The connection it came on, and its JSON-RPC id: a cancel names both.
    pub conn: u64,
    pub id: Value,
    reply: Replier,
    /// The terminal that asked (its `TEMPLUM_TERMINAL`): its block keeps it when it moves to
    /// another workspace.
    pub from: String,
    pub what: Waiting,
    /// When it was made, by the executor's clock: the inbox says how long it has waited.
    pub since: Instant,
    /// When the inbox first drew it, by the executor's clock.
    pub shown: Cell<Option<Instant>>,
    /// Its options' previews are shown whole (Show more), not cut after a few lines.
    pub more: Cell<bool>,
    /// The scroll of the request's body in the inbox.
    pub body: crate::markdown::PreviewScroll,
    /// The row the keys just moved the highlight to: the body scrolls to show it once, when it is
    /// drawn (a wheel that scrolled it away since isn't fought).
    pub reveal: Rc<Cell<Option<Row>>>,
    /// A request of an agent on another computer, shown here from a mirrored workspace: it is
    /// answered there, never here.
    pub remote: Option<RemoteAsk>,
}

/// Where a mirrored request is answered.
pub(crate) struct RemoteAsk {
    /// The remote workspace's id here.
    pub space: String,
    /// Home's id of the request (`r<n>`, unique across home's restarts: see `next_wait`).
    pub request: String,
    /// What answering it takes there.
    pub scope: Scope,
    /// The answer was sent: it leaves the inbox when home's state no longer has it.
    pub sent: bool,
    /// No notification for it: it was told already, or folded into a notice of several
    /// (`remote_tells`).
    pub quiet: bool,
}

/// A field of a question's dialog, and what it hears from it.
pub(crate) type Field = (Entity<InputState>, Subscription);

/// What a wait waits for.
pub(crate) enum Waiting {
    /// `ask_user` (docs/specs/ask-user.md): its questions, what the person picked, each
    /// question's Other field, and the note (asked with `questions` only). `wait` makes the
    /// fields. `stopped`: its client gave up on the call; the answer is typed into its terminal.
    Question {
        ask: Ask,
        form: Form,
        others: Vec<Field>,
        note: Option<Field>,
        stopped: bool,
        /// A plan's Markdown frame's scroll (`propose_plan`).
        scroll: crate::markdown::PreviewScroll,
    },
    /// The person's yes or no to what an agent asks to do (docs/specs/agent-tools.md, Asking the
    /// person): `then`, done once they said yes.
    Allow { then: Approved },
    /// `delegate` with `wait`, `wait_for_idle`: the end of the turn of the agent in the terminal
    /// `of` (the block `uid`).
    Turn { of: String, uid: String },
    /// `wait_for`: `event` in the terminal `of` (the block `uid`). `since`: the commands that had
    /// finished there and the alerts it had sent when the call came (`Status.finished`,
    /// `Status.attention`). `watch`: for `Event::Output`, the id of the watch set on the terminal
    /// and its pattern's text, unwatched when the wait ends however it ends.
    Event {
        of: String,
        uid: String,
        event: Event,
        since: (u64, u64),
        watch: Option<(u64, String)>,
    },
}

/// What a request with two buttons does once the person says yes.
pub(crate) enum Approved {
    /// `connect`: wires the asking block to the block `to` (its block id). Allow, Deny.
    Connect { to: String },
    /// `remove_worktree` with `discard`: removes the worktree `name` of `repo`, losing its
    /// `changed` files, `unmerged` commits and the files git ignores there, `ignored`. Delete, Keep.
    RemoveWorktree {
        repo: templum_git::Repo,
        name: String,
        changed: usize,
        unmerged: u32,
        ignored: Vec<String>,
    },
    /// `merge_worktree`: merges the worktree `name`'s `unmerged` commits into `into`, the branch
    /// of `repo`'s working tree. Merge, Cancel.
    MergeWorktree {
        repo: templum_git::Repo,
        name: String,
        unmerged: u32,
        into: String,
    },
    /// A request of another computer's agent, answered there: `line` says what it asks, `yes` and
    /// `no` are its buttons' words, home's.
    Remote {
        line: String,
        yes: String,
        no: String,
    },
}

impl Approved {
    /// Its two buttons' words: yes, then no.
    pub(crate) fn buttons(&self) -> (String, String) {
        let (yes, no) = match self {
            Approved::Connect { .. } => ("Allow", "Deny"),
            Approved::RemoveWorktree { .. } => ("Delete", "Keep"),
            Approved::MergeWorktree { .. } => ("Merge", "Cancel"),
            Approved::Remote { yes, no, .. } => (yes.as_str(), no.as_str()),
        };
        (yes.into(), no.into())
    }
}

/// A tool call that waits, as `serve` hands it over: where its answer goes.
pub(super) struct Asked {
    pub conn: u64,
    pub id: Value,
    pub reply: Replier,
    pub from: String,
}

impl Wait {
    /// Not drawn yet, or drawn under TOO_SOON ago: a press on it is ignored.
    pub fn too_soon(&self, cx: &App) -> bool {
        self.shown
            .get()
            .is_none_or(|at| cx.background_executor().now().duration_since(at) < TOO_SOON)
    }

    /// What it asks the person, on one line: the dot's tooltip.
    pub fn summary(&self) -> String {
        match &self.what {
            Waiting::Question { ask, .. } => ask.summary(),
            Waiting::Allow {
                then: Approved::Connect { to },
            } => format!("connect to {to}"),
            Waiting::Allow {
                then: Approved::RemoveWorktree { name, .. },
            } => format!("delete the worktree {name}"),
            Waiting::Allow {
                then: Approved::MergeWorktree { name, .. },
            } => format!("merge {name}"),
            Waiting::Allow {
                then: Approved::Remote { line, .. },
            } => line.clone(),
            Waiting::Turn { .. } | Waiting::Event { .. } => String::new(),
        }
    }
}

impl Workspace {
    /// Keeps `asked` until what it waits for comes, or `seconds` pass (None: no deadline); a
    /// question gets its fields. The person hears of a request when its block isn't on screen.
    pub(super) fn wait(
        &mut self,
        asked: Asked,
        what: Waiting,
        seconds: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.wait_as(asked, what, seconds, None, window, cx);
    }

    /// `wait`, for a request that may be another computer's (`remote`): it is shown here and
    /// answered there.
    pub(super) fn wait_as(
        &mut self,
        asked: Asked,
        mut what: Waiting,
        seconds: Option<u64>,
        remote: Option<RemoteAsk>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.next_wait += 1;
        let key = self.next_wait;
        if let Waiting::Question {
            ask, others, note, ..
        } = &mut what
        {
            (*others, *note) = super::questions::fields(key, ask, window, cx);
        }
        // ponytail: one timer per wait, left to run out once it is answered (it then finds
        // nothing); a call waits an hour at most, a question a day.
        if let Some(seconds) = seconds {
            cx.spawn(async move |ws, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(seconds))
                    .await;
                ws.update(cx, |ws, cx| ws.expire(key, seconds, cx)).ok();
            })
            .detach();
        }
        let wait = Wait {
            key,
            conn: asked.conn,
            id: asked.id,
            reply: asked.reply,
            from: asked.from,
            what,
            since: cx.background_executor().now(),
            shown: Cell::new(None),
            more: Cell::new(false),
            body: Default::default(),
            reveal: Rc::default(),
            remote,
        };
        let looks = matches!(
            wait.what,
            Waiting::Event {
                event: Event::CommandFinished | Event::Output | Event::Attention,
                ..
            }
        );
        if matches!(wait.what, Waiting::Question { .. } | Waiting::Allow { .. }) {
            if self
                .request_space(&wait)
                .is_some_and(|space| space == self.space.id)
            {
                self.inbox_arrived(cx);
            }
            self.tell_asking(&wait, window, cx);
            self.chime(cx);
            // Its block waits for its person now. The agent that handed it a task hears nothing:
            // the person sees the request in the inbox, and that agent can't answer it. (Another
            // computer's agent is not one of this one's to tell.)
            if wait.remote.is_none() {
                self.needs_you(&wait.from, Some(&wait.summary()), false, cx);
            }
        }
        self.waits.push(wait);
        if looks {
            self.look_until_it_comes(key, cx);
        }
        cx.notify();
    }

    /// Answers the wait `key`, once: it is gone after. False when it already was. A question
    /// whose call was cancelled goes without an answer: MCP sends none to a cancelled call.
    pub(super) fn answer(&mut self, key: u64, reply: Reply, cx: &mut Context<Self>) -> bool {
        let Some(i) = self.waits.iter().position(|w| w.key == key) else {
            return false;
        };
        let wait = self.waits.remove(i);
        self.inbox_settle(&[key]);
        self.request_ended(&wait, cx);
        self.unwatch(&wait);
        if !matches!(wait.what, Waiting::Question { stopped: true, .. }) {
            let _ = wait.reply.send(tools::tool_result(&wait.id, &reply));
        }
        cx.notify();
        true
    }

    /// The deadline of the wait `key`: withdrawn, and the agent told so.
    fn expire(&mut self, key: u64, seconds: u64, cx: &mut Context<Self>) {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return;
        };
        let screen = |of: &str| {
            let lines = self
                .pane(of)
                .map(|pane| pane.term.shared().lines(LAST_LINES).join("\n"))
                .unwrap_or_default();
            tools::tail(&lines, tools::MAX_TEXT).to_string()
        };
        let reply = match &wait.what {
            Waiting::Question { .. } | Waiting::Allow { .. } => Reply::err(format!(
                "No one answered within {seconds} seconds; the request was withdrawn."
            )),
            Waiting::Turn { of, uid } => Reply::err(format!(
                "No answer from {uid} within {seconds} seconds; it may still be working \
                 (wait_for_idle(\"{uid}\") waits again). The last lines of its screen:\n{}",
                screen(of)
            )),
            Waiting::Event {
                of,
                uid,
                watch: Some((_, pattern)),
                ..
            } => Reply::err(format!(
                "No line matching \"{pattern}\" in {uid} within {seconds} seconds. Its last lines:\n{}",
                screen(of)
            )),
            Waiting::Event { of, uid, event, .. } => Reply::err(format!(
                "No {} in {uid} within {seconds} seconds. The last lines of its screen:\n{}",
                awaited(*event),
                screen(of)
            )),
        };
        self.answer(key, reply, cx);
    }

    /// MCP's `notifications/cancelled` for the call `id` of connection `conn`: dropped, with no
    /// answer (the client wants none). A question stays, its answer to be typed into its
    /// terminal (docs/specs/ask-user.md, When the client stops waiting).
    pub(super) fn cancel(&mut self, conn: u64, id: &Value, cx: &mut Context<Self>) {
        let gone: Vec<Wait> = self
            .waits
            .extract_if(.., |w| {
                if w.conn != conn || w.id != *id {
                    return false;
                }
                match &mut w.what {
                    Waiting::Question { stopped, .. } => {
                        *stopped = true;
                        false
                    }
                    _ => true,
                }
            })
            .collect();
        for wait in &gone {
            self.unwatch(wait);
            self.request_ended(wait, cx);
        }
        self.inbox_settle(&gone.iter().map(|w| w.key).collect::<Vec<_>>());
        cx.notify();
    }

    /// A call from the terminal `from` waits for the turn of the agent in the terminal `of`.
    pub(super) fn waits_on(&self, from: &str, of: &str) -> bool {
        self.waits
            .iter()
            .any(|w| w.from == from && matches!(&w.what, Waiting::Turn { of: o, .. } if o == of))
    }

    /// The connection `conn` ended (its agent quit, its command exited): what it waited for has
    /// no one to answer.
    pub(super) fn closed(&mut self, conn: u64, cx: &mut Context<Self>) {
        self.tool_conns.remove(&conn);
        let gone: Vec<Wait> = self.waits.extract_if(.., |w| w.conn == conn).collect();
        for wait in &gone {
            self.unwatch(wait);
            self.request_ended(wait, cx);
        }
        self.inbox_settle(&gone.iter().map(|w| w.key).collect::<Vec<_>>());
        cx.notify();
    }

    /// A wait ends, however it ends: the watch it set on its terminal goes, or the terminal's
    /// reader thread would keep its lines for good.
    fn unwatch(&self, wait: &Wait) {
        if let Waiting::Event {
            of,
            watch: Some((id, _)),
            ..
        } = &wait.what
            && let Some(pane) = self.pane(of)
        {
            pane.term.unwatch(*id);
        }
    }

    /// Each frame: a wait whose block is gone (closed, its shell exited, its workspace stopped)
    /// is withdrawn. A block moved to another workspace keeps its terminal, and its waits.
    pub(super) fn drop_orphans(&mut self, cx: &mut Context<Self>) {
        let orphans: Vec<(u64, Reply)> = self
            .waits
            .iter()
            .filter_map(|w| Some((w.key, self.orphaned(w)?)))
            .collect();
        for (key, reply) in orphans {
            self.answer(key, reply, cx);
        }
    }

    /// Why `wait` can't be answered any more, if so.
    fn orphaned(&self, wait: &Wait) -> Option<Reply> {
        // Another computer's request stays while its workspace is here and home lists it, with a
        // viewer of its block or without.
        if wait.remote.is_some() {
            return self
                .request_space(wait)
                .is_none()
                .then(|| Reply::err("Its workspace was forgotten; the request was withdrawn."));
        }
        // The Templum Agent and the API ask from no block.
        if wait.from != templum_agent::ASKER
            && wait.from != templum_api::ASKER
            && self.asker(wait).is_none()
        {
            return Some(Reply::err(
                "Your block was closed; the request was withdrawn.",
            ));
        }
        match &wait.what {
            Waiting::Allow {
                then: Approved::Connect { to },
            } if !self.uid_taken(to) => Some(Reply::err(format!(
                "{to} was closed; the request was withdrawn."
            ))),
            Waiting::Turn { of, uid } if self.pane(of).is_none() => {
                Some(Reply::err(format!("{uid} was closed before it answered.")))
            }
            Waiting::Event { of, uid, .. } if self.pane(of).is_none() => {
                Some(Reply::err(format!("{uid} was closed while you waited.")))
            }
            Waiting::Question { .. }
            | Waiting::Allow { .. }
            | Waiting::Turn { .. }
            | Waiting::Event { .. } => None,
        }
    }

    /// `connect` from the terminal `from`: the person's answer to wait for, unless there is
    /// nothing to ask.
    pub(super) fn ask_to_connect(&self, from: &str, block: &str, timeout: u64) -> Outcome {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let Some(to) = space.block_by_uid(block) else {
            return Outcome::Now(Reply::err(format!(
                "There's no block {block} in your workspace: list_blocks lists them."
            )));
        };
        if to == me {
            return Outcome::Now(Reply::err(format!(
                "{block} is your own block: you may use it already."
            )));
        }
        if space.wired(me, to) {
            return Outcome::Now(Reply::ok(format!("{block} is already connected to you.")));
        }
        let asked = self
            .waits
            .iter()
            .any(|w| {
                w.from == from
                    && matches!(&w.what, Waiting::Allow { then: Approved::Connect { to } } if to == block)
            });
        if asked {
            return Outcome::Now(Reply::err(format!(
                "You already asked to connect to {block}; the person hasn't answered yet."
            )));
        }
        Outcome::Wait(
            Box::new(Waiting::Allow {
                then: Approved::Connect {
                    to: block.to_string(),
                },
            }),
            Some(timeout),
        )
    }

    /// The person's yes (`yes`) or no to the request `key`: Allow wires the two blocks, a step
    /// Ctrl+Z takes back, as wiring by hand is; Delete and Merge run on the background executor,
    /// the request gone from the inbox at once, and answer once git is done.
    pub(crate) fn answer_allow(&mut self, key: u64, yes: bool, cx: &mut Context<Self>) {
        let Some(at) = self.waits.iter().position(|w| w.key == key) else {
            return;
        };
        if self.waits[at].remote.is_some() {
            self.remote_answer(key, serde_json::json!({ "allow": yes }), cx);
            return;
        }
        let Waiting::Allow { then } = &self.waits[at].what else {
            return;
        };
        let from = self.waits[at].from.clone();
        let job = match (yes, then) {
            (false, Approved::Connect { to }) => Err(Reply::err(format!(
                "The person didn't connect you to {to}."
            ))),
            (false, _) => Err(Reply::err("The person said no.")),
            (true, Approved::Connect { to }) => {
                let to = to.clone();
                Err(self.connect_for(&from, &to, cx))
            }
            (true, then) => match self.worktree_still(&from, then) {
                Err(refused) => Err(refused),
                Ok(()) => match then {
                    Approved::RemoveWorktree { repo, name, .. } => {
                        Ok(self.worktree_remove(repo.clone(), name.clone(), true, cx))
                    }
                    Approved::MergeWorktree {
                        repo,
                        name,
                        unmerged,
                        into,
                    } => Ok(self.worktree_merge(
                        repo.clone(),
                        name.clone(),
                        into.clone(),
                        *unmerged,
                        cx,
                    )),
                    // Answered by the arms above, and by `remote_answer`.
                    Approved::Connect { .. } | Approved::Remote { .. } => {
                        Err(Reply::err("Connected."))
                    }
                },
            },
        };
        match job {
            Err(reply) => {
                self.answer(key, reply, cx);
                self.seen_asking(&from);
            }
            Ok(job) => {
                let Some(at) = self.waits.iter().position(|w| w.key == key) else {
                    return;
                };
                let wait = self.waits.remove(at);
                self.inbox_settle(&[key]);
                self.request_ended(&wait, cx);
                self.seen_asking(&from);
                cx.notify();
                cx.spawn(async move |_, _| {
                    let reply = job.await;
                    let _ = wait.reply.send(tools::tool_result(&wait.id, &reply));
                })
                .detach();
            }
        }
    }

    /// `connect`'s Allow: the terminal `from`'s block wired to the block `to` of its workspace.
    fn connect_for(&mut self, from: &str, to: &str, cx: &mut Context<Self>) -> Reply {
        let Some((space_id, me)) = self.caller(from) else {
            return Reply::err("Your block was closed; the request was withdrawn.");
        };
        let other = self
            .space_named(&space_id)
            .and_then(|space| space.block_by_uid(to));
        match other {
            Some(other) => {
                self.in_space(&space_id, |ws| ws.connect(me, other, cx));
                Reply::ok(format!("Connected to {to}."))
            }
            None => Reply::err(format!("{to} isn't in your workspace any more.")),
        }
    }

    /// The person answered a request of the terminal `from`: they saw its block, whose alert
    /// (`tell_asking`'s, or any other) goes, as seeing a block clears its alert; but not while
    /// another request of it waits, whose words the alert keeps (`request_ended`).
    pub(super) fn seen_asking(&mut self, from: &str) {
        if self.asking(from).is_none()
            && let Some(pane) = self.pane_mut(from)
        {
            pane.alert = None;
        }
    }

    /// "*Claude Code* wants to connect to *Dev server*": a request as the person reads it, the
    /// blocks by their titles (docs/specs/agent-tools.md, Asking the person).
    pub(crate) fn allow_line(&self, wait: &Wait, cx: &App) -> String {
        let Waiting::Allow { then } = &wait.what else {
            return String::new();
        };
        if let Approved::Remote { line, .. } = then {
            return line.clone();
        }
        let Some((space_id, me)) = self.caller(&wait.from) else {
            return String::new();
        };
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let title = |id| self.about(&space.blocks[&id], cx).1;
        let from = title(me);
        match then {
            Approved::Connect { to } => {
                let target = space.block_by_uid(to).map_or_else(|| to.clone(), title);
                format!("{from} wants to connect to {target}")
            }
            Approved::RemoveWorktree {
                name,
                changed,
                unmerged,
                ignored,
                ..
            } => {
                let ignored = match ignored.as_slice() {
                    [] => String::new(),
                    files => format!(
                        ", and files git ignores: {}",
                        super::worktrees::listed(files)
                    ),
                };
                format!(
                    "{from} wants to delete the worktree {name}, with {changed} changed files and \
                     {unmerged} commits not merged{ignored}."
                )
            }
            Approved::MergeWorktree {
                name,
                unmerged,
                into,
                ..
            } => format!("{from} wants to merge {name} ({unmerged} commits) into {into}."),
            Approved::Remote { line, .. } => line.clone(),
        }
    }

    /// The workspace and block a request comes from: for another computer's, the viewer of its
    /// block (a viewer is no caller of this computer's tools); else the terminal that called.
    pub(crate) fn asker(&self, wait: &Wait) -> Option<(String, BlockId)> {
        if wait.remote.is_none() {
            return self.caller(&wait.from);
        }
        self.spaces().filter(|s| s.is_remote()).find_map(|space| {
            space
                .blocks
                .iter()
                .find(|(_, body)| body.terminal().is_some_and(|p| p.term.id() == wait.from))
                .map(|(id, _)| (space.id.clone(), *id))
        })
    }

    /// The workspace a request is listed in: for another computer's, the viewer of its block's,
    /// else the remote workspace it came from while it is here; else the terminal's that called.
    pub(crate) fn request_space(&self, wait: &Wait) -> Option<String> {
        match &wait.remote {
            Some(ask) => self
                .asker(wait)
                .map(|(space, _)| space)
                .or_else(|| self.space_named(&ask.space).map(|s| s.id.clone())),
            None => self.asker(wait).map(|(space, _)| space),
        }
    }

    /// What the agent in terminal `terminal` asks the person, oldest first: the first, and how
    /// many. Its block shows a notice, and the inbox lists them.
    pub(crate) fn asking(&self, terminal: &str) -> Option<(&Wait, usize)> {
        let mut asking = self.waits.iter().filter(|w| {
            w.from == terminal && matches!(w.what, Waiting::Question { .. } | Waiting::Allow { .. })
        });
        let first = asking.next()?;
        Some((first, 1 + asking.count()))
    }

    /// A request's short sound, unless `[connections] sound` is off or one played under
    /// `chime::GAP` ago.
    pub(super) fn chime(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let recent = self
            .last_chime
            .is_some_and(|at| now.duration_since(at) < crate::chime::GAP);
        if self.config.connections.sound && !recent {
            self.last_chime = Some(now);
            crate::chime::play();
        }
    }

    /// The person doesn't see the request: it is in another workspace than the one shown, or the
    /// window is in the background. A notification says what it asks, clicking it shows that
    /// workspace with its inbox open at the request. The block gets an alert in every case, as a
    /// bell gives one (its workspace's dot, `go_to_waiting`), so a request missed here is found
    /// from another workspace; a request here, the window in front, is told by the inbox's
    /// badge alone.
    fn tell_asking(&mut self, wait: &Wait, window: &mut Window, cx: &mut Context<Self>) {
        let Some(space_id) = self.request_space(wait) else {
            return;
        };
        let me = self.asker(wait).map(|(_, me)| me);
        let space = self.space_named(&space_id).expect("the caller's workspace");
        // Another computer's request is titled by where it is: "API on winx-desktop".
        let title = match (
            space.remote.as_deref().filter(|_| wait.remote.is_some()),
            me,
        ) {
            (Some(remote), _) => {
                format!("{} on {}", space.name, crate::mirror::computer_of(remote))
            }
            (None, Some(me)) => self.about(&space.blocks[&me], cx).1,
            (None, None) => return,
        };
        let Some(text) = self.asking_text(wait, cx) else {
            return;
        };
        let quiet = wait.remote.as_ref().is_some_and(|a| a.quiet);
        #[cfg(test)]
        if wait.remote.is_some() && !quiet {
            self.said.push(format!("{title}: {text}"));
        }
        if let Some(pane) = self.pane_mut(&wait.from) {
            pane.alert = Some(Alert::Attention(Some(text.clone())));
        }
        // A request here, the window in front, is told by the badge: no notification.
        if quiet || window.is_window_active() && space_id == self.space.id {
            return;
        }
        self.notify_from(space_id, me, title, text, Some(wait.key), window, cx);
    }

    /// One notification for `count` requests of another computer's that were not told one each
    /// (the last of them, `key`, is what it opens).
    pub(super) fn tell_folded(
        &mut self,
        key: u64,
        count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return;
        };
        let Some(space_id) = self.request_space(wait) else {
            return;
        };
        let me = self.asker(wait).map(|(_, me)| me);
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let Some(remote) = space.remote.as_deref() else {
            return;
        };
        let title = format!("{} on {}", space.name, crate::mirror::computer_of(remote));
        let text = match count {
            1 => "1 more request asks you".to_string(),
            _ => format!("{count} more requests ask you"),
        };
        #[cfg(test)]
        self.said.push(format!("{title}: {text}"));
        if window.is_window_active() && space_id == self.space.id {
            return;
        }
        self.notify_from(space_id, me, title, text, Some(key), window, cx);
    }

    /// What the request `wait` says to the person, as a notification and the block's alert: None
    /// for what is no request of the person's.
    fn asking_text(&self, wait: &Wait, cx: &App) -> Option<String> {
        Some(match &wait.what {
            Waiting::Question { ask, .. } if ask.plan.is_some() => {
                format!("Asks you to approve a plan: {}", ask.summary())
            }
            Waiting::Question { ask, .. } => match ask.questions.len() - 1 {
                0 => format!("Asks you: {}", ask.summary()),
                more => format!("Asks you: {} (and {more} more)", ask.summary()),
            },
            Waiting::Allow { .. } => self.allow_line(wait, cx),
            Waiting::Turn { .. } | Waiting::Event { .. } => return None,
        })
    }

    /// The request `ended` of the terminal `from` is gone: its block's alert follows what still
    /// waits there. The oldest request left gives its words; with none left, the alert goes if it
    /// was the request's (a bell's or a command's stays).
    fn request_ended(&mut self, ended: &Wait, cx: &App) {
        let Some(gone) = self.asking_text(ended, cx) else {
            return;
        };
        let left = self
            .asking(&ended.from)
            .and_then(|(wait, _)| self.asking_text(wait, cx));
        if let Some(pane) = self.pane_mut(&ended.from) {
            match left {
                Some(text) => pane.alert = Some(Alert::Attention(Some(text))),
                None if pane.alert == Some(Alert::Attention(Some(gone))) => pane.alert = None,
                None => {}
            }
        }
    }

    /// `wait_for` from the terminal `from`: `event` in the terminal `block`. `idle` is
    /// `wait_for_idle`; `needs_you` answers at once when its agent, or its request, already asks
    /// its person; `output` waits for a line matching `pattern`, at once when one of its last
    /// `lines` does, or with `wait` false keeps watching while the caller works, its match typed
    /// back; the others wait for the next command or alert.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn wait_for(
        &mut self,
        from: &str,
        block: &str,
        event: Event,
        timeout: u64,
        pattern: Option<String>,
        lines: usize,
        wait: bool,
        cx: &mut Context<Self>,
    ) -> Outcome {
        if event == Event::Idle {
            return self.wait_for_idle(from, block, timeout);
        }
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return Outcome::Now(refused),
        };
        if to == me {
            return Outcome::Now(Reply::err("This block is yours: wait for another block."));
        }
        let at = (space_id.as_str(), to);
        self.event_wait(from, at, block, event, timeout, pattern, lines, wait, cx)
    }

    /// The body of `wait_for` but `idle`, for any asker `by` (a terminal's `TEMPLUM_TERMINAL`, or
    /// the Templum Agent's word): `event` in the terminal block `to` of `space`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn event_wait(
        &mut self,
        by: &str,
        (space_id, to): (&str, BlockId),
        block: &str,
        event: Event,
        timeout: u64,
        pattern: Option<String>,
        lines: usize,
        wait: bool,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let Some(pane) = self.space_named(space_id).and_then(|s| s.terminal(to)) else {
            return Outcome::Now(Reply::err(format!("{block} isn't a terminal.")));
        };
        let of = pane.term.id().to_string();
        if event == Event::NeedsYou {
            let asks = match &pane.state {
                Some(AgentState::NeedsYou(what)) => Some(what.clone()),
                _ => self.asking(&of).map(|(wait, _)| Some(wait.summary())),
            };
            if let Some(what) = asks {
                return Outcome::Now(waiting_for_person(block, what.as_deref(), &pane.tail()));
            }
        }
        let watch = match pattern.filter(|_| event == Event::Output) {
            Some(text) => {
                let pattern = match Pattern::new(&text) {
                    Ok(pattern) => pattern,
                    Err(refused) => return Outcome::Now(Reply::err(refused)),
                };
                // Set before looking back: a line printed meanwhile is seen.
                let id = self.watch_add(&of, pattern.clone()).expect("its pane");
                let back = pane.term.shared().lines(lines.min(tools::MAX_LINES));
                if let Some(line) = back
                    .iter()
                    .filter(|_| lines > 0)
                    .find(|l| pattern.is_match(l))
                {
                    pane.term.unwatch(id);
                    return Outcome::Now(printed(block, &text, line, pane));
                }
                Some((id, text))
            }
            None => None,
        };
        if let Some((watch, pattern)) = watch.clone().filter(|_| !wait) {
            let until = cx.background_executor().now() + Duration::from_secs(timeout);
            let watch = super::schedules::Watch {
                id: 0,
                by: by.to_string(),
                of,
                uid: block.to_string(),
                pattern: pattern.clone(),
                watch,
                timeout,
                until,
            };
            let id = self.watch_keep(watch, cx);
            let back = if by == templum_agent::ASKER {
                "comes back into this chat"
            } else if by == templum_api::ASKER {
                "comes back as a notice event"
            } else {
                "is typed into your terminal"
            };
            return Outcome::Now(Reply::ok(format!(
                "Watching {block} for \"{pattern}\" as w{id}: a match {back}."
            )));
        }
        let status = pane.term.status();
        let what = Waiting::Event {
            of,
            uid: block.to_string(),
            event,
            since: (status.finished, status.attention),
            watch,
        };
        Outcome::Wait(Box::new(what), Some(timeout))
    }

    /// The block of the terminal `terminal` waits for its person (its agent asks for `what`, or
    /// its request does): the `wait_for(needs_you)` on it are answered DRAWN later, with its
    /// screen as drawn by then. `tell`: a new wait, which the agent that handed it a task hears of,
    /// unless a `wait_for(needs_you)` of its was just answered with it.
    pub(super) fn needs_you(
        &mut self,
        terminal: &str,
        what: Option<&str>,
        tell: bool,
        cx: &mut Context<Self>,
    ) {
        let terminal = terminal.to_string();
        let what = what.map(str::to_string);
        cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(DRAWN).await;
            let _ = ws.update(cx, |ws, cx| {
                let screen = ws.pane(&terminal).map(Pane::tail).unwrap_or_default();
                let waiting: Vec<(u64, String, String)> = ws
                    .waits
                    .iter()
                    .filter_map(|w| match &w.what {
                        Waiting::Event {
                            of,
                            uid,
                            event: Event::NeedsYou,
                            ..
                        } if *of == terminal => Some((w.key, uid.clone(), w.from.clone())),
                        _ => None,
                    })
                    .collect();
                // The agent that handed it its task, when a call of its hears the dialog now.
                let asker = ws
                    .pane(&terminal)
                    .and_then(|p| p.handed.as_ref())
                    .map(|h| h.by.clone());
                let heard = waiting
                    .iter()
                    .any(|(_, _, from)| Some(from) == asker.as_ref());
                for (key, uid, _) in waiting {
                    ws.answer(key, waiting_for_person(&uid, what.as_deref(), &screen), cx);
                }
                if tell && !heard {
                    ws.tell_waiting(&terminal, what.as_deref(), &screen, cx);
                }
            });
        })
        .detach();
    }

    /// Looks at the terminal a `wait_for(command_finished | attention)` watches, every EVENT_LOOK,
    /// until the wait `key` is gone: the terminal's own thread counts its commands and alerts, so
    /// no frame has to come (a hidden window draws none).
    fn look_until_it_comes(&self, key: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(EVENT_LOOK).await;
                let waiting = ws.update(cx, |ws, cx| ws.look(key, cx)).unwrap_or(false);
                if !waiting {
                    break;
                }
            }
        })
        .detach();
    }

    /// The wait `key` is answered when its command or alert came since the call, or its block
    /// closed. False once the wait is gone.
    fn look(&mut self, key: u64, cx: &mut Context<Self>) -> bool {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return false;
        };
        let Waiting::Event {
            of,
            uid,
            event,
            since,
            watch,
        } = &wait.what
        else {
            return false;
        };
        // Its block closed: answered here, as no frame may come to `drop_orphans`.
        let Some(pane) = self.pane(of) else {
            let reply = Reply::err(format!("{uid} was closed while you waited."));
            self.answer(key, reply, cx);
            return false;
        };
        let status = pane.term.status();
        let reply = match event {
            Event::CommandFinished if status.finished > since.0 => {
                let code = match status.last.and_then(|o| o.code) {
                    Some(code) => format!("with exit code {code}"),
                    None => "without an exit code".to_string(),
                };
                let mut text = format!("A command finished in {uid}, {code}.");
                if let Some(output) = pane.term.shared().last_command() {
                    text.push('\n');
                    text.push_str(tools::tail(&output, tools::MAX_TEXT));
                }
                Reply::ok(text)
            }
            Event::Output => match watch
                .as_ref()
                .and_then(|(id, p)| Some((pane.term.hit(*id)?, p)))
            {
                Some((line, pattern)) => printed(uid, pattern, &line, pane),
                None => return true,
            },
            Event::Attention if status.attention > since.1 => Reply::ok(match status.message {
                Some(message) => format!("{uid} asks for attention: {message}"),
                None => format!("{uid} rang its bell."),
            }),
            _ => return true,
        };
        self.answer(key, reply, cx);
        false
    }
}

/// One of the question's fields has the keyboard.
pub(super) fn field_focused(wait: &Wait, window: &Window, cx: &App) -> bool {
    let Waiting::Question { others, note, .. } = &wait.what else {
        return false;
    };
    others
        .iter()
        .chain(note)
        .any(|(input, _)| input.read(cx).focus_handle(cx).is_focused(window))
}

/// How long a question waits, seconds: the shorter of the call's `timeout` and the setting's
/// minutes when not 0; with neither, until it is answered (None).
pub(super) fn deadline(call: Option<u64>, setting_minutes: u64) -> Option<u64> {
    let setting = (setting_minutes > 0).then_some(setting_minutes * 60);
    match (call, setting) {
        (Some(call), Some(setting)) => Some(call.min(setting)),
        (call, setting) => call.or(setting),
    }
}

/// `wait_for(needs_you)`'s answer: block `uid` waits for its person, for `what` when it says,
/// with its `screen`'s last lines (the dialog as drawn).
fn waiting_for_person(uid: &str, what: Option<&str>, screen: &str) -> Reply {
    let asks = match what.filter(|w| !w.is_empty()) {
        Some(what) => format!("{uid} is waiting for its person: {what}"),
        None => format!("{uid} is waiting for its person."),
    };
    Reply::ok(format!(
        "{asks} Its screen:\n{}",
        tools::tail(screen, tools::MAX_TEXT)
    ))
}

/// What a `wait_for` waited for, as its timeout says it: "No call for attention in t4q8p…".
fn awaited(event: Event) -> &'static str {
    match event {
        Event::Idle => "end of its turn",
        Event::NeedsYou => "wait for its person",
        Event::CommandFinished => "finished command",
        Event::Output => "line matching its pattern",
        Event::Attention => "call for attention",
    }
}

/// The answer to a `wait_for` with `pattern`: the block `uid` printed `line`, and its last lines.
fn printed(uid: &str, pattern: &str, line: &str, pane: &Pane) -> Reply {
    let last = pane.term.shared().lines(PRINTED_LINES).join("\n");
    Reply::ok(format!(
        "{uid} printed a line matching \"{pattern}\":\n{}\nIts last lines:\n{}",
        tools::tail(line, tools::MAX_TEXT),
        tools::tail(&last, tools::MAX_TEXT)
    ))
}
