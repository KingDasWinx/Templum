//! Claude Code's subagents in blocks of their own (docs/specs/claude-subagents.md): one opens
//! beside its parent as it starts, wired to it, a terminal that runs nothing and shows its
//! transcript as it grows; it closes a minute after it finished, unless the person reads it.
//! Never saved: it belongs to a live session.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::{Context, Window};
use templum_agent::follow::Follower;
use templum_canvas::chrome;
use templum_mcp::Reply;
use templum_terminal::Terminal;

use super::ops::Place;
use super::{Pane, Workspace};

/// A subagent block's size, in cells.
const COLS: u16 = 90;
const ROWS: u16 = 22;
/// How often its transcript is read again.
const EVERY: Duration = Duration::from_millis(250);
/// Its first line, dim.
const INTRO: &str = "\x1b[2mA subagent of Claude Code, read-only: what it asks is answered in its \
                     Claude Code's block.\x1b[0m\r\n";
/// How long a finished subagent's block stays once the person no longer reads it.
pub(super) const CLOSES_AFTER: Duration = Duration::from_secs(60);
/// How often a finished subagent's block is looked at.
const LOOK: Duration = Duration::from_secs(1);

/// The subagent a block shows: read-only, its parent's.
pub(crate) struct Follows {
    /// Its parent's terminal (`TEMPLUM_TERMINAL`).
    pub parent: String,
    /// The subagent's id.
    pub id: String,
    /// When it finished, or when the person last read it since: CLOSES_AFTER later it closes.
    pub done: Option<Instant>,
}

/// The transcript Claude Code writes for its subagent `id`, beside its session's `transcript`
/// (documented); none for an id that isn't one (its file name is made of it).
pub(super) fn transcript_of(transcript: &str, id: &str) -> Option<PathBuf> {
    let id_ok = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    id_ok.then(|| {
        Path::new(transcript)
            .with_extension("")
            .join("subagents")
            .join(format!("agent-{id}.jsonl"))
    })
}

impl Pane {
    /// It shows a subagent of the agent in `parent` (its terminal).
    pub fn follows_of(&self, parent: &str) -> bool {
        self.follows.as_ref().is_some_and(|f| f.parent == parent)
    }

    /// Nothing typed, pasted or pressed reaches it (its block `uid`): it shows a subagent, which
    /// has no terminal of its own.
    pub fn read_only(&self, uid: &str) -> Result<(), Reply> {
        match self.follows {
            Some(_) => Err(Reply::err(format!(
                "{uid} shows a Claude Code subagent's transcript, read-only: nothing typed or \
                 pressed there reaches the subagent. Its dialogs are answered in its Claude Code's \
                 block; read_terminal reads it."
            ))),
            None => Ok(()),
        }
    }
}

impl Workspace {
    /// The block showing the subagent `id` of the agent in the terminal `parent`.
    fn follower_mut(&mut self, parent: &str, id: &str) -> Option<&mut Pane> {
        std::iter::once(&mut self.space)
            .chain(&mut self.others)
            .flat_map(|s| s.blocks.values_mut())
            .filter_map(super::Body::terminal_mut)
            .find(|p| {
                p.follows
                    .as_ref()
                    .is_some_and(|f| f.parent == parent && f.id == id)
            })
    }

    /// The subagent `id` of type `kind` started in the agent of the terminal `parent`, whose
    /// session's transcript is `transcript`: a block beside that agent's, wired to it, follows its
    /// transcript. The keyboard, the selection and the camera stay, and Ctrl+Z doesn't take it
    /// away. One already open for it (a subagent resumed) works again.
    pub(super) fn open_subagent(
        &mut self,
        parent: &str,
        (id, kind): (&str, &str),
        transcript: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(pane) = self.follower_mut(parent, id) {
            if let Some(follows) = pane.follows.as_mut() {
                follows.done = None;
            }
            return;
        }
        let Some(file) = transcript.and_then(|t| transcript_of(t, id)) else {
            return;
        };
        let Some((space, me)) = self.caller(parent) else {
            return;
        };
        let folder = self
            .space_named(&space)
            .and_then(|s| s.terminal(me))
            .and_then(|p| p.seen.folder.clone());
        let (parent, id) = (parent.to_string(), id.to_string());
        let opened = self.in_space(&space, |ws| {
            let size = chrome::block_size(COLS, ROWS, ws.cell.size);
            let center = ws.space.camera().to_world(ws.center(window));
            let rect = ws.space.spot(&Place::Near(me), size, center)?;
            let grid = chrome::grid_size(chrome::content_rect(rect).size(), ws.cell.size);
            let name = if kind.is_empty() { "Subagent" } else { kind };
            let term = Terminal::viewer(
                name,
                grid,
                ws.theme.palette,
                ws.config.term_options(),
                ws.notify.clone(),
            );
            term.feed(INTRO.as_bytes());
            let terminal = term.id().to_string();
            let kept = (
                ws.space.focused,
                ws.space.selected.clone(),
                ws.space.selected_wire,
            );
            // In grid mode it takes half of its parent's tile.
            ws.space.focused = Some(me);
            ws.quiet = true;
            let block = ws.add_pane(rect, term, folder, None);
            ws.quiet = false;
            (ws.space.focused, ws.space.selected, ws.space.selected_wire) = kept;
            ws.space.add_wire(me, block);
            let opener = ws.space.uids.get(&me).cloned();
            let pane = ws.space.terminal_mut(block)?;
            pane.opened_by = opener;
            pane.follows = Some(Follows {
                parent,
                id,
                done: None,
            });
            Some(terminal)
        });
        if let Some(terminal) = opened.flatten() {
            self.follow(terminal, Follower::new(file, kind), cx);
        }
        cx.notify();
    }

