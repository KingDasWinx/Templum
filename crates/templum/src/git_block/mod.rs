//! A Git block: one repository, through the user's own `git`, run in the background. The
//! changes with their diffs, commits, history, branches, stashes, fetch, pull and push.
//! See docs/specs/cockpit.md, section 8.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, ElementId, Entity, EntityId, FocusHandle, ScrollHandle, SharedString,
    Subscription, UniformListScrollHandle, Window,
};
use templum_canvas::Size;
use templum_git::{Branch, Commit, CommitFile, Entry, GitError, Operation, Repo, Stash, Status};

use crate::session;
use crate::workspace::Workspace;

mod branches;
mod changes;
mod commit_form;
pub(crate) mod diff_view;
pub(crate) mod discarded;
mod history;
mod stashes;
pub(crate) mod switcher;
mod view;
pub(crate) mod widgets;
pub(crate) mod words;

pub(crate) use view::popup;
pub(crate) use widgets::field;

/// The popups' size (Git's, and the others in its place: crate::popup), pixels, until the person drags it to another; a smaller window gets a
/// smaller one.
pub(crate) const SIZE: Size = Size { w: 760.0, h: 540.0 };
/// The smallest the person can drag the popup to: the header's actions, a list column and a
/// diff beside it still fit. A window with less room than this gets the room.
pub(crate) const MIN: Size = Size { w: 600.0, h: 420.0 };

/// The size the popup is drawn at: the one the person set (`SIZE` while none), at least `MIN`,
/// at most `room`, which wins when it is smaller than `MIN`. Only what is drawn is clamped: the
/// size set is kept, so a window that grows back gives it back.
pub(crate) fn fit(set: Option<Size>, room: Size) -> Size {
    let wanted = set.unwrap_or(SIZE);
    Size::new(
        wanted.w.max(MIN.w).min(room.w),
        wanted.h.max(MIN.h).min(room.h),
    )
}
/// Past this many changed files, a Filter files field shows above the lists.
pub(crate) const FILTER_PAST: usize = 15;
/// The lists' column width and the rows' height, world units.
pub(crate) const LISTS_W: f32 = 300.0;
pub(crate) const ROW_H: f32 = 28.0;
/// The text's size at zoom 1, world units.
pub(crate) const FONT: f32 = 12.0;

/// How often the status is read while the block is on screen.
pub(crate) const POLL: Duration = Duration::from_secs(2);
/// How many commits History loads at a time.
pub(crate) use templum_git::ops::PAGE;
/// Past this many lanes the graph is cut (the commit's dot on the last one).
// ponytail: a history with more than 6 branches open at once draws 6; a horizontal scroll of the
// graph if people meet it.
pub(crate) const MAX_LANES: usize = 6;
/// A lane's width, world units.
pub(crate) const LANE_W: f32 = 12.0;
/// The summary field's placeholder while no message is generated.
const COMMIT_MESSAGE: &str = "Commit message";
/// Past this many characters a summary is longer than `git log --oneline` shows well.
pub(crate) const SUMMARY_MAX: usize = 72;

/// A comparison in the Branches tab: what `branch` has that `base` hasn't, as a pull request
/// shows it (docs/specs/git.md, section 5, Comparison).
pub(crate) struct Compare {
    pub branch: String,
    /// The default branch at first; the column's base button changes it.
    pub base: String,
    /// `git log base..branch`, newest first (an Rc: the column's rows hold it).
    pub commits: Rc<Vec<Commit>>,
    /// The commit the panel is narrowed to; None: the whole comparison.
    pub narrowed: Option<String>,
    /// Its own panel: the 2-second read never touches it.
    pub panel: diff_view::DiffPanel,
}

/// The commit shown on the right of the History tab.
pub(crate) struct Shown {
    pub commit: Commit,
    pub files: Vec<CommitFile>,
    /// The file last clicked: the panel scrolled to it.
    pub file: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Changes,
    History,
    Branches,
    Stashes,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Changes, Tab::History, Tab::Branches, Tab::Stashes];

    /// As the canvas file names it.
    pub fn name(self) -> &'static str {
        match self {
            Tab::Changes => "changes",
            Tab::History => "history",
            Tab::Branches => "branches",
            Tab::Stashes => "stashes",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Changes => "Changes",
            Tab::History => "History",
            Tab::Branches => "Branches",
            Tab::Stashes => "Stashes",
        }
    }

    /// Changes for a name it doesn't know.
    pub fn from_name(name: &str) -> Tab {
        Tab::ALL
            .into_iter()
            .find(|t| t.name() == name)
            .unwrap_or_default()
    }
}

