//! The workspaces: loading, switching, adding, renaming and deleting them, and their index.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, ImageSource, Window};
use templum_canvas::{BlockId, Camera, Point, Rect, Size, chrome};

use crate::alert;
use crate::attachments;
use crate::automations;
use crate::board;
use crate::checklists;
use crate::instructions;
use crate::notes;
use crate::prompt;
use crate::session;
use crate::space_dialog::{self, Look};

use super::remote::{Deed, RemoteSpace};
use super::{Body, DEFAULT_COLS, DEFAULT_ROWS, Space, Tool, Workspace, show_notification};

/// How long a workspace's processes have, once hung up, before what is left is killed.
const KILL_AFTER: Duration = Duration::from_secs(2);

/// Every process of a workspace's terminals, read now, while they still name their parents. A
/// shell that exited is left out: its pid may be another program's by now.
fn tree_of(space: &Space) -> Vec<templum_terminal::Running> {
    let shells: Vec<u32> = space
        .terminals()
        .filter(|(_, pane)| !pane.term.exited())
        .filter_map(|(_, pane)| pane.term.processes().shell)
        .collect();
    if shells.is_empty() {
        return Vec::new();
    }
    templum_terminal::tree(&shells)
}

impl Workspace {
    /// Opens every workspace of the index (without one: `default`, as the version without
    /// workspaces saved it), each canvas as it was saved, and shows the active one. A first run
    /// gets one terminal, centered.
    pub(super) fn load_spaces(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use session::Loaded;
        match self.data_dir.as_deref().map(board::load) {
            Some(Loaded::Saved(boards)) => {
                // Only boards read cleanly say what is held: otherwise nothing is deleted.
                if let Some(data) = &self.data_dir
                    && let Err(err) = attachments::sweep(data, &boards)
                {
                    eprintln!("templum: couldn't delete the files no card holds: {err}");
                }
                self.boards_saved = Some(boards.clone());
                self.boards = boards;
            }
            Some(Loaded::Moved { to, why }) => {
                // Their files go with them: the session starts empty, and a start after it
                // would sweep every file the set-aside boards list.
                let files = match self.data_dir.as_deref().map(attachments::set_aside) {
                    Some(Ok(true)) => ", their files as attachments.bak".to_string(),
                    Some(Err(err)) => format!(", and their files couldn't be set aside: {err}"),
                    Some(Ok(false)) | None => String::new(),
                };
                let message = format!(
                    "Couldn't read the boards ({why}); they were kept as {}{files}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
            Some(Loaded::Unreadable(why)) => {
                let message =
                    format!("Couldn't read the boards ({why}); they won't be saved this session");
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.boards_frozen = true;
            }
            Some(Loaded::Nothing) | None => {}
        }
        match self.data_dir.as_deref().map(checklists::load) {
            Some(Loaded::Saved(checklists)) => {
                self.checklists_saved = Some(checklists.clone());
                self.checklists = checklists;
            }
            Some(Loaded::Moved { to, why }) => {
                let message = format!(
                    "Couldn't read the checklists ({why}); they were kept as {}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
            Some(Loaded::Unreadable(why)) => {
                let message = format!(
                    "Couldn't read the checklists ({why}); they won't be saved this session"
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.checklists_frozen = true;
            }
            Some(Loaded::Nothing) | None => {}
        }
        match self.data_dir.as_deref().map(instructions::load) {
            Some(Loaded::Saved(standing)) => {
                self.standing_saved = Some(standing.clone());
                self.standing = standing;
            }
            Some(Loaded::Moved { to, why }) => {
                let message = format!(
                    "Couldn't read the instructions ({why}); they were kept as {}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
            Some(Loaded::Unreadable(why)) => {
                let message = format!(
                    "Couldn't read the instructions ({why}); they won't be saved this session"
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.standing_frozen = true;
            }
            Some(Loaded::Nothing) | None => {}
        }
        match self.data_dir.as_deref().map(automations::load) {
            Some(Loaded::Saved(saved)) => {
                self.automations_saved = Some(saved.clone());
                self.automations = saved;
                self.start_automations(SystemTime::now(), cx);
            }
            Some(Loaded::Moved { to, why }) => {
                let message = format!(
                    "Couldn't read the automations ({why}); they were kept as {}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
            Some(Loaded::Unreadable(why)) => {
                let message = format!(
                    "Couldn't read the automations ({why}); they won't be saved this session"
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.automations_frozen = true;
            }
            Some(Loaded::Nothing) | None => {}
        }
        match self.data_dir.as_deref().map(notes::load) {
            Some(Loaded::Saved(mut notes)) => {
                // Before the copy kept to tell a change: a note's rev starts from the clock, so a
                // restarted Templum never gives out one it gave before.
                notes.stamp(notes::rev_seed());
                self.notes_saved = Some(notes.clone());
                self.notes = notes;
            }
            Some(Loaded::Moved { to, why }) => {
                let message = format!(
                    "Couldn't read the notes ({why}); they were kept as {}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
            Some(Loaded::Unreadable(why)) => {
                let message = format!(
                    "Couldn't read the notes ({why}); neither they nor the canvas will be saved this session"
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.notes_frozen = true;
            }
            Some(Loaded::Nothing) | None => {}
        }
        // A file that was there but couldn't be read: not a new install, so no welcome.
        let mut damaged = false;
        let (index, no_index) = match self.data_dir.as_deref().map(session::load_index) {
            Some(Loaded::Saved(index)) => (index, false),
            Some(Loaded::Moved { to, why }) => {
                let message = format!(
                    "Couldn't read the workspace list ({why}); it was kept as {}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
                damaged = true;
                (session::Index::first_run(), true)
            }
            Some(Loaded::Unreadable(why)) => {
                let message = format!(
                    "Couldn't read the workspace list ({why}); it won't be saved this session"
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.index_frozen = true;
                damaged = true;
                (session::Index::first_run(), true)
            }
            Some(Loaded::Nothing) | None => (session::Index::first_run(), true),
        };
        let mut spaces = Vec::new();
        let mut first_saved = false;
        for (i, entry) in index.workspaces.iter().enumerate() {
            self.space = Space {
                remote: entry.remote.clone().map(|r| Box::new(RemoteSpace::new(r))),
                id: entry.id.clone(),
                name: entry.name.clone(),
                look: Look {
                    icon: entry.icon.as_deref().and_then(space_dialog::icon),
                    color: entry.color.clone(),
                    image: self
                        .data_dir
                        .as_deref()
                        .zip(entry.image.as_ref())
                        .map(|(d, name)| session::icons_dir(d).join(name)),
                },
                folder: entry.folder.clone(),
                opened: entry.opened,
                ..Space::default()
            };
            // A remote workspace has no canvas of its own: home's blocks come each time.
            match self
                .data_dir
                .as_deref()
                .filter(|_| entry.remote.is_none())
                .map(|d| session::load(d, &entry.id))
            {
                // A stopped one keeps its canvas to start when it is opened.
                Some(Loaded::Saved(saved)) if entry.stopped => self.space.saved = Some(saved),
                Some(Loaded::Saved(saved)) => {
                    first_saved |= i == 0;
                    self.restore(saved, window, cx);
                }
                Some(Loaded::Moved { to, why }) => {
                    let message = format!(
                        "Couldn't read the saved canvas ({why}); it was kept as {}",
                        to.display()
                    );
                    eprintln!("templum: {message}");
                    show_notification(window, cx, Notification::warning(message));
                    damaged |= i == 0;
                }
                Some(Loaded::Unreadable(why)) => {
                    let message = format!(
                        "Couldn't read the saved canvas ({why}); it won't be saved this session"
                    );
                    eprintln!("templum: {message}");
                    show_notification(window, cx, Notification::error(message));
                    self.space.frozen = true;
                    damaged |= i == 0;
                }
                Some(Loaded::Nothing) | None => {}
            }
            self.space.stopped = entry.stopped && entry.remote.is_none();
            if let Some(c) = entry.remote.as_ref().and_then(|r| r.camera)
                && c.x.abs() <= super::remote::MOST_FAR
                && c.y.abs() <= super::remote::MOST_FAR
            {
                self.space.canvas.camera = Camera::at(Point::new(c.x, c.y), c.zoom);
            }
            // Its file may hold notes' text only it keeps until notes.json can be written.
            self.space.frozen |= self.notes_frozen;
            spaces.push(std::mem::take(&mut self.space));
        }
        self.sidebar = index.sidebar;
        self.git_size = index.git_size.map(|s| Size::new(s.w, s.h));
        self.description_h = index.description_h;
        self.active = index.active.min(spaces.len() - 1);
        self.space = spaces.remove(self.active);
        self.others = spaces;
        self.space.opened = Some(crate::notes::now());
        if no_index && !first_saved {
            // A first run: the welcome, whose closing opens the first terminal in the shell
            // picked there. Without a data folder (`--bench`) nothing is kept: no welcome.
            if self.data_dir.is_some() && !damaged {
                self.open_welcome(true, window, cx);
            } else {
                self.open_first_terminal(window, cx);
            }
        } else if self.data_dir.is_some() {
            // Every other start opens on the Home, the workspace shown last loaded behind it.
            self.open_home(window, cx);
        }
    }

    /// A first run's terminal: one, centered.
    pub(crate) fn open_first_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.space.pristine = true;
        let vp = self.viewport(window);
        let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
        let rect = Rect::new((vp.w - size.w) / 2.0, (vp.h - size.h) / 2.0, size.w, size.h);
        // Templum's, not a change of the person's to take back.
        self.quiet = true;
        self.open(rect, window, cx);
        self.quiet = false;
    }

    /// Shows workspace `to` (a position in the sidebar). What only makes sense on screen ends
    /// with the one left: its fading blocks, its animations, a drag.
    pub(crate) fn switch_to(&mut self, to: usize, window: &mut Window, cx: &mut Context<Self>) {
        // The Home goes, the workspace shown asked for too: the person wants the canvas.
        self.close_screen(window, cx);
        if to == self.active || to > self.others.len() {
            return;
        }
        // A menu for something on the canvas left behind. A welcome makes way: a first run's
        // terminal opens in the workspace it was for.
        self.menu = None;
        self.close_welcome(window, cx);
        let left = &mut self.space;
        // Their shells close now.
        left.ghosts.clear();
        left.appearing.clear();
        left.hovered = None;
        left.guides.clear();
        left.flight = None;
        left.view = left.canvas.camera;
        left.selected_ink = None;
        left.selected_wire = None;
        self.drag = None;
        self.press = None;
        self.marked = None;
        let current = std::mem::take(&mut self.space);
        self.others.insert(self.active, current);
        self.space = self.others.remove(to);
        self.active = to;
        self.space.opened = Some(crate::notes::now());
        // A mirrored workspace has no drawing tool, and the Wire tool only while home can wire:
        // the one picked on the workspace left is let go.
        let wires = self.tool == Tool::Wire && self.why_not(Deed::Wire).is_none();
        if self.on_remote() && !matches!(self.tool, Tool::Select | Tool::Hand) && !wires {
            self.tool = Tool::Select;
            self.shapes_open = false;
        }
        // Its requests are another workspace's: the first is selected.
        self.inbox_at = None;
        if self.space.stopped {
            self.start_space(window, cx);
        }
        self.space.view = self.space.canvas.camera;
        window.focus(&self.focus_handle, cx);
        self.write_index(window, cx);
        cx.notify();
    }

    /// The next (`step` 1) or previous (-1) workspace, wrapping around.
    pub(super) fn cycle_space(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.others.len() as i32 + 1;
        let to = (self.active as i32 + step).rem_euclid(n) as usize;
        self.switch_to(to, window, cx);
    }

    /// Creates an empty workspace after the last one and shows it.
    pub(crate) fn new_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.make_space();
        self.switch_to(self.others.len(), window, cx);
    }

    /// Open folder… and a recent folder: the workspace whose folder it is is shown; else a new
    /// workspace named after the folder, with that folder, is made and shown.
    pub(crate) fn open_folder_space(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = (self.order().iter())
            .position(|s| !s.is_remote() && s.folder.as_deref() == Some(folder));
        if let Some(at) = at {
            return self.switch_to(at, window, cx);
        }
        if !folder.is_dir() {
            return self.warn(
                format!("{} isn't a folder anymore.", folder.display()),
                window,
                cx,
            );
        }
        let id = self.make_space();
        let name = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        if let Some(space) = self.space_by_id_mut(&id) {
            space.name = name;
            space.folder = Some(folder.to_path_buf());
        }
        self.switch_to(self.others.len(), window, cx);
    }

    /// Adds an empty workspace after the last one, hidden: its id. The index is the caller's
    /// to write.
    pub(super) fn make_space(&mut self) -> String {
        let dir = self.data_dir.clone();
        let id = session::next_id(|id| {
            self.spaces().any(|s| s.id == id)
                || dir.as_deref().is_some_and(|d| session::id_in_folder(d, id))
        });
        let total = self.others.len() + 1;
        let name = (total + 1..)
            .map(|n| format!("Workspace {n}"))
            .find(|name| !self.spaces().any(|s| s.name == *name))
            .expect("an unused name");
        self.others.push(Space {
            id: id.clone(),
            name,
            ..Space::default()
        });
        id
    }

    /// Moves blocks `ids` to the workspace `to` (None: a new one), running: a terminal's
    /// shell goes on there. They land in its view, clear of its blocks, as they were arranged;
    /// a notification shows them there. A stopped workspace takes none.
    pub(crate) fn move_blocks(
        &mut self,
        ids: Vec<BlockId>,
        to: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.why_not(Deed::MoveToWorkspace).is_some() {
            return;
        }
        let to = match to {
            Some(to) if self.others.iter().any(|s| s.id == to && s.takes_blocks()) => {
                to.to_string()
            }
            Some(_) => return,
            None => self.make_space(),
        };
        // Back to front, as they were.
        let order: Vec<BlockId> = self
            .space
            .canvas
            .blocks()
            .iter()
            .map(|b| b.id)
            .filter(|id| ids.contains(id))
            .collect();
        // Each keeps its id there.
        let taken: Vec<(Rect, Body, Option<String>)> = order
            .into_iter()
            .filter_map(|id| {
                let uid = self.space.uids.get(&id).cloned();
                let (_, home, body) = self.take_block(id, window, cx)?;
                Some((home, body, uid))
            })
            .collect();
        let Some(bounds) = taken.iter().map(|(r, _, _)| *r).reduce(|a, b| a.union(&b)) else {
            return;
        };
        let moved = self.with_space(&to, |ws| {
            let (dx, dy) = ws.landing(bounds, window);
            // Not a change to take back there.
            ws.quiet = true;
            let mut moved = Vec::new();
            for (r, mut body, uid) in taken {
                // Its find field searches the block by the id it had.
                if let Body::Terminal(pane) = &mut body {
                    pane.find = None;
                }
                let id = ws.add_block(Rect::new(r.x + dx, r.y + dy, r.w, r.h), body);
                if let Some(uid) = uid {
                    ws.space.uids.insert(id, uid);
                }
                moved.push(id);
            }
            ws.quiet = false;
            ws.space.pristine = false;
            (ws.space.name.clone(), moved)
        });
        // The selected request's block may have moved away with the others.
        if let Some(at) = self.inbox_at
            && !self.inbox().iter().any(|w| w.key == at)
        {
            self.inbox_settle(&[at]);
        }
        self.write_index(window, cx);
        if let Some((name, moved)) = moved
            && let Some(&first) = moved.first()
        {
            let ws = cx.entity().downgrade();
            let space = to.clone();
            let notification = Notification::info(match moved.len() {
                1 => format!("Moved to {name}"),
                n => format!("Moved {n} blocks to {name}"),
            })
            .on_click(move |_, window, cx| {
                ws.update(cx, |ws, cx| ws.reveal(&space, first, window, cx))
                    .ok();
            });
            show_notification(window, cx, notification);
        }
        cx.notify();
    }

    /// Runs `f` with the hidden workspace `id` as the one shown, then puts them back, as
    /// `load_spaces` does: what `f` reads of the order (`active`, `order`) is off meanwhile.
    fn with_space<R>(&mut self, id: &str, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        let i = self.others.iter().position(|s| s.id == id)?;
        std::mem::swap(&mut self.space, &mut self.others[i]);
        let result = f(self);
        std::mem::swap(&mut self.space, &mut self.others[i]);
        Some(result)
    }

    /// Runs `f` with the workspace `id` as the one shown: at once when it is, else as `with_space`
    /// does. None when there is no such workspace.
    pub(super) fn in_space<R>(&mut self, id: &str, f: impl FnOnce(&mut Self) -> R) -> Option<R> {
        if self.space.id == id {
            Some(f(self))
        } else {
            self.with_space(id, f)
        }
    }

    /// A new workspace, shown, with its Edit… dialog open to name it.
    pub(crate) fn add_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_space(window, cx);
        let id = self.space.id.clone();
        space_dialog::open(self, &id, window, cx);
    }

    /// Gives the workspace with `id` a name (an empty one is refused), a look and a folder. A
    /// new image is copied into the data folder, and one taken away removed from it.
    pub(crate) fn edit_space(
        &mut self,
        id: &str,
        name: &str,
        mut look: Look,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A workspace needs a name".into());
        }
        let Some(old) = self
            .spaces()
            .find(|s| s.id == id)
            .map(|s| s.look.image.clone())
        else {
            return Ok(());
        };
        if let Some(dir) = self.data_dir.clone()
            && look.image != old
        {
            match &look.image {
                Some(file) => {
                    let kept = session::copy_icon(&dir, id, file).map_err(|e| e.to_string())?;
                    let kept = session::icons_dir(&dir).join(kept);
                    // The same file name may still show the image it replaced.
                    ImageSource::from(kept.clone()).remove_asset(cx);
                    look.image = Some(kept);
                }
                None => {
                    if let Some(gone) = old.as_ref().and_then(|p| p.file_name()) {
                        session::remove_icon(&dir, &gone.to_string_lossy())
                            .map_err(|e| format!("Couldn't remove the image: {e}"))?;
                    }
                }
            }
        }
        if let Some(space) = self.space_by_id_mut(id) {
            space.name = name.to_string();
            space.look = look;
            space.folder = folder;
        }
        self.write_index(window, cx);
        cx.notify();
        Ok(())
    }

    /// Deletes the workspace with `id`: its processes end (see `end_later`), its file is kept
    /// as `.bak` and its image removed. The last one stays, unless it is a remote: a new workspace
    /// takes its place.
    pub(crate) fn delete_space(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.delete_space_as(id, false, window, cx);
    }

    /// `delete_space`; a remote one also revokes its share at home first when `revoke`.
    pub(super) fn delete_space_as(
        &mut self,
        id: &str,
        revoke: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(at) = self.order().iter().position(|s| s.id == id) else {
            return;
        };
        if self.others.is_empty() {
            if !self.space.is_remote() {
                return;
            }
            self.make_space();
        }
        if at == self.active {
            self.switch_to(if at > 0 { at - 1 } else { 1 }, window, cx);
        }
        let space = self
            .others
            .remove(if at < self.active { at } else { at - 1 });
        if at < self.active {
            self.active -= 1;
        }
        // Its key goes with it; it never had a file.
        if space.is_remote()
            && let Some(mirror) = &self.mirror
        {
            // The keys that new codes replaced and the client never heard `Kept` for go too.
            for (entry, file) in space.remote.iter().flat_map(|r| &r.old_keys) {
                mirror.drop_key(entry, *file);
            }
            mirror.forget(&space.id, revoke);
        }
        if let Some(dir) = &self.data_dir {
            if !space.is_remote()
                && let Err(err) = session::set_aside(dir, &space.id)
            {
                eprintln!("templum: couldn't set {}.json aside: {err}", space.id);
            }
            if let Some(image) = space.look.image.as_ref().and_then(|p| p.file_name())
                && let Err(err) = session::remove_icon(dir, &image.to_string_lossy())
            {
                eprintln!("templum: couldn't remove {}'s image: {err}", space.id);
            }
        }
        let tree = tree_of(&space);
        let deleted = space.id.clone();
        // The drafts and the selection of a remote's rules go with it: another remote that takes
        // its id inherits none.
        if space.is_remote() {
            self.forget_home_rules(&deleted);
        }
        // Dropping its terminals hangs their shells up.
        drop(space);
        self.end_later(tree, cx);
        self.api_forget_workspace(&deleted, cx);
        self.notes.forget_workspace(&deleted);
        self.write_index(window, cx);
        cx.notify();
    }

    /// Stops the workspace with `id` (see `stop_now`); a terminal at work or an unsaved file
    /// asks first, naming them.
    pub(crate) fn stop_space(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(space) = self.spaces().find(|s| s.id == id) else {
            return;
        };
        if !self.can_stop(space) {
            return;
        }
        let (name, busy, unsaved) = (space.name.clone(), self.busy_in(space), space.unsaved(None));
        if busy.is_empty() && unsaved.is_empty() {
            self.stop_now(id, window, cx);
            return;
        }
        let ws = cx.entity();
        let id = id.to_string();
        let title = format!("Stop {name}?");
        let then = "Opening it starts its terminals again.";
        if unsaved.is_empty() {
            prompt::confirm(
                title,
                format!("{} {then}", busy.join(" ")),
                "Stop",
                move |window, cx| ws.update(cx, |ws, cx| ws.stop_now(&id, window, cx)),
                window,
                cx,
            );
            return;
        }
        let (saver, save_id) = (ws.clone(), id.clone());
        let mut why = format!("Unsaved: {}.", unsaved.join(", "));
        for b in &busy {
            why = format!("{why} {b}");
        }
        prompt::save_or_discard(
            title,
            why,
            move |window, cx| {
                saver.update(cx, |ws, cx| {
                    let saved = ws.save_spaces(Some(&save_id), window, cx);
                    if saved {
                        ws.stop_now(&save_id, window, cx);
                    }
                    saved
                })
            },
            move |window, cx| ws.update(cx, |ws, cx| ws.stop_now(&id, window, cx)),
            window,
            cx,
        );
    }

    /// A workspace can be stopped unless it is the last one, or stopped already.
    pub(crate) fn can_stop(&self, space: &Space) -> bool {
        !self.others.is_empty() && !space.stopped && !space.is_remote()
    }

    /// Ends every program of the workspace with `id`, as quitting would: its canvas is written
    /// (the agents in it found, to resume), its blocks dropped and its processes ended. The one
    /// shown first gives its place to the one before it. Opening it starts it again.
    pub(crate) fn stop_now(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.order().iter().position(|s| s.id == id) else {
            return;
        };
        if self.others.is_empty() {
            return;
        }
        if at == self.active {
            self.switch_to(if at > 0 { at - 1 } else { 1 }, window, cx);
        }
        let (known, accounts) = (self.known_agents(), self.known_accounts());
        // A snapshot still being written is older: it can't replace this one.
        self.generation += 1;
        let generation = self.generation;
        let dir = self.data_dir.clone();
        let Some(space) = self
            .others
            .iter_mut()
            .find(|s| s.id == id && !s.stopped && !s.is_remote())
        else {
            return;
        };
        let (saved, _) = space.layout(known, accounts, cx).snapshot(dir.as_deref());
        let mut failed = None;
        // Written even when it looks unchanged: a snapshot in flight may have written an older one.
        if let Some(dir) = dir.filter(|_| !space.frozen)
            && let Err(err) = session::save(&dir, &space.id, &saved, generation)
        {
            failed = Some(format!("Couldn't save {}'s canvas: {err}", space.name));
        }
        space.saved = Some(saved);
        let tree = tree_of(space);
        space.empty();
        space.stopped = true;
        if let Some(message) = failed {
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::error(message));
        }
        self.end_later(tree, cx);
        self.write_index(window, cx);
        cx.notify();
    }

    /// Brings the shown workspace, stopped, back as Templum starts one: its canvas, terminals in
    /// their folders, agents resumed.
    fn start_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.space.stopped = false;
        if let Some(saved) = self.space.saved.clone() {
            self.restore(saved, window, cx);
        }
    }

    /// Kills, KILL_AFTER from now, what of `tree` the hangup left running.
    pub(super) fn end_later(&self, tree: Vec<templum_terminal::Running>, cx: &mut Context<Self>) {
        if tree.is_empty() {
            return;
        }
        let executor = cx.background_executor().clone();
        let timer = executor.timer(KILL_AFTER);
        executor
            .spawn(async move {
                timer.await;
                templum_terminal::kill_left(&tree);
            })
            .detach();
    }

    /// Why ending a workspace's terminals would lose work: each one at work, by its title.
    pub(crate) fn busy_in(&self, space: &Space) -> Vec<String> {
        space
            .terminals()
            .filter_map(|(_, pane)| {
                let why = alert::busy(&pane.term.status(), self.agent_name(pane))?;
                Some(format!("{}: {why}", pane.term.title()))
            })
            .collect()
    }

    /// Puts the workspace with `id` at position `to`, the others following; the one shown
    /// stays shown.
    pub(crate) fn move_space(
        &mut self,
        id: &str,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.order().iter().position(|s| s.id == id) else {
            return;
        };
        if from == to || to > self.others.len() {
            return;
        }
        let shown = self.space.id.clone();
        let mut all = std::mem::take(&mut self.others);
        all.insert(self.active, std::mem::take(&mut self.space));
        let moved = all.remove(from);
        all.insert(to, moved);
        self.active = all.iter().position(|s| s.id == shown).unwrap_or(0);
        self.space = all.remove(self.active);
        self.others = all;
        self.write_index(window, cx);
        cx.notify();
    }

    pub(crate) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar = !self.sidebar;
        self.write_index(window, cx);
        cx.notify();
    }

    /// The workspaces as the index lists them.
    pub(super) fn index(&self) -> session::Index {
        // The remote the Connect dialog waits on is kept once it is live, not before.
        let order: Vec<&Space> = self
            .order()
            .into_iter()
            .filter(|s| self.pending_connect.as_deref() != Some(s.id.as_str()))
            .collect();
        session::Index {
            version: session::VERSION,
            workspaces: order
                .iter()
                .map(|s| session::IndexEntry {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    // Home's look on a remote tile is home's to keep, not the person's.
                    icon: s
                        .look
                        .icon
                        .filter(|_| !s.remote.as_ref().is_some_and(|r| r.home_look))
                        .map(|i| space_dialog::icon_name(i).to_string()),
                    color: s
                        .look
                        .color
                        .clone()
                        .filter(|_| !s.remote.as_ref().is_some_and(|r| r.home_look)),
                    image: s
                        .look
                        .image
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned()),
                    folder: s.folder.clone(),
                    stopped: s.stopped,
                    opened: s.opened,
                    remote: s.remote.as_ref().map(|r| session::RemoteEntry {
                        // Before home's first state the camera is nobody's: it isn't kept. Nor
                        // is one that isn't finite or lies beyond MOST_FAR: the entry couldn't be
                        // read back, or would start a dot grid with no end.
                        // Grid mode's is not the canvas's.
                        camera: (r.placed || s.camera() != Camera::default())
                            .then_some(s.camera())
                            .filter(|c| {
                                c.origin.x.abs() <= super::remote::MOST_FAR
                                    && c.origin.y.abs() <= super::remote::MOST_FAR
                            })
                            .map(|c| session::SavedCamera {
                                x: c.origin.x,
                                y: c.origin.y,
                                zoom: c.zoom(),
                            }),
                        ..r.entry.clone()
                    }),
                })
                .collect(),
            active: order
                .iter()
                .position(|s| s.id == self.space.id)
                .unwrap_or(0),
            sidebar: self.sidebar,
            git_size: self.git_size.map(|s| session::SavedSize { w: s.w, h: s.h }),
            description_h: self.description_h,
        }
    }

    /// Writes the index at once: it changes only on a user's action.
    pub(crate) fn write_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self
            .data_dir
            .as_ref()
            .filter(|_| !self.index_frozen && !self.first_run_welcome())
        else {
            return;
        };
        if let Err(err) = session::save_index(dir, &self.index()) {
            let message = format!("Couldn't save the workspace list: {err}");
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::error(message));
        }
    }

    /// Every workspace: the active one, then the hidden ones.
    pub(crate) fn spaces(&self) -> impl Iterator<Item = &Space> {
        std::iter::once(&self.space).chain(&self.others)
    }

    /// The workspaces in the sidebar's order.
    pub(crate) fn order(&self) -> Vec<&Space> {
        let mut order: Vec<&Space> = self.others.iter().collect();
        order.insert(self.active, &self.space);
        order
    }

    pub(super) fn space_by_id_mut(&mut self, id: &str) -> Option<&mut Space> {
        std::iter::once(&mut self.space)
            .chain(&mut self.others)
            .find(|s| s.id == id)
    }
}