    /// The block of the terminal `terminal` shows what `follower` reads of its transcript, EVERY
    /// once more, off the UI thread, until it closes.
    fn follow(&self, terminal: String, mut follower: Follower, cx: &mut Context<Self>) {
        cx.spawn(async move |ws, cx| {
            loop {
                let read = cx.background_executor().spawn(async move {
                    let text = follower.read();
                    (follower, text)
                });
                let text;
                (follower, text) = read.await;
                let open = ws.update(cx, |ws, _| {
                    let Some(pane) = ws.pane(&terminal) else {
                        return false;
                    };
                    if !text.is_empty() {
                        // The terminal's new lines: it doesn't turn \n into \r\n.
                        pane.term.feed(text.replace('\n', "\r\n").as_bytes());
                    }
                    true
                });
                if !open.unwrap_or(false) {
                    break;
                }
                cx.background_executor().timer(EVERY).await;
            }
        })
        .detach();
    }

    /// The subagent `id` of the agent in the terminal `parent` finished, or every one of its
    /// subagents when None (its session ended): their blocks say so, and close CLOSES_AFTER later
    /// unless the person reads them.
    pub(super) fn subagents_done(
        &mut self,
        parent: &str,
        id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = cx.background_executor().now();
        let finished: Vec<String> = std::iter::once(&mut self.space)
            .chain(&mut self.others)
            .flat_map(|s| s.blocks.values_mut())
            .filter_map(super::Body::terminal_mut)
            .filter_map(|pane| {
                let follows = pane.follows.as_mut()?;
                let ours = follows.parent == parent && id.is_none_or(|id| follows.id == id);
                if !ours || follows.done.is_some() {
                    return None;
                }
                follows.done = Some(now);
                Some(pane.term.id().to_string())
            })
            .collect();
        for terminal in finished {
            cx.spawn_in(window, async move |ws, cx| {
                loop {
                    cx.background_executor().timer(LOOK).await;
                    let looking = ws
                        .update_in(cx, |ws, window, cx| ws.close_unread(&terminal, window, cx))
                        .unwrap_or(false);
                    if !looking {
                        break;
                    }
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// A look at the finished subagent block of the terminal `terminal`: read now (on screen, and
    /// focused, pointed at, selected, or scrolled back from its end), its minute starts over; unread for
    /// CLOSES_AFTER, it closes. False once there is nothing more to look for: it closed, it is
    /// gone, or it works again.
    fn close_unread(
        &mut self,
        terminal: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((space_id, block)) = self.caller(terminal) else {
            return false;
        };
        let now = cx.background_executor().now();
        // Only on screen: a hidden workspace keeps its focus and selection from when it was shown.
        let space = &self.space;
        let reading = space.id == space_id
            && (space.focused == Some(block)
                || space.hovered == Some(block)
                || space.selected.contains(&block)
                || space
                    .terminal(block)
                    .is_some_and(|p| p.term.scrolled_back()));
        let Some(follows) = self.pane_mut(terminal).and_then(|p| p.follows.as_mut()) else {
            return false;
        };
        let Some(done) = follows.done else {
            return false;
        };
        if reading {
            follows.done = Some(now);
            return true;
        }
        if now < done + CLOSES_AFTER {
            return true;
        }
        self.in_space(&space_id, |ws| ws.remove_block(block, window, cx));
        false
    }

    /// The blocks of the subagents of the agent in the terminal `parent`, closed with it.
    pub(super) fn close_subagents_of(
        &mut self,
        parent: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theirs: Vec<(String, templum_canvas::BlockId)> = self
            .spaces()
            .flat_map(|s| {
                s.terminals()
                    .filter(|(_, p)| p.follows_of(parent))
                    .map(|(id, _)| (s.id.clone(), id))
            })
            .collect();
        for (space, id) in theirs {
            self.in_space(&space, |ws| ws.remove_block(id, window, cx));
        }
    }
}
