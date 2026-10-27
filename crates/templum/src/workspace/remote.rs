//! Remote workspaces (docs/specs/templum-to-templum.md, The remote workspace): a `Space` that
//! mirrors a workspace of another Templum. The index keeps its connection, never a canvas;
//! `templum_api::client::Mirror` follows it on a thread of its own and wakes this one.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use futures::StreamExt;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::EditorState;
use gpui_kit::component::notification::Notification;
use gpui_kit::{AppContext as _, ClipboardItem, Context, Entity, Window};
use templum_api::client::{
    self, About, Answered, Call, Command, Inherit, Kept, KeySource, Mirror, Secrets, Status, Tools,
    Update, Why,
};
use templum_api::code::{self, Code, Key, Trust};
use templum_api::model::{AgentView, BlockView, MeView, RequestView, State, WorkspaceView, same};
use templum_api::store::Scope;
use templum_canvas::{BlockId, Camera, Point, Rect, Size, Wire, chrome};
use templum_mcp::control::Kind;
use templum_terminal::{MouseInput, Terminal};

use super::holds::Holds;
use super::waits::{Approved, Asked, RemoteAsk, Wait, Waiting};
use super::{
    Body, DEFAULT_COLS, DEFAULT_ROWS, Mirrored, Pane, RemotePane, Space, Workspace,
    show_notification,
};
use crate::board::{Boards, Draft};
use crate::checklist::Checklist;
use crate::checklists::Checklists;
use crate::kanban::Kanban;
use crate::note::Note;
use crate::notes::Notes;
use crate::session::{Keeping, RemoteEntry};

/// The most blocks a mirror takes from home, the front ones: a canvas of home's is not trusted to
/// have a size this computer can draw.
pub(crate) const MOST_BLOCKS: usize = 500;

/// The most wires a mirror takes from home: each is drawn every frame, against every block.
pub(crate) const MOST_WIRES: usize = 200;

/// The farthest from the origin a block or a camera of home's may sit: past it the canvas' union overflows
/// and the dot grid has no end to paint.
pub(super) const MOST_FAR: f32 = 1e7;

/// The longest a question of home's agent is, in characters.
const ASKS_CHARS: usize = 200;

/// The most requests of one remote workspace the inbox takes, and in one of them the most
/// questions, options in a question, and characters in a line of text, a description and a
/// preview or a plan: what home sends is drawn, so it is capped.
const MOST_REQUESTS: usize = 50;
/// The longest a mirrored request may read as aged: older than a week is a week.
pub(super) const AGE_MOST: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The instant a request home made at `since` (Unix seconds) is aged from, `now` being the clock
/// of this computer and `wall` its Unix time: `now` less the age home's clock says, never ahead
/// of `now` (home's clock may be ahead) and never more than [`AGE_MOST`] back.
pub(super) fn aged(since: i64, now: Instant, wall: i64) -> Instant {
    let age = Duration::from_secs(wall.saturating_sub(since).max(0) as u64).min(AGE_MOST);
    now.checked_sub(age).unwrap_or(now)
}
const MOST_QUESTIONS: usize = 10;
const MOST_OPTIONS: usize = 30;
pub(super) const LINE_CHARS: usize = 300;
pub(super) const TEXT_CHARS: usize = 4000;
const PLAN_CHARS: usize = 64 * 1024;

/// The most notifications of one remote's requests in [`NOTICE_SPAN`]: the rest of a burst is one
/// notice of how many.
const MOST_NOTICES: usize = 3;
const NOTICE_SPAN: Duration = Duration::from_secs(60);
/// The most request ids of one connection remembered as told.
const MOST_TOLD: usize = 10_000;

/// What the person hears of a new request of home's.
#[derive(PartialEq)]
enum Notice {
    Tell,
    /// Over the cap: counted in the one notice of several.
    Fold,
    /// Told before (home listed it, dropped it, listed it again).
    Again,
}

/// Home's multi-line `text` as it is drawn: no control characters but newlines and tabs, at most
/// `most` characters.
pub(super) fn clean_text(text: &str, most: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(most)
        .collect()
}

/// Home's `refusal` as it is drawn: the sentence on one short line, the details (git's stderr)
/// with their newlines and tabs, at most [`client::DETAILS_MOST`] characters; blank details are none.
pub(super) fn clean_refusal(refusal: client::Refusal) -> client::Refusal {
    client::Refusal {
        sentence: super::bus::clean_to(&refusal.sentence, 200),
        details: refusal
            .details
            .map(|d| clean_text(&d, client::DETAILS_MOST))
            .filter(|d| !d.trim().is_empty()),
        maybe_done: refusal.maybe_done,
    }
}

/// What the person is asked by home's `request`, cleaned and capped; None when it asks nothing.
fn waiting_of(request: &RequestView) -> Option<Waiting> {
    use super::bus::clean_to;
    use templum_mcp::ask::{Ask, Choice, Form, Question};
    let ask = match request.kind.as_str() {
        "plan" => {
            let plan = request.plan.as_ref()?;
            templum_mcp::ask::plan(
                plan.title.as_deref().map(|t| clean_to(t, 60)),
                clean_text(&plan.text, PLAN_CHARS),
            )
        }
        "question" => {
            let questions: Vec<Question> = request
                .questions
                .iter()
                .take(MOST_QUESTIONS)
                .map(|q| Question {
                    text: clean_to(&q.question, LINE_CHARS),
                    header: q.header.as_deref().map(|h| clean_to(h, 60)),
                    multi: q.multi,
                    options: q
                        .options
                        .iter()
                        .take(MOST_OPTIONS)
                        .map(|o| Choice {
                            label: clean_to(&o.label, LINE_CHARS),
                            description: o
                                .description
                                .as_deref()
                                .map(|d| clean_text(d, TEXT_CHARS)),
                            preview: o.preview.as_deref().map(|p| clean_text(p, TEXT_CHARS)),
                        })
                        .collect(),
                })
                .collect();
            if questions.is_empty() {
                return None;
            }
            Ask {
                questions,
                json: true,
                plan: None,
                why: request
                    .why
                    .agent
                    .as_deref()
                    .map(|w| clean_to(w, TEXT_CHARS)),
            }
        }
        _ => {
            let (yes, no) = match &request.buttons {
                Some([yes, no]) => (clean_to(yes, 24), clean_to(no, 24)),
                None => ("Allow".into(), "Deny".into()),
            };
            return Some(Waiting::Allow {
                then: Approved::Remote {
                    line: clean_to(&request.summary, ASKS_CHARS),
                    yes,
                    no,
                },
            });
        }
    };
    Some(Waiting::Question {
        form: Form::new(&ask),
        ask,
        others: Vec::new(),
        note: None,
        stopped: false,
        scroll: Default::default(),
    })
}

/// What a remote workspace keeps besides its canvas.
pub(crate) struct RemoteSpace {
    pub entry: RemoteEntry,
    pub status: Status,
    /// Who the share is, from home's `GET /v1/me`.
    pub me: Option<MeView>,
    /// Home's workspace, as the newest state shows it.
    pub home: Option<WorkspaceView>,
    /// Home's state as last taken, its items the client's own (`Arc`s): the next take shows by
    /// pointer which collections changed.
    pub state: State,
    /// The tools home has, as read at the last connect; None until read (and when it couldn't be).
    pub tools: Option<Tools>,
    /// The last round trip to home.
    pub latency: Option<Duration>,
    /// The last time it was live.
    pub live_at: Option<SystemTime>,
    /// When `status` last changed.
    pub since: Instant,
    /// The copy of home's boards, kept from its state and never saved: a kanban of this workspace
    /// draws it (`remote_boards.rs`).
    pub boards: Boards,
    /// The copies of home's rules for this workspace and of its agents' schedules, kept from its
    /// state and never saved: the Automations popup lists them (`remote_rules.rs`). They are
    /// never in `Workspace::automations`: home's automations run only at home.
    pub rules: Vec<super::remote_rules::HomeRule>,
    /// Each rule's last run (home's `at`) as the person last saw it, by home's id: a run that
    /// differs from it is new, with no comparing of clocks (`remote_rules::home_unseen`).
    pub rules_seen: HashMap<u64, Option<i64>>,
    pub schedules: Vec<super::remote_rules::HomeSchedule>,
    /// Home's requests past [`MOST_REQUESTS`], not listed: the inbox says how many.
    pub requests_more: usize,
    /// Home's block ids, which never become the ids agents name blocks by (`Space::uids`).
    pub homes: HashMap<String, BlockId>,
    /// `homes` the other way round.
    names: HashMap<BlockId, String>,
    /// The camera is the person's: home's view or the blocks' fit placed it once.
    pub placed: bool,
    /// The space's icon and color are home's, so the index doesn't keep them as the person's.
    pub home_look: bool,
    /// The terminals the client was last told to stream (home's id, the block here), sorted, and
    /// when that set was last chosen.
    pub streaming: Vec<(String, BlockId)>,
    pub chosen: Option<Instant>,
    /// The client was told to connect it this run: a remote kept nowhere and not started has
    /// nothing to try again, only a new code.
    pub started: bool,
    /// The keys new codes replaced: deleted once the newest one is kept (`replace_code`), or with
    /// the remote. A code pasted again before `Kept` leaves the one before it here too.
    pub old_keys: Vec<(String, bool)>,
    /// Where the current Save reads the secrets a code left empty: (entry, whether in a file). A
    /// code pasted again before `Kept` reads from the same key, the one that holds them.
    pub inherits: Option<(String, bool)>,
    /// Home's rectangle of each block, as its state last said it.
    pub rects: HashMap<BlockId, Rect>,
    /// The blocks moved or resized here and not yet shown by home: the rectangle the person left
    /// them at. It stands over home's until home shows it, or [`holds::HOLD`] passes.
    pub held: Holds<BlockId, Rect>,
    /// The blocks closed here and not yet gone from home's state (true: closed here). They are
    /// not drawn until home's state loses them, or [`holds::HOLD`] passes and they come back.
    pub closed: Holds<BlockId, bool>,
    /// The wires made or taken away here and not yet shown by home: by their two ends, the
    /// smaller first (true: wired here, false: unwired here).
    pub wires_held: Holds<(BlockId, BlockId), bool>,
    /// The cards moved here and not yet shown by home: their column and the card after them
    /// (None: at the end). They stand over home's boards until home's state shows it, a refusal
    /// ends them, or [`holds::HOLD`] passes.
    pub cards_moved: Holds<u64, (u64, Option<u64>)>,
    /// The cards deleted here and not yet gone from home's state (true: deleted here). They are
    /// not drawn until home's state loses them, or [`holds::HOLD`] passes and they come back.
    pub cards_deleted: Holds<u64, bool>,
    /// The cards edited here (Done) and not yet shown by home: the work that was sent, laid over
    /// the copy as the person left it, with the items it adds left out (their ids are this
    /// mirror's own; home's come with its state).
    pub cards_edited: Holds<u64, Draft>,
    /// The cards the last copy of the boards laid an edit over: they are cleaned again from home's
    /// state at the next copy, not moved over.
    pub(super) laid: HashSet<u64>,
    /// The copies of home's notes and checklists, kept from its state and never saved: a note
    /// block and a checklist block of this workspace draw them (`remote_content.rs`).
    pub notes: Notes,
    pub checklists: Checklists,
    /// The editor of each note a note block of home's workspace shows (editable while the rule
    /// lets it, its `Change` and `Blur` sent to `remote_notes.rs`): made where a window is, laid
    /// into the blocks' tabs without one.
    pub note_editors: HashMap<u64, Entity<EditorState>>,
    /// The notes whose text home cut, or this copy's cleaning did: never edited from here.
    pub notes_cut: HashSet<u64>,
    /// The notes typed in here and not yet taken by home, by home's id (`remote_notes.rs`), and
    /// the task that wakes the next send.
    pub(super) edits: HashMap<u64, super::remote_notes::NoteEdit>,
    pub(super) note_wake: Option<gpui_kit::Task<()>>,
    /// What waits until the text typed in a note is at home: a tab or a note block to close.
    pub(super) closing: Vec<super::remote_notes::Closing>,
    /// The notes renamed and recolored here, and the tabs closed here, not yet shown by home:
    /// they stand over the copy until home's state shows it, a refusal ends them, or
    /// [`holds::HOLD`] passes.
    pub notes_renamed: Holds<u64, String>,
    pub notes_colored: Holds<u64, Option<crate::board::Label>>,
    pub tabs_closed: Holds<u64, bool>,
    /// The notes the last copy laid a hold over: cleaned again from home's state at the next copy.
    pub(super) notes_laid: HashSet<u64>,
    /// The note block a tab was asked of (`new_note`, `open_note`): its local id, the tabs home
    /// showed then, what had the keyboard and when it was asked. The new tab is shown with the
    /// keyboard when home's state brings it.
    pub(super) tab_awaited: Option<(BlockId, Vec<u64>, Option<BlockId>, Instant)>,
    /// The items checked, edited and deleted here and not yet shown by home (by checklist and
    /// item): they stand over the copy until home's state shows it, a refusal ends them, or
    /// [`holds::HOLD`] passes.
    pub items_checked: Holds<(u64, u64), bool>,
    pub items_edited: Holds<(u64, u64), String>,
    pub items_deleted: Holds<(u64, u64), bool>,
    /// The items moved here, by the item they went before (None: the end), and the checklists
    /// renamed here, held the same way.
    pub items_moved: Holds<(u64, u64), Option<u64>>,
    pub lists_renamed: Holds<u64, String>,
    /// The checklists the last copy laid an item hold over: cleaned again from home's state at
    /// the next copy, not moved over.
    pub(super) lists_laid: HashSet<u64>,
    /// The block home was asked to add or open and has not shown yet: it gets the keyboard when
    /// it comes, if the person has not moved on.
    pub awaited: Option<Awaited>,
    /// The sentences a shortcut already told this run (`tell_shortcut`).
    pub shortcuts_told: HashSet<String>,
    /// The last id handed to something made here before home hears of it, counting down from
    /// the top: home's ids count up from 1, so none ever meets one of these.
    next_id: u64,
    /// The agents home can open: None until read. Read at each connect and again when a menu
    /// opens [`AGENTS_AGAIN`] after the last read.
    pub agents: Option<Vec<HomeAgent>>,
    pub agents_at: Option<Instant>,
    agents_asking: bool,
    /// The shells home's terminals can run: None until read. Read with `agents`, and again when
    /// a menu that lists them opens [`AGENTS_AGAIN`] after the last read.
    pub shells: Option<HomeShells>,
    pub shells_at: Option<Instant>,
    shells_asking: bool,
    /// Home's recent folders of the workspace, newest first (`home_recent`): read when the Git
    /// popup or its folder menu opens; none from a home without `recent_folders`.
    pub recent: Vec<String>,
    /// What typing already told, once: (home's block, which sentence). "Not connected" is told
    /// again after the remote was live in between.
    pub told: HashSet<(String, &'static str)>,
    /// Home's ids of the requests the person was told of (or whose notice was folded), and when
    /// the last notifications were made: a home that went wrong can't flood the person
    /// (`remote_tells`).
    requests_told: HashSet<String>,
    requests_budget: Budget,
    /// The same for home's notices (a task's end, a watch), counted apart: a burst of requests
    /// never swallows the end of a task.
    notice_budget: Budget,
    /// The notices past the budget, not yet told as how many more.
    notices_folded: usize,
}

/// A block home was asked to add: its id there, what had the keyboard when it was asked, and
/// when home answered.
pub(crate) struct Awaited {
    pub home: String,
    pub before: Option<BlockId>,
    pub at: Instant,
}

/// How long after home answered a block it added may still take the keyboard.
const KEYBOARD_WAIT: Duration = Duration::from_secs(5);

/// The most agents of home's a mirror lists, and the longest an id of one is, in bytes.
pub(crate) const MOST_AGENTS: usize = 100;
const AGENT_ID_MOST: usize = 64;
/// The longest a task's text of home's agent is, in characters, and its id, in bytes.
pub(crate) const TASK_CHARS: usize = 2_000;
const TASK_ID_MOST: usize = 16;

/// How old home's agent list may be when a menu opens before it is read again.
const AGENTS_AGAIN: Duration = Duration::from_secs(60);

/// The most folders the Open dialog lists besides the one of home's workspace, and the most of
/// home's recent folders a mirror keeps.
pub(crate) const MOST_FOLDERS: usize = 20;

/// An agent home can open, as its `list_agents` says.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HomeAgent {
    pub id: String,
    pub title: String,
    pub installed: bool,
}

/// Home's agents out of its `list_agents` answer, a line each as home writes them
/// (`Workspace::agent_list`): `id  "title"  installed|not installed  …`. Home is not trusted: at
/// most [`MOST_AGENTS`], a line with an id that is long, quoted or has a control character is
/// left out, and a title is one clean line. An agent home's person hid (`  hidden` ends its line)
/// is left out, as home's own menus leave it out.
pub(crate) fn home_agents(text: &str) -> Vec<HomeAgent> {
    text.lines()
        .filter_map(home_agent)
        .take(MOST_AGENTS)
        .collect()
}

fn home_agent(line: &str) -> Option<HomeAgent> {
    let (id, rest) = line.split_once("  ")?;
    if id.is_empty() || id.len() > AGENT_ID_MOST || id.chars().any(|c| c.is_control() || c == '"') {
        return None;
    }
    // The title is Rust's `{:?}` of a string: `\"` and `\\` are escaped. A long one is read to its
    // end and kept short.
    let mut chars = rest.strip_prefix('"')?.chars();
    let mut title = String::new();
    let mut closed = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                closed = true;
                break;
            }
            '\\' => {
                let escaped = chars.next()?;
                if title.len() < TITLE_READ {
                    title.push(escaped);
                }
            }
            c if title.len() < TITLE_READ => title.push(c),
            _ => {}
        }
    }
    if !closed || chars.as_str().ends_with("  hidden") {
        return None;
    }
    let title = super::bus::clean_title(&title);
    Some(HomeAgent {
        installed: chars.as_str().starts_with("  installed"),
        title: if title.is_empty() {
            id.to_string()
        } else {
            title
        },
        id: id.to_string(),
    })
}

/// How much of a title is read before `clean_title` cuts it.
const TITLE_READ: usize = 1000;

/// The most shells of home's a mirror lists besides its default.
pub(crate) const MOST_SHELLS: usize = 32;

/// The shells home can open a terminal in, as its `list_shells` says: its default's name (blank
/// when it gave none), and the others', which `add_block`'s `shell` takes back.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct HomeShells {
    pub default: String,
    pub others: Vec<String>,
}

/// Home's shells out of its `list_shells` answer, a name a line as home writes them
/// (`Workspace::shell_list`), the default's first and ending with `  default`. Home is not
/// trusted: at most [`MOST_SHELLS`] others, each once, and a name that isn't already one clean
/// line of a title's length (`clean_title`) is left out, not cleaned: home takes back only the
/// names it wrote.
pub(crate) fn home_shells(text: &str) -> HomeShells {
    let fit = |name: &&str| !name.is_empty() && super::bus::clean_title(name) == *name;
    let default = (text.lines().find_map(|l| l.strip_suffix("  default")))
        .filter(fit)
        .unwrap_or_default()
        .to_string();
    let mut others: Vec<String> = Vec::new();
    for name in text
        .lines()
        .filter(|l| !l.ends_with("  default"))
        .filter(fit)
    {
        if others.len() == MOST_SHELLS {
            break;
        }
        if name != default && !others.iter().any(|s| s == name) {
            others.push(name.into());
        }
    }
    HomeShells { default, others }
}

/// Home's recent folders out of its `recent_folders` answer, a folder a line, newest first.
/// Home is not trusted: at most [`MOST_FOLDERS`], each once, and a line that isn't already one
/// clean line of a folder's length is left out, not cleaned: it would name another folder.
pub(crate) fn home_recent(text: &str) -> Vec<String> {
    let mut recent: Vec<String> = Vec::new();
    for line in text.lines() {
        if recent.len() == MOST_FOLDERS {
            break;
        }
        let fit = !line.is_empty() && super::bus::clean_to(line, LINE_CHARS) == line;
        if fit && !recent.iter().any(|f| f == line) {
            recent.push(line.into());
        }
    }
    recent
}

/// How many notifications of one kind a remote may make in a [`NOTICE_SPAN`], so a home that went
/// wrong can't flood the person.
#[derive(Default)]
struct Budget {
    at: VecDeque<Instant>,
    /// When the last notice of how many more there were was given: one a [`NOTICE_SPAN`].
    folded_at: Option<Instant>,
}

impl Budget {
    /// Whether a notification is made now: at most [`MOST_NOTICES`] in a [`NOTICE_SPAN`].
    fn tell(&mut self, now: Instant) -> bool {
        while (self.at.front()).is_some_and(|at| now.saturating_duration_since(*at) >= NOTICE_SPAN)
        {
            self.at.pop_front();
        }
        if self.at.len() >= MOST_NOTICES {
            return false;
        }
        self.at.push_back(now);
        true
    }