/// What the folder turned out to hold.
pub(crate) enum Found {
    /// The look-up hasn't come back yet.
    Looking,
    /// `git` couldn't run.
    NoGit(GitError),
    NotARepo,
    Repo(Repo),
    /// A mirrored workspace's repository: its root, as home writes it. Nothing here is a path of
    /// this computer, and no `Repo` is held: nothing runs or reads on its behalf.
    Home(String),
    /// Home couldn't look for it: its words.
    Refused(String),
}

/// Which of fetch, pull, push and sync runs: the bar's Sync button spins, and a click on it
/// cancels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sync {
    Fetch,
    Pull,
    Push,
    /// Sync: pull, then push.
    Both,
    /// A clone, from the not-a-repository view or ⋯.
    Cloning,
}

/// The lists of the Changes tab: the conflicts, then every other changed file once, as a
/// checklist (checked: it goes into the next commit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum List {
    Conflicts,
    Files,
}

impl List {
    pub fn name(self) -> &'static str {
        match self {
            List::Conflicts => "conflicts",
            List::Files => "files",
        }
    }
}

/// A file's box in the Changes checklist: what of it is staged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Check {
    /// All of it goes into the next commit.
    Checked,
    /// Some of it (hunks staged from the panel), not all.
    Partial,
    Unchecked,
}

impl Check {
    pub fn of(e: &Entry) -> Check {
        match (e.staged, e.unstaged) {
            (Some(_), None) => Check::Checked,
            (Some(_), Some(_)) => Check::Partial,
            (None, _) => Check::Unchecked,
        }
    }
}

