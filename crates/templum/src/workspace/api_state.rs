//! The state the API shows (docs/specs/api.md, Resources), built from the workspaces on the UI
//! thread: walked as `overview` walks them, so a feature that is in the state is on the event
//! stream without more code.

use std::sync::Arc;
use std::time::{Instant, SystemTime};

use gpui_kit::App;
use templum_api::model::{
    AgentView, App as AppView, Assignee, AttachmentView, AutomationView, BlockView, BoardView,
    CardRef, CardView, ChecklistView, ColumnView, CommentView, DrawingView, DrawingsView, ItemView,
    LastView, LimitsView, NOTE_TEXT_MOST, NOTES_TEXT_MOST, NoteView, NotesShown, OptionView,
    PlanView, QuestionView, Rect, RequestView, ScheduleView, SessionView, ShareView, State,
    TaskView, UsageView, ViewView, Why, WindowView, WireView, WorkspaceView, json_len,
    newest_drawings,
};
use templum_api::store::Scope;

use templum_canvas::Drawing;

use super::agents_view::Doing;
use super::waits::{Approved, Waiting};
use super::{Body, Space, Workspace};
use crate::note::Note;
use crate::notes;
use crate::session::InkItem;

/// Unix seconds for an instant of the executor's clock, given the clock's `now` and the time.
fn unix_of(at: Instant, now: Instant, wall: i64) -> i64 {
    wall - now.saturating_duration_since(at).as_secs() as i64
}