    /// Whether the notification of how many more is given now: one a [`NOTICE_SPAN`].
    fn fold(&mut self, now: Instant) -> bool {
        let again =
            (self.folded_at).is_none_or(|at| now.saturating_duration_since(at) >= NOTICE_SPAN);
        if again {
            self.folded_at = Some(now);
        }
        again
    }
}

impl RemoteSpace {
    pub fn new(entry: RemoteEntry) -> RemoteSpace {
        RemoteSpace {
            placed: entry
                .camera
                .is_some_and(|c| c.x.abs() <= MOST_FAR && c.y.abs() <= MOST_FAR),
            started: false,
            home_look: false,
            streaming: Vec::new(),
            chosen: None,
            told: Default::default(),
            requests_told: HashSet::new(),
            requests_budget: Budget::default(),
            notice_budget: Budget::default(),
            notices_folded: 0,
            boards: Boards::default(),
            rules: Vec::new(),
            rules_seen: HashMap::new(),
            schedules: Vec::new(),
            requests_more: 0,
            rects: HashMap::new(),
            held: Holds::default(),
            closed: Holds::default(),
            wires_held: Holds::default(),
            cards_moved: Holds::default(),
            cards_deleted: Holds::default(),
            cards_edited: Holds::default(),
            laid: HashSet::new(),
            notes: Notes::default(),
            checklists: Checklists::default(),
            note_editors: HashMap::new(),
            notes_cut: HashSet::new(),
            edits: HashMap::new(),
            note_wake: None,
            closing: Vec::new(),
            notes_renamed: Holds::default(),
            notes_colored: Holds::default(),
            tabs_closed: Holds::default(),
            notes_laid: HashSet::new(),
            tab_awaited: None,
            items_checked: Holds::default(),
            items_edited: Holds::default(),
            items_deleted: Holds::default(),
            items_moved: Holds::default(),
            lists_renamed: Holds::default(),
            lists_laid: HashSet::new(),
            awaited: None,
            shortcuts_told: HashSet::new(),
            next_id: u64::MAX,
            agents: None,
            agents_at: None,
            agents_asking: false,
            shells: None,
            shells_at: None,
            shells_asking: false,
            recent: Vec::new(),
            old_keys: Vec::new(),
            inherits: None,
            entry,
            status: Status::Connecting,
            me: None,
            home: None,
            tools: None,
            state: State::default(),
            latency: None,
            live_at: None,
            since: Instant::now(),
            homes: HashMap::new(),
            names: HashMap::new(),
        }
    }

    /// The canvas and the tile are dimmed: it isn't live, nor on its way to being.
    pub fn dim(&self) -> bool {
        !matches!(self.status, Status::Live | Status::Connecting)
    }

    /// What the mirror's API shows of it: never its blocks (docs/specs/templum-to-templum.md,
    /// What the mirror's API shows).
    pub fn view(&self) -> templum_api::model::RemoteView {
        templum_api::model::RemoteView {
            computer: self
                .me
                .as_ref()
                .map(|m| m.computer.clone())
                .filter(|c| !c.is_empty()),
            host: host_of(&self.entry.url),
            way: self.entry.way.clone(),
            status: self.status.word().to_string(),
            latency_ms: self.latency.map(|l| l.as_millis() as u64),
        }
    }

    /// Why `deed` can't be done on this remote now, in the rule's order: it is no call yet, the
    /// remote isn't live, home's Templum lacks the tool (once its tools were read: unknown ones
    /// dim nothing), or the share is below the scope the call takes.
    pub(crate) fn why_not(&self, deed: Deed) -> Option<Nay> {
        if deed == Deed::Draw {
            return Some(Nay::Drawing);
        }
        let Some((tool, args)) = deed.call() else {
            return Some(Nay::NotYet);
        };
        if self.status != Status::Live {
            return Some(Nay::NotConnected);
        }
        if self.tools.as_ref().is_some_and(|tools| {
            !tools.has(tool)
                || deed.needs().iter().any(|arg| !tools.takes(tool, arg))
                // A home that could add a note the mirror can't type in is older too.
                || (deed == Deed::AddNote && !tools.has("edit_note"))
        }) {
            return Some(Nay::Older);
        }
        let takes = templum_api::registry::scope(tool, &args).unwrap_or(Scope::Full);
        let takes = match deed {
            Deed::Answer(asked) => takes.max(asked),
            // Home's glue asks Full to switch or run a rule that runs a command; the registry's
            // scope doesn't know the rule.
            Deed::SetRule(true) | Deed::RunRule(true) => takes.max(Scope::Full),
            _ => takes,
        };
        self.me
            .as_ref()
            .is_none_or(|me| takes > me.scope)
            .then_some(Nay::NeedsFull)
    }

    /// An id for what the person makes here before home hears of it (`Workspace::fresh_id`).
    pub(crate) fn fresh_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id -= 1;
        id
    }

    /// Whether home's tool `tool` takes `argument`, or its tools are unknown: the rule's second
    /// rung, an argument a home lacks is left out when the act still means the same.
    pub(crate) fn takes(&self, tool: &str, argument: &str) -> bool {
        self.tools
            .as_ref()
            .is_none_or(|tools| tools.takes(tool, argument))
    }

    /// Home's agents are not read yet, or were read [`AGENTS_AGAIN`] ago or more.
    pub(crate) fn agents_stale(&self, now: Instant) -> bool {
        stale(self.agents_at, now)
    }

    /// The same for home's shells.
    pub(crate) fn shells_stale(&self, now: Instant) -> bool {
        stale(self.shells_at, now)
    }

    /// Home's workspace this one mirrors: as its state names it, else the entry's.
    pub(crate) fn home_workspace(&self) -> Option<String> {
        (self.home.as_ref().map(|h| h.id.clone())).or_else(|| self.entry.home.clone())
    }

    /// Home's id of the block `id` here.
    pub fn home_of(&self, id: BlockId) -> Option<String> {
        self.names.get(&id).cloned()
    }

    /// The connection the client makes, with its key from `key`. An index that names no way in
    /// Templum knows, or a pin that is missing, reads as one the key was not kept for, which the
    /// client refuses (`Remote::binds`).
    pub fn remote(&self, key: KeySource) -> client::Remote {
        let entry = &self.entry;
        client::Remote {
            url: entry.url.clone(),
            trust: entry.trust(),
            trust_added: entry.trust_added_authority,
            access_id: entry.access_id.clone(),
            proxy: entry.proxy.clone(),
            key,
        }
    }
}

impl RemoteEntry {
    /// The way in it names. One the index names wrongly reads as the public one, or a pin that
    /// is missing as an empty pin, which the client refuses (`Remote::binds`).
    pub(crate) fn trust(&self) -> Trust {
        match self.way.as_str() {
            "pinned" => Trust::Pinned(self.pin.clone().unwrap_or_default()),
            "loopback" => Trust::Loopback,
            _ => Trust::Public,
        }
    }

    /// The entry for a pasted `code`, whose key will be kept under `entry`.
    pub(crate) fn of(code: &Code, entry: String) -> RemoteEntry {
        let (way, pin) = match &code.trust {
            Trust::Pinned(pin) => ("pinned", Some(pin.clone())),
            Trust::Public => ("public", None),
            Trust::Loopback => ("loopback", None),
        };
        RemoteEntry {
            url: code.url.clone(),
            way: way.into(),
            pin,
            entry,
            ..RemoteEntry::default()
        }
    }
}

/// A list of home's read at `at` is not read yet, or was read [`AGENTS_AGAIN`] ago or more.
fn stale(at: Option<Instant>, now: Instant) -> bool {
    at.is_none_or(|at| now.saturating_duration_since(at) >= AGENTS_AGAIN)
}

/// What the Connect dialog hands over: the code and what More… added.
#[derive(Debug)]
pub(crate) struct Connecting {
    pub code: Code,
    /// A Cloudflare Access service token: Client ID and Client Secret (which prints as `tpl_…`).
    /// A new code for a connection may leave the secret empty: the old one stays.
    pub access: Option<(String, Option<Key>)>,
    /// The proxy as typed, `http://[user:password@]host:port`; a `Key` so a login in it never
    /// prints.
    pub proxy: Option<Key>,
    /// Keep the key in a file (no keyring answers).
    pub file: bool,
}

/// Two rectangles that differ by no more than half a unit are the same one: home's float
/// round trip.
fn same_rect(a: Rect, b: Rect) -> bool {
    [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)]
        .iter()
        .all(|(a, b)| (a - b).abs() <= 0.5)
}

impl RemoteSpace {
    /// The rectangle to draw block `id` at, home having it at `home`: the one held when it
    /// is newer than [`holds::HOLD`] and home's isn't it yet; else home's, and the hold goes.
    fn rect_shown(&mut self, id: BlockId, home: Rect, now: Instant) -> Rect {
        self.rects.insert(id, home);
        self.held
            .shown(id, home, now, |mine, home| same_rect(*mine, *home))
    }
}

/// What a Connect dialog's draft becomes: the index entry, the secrets the client keeps, whether
/// the key goes to a file, and the code.
fn prepared(draft: Connecting) -> Result<(RemoteEntry, Secrets, bool, Code), String> {
    let (proxy, proxy_auth) = match draft.proxy.as_ref().map(|p| p.expose().trim()) {
        Some(text) if !text.is_empty() => {
            let (address, login) = client::proxy_of(text)?;
            (Some(address), login)
        }
        _ => (None, None),
    };
    let Connecting {
        code, access, file, ..
    } = draft;
    let entry = RemoteEntry {
        access_id: access.as_ref().map(|(id, _)| id.clone()),
        proxy: proxy.clone(),
        ..RemoteEntry::of(&code, client::entry_name())
    };
    let secrets = Secrets {
        token: code.key.clone(),
        access_secret: access.and_then(|(_, secret)| Some(secret?.expose().to_string())),
        proxy_auth,
        url: code.url.clone(),
        trust: code.trust.clone(),
        trust_added: false,
        proxy,
    };
    Ok((entry, secrets, file, code))
}

/// What the new code leaves empty of the Access token and the proxy login stays: the client
/// takes it from the old key, which Cloudflare won't show again. Only for the same Client ID
/// (and, for the Access token, the same address: the client checks it against the old key) and
/// proxy. `kept`: where the old secrets are kept, (entry, whether in a file).
fn inherited(
    fresh: &RemoteEntry,
    secrets: &Secrets,
    old: &RemoteEntry,
    kept: Option<(String, bool)>,
) -> Option<Inherit> {
    let access = fresh.access_id.is_some()
        && secrets.access_secret.is_none()
        && fresh.access_id == old.access_id;
    let login = fresh.proxy.is_some() && secrets.proxy_auth.is_none() && fresh.proxy == old.proxy;
    let (entry, file) = kept?;
    (access || login).then_some(Inherit {
        entry,
        file,
        access,
        login,
    })
}

/// Where the secrets a new code may inherit are kept: (entry, whether in a file). An old entry not
/// kept yet has none to read (or a Save still running, or one that stopped): the key its own Save
/// read from, dropped only once one is kept, has them. Kept nowhere (and read from nothing): there
/// is nothing to read.
fn holder(old: &RemoteEntry, inherits: &Option<(String, bool)>) -> Option<(String, bool)> {
    match old.key {
        Keeping::None => inherits.clone(),
        key => Some((old.entry.clone(), key == Keeping::File)),
    }
}

/// What the UI thread does with home's answer to a call, when it comes.
pub(crate) type Then =
    Box<dyn FnOnce(&mut Workspace, Answered, &mut Window, &mut Context<Workspace>)>;

/// The most terminals of one remote streamed at a time (the client's own cap is the same).
pub(crate) const MOST_STREAMS: usize = 11;
/// How often the set of terminals to stream is chosen again, at most.
pub(crate) const STREAM_EVERY: Duration = Duration::from_millis(100);

/// The terminals to stream out of `blocks` (home's id and rectangle of each remote terminal that
/// is drawn in full and in view): the focused one first, then the nearest to `center`, at most
/// [`MOST_STREAMS`].
pub(crate) fn streamed(
    blocks: &[(String, Rect)],
    center: Point,
    focused: Option<&str>,
) -> Vec<String> {
    let far = |r: &Rect| {
        let (x, y) = (r.x + r.w / 2.0 - center.x, r.y + r.h / 2.0 - center.y);
        x.hypot(y)
    };
    let mut near: Vec<&(String, Rect)> = blocks.iter().collect();
    near.sort_by(|a, b| {
        let (a_first, b_first) = (Some(a.0.as_str()) == focused, Some(b.0.as_str()) == focused);
        b_first.cmp(&a_first).then(far(&a.1).total_cmp(&far(&b.1)))
    });
    near.into_iter()
        .take(MOST_STREAMS)
        .map(|(id, _)| id.clone())
        .collect()
}

/// The zoom a remote viewer of `grid` cells is drawn at in a block whose content is `content`
/// (world units), the camera being at `zoom`: the largest step of the zoom's that holds home's
/// whole grid (a cell is `cell` at zoom one), else the smallest.
pub(crate) fn fit_zoom(zoom: f32, content: Size, grid: (u16, u16), cell: Size) -> f32 {
    let (cols, rows) = (f32::from(grid.0.max(1)), f32::from(grid.1.max(1)));
    let k = (content.w / (cols * cell.w)).min(content.h / (rows * cell.h));
    let want = zoom * k * (1.0 + 1e-4);
    templum_canvas::ZOOM_STEPS
        .into_iter()
        .rev()
        .find(|step| *step <= want)
        .unwrap_or(templum_canvas::ZOOM_STEPS[0])
}

/// What hearing a remote's update leaves for the drain.
#[derive(Default)]
pub(super) struct Heard {
    /// The index must be written.
    pub index: bool,
    /// The Connect dialog's remote moved: its id, status and words.
    pub settled: Option<(String, Status, String)>,
}

/// What the person does on a remote workspace (docs/specs/templum-to-templum.md, One rule for
/// what the mirror can do): each is a call of home's, or not on a mirror yet. A later part gives a
/// deed its call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Deed {
    Type,
    /// A mouse event for the program in a terminal, which asked for the mouse.
    Mouse,
    /// Reading home's history of a terminal, to scroll back through it.
    ScrollBack,
    Move,
    /// Answering a request that asks this scope of the share.
    Answer(Scope),
    Start,
    /// A terminal running home's default shell.
    AddTerminal,
    /// A terminal running another shell of home's (`add_block`'s `shell`).
    AddShell,
    AddNote,
    /// A key typed in a note, its tabs' + and ×, its Rename, Color and All notes.
    EditNote,
    AddChecklist,
    AddKanban,
    AddEditor,
    OpenAgent,
    /// The Notes dialog, and its rows of a note closed at home.
    OpenNote,
    /// A note deleted for good from the Notes dialog.
    DeleteNote,
    /// The Checklists dialog, and a new checklist in a checklist block that shows none.
    OpenChecklist,
    Close,
    Wire,
    /// Taking a wire away.
    Unwire,
    /// Naming a wire, or taking its name off.
    Label,
    /// Handing an agent a task.
    Delegate,
    Draw,
    MoveToWorkspace,
    CopyBlocks,
    /// An operation of home's `git` by its name (`templum_git::ops::Op::name`).
    Git(&'static str),
    /// What the Git popup leaves on home for now: Clone, Choose another folder, Generate's ⚙.
    GitLeftOut,
    /// A card dragged to a column or another place in it.
    MoveCard,
    /// A new card made (Done on a card that doesn't exist yet).
    CreateCard,
    /// Done on a card that exists.
    EditCard,
    CommentCard,
    EditComment,
    DeleteComment,
    DeleteCard,
    /// An item of a checklist checked or unchecked.
    CheckItem,
    /// New items at the end of a checklist.
    AddItems,
    /// An item's text changed.
    EditItem,
    /// An item taken out of a checklist.
    DeleteItem,
    /// A checklist renamed, or its items moved.
    EditChecklist,
    /// A column added, renamed, moved or deleted.
    EditColumn,
    /// A board renamed or deleted.
    EditBoard,
    /// A kanban shown another board, or a new one.
    ShowBoard,
    /// A rule of home's switched; true when it runs a command (home's glue asks Full for that).
    SetRule(bool),
    /// A rule of home's run now; true when it runs a command.
    RunRule(bool),
    /// A rule of home's saved or duplicated; true when it runs a command.
    SaveRule(bool),
    DeleteRule,
    /// A delivery of an agent of home's cancelled.
    CancelSchedule,
}

impl Deed {
    /// The tool of home's it calls and arguments enough for `registry::scope` to say what scope it
    /// takes; None for a deed that isn't a call yet.
    pub(crate) fn call(self) -> Option<(&'static str, serde_json::Value)> {
        use serde_json::json;
        Some(match self {
            Deed::Type => ("terminal_input", json!({ "block": "b", "keys": ["a"] })),
            Deed::ScrollBack => ("read_terminal", json!({ "block": "b" })),
            Deed::Mouse => (
                "terminal_input",
                json!({
                    "block": "b",
                    "mouse": [{ "kind": "press", "button": "left", "row": 0, "col": 0 }],
                }),
            ),
            Deed::Move => ("move_block", json!({ "block": "b", "x": 0.0, "y": 0.0 })),
            Deed::Answer(_) => ("answer_request", json!({ "request": "r1" })),
            Deed::Start => ("show", json!({ "workspace": "w" })),
            Deed::OpenAgent => ("open_agent", json!({ "agent": "a", "workspace": "w" })),
            Deed::Close => ("close_block", json!({ "block": "b" })),
            Deed::Delegate => ("delegate", json!({ "block": "b", "task": "t" })),
            Deed::Wire => ("connect", json!({ "a": "a", "b": "b" })),
            Deed::Unwire => ("disconnect", json!({ "a": "a", "b": "b" })),
            Deed::Label => ("connect", json!({ "a": "a", "b": "b", "label": "l" })),
            Deed::AddTerminal => ("add_block", json!({ "workspace": "w", "kind": "terminal" })),
            Deed::AddShell => (
                "add_block",
                json!({ "workspace": "w", "kind": "terminal", "shell": "s" }),
            ),
            Deed::AddKanban => ("add_block", json!({ "workspace": "w", "kind": "kanban" })),
            Deed::AddNote => ("add_block", json!({ "workspace": "w", "kind": "note" })),
            Deed::EditNote => ("edit_note", json!({ "block": "n", "note": 1, "name": "x" })),
            Deed::OpenNote => ("open_note", json!({ "note": 1, "block": "n" })),
            Deed::DeleteNote => ("delete_note", json!({ "note": 1 })),
            Deed::AddChecklist => (
                "add_block",
                json!({ "workspace": "w", "kind": "checklist" }),
            ),
            Deed::OpenChecklist => ("new_checklist", json!({ "block": "c" })),
            Deed::EditChecklist => ("edit_checklist", json!({ "block": "c", "name": "n" })),
            Deed::CheckItem => ("check_item", json!({ "block": "b", "item": 1 })),
            Deed::AddItems => ("add_items", json!({ "block": "b", "text": "t" })),
            Deed::EditItem => ("edit_item", json!({ "block": "b", "item": 1, "text": "t" })),
            Deed::DeleteItem => ("delete_item", json!({ "block": "b", "item": 1 })),
            Deed::MoveCard => ("move_card", json!({ "card": 1, "column": "1" })),
            Deed::CreateCard => ("create_card", json!({ "title": "t" })),
            Deed::EditCard => ("edit_card", json!({ "card": 1, "title": "t" })),
            Deed::CommentCard => ("comment_card", json!({ "card": 1, "text": "t" })),
            Deed::EditComment => (
                "edit_comment",
                json!({ "card": 1, "comment": 1, "text": "t" }),
            ),
            Deed::DeleteComment => ("delete_comment", json!({ "card": 1, "comment": 1 })),
            Deed::DeleteCard => ("delete_card", json!({ "card": 1 })),
            Deed::EditColumn => ("edit_column", json!({ "board": "1" })),
            Deed::EditBoard => ("edit_board", json!({ "board": "1", "name": "n" })),
            Deed::ShowBoard => ("show_board", json!({ "block": "k", "board": "1" })),
            Deed::SetRule(_) => ("set_automation", json!({ "automation": 1, "on": true })),
            Deed::RunRule(_) => ("run_automation", json!({ "automation": 1 })),
            Deed::SaveRule(command) => (
                "save_automation",
                json!({ "automation": { "do": if command {
                    json!({ "command": { "text": "t" } })
                } else {
                    json!({ "notify": { "text": "t" } })
                } } }),
            ),
            Deed::DeleteRule => ("delete_automation", json!({ "automation": 1 })),
            Deed::CancelSchedule => ("cancel_schedule", json!({ "schedule": "s1" })),
            Deed::Git(op) if templum_git::ops::OPS.contains(&op) => ("git", json!({ "op": op })),
            _ => return None,
        })
    }
}

impl Deed {
    /// The arguments home's tool must take for the deed to mean the same (the rule's second rung):
    /// a home without one is older than the deed needs.
    pub(crate) fn needs(self) -> &'static [&'static str] {
        match self {
            Deed::Label => &["label"],
            Deed::Mouse => &["mouse"],
            Deed::AddShell => &["shell"],
            _ => &[],
        }
    }
}