/// A command waiting for the block's running one.
pub(crate) type Queued = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// A Git block's state: the folder, what it holds, and the last read.
pub(crate) struct Git {
    /// What the user chose; the repository's root may be above it.
    pub folder: PathBuf,
    /// A mirrored workspace's Git: the folder at home its repository is found from, as home
    /// writes it. Nothing on this computer reads it.
    pub home: Option<String>,
    /// On a mirror: the status the panel's diff was last asked with, and when.
    pub home_diff: Option<(Status, Instant)>,
    /// The folder wasn't there at the last look, on opening and every POLL: the body says so.
    /// Never looked up while drawing: a stat each frame stalls on a slow or network disk.
    pub gone: bool,
    /// How many times the block changed folder (see `GitAt`).
    pub generation: u64,
    pub found: Found,
    pub tab: Tab,
    pub status: Status,
    pub operation: Option<Operation>,
    /// `user.name` and `user.email`; None: not set, so Commit is disabled.
    pub author: Option<(String, String)>,
    /// A command runs in the background: reads wait, actions are disabled.
    pub busy: bool,
    /// Actions asked and not done yet (reads don't count): while any, the action buttons are
    /// disabled.
    pub acting: u32,
    /// Kills the running fetch, pull or push (its spinning button).
    pub cancel: Option<Arc<AtomicBool>>,
    /// Generate ✨ runs: set, it stops the agent.
    pub generating: Option<Arc<AtomicBool>>,
    /// A refresh was asked while busy: one runs when the command ends.
    pub pending: bool,
    /// Other commands asked while busy, run in turn as each one ends.
    pub queued: Vec<Queued>,
    /// Reads completed so far (the tests count them).
    pub reads: u64,
    /// The fetch, pull or push running.
    pub sync: Option<Sync>,
    /// Why the last read failed, shown under the top bar.
    pub error: Option<String>,
    /// The default branch ("main", or "origin/main" with no local one), as Back to and Compare
    /// name it; None when the repository has none. Read after each action, not on every poll.
    pub default_branch: Option<String>,
    /// The diff panel of the Changes tab.
    pub panel: diff_view::DiffPanel,
    /// The Branches tab's comparison, while one shows.
    pub compare: Option<Compare>,
    /// History's diff panel: the commit shown, or a file's blame.
    pub history_panel: diff_view::DiffPanel,
    /// Stashes' diff panel: the stash shown.
    pub stash_panel: diff_view::DiffPanel,
    /// The row whose diff is shown.
    pub selected: Option<(List, String)>,
    pub summary: Entity<InputState>,
    /// The summary's placeholder as last set (the field can't be asked): "Commit message", or the
    /// message an empty summary commits as.
    pub placeholder: SharedString,
    pub description: Entity<TextareaState>,
    /// Commit replaces the last commit; the fields were filled with its message.
    pub amend: bool,
    /// Ctrl+Enter in either field commits.
    _enter: Vec<Subscription>,
    /// The loaded commits, newest first (an Rc: the virtual list's closure holds them).
    pub commits: Rc<Vec<Commit>>,
    /// The last page was full: Load more shows.
    pub more: bool,
    /// The loaded commits' graph, one row per commit; empty while the list is filtered (a search,
    /// a file's history: their parents aren't in it).
    pub graph: Rc<Vec<templum_git::graph::GraphRow>>,
    /// HEAD's commits not on its upstream: History's "not pushed" mark.
    pub unpushed: Rc<std::collections::HashSet<String>>,
    /// History lists every branch, tag and remote branch.
    pub all_branches: bool,
    /// History is narrowed to this file (from the repository's root, with '/').
    pub file: Option<String>,
    /// The file at each loaded commit (a rename's old path before it), in a file's history.
    pub file_paths: Rc<Vec<CommitFile>>,
    /// History's panel shows `file`'s blame while no commit is shown.
    pub blame: bool,
    /// The commit it is blamed at; None: the working file.
    pub blame_at: Option<String>,
    pub search: Entity<InputState>,
    /// Where History's search looks.
    pub search_mode: templum_git::Search,
    /// The text History's list was read with; empty: no search.
    pub searched: String,
    pub shown: Option<Shown>,
    /// The commit clicked, until another place is asked for: a read for any other is dropped.
    pub showing: Option<String>,
    /// A list read was asked while a command ran.
    pub pending_lists: bool,
    pub branches: Vec<Branch>,
    pub stashes: Vec<Stash>,
    /// The stash whose diff is shown, by its index in `stash@{N}`.
    pub stash_shown: Option<usize>,
    /// The Branches tab's filter.
    pub filter: Entity<InputState>,
    /// The Branches tab's remote branches are folded.
    pub remote_folded: bool,
    _filter: Subscription,
    /// The block's own keyboard focus (not a field's): its keys act while it has it.
    pub focus: FocusHandle,
    /// The left column's list, scrolled by ↑ and ↓.
    pub lists_scroll: UniformListScrollHandle,
    /// The scroll of History's commit details, of a comparison's commits and of the stashes:
    /// kept here, as `lists_scroll` is, since GPUI keeps an element's own scroll only while it
    /// is drawn, and far away the block is its card.
    pub details_scroll: ScrollHandle,
    pub compare_scroll: ScrollHandle,
    pub stash_scroll: ScrollHandle,
    /// The left column is folded: the diff panel takes the whole width.
    pub lists_folded: bool,
    /// Every panel of the block side by side (Split) or not; the block remembers it.
    pub split: bool,
    /// Filter files, over the Changes tab's lists (shown past FILTER_PAST files).
    pub files_filter: Entity<InputState>,
    _files_filter: Subscription,
    _search: Subscription,
}