fn some(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

impl Workspace {
    /// Everything the API shows, as of now.
    pub(crate) fn api_state(&self, cx: &App) -> State {
        self.state_of(cx, true)
    }

    /// The state without the notes, checklists and ink (empty): what a rule over blocks,
    /// workspaces, boards and automations reads, without copying the content.
    pub(super) fn api_scope(&self, cx: &App) -> State {
        self.state_of(cx, false)
    }

    fn state_of(&self, cx: &App, content: bool) -> State {
        let (now, wall) = (
            cx.background_executor().now(),
            super::automations::unix(SystemTime::now()),
        );
        let spaces = self.order();
        State {
            app: AppView {
                version: env!("CARGO_PKG_VERSION").to_string(),
                api: 1,
                // None on the Home: no workspace is.
                shown: self.shown_space().map(|s| s.id.clone()),
            },
            workspaces: spaces
                .iter()
                .map(|space| WorkspaceView {
                    id: space.id.clone(),
                    name: space.name.clone(),
                    folder: space.folder.as_ref().map(|f| f.display().to_string()),
                    color: space.look.color.clone(),
                    shown: self.shown_space().is_some_and(|s| s.id == space.id),
                    stopped: space.stopped,
                    icon: space
                        .look
                        .icon
                        .map(|icon| crate::space_dialog::icon_name(icon).to_string()),
                    opened: space.opened.and_then(|t| i64::try_from(t).ok()),
                    remote: space
                        .remote
                        .as_deref()
                        .map(super::remote::RemoteSpace::view),
                })
                .map(Arc::new)
                .collect(),
            blocks: spaces
                .iter()
                .flat_map(|space| self.block_views(space, now, wall, cx))
                .map(Arc::new)
                .collect(),
            wires: spaces
                .iter()
                .flat_map(|space| wire_views(space))
                .map(Arc::new)
                .collect(),
            requests: arcs(self.request_views(now, wall)),
            boards: self
                .boards
                .boards
                .iter()
                .map(|board| BoardView {
                    id: board.id,
                    name: board.name.clone(),
                    columns: board
                        .columns
                        .iter()
                        .map(|c| ColumnView {
                            id: c.id,
                            title: c.title.clone(),
                            cards: c.cards.iter().map(|card| card.id).collect(),
                        })
                        .collect(),
                })
                .map(Arc::new)
                .collect(),
            cards: self
                .boards
                .boards
                .iter()
                .flat_map(|board| board.columns.iter().map(move |c| (board.id, c)))
                .flat_map(|(board, column)| {
                    column
                        .cards
                        .iter()
                        .map(move |card| card_view(board, column.id, card))
                })
                .map(Arc::new)
                .collect(),
            notes: arcs(if content {
                self.note_views(&spaces)
            } else {
                Vec::new()
            }),
            checklists: arcs(if content {
                self.checklist_views(&spaces)
            } else {
                Vec::new()
            }),
            drawings: arcs(if content {
                drawing_views(&spaces)
            } else {
                Vec::new()
            }),
            automations: arcs(self.automation_views(cx)),
            schedules: arcs(self.schedule_views()),
            shares: arcs(self.share_views()),
            view: self.view_of_shown(),
            usage: (!self.limits.is_empty()).then(|| UsageView {
                limits: self
                    .limits
                    .iter()
                    .map(|(agent, limits)| LimitsView {
                        agent: agent.clone(),
                        at: limits.at,
                        windows: limits
                            .windows
                            .iter()
                            .map(|w| WindowView {
                                minutes: w.minutes,
                                used_percent: w.used_percent,
                                resets_at: w.resets_at,
                                scope: w.scope.clone(),
                            })
                            .collect(),
                    })
                    .collect(),
            }),
        }
    }

    /// The notes a note block of this computer shows, each once, in the order of the workspaces and
    /// the tabs; a text past `NOTE_TEXT_MOST` bytes is cut at a character's boundary, and once the
    /// texts sent reach `NOTES_TEXT_MOST` (as JSON) the rest come with `cut` and no text.
    fn note_views(&self, spaces: &[&Space]) -> Vec<NoteView> {
        let mut left = NOTES_TEXT_MOST;
        let mut seen = std::collections::HashSet::new();
        let ids: Vec<u64> = bodies(spaces)
            .filter_map(Body::note)
            .flat_map(Note::ids)
            .filter(|id| seen.insert(*id))
            .collect();
        let mut views: Vec<NoteView> = ids
            .into_iter()
            .filter_map(|id| self.notes.get(id))
            .map(|entry| {
                let mut end = entry.text.len().min(NOTE_TEXT_MOST);
                while !entry.text.is_char_boundary(end) {
                    end -= 1;
                }
                let mut text = &entry.text[..end];
                let size = json_len(text);
                if size > left {
                    text = "";
                } else {
                    left -= size;
                }
                NoteView {
                    id: entry.id,
                    name: entry.name.clone(),
                    text: text.to_string(),
                    color: entry.color.map(|label| label.name().to_string()),
                    cut: text.len() < entry.text.len(),
                    rev: entry.rev,
                    changed: entry.changed,
                    workspace: None,
                }
            })
            .collect();
        // Then the notes no block shows that were last open in one of this computer's own
        // workspaces, the most recently changed first: what lets a share reopen them. They carry
        // no text, so they take nothing of the budget.
        let mut closed: Vec<&notes::Entry> = self
            .notes
            .notes
            .iter()
            .filter(|n| !seen.contains(&n.id))
            .filter(|n| {
                n.closed_in
                    .as_deref()
                    .and_then(|w| self.space_named(w))
                    .is_some_and(|w| !w.is_remote())
            })
            .collect();
        closed.sort_by_key(|n| std::cmp::Reverse((n.changed, n.id)));
        views.extend(closed.into_iter().map(|entry| NoteView {
            id: entry.id,
            name: entry.name.clone(),
            text: String::new(),
            color: entry.color.map(|label| label.name().to_string()),
            cut: false,
            rev: entry.rev,
            changed: entry.changed,
            workspace: entry.closed_in.clone(),
        }));
        views
    }

    /// The checklists a checklist block of this computer shows, each once.
    fn checklist_views(&self, spaces: &[&Space]) -> Vec<ChecklistView> {
        let mut seen = std::collections::HashSet::new();
        bodies(spaces)
            .filter_map(Body::checklist)
            .filter_map(|checklist| self.checklists.get(checklist.list?))
            .filter(|list| seen.insert(list.id))
            .map(|list| ChecklistView {
                id: list.id,
                name: list.name_shown().to_string(),
                items: list
                    .items
                    .iter()
                    .map(|item| ItemView {
                        id: item.id,
                        text: item.text.clone(),
                        done: item.done,
                    })
                    .collect(),
            })
            .collect()
    }

    /// The shares: the tokens that were made by sharing a workspace, oldest first, with whether an
    /// event stream of each is open now (asked of the server, no state built). A share of a
    /// workspace that is gone is still here, naming it.
    pub(crate) fn share_views(&self) -> Vec<ShareView> {
        let Some(store) = self.api.store.as_ref() else {
            return Vec::new();
        };
        let mirrored = self
            .api
            .server
            .as_ref()
            .map(|server| server.mirrored())
            .unwrap_or_default();
        store
            .tokens
            .iter()
            .filter_map(|token| {
                let share = token.share.as_ref()?;
                Some(ShareView {
                    id: token.share_id()?,
                    workspace: share.workspace.clone(),
                    name: token.name.clone(),
                    scope: token.scope,
                    created: token.created,
                    expires: token.expires,
                    way: share.way,
                    address: share.address.clone(),
                    last: token.last.as_ref().map(|used| LastView {
                        at: used.at,
                        addr: used.addr.clone(),
                    }),
                    mirrored: mirrored.iter().any(|(id, _)| *id == token.id),
                })
            })
            .collect()
    }

    /// What the shown workspace's canvas shows: the world rectangle at its camera and the canvas's
    /// size at the last paint; None before the first, in grid mode (tiles, not the canvas) and on
    /// the Home.
    fn view_of_shown(&self) -> Option<ViewView> {
        let (viewport, camera) = (self.api.viewport, &self.space.canvas.camera);
        if viewport.w <= 0.0 || viewport.h <= 0.0 || self.space.grid.is_some() || self.home_up() {
            return None;
        }
        let zoom = camera.zoom();
        Some(ViewView {
            workspace: self.space.id.clone(),
            x: camera.origin.x,
            y: camera.origin.y,
            w: viewport.w / zoom,
            h: viewport.h / zoom,
            zoom,
        })
    }

    /// The blocks of a workspace, back to front.
    fn block_views(&self, space: &Space, now: Instant, wall: i64, cx: &App) -> Vec<BlockView> {
        space
            .canvas
            .blocks()
            .iter()
            .enumerate()
            .filter_map(|(z, block)| {
                let body = space.blocks.get(&block.id)?;
                let (kind, title, folder) = self.about(body, cx);
                let rect = space.home(block);
                let pane = space.terminal(block.id);
                Some(BlockView {
                    id: space.uids.get(&block.id)?.clone(),
                    workspace: space.id.clone(),
                    kind: kind.to_string(),
                    title,
                    folder: folder.map(|f| f.display().to_string()),
                    rect: Rect {
                        x: rect.x,
                        y: rect.y,
                        w: rect.w,
                        h: rect.h,
                    },
                    z,
                    opened_by: pane.and_then(|p| p.opened_by.clone()),
                    board: space.kanban(block.id).and_then(|k| k.board),
                    notes: body.note().map(|note| NotesShown {
                        tabs: note.ids(),
                        shown: note.tab().map(|tab| tab.id),
                    }),
                    checklist: body
                        .checklist()
                        .and_then(|c| c.list)
                        .filter(|list| self.checklists.get(*list).is_some()),
                    read_only: pane.is_some_and(|p| p.follows.is_some()),
                    agent: pane
                        .filter(|p| p.agent().is_some() && p.follows.is_none())
                        .and_then(|p| self.agent_view(p, now, wall)),
                })
            })
            .collect()
    }

    /// An agent as the Agents view tells it: its state is the dot's, `done` when its turn ended
    /// unseen.
    fn agent_view(&self, pane: &super::Pane, now: Instant, wall: i64) -> Option<AgentView> {
        let (_, since) = pane.watch.shown()?;
        let doing = match self.doing(pane) {
            Doing::Idle if pane.watch.is_done() => Doing::Done,
            doing => doing,
        };
        let (state, asks, subagents) = match &doing {
            Doing::NeedsYou(asks) => ("needs_you", some(asks), 0),
            Doing::Starting => ("starting", None, 0),
            Doing::AwaitsSubagents(n) => ("waiting_subagents", None, *n),
            Doing::Working(n) => ("working", None, *n),
            Doing::Done => ("done", None, 0),
            Doing::Idle => ("idle", None, 0),
        };
        let session = pane.usage.as_ref();
        Some(AgentView {
            agent: pane.agent()?.to_string(),
            state: state.to_string(),
            asks,
            subagents,
            since: Some(unix_of(since, now, wall)),
            model: pane
                .model
                .clone()
                .or_else(|| session.and_then(|s| s.model.clone())),
            effort: pane.effort.clone(),
            usage: session.map(|s| SessionView {
                context_percent: s.context_percent,
                tokens: s.tokens,
                cost_usd: s.cost_usd,
            }),
            task: pane
                .handed
                .as_ref()
                .filter(|handed| !handed.text.is_empty())
                .map(|handed| TaskView {
                    id: format!("t{}", handed.id),
                    text: handed.text.clone(),
                    by: if handed.by == templum_api::ASKER {
                        Some(handed.by.clone())
                    } else {
                        self.uid_of_terminal(&handed.by)
                    },
                    since: unix_of(handed.since, now, wall),
                }),
            role: pane.role.clone(),
        })
    }

    /// The uid of the block a terminal id (`w1/abc12`-style, as `caller` takes it) is, if it still is one.
    fn uid_of_terminal(&self, terminal: &str) -> Option<String> {
        let (space, id) = self.caller(terminal)?;
        self.space_named(&space)?.uids.get(&id).cloned()
    }

    /// The deliveries agents asked for later, in the order they were asked.
    fn schedule_views(&self) -> Vec<ScheduleView> {
        self.schedules
            .iter()
            .map(|s| ScheduleView {
                id: format!("s{}", s.id),
                by: self.uid_of_terminal(&s.by),
                to: s.to.clone(),
                text: s.text.clone(),
                next: super::automations::unix(s.next),
                every: s.every.map(|every| every.as_secs()),
                until: super::automations::unix(s.until),
            })
            .collect()
    }

    /// What waits for the person: the inbox's requests and the Templum Agent's approvals.
    fn request_views(&self, now: Instant, wall: i64) -> Vec<RequestView> {
        let place = |terminal: &str| {
            self.caller(terminal).map(|(space, id)| {
                let uid = self
                    .space_named(&space)
                    .and_then(|s| s.uids.get(&id).cloned());
                (uid, space)
            })
        };
        let mut views: Vec<RequestView> = self
            .waits
            .iter()
            .filter_map(|wait| {
                // Another computer's requests are shown here, never relayed: this computer's API
                // doesn't list them and can't answer them.
                if wait.remote.is_some() {
                    return None;
                }
                let (kind, buttons, scope) = match &wait.what {
                    Waiting::Question { ask, .. } if ask.plan.is_some() => {
                        ("plan", None, Scope::Control)
                    }
                    Waiting::Question { .. } => ("question", None, Scope::Control),
                    Waiting::Allow { then } => {
                        let (yes, no) = then.buttons();
                        let buttons = Some([yes, no]);
                        match then {
                            Approved::Connect { .. } => ("connect", buttons, Scope::Control),
                            Approved::RemoveWorktree { .. } => {
                                ("remove_worktree", buttons, Scope::Full)
                            }
                            Approved::MergeWorktree { .. } => {
                                ("merge_worktree", buttons, Scope::Full)
                            }
                            Approved::Remote { .. } => return None,
                        }
                    }
                    Waiting::Turn { .. } | Waiting::Event { .. } => return None,
                };
                let (block, workspace) = match place(&wait.from) {
                    Some((uid, space)) => (uid, Some(space)),
                    None => (None, None),
                };
                let why = self.why(wait);
                let (questions, plan, stopped) = match &wait.what {
                    Waiting::Question { ask, stopped, .. } => (
                        ask.questions
                            .iter()
                            .map(|q| QuestionView {
                                question: q.text.clone(),
                                header: q.header.clone(),
                                multi: q.multi,
                                options: q
                                    .options
                                    .iter()
                                    .map(|o| OptionView {
                                        label: o.label.clone(),
                                        description: o.description.clone(),
                                        preview: o.preview.clone(),
                                    })
                                    .collect(),
                            })
                            .collect(),
                        ask.plan.as_ref().map(|p| PlanView {
                            title: p.title.clone(),
                            text: p.text.clone(),
                        }),
                        *stopped,
                    ),
                    _ => (Vec::new(), None, false),
                };
                Some(RequestView {
                    id: format!("r{}", wait.key),
                    kind: kind.to_string(),
                    block,
                    workspace,
                    since: unix_of(wait.since, now, wall),
                    summary: wait.summary(),
                    why: Why {
                        card: why.card.map(|(id, title)| CardRef { id, title }),
                        task: why.task,
                        agent: why.agent,
                    },
                    questions,
                    plan,
                    buttons,
                    stopped,
                    scope,
                    ..Default::default()
                })
            })
            .collect();
        views.extend(
            self.templum_agent
                .approvals
                .iter()
                .map(|approval| RequestView {
                    id: format!("a{}", approval.id),
                    kind: "approval".into(),
                    since: approval.shown.get().map_or(0, |at| unix_of(at, now, wall)),
                    summary: approval.said.clone(),
                    scope: Scope::Full,
                    tool: Some(approval.tool.clone()),
                    said: Some(approval.said.clone()),
                    details: Some(approval.details.clone()),
                    facts: Some(approval.facts.clone()),
                    reason: approval.parsed.reason.clone(),
                    ..Default::default()
                }),
        );
        views
    }

    fn automation_views(&self, cx: &App) -> Vec<AutomationView> {
        self.automations
            .automations
            .iter()
            .map(|a| AutomationView {
                id: a.id,
                name: a.name.clone(),
                on: a.on,
                workspace: a.workspace.clone(),
                when: serde_json::to_value(&a.when).unwrap_or_default(),
                act: serde_json::to_value(&a.act).unwrap_or_default(),
                max_runs: a.max_runs,
                cooldown: a.when.is_event().then_some(a.cooldown),
                runs: a.runs,
                last: a
                    .last
                    .as_ref()
                    .and_then(|last| serde_json::to_value(last).ok()),
                next: self
                    .automation_runs
                    .get(&a.id)
                    .and_then(|run| run.next)
                    .map(super::automations::unix),
                sentence: self.automation_sentence(a, cx),
            })
            .collect()
    }
}

/// Home hands out its state as `Arc` items, so a mirror takes it by pointer.
fn arcs<T>(items: Vec<T>) -> Vec<Arc<T>> {
    items.into_iter().map(Arc::new).collect()
}

/// The blocks' bodies of each workspace, back to front.
fn bodies<'a>(spaces: &'a [&Space]) -> impl Iterator<Item = &'a Body> {
    spaces.iter().flat_map(|space| {
        space
            .canvas
            .blocks()
            .iter()
            .filter_map(|block| space.blocks.get(&block.id))
    })
}