/// Why a deed can't be done now, in the order the rule asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nay {
    /// No call of home's does it yet.
    NotYet,
    /// Drawing is on home for now.
    Drawing,
    NotConnected,
    /// Home's Templum lacks the tool.
    Older,
    /// The share is below the scope the call takes.
    NeedsFull,
}

impl Nay {
    /// The sentence that says it, `computer` being the other computer's name.
    pub(crate) fn words(self, computer: &str) -> String {
        match self {
            Nay::NotYet => NOT_YET.into(),
            Nay::Drawing => format!("Drawing on a mirror isn't there yet: draw on {computer}."),
            Nay::NotConnected => "Not connected.".into(),
            Nay::Older => format!("{computer}'s Templum is older: update it to do this from here."),
            Nay::NeedsFull => format!("Needs a Full share: share again on {computer}."),
        }
    }
}

/// The deed a shortcut does, for the rule to say whether the mirror can: None for a command that
/// is no deed of the canvas.
pub(super) fn command_deed(command: crate::keys::Command) -> Option<Deed> {
    use crate::keys::Command;
    Some(match command {
        Command::NewTerminal => Deed::AddTerminal,
        Command::CloseTerminal => Deed::Close,
        Command::NewKanban => Deed::AddKanban,
        Command::NewNote => Deed::AddNote,
        Command::NewEditor | Command::OpenFile => Deed::AddEditor,
        Command::NewGit => Deed::Git("look"),
        _ => return None,
    })
}

/// What typing says when the remote isn't live (the client's own sentence, said once).
pub(crate) const NOT_CONNECTED: &str = "Not connected: what you type isn't sent.";
/// `RemoteSpace::told`'s tags.
const NOT_LIVE: &str = "not live";
const NEEDS_FULL: &str = "needs full";
const OLDER: &str = "older";
const MOUSE_NEEDS_FULL: &str = "mouse needs full";

/// What the banner over a remote's canvas can be asked to do (its buttons).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Act {
    TryNow,
    Paste,
    Forget,
    StartIt,
    Connect,
    ConnectHere,
    DontConnect,
    ConnectAnyway,
}

impl Act {
    /// The button's words; a retry of a remote that is still trying says "Try now".
    pub(crate) fn label(self, status: &Status) -> &'static str {
        match self {
            Act::TryNow if matches!(status, Status::Reconnecting { .. }) => "Try now",
            Act::TryNow => "Try again",
            Act::Paste => "Paste a new code…",
            Act::Forget => "Forget",
            Act::StartIt => "Start it",
            Act::Connect => "Connect",
            Act::ConnectHere => "Connect here again",
            Act::DontConnect => "Don't connect",
            Act::ConnectAnyway => "Connect anyway",
        }
    }
}

/// What the banner says of a remote: its words, its buttons, and whether the canvas is dimmed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Banner {
    pub text: String,
    pub acts: Vec<Act>,
    pub dim: bool,
}

/// The host of a code's address as a person says it: no port when it is the scheme's own.
pub(crate) fn host_of(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", url));
    let host = rest.split('/').next().unwrap_or(rest);
    let own = if scheme.eq_ignore_ascii_case("http") {
        ":80"
    } else {
        ":443"
    };
    host.strip_suffix(own).unwrap_or(host).to_string()
}

/// "2 min ago", from whole seconds.
fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s ago"),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => format!("{} d ago", seconds / 86_400),
    }
}

/// How long is left of a share that ends within three days (the banner says so), else None:
/// "5 h" under a day, else whole days rounded up.
fn ends_in(expires: Option<u64>, wall: SystemTime) -> Option<String> {
    let now = wall.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    let left = expires?.checked_sub(now)?;
    if left >= 3 * 86_400 {
        return None;
    }
    Some(crate::settings::api::span(if left < 86_400 {
        left
    } else {
        left.div_ceil(86_400) * 86_400
    }))
}

/// What the banner of `remote` says now (docs/specs/templum-to-templum.md, When it isn't live).
/// Every state but Live and Connecting is dim.
pub(crate) fn banner_words(remote: &RemoteSpace, now: Instant, wall: SystemTime) -> Banner {
    let host = host_of(&remote.entry.url);
    let name = remote
        .home
        .as_ref()
        .map(|h| h.name.as_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("this workspace");
    let computer = remote
        .me
        .as_ref()
        .map(|m| m.computer.as_str())
        .filter(|c| !c.is_empty())
        .unwrap_or(&host);
    let said = |text: String, acts: &[Act], dim: bool| Banner {
        text,
        acts: acts.to_vec(),
        dim,
    };
    match &remote.status {
        Status::Live if remote.home.as_ref().is_some_and(|h| h.stopped) => said(
            format!("{name} is stopped on {computer}."),
            &[Act::StartIt],
            false,
        ),
        Status::Live => {
            let mut text = format!("Mirror of {name} on {computer}");
            if let Some(me) = &remote.me {
                text += &format!(" · {}", me.scope.word());
            }
            if let Some(latency) = remote.latency {
                text += &format!(" · {} ms", latency.as_millis());
            }
            if let Some(left) = ends_in(remote.me.as_ref().and_then(|m| m.expires), wall) {
                text += &format!(" · ends in {left}: share again on {computer}");
            }
            said(text, &[], false)
        }
        Status::Connecting => said("Connecting…".into(), &[], false),
        Status::Reconnecting {
            attempt,
            next,
            live_at,
        } => {
            let left = next.saturating_sub(now.saturating_duration_since(remote.since));
            let mut text = format!(
                "Reconnecting to {computer}… try {attempt}, next in {} s.",
                left.as_secs()
            );
            if let Some(at) = live_at.or(remote.live_at)
                && let Ok(since) = wall.duration_since(at)
            {
                text += &format!(" Live {}.", ago(since.as_secs()));
            }
            said(text, &[Act::TryNow], true)
        }
        Status::Cut(why) => {
            let (text, acts): (String, &[Act]) = match why {
                Why::Expired => (
                    format!(
                        "This share of {name} expired. Share again on {computer}, then Paste a new code…"
                    ),
                    &[Act::Paste, Act::Forget],
                ),
                Why::Revoked => (
                    format!(
                        "{computer} revoked this share. Share again there, then Paste a new code…"
                    ),
                    &[Act::Paste, Act::Forget],
                ),
                Why::Deleted => (format!("{name} was deleted on {computer}."), &[Act::Forget]),
                Why::Replaced => (
                    "Another computer connected with this share's code, so this one stopped. \
                     Each computer needs its own share."
                        .into(),
                    &[Act::ConnectHere, Act::Forget],
                ),
                Why::PinChanged => (
                    format!(
                        "{computer}'s certificate changed since you connected. If you made a New \
                         certificate there, share again and paste the new code. If you didn't, \
                         someone may be between the two computers: don't connect."
                    ),
                    &[Act::Paste, Act::Forget],
                ),
                Why::Certificate(bad) => {
                    let what = match bad {
                        client::Bad::Expired => "it expired",
                        client::Bad::Name => "it is for another name",
                        client::Bad::Issuer | client::Bad::Alone => {
                            "no authority this computer trusts signed it"
                        }
                        client::Bad::Other => "it couldn't be checked",
                    };
                    let mut text = format!(
                        "{host}'s certificate isn't valid: {what}. Fix it where the tunnel or \
                         proxy runs, then Try again."
                    );
                    if *bad == client::Bad::Alone {
                        text += " If it is Templum's own, share Directly instead: it is pinned.";
                    }
                    (text, &[Act::TryNow])
                }
                Why::AddedAuthority => (
                    format!(
                        "HTTPS to {host} is signed by an authority this computer added, not a \
                         public one. A company proxy or security program may be reading this \
                         connection: your key, what home's terminals show and what you type. \
                         With a Full share it could run commands on {computer}. Connect only if \
                         you added that authority yourself (a home lab, your own proxy)."
                    ),
                    &[Act::DontConnect, Act::ConnectAnyway],
                ),
                Why::Access => (
                    format!(
                        "Cloudflare Access stands in front of {host}. Add a service token in \
                         Paste a new code… ▸ More…, and allow it in the Access policy."
                    ),
                    &[Act::TryNow, Act::Paste],
                ),
                Why::NotTemplum { status, kind } => {
                    // What answers at the code's URL names its own type: not trusted to be short.
                    let kind = super::bus::clean_to(kind, 60);
                    let got = if *status == 0 {
                        kind
                    } else {
                        format!("HTTP {status}, {kind}")
                    };
                    (
                        format!(
                            "{host} didn't answer as a Templum ({got}). Check that the tunnel \
                             points at port 7411."
                        ),
                        &[Act::TryNow],
                    )
                }
                Why::TooOld => (
                    format!("{computer} runs a Templum that can't be mirrored: update it."),
                    &[Act::TryNow],
                ),
                Why::Proxy { proxy, status } => {
                    let proxy = proxy.split_once("://").map_or(proxy.as_str(), |(_, p)| p);
                    let because = match status {
                        407 => "407: it asks for a login Templum can't give".to_string(),
                        0 => "it can't be used".to_string(),
                        other => format!("HTTP {other}"),
                    };
                    (
                        format!("The proxy {proxy} refused the connection ({because})."),
                        &[Act::TryNow],
                    )
                }
                Why::Keyring => (
                    "Templum couldn't read this connection's key from the keyring. Try again, \
                     or Paste a new code…"
                        .into(),
                    &[Act::TryNow, Act::Paste],
                ),
            };
            said(text, acts, true)
        }
        Status::Disconnected => said("Disconnected. Connect".into(), &[Act::Connect], true),
    }
}

/// The words for a remote that stopped a Connect: the banner's, except that a remote the dialog
/// forgets isn't "reconnecting".
fn failure_words(remote: &RemoteSpace) -> String {
    match remote.status {
        Status::Reconnecting { .. } => {
            format!("Couldn't reach {}.", crate::mirror::computer_of(remote))
        }
        _ => banner_words(remote, Instant::now(), SystemTime::now()).text,
    }
}

/// What a viewer of home's terminal is made with.
pub(super) struct Viewers {
    cell: templum_canvas::Size,
    palette: templum_terminal::Palette,
    options: templum_terminal::TermOptions,
    notify: templum_terminal::Notify,
}

/// An agent as home tells it, in the shape of a local agent's session, for the chip.
fn session_of(agent: &AgentView) -> Option<templum_usage::Session> {
    let usage = agent.usage.as_ref()?;
    Some(templum_usage::Session {
        model: agent.model.clone(),
        // Home's numbers are drawn in the chip: only ones a chip can show.
        context_percent: usage.context_percent.filter(|p| (0.0..=100.0).contains(p)),
        tokens: usage.tokens,
        cost_usd: usage.cost_usd.filter(|c| (0.0..1e6).contains(c)),
        ..templum_usage::Session::default()
    })
}

/// The banner's ⓘ: what a mirror doesn't have yet.
pub(crate) const NOT_MIRRORED: &str = "Not mirrored yet: editors' content (a card shows each). \
     Not on a mirrored workspace yet: editors, deleting a checklist, drawing, Move \
     to workspace, Copy and Paste of blocks, Stop, a card's attachments, and in the Git popup \
     Clone, Choose another folder, Generate's ⚙, Open in editor and Run in a terminal.";

/// What the Git popup's menus ask of the rule when they are built: what needs a Full share, and
/// what the popup leaves on home for now.
pub(crate) const GIT_GATES: [Deed; 7] = [
    Deed::Git("discard"),
    Deed::Git("discard_hunk"),
    Deed::Git("restore_version"),
    Deed::Git("delete_branch"),
    Deed::Git("stash_drop"),
    Deed::Git("abort"),
    Deed::GitLeftOut,
];

/// The tooltip of a control a mirrored workspace doesn't have yet.
pub(crate) const NOT_YET: &str = "Not on a mirrored workspace yet";

impl Space {
    pub(crate) fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// The other computer a remote workspace mirrors: its name once it said, else its address.
    /// Home says its own name, and it goes into an agent's instructions: `clean_title`.
    fn mirror_of(&self) -> Option<String> {
        let remote = self.remote.as_deref()?;
        Some(
            remote
                .me
                .as_ref()
                .map(|m| super::bus::clean_title(&m.computer))
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| host_of(&remote.entry.url)),
        )
    }

    /// Why this computer's automations never run for a remote workspace: "API is a mirror of
    /// winx-desktop: its automations run there." None for a local one (the person's hard rule:
    /// home's automations run only at home).
    pub(crate) fn automations_there(&self) -> Option<String> {
        Some(format!(
            "{} is a mirror of {}: its automations run there.",
            super::bus::clean_title(&self.name),
            self.mirror_of()?
        ))
    }

    /// How `overview` names a remote workspace: "API (mirror of winx-desktop)".
    pub(crate) fn mirror_title(&self) -> Option<String> {
        Some(format!(
            "{} (mirror of {})",
            super::bus::clean_title(&self.name),
            self.mirror_of()?
        ))
    }

    /// What a tool naming a remote workspace is told: "API is a mirror of winx-desktop: act on it
    /// there." Its blocks are home's, never reached from here.
    pub(super) fn mirror_words(&self) -> Option<String> {
        Some(format!(
            "{} is a mirror of {}: act on it there.",
            super::bus::clean_title(&self.name),
            self.mirror_of()?
        ))
    }

    /// The remote terminals of this workspace to stream now, by home's id: those in view (drawn
    /// in full, else only the focused one), the focused first, at most [`MOST_STREAMS`].
    pub(super) fn streamed_set(
        &self,
        remote: &RemoteSpace,
        vp: Size,
        full: bool,
    ) -> Vec<(String, BlockId)> {
        let focused = self.focused.and_then(|id| remote.home_of(id));
        let camera = self.view;
        let center = Point::new(
            camera.origin.x + vp.w / 2.0 / camera.zoom(),
            camera.origin.y + vp.h / 2.0 / camera.zoom(),
        );
        let seen: std::collections::HashSet<BlockId> = self
            .canvas
            .visible_from(&camera, vp)
            .filter(|b| self.shown(b.id))
            .map(|b| b.id)
            .collect();
        let blocks: Vec<(String, Rect)> = remote
            .homes
            .iter()
            .filter(|(home, id)| {
                self.terminal(**id)
                    .and_then(|pane| pane.remote.as_ref())
                    .is_some_and(|r| r.block == **home)
                    && (Some(home.as_str()) == focused.as_deref() || (full && seen.contains(*id)))
            })
            .filter_map(|(home, id)| Some((home.clone(), self.canvas.get(*id)?.rect)))
            .collect();
        streamed(&blocks, center, focused.as_deref())
            .into_iter()
            .filter_map(|home| Some((home.clone(), *remote.homes.get(&home)?)))
            .collect()
    }

    /// Home's blocks of workspace `home`, back to front, and its wires become this canvas's.
    pub(super) fn mirror_state(
        &mut self,
        remote: &mut RemoteSpace,
        state: &State,
        home: Option<&str>,
        make: &Viewers,
        (now, dragged): (Instant, &HashSet<BlockId>),
    ) {
        let mut blocks: Vec<&Arc<BlockView>> = state
            .blocks
            .iter()
            .filter(|b| home.is_some_and(|h| h == b.workspace))
            .collect();
        blocks.sort_by_key(|b| b.z);
        self.mirror_blocks(remote, &blocks, make, (now, dragged));
        self.mirror_wires(remote, state, now);
    }

    /// Home's blocks, back to front, become this canvas's: new ones are added quietly (no id for
    /// agents, no undo step, no focus, no pop-in), known ones take home's rectangle, title and
    /// agent, and those home no longer has go without a trace.
    fn mirror_blocks(
        &mut self,
        remote: &mut RemoteSpace,
        blocks: &[&Arc<BlockView>],
        make: &Viewers,
        (now, dragged): (Instant, &HashSet<BlockId>),
    ) {
        // The front MOST_BLOCKS whose rectangle is sane; a rectangle is clamped like a restored
        // canvas', and one beyond MOST_FAR (or not finite) is left out.
        let mut blocks: Vec<(&Arc<BlockView>, Rect)> = blocks
            .iter()
            .rev()
            .filter_map(|b| Some((*b, mirrored_rect(b, make.cell)?)))
            .take(MOST_BLOCKS)
            .collect();
        blocks.reverse();
        let wanted: std::collections::HashSet<&str> =
            blocks.iter().map(|(b, _)| b.id.as_str()).collect();
        let gone: Vec<(String, BlockId)> = remote
            .homes
            .iter()
            .filter(|(home, _)| !wanted.contains(home.as_str()))
            .map(|(home, id)| (home.clone(), *id))
            .collect();
        for (home, id) in gone {
            remote.homes.remove(&home);
            remote.names.remove(&id);
            remote.rects.remove(&id);
            remote.held.end(&id);
            remote.closed.end(&id);
            self.canvas.remove(id);
            // A tile's neighbour takes its place; the last one's going brings the canvas back.
            if self.untile(id).is_some() && self.grid.is_none() {
                self.calm();
            }
            self.blocks.remove(&id);
            self.appearing.remove(&id);
            self.selected.retain(|s| *s != id);
            self.wires.retain(|w| !w.touches(id));
            if self.hovered == Some(id) {
                self.hovered = None;
            }
            if self.focused == Some(id) {
                self.focused = self.top();
            }
            if self.selected_wire.is_some_and(|w| w.touches(id)) {
                self.selected_wire = None;
            }
        }
        for (b, rect) in &blocks {
            let rect = *rect;
            let b = &tidy_block(b);
            let id = match remote.homes.get(&b.id).copied() {
                // Closed here, home not caught up: not drawn.
                Some(id) if remote.closed.shown(id, false, now, |a, b| a == b) => continue,
                Some(id) if self.canvas.get(id).is_some() => id,
                // Closed here and still home's past the hold, or refused: a new block.
                stale => {
                    if let Some(stale) = stale {
                        remote.names.remove(&stale);
                    }
                    let id = self.canvas.add(rect);
                    self.blocks.insert(id, remote_body(b, rect, make));
                    remote.homes.insert(b.id.clone(), id);
                    remote.names.insert(id, b.id.clone());
                    id
                }
            };
            // The person's hand has the block: it follows the pointer, not home. A tile stays the
            // grid's: its place on the canvas follows home.
            let shown = remote.rect_shown(id, rect, now);
            if !dragged.contains(&id)
                && let Some(place) = self.home_mut(id)
            {
                *place = shown;
            }
            match self.blocks.get_mut(&id) {
                Some(Body::Terminal(pane)) => {
                    pane.remote = Some(RemotePane {
                        block: b.id.clone(),
                        title: b.title.clone(),
                        agent: b.agent.clone(),
                        read_only: b.read_only,
                    });
                    pane.usage = b.agent.as_ref().and_then(session_of);
                }
                Some(Body::Mirrored(card)) => card.title.clone_from(&b.title),
                Some(Body::Kanban(kanban)) if kanban.remote => kanban.board = b.board,
                Some(Body::Checklist(checklist)) if checklist.remote => {
                    checklist.list = b.checklist
                }
                _ => {}
            }
        }
        // Home's order, back to front.
        let order: Vec<BlockId> = blocks
            .iter()
            .filter_map(|(b, _)| remote.homes.get(&b.id).copied())
            .filter(|id| self.canvas.get(*id).is_some())
            .collect();
        let now: Vec<BlockId> = self.canvas.blocks().iter().map(|b| b.id).collect();
        if order != now {
            for id in order {
                self.canvas.raise(id);
            }
        }
    }

    /// Home's wires, between the blocks mirrored here (the first `MOST_WIRES` of them, each
    /// once), and their labels (kept by home's ids). A wire made or taken away here stands over
    /// home's until home shows it or the hold passes.
    fn mirror_wires(&mut self, remote: &mut RemoteSpace, state: &State, now: Instant) {
        let mut seen = std::collections::HashSet::new();
        let mut wires = Vec::new();
        let mut labels = HashMap::new();
        for w in &state.wires {
            if wires.len() == MOST_WIRES {
                break;
            }
            let (Some(a), Some(b)) = (remote.homes.get(&w.a), remote.homes.get(&w.b)) else {
                continue;
            };
            // A block closed here and not gone from home's state yet is not on the canvas.
            if self.canvas.get(*a).is_none() || self.canvas.get(*b).is_none() {
                continue;
            }
            let key = super::wires::label_key(&w.a, &w.b);
            let Some(wire) = Wire::new(*a, *b) else {
                continue;
            };
            if !seen.insert(key.clone()) {
                continue;
            }
            wires.push(wire);
            if let Some(label) = w
                .label
                .as_deref()
                .map(super::bus::clean_title)
                .filter(|l| !l.is_empty())
            {
                labels.insert(key, label);
            }
        }
        for key @ (a, b) in remote.wires_held.keys() {
            let wire = Wire::new(a, b)
                .filter(|_| self.canvas.get(a).is_some() && self.canvas.get(b).is_some());
            let Some(wire) = wire else {
                remote.wires_held.end(&key);
                continue;
            };
            let home = wires.contains(&wire);
            let want = remote
                .wires_held
                .shown(key, home, now, |mine, home| mine == home);
            if want && !home {
                wires.push(wire);
            } else if !want && home {
                wires.retain(|w| *w != wire);
            }
        }
        if self.wires != wires {
            self.wires = wires;
        }
        if self.selected_wire.is_some_and(|w| !self.wires.contains(&w)) {
            self.selected_wire = None;
        }
        self.labels = labels;
    }
}