impl Git {
    pub fn new(folder: PathBuf, tab: Tab, window: &mut Window, cx: &mut Context<Workspace>) -> Git {
        let summary = cx.new(|cx| InputState::new(window, cx).placeholder(COMMIT_MESSAGE));
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .placeholder("Description")
        });
        let (from_summary, from_description) = (summary.entity_id(), description.entity_id());
        let _enter = vec![
            cx.subscribe_in(
                &summary,
                window,
                move |ws, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter {
                        secondary: true, ..
                    } = event
                    {
                        ws.commit_from(from_summary, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &description,
                window,
                move |ws, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter {
                        secondary: true, ..
                    } = event
                    {
                        ws.commit_from(from_description, window, cx);
                    }
                },
            ),
        ];
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search, then Enter"));
        let from_search = search.entity_id();
        let _search = cx.subscribe_in(
            &search,
            window,
            move |ws, field, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => ws.search_from(from_search, window, cx),
                // Cleared: the whole history back, without waiting for Enter.
                InputEvent::Change if field.read(cx).value().trim().is_empty() => {
                    ws.search_from(from_search, window, cx)
                }
                _ => {}
            },
        );
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter branches"));
        let _filter = cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify()
            }
        });
        let files_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter files"));
        let _files_filter = cx.subscribe(&files_filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify()
            }
        });
        Git {
            gone: !folder.is_dir(),
            folder,
            home: None,
            home_diff: None,
            generation: 0,
            found: Found::Looking,
            tab,
            status: Status::default(),
            operation: None,
            author: None,
            busy: false,
            acting: 0,
            cancel: None,
            generating: None,
            pending: false,
            queued: Vec::new(),
            reads: 0,
            sync: None,
            error: None,
            default_branch: None,
            panel: diff_view::DiffPanel::new(),
            compare: None,
            history_panel: diff_view::DiffPanel::range(String::new())
                .with_hint("Click a commit to see its changes"),
            stash_panel: diff_view::DiffPanel::range(String::new())
                .with_hint("Click a stash to see its changes"),
            selected: None,
            summary,
            placeholder: COMMIT_MESSAGE.into(),
            description,
            amend: false,
            _enter,
            commits: Rc::new(Vec::new()),
            more: false,
            graph: Rc::new(Vec::new()),
            unpushed: Rc::new(std::collections::HashSet::new()),
            all_branches: false,
            file: None,
            file_paths: Rc::new(Vec::new()),
            blame: false,
            blame_at: None,
            search,
            search_mode: templum_git::Search::default(),
            searched: String::new(),
            shown: None,
            showing: None,
            pending_lists: false,
            branches: Vec::new(),
            stashes: Vec::new(),
            stash_shown: None,
            filter,
            remote_folded: true,
            _filter,
            focus: cx.focus_handle(),
            lists_scroll: UniformListScrollHandle::new(),
            details_scroll: ScrollHandle::new(),
            compare_scroll: ScrollHandle::new(),
            stash_scroll: ScrollHandle::new(),
            lists_folded: false,
            split: false,
            files_filter,
            _files_filter,
            _search,
        }
    }

    /// What a commit of exactly one file says when the summary is left empty ("Update a.rs"; see
    /// `words::generic_summary`). None with no files or two or more (the file `commit_label`
    /// counts), on an Amend, whose files aren't listed, and during a merge, rebase, cherry-pick or
    /// revert, which prepare the message themselves.
    pub fn generic_summary(&self) -> Option<String> {
        if self.amend || self.operation.is_some() {
            return None;
        }
        let staged = !self.commit_all();
        let entries = if staged {
            self.status.staged()
        } else {
            self.status.unstaged()
        };
        match entries.as_slice() {
            [entry] => words::generic_summary(entry, staged),
            _ => None,
        }
    }

    /// The summary as the commit will carry it: what was typed, else the generic one.
    fn summary_text(&self, cx: &App) -> String {
        let typed = self.summary.read(cx).value().trim().to_string();
        if typed.is_empty() {
            self.generic_summary().unwrap_or_default()
        } else {
            typed
        }
    }

    /// The summary field's placeholder says what an empty summary will commit as.
    pub fn point_placeholder(&mut self, window: &mut Window, cx: &mut App) {
        let text: SharedString = self
            .generic_summary()
            .map_or(COMMIT_MESSAGE.into(), Into::into);
        if text != self.placeholder {
            self.placeholder = text.clone();
            self.summary
                .update(cx, |summary, cx| summary.set_placeholder(text, window, cx));
        }
    }

    /// The Filter files field shows.
    pub fn filtering(&self) -> bool {
        self.status.entries.len() > FILTER_PAST
    }

    /// The left column's rows: each list's header (Conflicts only when there are some), then its
    /// entries matching Filter files without case, by their index in the whole list (what the
    /// rows' ids, buttons and menus use). A filter left from before narrows nothing once the
    /// field is gone.
    pub fn listed(&self, cx: &App) -> Vec<(List, Option<usize>)> {
        let query = if self.filtering() {
            self.files_filter.read(cx).value().trim().to_lowercase()
        } else {
            String::new()
        };
        let s = &self.status;
        let mut items = Vec::new();
        for (which, entries) in [(List::Conflicts, s.conflicts()), (List::Files, s.files())] {
            if which == List::Conflicts && entries.is_empty() {
                continue;
            }
            items.push((which, None));
            items.extend(
                entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| query.is_empty() || e.path.to_lowercase().contains(&query))
                    .map(|(i, _)| (which, Some(i))),
            );
        }
        items
    }

    /// Commit takes every change (`git add --all` first): nothing staged, something changed, no
    /// conflict (adding one would mark it resolved), no Amend.
    pub fn commit_all(&self) -> bool {
        !self.amend
            && self.status.staged().is_empty()
            && !self.status.unstaged().is_empty()
            && self.status.conflicts().is_empty()
    }

    /// "Commit (3 files)", the files checked (wholly or in part); with none, "Commit all changes
    /// (5 files)"; "Commit" with nothing to count (an Amend of the message alone).
    pub fn commit_label(&self) -> String {
        let (label, n) = if self.commit_all() {
            ("Commit all changes", self.status.unstaged().len())
        } else {
            ("Commit", self.status.staged().len())
        };
        match n {
            0 => label.to_string(),
            1 => format!("{label} (1 file)"),
            n => format!("{label} ({n} files)"),
        }
    }

    /// The panel's section a file's click goes to: Staged when nothing of it is left unstaged,
    /// else Changes.
    pub fn in_staged(&self, path: &str) -> bool {
        self.status
            .entries
            .iter()
            .find(|e| e.path == path)
            .is_some_and(|e| e.staged.is_some() && e.unstaged.is_none())
    }

    pub fn summary_len(&self, cx: &App) -> usize {
        self.summary.read(cx).value().chars().count()
    }

    pub fn summary_too_long(&self, cx: &App) -> bool {
        self.summary_len(cx) > SUMMARY_MAX
    }

    /// Commit is possible: something staged (or every change, see `commit_all`), a summary (one
    /// file needs none, see `generic_summary`), an author, nothing running. With Amend, nothing
    /// staged is fine (the message alone changes).
    pub fn can_commit(&self, cx: &App) -> bool {
        (self.amend || !self.status.staged().is_empty() || self.commit_all())
            && !self.summary_text(cx).is_empty()
            && self.author.is_some()
            && self.acting == 0
    }

    /// The summary or the description is this entity: which block a Ctrl+Enter came from.
    pub fn owns_field(&self, field: EntityId) -> bool {
        self.summary.entity_id() == field || self.description.entity_id() == field
    }

    /// The diff panel on screen: each tab its own (the Branches tab's is the comparison's while
    /// one shows), so a 2-second read of the working changes never touches another tab's.
    pub fn diff(&self) -> &diff_view::DiffPanel {
        match (self.tab, &self.compare) {
            (Tab::History, _) => &self.history_panel,
            (Tab::Stashes, _) => &self.stash_panel,
            (Tab::Branches, Some(c)) => &c.panel,
            _ => &self.panel,
        }
    }

    pub fn diff_mut(&mut self) -> &mut diff_view::DiffPanel {
        match (self.tab, &mut self.compare) {
            (Tab::History, _) => &mut self.history_panel,
            (Tab::Stashes, _) => &mut self.stash_panel,
            (Tab::Branches, Some(c)) => &mut c.panel,
            _ => &mut self.panel,
        }
    }

    /// History leaves the commit shown (or being read): the list again, the panel empty unless
    /// it shows the file's blame.
    pub fn hide_shown(&mut self) {
        self.shown = None;
        self.showing = None;
        if !self.blame {
            self.history_panel.clear();
        }
    }

    /// Split on or off, for every panel of the block.
    pub fn set_split(&mut self, on: bool) {
        self.split = on;
        let panels = [
            &mut self.panel,
            &mut self.history_panel,
            &mut self.stash_panel,
        ]
        .into_iter()
        .chain(self.compare.as_mut().map(|c| &mut c.panel));
        for p in panels {
            p.split = on;
            p.rebuild();
        }
    }

    /// The working panel's conflicted file at `path`, as `conflict_diff` made it.
    fn conflict_file(&self, path: &str) -> Option<&templum_git::FileDiff> {
        self.panel
            .files
            .iter()
            .map(|s| &s.diff)
            .find(|d| d.status == templum_git::Change::Conflict && d.path() == path)
    }

    /// The labels of a conflicted file's parts (its first region's): what the whole-file
    /// choices are named after. "HEAD" and "the other side" when it has no region to read.
    pub fn conflict_labels(&self, path: &str) -> (String, String) {
        match self.conflict_file(path).map(|d| &d.hunks[..]) {
            Some([first, second, ..]) => (first.context.clone(), second.context.clone()),
            _ => ("HEAD".to_string(), "the other side".to_string()),
        }
    }

    /// The file still has conflict markers (as last read).
    pub fn markers_left(&self, path: &str) -> bool {
        self.conflict_file(path)
            .is_some_and(|d| !d.hunks.is_empty())
    }

    /// The Git of a mirrored workspace, looked for from `folder` at home. It holds no `Repo` and
    /// its `folder` is empty: no code path of this computer can stat or run git on it.
    pub fn at_home(
        folder: String,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Git {
        let mut git = Git::new(PathBuf::new(), tab, window, cx);
        git.gone = false;
        git.home = Some(folder);
        git
    }

    pub fn repo(&self) -> Option<&Repo> {
        match &self.found {
            Found::Repo(repo) => Some(repo),
            _ => None,
        }
    }

    /// A repository was found, here or at home.
    pub fn has_repo(&self) -> bool {
        matches!(self.found, Found::Repo(_) | Found::Home(_))
    }

    /// The repository's root, else the folder: this computer's path as it displays, or home's as
    /// it wrote it.
    pub fn place(&self) -> String {
        match (&self.found, &self.home) {
            (Found::Home(root), _) => root.clone(),
            (_, Some(home)) => home.clone(),
            _ => self
                .repo()
                .map_or(&self.folder, |r| &r.root)
                .display()
                .to_string(),
        }
    }

    /// The repository's folder name, else the chosen folder's.
    pub fn name(&self) -> String {
        if self.home.is_some() {
            return home_name(&self.place());
        }
        let dir = self.repo().map_or(&self.folder, |r| &r.root);
        dir.file_name().map_or_else(
            || dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// As the canvas file holds it. The commit message being written is kept, unless Amend
    /// filled it with the last commit's.
    pub fn saved(&self, cx: &App) -> session::SavedGit {
        let draft = |text: &str| {
            if self.amend {
                String::new()
            } else {
                text.to_string()
            }
        };
        session::SavedGit {
            folder: self.folder.to_string_lossy().into_owned(),
            tab: self.tab.name().to_string(),
            summary: draft(&self.summary.read(cx).value()),
            description: draft(&self.description.read(cx).value()),
            lists: if self.lists_folded {
                "folded".to_string()
            } else {
                String::new()
            },
            diff: if self.split {
                "split".to_string()
            } else {
                String::new()
            },
        }
    }
}

/// The last part of one of home's paths, split on `/` and `\`; the whole path when it has none.
pub(crate) fn home_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

pub(crate) fn row_id(list: &str, i: usize) -> ElementId {
    ElementId::Name(format!("git-row-{list}-{i}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use templum_git::Change;

    #[test]
    fn home_s_folder_is_named_by_its_last_part() {
        assert_eq!(home_name("/home/ada/app"), "app");
        assert_eq!(home_name("C:\\Users\\ada\\app\\"), "app");
        assert_eq!(home_name("/"), "/");
    }

    #[test]
    fn a_files_box_is_what_of_it_is_staged() {
        let entry = |staged, unstaged| Entry {
            path: "a.txt".to_string(),
            from: None,
            staged,
            unstaged,
        };
        let m = Some(Change::Modified);
        assert_eq!(Check::of(&entry(m, None)), Check::Checked);
        assert_eq!(Check::of(&entry(m, m)), Check::Partial);
        assert_eq!(Check::of(&entry(None, m)), Check::Unchecked);
        assert_eq!(
            Check::of(&entry(None, Some(Change::Untracked))),
            Check::Unchecked
        );
    }

    #[test]
    fn the_popup_is_drawn_at_the_size_set_inside_the_minimum_and_the_room() {
        let big = Size::new(2000.0, 1500.0);
        // Nothing set: today's size, or the room when the window has less.
        assert_eq!(fit(None, big), SIZE);
        assert_eq!(fit(None, Size::new(700.0, 500.0)), Size::new(700.0, 500.0));
        // A set size is drawn as it is while it fits.
        assert_eq!(
            fit(Some(Size::new(900.0, 650.0)), big),
            Size::new(900.0, 650.0)
        );
        // Too small a set size is lifted to the minimum; too large, cut to the room.
        assert_eq!(fit(Some(Size::new(10.0, 10.0)), big), MIN);
        let room = Size::new(1000.0, 600.0);
        assert_eq!(fit(Some(Size::new(2000.0, 2000.0)), room), room);
        // A window with less room than the minimum wins over it.
        let tiny = Size::new(400.0, 300.0);
        assert_eq!(fit(Some(Size::new(900.0, 650.0)), tiny), tiny);
        // The size set isn't changed by drawing it smaller: the same set size fits again in a
        // window that grew back.
        let set = Some(Size::new(1400.0, 900.0));
        assert_eq!(fit(set, Size::new(800.0, 500.0)), Size::new(800.0, 500.0));
        assert_eq!(fit(set, big), Size::new(1400.0, 900.0));
    }
}
