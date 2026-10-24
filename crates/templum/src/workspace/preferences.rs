//! config.toml, the themes and the settings panel: reading, applying and saving them.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, KeyDownEvent, Keystroke, Window};
use templum_canvas::chrome;
use templum_git::Repo;
use templum_terminal::Rgb;

use crate::agents;
use crate::config::{self, Config, Edit};
use crate::gallery;
use crate::grid::CellMetrics;
use crate::keys::{self, Command, Keymap, Keys};
use crate::settings::{self, Panel};
use crate::theme::{self, Theme};
use crate::theme_picker::Picker;

use super::{Workspace, missing_themes, show_notification};

/// A command Templum couldn't make, as a notification.
pub(super) fn tell_commands(
    failure: Option<String>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(message) = failure {
        eprintln!("templum: {message}");
        show_notification(window, cx, Notification::warning(message));
    }
}

impl Workspace {
    /// Saves an agent from the settings: `id` names one already known (one of Templum's, or of
    /// the user's), None a new one of the user's. One of Templum's back at its defaults leaves
    /// config.toml. Returns its id.
    pub(crate) fn save_agent(
        &mut self,
        id: Option<&str>,
        draft: &agents::Draft,
        cx: &mut Context<Self>,
    ) -> Result<String, &'static str> {
        if let Some(problem) = draft.problem() {
            return Err(problem);
        }
        let builtin = id.and_then(templum_terminal::Agent::from_name);
        let pending = self.pending.as_ref().unwrap_or(&self.config);
        let id = match id {
            Some(id) => id.to_string(),
            None => agents::new_id(&draft.name, |x| pending.agents.contains_key(x)),
        };
        match draft.to_config(builtin, &agents::codex_models()) {
            Some(set) => {
                let value = agents::to_value(&set);
                let key = id.clone();
                self.edit(
                    Edit::set("agents", id.clone(), value),
                    |c| {
                        c.agents.insert(key, set);
                    },
                    cx,
                );
            }
            None => self.remove_agent(&id, cx),
        }
        Ok(id)
    }

    /// Saves an instruction from Settings ▸ Instructions: `id` names one already there, None a
    /// new one, which goes last. Refuses, with the words a notification shows, what the dialog
    /// can't save: no name, a name over `NAME_MAX` characters, no text, or chosen agents with
    /// none chosen (docs/specs/instructions.md). Returns its id.
    pub(crate) fn save_instruction(
        &mut self,
        id: Option<u64>,
        draft: &crate::instructions::Draft,
        cx: &mut Context<Self>,
    ) -> Result<u64, &'static str> {
        let name = draft.name.trim();
        if name.is_empty() {
            return Err("An instruction needs a name");
        }
        if name.chars().count() > crate::instructions::NAME_MAX {
            return Err("A name is at most 60 characters");
        }
        if crate::instructions::clean_text(&draft.text)
            .trim()
            .is_empty()
        {
            return Err("An instruction needs a text");
        }
        if draft.agents.as_ref().is_some_and(Vec::is_empty) {
            return Err("Choose at least one agent, or Every agent");
        }
        let draft = crate::instructions::Draft {
            name: name.to_string(),
            ..draft.clone()
        };
        let id = match id.filter(|id| self.standing.get(*id).is_some()) {
            Some(id) => {
                self.standing.set(id, draft);
                id
            }
            None => self.standing.add(draft),
        };
        cx.notify();
        Ok(id)
    }

    pub(crate) fn delete_instruction(&mut self, id: u64, cx: &mut Context<Self>) {
        self.standing.remove(id);
        cx.notify();
    }

    pub(crate) fn set_instruction_on(&mut self, id: u64, on: bool, cx: &mut Context<Self>) {
        self.standing.set_on(id, on);
        cx.notify();
    }

    /// Saves an account from Settings ▸ Agents ▸ Accounts (docs/specs/accounts.md): `id` is its
    /// name, the one of an account already there or a new one's, which `agents::name_problem`
    /// checks first. Its folder then gets its choices, a real item in the way of a link kept aside
    /// in Templum's folder and left in one the person chose, and its command is written. A change
    /// of folder waits for its blocks to close and for no other account to share from it.
    /// Returns its id.
    pub(crate) fn save_account(
        &mut self,
        id: &str,
        draft: &agents::AccountDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let pending = self.pending.as_ref().unwrap_or(&self.config);
        let id = id.to_string();
        if !pending.agents.contains_key(&id) {
            let ids = self.taken_ids();
            let taken = |x: &str| ids.iter().any(|t| t.eq_ignore_ascii_case(x));
            let commands = agents::commands_dir();
            let data = self.data_dir.as_deref();
            if let Some(problem) =
                agents::name_problem(&id, taken, data, commands.as_deref(), agents::locate)
            {
                return Err(problem);
            }
        }
        if let Some(problem) = draft.problem(&id, &pending.agents, self.data_dir.as_deref()) {
            return Err(problem);
        }
        let config = draft.to_config();
        let mut all = pending.agents.clone();
        all.insert(id.clone(), config.clone());
        let list = agents::effective(&all, &agents::codex_models(), self.data_dir.as_deref());
        let Some(entry) = list.iter().find(|a| a.id == id).cloned() else {
            return Err("An account needs Templum's data folder".to_string());
        };
        let account = entry.account.as_ref().expect("an account");
        let old = self
            .agents
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| a.account.as_ref());
        let moved = old.is_some_and(|old| old.folder != account.folder);
        if moved && let Some(why) = self.account_in_use(&id) {
            return Err(format!("{why} to change its folder"));
        }
        let key = id.clone();
        self.edit(
            Edit::set("agents", id.clone(), agents::to_value(&config)),
            |c| {
                c.agents.insert(key, config);
            },
            cx,
        );
        let report = entry.apply(&list, templum_usage::accounts::When::Saved);
        self.tell_account(&entry, &report, window, cx);
        let (state, failure) = agents::make_command(&entry, &list, self.data_dir.as_deref());
        self.commands.insert(id.clone(), state);
        tell_commands(failure, window, cx);
        Ok(id)
    }

    /// The ids an account's name must differ from, in any case: those `[agents]` holds, with what
    /// Settings hasn't applied yet, and the agents in effect.
    pub(crate) fn taken_ids(&self) -> Vec<String> {
        let pending = self.pending.as_ref().unwrap_or(&self.config);
        pending
            .agents
            .keys()
            .cloned()
            .chain(self.agents.iter().map(|a| a.id.clone()))
            .collect()
    }

    /// Makes the command of each account in effect, or says why it has none: `commands`. Run where
    /// the agents in effect are computed. Returns the failures to tell, each once while it stays
    /// the same.
    pub(crate) fn sync_commands(&mut self) -> Vec<String> {
        let before = std::mem::take(&mut self.commands);
        let mut failures = Vec::new();
        for entry in self.agents.iter().filter(|a| a.account.is_some()) {
            let (state, failure) =
                agents::make_command(entry, &self.agents, self.data_dir.as_deref());
            if before.get(&entry.id) != Some(&state) {
                failures.extend(failure);
            }
            self.commands.insert(entry.id.clone(), state);
        }
        failures
    }

    /// Whether the account `id` has a command of Templum's own to remove with it.
    pub(crate) fn command_removed(&self, id: &str) -> bool {
        let (Some(dir), Some(data)) = (agents::commands_dir(), self.data_dir.as_deref()) else {
            return false;
        };
        templum_usage::accounts::command::is_own(&dir.join(id), id, data)
    }

    /// Why the account `id` can't be deleted or moved now, if it can't: a block of a running
    /// workspace is its (its agent running there or only its shell), or another account shares
    /// one of its items.
    pub(crate) fn account_in_use(&self, id: &str) -> Option<String> {
        let entry = self.agents.iter().find(|a| a.id == id)?;
        let blocks = self
            .spaces()
            .filter(|s| !s.stopped)
            .flat_map(|s| s.terminals())
            .filter(|(_, p)| p.account.as_deref() == Some(id))
            .count();
        if blocks > 0 {
            let s = if blocks == 1 { "" } else { "s" };
            return Some(format!(
                "{} has {blocks} block{s}: close them first",
                entry.title
            ));
        }
        self.agents.iter().filter(|a| a.id != id).find_map(|other| {
            let (key, _) = other
                .account
                .as_ref()?
                .share
                .iter()
                .find(|(_, to)| *to == id)?;
            Some(format!(
                "{} shares {}'s {key}: change it first",
                other.title, entry.title
            ))
        })
    }

    /// Deletes the account `id` (docs/specs/accounts.md, Deleting an account), unless
    /// `account_in_use` says why not: its entry, and the folder Templum made for it, its links
    /// first (what they point at stays). A folder the person chose stays.
    pub(crate) fn delete_account(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(why) = self.account_in_use(id) {
            return Err(why);
        }
        let Some(entry) = self.agents.iter().find(|a| a.id == id).cloned() else {
            return Err(format!("Templum has no account {id}"));
        };
        let Some(account) = entry.account else {
            return Err(format!("{} isn't an account", entry.title));
        };
        // Never another account's folder (a config written before Templum refused it there),
        // compared as `check_account` does.
        let folder = crate::config::resolved(&account.folder);
        let inside = self.agents.iter().find(|a| {
            a.id != id
                && a.account.as_ref().is_some_and(|other| {
                    crate::config::resolved(&other.folder).starts_with(&folder)
                })
        });
        if !account.chosen
            && let Some(other) = inside
        {
            return Err(format!(
                "{}'s folder is in {}'s: change it first",
                other.title, entry.title
            ));
        }
        if let (Some(dir), Some(data)) = (agents::commands_dir(), self.data_dir.as_deref()) {
            templum_usage::accounts::command::remove(&dir, id, data)
                .map_err(|e| format!("Couldn't remove the command {id}: {e}"))?;
        }
        self.commands.remove(id);
        if !account.chosen {
            match templum_usage::accounts::delete(account.kind, &account.folder) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(format!("Couldn't delete {}: {e}", account.folder.display()));
                }
            }
        }
        self.remove_agent(id, cx);
        Ok(())
    }

    /// One of Templum's agents back to its defaults, or one of the user's deleted.
    pub(crate) fn remove_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        let key = id.to_string();
        self.edit(
            Edit::remove("agents", id.to_string()),
            |c| {
                c.agents.remove(&key);
            },
            cx,
        );
    }

    /// Draws with `theme` from the next frame on, in every terminal and gpui-kit widget.
    pub(crate) fn apply(&mut self, theme: Rc<Theme>, cx: &mut Context<Self>) {
        for pane in self.spaces().flat_map(|s| s.terminals().map(|(_, p)| p)) {
            pane.term.set_palette(theme.palette);
        }
        theme::apply_to_kit(&theme, cx);
        self.theme = theme;
        cx.notify();
    }

    /// The system switched between light and dark.
    pub(crate) fn set_dark(&mut self, system_dark: bool, cx: &mut Context<Self>) {
        // GPUI also reports appearance on Wayland decoration and window-control changes.
        if system_dark == self.system_dark {
            return;
        }
        self.system_dark = system_dark;
        // A preview from the other mode must not survive (Esc would restore the wrong theme).
        self.picker = None;
        self.refresh_theme(cx);
    }

    /// Picks the theme for the mode in effect (the system's, unless config.toml pins one).
    pub(crate) fn refresh_theme(&mut self, cx: &mut Context<Self>) {
        self.dark = self.config.theme.mode.dark(self.system_dark);
        let theme = theme::pick(
            &self.themes,
            self.config.theme.name(self.dark),
            theme::default_name(self.dark),
        );
        self.apply(theme, cx);
    }

    /// Every config::POLL: reloads config.toml when it changed (or appeared, or went away).
    pub(super) fn poll_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stamp = config::Stamp::of(self.config_dir.as_deref());
        if stamp != self.config_stamp {
            // The panel's unsaved changes go in first: saving re-reads the file, so the reload
            // brings back both them and what changed by hand. If the file still doesn't parse,
            // the reload says so.
            if !self.unsaved.is_empty() {
                self.save_config();
            }
            self.config_stamp = config::Stamp::of(self.config_dir.as_deref());
            self.reload_config(window, cx);
        }
        self.poll_theme(window, cx);
    }

    /// Reads the theme in use again when its file changed (saved from an editor block, or any
    /// editor), and applies it. A file that doesn't read keeps the last good colors and says so
    /// once: the stamp is kept, so only the next save is read.
    fn poll_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.theme_path() else {
            self.theme_file = None;
            return;
        };
        let mut stamp = config::Stamp::file(&path);
        if self
            .theme_file
            .as_ref()
            .is_some_and(|(p, s)| *p == path && *s == stamp)
        {
            return;
        }
        // The color editor's changes go in first: the write merges them with the file's, and the
        // read below shows both.
        if self.theme_edits.as_ref().is_some_and(|(p, _)| *p == path) {
            if let Some(message) = self.save_theme_edits() {
                show_notification(window, cx, Notification::error(message));
            }
            stamp = config::Stamp::file(&path);
        }
        self.theme_file = Some((path.clone(), stamp));
        match theme::read(&path) {
            Ok(read) => {
                let read = Rc::new(read);
                if let Some(entry) = self
                    .themes
                    .iter_mut()
                    .find(|e| e.path.as_ref() == Some(&path))
                {
                    entry.theme = read.clone();
                }
                // Esc in the picker must put back this, not the copy from before the save.
                if let Some(picker) = &mut self.picker
                    && picker.before.name == read.name
                {
                    picker.before = read.clone();
                }
                if read != self.theme {
                    self.apply(read, cx);
                }
            }
            Err(err) => {
                let message = format!(
                    "Theme \"{}\": {err}. Keeping the last good one",
                    self.theme.name
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
        }
    }

    /// Reads config.toml again and applies what changed. An invalid file keeps the current
    /// config and says why.
    pub(crate) fn reload_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match config::load(self.config_dir.as_deref(), self.data_dir.as_deref()) {
            Ok(new) => {
                if std::mem::take(&mut self.config_failed) {
                    show_notification(
                        window,
                        cx,
                        Notification::info(format!("{} loaded", config::FILE)),
                    );
                }
                // A file that loads is a file a save can write again.
                self.save_error = None;
                // The panel's half-typed shell fields would hide the file's new values.
                if new.shell != self.config.shell {
                    self.drafts.clear();
                }
                self.apply_config(new, window, cx);
            }
            Err(message) => {
                eprintln!("templum: {message}");
                self.config_failed = true;
                show_notification(window, cx, Notification::error(message));
            }
        }
    }

    pub(super) fn apply_config(
        &mut self,
        new: Config,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let old = std::mem::replace(&mut self.config, new);
        if self.config.font != old.font {
            self.set_font(window, cx);
        }
        if self.config.theme != old.theme {
            self.picker = None;
            // A theme file may have been added since startup.
            self.themes = theme::load(self.config_dir.as_deref());
            for warning in missing_themes(&self.themes, &self.config.theme) {
                eprintln!("templum: {warning}");
                show_notification(window, cx, Notification::warning(warning));
            }
            self.refresh_theme(cx);
        }
        if self.config.cursor != old.cursor || self.config.terminal != old.terminal {
            let options = self.config.term_options();
            for pane in self.spaces().flat_map(|s| s.terminals().map(|(_, p)| p)) {
                pane.term.shared().set_options(options);
            }
        }
        if self.config.agents != old.agents {
            self.agents = agents::effective(
                &self.config.agents,
                &agents::codex_models(),
                self.data_dir.as_deref(),
            );
            self.find_programs();
            let failures = self.sync_commands();
            for failure in failures {
                tell_commands(Some(failure), window, cx);
            }
        }
        let (agent, was) = (&self.config.templum_agent, &old.templum_agent);
        if (&agent.agent, &agent.model, &agent.effort) != (&was.agent, &was.model, &was.effort) {
            self.engine_settings_changed();
        }
        if self.config.agents != old.agents
            || self.config.git.commit_message != old.git.commit_message
        {
            self.commit_agent = agents::commit_agent(
                &self.agents,
                &self.config.git.commit_message,
                templum_terminal::installed,
            );
        }
        // Off: every agent's state is read from its screen again; its hooks are dropped.
        if old.connections.enabled && !self.config.connections.enabled {
            for space in std::iter::once(&mut self.space).chain(&mut self.others) {
                for body in space.blocks.values_mut() {
                    if let Some(pane) = body.terminal_mut() {
                        pane.speaks = false;
                    }
                }
            }
        }
        if self.config.keys != old.keys {
            let (keymap, warnings) = Keymap::new(&self.config.keys, cfg!(target_os = "macos"))
                .expect("config::parse validated the shortcuts");
            self.keymap = keymap;
            for warning in warnings {
                eprintln!("templum: {warning}");
                show_notification(window, cx, Notification::warning(warning));
            }
        }
        cx.notify();
    }

    /// A change from the settings panel: in effect from the next frame, written SAVE_DELAY after
    /// the last one. A change that changes nothing is dropped.
    pub(crate) fn edit(
        &mut self,
        edit: Edit,
        change: impl FnOnce(&mut Config),
        cx: &mut Context<Self>,
    ) {
        // Back to the default: what was half-typed for the key goes too (drafts are keyed by it).
        if edit.value.is_none() && self.drafts.remove(&*edit.key).is_some() {
            cx.notify();
        }
        let current = self.pending.as_ref().unwrap_or(&self.config);
        let mut config = current.clone();
        change(&mut config);
        if config == *current {
            return;
        }
        self.pending = Some(config);
        self.unsaved
            .retain(|e| e.table != edit.table || e.key != edit.key);
        self.unsaved.push(edit);
        cx.notify();
    }

    /// Writes the unsaved changes: the color editor's to the theme's file, the settings' to
    /// config.toml. Returns why one failed.
    pub(super) fn save(&mut self) -> Option<String> {
        let theme = self.save_theme_edits();
        match (self.save_config(), theme) {
            (Some(config), Some(theme)) => Some(format!("{config}\n{theme}")),
            (config, theme) => config.or(theme),
        }
    }

    /// Writes the unsaved settings changes to config.toml. If that fails they stay in effect and
    /// unsaved, for the next try, and the message is returned and kept for the panel's footer.
    fn save_config(&mut self) -> Option<String> {
        let edits = std::mem::take(&mut self.unsaved);
        // No config folder: changes last until Templum quits (the panel's footer says so).
        let dir = self.config_dir.clone()?;
        if edits.is_empty() {
            return None;
        }
        // The write merges in a hand edit the poll hasn't seen yet: then the poll must still reload.
        let hand_edited = config::Stamp::of(Some(&dir)) != self.config_stamp;
        match config::save_edits(&dir, &edits) {
            Ok(()) => {
                // Templum's own write: the poll must not reload it.
                if !hand_edited {
                    self.config_stamp = config::Stamp::of(Some(&dir));
                }
                self.save_error = None;
                None
            }
            Err(err) => {
                let message = format!("Not saved: {err}");
                eprintln!("templum: {message}");
                self.unsaved = edits;
                self.save_error = Some(message.clone());
                Some(message)
            }
        }
    }

    /// Writes the color editor's changes to the theme's file. If that fails they are dropped, and
    /// the message says why: what the file holds wins (it may be broken mid-edit by hand), and the
    /// poll reads it again. After a good write the poll reads it once too, which applies nothing
    /// new unless the list or the screen fell behind (a reload of the folder meanwhile).
    fn save_theme_edits(&mut self) -> Option<String> {
        let (path, edits) = self.theme_edits.take()?;
        let saved = config::save_edits_to(&path, &edits);
        // Read again at the next poll.
        self.theme_file = None;
        saved.err().map(|err| {
            let file = path.file_name().unwrap_or_default().to_string_lossy();
            let message = format!("Not saved: {}", err.describe(&file));
            eprintln!("templum: {message}");
            message
        })
    }

    /// A color of the theme in use, from the color editor (`i` as in `theme::key`; None takes a
    /// `[templum]` color out): shown at once, written to the theme's file SAVE_DELAY after the
    /// last change. A file that doesn't parse is never written: the change isn't shown, and a
    /// notification (one, however many changes) says where the error is.
    pub(crate) fn set_theme_color(
        &mut self,
        i: usize,
        color: Option<Rgb>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.theme_path() else {
            return;
        };
        // Changes to another theme (the mode switched) are written first.
        if self.theme_edits.as_ref().is_some_and(|(p, _)| *p != path) {
            self.save_or_notify(window, cx);
        }
        let (table, key) = theme::key(i);
        let edit = match color {
            Some(c) => Edit::set(table, key, theme::hex(c)),
            None => Edit::remove(table, key),
        };
        // A copy: the pending ones stay if this one can't be shown.
        let mut edits = self
            .theme_edits
            .as_ref()
            .map(|(_, edits)| edits.clone())
            .unwrap_or_default();
        edits.retain(|e| e.table != edit.table || e.key != edit.key);
        edits.push(edit);
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let shown = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| config::apply_edits(&text, &edits).map_err(|e| e.describe(&file)))
            .and_then(|text| theme::parse(&self.theme.name, &text).map_err(|e| e.to_string()));
        let theme = match shown {
            Ok(theme) => Rc::new(theme),
            Err(err) => {
                let message = format!("Not saved: {err}");
                eprintln!("templum: {message}");
                show_notification(
                    window,
                    cx,
                    Notification::error(message).id1::<ThemeColor>(file),
                );
                return;
            }
        };
        self.theme_edits = Some((path.clone(), edits));
        if let Some(entry) = self
            .themes
            .iter_mut()
            .find(|e| e.path.as_ref() == Some(&path))
        {
            entry.theme = theme.clone();
        }
        self.apply(theme, cx);
        self._save = cx.spawn_in(window, async move |ws, cx| {
            cx.background_executor().timer(super::SAVE_DELAY).await;
            ws.update_in(cx, |ws, window, cx| ws.save_or_notify(window, cx))
                .ok();
        });
    }

    /// Applies the settings changes now rather than on the next frame, and writes them: what
    /// opens next (the welcome's first terminal) uses them.
    pub(crate) fn apply_and_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(config) = self.pending.take() {
            self.apply_config(config, window, cx);
        }
        self.save_or_notify(window, cx);
    }

    /// `save`, with a notification when it fails.
    pub(super) fn save_or_notify(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(message) = self.save() {
            show_notification(window, cx, Notification::error(message));
        }
        cx.notify();
    }

    /// New cell metrics: every block keeps its rect, and its terminal gets the columns and rows
    /// that now fit.
    pub(super) fn set_font(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let font = &self.config.font;
        let (cell, warning) = CellMetrics::measure(window, font.family.as_deref(), font.size);
        if let Some(warning) = warning {
            eprintln!("templum: {warning}");
            show_notification(window, cx, Notification::warning(warning));
        }
        self.cell = cell;
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            for (id, body) in space.blocks.iter_mut() {
                if let (Some(pane), Some(block)) = (body.terminal_mut(), space.canvas.get(*id)) {
                    let content = chrome::content_rect(block.rect).size();
                    let (cols, rows) = chrome::grid_size(content, self.cell.size);
                    pane.fit(cols, rows, self.cell.px());
                }
            }
        }
    }

    /// The file of the theme in use; None for a built-in (or a theme previewed from the gallery).
    pub(crate) fn theme_path(&self) -> Option<PathBuf> {
        self.themes
            .iter()
            .find(|e| e.theme.name == self.theme.name)
            .and_then(|e| e.path.clone())
    }

    /// Makes `name` the theme of the mode in effect, as a click in the settings does, and shows it
    /// now: a preview on screen must not stay when the config already names `name`.
    pub(crate) fn set_mode_theme(&mut self, name: &str, cx: &mut Context<Self>) {
        // It may be a file just written.
        self.themes = theme::load(self.config_dir.as_deref());
        let (dark, owned) = (self.dark, name.to_string());
        self.edit(
            Edit::set("theme", if dark { "dark" } else { "light" }, name),
            |c| settings::set_theme(c, dark, owned),
            cx,
        );
        if let Some(theme) = theme::find(&self.themes, name) {
            self.apply(theme, cx);
        }
    }

    /// A new theme file from the theme in use, made the mode's theme. False, with a notification,
    /// when it can't be made (a name taken or not allowed): the name dialog stays open.
    pub(crate) fn new_theme(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let made = match &self.config_dir {
            Some(dir) => theme::create(dir, name, &theme::template(name, &self.theme)),
            None => Err(NO_CONFIG_DIR.to_string()),
        };
        match made {
            Ok(_) => {
                self.set_mode_theme(name.trim(), cx);
                true
            }
            Err(message) => {
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
                false
            }
        }
    }

    /// A theme from the gallery, written to the themes folder and made the mode's theme. One of
    /// that name already there (any case, a built-in too) is chosen instead: nothing is overwritten.
    pub(crate) fn install_theme(
        &mut self,
        card: &gallery::Card,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.themes = theme::load(self.config_dir.as_deref());
        let name = &card.theme.name;
        if let Some(there) = self
            .themes
            .iter()
            .find(|e| e.theme.name.eq_ignore_ascii_case(name))
            .map(|e| e.theme.name.clone())
        {
            self.set_mode_theme(&there, cx);
            return;
        }
        let made = match &self.config_dir {
            Some(dir) => theme::create(dir, name, &gallery::credited(card)),
            None => Err(NO_CONFIG_DIR.to_string()),
        };
        match made {
            Ok(_) => self.set_mode_theme(name, cx),
            Err(message) => {
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
                // The card was previewed: back to the mode's theme.
                self.refresh_theme(cx);
            }
        }
    }

    /// Opens the theme in use's file in an editor block, closing the settings panel (which writes
    /// what's pending). Saving it there applies it: `poll_theme` reads it.
    pub(crate) fn open_theme_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.theme_path() else {
            return;
        };
        let Some(folder) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        self.close_settings(window, cx);
        self.open_in_editor(&folder, &path, None, None, window, cx);
    }

    /// Opens the theme picker (re-reading the user's themes folder), or closes it like Esc.
    pub(crate) fn toggle_theme_picker(&mut self, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            self.cancel_theme(cx);
            return;
        }
        self.themes = theme::load(self.config_dir.as_deref());
        let selected = self
            .themes
            .iter()
            .position(|e| e.theme.name == self.theme.name);
        self.picker = Some(Picker {
            before: self.theme.clone(),
            selected,
            error: None,
        });
        cx.notify();
    }

    pub(crate) fn preview_theme(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(theme) = self.themes.get(i).map(|e| e.theme.clone()) else {
            return;
        };
        if let Some(picker) = &mut self.picker {
            picker.selected = Some(i);
        }
        self.apply(theme, cx);
    }

    /// ↑/↓ in the picker, wrapping around.
    pub(super) fn move_theme(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = self.themes.len() as i32;
        if n == 0 {
            return;
        }
        let at = self
            .picker
            .as_ref()
            .and_then(|p| p.selected)
            .map_or(if delta > 0 { -1 } else { 0 }, |i| i as i32);
        self.preview_theme((at + delta).rem_euclid(n) as usize, cx);
    }

    /// Keeps theme `i` for the current mode and saves it. If saving fails, the choice still
    /// applies and the picker stays open to say why.
    pub(crate) fn choose_theme(&mut self, i: usize, cx: &mut Context<Self>) {
        self.preview_theme(i, cx);
        let name = self.theme.name.clone();
        let key = if self.dark { "dark" } else { "light" };
        for config in [Some(&mut self.config), self.pending.as_mut()]
            .into_iter()
            .flatten()
        {
            if self.dark {
                config.theme.dark = name.clone();
            } else {
                config.theme.light = name.clone();
            }
        }
        let saved = match &self.config_dir {
            Some(dir) => config::save_theme(dir, self.dark, &name),
            None => Err(config::SaveError::NoDir),
        };
        match saved {
            Ok(()) => {
                // A panel change of the same theme, not written yet, would write over this choice.
                self.unsaved.retain(|e| e.table != "theme" || e.key != key);
                self.picker = None;
            }
            Err(err) => {
                eprintln!("templum: couldn't save the theme: {err}");
                if let Some(picker) = &mut self.picker {
                    picker.before = self.theme.clone();
                    picker.error = Some(format!("Not saved: {err}"));
                }
            }
        }
        cx.notify();
    }

    /// Esc or a click outside: back to the theme from before the picker opened.
    pub(crate) fn cancel_theme(&mut self, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker.take() {
            self.apply(picker.before, cx);
        }
    }

    /// What the settings' rows choose from: the themes (the folder read again), the installed
    /// monospaced fonts and the shells found. Read when the panel or the welcome opens.
    pub(crate) fn load_lists(&mut self, window: &mut Window) {
        self.themes = theme::load(self.config_dir.as_deref());
        self.fonts = settings::monospace_families(window);
        self.load_shells();
    }

    /// `load_shells` again when the list was read before the login shell's PATH arrived and it
    /// has arrived since: a shell only that PATH has (Homebrew's fish, from the Dock) is there now.
    pub(crate) fn reload_early_shells(&mut self) {
        if self.shells_early && !templum_terminal::shell_path_pending() {
            self.load_shells();
        }
    }

    /// The shells found and the default's name, read now (`shells_early` says whether the login
    /// shell's PATH was still to come).
    pub(crate) fn load_shells(&mut self) {
        self.shells_early = templum_terminal::shell_path_pending();
        self.shells = templum_terminal::available_shells();
        self.default_shell = templum_terminal::default_shell_label();
    }

    /// Opens the settings panel (re-reading the themes folder for its lists), or closes it.
    pub(crate) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.as_ref().is_some_and(|p| !p.closing) {
            self.close_settings(window, cx);
            return;
        }
        self.load_lists(window);
        self.find_programs();
        self.git_version = None;
        let program = self.config.git.program().to_string();
        cx.spawn_in(window, async move |ws, cx| {
            let version = cx
                .background_executor()
                .spawn(
                    async move { Repo::version(&program).map_err(|e| e.first_line().to_string()) },
                )
                .await;
            ws.update(cx, |ws, cx| {
                ws.git_version = Some(version);
                cx.notify();
            })
            .ok();
        })
        .detach();
        // The panel would open under the screen; it has everything the welcome has.
        self.close_screen(window, cx);
        self.close_welcome(window, cx);
        // Closing the welcome may have started a change, which is the reading then.
        self.load_autostart(cx);
        let now = Instant::now();
        self.settings = Some(match &self.settings {
            // Still sliding out: it turns back from where it is.
            Some(closing) => closing.reversed(now),
            None => Panel::new(now),
        });
        window.focus(&self.settings_focus, cx);
        cx.notify();
    }

    /// Shows another section of the settings panel.
    pub(crate) fn set_section(&mut self, section: settings::Section, cx: &mut Context<Self>) {
        if let Some(panel) = &mut self.settings {
            panel.section = section;
            cx.notify();
        }
    }

    /// Slides the panel out, saves what's pending, and gives the keyboard back to the terminals.
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = &self.settings
            && !panel.closing
        {
            self.settings = Some(panel.reversed(Instant::now()));
        }
        self.drafts.clear();
        self.recording = None;
        self.key_note = None;
        self.save_or_notify(window, cx);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Keys while the panel has the focus (and its fields left them): the theme picker's keys, the
    /// shortcuts that don't act on a terminal, and Esc to close it.
    pub(crate) fn on_settings_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = self.recording {
            // While recording, no shortcut runs and nothing reaches a terminal.
            cx.stop_propagation();
            // A held key repeats: only its first press counts.
            if e.is_held {
                return;
            }
            let k = &e.keystroke;
            let bare = !k.modifiers.modified();
            match k.key.as_str() {
                "escape" if bare => {
                    self.recording = None;
                    self.key_note = None;
                }
                "backspace" if bare => self.set_shortcut(command, Keys::Many(Vec::new()), cx),
                _ if !stands_as_shortcut(k) => {
                    self.key_note = Some(
                        if cfg!(target_os = "macos") {
                            "Add Cmd, Ctrl or Option, or use an F key"
                        } else if cfg!(windows) {
                            "Add Ctrl, Alt or Win, or use an F key"
                        } else {
                            "Add Ctrl, Alt or Super, or use an F key"
                        }
                        .to_string(),
                    );
                }
                _ => match self.keymap.command(k) {
                    Some(owner) if owner != command => {
                        self.key_note = Some(format!(
                            "{} is already {}",
                            keys::display(&k.unparse()),
                            owner.title()
                        ));
                    }
                    _ => self.set_shortcut(command, Keys::One(k.unparse()), cx),
                },
            }
            cx.notify();
            return;
        }
        // A shortcut just recorded may still be held: its repeats mustn't run it.
        if e.is_held {
            return;
        }
        // Copy, paste and close act on the focused terminal, which doesn't have the keyboard now.
        let command = self.keymap.command(&e.keystroke).filter(|c| {
            !matches!(
                c,
                Command::Copy
                    | Command::Paste
                    | Command::CloseTerminal
                    | Command::PreviousPrompt
                    | Command::NextPrompt
                    | Command::Find
            )
        });
        if self.picker.is_some() || command.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
        } else if e.keystroke.key == "escape" {
            self.close_settings(window, cx);
            cx.stop_propagation();
        }
    }

    /// The next key press with a main key becomes `command`'s only shortcut.
    pub(crate) fn record(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = Some(command);
        self.key_note = None;
        window.focus(&self.settings_focus, cx);
        cx.notify();
    }

    /// "Reset to default" of a shortcut. "Reset All" asks for several in one go, so they are
    /// decided together once the event is over.
    pub(crate) fn reset_shortcut(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resetting.push(command);
        if self.resetting.len() == 1 {
            cx.defer_in(window, |ws, window, cx| ws.finish_resets(window, cx));
        }
    }

    /// A reset is refused while a command that keeps its shortcuts has one of the defaults, which
    /// would then be set twice. Commands reset together may trade keys (their defaults differ).
    pub(super) fn finish_resets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let macos = cfg!(target_os = "macos");
        let mut resetting = std::mem::take(&mut self.resetting);
        let keys = &self.pending.as_ref().unwrap_or(&self.config).keys;
        let (keymap, _) = Keymap::new(keys, macos).expect("config::parse validated the shortcuts");
        // Refusing one can make another's default stay taken: repeat until nothing changes.
        while let Some((i, owner, default)) = resetting.iter().enumerate().find_map(|(i, c)| {
            c.defaults(macos).iter().find_map(|default| {
                let typed = Keystroke::parse(default).expect("the default shortcuts parse");
                keymap
                    .command(&typed)
                    .filter(|owner| owner != c && !resetting.contains(owner))
                    .map(|owner| (i, owner, *default))
            })
        }) {
            resetting.remove(i);
            let message = format!(
                "Not reset: {} is {} now",
                keys::display(default),
                owner.title()
            );
            show_notification(window, cx, Notification::warning(message));
        }
        for command in resetting {
            self.edit(
                Edit::remove("keys", command.name()),
                |c| c.keys.set(command, None),
                cx,
            );
        }
    }

    pub(super) fn set_shortcut(&mut self, command: Command, keys: Keys, cx: &mut Context<Self>) {
        self.recording = None;
        self.key_note = None;
        let value: toml_edit::Value = match &keys {
            Keys::One(one) => one.as_str().into(),
            Keys::Many(many) => many
                .iter()
                .map(String::as_str)
                .collect::<toml_edit::Array>()
                .into(),
        };
        self.edit(
            Edit::set("keys", command.name(), value),
            |c| c.keys.set(command, Some(keys)),
            cx,
        );
    }

    /// Opens config.toml in the system's editor, writing the commented template first if needed.
    pub(crate) fn open_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.config_dir.clone() else {
            let message = NO_CONFIG_DIR.to_string();
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::error(message));
            return;
        };
        match config::ensure_file(&dir) {
            Ok(path) => cx.open_with_system(&path),
            Err(err) => {
                let message = format!(
                    "Couldn't create {}: {err}",
                    dir.join(config::FILE).display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
            }
        }
    }
}

/// Tells the color editor's notifications apart, so one stands for many changes.
struct ThemeColor;

const NO_CONFIG_DIR: &str = "No config folder: neither HOME nor APPDATA is set";

/// A shortcut needs Ctrl, Alt or Super (Cmd), or is an F key: any other key is typing, which the
/// terminals must keep getting.
fn stands_as_shortcut(k: &Keystroke) -> bool {
    let m = &k.modifiers;
    let f_key = k
        .key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=35).contains(&n));
    m.control || m.alt || m.platform || m.function || f_key
}