/// Home's rectangle of `b` as this canvas keeps it: None when it is not finite or lies beyond
/// `MOST_FAR`.
fn mirrored_rect(b: &BlockView, cell: Size) -> Option<Rect> {
    let r = Rect::new(b.rect.x, b.rect.y, b.rect.w, b.rect.h);
    if r.x.abs() > MOST_FAR || r.y.abs() > MOST_FAR {
        return None;
    }
    super::persist::restorable_rect(r, cell)
}

/// Home's block with its words cleaned where they are used: they reach this computer's tiles,
/// index and API (docs/specs/templum-to-templum.md, threat model). One line each, and short.
fn tidy_block(b: &BlockView) -> BlockView {
    use super::bus::{clean_title, clean_to};
    let mut b = b.clone();
    b.title = clean_title(&b.title);
    b.kind = clean_title(&b.kind);
    if let Some(a) = &mut b.agent {
        a.agent = clean_title(&a.agent);
        a.state = clean_title(&a.state);
        a.asks = a.asks.as_deref().map(|s| clean_to(s, ASKS_CHARS));
        a.model = a.model.as_deref().map(clean_title);
        a.effort = a.effort.as_deref().map(clean_title);
        a.role = a.role.as_deref().map(clean_title);
        // The task is drawn in the Agents popup: its id and `by` are short plain words, else none.
        a.task = a.task.take().and_then(|t| {
            let plain = |s: &str, most: usize| s.len() <= most && !s.chars().any(char::is_control);
            plain(&t.id, TASK_ID_MOST).then(|| templum_api::model::TaskView {
                text: clean_text(&t.text, TASK_CHARS),
                by: t.by.filter(|by| plain(by, AGENT_ID_MOST)),
                ..t
            })
        });
    }
    b
}

/// Who home says it is: its computer's name is shown on tiles and in the banner, and the id of its
/// first workspace is written to index.json, so one that is long or has control characters is
/// ignored.
pub(super) fn tidy_me(me: &mut MeView) {
    me.computer = super::bus::clean_title(&me.computer);
    if let Some(ids) = &mut me.workspaces {
        ids.retain(|id| id.len() <= 64 && !id.chars().any(char::is_control));
    }
}

/// The body of home's block `b`, new here at `rect`: a viewer for a terminal, else the card of
/// what isn't mirrored yet.
fn remote_body(b: &BlockView, rect: Rect, make: &Viewers) -> Body {
    if b.kind == "kanban" {
        return Body::Kanban(Kanban {
            board: b.board,
            remote: true,
        });
    }
    if b.kind == "note" {
        return Body::Note(Note {
            remote: true,
            ..Note::default()
        });
    }
    if b.kind == "checklist" {
        return Body::Checklist(Checklist {
            list: b.checklist,
            editing: None,
            scroll: Default::default(),
            remote: true,
        });
    }
    if b.kind != "terminal" {
        return Body::Mirrored(Mirrored {
            kind: b.kind.clone(),
            title: b.title.clone(),
        });
    }
    // Home's grid is only a guess from the rectangle until its first frame sets it. Home's
    // rectangle is not trusted: the guess is capped, or a huge one would allocate a huge grid.
    let (cols, rows) = chrome::grid_size(chrome::content_rect(rect).size(), make.cell);
    let grid = (cols.min(DEFAULT_COLS), rows.min(DEFAULT_ROWS));
    let options = templum_terminal::TermOptions {
        scrollback: 0,
        ..make.options
    };
    let term = Terminal::viewer(&b.title, grid, make.palette, options, make.notify.clone());
    let mut pane = Pane::new(term, None, None);
    pane.remote = Some(RemotePane {
        block: b.id.clone(),
        title: b.title.clone(),
        agent: b.agent.clone(),
        read_only: b.read_only,
    });
    Body::Terminal(Box::new(pane))
}

impl Workspace {
    /// The workspace shown is a mirror of another computer's. What a remote must never hold (an
    /// id for an agent, a wire, a label, ink) is guarded by this; what the person does asks
    /// `why_not`.
    pub(crate) fn on_remote(&self) -> bool {
        self.space.is_remote()
    }

    /// `r<n>`, the smallest `n` from 1 no workspace has.
    pub(super) fn remote_id(&self) -> String {
        (1..)
            .map(|n| format!("r{n}"))
            .find(|id| !self.spaces().any(|s| s.id == *id))
            .expect("an unused id")
    }