/// The ink of each workspace of this computer that has some, the newest within the caps.
fn drawing_views(spaces: &[&Space]) -> Vec<DrawingsView> {
    let points = |drawing: &Drawing| match drawing {
        Drawing::Stroke { points, .. } => points.len(),
        Drawing::Shape { .. } => 2,
    };
    spaces
        .iter()
        .filter(|space| !space.is_remote() && !space.ink.is_empty())
        .map(|space| DrawingsView {
            id: space.id.clone(),
            drawings: newest_drawings(&space.ink, points)
                .iter()
                .map(|drawing| DrawingView::from(&InkItem::of(drawing)))
                .collect(),
        })
        .filter(|view| !view.drawings.is_empty())
        .collect()
}

fn wire_views(space: &Space) -> Vec<WireView> {
    space
        .wires
        .iter()
        .filter_map(|wire| {
            let (a, b) = (
                space.uids.get(&wire.a)?.clone(),
                space.uids.get(&wire.b)?.clone(),
            );
            let key = if a <= b {
                (a.clone(), b.clone())
            } else {
                (b.clone(), a.clone())
            };
            Some(WireView {
                a,
                b,
                label: space.labels.get(&key).cloned(),
            })
        })
        .collect()
}

fn card_view(board: u64, column: u64, card: &crate::board::Card) -> CardView {
    CardView {
        id: card.id,
        board,
        column,
        title: card.title.clone(),
        description: card.description.clone(),
        labels: card.labels.iter().map(|l| l.name().to_string()).collect(),
        priority: card.priority.map(|p| p.name().to_string()),
        assignee: card.assignee.as_ref().map(|a| match a {
            crate::board::Assignee::Block(uid) => Assignee {
                block: Some(uid.clone()),
                agent: None,
            },
            crate::board::Assignee::Agent(agent) => Assignee {
                block: None,
                agent: Some(agent.clone()),
            },
        }),
        checklist: card
            .checklist
            .iter()
            .map(|i| ItemView {
                id: i.id,
                text: i.text.clone(),
                done: i.done,
            })
            .collect(),
        comments: card
            .comments
            .iter()
            .map(|c| CommentView {
                number: c.id,
                by: c.by.clone(),
                text: c.text.clone(),
                at: c.at as i64,
                edited: c.edited,
            })
            .collect(),
        attachments: card
            .attachments
            .iter()
            .map(|a| AttachmentView {
                name: a.name.clone(),
                size: a.size,
            })
            .collect(),
    }
}
