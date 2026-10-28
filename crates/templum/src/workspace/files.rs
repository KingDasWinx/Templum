//! Editor blocks: their folder, their tabs, and saving and reloading their files.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::input::{EditorState, Position};
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window, px};
use templum_canvas::{BlockId, Point, Rect};

use crate::editor::{self, Editor};
use crate::launcher;
use crate::prompt;

use super::remote::Deed;
use super::{Body, Workspace, remember, show_notification};

impl Workspace {
    /// Asks for a folder, then opens an editor on it at the viewport's center.
    pub(crate) fn new_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.why_not(Deed::AddEditor).is_some() {
            return;
        }
        let at = self.room_in_view(editor::SIZE, window, cx).origin();
        self.ask_editor_folder(at, window, cx);
    }

    /// The folder dialog; the editor's top-left corner goes to `at` (world units).
    pub(crate) fn ask_editor_folder(&self, at: Point, window: &mut Window, cx: &mut Context<Self>) {
        if self.why_not(Deed::AddEditor).is_some() {
            return;
        }
        let entity = cx.entity();
        launcher::ask_folder_for(
            self,
            "Open an editor".to_string(),
            Rc::new(move |folder, window, cx| {
                entity.update(cx, |ws, cx| ws.add_editor_at(folder, at, window, cx))
            }),
            window,
            cx,
        );
    }

    /// An editor on `folder`, its top-left corner at `at`; the folder becomes a recent one.
    pub(crate) fn add_editor_at(
        &mut self,
        folder: &Path,
        at: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        if self.why_not(Deed::AddEditor).is_some() {
            return None;
        }
        if !folder.is_dir() {
            let message = format!("{} isn't a folder", folder.display());
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
            return None;
        }
        self.space.pristine = false;
        let rect = Rect::new(at.x, at.y, editor::SIZE.w, editor::SIZE.h);
        let editor = Editor::new(folder.to_path_buf(), true, cx);
        let id = self.add_block(rect, Body::Editor(Box::new(editor)));
        remember(&mut self.space.recent, folder.to_path_buf());
        window.focus(&self.focus_handle, cx);
        cx.notify();
        Some(id)
    }

    /// The folder dialog, to point an editor at another folder: from its menu, or when its
    /// folder is gone.
    pub(crate) fn choose_editor_folder(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity = cx.entity();
        launcher::ask_folder_for(
            self,
            "Change the editor's folder".to_string(),
            Rc::new(move |folder, window, cx| {
                entity.update(cx, |ws, cx| ws.set_editor_folder(id, folder, window, cx))
            }),
            window,
            cx,
        );
    }

    /// Points an editor block at another folder, read afresh. A path that isn't a folder
    /// changes nothing.
    pub(crate) fn set_editor_folder(
        &mut self,
        id: BlockId,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        self.space.editor(id)?;
        if !folder.is_dir() {
            let message = format!("{} isn't a folder", folder.display());
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
            return None;
        }
        let editor = self.space.editor_mut(id)?;
        editor.folder = folder.to_path_buf();
        editor.gone = false;
        editor.expanded.clear();
        // Changed files stay open (Save writes them where they were); the others close.
        editor.tabs.retain(|t| t.dirty);
        editor.active = (!editor.tabs.is_empty()).then_some(0);
        editor.refresh(cx);
        remember(&mut self.space.recent, folder.to_path_buf());
        cx.notify();
        Some(id)
    }

    /// Opens `path` in the editor block's tab (at `line`, 1-based, when given) and gives it
    /// the keyboard. A file that can't be opened says why.
    pub(crate) fn open_file(
        &mut self,
        block: BlockId,
        path: &Path,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.space.editor_mut(block) else {
            return;
        };
        match editor.open_tab(path, window, cx) {
            Ok(i) => {
                if let Some(line) = line {
                    let at = Position {
                        line: line.saturating_sub(1),
                        character: 0,
                    };
                    editor.tabs[i]
                        .editor
                        .update(cx, |e, cx| e.set_cursor_position(at, window, cx));
                }
                self.focus_block(block, window, cx);
            }
            Err(message) => {
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
        }
        cx.notify();
    }

    pub(crate) fn activate_tab(
        &mut self,
        block: BlockId,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.space.editor_mut(block)
            && i < editor.tabs.len()
        {
            editor.active = Some(i);
            self.focus_block(block, window, cx);
        }
    }

    /// Removes a tab; the next one (or the previous, for the last) becomes active.
    pub(super) fn remove_tab(
        &mut self,
        block: BlockId,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.release_focus(block, window, cx);
        let Some(editor) = self.space.editor_mut(block) else {
            return;
        };
        if i >= editor.tabs.len() {
            return;
        }
        editor.tabs.remove(i);
        let n = editor.tabs.len();
        let a = editor.active.unwrap_or(0);
        let next = if a > i { a - 1 } else { a };
        editor.active = (n > 0).then(|| next.min(n - 1));
        self.focus_block(block, window, cx);
    }

    /// Closes a tab. One with unsaved edits asks first: Save, Don't save, Cancel.
    pub(crate) fn close_tab(
        &mut self,
        block: BlockId,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dirty = self
            .space
            .editor(block)
            .and_then(|e| e.tabs.get(i))
            .filter(|t| t.dirty)
            .map(editor::Tab::name);
        let Some(name) = dirty else {
            self.remove_tab(block, i, window, cx);
            return;
        };
        let (ws, space) = (cx.entity(), self.space.id.clone());
        let (saver, shown) = (ws.clone(), space.clone());
        prompt::save_or_discard(
            format!("Save {name}?"),
            "Its changes are lost otherwise.",
            move |window, cx| {
                saver.update(cx, |ws, cx| {
                    if !ws.still_shown(&shown, window, cx) {
                        return false;
                    }
                    let saved = ws.save_tab(block, i, window, cx);
                    if saved {
                        ws.remove_tab(block, i, window, cx);
                    }
                    saved
                })
            },
            move |window, cx| {
                ws.update(cx, |ws, cx| {
                    if ws.still_shown(&space, window, cx) {
                        ws.remove_tab(block, i, window, cx);
                    }
                })
            },
            window,
            cx,
        );
    }

    /// Writes one tab's file; a failure says so and the tab stays changed.
    pub(crate) fn save_tab(
        &mut self,
        block: BlockId,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.space.editor_mut(block).and_then(|e| e.tabs.get_mut(i)) else {
            return true;
        };
        let result = tab.save(cx);
        let name = tab.name();
        cx.notify();
        match result {
            Ok(()) => true,
            Err(err) => {
                let message = format!("Couldn't save {name}: {err}");
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                false
            }
        }
    }

    /// The `save` command: the focused editor's active tab.
    pub(super) fn save_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(block) = self.focused_editor()
            && let Some(i) = self.space.editor(block).and_then(|e| e.active)
        {
            self.save_tab(block, i, window, cx);
        }
    }

    /// Writes every changed tab of an editor block; false when one failed.
    pub(crate) fn save_all(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let dirty: Vec<usize> = self
            .space
            .editor(block)
            .map(|e| {
                e.tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.dirty)
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        // Every one is tried, even after a failure.
        let mut ok = true;
        for i in dirty {
            ok &= self.save_tab(block, i, window, cx);
        }
        ok
    }

    /// Every changed tab of the workspace `only`, or of every one; false when one failed.
    pub(crate) fn save_spaces(
        &mut self,
        only: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut failed = None;
        for space in std::iter::once(&mut self.space)
            .chain(&mut self.others)
            .filter(|s| only.is_none_or(|id| s.id == id))
        {
            for tab in space
                .blocks
                .values_mut()
                .filter_map(|b| b.editor_mut())
                .flat_map(|e| e.tabs.iter_mut())
                .filter(|t| t.dirty)
            {
                if let Err(err) = tab.save(cx) {
                    failed.get_or_insert(format!("Couldn't save {}: {err}", tab.name()));
                }
            }
        }
        cx.notify();
        match failed {
            Some(message) => {
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                false
            }
            None => true,
        }
    }

    /// Every workspace's changed files' names.
    pub(super) fn unsaved_names(&self) -> Vec<String> {
        self.spaces().flat_map(|s| s.unsaved(None)).collect()
    }

    /// Asks before the window or Templum closes with unsaved files: Save all, Don't save or
    /// Cancel; either of the first two quits. True when nothing is unsaved and it may close now.
    pub(crate) fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.may_close_then(|cx: &mut App| cx.quit(), window, cx)
    }

    /// [`Self::may_close`], with `then` instead of quitting (a restart).
    pub(crate) fn may_close_then(
        &mut self,
        then: impl Fn(&mut App) + Clone + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let unsaved = self.unsaved_names();
        if unsaved.is_empty() {
            return true;
        }
        let ws = cx.entity();
        let after_save = then.clone();
        prompt::save_or_discard(
            "Save changes before quitting?",
            format!("Unsaved: {}", unsaved.join(", ")),
            move |window, cx| {
                let saved = ws.update(cx, |ws, cx| ws.save_spaces(None, window, cx));
                if saved {
                    after_save(cx);
                }
                saved
            },
            move |_, cx| then(cx),
            window,
            cx,
        );
        false
    }

    pub(super) fn close_active_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(block) = self.focused_editor()
            && let Some(i) = self.space.editor(block).and_then(|e| e.active)
        {
            self.close_tab(block, i, window, cx);
        }
    }

    pub(crate) fn toggle_wrap(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.space.editor_mut(block) {
            editor.wrap = !editor.wrap;
            let wrap = editor.wrap;
            for tab in &editor.tabs {
                tab.editor
                    .update(cx, |e, cx| e.set_soft_wrap(wrap, window, cx));
            }
            cx.notify();
        }
    }

    /// A Markdown file shows rendered, or its editor again (as a note's preview does).
    pub(crate) fn toggle_file_preview(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.release_focus(block, window, cx);
        if let Some(tab) = self
            .space
            .editor_mut(block)
            .and_then(|e| e.active_tab_mut())
        {
            tab.preview = !tab.preview;
        }
        self.focus_block(block, window, cx);
    }

    /// The user typed in a tab's editor: it has unsaved edits.
    pub(crate) fn on_edit(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let tab = self
            .space
            .blocks
            .values_mut()
            .filter_map(|b| b.editor_mut())
            .flat_map(|e| e.tabs.iter_mut())
            .find(|t| t.editor == *editor);
        if let Some(tab) = tab
            && !tab.dirty
        {
            tab.dirty = true;
            cx.notify();
        }
    }

    /// Every POLL: an open file changed on disk is reloaded (clean) or flagged (edited), a
    /// gone one is marked changed so Save brings it back; and the focused editor's tree is
    /// read again once TREE_REFRESH has passed. Only the workspace on screen: hidden ones catch
    /// up when shown.
    pub(super) fn check_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.space.focused;
        let mut changed = false;
        for (id, body) in &mut self.space.blocks {
            let Body::Editor(editor) = body else {
                continue;
            };
            let gone = !editor.folder.is_dir();
            changed |= gone != editor.gone;
            editor.gone = gone;
            for tab in &mut editor.tabs {
                let now = editor::mtime(&tab.path);
                if now == tab.mtime {
                    continue;
                }
                match (now, tab.dirty) {
                    (None, _) => {
                        tab.dirty = true;
                        tab.mtime = None;
                    }
                    (Some(_), false) => {
                        // Can't be read as it is now (over 32 MB, not a file any more): the
                        // bar says it changed, and Reload says why.
                        if tab.reload(window, cx).is_err() {
                            tab.conflict = true;
                            tab.mtime = now;
                        }
                    }
                    (Some(_), true) => {
                        tab.conflict = true;
                        tab.mtime = now;
                    }
                }
                changed = true;
            }
            if focused == Some(*id) && editor.tree_read.elapsed() >= editor::TREE_REFRESH {
                changed |= editor.refresh(cx);
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// The bar's Reload: the disk's text replaces the edits.
    pub(crate) fn reload_tab(
        &mut self,
        block: BlockId,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.space.editor_mut(block).and_then(|e| e.tabs.get_mut(i)) {
            if let Err(message) = tab.reload(window, cx) {
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
            cx.notify();
        }
    }

    /// The bar's Keep mine: the edits stay, the next Save writes over the disk's version.
    pub(crate) fn keep_mine(&mut self, block: BlockId, i: usize, cx: &mut Context<Self>) {
        if let Some(tab) = self.space.editor_mut(block).and_then(|e| e.tabs.get_mut(i)) {
            tab.conflict = false;
            cx.notify();
        }
    }

    /// Opens `path` (at `line`, 1-based) in an editor of this workspace: one whose folder holds
    /// the file, else any, else a new one on `folder`, to the right of `beside` when given, else
    /// at the viewport's center. The camera flies there if the block is off screen.
    pub(crate) fn open_in_editor(
        &mut self,
        folder: &Path,
        path: &Path,
        line: Option<u32>,
        beside: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let holding = |b: &Body| b.editor().is_some_and(|e| path.starts_with(&e.folder));
        let found = self
            .space
            .blocks
            .iter()
            .find(|(_, b)| holding(b))
            .or_else(|| self.space.blocks.iter().find(|(_, b)| b.editor().is_some()))
            .map(|(id, _)| *id);
        let id = found.or_else(|| {
            let size = editor::SIZE;
            let anchor = beside.and_then(|b| self.space.canvas.get(b).map(|b| self.space.home(b)));
            let at = match anchor {
                Some(anchor) => self.space.room_beside(&[anchor], size).origin(),
                None => self.room_in_view(size, window, cx).origin(),
            };
            self.add_editor_at(folder, at, window, cx)
        });
        let Some(id) = id else {
            return;
        };
        self.open_file(id, path, line, window, cx);
        let space = self.space.id.clone();
        self.reveal(&space, id, window, cx);
    }

    /// The `open_file` command: the focused editor's files, found by typing part of a name;
    /// Enter or a click opens the one chosen.
    pub(super) fn open_file_by_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(block) = self.focused_editor() else {
            return;
        };
        let Some(folder) = self.space.editor(block).map(|e| e.folder.clone()) else {
            return;
        };
        let files = editor::files_in(&folder, editor::FILES_CAP);
        let state = cx.new(|cx| CommandState::new(window, cx));
        let ws = cx.entity();
        let palette = state.clone();
        // What the query picks, kept until it changes: the dialog is drawn on every frame.
        let picked: Rc<RefCell<Option<Picked>>> = Rc::default();
        window.open_dialog(cx, move |dialog, _, cx| {
            let query = palette.read(cx).query(cx);
            let mut cache = picked.borrow_mut();
            if cache.as_ref().is_none_or(|(q, _)| *q != query) {
                let found = editor::pick(&files, &query, editor::PICKED);
                *cache = Some((query, found.into_iter().map(String::from).collect()));
            }
            let shown = cache.as_ref().map_or(Vec::new(), |(_, f)| f.clone());
            drop(cache);
            let items = shown
                .iter()
                .map(|file| CommandItem::new().label(file.clone()));
            let (ws, folder) = (ws.clone(), folder.clone());
            dialog.title("Open a file").w(px(560.0)).child(
                Command::new(&palette)
                    .items(items)
                    // The list above is already the query's: no second filter.
                    .filterable(false)
                    .placeholder("Type part of a file's name")
                    .on_confirm(move |at, window, cx| {
                        let Some(file) = shown.get(at.row).map(|f| folder.join(f)) else {
                            return;
                        };
                        window.close_dialog(cx);
                        ws.update(cx, |ws, cx| ws.open_file(block, &file, None, window, cx));
                    }),
            )
        });
        state.update(cx, |state, cx| state.focus(window, cx));
    }

    /// The `go_to_line` command: a line number, then the cursor goes there.
    pub(super) fn go_to_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(block) = self.focused_editor() else {
            return;
        };
        let Some(path) = self
            .space
            .editor(block)
            .and_then(|e| e.active_tab())
            .filter(|t| matches!(t.content, editor::Content::Text { .. }))
            .map(|t| t.path.clone())
        else {
            return;
        };
        let (ws, space) = (cx.entity(), self.space.id.clone());
        prompt::ask_text(
            "Go to line",
            "Go",
            "",
            move |text, window, cx| {
                let Ok(line) = text.trim().parse::<u32>() else {
                    return false;
                };
                ws.update(cx, |ws, cx| {
                    if ws.still_shown(&space, window, cx) {
                        ws.open_file(block, &path, Some(line), window, cx);
                    }
                });
                true
            },
            window,
            cx,
        );
    }

    /// The focused block, when it is an editor.
    pub(super) fn focused_editor(&self) -> Option<BlockId> {
        self.space
            .focused
            .filter(|id| self.space.editor(*id).is_some())
    }
}

/// The file picker's query and the files it picks.
type Picked = (SharedString, Vec<String>);