    /// The client, started at the first need. It wakes this workspace through a channel, as a
    /// terminal's reader does, and everything it says is taken at most once per wake.
    pub(super) fn mirror(&mut self, window: &mut Window, cx: &mut Context<Self>) -> &Mirror {
        if self.mirror.is_none() {
            let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = tx.unbounded_send(());
            });
            self.mirror = Some(Mirror::start(self.keys.clone(), wake));
            self._mirror = cx.spawn_in(window, async move |ws, cx| {
                while rx.next().await.is_some() {
                    while rx.try_recv().is_ok() {}
                    if ws
                        .update_in(cx, |ws, window, cx| ws.drain_mirror(window, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        self.mirror.as_ref().expect("started above")
    }

    /// Starts every remote that has its key; one without is disconnected.
    pub(super) fn connect_remotes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let remotes: Vec<_> = self
            .spaces()
            .filter_map(|s| s.remote.as_ref().map(|r| (s.id.clone(), r.entry.clone())))
            .collect();
        for (id, entry) in remotes {
            if entry.key == Keeping::None {
                if let Some(remote) = self.remote_mut(&id) {
                    remote.status = Status::Disconnected;
                }
                continue;
            }
            let key = KeySource::Kept {
                entry: entry.entry.clone(),
                file: entry.key == Keeping::File,
            };
            let Some(remote) = self.remote_mut(&id).map(|r| r.remote(key)) else {
                continue;
            };
            self.mirror(window, cx).connect(&id, remote);
            if let Some(remote) = self.remote_mut(&id) {
                remote.started = true;
            }
        }
        cx.notify();
    }

    pub(super) fn remote_mut(&mut self, id: &str) -> Option<&mut RemoteSpace> {
        self.space_by_id_mut(id)?.remote.as_deref_mut()
    }

    /// Takes what the client said since the last wake: statuses and the rest into each remote,
    /// then its newest state into the canvas. Once per wake, never per paint.
    pub(super) fn drain_mirror(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mirror) = &self.mirror else {
            return;
        };
        let updates = mirror.updates();
        let ids: Vec<String> = self
            .spaces()
            .filter(|s| s.is_remote())
            .map(|s| s.id.clone())
            .collect();
        let states: Vec<(String, State)> = ids
            .iter()
            .cloned()
            .filter_map(|id| Some((id.clone(), mirror.state(&id)?)))
            .collect();
        let mut index = false;
        let mut connected = Vec::new();
        for (id, update) in updates {
            let heard = self.heard(&id, update, window, cx);
            index |= heard.index;
            connected.extend(heard.settled);
        }
        for (id, state) in states {
            self.apply_remote(&id, state, window, cx);
        }
        for id in ids {
            self.fold_notices(&id, window, cx);
        }
        for (id, status, words) in connected {
            self.connect_moved(&id, &status, words, window, cx);
        }
        if index {
            self.write_index(window, cx);
        }
        cx.notify();
    }

    /// Remote `id` said `update`. What it leaves to the drain: whether the index changed, and how
    /// the Connect dialog's remote settled.
    pub(super) fn heard(
        &mut self,
        id: &str,
        update: Update,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Heard {
        let mut heard = Heard::default();
        // The probe's answer has no remote.
        if id.is_empty() {
            if let Update::Keyring(answers) = update {
                self.keyring = Some(answers);
            }
            return heard;
        }
        match update {
            Update::Refused { about, sentence } => {
                self.refused(id, about, &sentence, window, cx);
                return heard;
            }
            Update::Notice(notice) => {
                self.remote_notice(id, notice, window, cx);
                return heard;
            }
            Update::History(block) => {
                self.history_came(id, &block, cx);
                return heard;
            }
            Update::Dropped(_) => {
                let computer = self
                    .space_named(id)
                    .and_then(|s| s.remote.as_deref())
                    .map_or_else(|| "home".into(), crate::mirror::computer_of);
                let sentence =
                    format!("Typing faster than {computer} answers: some keys were dropped.");
                self.warn(sentence, window, cx);
                return heard;
            }
            _ => {}
        }
        let pending = self.pending_connect.as_deref() == Some(id);
        // Home's lists are read once its tools are known and it is live, whichever comes last:
        // a call made before it is live is refused.
        let mut reads_lists = false;
        // Notes typed while it was not live (or before who the share is was known) go now.
        let mut note_steps = false;
        let Some(remote) = self.remote_mut(id) else {
            return heard;
        };
        match update {
            Update::Status(status) => {
                remote.since = Instant::now();
                if status == Status::Live {
                    remote.live_at = Some(SystemTime::now());
                    remote.told.retain(|(_, said)| *said != NOT_LIVE);
                    reads_lists = remote.tools.is_some();
                    note_steps = true;
                }
                remote.status = status.clone();
                if pending {
                    let words = failure_words(remote);
                    heard.settled = Some((id.to_string(), status, words));
                } else if status == Status::Live {
                    // A new code pasted for this remote is a key: it doesn't wait in the clipboard.
                    if let Some(code) = self.connect_clip.take() {
                        let held = cx.read_from_clipboard().and_then(|item| item.text());
                        if held.is_some_and(|text| text.trim() == code) {
                            cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                        }
                    }
                }
            }
            Update::Me(mut me) => {
                tidy_me(&mut me);
                let first = me.workspaces.as_ref().and_then(|w| w.first()).cloned();
                if first.is_some() && first != remote.entry.home {
                    remote.entry.home = first;
                    heard.index = true;
                }
                remote.me = Some(me);
                note_steps = true;
            }
            Update::Latency(latency) => remote.latency = Some(latency),
            Update::Tools(tools) => {
                remote.tools = Some(tools);
                reads_lists = remote.status == Status::Live;
            }
            Update::Kept(kept) => {
                remote.entry.key = match kept {
                    Kept::Keyring => Keeping::Keyring,
                    Kept::File => Keeping::File,
                    Kept::Memory => Keeping::None,
                };
                heard.index = true;
                // The new code's key is kept: the one it replaced is not needed.
                remote.inherits = None;
                let old = std::mem::take(&mut remote.old_keys);
                for (entry, file) in old {
                    self.mirror(window, cx).drop_key(&entry, file);
                }
            }
            // Said above, or the probe's, which has no remote.
            Update::Refused { .. }
            | Update::Dropped(_)
            | Update::History(_)
            | Update::Keyring(_)
            | Update::Notice(_) => {}
        }
        if reads_lists {
            self.read_home_agents(id, cx);
            self.read_home_shells(id, cx);
        }
        if note_steps {
            self.note_steps(id, Instant::now(), window, cx);
        }
        heard
    }

    /// Home refused something, or the client did: its sentence is shown; what it was about goes
    /// back to how home has it.
    fn refused(
        &mut self,
        id: &str,
        about: About,
        sentence: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.warn(super::bus::clean_to(sentence, 200), window, cx);
        if let About::Move(block) = about
            && let Some(space) = self.space_by_id_mut(id)
            && let Some(mut remote) = space.remote.take()
        {
            if let Some(local) = remote.homes.get(&block).copied() {
                remote.held.end(&local);
                if let (Some(rect), Some(place)) = (remote.rects.get(&local), space.home_mut(local))
                {
                    *place = *rect;
                }
            }
            space.remote = Some(remote);
        }
    }

    /// Why `deed` can't be done on the workspace shown, in words: None on a local workspace, and
    /// when the mirror can do it.
    pub(crate) fn why_not(&self, deed: Deed) -> Option<String> {
        let remote = self.space.remote.as_deref()?;
        let nay = remote.why_not(deed)?;
        Some(nay.words(&crate::mirror::computer_of(remote)))
    }

    /// The blocks `ids` of the remote shown were moved or resized here (a drag let go, Ctrl+Z):
    /// home moves each one (a `move_block` per block), and the rectangle left here stands until
    /// home's state shows it or [`holds::HOLD`] passes. With the remote not live nothing is sent and
    /// they go back where home has them.
    pub(super) fn remote_moved(&mut self, ids: &[BlockId], now: Instant) {
        let Some(mut remote) = self.space.remote.take() else {
            return;
        };
        let live = remote.why_not(Deed::Move).is_none();
        for id in ids {
            // A tile's place on the canvas is what moved (Ctrl+Z in grid mode).
            let (Some(place), Some(home)) = (self.space.home_mut(*id), remote.home_of(*id)) else {
                continue;
            };
            let Some(last) = remote.rects.get(id).copied() else {
                continue;
            };
            // What home has, as far as this knows: a move sent and not shown yet counts.
            let sent = remote.held.get(id).copied().unwrap_or(last);
            if !live {
                *place = last;
                remote.held.end(id);
            } else if same_rect(*place, sent) {
                continue;
            } else {
                let r = *place;
                remote.held.hold(*id, r, now);
                let rect = templum_api::model::Rect {
                    x: r.x,
                    y: r.y,
                    w: r.w,
                    h: r.h,
                };
                self.mirror_send(&self.space.id, Command::Move { block: home, rect });
            }
        }
        self.space.remote = Some(remote);
    }

    /// Each frame: the rectangles held for [`holds::HOLD`] are home's again. The time the next one is
    /// due, if there is one (`stream_at` repaints then).
    pub(super) fn expire_holds(&mut self, now: Instant) -> Option<Instant> {
        let remote = self.space.remote.as_mut()?;
        let (ended, due) = remote
            .held
            .expire(now, |id| self.placed.iter().any(|(p, _)| p == id));
        let ended: Vec<(BlockId, Rect)> = ended
            .into_iter()
            .filter_map(|id| Some((id, *remote.rects.get(&id)?)))
            .collect();
        let (back, closed_due) = remote.closed.expire(now, |_| false);
        let (undone, wires_due) = remote.wires_held.expire(now, |_| false);
        let (moved, moved_due) = remote.cards_moved.expire(now, |_| false);
        let (deleted, deleted_due) = remote.cards_deleted.expire(now, |_| false);
        let (edited, edited_due) = remote.cards_edited.expire(now, |_| false);
        let (checked, checked_due) = remote.items_checked.expire(now, |_| false);
        let (typed, typed_due) = remote.items_edited.expire(now, |_| false);
        let (unlisted, unlisted_due) = remote.items_deleted.expire(now, |_| false);
        let (reordered, reordered_due) = remote.items_moved.expire(now, |_| false);
        let (renamed, renamed_due) = remote.lists_renamed.expire(now, |_| false);
        let (titled, titled_due) = remote.notes_renamed.expire(now, |_| false);
        let (tinted, tinted_due) = remote.notes_colored.expire(now, |_| false);
        let (shut, shut_due) = remote.tabs_closed.expire(now, |_| false);
        for (id, rect) in ended {
            if let Some(place) = self.space.home_mut(id) {
                *place = rect;
            }
        }
        // What was done here that home's state never showed ends: home's last state is drawn
        // again (a block closed here that home still lists comes back, a card moved here goes
        // where home has it).
        if !back.is_empty()
            || !undone.is_empty()
            || !moved.is_empty()
            || !deleted.is_empty()
            || !edited.is_empty()
            || !checked.is_empty()
            || !typed.is_empty()
            || !unlisted.is_empty()
            || !reordered.is_empty()
            || !renamed.is_empty()
            || !titled.is_empty()
            || !tinted.is_empty()
            || !shut.is_empty()
        {
            let id = self.space.id.clone();
            self.remirror(&id, now);
        }
        [
            due,
            closed_due,
            wires_due,
            moved_due,
            deleted_due,
            edited_due,
            checked_due,
            typed_due,
            unlisted_due,
            reordered_due,
            renamed_due,
            titled_due,
            tinted_due,
            shut_due,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// What `apply_remote` builds a mirrored canvas with.
    pub(super) fn viewers(&self) -> Viewers {
        Viewers {
            cell: self.cell.size,
            palette: self.theme.palette,
            options: self.config.term_options(),
            notify: self.notify.clone(),
        }
    }

    /// Home's last state of remote `id` is drawn again: a hold ended with no new state to
    /// end it, and home's state is the truth.
    pub(super) fn remirror(&mut self, id: &str, now: Instant) {
        let make = self.viewers();
        let dragged: HashSet<BlockId> = if self.space.id == id {
            self.placed.iter().map(|(id, _)| *id).collect()
        } else {
            HashSet::new()
        };
        let Some(space) = self.space_by_id_mut(id) else {
            return;
        };
        let Some(mut remote) = space.remote.take() else {
            return;
        };
        let state = remote.state.clone();
        let home = remote.home.as_ref().map(|h| h.id.clone());
        space.mirror_state(&mut remote, &state, home.as_deref(), &make, (now, &dragged));
        remote.mirror_boards(&state, now);
        remote.mirror_checklists(&state, now);
        remote.mirror_notes(&state, now);
        remote.end_closed_tabs(&state);
        // The editors are made where a window is; the tabs of a block brought back are laid here.
        space.lay_note_tabs(&remote, &state);
        space.remote = Some(remote);
    }

    /// The newest state of remote `id` into its workspace: home's blocks at home's rectangles,
    /// back to front, as no step of Ctrl+Z and nothing the mirror's stores or agents know of.
    pub(super) fn apply_remote(
        &mut self,
        id: &str,
        state: State,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewport = self.viewport(window);
        let now = Instant::now();
        let dragged: HashSet<BlockId> = if self.space.id == id {
            self.placed.iter().map(|(id, _)| *id).collect()
        } else {
            HashSet::new()
        };
        let make = self.viewers();
        let Some(space) = self.space_by_id_mut(id) else {
            return;
        };
        let Some(mut remote) = space.remote.take() else {
            return;
        };
        let home = state
            .workspaces
            .iter()
            .find(|w| Some(&w.id) == remote.entry.home.as_ref())
            .or(state.workspaces.first())
            .map(|w| WorkspaceView {
                name: super::bus::clean_title(&w.name),
                ..WorkspaceView::clone(w)
            });
        if let Some(home) = &home {
            if !home.name.is_empty() && space.name != home.name {
                space.name.clone_from(&home.name);
            }
            // The person's own look wins over home's.
            if remote.home_look || (space.look.icon.is_none() && space.look.color.is_none()) {
                space.look.icon = home.icon.as_deref().and_then(crate::space_dialog::icon);
                space.look.color = home
                    .color
                    .clone()
                    .filter(|c| crate::space_dialog::COLORS.contains(&c.as_str()));
                remote.home_look = true;
            }
        }
        // What the three depend on one another for, and what a change of cards, boards,
        // automations or shares alone doesn't touch: all or none.
        let changed = !same(&state.workspaces, &remote.state.workspaces)
            || !same(&state.blocks, &remote.state.blocks)
            || !same(&state.wires, &remote.state.wires)
            || !same(&state.requests, &remote.state.requests);
        if changed {
            let home_id = home.as_ref().map(|h| h.id.as_str());
            space.mirror_state(&mut remote, &state, home_id, &make, (now, &dragged));
        }
        if !remote.placed && home.is_some() {
            remote.placed = true;
            let mut camera = space.canvas.camera;
            match state
                .view
                .as_ref()
                .filter(|v| Some(&v.workspace) == home.as_ref().map(|h| &h.id))
            {
                Some(v) if v.x.abs() <= MOST_FAR && v.y.abs() <= MOST_FAR => {
                    camera = Camera::at(Point::new(v.x, v.y), v.zoom);
                }
                _ => {
                    if let Some(bounds) = space.canvas.bounds() {
                        camera.fit(bounds, viewport);
                    }
                }
            }
            space.canvas.camera = camera;
            space.view = camera;
            space.flight = None;
        }
        // The boards' copy depends on the cards alone (and on the state it replaces, for the
        // cards home didn't change).
        if !same(&state.boards, &remote.state.boards) || !same(&state.cards, &remote.state.cards) {
            remote.mirror_boards(&state, now);
        }
        let notes_changed = changed || !same(&state.notes, &remote.state.notes);
        let made = if same(&state.notes, &remote.state.notes) {
            HashSet::new()
        } else {
            remote.mirror_notes(&state, now)
        };
        if !same(&state.checklists, &remote.state.checklists) {
            remote.mirror_checklists(&state, now);
        }
        if !same(&state.drawings, &remote.state.drawings)
            || !same(&state.workspaces, &remote.state.workspaces)
        {
            space.mirror_drawings(&state, home.as_ref().map(|h| h.id.as_str()));
        }
        let home_id = home.as_ref().map(|h| h.id.clone());
        let rules_changed = changed || !same(&state.automations, &remote.state.automations);
        if rules_changed {
            remote.mirror_rules(id, &state, home_id.as_deref());
        }
        if !same(&state.schedules, &remote.state.schedules) {
            remote.mirror_schedules(&state);
        }
        remote.home = home;
        let requests = changed.then(|| state.requests.clone());
        remote.state = state;
        space.remote = Some(remote);
        if notes_changed {
            self.remote_notes(id, &made, window, cx);
        }
        if let Some(requests) = requests {
            self.remote_requests(id, home_id.as_deref(), &requests, window, cx);
        }
        if rules_changed {
            self.reload_home_form(id);
        }
        self.focus_awaited(id, now, window, cx);
    }

    /// What the person pasted into the Connect dialog becomes a remote workspace, hidden until it
    /// is live. Its key goes to the client to be kept (`KeySource::Save`); the index keeps only the
    /// entry's name. Answers the new workspace's id, or what the dialog should say.
    pub(crate) fn connect_code(
        &mut self,
        draft: Connecting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let (mut entry, secrets, file, code) = prepared(draft)?;
        // An Access Client ID alone is no token.
        if secrets.access_secret.is_none() {
            entry.access_id = None;
        }
        let id = self.remote_id();
        let mut space = RemoteSpace::new(entry);
        space.started = true;
        let remote = space.remote(KeySource::Save {
            entry: space.entry.entry.clone(),
            secrets,
            file,
            from: None,
        });
        self.connect_clip = Some(code::encode(
            &code.url,
            &code.key,
            match &code.trust {
                Trust::Pinned(pin) => Some(pin.as_str()),
                _ => None,
            },
        ));
        self.others.push(Space {
            id: id.clone(),
            name: "Connecting…".into(),
            remote: Some(Box::new(space)),
            ..Space::default()
        });
        self.pending_connect = Some(id.clone());
        self.connect_done = None;
        self.mirror(window, cx).connect(&id, remote);
        self.write_index(window, cx);
        cx.notify();
        Ok(id)
    }

    /// A new code for remote `id`, whose share expired or was revoked, or whose certificate
    /// changed: the connection takes its address, way in, pin, proxy and Access token, and a key
    /// of its own; the old key is deleted once the new one is kept. Its blocks are replaced by
    /// home's new state as any state is.
    pub(crate) fn replace_code(
        &mut self,
        id: &str,
        draft: Connecting,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (fresh, secrets, file, code) = prepared(draft)?;
        let clip = code::encode(
            &code.url,
            &code.key,
            match &code.trust {
                Trust::Pinned(pin) => Some(pin.as_str()),
                _ => None,
            },
        );
        let Some(remote) = self.remote_mut(id) else {
            return Err("This workspace is gone.".into());
        };
        // Even an entry not kept yet: its Save may have been written before it was replaced.
        let old = &remote.entry;
        let kept_in_file = old.key == Keeping::File;
        let holder = holder(old, &remote.inherits);
        remote.old_keys.push((old.entry.clone(), kept_in_file));
        remote.inherits = holder.clone();
        let from = inherited(&fresh, &secrets, old, holder);
        remote.entry = RemoteEntry {
            // What the person chose of the look, and where the camera is, stay.
            camera: remote.entry.camera,
            ..fresh
        };
        remote.status = Status::Connecting;
        remote.since = Instant::now();
        remote.started = true;
        remote.me = None;
        remote.tools = None;
        remote.latency = None;
        remote.told.clear();
        remote.awaited = None;
        remote.agents = None;
        remote.agents_at = None;
        remote.agents_asking = false;
        remote.shells = None;
        remote.shells_at = None;
        remote.shells_asking = false;
        remote.recent.clear();
        remote.requests_told.clear();
        remote.requests_budget = Budget::default();
        remote.notice_budget = Budget::default();
        remote.notices_folded = 0;
        let connection = remote.remote(KeySource::Save {
            entry: remote.entry.entry.clone(),
            secrets,
            file,
            from,
        });
        // The new home may number its requests as the old one did: none of them is the same.
        let stale: Vec<u64> = self
            .waits
            .iter()
            .filter(|w| w.remote.as_ref().is_some_and(|a| a.space == id))
            .map(|w| w.key)
            .collect();
        for key in stale {
            self.answer(key, templum_mcp::Reply::ok(""), cx);
        }
        self.connect_clip = Some(clip);
        self.mirror(window, cx).connect(id, connection);
        self.write_index(window, cx);
        cx.notify();
        Ok(())
    }

    /// The local look (icon and color) the person gave remote `id` in Edit…; with none, home's
    /// shows again. Its name is home's.
    pub(crate) fn edit_remote_look(
        &mut self,
        id: &str,
        look: crate::space_dialog::Look,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(space) = self.space_by_id_mut(id)
            && let Some(remote) = space.remote.as_deref_mut()
        {
            // Saving the look as it opened (home's) doesn't make it the person's own.
            let opened =
                remote.home_look && look.icon == space.look.icon && look.color == space.look.color;
            if !opened {
                space.look.icon = look.icon;
                space.look.color = look.color.clone();
                // The person's own look wins over home's while they have one.
                remote.home_look = look.icon.is_none() && look.color.is_none();
            }
        }
        self.write_index(window, cx);
        cx.notify();
    }

    /// How the Connect dialog's remote went: None while it tries; Ok once it is live; the words
    /// of what stopped it otherwise (the remote is forgotten then).
    pub(crate) fn connect_result(&self) -> Option<Result<(), String>> {
        self.connect_done.clone()
    }

    /// Remote `id`, the Connect dialog's, said how it is doing. A drain may bring several
    /// statuses: the first one that settles it is the answer.
    pub(super) fn connect_moved(
        &mut self,
        id: &str,
        status: &Status,
        words: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_connect.as_deref() != Some(id) {
            return;
        }
        match status {
            Status::Live => {
                self.pending_connect = None;
                self.connect_done = Some(Ok(()));
                if let Some(at) = self.order().iter().position(|s| s.id == id) {
                    self.switch_to(at, window, cx);
                }
                // The code is a key: it doesn't wait in the clipboard.
                let held = cx.read_from_clipboard().and_then(|item| item.text());
                if held.is_some_and(|text| Some(text.trim()) == self.connect_clip.as_deref()) {
                    cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
                }
                self.connect_clip = None;
                self.write_index(window, cx);
            }
            // A certificate from an added authority waits for the person's choice.
            Status::Cut(Why::AddedAuthority) => {}
            Status::Reconnecting { .. } | Status::Cut(_) => {
                self.give_up_connecting(id, window, cx);
                self.connect_done = Some(Err(words));
            }
            Status::Connecting | Status::Disconnected => {}
        }
    }

    /// What a banner button of remote `id` does.
    pub(crate) fn remote_act(
        &mut self,
        id: &str,
        act: Act,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = self.pending_connect.as_deref() == Some(id);
        let Some(remote) = self.remote_mut(id) else {
            return;
        };
        let started = remote.started;
        match act {
            // Nothing was ever started for it this run: only a new code can.
            Act::Connect if !started => crate::connect_dialog::open_for(self, id, window, cx),
            Act::TryNow | Act::Connect | Act::ConnectHere => {
                self.mirror_send(id, Command::TryNow);
            }
            Act::Paste => crate::connect_dialog::open_for(self, id, window, cx),
            Act::Forget => crate::mirror::forget_dialog(self, id, window, cx),
            Act::StartIt => {
                if let Some(nay) = remote.why_not(Deed::Start) {
                    let words = nay.words(&crate::mirror::computer_of(remote));
                    return self.warn(words, window, cx);
                }
                let Some(workspace) = remote.entry.home.clone() else {
                    return self.warn("Not connected.".into(), window, cx);
                };
                let call = Call {
                    tool: "show".into(),
                    args: serde_json::json!({ "workspace": workspace }),
                    slow: false,
                };
                // A refusal for a remote forgotten meanwhile is not said.
                let space = id.to_string();
                let then: Then = Box::new(move |ws, answered, window, cx| {
                    if ws.space_named(&space).is_some() {
                        Self::warn_refusal(ws, answered, window, cx);
                    }
                });
                self.remote_call(id, call, then, cx);
            }
            Act::DontConnect if pending => {
                self.give_up_connecting(id, window, cx);
                self.connect_done = Some(Err("Not connected.".into()));
            }
            Act::DontConnect => self.mirror_send(id, Command::Disconnect),
            Act::ConnectAnyway => {
                // Kept for this remote only; the client keeps the key that way and starts again.
                remote.entry.trust_added_authority = true;
                self.mirror_send(id, Command::TrustAdded);
                self.write_index(window, cx);
            }
        }
        cx.notify();
    }

    /// Disconnect (or Connect, when it is disconnected) from the tile's menu and the banner.
    pub(crate) fn toggle_remote(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let live = self
            .spaces()
            .find(|s| s.id == id)
            .and_then(|s| s.remote.as_deref())
            .is_some_and(|r| matches!(r.status, Status::Live | Status::Reconnecting { .. }));
        let act = if live { Act::DontConnect } else { Act::Connect };
        self.remote_act(id, act, window, cx);
    }

    /// Forget remote `id`: its share is revoked at home first when `revoke`, its key deleted, its
    /// connections ended and its workspace removed.
    pub(crate) fn forget_remote(
        &mut self,
        id: &str,
        revoke: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_connect.as_deref() == Some(id) {
            self.pending_connect = None;
        }
        self.delete_space_as(id, revoke, window, cx);
    }

    /// What the Connect dialog's remote asks of the person, while it waits for a choice: a
    /// certificate from an authority this computer added.
    pub(crate) fn connect_choice(&self) -> Option<String> {
        let id = self.pending_connect.as_deref()?;
        let remote = self.spaces().find(|s| s.id == id)?.remote.as_deref()?;
        matches!(remote.status, Status::Cut(Why::AddedAuthority))
            .then(|| banner_words(remote, Instant::now(), SystemTime::now()).text)
    }

    /// The Connect dialog is over without a live remote: it is forgotten, key and all.
    pub(crate) fn give_up_connecting(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_connect = None;
        self.connect_clip = None;
        self.delete_space(id, window, cx);
    }

    /// The dialog was closed before the remote was live.
    pub(crate) fn cancel_connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.pending_connect.clone() {
            self.give_up_connecting(&id, window, cx);
        }
        self.connect_done = None;
    }

    /// Connect to a Templum…: the dialog that turns a code into a remote workspace.
    pub(crate) fn connect_templum(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::connect_dialog::open(self, window, cx);
    }

    /// Asks whether the system's keyring answers; `keyring` holds the answer when it comes.
    pub(crate) fn probe_keyring(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mirror(window, cx).probe();
    }

    /// Chooses which terminals of each remote workspace the client streams: the ones on screen at
    /// Full detail of the workspace shown while it is live, nearest the canvas's center first,
    /// and always the focused one; none for a workspace not shown or not live. Called as the
    /// canvas is drawn, it chooses at most every [`STREAM_EVERY`] and only tells the client when
    /// the set changed. The time a choice waits for, if one does (`stream_at` repaints then).
    pub(super) fn stream_terminals(&mut self, now: Instant) -> Option<Instant> {
        let mirror = self.mirror.as_ref()?;
        let mut due: Option<Instant> = None;
        let vp = self.api.viewport;
        let font = self.cell.font_size;
        let full = templum_canvas::level(font, self.space.rest) == templum_canvas::Level::Full;
        let shown = std::iter::once((true, &mut self.space))
            .chain(self.others.iter_mut().map(|s| (false, s)));
        for (shown, space) in shown {
            let Some(mut remote) = space.remote.take() else {
                continue;
            };
            let live = shown && remote.status == Status::Live;
            let mut set: Vec<(String, BlockId)> = Vec::new();
            if live {
                if let Some(at) = remote
                    .chosen
                    .filter(|at| now.saturating_duration_since(*at) < STREAM_EVERY)
                {
                    due = Some(due.map_or(at + STREAM_EVERY, |d| d.min(at + STREAM_EVERY)));
                    space.remote = Some(remote);
                    continue;
                }
                remote.chosen = Some(now);
                set = space.streamed_set(&remote, vp, full);
            }
            let mut same: Vec<_> = set.clone();
            same.sort();
            if same != remote.streaming {
                remote.streaming = same;
                let feeders = set
                    .into_iter()
                    .filter_map(|(home, id)| Some((home, space.terminal(id)?.term.feeder())))
                    .collect();
                mirror.send(&space.id, Command::Terminals(feeders));
            }
            space.remote = Some(remote);
        }
        due
    }

    /// The remote shown is live, with its blocks, and the client streams every terminal the
    /// canvas draws in full and in view (`--bench-mirror` waits for it).
    pub(crate) fn mirror_streams_all(&self) -> bool {
        let Some(remote) = self.space.remote.as_deref() else {
            return false;
        };
        if remote.status != Status::Live || remote.homes.is_empty() {
            return false;
        }
        let full = templum_canvas::level(self.cell.font_size, self.space.rest)
            == templum_canvas::Level::Full;
        let mut want = self.space.streamed_set(remote, self.api.viewport, full);
        want.sort();
        !want.is_empty() && want == remote.streaming
    }

    /// Repaints at `due` to make the stream choice that waits, one timer at a time: the canvas
    /// is drawn for the output's rate, not for a choice that only needs a moment.
    pub(super) fn stream_at(&mut self, due: Instant, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.stream_waiting, true) {
            return;
        }
        cx.spawn_in(window, async move |ws, cx| {
            cx.background_executor()
                .timer(due.saturating_duration_since(Instant::now()))
                .await;
            ws.update(cx, |ws, cx| {
                ws.stream_waiting = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The focused terminal of the remote shown, when what is typed can reach it: see
    /// [`Workspace::typing_into`].
    fn typing_at(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(String, String)> {
        let id = self.space.focused?;
        self.typing_into(id, window, cx)
    }

    /// Scrolling back in remote terminal `id`: the client reads home's history into its viewer
    /// (`Command::History`, which decides whether it needs reading), unless a full-screen program
    /// or one that took the mouse has the screen, or the rule says no. The UI thread never holds
    /// the text: it hears `Update::History` when it is there.
    pub(crate) fn remote_history(&self, id: BlockId) {
        let Some(pane) = self.space.terminal(id) else {
            return;
        };
        let Some(block) = pane.remote.as_ref().map(|r| r.block.clone()) else {
            return;
        };
        let modes = pane.term.shared().modes();
        let allowed = self
            .space
            .remote
            .as_deref()
            .is_some_and(|remote| remote.why_not(Deed::ScrollBack).is_none());
        if !modes.full_screen && !modes.mouse && allowed {
            self.mirror_send(&self.space.id, Command::History { block });
        }
    }

    /// The wheel moved remote terminal `id` by `lines` (up is positive) and its program took no
    /// mouse: a full-screen program is sent arrow keys, as typing; else the viewer scrolls through
    /// the history it has, and home's is read when the person starts to go back.
    pub(crate) fn remote_scroll(
        &mut self,
        id: BlockId,
        lines: i32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.space.terminal(id) else {
            return;
        };
        if pane.term.shared().modes().full_screen {
            if let Some((space, block)) = self.typing_into(id, window, cx) {
                let key = if lines > 0 { "up" } else { "down" };
                let keys = vec![key.to_string(); lines.unsigned_abs() as usize];
                self.mirror_send(&space, Command::Keys { block, keys });
            }
            return;
        }
        if lines > 0 && !pane.term.scrolled_back() {
            self.remote_history(id);
        }
        pane.scrolled.set(true);
        pane.term.scroll(lines);
    }

    /// Shift+PageUp or Shift+PageDown in the focused remote terminal pages through the history
    /// the viewer has (home's is read when the person starts to go back). False: a program has
    /// the screen or the mouse, and the key goes home as any other.
    pub(super) fn remote_page(&mut self, up: bool) -> bool {
        let Some(id) = self.space.focused else {
            return false;
        };
        let Some(pane) = self.space.terminal(id) else {
            return false;
        };
        let modes = pane.term.shared().modes();
        if modes.full_screen || modes.mouse {
            return false;
        }
        let rows = i32::from(pane.term.shared().grid_size().1);
        if up && !pane.term.scrolled_back() {
            self.remote_history(id);
        }
        pane.scrolled.set(true);
        pane.term.scroll(if up { rows } else { -rows });
        true
    }

    /// Home's history of the block `block` is in its viewer: an open Find searches again.
    pub(super) fn history_came(&mut self, id: &str, block: &str, cx: &mut Context<Self>) {
        if self.space.id != id {
            return;
        }
        let viewer = self
            .space
            .remote
            .as_deref()
            .and_then(|remote| remote.homes.get(block).copied());
        if let Some(viewer) = viewer {
            self.search_again(viewer, cx);
        }
        cx.notify();
    }

    /// The remote terminal `id`, when what is typed can reach it: its workspace's id and home's
    /// id of the block. Else None, and the person is told why, once: the remote isn't live (keys
    /// are dropped, never queued, never sent late), or the share only controls.
    pub(crate) fn typing_into(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(String, String)> {
        let block = self
            .space
            .terminal(id)?
            .remote
            .as_ref()
            .filter(|r| !r.read_only)?
            .block
            .clone();
        let space = self.space.id.clone();
        let remote = self.space.remote.as_mut()?;
        let computer = crate::mirror::computer_of(remote);
        let (said, sentence) = match remote.why_not(Deed::Type) {
            None => return Some((space, block)),
            Some(Nay::NotConnected | Nay::NotYet | Nay::Drawing) => {
                (NOT_LIVE, NOT_CONNECTED.to_string())
            }
            Some(Nay::NeedsFull) => (
                NEEDS_FULL,
                format!("Typing needs a Full share: share again on {computer}."),
            ),
            Some(older @ Nay::Older) => (OLDER, older.words(&computer)),
        };
        if remote.told.insert((block, said)) {
            self.warn(sentence, window, cx);
        }
        None
    }

    /// The mouse event `input` for the program in remote terminal `id`: sent home when the program
    /// there asked for it (its modes, as the last frame painted them into the viewer), the pane
    /// isn't read-only and the rule lets the share. False: nothing was sent, and the person's
    /// press does what it does with no mouse program (select).
    pub(crate) fn remote_mouse(&self, id: BlockId, input: MouseInput) -> bool {
        let Some(pane) = self.space.terminal(id) else {
            return false;
        };
        let Some(block) = pane
            .remote
            .as_ref()
            .filter(|r| !r.read_only)
            .map(|r| &r.block)
        else {
            return false;
        };
        let allowed = self
            .space
            .remote
            .as_deref()
            .is_some_and(|remote| remote.why_not(Deed::Mouse).is_none());
        if !allowed || !pane.term.shared().modes().wants(&input) {
            return false;
        }
        self.mirror_send(
            &self.space.id,
            Command::Mouse {
                block: block.clone(),
                event: input,
            },
        );
        true
    }

    /// A press in remote terminal `id` whose program asked for the mouse, on a share that can't
    /// send it (or a home that can't take it): says why, once per terminal. The press then selects
    /// text, as it does with a mouse program under Shift.
    pub(super) fn tell_mouse(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.space.terminal(id) else {
            return;
        };
        let Some(block) = pane
            .remote
            .as_ref()
            .filter(|r| !r.read_only)
            .map(|r| r.block.clone())
        else {
            return;
        };
        if !pane.term.shared().modes().mouse {
            return;
        }
        let Some(remote) = self.space.remote.as_mut() else {
            return;
        };
        let computer = crate::mirror::computer_of(remote);
        let (said, sentence) = match remote.why_not(Deed::Mouse) {
            Some(Nay::NeedsFull) => (
                MOUSE_NEEDS_FULL,
                format!(
                    "Using the mouse in a terminal needs a Full share: share again on {computer}."
                ),
            ),
            Some(older @ Nay::Older) => (OLDER, older.words(&computer)),
            // Not live: the banner and the veil say it.
            _ => return,
        };
        if remote.told.insert((block, said)) {
            self.warn(sentence, window, cx);
        }
    }

    /// A warning about a mirrored workspace, in words the person reads.
    pub(crate) fn warn(&mut self, sentence: String, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(test)]
        self.said.push(sentence.clone());
        show_notification(window, cx, Notification::warning(sentence));
    }

    /// Whether the workspace `space` is the one shown. A dialog acts on the workspace it was
    /// opened on only: block ids count from 0 in every workspace, so another one shown by the
    /// time it is confirmed (a `show`, a notification) would take them for its own. When it
    /// isn't, this says so and the dialog does nothing.
    pub(crate) fn still_shown(
        &mut self,
        space: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let shown = self.space.id == space;
        if !shown {
            self.warn(
                "Another workspace is shown now: nothing was done.".into(),
                window,
                cx,
            );
        }
        shown
    }

    /// Whether a dialog of workspace `space` may act: home's ids and this computer's are alike, so
    /// with a mirror in it (the dialog's, or the one shown) it acts on its own workspace only,
    /// shown (`still_shown`).
    pub(crate) fn still_on(
        &mut self,
        space: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mirrored = self.on_remote() || self.space_named(space).is_none_or(|s| s.is_remote());
        !mirrored || self.still_shown(space, window, cx)
    }

    /// `keys` (names, `templum_terminal::key_name`) typed into the focused remote terminal: home
    /// presses them as its own keyboard does.
    pub(super) fn remote_type(
        &mut self,
        keys: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if keys.is_empty() {
            return;
        }
        if let Some((space, block)) = self.typing_at(window, cx) {
            self.to_bottom();
            self.mirror_send(&space, Command::Keys { block, keys });
        }
    }

    /// The focused terminal shows its live screen again, as typing in a local one does.
    fn to_bottom(&self) {
        if let Some(pane) = self.space.focused_terminal()
            && pane.term.scrolled_back()
        {
            pane.term.scroll(-(templum_terminal::VIEWER_HISTORY as i32));
        }
    }

    /// `text` pasted into the focused remote terminal; home brackets it when the program asked.
    pub(super) fn remote_paste(
        &mut self,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((space, block)) = self.typing_at(window, cx) else {
            return;
        };
        if text.len() > templum_mcp::tools::MAX_TEXT {
            self.warn(
                "Paste at most 256 KiB into a mirrored terminal.".into(),
                window,
                cx,
            );
            return;
        }
        self.to_bottom();
        self.mirror_send(&space, Command::Paste { block, text });
    }

    /// Why the mirrored request `wait` can't be answered now, if so: its remote isn't live, or its
    /// share can't answer what it asks (a worktree's request needs a Full share). None for a
    /// request of this computer's.
    pub(crate) fn remote_blocked(&self, wait: &Wait) -> Option<String> {
        let ask = wait.remote.as_ref()?;
        let remote = self.space_named(&ask.space)?.remote.as_deref()?;
        let nay = remote.why_not(Deed::Answer(ask.scope))?;
        Some(nay.words(&crate::mirror::computer_of(remote)))
    }

    /// The answer `args` to the mirrored request `key` goes to its home (`answer_request`); the
    /// request leaves the inbox until home's state drops it, or comes back if home refuses.
    /// Nothing is sent for one that can't be answered now, or already was.
    pub(super) fn remote_answer(
        &mut self,
        key: u64,
        args: serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        let Some(wait) = self.waits.iter().find(|w| w.key == key) else {
            return;
        };
        if self.remote_blocked(wait).is_some() {
            return;
        }
        let Some(ask) = wait.remote.as_ref().filter(|a| !a.sent) else {
            return;
        };
        let (space, request) = (ask.space.clone(), ask.request.clone());
        let mut fields = match args {
            serde_json::Value::Object(fields) => fields,
            _ => serde_json::Map::new(),
        };
        fields.insert("request".into(), request.into());
        let call = Call {
            tool: "answer_request".into(),
            args: serde_json::Value::Object(fields),
            slow: false,
        };
        // Home refused: the request shows in the inbox again, and its words are said. One that
        // left the inbox meanwhile (home's state dropped it, or its remote was forgotten) is not
        // there to show again, and its words are not said: nobody waits for them.
        let then: Then = Box::new(move |ws, answered, window, cx| {
            if answered.is_ok() {
                return;
            }
            let Some(ask) = ws
                .waits
                .iter_mut()
                .find(|w| w.key == key)
                .and_then(|w| w.remote.as_mut())
            else {
                return;
            };
            ask.sent = false;
            Self::warn_refusal(ws, answered, window, cx);
            cx.notify();
        });
        self.remote_call(&space, call, then, cx);
        if let Some(ask) = self
            .waits
            .iter_mut()
            .find(|w| w.key == key)
            .and_then(|w| w.remote.as_mut())
        {
            ask.sent = true;
        }
        self.inbox_settle(&[key]);
        cx.notify();
    }

    /// Home's requests of the remote workspace `id` (`home` is its workspace there) become the
    /// inbox's waits: each new one a wait marked remote that asks, chimes and notifies as a local
    /// one does; one home no longer lists is withdrawn without an answer. Answered at home, or by
    /// another mirror, it just leaves. Bounded: what home sends is cleaned and capped.
    pub(super) fn remote_requests(
        &mut self,
        id: &str,
        home: Option<&str>,
        requests: &[Arc<RequestView>],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut wanted: Vec<&Arc<RequestView>> = requests
            .iter()
            .filter(|r| r.workspace.as_deref() == home && home.is_some())
            .filter(|r| r.id.len() <= 32 && !r.id.chars().any(char::is_control))
            .filter(|r| {
                matches!(
                    r.kind.as_str(),
                    "question" | "plan" | "connect" | "remove_worktree" | "merge_worktree"
                )
            })
            .collect();
        // The inbox says how many more home has than it lists.
        let more = wanted.len().saturating_sub(MOST_REQUESTS);
        wanted.truncate(MOST_REQUESTS);
        if let Some(remote) = self.remote_mut(id) {
            remote.requests_more = more;
        }
        let gone: Vec<u64> = self
            .waits
            .iter()
            .filter_map(|w| {
                let ask = w.remote.as_ref().filter(|a| a.space == id)?;
                (!wanted.iter().any(|r| r.id == ask.request)).then_some(w.key)
            })
            .collect();
        for key in gone {
            // Its asker is another computer's agent: there is no reply to send.
            self.answer(key, templum_mcp::Reply::ok(""), cx);
        }
        let mut folded = Vec::new();
        for request in wanted {
            let known = self.waits.iter().position(|w| {
                w.remote
                    .as_ref()
                    .is_some_and(|a| a.space == id && a.request == request.id)
            });
            match known {
                None => self.remote_wait(id, request, &mut folded, window, cx),
                // Listed while its block had no viewer: it has one now.
                Some(at) if self.waits[at].from.is_empty() => {
                    self.waits[at].from = self.viewer_terminal(id, request);
                }
                Some(_) => {}
            }
        }
        if let Some(last) = folded.last().filter(|_| self.fold_tells(id)) {
            self.tell_folded(*last, folded.len(), window, cx);
        }
    }

    /// Whether the person is notified of home's request `request` of remote `id`: once per id
    /// per connection, and at most [`MOST_NOTICES`] a [`NOTICE_SPAN`].
    fn remote_tells(&mut self, id: &str, request: &str) -> Notice {
        let Some(remote) = self.remote_mut(id) else {
            return Notice::Again;
        };
        if remote.requests_told.len() >= MOST_TOLD {
            remote.requests_told.clear();
        }
        if !remote.requests_told.insert(request.to_string()) {
            return Notice::Again;
        }
        if remote.requests_budget.tell(Instant::now()) {
            Notice::Tell
        } else {
            Notice::Fold
        }
    }

    /// Whether the notice of how many more requests ask is given now: at most one a
    /// [`NOTICE_SPAN`], so a home that makes a new request every state can't flood the person.
    fn fold_tells(&mut self, id: &str) -> bool {
        self.remote_mut(id)
            .is_some_and(|remote| remote.requests_budget.fold(Instant::now()))
    }

    /// The terminal of the viewer here of the block home's `request` is from; empty when it has
    /// no viewer (yet).
    fn viewer_terminal(&self, id: &str, request: &RequestView) -> String {
        let Some(space) = self.space_named(id) else {
            return String::new();
        };
        request
            .block
            .as_ref()
            .and_then(|block| space.remote.as_deref()?.homes.get(block))
            .and_then(|local| space.terminal(*local))
            .map(|pane| pane.term.id().to_string())
            .unwrap_or_default()
    }

    /// A new request of home's, a wait here. Listed from no terminal when its block has no viewer
    /// here (`remote_requests` fills it in when one comes); skipped when it asks nothing.
    fn remote_wait(
        &mut self,
        id: &str,
        request: &RequestView,
        folded: &mut Vec<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.space_named(id).is_none() {
            return;
        }
        let from = self.viewer_terminal(id, request);
        let Some(what) = waiting_of(request) else {
            return;
        };
        let notice = self.remote_tells(id, &request.id);
        let quiet = notice != Notice::Tell;
        // Nobody listens for the reply: home's agent is answered at home.
        let (reply, _) = std::sync::mpsc::channel();
        let asked = Asked {
            conn: u64::MAX,
            id: serde_json::Value::Null,
            reply,
            from,
        };
        let remote = RemoteAsk {
            space: id.to_string(),
            request: request.id.clone(),
            scope: request.scope,
            sent: false,
            quiet,
        };
        self.wait_as(asked, what, None, Some(remote), window, cx);
        // It has waited as long as home says.
        let (now, wall) = (
            cx.background_executor().now(),
            super::automations::unix(SystemTime::now()),
        );
        if let Some(wait) = self.waits.iter_mut().find(|w| w.key == self.next_wait) {
            wait.since = aged(request.since, now, wall);
        }
        if notice == Notice::Fold {
            folded.push(self.next_wait);
        }
    }

    /// A notice of home's about its workspace (a task's end, a watch, an agent that waits): a
    /// notification titled by the workspace and the computer, whose click shows the block it is
    /// about or else the workspace. At most [`MOST_NOTICES`] a [`NOTICE_SPAN`]; the rest are
    /// counted for `fold_notices`. One about another workspace of home's is not this one's.
    pub(super) fn remote_notice(
        &mut self,
        id: &str,
        notice: templum_api::model::Notice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(space) = self.space_named(id) else {
            return;
        };
        let Some(remote) = space.remote.as_deref() else {
            return;
        };
        if notice.workspace.is_none() || notice.workspace != remote.entry.home {
            return;
        }
        let title = format!("{} on {}", space.name, crate::mirror::computer_of(remote));
        let text = super::bus::clean_to(&notice.text, templum_api::client::NOTICE_CHARS);
        if !self
            .remote_mut(id)
            .is_some_and(|remote| remote.notice_budget.tell(Instant::now()))
        {
            if let Some(remote) = self.remote_mut(id) {
                remote.notices_folded += 1;
            }
            return;
        }
        #[cfg(test)]
        self.said.push(format!("{title}: {text}"));
        let (ws, space, block) = (cx.entity().downgrade(), id.to_string(), notice.block);
        let automation = notice.automation;
        let notification = Notification::new()
            .title(title)
            .message(text)
            .delivery(self.delivery(window))
            .on_click(move |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.open_home_notice(&space, block.as_deref(), automation, window, cx)
                })
                .ok();
            });
        show_notification(window, cx, notification);
    }

    /// The click on a notice of home's: the Automations popup at its rule, else the block it is
    /// about, else the workspace.
    pub(crate) fn open_home_notice(
        &mut self,
        space: &str,
        block: Option<&str>,
        automation: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(key) = automation.and_then(|id| super::remote_rules::home_key(space, id)) {
            return self.show_automation(key, window, cx);
        }
        let local = block.and_then(|block| {
            self.space_named(space)?
                .remote
                .as_deref()?
                .homes
                .get(block)
                .copied()
        });
        match local {
            Some(local) => self.reveal(space, local, window, cx),
            None => {
                if let Some(to) = self.order().iter().position(|s| s.id == space) {
                    self.switch_to(to, window, cx);
                }
            }
        }
    }

    /// The notices past the budget become one notification of how many more, at most one a
    /// [`NOTICE_SPAN`].
    pub(super) fn fold_notices(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(space) = self.space_named(id) else {
            return;
        };
        let Some(remote) = space.remote.as_deref().filter(|r| r.notices_folded > 0) else {
            return;
        };
        let (title, computer, count) = (
            format!("{} on {}", space.name, crate::mirror::computer_of(remote)),
            crate::mirror::computer_of(remote),
            remote.notices_folded,
        );
        if !self
            .remote_mut(id)
            .is_some_and(|remote| remote.notice_budget.fold(Instant::now()))
        {
            return;
        }
        if let Some(remote) = self.remote_mut(id) {
            remote.notices_folded = 0;
        }
        let text = match count {
            1 => format!("1 more notice from {computer}"),
            n => format!("{n} more notices from {computer}"),
        };
        #[cfg(test)]
        self.said.push(format!("{title}: {text}"));
        show_notification(
            window,
            cx,
            Notification::new()
                .title(title)
                .message(text)
                .delivery(self.delivery(window)),
        );
    }

    /// Tells remote `id` something; nothing without a client.
    pub(crate) fn mirror_send(&self, id: &str, command: Command) {
        if let Some(mirror) = &self.mirror {
            mirror.send(id, command);
        }
    }

    /// Calls a tool of remote `space`'s home. `then` runs on the UI thread with home's answer,
    /// when it comes: the UI never waits for it, and a refusal's sentence is cleaned first. With
    /// no client the answer is "Not connected." (tests keep the call in `calls` to answer it).
    pub(crate) fn remote_call(
        &mut self,
        space: &str,
        call: Call,
        then: Then,
        cx: &mut Context<Self>,
    ) {
        let Some(mirror) = &self.mirror else {
            #[cfg(test)]
            return self.calls.push((space.into(), call, then));
            #[cfg(not(test))]
            return self.answered_later(
                std::future::ready(Err(client::Refusal::of("Not connected."))),
                then,
                cx,
            );
        };
        let answer = mirror.call(space, call);
        self.answered_later(answer, then, cx);
    }

    fn answered_later(
        &mut self,
        answer: impl std::future::Future<Output = Answered> + 'static,
        then: Then,
        cx: &mut Context<Self>,
    ) {
        let window = self.window_handle;
        cx.spawn(async move |ws, cx| {
            let mut answered = answer.await;
            answered = answered.map_err(clean_refusal);
            cx.update_window(window, |_, window, cx| {
                ws.update(cx, |ws, cx| then(ws, answered, window, cx)).ok();
            })
            .ok();
        })
        .detach();
    }

    /// The person adds a block of `kind` with its top-left corner at `at` on the remote shown:
    /// home adds it (`add_block`), where it places it itself when its Templum can't place one. Nothing
    /// is added here; the block comes with home's state, and takes the keyboard (`awaiting`).
    pub(super) fn remote_add(&mut self, kind: Kind, at: Point, cx: &mut Context<Self>) {
        self.remote_add_in(kind, at, None, None, cx);
    }

    /// On the remote shown, a terminal of home's running its shell `shell` (a name of its
    /// `list_shells`; None: its default) with its top-left corner at `at`, else where a new
    /// terminal goes: home opens it (`remote_add_in`).
    pub(crate) fn open_home_shell(
        &mut self,
        shell: Option<String>,
        at: Option<Point>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let deed = match shell {
            Some(_) => Deed::AddShell,
            None => Deed::AddTerminal,
        };
        if self.why_not(deed).is_some() {
            return;
        }
        let at = at.unwrap_or_else(|| self.new_terminal_rect(window, cx).origin());
        self.remote_add_in(Kind::Terminal, at, None, shell, cx);
    }

    /// `remote_add` in `folder` at home (a terminal's or an editor's), the workspace's when None;
    /// a terminal runs home's shell `shell`, its default when None.
    pub(crate) fn remote_add_in(
        &mut self,
        kind: Kind,
        at: Point,
        folder: Option<String>,
        shell: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let (deed, name) = match kind {
            Kind::Terminal if shell.is_some() => (Deed::AddShell, "terminal"),
            Kind::Terminal => (Deed::AddTerminal, "terminal"),
            Kind::Kanban => (Deed::AddKanban, "kanban"),
            Kind::Checklist => (Deed::AddChecklist, "checklist"),
            Kind::Note => (Deed::AddNote, "note"),
            Kind::Editor => return,
        };
        let Some(remote) = self.space.remote.as_deref() else {
            return;
        };
        let Some(workspace) = remote
            .home_workspace()
            .filter(|_| remote.why_not(deed).is_none())
        else {
            return;
        };
        let mut args = serde_json::json!({ "workspace": workspace, "kind": name });
        if remote.takes("add_block", "x") {
            args["x"] = at.x.into();
            args["y"] = at.y.into();
        }
        if kind == Kind::Checklist {
            args["text"] = crate::checklists::NAME.into();
        }
        if let Some(folder) = folder {
            args["folder"] = folder.into();
        }
        if let Some(shell) = shell {
            args["shell"] = shell.into();
        }
        let (space, before) = (self.space.id.clone(), self.space.focused);
        self.mark_remote();
        let call = Call {
            tool: "add_block".into(),
            args,
            slow: false,
        };
        let then: Then = Box::new(move |ws, answered, window, cx| match answered {
            Ok(answer) => ws.awaiting(&space, &answer, before, window, cx),
            Err(refusal) => ws.warn(refusal.sentence, window, cx),
        });
        let space = self.space.id.clone();
        self.remote_call(&space, call, then, cx);
    }

    /// Home answered `answer` to an add: its `block` is waited for, to take the keyboard when
    /// home's state brings it (or has already), unless the person moved the keyboard since.
    pub(super) fn awaiting(
        &mut self,
        space: &str,
        answer: &serde_json::Value,
        before: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(home) = answer["block"]
            .as_str()
            .filter(|b| b.len() <= 64 && !b.chars().any(char::is_control))
        else {
            return;
        };
        let Some(entry) = self.space_named(space) else {
            return;
        };
        if entry.focused != before {
            return;
        }
        let home = home.to_string();
        let Some(remote) = self.remote_mut(space) else {
            return;
        };
        let now = Instant::now();
        remote.awaited = Some(Awaited {
            home,
            before,
            at: now,
        });
        self.focus_awaited(space, now, window, cx);
    }

    /// The block home was asked to add takes the keyboard once it is on the canvas, if the remote
    /// is shown, the keyboard is where it was when asked (on the canvas, no dialog open) and
    /// [`KEYBOARD_WAIT`] has not passed; when one of those stops being so, it is not waited
    /// for anymore.
    pub(super) fn focus_awaited(
        &mut self,
        space: &str,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.space_named(space) else {
            return;
        };
        let Some(remote) = entry.remote.as_deref() else {
            return;
        };
        let Some(awaited) = &remote.awaited else {
            return;
        };
        let waiting = self.space.id == space
            && entry.focused == awaited.before
            && now.saturating_duration_since(awaited.at) <= KEYBOARD_WAIT
            && !window.has_active_dialog(cx)
            && self.canvas_has_keys(window);
        let block =
            (remote.homes.get(&awaited.home).copied()).filter(|id| entry.canvas.get(*id).is_some());
        let before = awaited.before;
        match (waiting, block) {
            (true, None) => return,
            (true, Some(id)) => {
                // In grid mode it takes half of the tile that had the keyboard, as a local one.
                if !self.space.shown(id) {
                    self.join_grid(id, before);
                }
                self.focus_block(id, window, cx);
            }
            (false, _) => {}
        }
        if let Some(remote) = self.remote_mut(space) {
            remote.awaited = None;
        }
    }

    /// Asks home which agents it can open (`list_agents`), unless that is under way: its answer
    /// is the list `open_menu` and the Open dialog use. A refusal says nothing.
    pub(crate) fn read_home_agents(&mut self, space: &str, cx: &mut Context<Self>) {
        let Some(remote) = self.remote_mut(space) else {
            return;
        };
        if std::mem::replace(&mut remote.agents_asking, true) {
            return;
        }
        let id = space.to_string();
        let then: Then = Box::new(move |ws, answered, _, _| {
            let Some(remote) = ws.remote_mut(&id) else {
                return;
            };
            remote.agents_asking = false;
            if let Ok(answer) = answered {
                remote.agents = Some(home_agents(answer["text"].as_str().unwrap_or_default()));
                remote.agents_at = Some(Instant::now());
            }
        });
        let call = Call {
            tool: "list_agents".into(),
            args: serde_json::json!({}),
            slow: false,
        };
        self.remote_call(space, call, then, cx);
    }

    /// Asks home which shells its terminals can run (`list_shells`), unless that is under way or
    /// home's Templum has no such tool: its answer is the list Add ▸ Terminal ▸ shows. A refusal
    /// says nothing.
    pub(crate) fn read_home_shells(&mut self, space: &str, cx: &mut Context<Self>) {
        let Some(remote) = self.remote_mut(space) else {
            return;
        };
        let lists = remote.tools.as_ref().is_some_and(|t| t.has("list_shells"));
        if !lists || std::mem::replace(&mut remote.shells_asking, true) {
            return;
        }
        let id = space.to_string();
        let then: Then = Box::new(move |ws, answered, _, _| {
            let Some(remote) = ws.remote_mut(&id) else {
                return;
            };
            remote.shells_asking = false;
            if let Ok(answer) = answered {
                remote.shells = Some(home_shells(answer["text"].as_str().unwrap_or_default()));
                remote.shells_at = Some(Instant::now());
            }
        });
        let call = Call {
            tool: "list_shells".into(),
            args: serde_json::json!({}),
            slow: false,
        };
        self.remote_call(space, call, then, cx);
    }

    /// Asks home for its recent folders of the workspace `space` mirrors (`recent_folders`),
    /// unless home's Templum has no such tool: the Git popup's folder menu lists them. A refusal
    /// says nothing.
    pub(crate) fn read_home_recent(&mut self, space: &str, cx: &mut Context<Self>) {
        let Some(remote) = self.remote_mut(space) else {
            return;
        };
        let lists = remote
            .tools
            .as_ref()
            .is_some_and(|t| t.has("recent_folders"));
        let Some(home) = remote.home_workspace().filter(|_| lists) else {
            return;
        };
        let id = space.to_string();
        let then: Then = Box::new(move |ws, answered, _, _| {
            if let (Some(remote), Ok(answer)) = (ws.remote_mut(&id), answered) {
                remote.recent = home_recent(answer["text"].as_str().unwrap_or_default());
            }
        });
        let call = Call {
            tool: "recent_folders".into(),
            args: serde_json::json!({ "workspace": home }),
            slow: false,
        };
        self.remote_call(space, call, then, cx);
    }

    /// The folders the Open dialog offers on the remote shown, as paths of home's computer:
    /// its workspace's folder first, then each folder of its terminals, each once.
    pub(crate) fn home_folders(&self) -> Vec<(String, Option<&'static str>)> {
        let Some(remote) = self.space.remote.as_deref() else {
            return Vec::new();
        };
        let Some(home) = &remote.home else {
            return Vec::new();
        };
        let mut terminals: Vec<&Arc<BlockView>> = remote
            .state
            .blocks
            .iter()
            .filter(|b| b.workspace == home.id && b.kind == "terminal")
            .collect();
        terminals.sort_by_key(|b| b.z);
        let own = home.folder.iter().map(|f| (f, Some("Workspace folder")));
        let theirs = terminals
            .iter()
            .filter_map(|b| b.folder.as_ref())
            .map(|f| (f, None));
        let mut seen = HashSet::new();
        own.chain(theirs)
            .map(|(path, tag)| (super::bus::clean_to(path, LINE_CHARS), tag))
            .filter(|(path, _)| !path.is_empty() && seen.insert(path.clone()))
            .take(MOST_FOLDERS + 1)
            .collect()
    }

    /// The person opens home's agent `agent` in `folder` (relative to home's workspace's folder,
    /// or whole; blank: its folder) with its block's top-left corner at `at`. Home opens it
    /// (`open_agent`) and `then` gets how it went: home's refusal, said in the dialog that
    /// asked. Nothing is opened here. Err at once when the remote can't do it now.
    pub(crate) fn remote_open_agent(
        &mut self,
        agent: &str,
        folder: &str,
        at: Point,
        then: impl FnOnce(&mut Workspace, Result<(), String>, &mut Window, &mut Context<Workspace>)
        + 'static,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(remote) = self.space.remote.as_deref() else {
            return Err("This workspace is gone.".into());
        };
        if let Some(nay) = remote.why_not(Deed::OpenAgent) {
            return Err(nay.words(&crate::mirror::computer_of(remote)));
        }
        let Some(workspace) = remote.home_workspace() else {
            return Err("This workspace is gone.".into());
        };
        let mut args = serde_json::json!({ "agent": agent, "workspace": workspace });
        if !folder.trim().is_empty() {
            args["folder"] = folder.trim().into();
        }
        if remote.takes("open_agent", "x") {
            args["x"] = at.x.into();
            args["y"] = at.y.into();
        }
        let (space, before) = (self.space.id.clone(), self.space.focused);
        self.mark_remote();
        let call = Call {
            tool: "open_agent".into(),
            args,
            slow: false,
        };
        let reply: Then = Box::new(move |ws, answered, window, cx| match answered {
            Ok(answer) => {
                // The Open dialog closes first: it doesn't keep the keyboard from the block.
                then(ws, Ok(()), window, cx);
                ws.awaiting(&space, &answer, before, window, cx);
            }
            Err(refusal) => then(ws, Err(refusal.sentence), window, cx),
        });
        let space = self.space.id.clone();
        self.remote_call(&space, call, reply, cx);
        Ok(())
    }

    /// The person closes `ids` of the remote shown (the header's ✕, Close in a menu, Ctrl+Shift+W):
    /// a terminal home shows an agent in asks first, as a local one does; the rest go at once.
    pub(super) fn remote_close(
        &mut self,
        ids: Vec<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.why_not(Deed::Close).is_some() {
            return;
        }
        // A note block whose text isn't at home yet closes once it is: that goes first.
        let space = self.space.id.clone();
        let mut ids = ids;
        ids.retain(|id| {
            !self.close_when_saved(&space, super::remote_notes::Closing::Block(*id), window, cx)
        });
        if ids.is_empty() {
            return;
        }
        let busy: Vec<String> = ids
            .iter()
            .filter_map(|id| {
                let pane = self.space.terminal(*id)?;
                let agent = pane.remote.as_ref()?.agent.as_ref()?;
                let why =
                    crate::alert::busy(&templum_terminal::Status::default(), Some(&agent.agent))?;
                Some(if ids.len() == 1 {
                    why
                } else {
                    format!("{}: {why}", pane.remote.as_ref()?.title)
                })
            })
            .collect();
        if busy.is_empty() {
            return self.close_remote_now(&ids, window, cx);
        }
        let title = if ids.len() == 1 {
            "Close the terminal?".to_string()
        } else {
            format!("Close {} blocks?", ids.len())
        };
        let (ws, space) = (cx.entity(), self.space.id.clone());
        crate::prompt::confirm(
            title,
            busy.join(" "),
            "Close",
            move |window, cx| {
                ws.update(cx, |ws, cx| {
                    if ws.still_shown(&space, window, cx) {
                        ws.close_remote_now(&ids, window, cx);
                    }
                })
            },
            window,
            cx,
        );
    }

    /// Home closes `ids` (`close_block` each); they leave this canvas at once, as a closed block
    /// fades, and stay closed here until home's state loses them. A refusal brings its block
    /// back, and is said once for all the blocks of this close.
    pub(super) fn close_remote_now(
        &mut self,
        ids: &[BlockId],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let space = self.space.id.clone();
        let said = std::rc::Rc::new(std::cell::Cell::new(false));
        let now = Instant::now();
        let mut sent = false;
        for id in ids {
            let Some(remote) = self.space.remote.as_mut() else {
                return;
            };
            let Some(block) = remote.home_of(*id) else {
                continue;
            };
            remote.closed.hold(*id, true, now);
            remote.rects.remove(id);
            remote.held.end(id);
            for key in remote.wires_held.keys() {
                if key.0 == *id || key.1 == *id {
                    remote.wires_held.end(&key);
                }
            }
            sent = true;
            let (id, said, space) = (*id, said.clone(), space.clone());
            let call = Call {
                tool: "close_block".into(),
                args: serde_json::json!({ "block": block }),
                slow: false,
            };
            let then: Then = Box::new(move |ws, answered, window, cx| {
                let Err(refusal) = answered else {
                    return;
                };
                if let Some(remote) = ws.remote_mut(&space) {
                    remote.closed.end(&id);
                }
                ws.remirror(&space, Instant::now());
                if !said.replace(true) {
                    ws.warn(refusal.sentence, window, cx);
                }
            });
            self.remote_call(&self.space.id.clone(), call, then, cx);
            if self.space.focused == Some(id) {
                self.space.focused = None;
            }
            self.remove_block(id, window, cx);
        }
        if sent {
            self.mark_remote();
        }
    }

    /// The person wires (`on`) or unwires `wire` on the remote shown: the wire shows at once and
    /// stands over home's state until home shows it or [`holds::HOLD`] passes; home makes or takes
    /// it (`connect`, `disconnect`), and a refusal puts it back as home has it. False when nothing
    /// is sent: it can't be done now, it already is so here, or an end isn't home's.
    pub(super) fn remote_wire(&mut self, wire: Wire, on: bool, cx: &mut Context<Self>) -> bool {
        let Some(remote) = self.space.remote.as_deref() else {
            return false;
        };
        let deed = if on { Deed::Wire } else { Deed::Unwire };
        let ends = remote.home_of(wire.a).zip(remote.home_of(wire.b));
        let Some((a, b)) = ends.filter(|_| remote.why_not(deed).is_none()) else {
            return false;
        };
        let key = (wire.a.min(wire.b), wire.a.max(wire.b));
        let done = if on {
            self.space.add_wire(wire.a, wire.b).is_some()
        } else {
            self.space.remove_wire(wire)
        };
        if !done {
            return false;
        }
        let space = self.space.id.clone();
        if let Some(remote) = self.space.remote.as_mut() {
            remote.wires_held.hold(key, on, Instant::now());
        }
        self.mark_remote();
        let call = Call {
            tool: if on { "connect" } else { "disconnect" }.into(),
            args: serde_json::json!({ "a": a, "b": b }),
            slow: false,
        };
        let id = space.clone();
        let then: Then = Box::new(move |ws, answered, window, cx| {
            let Err(refusal) = answered else {
                return;
            };
            if let Some(remote) = ws.remote_mut(&id) {
                remote.wires_held.end(&key);
            }
            ws.remirror(&id, Instant::now());
            ws.warn(refusal.sentence, window, cx);
        });
        self.remote_call(&space, call, then, cx);
        cx.notify();
        true
    }

    /// The person names `wire` (`label`; None takes its name off) on the remote shown: home does
    /// (`connect` with a `label`) and the name shows when home's state brings it. False when
    /// nothing is sent.
    pub(super) fn remote_label(
        &mut self,
        wire: Wire,
        label: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(remote) = self.space.remote.as_deref() else {
            return false;
        };
        let ends = remote.home_of(wire.a).zip(remote.home_of(wire.b));
        let Some((a, b)) = ends
            .filter(|_| remote.why_not(Deed::Label).is_none() && self.space.wires.contains(&wire))
        else {
            return false;
        };
        self.mark_remote();
        let call = Call {
            tool: "connect".into(),
            args: serde_json::json!({ "a": a, "b": b, "label": label.unwrap_or_default() }),
            slow: false,
        };
        let then: Then = Box::new(|ws, answered, window, cx| ws.warn_refusal(answered, window, cx));
        let space = self.space.id.clone();
        self.remote_call(&space, call, then, cx);
        true
    }

    /// The person hands the agent in the remote terminal `block` a task: home types it there
    /// (`delegate`), as it types the API's, and its end comes as a notice. Nothing is a step of
    /// Ctrl+Z and a success says nothing; a task too long for home, or home's refusal, is said.
    pub(crate) fn remote_hand_task(
        &mut self,
        block: BlockId,
        task: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if task.trim().is_empty() || self.why_not(Deed::Delegate).is_some() {
            return;
        }
        let Some(home) = self.space.remote.as_deref().and_then(|r| r.home_of(block)) else {
            return;
        };
        if task.len() > templum_mcp::tools::MAX_TEXT {
            return self.warn(
                "A task is at most 256 KiB: hand it in parts.".into(),
                window,
                cx,
            );
        }
        let call = Call {
            tool: "delegate".into(),
            args: serde_json::json!({ "block": home, "task": task }),
            slow: false,
        };
        let then: Then = Box::new(|ws, answered, window, cx| ws.warn_refusal(answered, window, cx));
        let space = self.space.id.clone();
        self.remote_call(&space, call, then, cx);
    }

    /// Hand a task…: asks for the task to give the agent in the remote terminal `block`.
    pub(crate) fn ask_task(&mut self, block: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(remote) = self.space.remote.as_deref() else {
            return;
        };
        let Some(pane) = self.space.terminal(block).and_then(|p| p.remote.as_ref()) else {
            return;
        };
        let agent = pane.agent.as_ref().map(|a| a.agent.clone());
        let about = format!(
            "{:?} on {}: it types it in, and its end comes back as a notice.",
            pane.title,
            crate::mirror::computer_of(remote)
        );
        let (ws, space) = (cx.entity(), self.space.id.clone());
        crate::prompt::ask_lines(
            format!("Hand a task to {}", agent.as_deref().unwrap_or("the agent")),
            about,
            "Hand over",
            move |task, window, cx| {
                ws.update(cx, |ws, cx| {
                    if ws.still_shown(&space, window, cx) {
                        ws.remote_hand_task(block, task, window, cx);
                    }
                });
                true
            },
            window,
            cx,
        );
    }

    /// A shortcut the remote shown can't do says why (`words`), the first time this run; a held
    /// key or a habit never stacks notifications.
    pub(super) fn tell_shortcut(
        &mut self,
        words: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let told =
            (self.space.remote.as_mut()).is_none_or(|r| !r.shortcuts_told.insert(words.clone()));
        if !told {
            self.warn(words, window, cx);
        }
    }

    /// The `then` of a call whose only result is its refusal, said.
    fn warn_refusal(&mut self, answered: Answered, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(refusal) = answered {
            self.warn(refusal.sentence, window, cx);
        }
    }

    /// Test: answers the call `n` that was kept for want of a client.
    #[cfg(test)]
    pub(crate) fn answer_call(
        &mut self,
        n: usize,
        answer: Answered,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (_, _, then) = self.calls.remove(n);
        then(self, answer, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_code_keeps_what_it_leaves_empty_of_the_same_access_token_and_proxy() {
        let secrets = |access: Option<&str>, login: Option<&str>| Secrets {
            token: Key::new("tpl_x".into()),
            access_secret: access.map(Into::into),
            proxy_auth: login.map(Into::into),
            url: "http://a.example".into(),
            trust: Trust::Public,
            trust_added: false,
            proxy: None,
        };
        let entry = |id: Option<&str>, proxy: Option<&str>| RemoteEntry {
            access_id: id.map(Into::into),
            proxy: proxy.map(Into::into),
            entry: "remote-old".into(),
            key: Keeping::File,
            ..RemoteEntry::default()
        };
        let kept = || Some(("remote-old".to_string(), true));
        let old = entry(Some("id"), Some("http://p:8080"));
        let from = inherited(
            &entry(Some("id"), Some("http://p:8080")),
            &secrets(None, None),
            &old,
            kept(),
        )
        .unwrap();
        assert_eq!(
            (from.entry.as_str(), from.file, from.access, from.login),
            ("remote-old", true, true, true)
        );
        // Typed anew, changed, or never there: nothing of the old is mixed in.
        assert!(
            inherited(
                &entry(Some("id"), None),
                &secrets(Some("s"), None),
                &old,
                kept()
            )
            .is_none()
        );
        assert!(
            inherited(
                &entry(Some("other"), None),
                &secrets(None, None),
                &old,
                kept()
            )
            .is_none()
        );
        let from = inherited(
            &entry(Some("id"), Some("http://q:8080")),
            &secrets(None, None),
            &old,
            kept(),
        )
        .unwrap();
        assert!(
            from.access && !from.login,
            "another proxy has its own login"
        );
        assert!(inherited(&entry(None, None), &secrets(None, None), &old, kept()).is_none());
        // Kept nowhere: nothing to read it from.
        let (fresh, empty) = (entry(Some("id"), None), secrets(None, None));
        assert!(inherited(&fresh, &empty, &old, None).is_none());
    }

    #[test]
    fn a_new_code_pasted_before_the_last_is_kept_inherits_from_the_key_before_it() {
        let entry = |name: &str, key: Keeping| RemoteEntry {
            entry: name.into(),
            key,
            ..RemoteEntry::default()
        };
        let inherits = Some(("remote-a".to_string(), true));
        assert_eq!(
            holder(&entry("remote-b", Keeping::None), &inherits),
            Some(("remote-a".into(), true))
        );
        assert_eq!(
            holder(&entry("remote-b", Keeping::Keyring), &inherits),
            Some(("remote-b".into(), false))
        );
        assert_eq!(
            holder(&entry("remote-b", Keeping::File), &inherits),
            Some(("remote-b".into(), true))
        );
        assert_eq!(holder(&entry("remote-b", Keeping::None), &None), None);
    }

    fn line(n: usize) -> Vec<(String, Rect)> {
        (0..n)
            .map(|i| {
                (
                    format!("b{i:02}"),
                    Rect::new(i as f32 * 100.0, 0.0, 50.0, 50.0),
                )
            })
            .collect()
    }

    /// A string of `n` characters with a control character and a newline in every ten.
    fn hostile(n: usize) -> String {
        (0..n)
            .map(|i| match i % 10 {
                3 => '\u{1b}',
                7 => '\n',
                _ => 'x',
            })
            .collect()
    }

    #[test]
    fn home_s_words_in_a_request_are_cleaned_and_capped() {
        use templum_api::model::{OptionView, PlanView, QuestionView, Why};
        let question = QuestionView {
            question: hostile(5000),
            header: Some(hostile(500)),
            multi: false,
            options: (0..100)
                .map(|_| OptionView {
                    label: hostile(5000),
                    description: Some(hostile(50_000)),
                    preview: Some(hostile(50_000)),
                })
                .collect(),
        };
        let request = RequestView {
            kind: "question".into(),
            questions: vec![question; 40],
            why: Why {
                agent: Some(hostile(50_000)),
                ..Why::default()
            },
            ..RequestView::default()
        };
        let Some(Waiting::Question { ask, .. }) = waiting_of(&request) else {
            panic!("a question");
        };
        assert_eq!(ask.questions.len(), MOST_QUESTIONS);
        let loose = |s: &str| {
            s.chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        };
        for q in &ask.questions {
            assert!(q.text.chars().count() <= LINE_CHARS && !loose(&q.text));
            assert!(q.header.as_ref().unwrap().chars().count() <= 60);
            assert_eq!(q.options.len(), MOST_OPTIONS);
            for o in &q.options {
                assert!(o.label.chars().count() <= LINE_CHARS && !loose(&o.label));
                for text in [&o.description, &o.preview] {
                    let text = text.as_ref().unwrap();
                    assert!(text.chars().count() <= TEXT_CHARS && !loose(text));
                    assert!(text.contains('\n'), "newlines stay in a preview");
                }
            }
        }
        assert!(ask.why.as_ref().unwrap().chars().count() <= TEXT_CHARS);

        // A question with no questions asks nothing; neither does a plan with no plan.
        let none = RequestView {
            kind: "question".into(),
            ..RequestView::default()
        };
        assert!(waiting_of(&none).is_none());
        let none = RequestView {
            kind: "plan".into(),
            ..RequestView::default()
        };
        assert!(waiting_of(&none).is_none());

        let plan = RequestView {
            kind: "plan".into(),
            plan: Some(PlanView {
                title: Some(hostile(500)),
                text: hostile(PLAN_CHARS * 2),
            }),
            ..RequestView::default()
        };
        let Some(Waiting::Question { ask, .. }) = waiting_of(&plan) else {
            panic!("a plan");
        };
        let plan = ask.plan.unwrap();
        assert_eq!(plan.text.chars().count(), PLAN_CHARS);
        assert!(!loose(&plan.text));
        assert!(plan.title.unwrap().chars().count() <= 60);

        // A yes or no: the summary is one short line, the buttons' words short.
        let allow = RequestView {
            kind: "connect".into(),
            summary: hostile(5000),
            buttons: Some([hostile(500), hostile(500)]),
            ..RequestView::default()
        };
        let Some(Waiting::Allow {
            then: Approved::Remote { line, yes, no },
        }) = waiting_of(&allow)
        else {
            panic!("an allow");
        };
        assert!(line.chars().count() <= ASKS_CHARS && !line.chars().any(char::is_control));
        for word in [yes, no] {
            assert!(word.chars().count() <= 24 && !word.chars().any(char::is_control));
        }
    }

    #[test]
    fn a_refusal_is_cleaned_where_it_enters() {
        let cleaned = clean_refusal(client::Refusal {
            sentence: "a\u{1b}[31mb\nc".into(),
            details: Some("x\u{1b}y\n\tz".into()),
            maybe_done: true,
        });
        assert!(!cleaned.sentence.contains(['\u{1b}', '\n']), "{cleaned:?}");
        assert_eq!(cleaned.details.as_deref(), Some("xy\n\tz"));
        assert!(cleaned.maybe_done, "what home may have done is kept");
        let blank = clean_refusal(client::Refusal {
            sentence: "x".into(),
            details: Some("\u{1b}".into()),
            maybe_done: false,
        });
        assert_eq!(blank.details, None);
    }

    #[test]
    fn a_request_older_than_the_clock_can_say_is_a_week_old() {
        let now = Instant::now();
        let week = now.checked_sub(AGE_MOST).unwrap();
        assert_eq!(aged(i64::MIN, now, 1_000), week);
        assert_eq!(aged(i64::MIN, now, i64::MAX), week);
        assert_eq!(aged(i64::MAX, now, 1_000), now);
    }

    #[test]
    fn clean_text_keeps_newlines_and_tabs_and_nothing_else_control() {
        assert_eq!(clean_text("a\u{1b}[31mb\n\tc\u{7}", 100), "a[31mb\n\tc");
        assert_eq!(clean_text("héllo", 3), "hél");
    }

    #[test]
    fn the_nearest_eleven_stream_and_the_focused_one_always() {
        let blocks = line(15);
        // The center is the 8th block's (index 7).
        let center = Point::new(7.0 * 100.0 + 25.0, 25.0);
        let near = streamed(&blocks, center, None);
        assert_eq!(near.len(), MOST_STREAMS);
        assert_eq!(near[0], "b07", "the nearest first");
        for far in ["b00", "b14"] {
            assert!(
                !near.contains(&far.to_string()),
                "{far} is too far: {near:?}"
            );
        }
        // The 15th is focused: first, and the 12th nearest is out.
        let focused = streamed(&blocks, center, Some("b14"));
        assert_eq!(focused.len(), MOST_STREAMS);
        assert_eq!(focused[0], "b14");
        assert!(!focused.contains(&near[MOST_STREAMS - 1]), "{focused:?}");
        // A focused block that is not among them adds nothing.
        assert_eq!(streamed(&blocks, center, Some("zzz")), near);
    }

    #[test]
    fn fit_zoom_takes_the_largest_step_that_fits() {
        let cell = Size::new(8.0, 16.0);
        let fit = |zoom, w, h| fit_zoom(zoom, Size::new(w, h), (80, 24), cell);
        assert_eq!(fit(1.0, 640.0, 384.0), 1.0);
        assert_eq!(fit(1.0, 320.0, 192.0), 0.5);
        // k = 330 / 640 = 0.515…, and 2 × k = 1.03: the step below it.
        assert_eq!(fit(2.0, 330.0, 400.0), 1.0);
        assert_eq!(
            fit(1.0, 10.0, 10.0),
            0.25,
            "the smallest step when none fits"
        );
        // A bigger block than the grid needs does not stretch past the largest step.
        assert_eq!(fit(3.0, 6400.0, 3840.0), 3.0);
    }

    fn remote_at(url: &str, status: Status) -> RemoteSpace {
        let mut remote = RemoteSpace::new(RemoteEntry {
            url: url.into(),
            way: "public".into(),
            ..RemoteEntry::default()
        });
        remote.home = Some(WorkspaceView {
            name: "API".into(),
            ..WorkspaceView::default()
        });
        remote.me = Some(MeView {
            computer: "winx-desktop".into(),
            scope: templum_api::store::Scope::Full,
            ..MeView::default()
        });
        remote.status = status;
        remote
    }

    #[test]
    fn the_one_rule_says_why_in_its_order() {
        let mut remote = remote_at(
            "https://winx-desktop.example.com",
            Status::Reconnecting {
                attempt: 1,
                next: Duration::from_secs(1),
                live_at: None,
            },
        );
        let words = |r: &RemoteSpace, d| r.why_not(d).map(|n| n.words("winx-desktop"));
        assert_eq!(
            words(&remote, Deed::AddEditor).as_deref(),
            Some("Not on a mirrored workspace yet")
        );
        assert_eq!(
            words(&remote, Deed::Move).as_deref(),
            Some("Not connected.")
        );
        remote.status = Status::Live;
        remote.me = Some(MeView {
            scope: Scope::Control,
            ..MeView::default()
        });
        assert_eq!(
            remote.why_not(Deed::Move),
            None,
            "tools unknown: nothing is older"
        );
        assert_eq!(
            words(&remote, Deed::Type).as_deref(),
            Some("Needs a Full share: share again on winx-desktop.")
        );
        assert_eq!(remote.why_not(Deed::Answer(Scope::Control)), None);
        assert_eq!(
            remote.why_not(Deed::Answer(Scope::Full)),
            Some(Nay::NeedsFull)
        );
        remote.tools = Some(Tools::read(&serde_json::json!([{ "name": "show" }])));
        assert_eq!(
            words(&remote, Deed::Move).as_deref(),
            Some("winx-desktop's Templum is older: update it to do this from here.")
        );
        assert_eq!(remote.why_not(Deed::Start), None);
    }

    #[test]
    fn a_request_is_aged_from_home_s_since() {
        let now = Instant::now();
        // Home's clock says 120 s: two minutes old.
        assert_eq!(aged(1000, now, 1120), now - Duration::from_secs(120));
        // Home's clock ahead of this one's: as old as now.
        assert_eq!(aged(2000, now, 1000), now);
        // Older than a week is a week.
        assert_eq!(aged(0, now, 10_000_000), now - AGE_MOST);
    }

    #[test]
    fn every_deed_home_runs_has_a_scope_in_the_registry() {
        let scope = |deed: Deed| {
            let (tool, args) = deed.call().expect("a call");
            templum_api::registry::scope(tool, &args).expect("parses")
        };
        assert_eq!(scope(Deed::Type), Scope::Full);
        assert_eq!(scope(Deed::Mouse), Scope::Full);
        assert_eq!(scope(Deed::ScrollBack), Scope::Read);
        assert_eq!(scope(Deed::Move), Scope::Control);
        assert_eq!(scope(Deed::Start), Scope::Control);
        assert_eq!(scope(Deed::Answer(Scope::Control)), Scope::Control);
        for control in [
            Deed::AddTerminal,
            Deed::AddKanban,
            Deed::AddChecklist,
            Deed::OpenAgent,
            Deed::Close,
            Deed::Wire,
            Deed::Unwire,
            Deed::Label,
            Deed::Delegate,
            Deed::MoveCard,
            Deed::CreateCard,
            Deed::EditCard,
            Deed::CommentCard,
            Deed::EditComment,
            Deed::DeleteComment,
            Deed::DeleteCard,
            Deed::EditColumn,
            Deed::EditBoard,
            Deed::ShowBoard,
            Deed::CheckItem,
            Deed::AddItems,
            Deed::EditItem,
            Deed::DeleteItem,
            Deed::EditChecklist,
            Deed::OpenChecklist,
            Deed::SetRule(false),
            Deed::RunRule(false),
            Deed::SaveRule(false),
            Deed::CancelSchedule,
        ] {
            assert_eq!(scope(control), Scope::Control, "{control:?}");
        }
        assert_eq!(scope(Deed::SaveRule(true)), Scope::Full);
        assert_eq!(scope(Deed::DeleteRule), Scope::Full);
    }

    #[test]
    fn a_command_rule_takes_full_to_switch_run_save_or_delete() {
        let mut remote = remote_at("https://winx-desktop.example.com", Status::Live);
        remote.me = Some(MeView {
            scope: Scope::Control,
            ..MeView::default()
        });
        for full in [
            Deed::SetRule(true),
            Deed::RunRule(true),
            Deed::SaveRule(true),
            Deed::DeleteRule,
        ] {
            assert_eq!(remote.why_not(full), Some(Nay::NeedsFull), "{full:?}");
        }
        for control in [
            Deed::SetRule(false),
            Deed::RunRule(false),
            Deed::SaveRule(false),
            Deed::CancelSchedule,
        ] {
            assert_eq!(remote.why_not(control), None, "{control:?}");
        }
        remote.tools = Some(Tools::read(&serde_json::json!([
            { "name": "set_automation", "inputSchema": { "properties": {} } },
        ])));
        assert_eq!(remote.why_not(Deed::CancelSchedule), Some(Nay::Older));
        assert_eq!(remote.why_not(Deed::SetRule(false)), None);
    }

    #[test]
    fn the_mouse_needs_a_home_that_takes_it() {
        let mut remote = remote_at("https://winx-desktop.example.com", Status::Live);
        remote.me = Some(MeView {
            scope: Scope::Full,
            ..MeView::default()
        });
        let input = |props: serde_json::Value| {
            Tools::read(&serde_json::json!([
                { "name": "terminal_input", "inputSchema": { "properties": props } }
            ]))
        };
        remote.tools = Some(input(
            serde_json::json!({ "block": {}, "text": {}, "keys": {} }),
        ));
        assert_eq!(remote.why_not(Deed::Mouse), Some(Nay::Older));
        assert_eq!(remote.why_not(Deed::Type), None, "typing still works");
        remote.tools = Some(input(serde_json::json!({ "block": {}, "mouse": {} })));
        assert_eq!(remote.why_not(Deed::Mouse), None);
    }

    #[test]
    fn ids_handed_on_a_mirror_never_meet_home_s() {
        let mut remote = RemoteSpace::new(RemoteEntry::default());
        assert_eq!(remote.fresh_id(), u64::MAX);
        assert_eq!(remote.fresh_id(), u64::MAX - 1);
    }

    #[test]
    fn rename_is_dimmed_on_a_home_without_label() {
        let mut remote = remote_at("https://winx-desktop.example.com", Status::Live);
        remote.me.as_mut().unwrap().scope = Scope::Control;
        remote.tools = Some(Tools::read(&serde_json::json!([
            { "name": "connect", "inputSchema": { "properties": { "a": {}, "b": {} } } },
            { "name": "disconnect" },
        ])));
        assert_eq!(remote.why_not(Deed::Label), Some(Nay::Older));
        assert_eq!(remote.why_not(Deed::Wire), None);
        assert_eq!(remote.why_not(Deed::Unwire), None);
        assert_eq!(
            remote
                .why_not(Deed::Label)
                .map(|n| n.words("winx-desktop"))
                .as_deref(),
            Some("winx-desktop's Templum is older: update it to do this from here.")
        );
    }

    #[test]
    fn home_s_agent_list_is_capped_and_cleaned() {
        let many: String = (0..150)
            .map(|n| format!("a{n}  \"Agent {n}\"  installed  no model  no effort\n"))
            .collect();
        let agents = home_agents(&many);
        assert_eq!(agents.len(), MOST_AGENTS);
        assert_eq!(
            agents[7],
            HomeAgent {
                id: "a7".into(),
                title: "Agent 7".into(),
                installed: true
            }
        );
        let text = [
            "codex  \"Co\\\"dex\"  not installed  no model  no effort".to_string(),
            format!("{}  \"Long id\"  installed", "x".repeat(65)),
            "quo\"te  \"Quoted id\"  installed".to_string(),
            "bell\u{7}  \"Control id\"  installed".to_string(),
            "notitle  installed  no model".to_string(),
            "unclosed  \"Never ends  installed".to_string(),
            String::new(),
            format!("big  \"{}\"  installed  an account", "y".repeat(1_000_000)),
            "bell  \"Rings\u{7}here\"  installed".to_string(),
            "blank  \"\"  not installed".to_string(),
            "shy  \"Hidden\"  installed  no model  no effort  an account  hidden".to_string(),
        ]
        .join("\n");
        let agents = home_agents(&text);
        let ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["codex", "big", "bell", "blank"]);
        assert_eq!(agents[0].title, "Co\"dex");
        assert!(!agents[0].installed);
        assert!(agents[1].installed && agents[1].title.chars().count() <= 60);
        assert!(!agents[1].title.contains('\n'));
        assert!(
            !agents[2].title.chars().any(char::is_control),
            "{:?}",
            agents[2].title
        );
        assert_eq!(agents[3].title, "blank", "no title: the id");
    }

    #[test]
    fn notes_and_adds_are_home_s_calls() {
        let mut remote = remote_at("https://winx-desktop.example.com", Status::Live);
        remote.me.as_mut().unwrap().scope = Scope::Control;
        let words = |r: &RemoteSpace, d| r.why_not(d).map(|n| n.words("winx-desktop"));
        assert_eq!(
            words(&remote, Deed::AddEditor).as_deref(),
            Some("Not on a mirrored workspace yet")
        );
        for add in [
            Deed::AddTerminal,
            Deed::AddKanban,
            Deed::AddChecklist,
            Deed::AddNote,
            Deed::EditNote,
            Deed::OpenNote,
        ] {
            assert_eq!(remote.why_not(add), None, "{add:?}");
        }
        // Deleting a note takes a Full share.
        assert_eq!(
            words(&remote, Deed::DeleteNote).as_deref(),
            Some("Needs a Full share: share again on winx-desktop.")
        );
        // A home that could add a note but not type in it is older for adding one.
        let tools = |names: &[&str]| {
            Tools::read(&serde_json::Value::Array(
                names
                    .iter()
                    .map(|n| serde_json::json!({ "name": n, "inputSchema": { "properties": {} } }))
                    .collect(),
            ))
        };
        remote.tools = Some(tools(&["add_block", "edit_item"]));
        for deed in [Deed::AddNote, Deed::EditNote, Deed::OpenNote] {
            assert_eq!(remote.why_not(deed), Some(Nay::Older), "{deed:?}");
        }
        assert_eq!(remote.why_not(Deed::AddKanban), None);
        remote.tools = Some(tools(&["add_block", "edit_note", "open_note"]));
        assert_eq!(remote.why_not(Deed::AddNote), None);
        remote.tools = None;
        assert_eq!(
            words(&remote, Deed::AddEditor).as_deref(),
            Some("Not on a mirrored workspace yet")
        );
        for add in [Deed::AddTerminal, Deed::AddKanban, Deed::AddChecklist] {
            assert_eq!(remote.why_not(add), None, "{add:?}");
        }
        // An argument home lacks is left out; unknown tools take everything.
        assert!(remote.takes("add_block", "x"));
        remote.tools = Some(Tools::read(&serde_json::json!([
            { "name": "add_block", "inputSchema": { "properties": { "kind": {} } } }
        ])));
        assert!(remote.takes("add_block", "kind") && !remote.takes("add_block", "x"));
    }

    #[test]
    fn the_banner_tells_a_slow_round_trip_and_a_coming_expiry() {
        let (now, wall) = (Instant::now(), SystemTime::now());
        let at = wall
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut live = remote_at("https://templum.example.com", Status::Live);
        live.latency = Some(Duration::from_millis(84));
        live.me.as_mut().unwrap().expires = Some(at + 30 * 86_400);
        let text = |live: &RemoteSpace| banner_words(live, now, wall).text;
        assert_eq!(text(&live), "Mirror of API on winx-desktop · Full · 84 ms");
        // The yellow round trip is the banner's own (it needs the real number, in the text).
        live.latency = Some(Duration::from_millis(312));
        assert!(text(&live).ends_with("· Full · 312 ms"));
        assert_eq!(
            crate::mirror::slow_tip(Duration::from_millis(312), "winx-desktop"),
            "Each key waits about 312 ms for winx-desktop's answer."
        );
        live.latency = Some(Duration::from_millis(84));
        // 2 days 3 hours left: three days; 5 hours left: 5 h; a day and a bit: two days.
        for (left, says) in [
            (2 * 86_400 + 3 * 3600, "3 days"),
            (86_400 + 60, "2 days"),
            (5 * 3600 + 20, "5 h"),
            (40 * 60, "40 min"),
        ] {
            live.me.as_mut().unwrap().expires = Some(at + left);
            assert_eq!(
                text(&live),
                format!(
                    "Mirror of API on winx-desktop · Full · 84 ms · ends in {says}: share again on winx-desktop"
                ),
                "{left} s left"
            );
        }
        // Past it, the token is cut by home; nothing to tell here.
        live.me.as_mut().unwrap().expires = Some(at + 3 * 86_400);
        assert!(!text(&live).contains("ends in"));
    }

    #[test]
    fn every_state_has_its_words() {
        let (now, wall) = (Instant::now(), SystemTime::now());
        let cut = |why| remote_at("https://templum.example.com", Status::Cut(why));
        let says = |remote: &RemoteSpace, text: &str, acts: &[Act], dim: bool| {
            let banner = banner_words(remote, now, wall);
            assert_eq!(
                (banner.text.as_str(), banner.acts.as_slice(), banner.dim),
                (text, acts, dim)
            );
        };
        let mut live = remote_at("https://templum.example.com", Status::Live);
        says(&live, "Mirror of API on winx-desktop · Full", &[], false);
        live.latency = Some(Duration::from_millis(84));
        says(
            &live,
            "Mirror of API on winx-desktop · Full · 84 ms",
            &[],
            false,
        );
        live.me.as_mut().unwrap().expires = Some(
            wall.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 2 * 86_400
                + 60,
        );
        says(
            &live,
            "Mirror of API on winx-desktop · Full · 84 ms · ends in 3 days: share again on winx-desktop",
            &[],
            false,
        );
        live.me.as_mut().unwrap().expires = Some(
            wall.duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 40 * 86_400,
        );
        says(
            &live,
            "Mirror of API on winx-desktop · Full · 84 ms",
            &[],
            false,
        );
        live.home.as_mut().unwrap().stopped = true;
        says(
            &live,
            "API is stopped on winx-desktop.",
            &[Act::StartIt],
            false,
        );
        says(
            &remote_at("https://templum.example.com", Status::Connecting),
            "Connecting…",
            &[],
            false,
        );
        let mut reconnecting = remote_at(
            "https://templum.example.com",
            Status::Reconnecting {
                attempt: 3,
                next: Duration::from_secs(10),
                live_at: Some(wall - Duration::from_secs(120)),
            },
        );
        reconnecting.since = now - Duration::from_secs(2);
        says(
            &reconnecting,
            "Reconnecting to winx-desktop… try 3, next in 8 s. Live 2 min ago.",
            &[Act::TryNow],
            true,
        );
        reconnecting.status = Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        };
        says(
            &reconnecting,
            "Reconnecting to winx-desktop… try 1, next in 0 s.",
            &[Act::TryNow],
            true,
        );
        says(
            &cut(Why::Expired),
            "This share of API expired. Share again on winx-desktop, then Paste a new code…",
            &[Act::Paste, Act::Forget],
            true,
        );
        says(
            &cut(Why::Revoked),
            "winx-desktop revoked this share. Share again there, then Paste a new code…",
            &[Act::Paste, Act::Forget],
            true,
        );
        says(
            &cut(Why::Deleted),
            "API was deleted on winx-desktop.",
            &[Act::Forget],
            true,
        );
        says(
            &cut(Why::Replaced),
            "Another computer connected with this share's code, so this one stopped. Each computer needs its own share.",
            &[Act::ConnectHere, Act::Forget],
            true,
        );
        says(
            &cut(Why::PinChanged),
            "winx-desktop's certificate changed since you connected. If you made a New certificate there, share again and paste the new code. If you didn't, someone may be between the two computers: don't connect.",
            &[Act::Paste, Act::Forget],
            true,
        );
        let invalid = |what: &str| {
            format!(
                "templum.example.com's certificate isn't valid: {what}. Fix it where the tunnel or proxy runs, then Try again."
            )
        };
        for (bad, what) in [
            (client::Bad::Expired, "it expired"),
            (client::Bad::Name, "it is for another name"),
            (
                client::Bad::Issuer,
                "no authority this computer trusts signed it",
            ),
        ] {
            says(
                &cut(Why::Certificate(bad)),
                &invalid(what),
                &[Act::TryNow],
                true,
            );
        }
        says(
            &cut(Why::Certificate(client::Bad::Alone)),
            &format!(
                "{} If it is Templum's own, share Directly instead: it is pinned.",
                invalid("no authority this computer trusts signed it")
            ),
            &[Act::TryNow],
            true,
        );
        says(
            &cut(Why::AddedAuthority),
            "HTTPS to templum.example.com is signed by an authority this computer added, not a public one. A company proxy or security program may be reading this connection: your key, what home's terminals show and what you type. With a Full share it could run commands on winx-desktop. Connect only if you added that authority yourself (a home lab, your own proxy).",
            &[Act::DontConnect, Act::ConnectAnyway],
            true,
        );
        says(
            &cut(Why::Access),
            "Cloudflare Access stands in front of templum.example.com. Add a service token in Paste a new code… ▸ More…, and allow it in the Access policy.",
            &[Act::TryNow, Act::Paste],
            true,
        );
        says(
            &cut(Why::NotTemplum {
                status: 404,
                kind: "text/html".into(),
            }),
            "templum.example.com didn't answer as a Templum (HTTP 404, text/html). Check that the tunnel points at port 7411.",
            &[Act::TryNow],
            true,
        );
        // What answers at the code's URL names its own type: only the start of it is shown.
        let long = banner_words(
            &cut(Why::NotTemplum {
                status: 200,
                kind: "x".repeat(400_000),
            }),
            now,
            wall,
        );
        assert!(long.text.chars().count() < 200, "{}", long.text.len());
        says(
            &cut(Why::TooOld),
            "winx-desktop runs a Templum that can't be mirrored: update it.",
            &[Act::TryNow],
            true,
        );
        for (status, because) in [
            (407, "407: it asks for a login Templum can't give"),
            (403, "HTTP 403"),
        ] {
            says(
                &cut(Why::Proxy {
                    proxy: "http://proxy.corp:8080".into(),
                    status,
                }),
                &format!("The proxy proxy.corp:8080 refused the connection ({because})."),
                &[Act::TryNow],
                true,
            );
        }
        says(
            &cut(Why::Keyring),
            "Templum couldn't read this connection's key from the keyring. Try again, or Paste a new code…",
            &[Act::TryNow, Act::Paste],
            true,
        );
        says(
            &remote_at("https://templum.example.com", Status::Disconnected),
            "Disconnected. Connect",
            &[Act::Connect],
            true,
        );
        // A pinned remote shows host and port; a scheme's own port is left out.
        let pinned = remote_at("https://192.168.1.5:7411", Status::Cut(Why::Access));
        assert!(
            banner_words(&pinned, now, wall)
                .text
                .contains("in front of 192.168.1.5:7411.")
        );
        assert_eq!(
            host_of("https://templum.example.com:443/x"),
            "templum.example.com"
        );
    }
}
