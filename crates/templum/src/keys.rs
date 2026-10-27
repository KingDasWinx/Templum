//! Keyboard shortcuts: the commands, their defaults per OS, and the keymap built from [keys].

use gpui_kit::{KeybindingKeystroke, Keystroke};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    NewTerminal,
    NewKanban,
    NewNote,
    NewEditor,
    NewGit,
    CloseTerminal,
    CloseTab,
    Save,
    GoToLine,
    Copy,
    Paste,
    PreviousPrompt,
    NextPrompt,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FitAll,
    ToggleGrid,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    SwapLeft,
    SwapRight,
    SwapUp,
    SwapDown,
    ThemePicker,
    OpenConfig,
    Settings,
    Agent,
    NextWorkspace,
    PreviousWorkspace,
    NewWorkspace,
    ConnectTemplum,
    ToggleSidebar,
    Workspaces,
    GoToWaiting,
    AllAgents,
    Automations,
    Find,
    OpenFile,
    Quit,
}

impl Command {
    /// The [keys] order: when two commands share a shortcut, the earlier one wins.
    pub const ALL: [Command; 42] = [
        Command::NewTerminal,
        Command::NewKanban,
        Command::NewNote,
        Command::NewEditor,
        Command::NewGit,
        Command::CloseTerminal,
        Command::CloseTab,
        Command::Save,
        Command::GoToLine,
        Command::Copy,
        Command::Paste,
        Command::PreviousPrompt,
        Command::NextPrompt,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomReset,
        Command::FitAll,
        Command::ToggleGrid,
        Command::FocusLeft,
        Command::FocusRight,
        Command::FocusUp,
        Command::FocusDown,
        Command::SwapLeft,
        Command::SwapRight,
        Command::SwapUp,
        Command::SwapDown,
        Command::ThemePicker,
        Command::OpenConfig,
        Command::Settings,
        Command::Agent,
        Command::NextWorkspace,
        Command::PreviousWorkspace,
        Command::NewWorkspace,
        Command::ConnectTemplum,
        Command::ToggleSidebar,
        Command::Workspaces,
        Command::GoToWaiting,
        Command::AllAgents,
        Command::Automations,
        Command::Find,
        Command::OpenFile,
        Command::Quit,
    ];

    /// Its key in [keys].
    pub fn name(self) -> &'static str {
        match self {
            Command::NewTerminal => "new_terminal",
            Command::NewKanban => "new_kanban",
            Command::NewNote => "new_note",
            Command::NewEditor => "new_editor",
            Command::NewGit => "new_git",
            Command::CloseTerminal => "close_terminal",
            Command::CloseTab => "close_tab",
            Command::Save => "save",
            Command::GoToLine => "go_to_line",
            Command::Copy => "copy",
            Command::Paste => "paste",
            Command::PreviousPrompt => "previous_prompt",
            Command::NextPrompt => "next_prompt",
            Command::ZoomIn => "zoom_in",
            Command::ZoomOut => "zoom_out",
            Command::ZoomReset => "zoom_reset",
            Command::FitAll => "fit_all",
            Command::ToggleGrid => "toggle_grid",
            Command::FocusLeft => "focus_left",
            Command::FocusRight => "focus_right",
            Command::FocusUp => "focus_up",
            Command::FocusDown => "focus_down",
            Command::SwapLeft => "swap_left",
            Command::SwapRight => "swap_right",
            Command::SwapUp => "swap_up",
            Command::SwapDown => "swap_down",
            Command::ThemePicker => "theme_picker",
            Command::OpenConfig => "open_config",
            Command::Settings => "settings",
            Command::Agent => "agent",
            Command::NextWorkspace => "next_workspace",
            Command::PreviousWorkspace => "previous_workspace",
            Command::NewWorkspace => "new_workspace",
            Command::ConnectTemplum => "connect_templum",
            Command::ToggleSidebar => "toggle_sidebar",
            Command::Workspaces => "workspaces",
            Command::GoToWaiting => "go_to_waiting",
            Command::AllAgents => "all_agents",
            Command::Automations => "automations",
            Command::Find => "find",
            Command::OpenFile => "open_file",
            Command::Quit => "quit",
        }
    }

    /// As the settings panel and its messages name it.
    pub fn title(self) -> &'static str {
        match self {
            Command::NewTerminal => "New terminal",
            Command::NewKanban => "New kanban board",
            Command::NewNote => "New note",
            Command::NewEditor => "New editor",
            Command::NewGit => "Git",
            Command::CloseTerminal => "Close terminal",
            Command::CloseTab => "Close the file's tab (in an editor)",
            Command::Save => "Save the file (in an editor)",
            Command::GoToLine => "Go to line (in an editor)",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::PreviousPrompt => "Previous prompt",
            Command::NextPrompt => "Next prompt",
            Command::ZoomIn => "Zoom in",
            Command::ZoomOut => "Zoom out",
            Command::ZoomReset => "Zoom 100%",
            Command::FitAll => "Fit all blocks",
            Command::ToggleGrid => "Fill the workspace with the block, or go back to the canvas",
            Command::FocusLeft => "Focus the tile on the left (grid mode)",
            Command::FocusRight => "Focus the tile on the right (grid mode)",
            Command::FocusUp => "Focus the tile above (grid mode)",
            Command::FocusDown => "Focus the tile below (grid mode)",
            Command::SwapLeft => "Swap with the tile on the left (grid mode)",
            Command::SwapRight => "Swap with the tile on the right (grid mode)",
            Command::SwapUp => "Swap with the tile above (grid mode)",
            Command::SwapDown => "Swap with the tile below (grid mode)",
            Command::ThemePicker => "Theme picker",
            Command::OpenConfig => "Edit config.toml",
            Command::Settings => "Settings",
            Command::Agent => "Templum Agent",
            Command::NextWorkspace => "Next workspace",
            Command::PreviousWorkspace => "Previous workspace",
            Command::NewWorkspace => "New workspace",
            Command::ConnectTemplum => "Connect to a Templum…",
            Command::ToggleSidebar => "Show or hide the sidebar",
            Command::Workspaces => "Home: your workspaces and what waits for you",
            Command::GoToWaiting => "Go to the terminal waiting for you",
            Command::AllAgents => "Show all agents",
            Command::Automations => "Show automations",
            Command::Find => "Find in the terminal",
            Command::OpenFile => "Open a file by name (in an editor)",
            Command::Quit => "Quit Templum",
        }
    }

    /// Runs only while an editor block has the focus; otherwise its key goes to the terminal.
    pub fn editor_only(self) -> bool {
        matches!(
            self,
            Command::CloseTab | Command::Save | Command::GoToLine | Command::OpenFile
        )
    }

    /// Moving between tiles only means something in grid mode: elsewhere its key goes on to
    /// the terminal.
    pub fn grid_only(self) -> bool {
        matches!(
            self,
            Command::FocusLeft
                | Command::FocusRight
                | Command::FocusUp
                | Command::FocusDown
                | Command::SwapLeft
                | Command::SwapRight
                | Command::SwapUp
                | Command::SwapDown
        )
    }

    /// Left to the program that took over a terminal (full screen, the mouse, the kitty keyboard,
    /// an agent): its own hotkeys use these keys (Ctrl+- undoes in Claude Code), and they mean
    /// little there. The window's shortcuts (blocks, workspaces, grid, copy, paste) stay.
    pub fn yields_to_programs(self) -> bool {
        matches!(
            self,
            Command::ZoomIn
                | Command::ZoomOut
                | Command::ZoomReset
                | Command::Settings
                | Command::ThemePicker
                | Command::OpenConfig
                | Command::PreviousPrompt
                | Command::NextPrompt
        )
    }

    /// Shift+1 is reported as "!" on some layouts: fit_all binds both.
    pub fn defaults(self, macos: bool) -> &'static [&'static str] {
        match (self, macos) {
            (Command::NewTerminal, false) => &["ctrl-shift-t"],
            (Command::NewTerminal, true) => &["cmd-t"],
            (Command::NewKanban, false) => &["ctrl-shift-k"],
            (Command::NewKanban, true) => &["cmd-shift-k"],
            (Command::NewNote, false) => &["ctrl-shift-m"],
            (Command::NewNote, true) => &["cmd-shift-m"],
            (Command::NewEditor, false) => &["ctrl-shift-e"],
            (Command::NewEditor, true) => &["cmd-shift-e"],
            (Command::NewGit, false) => &["ctrl-shift-g"],
            (Command::NewGit, true) => &["cmd-shift-g"],
            (Command::CloseTerminal, false) => &["ctrl-shift-w"],
            (Command::CloseTerminal, true) => &["cmd-w"],
            (Command::CloseTab, _) => &["ctrl-w"],
            (Command::Save, false) => &["ctrl-s"],
            (Command::Save, true) => &["cmd-s"],
            (Command::GoToLine, false) => &["ctrl-g"],
            (Command::GoToLine, true) => &["cmd-g"],
            (Command::Copy, false) => &["ctrl-shift-c"],
            (Command::Copy, true) => &["cmd-c"],
            (Command::Paste, false) => &["ctrl-shift-v"],
            (Command::Paste, true) => &["cmd-v"],
            (Command::PreviousPrompt, false) => &["ctrl-shift-up"],
            (Command::PreviousPrompt, true) => &["cmd-up"],
            (Command::NextPrompt, false) => &["ctrl-shift-down"],
            (Command::NextPrompt, true) => &["cmd-down"],
            (Command::ZoomIn, false) => &["ctrl-=", "ctrl-+"],
            (Command::ZoomIn, true) => &["cmd-=", "cmd-+"],
            (Command::ZoomOut, false) => &["ctrl--"],
            (Command::ZoomOut, true) => &["cmd--"],
            (Command::ZoomReset, false) => &["ctrl-0"],
            (Command::ZoomReset, true) => &["cmd-0"],
            (Command::FitAll, false) => &["ctrl-shift-1", "ctrl-!"],
            (Command::FitAll, true) => &["cmd-shift-1", "cmd-!"],
            (Command::ToggleGrid, false) => &["ctrl-shift-enter"],
            (Command::ToggleGrid, true) => &["cmd-shift-enter"],
            (Command::FocusLeft, _) => &["ctrl-alt-left"],
            (Command::FocusRight, _) => &["ctrl-alt-right"],
            (Command::FocusUp, _) => &["ctrl-alt-up"],
            (Command::FocusDown, _) => &["ctrl-alt-down"],
            (Command::SwapLeft, _) => &["ctrl-alt-shift-left"],
            (Command::SwapRight, _) => &["ctrl-alt-shift-right"],
            (Command::SwapUp, _) => &["ctrl-alt-shift-up"],
            (Command::SwapDown, _) => &["ctrl-alt-shift-down"],
            (Command::ThemePicker, false) => &["ctrl-shift-p"],
            (Command::ThemePicker, true) => &["cmd-shift-p"],
            (Command::OpenConfig, _) => &[],
            (Command::Settings, false) => &["ctrl-,"],
            (Command::Settings, true) => &["cmd-,"],
            (Command::Agent, false) => &["ctrl-shift-space"],
            (Command::Agent, true) => &["cmd-shift-space"],
            (Command::NextWorkspace, _) => &["ctrl-tab"],
            (Command::PreviousWorkspace, _) => &["ctrl-shift-tab"],
            (Command::NewWorkspace, false) => &["ctrl-shift-n"],
            (Command::NewWorkspace, true) => &["cmd-shift-n"],
            (Command::ConnectTemplum, _) => &[],
            (Command::ToggleSidebar, _) => &[],
            (Command::Workspaces, false) => &["ctrl-shift-o"],
            (Command::Workspaces, true) => &["cmd-shift-o"],
            (Command::GoToWaiting, false) => &["ctrl-shift-a"],
            (Command::GoToWaiting, true) => &["cmd-shift-a"],
            (Command::AllAgents, false) => &["ctrl-shift-l"],
            (Command::AllAgents, true) => &["cmd-shift-l"],
            (Command::Automations, false) => &["ctrl-shift-j"],
            (Command::Automations, true) => &["cmd-shift-j"],
            (Command::Find, false) => &["ctrl-shift-f"],
            (Command::Find, true) => &["cmd-f"],
            (Command::OpenFile, false) => &["ctrl-p"],
            (Command::OpenFile, true) => &["cmd-p"],
            (Command::Quit, false) => &["ctrl-shift-q"],
            (Command::Quit, true) => &["cmd-q"],
        }
    }
}

/// One shortcut or a list, as written in [keys].
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum Keys {
    One(String),
    Many(Vec<String>),
}

/// [keys]: a command left out keeps its defaults.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct KeysConfig {
    pub new_terminal: Option<Keys>,
    pub new_kanban: Option<Keys>,
    pub new_note: Option<Keys>,
    pub new_editor: Option<Keys>,
    pub new_git: Option<Keys>,
    pub close_terminal: Option<Keys>,
    pub close_tab: Option<Keys>,
    pub save: Option<Keys>,
    pub go_to_line: Option<Keys>,
    pub copy: Option<Keys>,
    pub paste: Option<Keys>,
    pub previous_prompt: Option<Keys>,
    pub next_prompt: Option<Keys>,
    pub zoom_in: Option<Keys>,
    pub zoom_out: Option<Keys>,
    pub zoom_reset: Option<Keys>,
    pub fit_all: Option<Keys>,
    pub toggle_grid: Option<Keys>,
    pub focus_left: Option<Keys>,
    pub focus_right: Option<Keys>,
    pub focus_up: Option<Keys>,
    pub focus_down: Option<Keys>,
    pub swap_left: Option<Keys>,
    pub swap_right: Option<Keys>,
    pub swap_up: Option<Keys>,
    pub swap_down: Option<Keys>,
    pub theme_picker: Option<Keys>,
    pub open_config: Option<Keys>,
    pub settings: Option<Keys>,
    pub agent: Option<Keys>,
    pub next_workspace: Option<Keys>,
    pub previous_workspace: Option<Keys>,
    pub new_workspace: Option<Keys>,
    pub connect_templum: Option<Keys>,
    pub toggle_sidebar: Option<Keys>,
    pub workspaces: Option<Keys>,
    pub go_to_waiting: Option<Keys>,
    pub all_agents: Option<Keys>,
    pub automations: Option<Keys>,
    pub find: Option<Keys>,
    pub open_file: Option<Keys>,
    pub quit: Option<Keys>,
}

impl KeysConfig {
    pub fn get(&self, command: Command) -> Option<&Keys> {
        match command {
            Command::NewTerminal => self.new_terminal.as_ref(),
            Command::NewKanban => self.new_kanban.as_ref(),
            Command::NewNote => self.new_note.as_ref(),
            Command::NewEditor => self.new_editor.as_ref(),
            Command::NewGit => self.new_git.as_ref(),
            Command::CloseTerminal => self.close_terminal.as_ref(),
            Command::CloseTab => self.close_tab.as_ref(),
            Command::Save => self.save.as_ref(),
            Command::GoToLine => self.go_to_line.as_ref(),
            Command::Copy => self.copy.as_ref(),
            Command::Paste => self.paste.as_ref(),
            Command::PreviousPrompt => self.previous_prompt.as_ref(),
            Command::NextPrompt => self.next_prompt.as_ref(),
            Command::ZoomIn => self.zoom_in.as_ref(),
            Command::ZoomOut => self.zoom_out.as_ref(),
            Command::ZoomReset => self.zoom_reset.as_ref(),
            Command::FitAll => self.fit_all.as_ref(),
            Command::ToggleGrid => self.toggle_grid.as_ref(),
            Command::FocusLeft => self.focus_left.as_ref(),
            Command::FocusRight => self.focus_right.as_ref(),
            Command::FocusUp => self.focus_up.as_ref(),
            Command::FocusDown => self.focus_down.as_ref(),
            Command::SwapLeft => self.swap_left.as_ref(),
            Command::SwapRight => self.swap_right.as_ref(),
            Command::SwapUp => self.swap_up.as_ref(),
            Command::SwapDown => self.swap_down.as_ref(),
            Command::ThemePicker => self.theme_picker.as_ref(),
            Command::OpenConfig => self.open_config.as_ref(),
            Command::Settings => self.settings.as_ref(),
            Command::Agent => self.agent.as_ref(),
            Command::NextWorkspace => self.next_workspace.as_ref(),
            Command::PreviousWorkspace => self.previous_workspace.as_ref(),
            Command::NewWorkspace => self.new_workspace.as_ref(),
            Command::ConnectTemplum => self.connect_templum.as_ref(),
            Command::ToggleSidebar => self.toggle_sidebar.as_ref(),
            Command::Workspaces => self.workspaces.as_ref(),
            Command::GoToWaiting => self.go_to_waiting.as_ref(),
            Command::AllAgents => self.all_agents.as_ref(),
            Command::Automations => self.automations.as_ref(),
            Command::Find => self.find.as_ref(),
            Command::OpenFile => self.open_file.as_ref(),
            Command::Quit => self.quit.as_ref(),
        }
    }

    pub fn set(&mut self, command: Command, keys: Option<Keys>) {
        let slot = match command {
            Command::NewTerminal => &mut self.new_terminal,
            Command::NewKanban => &mut self.new_kanban,
            Command::NewNote => &mut self.new_note,
            Command::NewEditor => &mut self.new_editor,
            Command::NewGit => &mut self.new_git,
            Command::CloseTerminal => &mut self.close_terminal,
            Command::CloseTab => &mut self.close_tab,
            Command::Save => &mut self.save,
            Command::GoToLine => &mut self.go_to_line,
            Command::Copy => &mut self.copy,
            Command::Paste => &mut self.paste,
            Command::PreviousPrompt => &mut self.previous_prompt,
            Command::NextPrompt => &mut self.next_prompt,
            Command::ZoomIn => &mut self.zoom_in,
            Command::ZoomOut => &mut self.zoom_out,
            Command::ZoomReset => &mut self.zoom_reset,
            Command::FitAll => &mut self.fit_all,
            Command::ToggleGrid => &mut self.toggle_grid,
            Command::FocusLeft => &mut self.focus_left,
            Command::FocusRight => &mut self.focus_right,
            Command::FocusUp => &mut self.focus_up,
            Command::FocusDown => &mut self.focus_down,
            Command::SwapLeft => &mut self.swap_left,
            Command::SwapRight => &mut self.swap_right,
            Command::SwapUp => &mut self.swap_up,
            Command::SwapDown => &mut self.swap_down,
            Command::ThemePicker => &mut self.theme_picker,
            Command::OpenConfig => &mut self.open_config,
            Command::Settings => &mut self.settings,
            Command::Agent => &mut self.agent,
            Command::NextWorkspace => &mut self.next_workspace,
            Command::PreviousWorkspace => &mut self.previous_workspace,
            Command::NewWorkspace => &mut self.new_workspace,
            Command::ConnectTemplum => &mut self.connect_templum,
            Command::ToggleSidebar => &mut self.toggle_sidebar,
            Command::Workspaces => &mut self.workspaces,
            Command::GoToWaiting => &mut self.go_to_waiting,
            Command::AllAgents => &mut self.all_agents,
            Command::Automations => &mut self.automations,
            Command::Find => &mut self.find,
            Command::OpenFile => &mut self.open_file,
            Command::Quit => &mut self.quit,
        };
        *slot = keys;
    }
}

/// The shortcuts in effect, matched by the workspace's key handler (not GPUI bindings, which
/// can't be removed one by one).
pub(crate) struct Keymap {
    bindings: Vec<Binding>,
}

struct Binding {
    keystroke: Keystroke,
    target: KeybindingKeystroke,
    command: Command,
    /// As written, for messages and the empty-canvas hint.
    text: String,
}

impl Keymap {
    /// Errors: a shortcut that doesn't parse. Warnings: a shortcut set for two commands (the
    /// earlier one keeps it).
    pub fn new(keys: &KeysConfig, macos: bool) -> Result<(Keymap, Vec<String>), String> {
        let mut bindings: Vec<Binding> = Vec::new();
        let mut warnings = Vec::new();
        for command in Command::ALL {
            let texts: Vec<&str> = match keys.get(command) {
                Some(Keys::One(one)) => vec![one.as_str()],
                Some(Keys::Many(many)) => many.iter().map(String::as_str).collect(),
                None => command.defaults(macos).to_vec(),
            };
            for text in texts {
                let keystroke = Keystroke::parse(text).map_err(|_| {
                    format!(
                        "config.toml: keys.{} has an invalid shortcut \"{text}\"",
                        command.name()
                    )
                })?;
                if let Some(taken) = bindings.iter().find(|b| b.keystroke == keystroke) {
                    warnings.push(format!(
                        "\"{text}\" is set for both {} and {}; {} keeps it",
                        taken.command.name(),
                        command.name(),
                        taken.command.name()
                    ));
                    continue;
                }
                bindings.push(Binding {
                    target: KeybindingKeystroke::from_keystroke(keystroke.clone()),
                    keystroke,
                    command,
                    text: text.to_string(),
                });
            }
        }
        Ok((Keymap { bindings }, warnings))
    }

    /// The command a key press triggers.
    pub fn command(&self, typed: &Keystroke) -> Option<Command> {
        self.bindings
            .iter()
            .find(|b| typed.should_match(&b.target))
            .map(|b| b.command)
    }

    /// Every shortcut of `command`, in the order written.
    pub fn keystrokes(&self, command: Command) -> Vec<Keystroke> {
        self.bindings
            .iter()
            .filter(|b| b.command == command)
            .map(|b| b.keystroke.clone())
            .collect()
    }

    /// The first shortcut of `command`, as written.
    pub fn first(&self, command: Command) -> Option<&str> {
        self.bindings
            .iter()
            .find(|b| b.command == command)
            .map(|b| b.text.as_str())
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::new(&KeysConfig::default(), cfg!(target_os = "macos"))
            .expect("the default shortcuts parse")
            .0
    }
}

/// "ctrl-shift-t" → "Ctrl+Shift+T", as menus show shortcuts.
pub(crate) fn display(shortcut: &str) -> String {
    let mut parts: Vec<&str> = shortcut.split('-').collect();
    // The minus key ("ctrl--") splits into two empty parts.
    if parts.ends_with(&["", ""]) {
        parts.truncate(parts.len() - 2);
        parts.push("-");
    }
    parts
        .iter()
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_git_has_its_shortcut_and_key() {
        assert_eq!(Command::NewGit.name(), "new_git");
        assert_eq!(Command::NewGit.defaults(false), ["ctrl-shift-g"]);
        assert_eq!(Command::NewGit.defaults(true), ["cmd-shift-g"]);
        assert!(Command::ALL.contains(&Command::NewGit));
        let mut keys = KeysConfig::default();
        keys.set(Command::NewGit, Some(Keys::One("ctrl-g".into())));
        assert_eq!(keys.get(Command::NewGit), Some(&Keys::One("ctrl-g".into())));
    }

    #[test]
    fn all_agents_is_ctrl_shift_l() {
        assert_eq!(Command::AllAgents.defaults(false), ["ctrl-shift-l"]);
        assert_eq!(Command::AllAgents.defaults(true), ["cmd-shift-l"]);
        assert_eq!(Command::AllAgents.name(), "all_agents");
        assert_eq!(Command::AllAgents.title(), "Show all agents");
        assert_eq!(
            defaults(false).command(&typed("ctrl-shift-l")),
            Some(Command::AllAgents)
        );
        assert_eq!(
            defaults(true).command(&typed("cmd-shift-l")),
            Some(Command::AllAgents)
        );
    }

    #[test]
    fn automations_is_ctrl_shift_j() {
        assert_eq!(Command::Automations.defaults(false), ["ctrl-shift-j"]);
        assert_eq!(Command::Automations.defaults(true), ["cmd-shift-j"]);
        assert_eq!(Command::Automations.name(), "automations");
        assert_eq!(Command::Automations.title(), "Show automations");
        assert_eq!(
            defaults(false).command(&typed("ctrl-shift-j")),
            Some(Command::Automations)
        );
        assert_eq!(
            defaults(true).command(&typed("cmd-shift-j")),
            Some(Command::Automations)
        );
    }

    fn typed(s: &str) -> Keystroke {
        Keystroke::parse(s).unwrap()
    }

    fn defaults(macos: bool) -> Keymap {
        Keymap::new(&KeysConfig::default(), macos).unwrap().0
    }

    #[test]
    fn defaults_match_todays_shortcuts() {
        let linux = defaults(false);
        assert_eq!(
            linux.command(&typed("ctrl-shift-t")),
            Some(Command::NewTerminal)
        );
        assert_eq!(
            linux.command(&typed("ctrl-shift-w")),
            Some(Command::CloseTerminal)
        );
        assert_eq!(linux.command(&typed("ctrl-shift-c")), Some(Command::Copy));
        assert_eq!(linux.command(&typed("ctrl-shift-v")), Some(Command::Paste));
        assert_eq!(linux.command(&typed("ctrl-+")), Some(Command::ZoomIn));
        assert_eq!(linux.command(&typed("ctrl--")), Some(Command::ZoomOut));
        assert_eq!(linux.command(&typed("ctrl-0")), Some(Command::ZoomReset));
        assert_eq!(
            linux.command(&typed("ctrl-shift-p")),
            Some(Command::ThemePicker)
        );
        assert_eq!(linux.command(&typed("ctrl-,")), Some(Command::Settings));
        assert_eq!(linux.command(&typed("ctrl-shift-q")), Some(Command::Quit));
        assert_eq!(linux.command(&typed("ctrl-s")), Some(Command::Save));
        assert_eq!(linux.command(&typed("ctrl-g")), Some(Command::GoToLine));
        assert_eq!(linux.command(&typed("ctrl-t")), None);
        assert_eq!(
            linux.command(&typed("ctrl-shift-e")),
            Some(Command::NewEditor)
        );
        let mac = defaults(true);
        assert_eq!(mac.command(&typed("cmd-shift-e")), Some(Command::NewEditor));
        assert_eq!(mac.command(&typed("cmd-t")), Some(Command::NewTerminal));
        assert_eq!(mac.command(&typed("cmd-,")), Some(Command::Settings));
        assert_eq!(mac.command(&typed("cmd-q")), Some(Command::Quit));
        assert_eq!(mac.command(&typed("cmd-s")), Some(Command::Save));
        assert_eq!(Command::Agent.defaults(false), ["ctrl-shift-space"]);
        assert_eq!(Command::Agent.defaults(true), ["cmd-shift-space"]);
        assert_eq!(
            linux.command(&typed("ctrl-shift-space")),
            Some(Command::Agent)
        );
        assert_eq!(mac.command(&typed("cmd-shift-space")), Some(Command::Agent));
    }

    #[test]
    fn connect_templum_is_a_command_without_a_default_key() {
        let command = Command::ConnectTemplum;
        assert_eq!(command.name(), "connect_templum");
        assert_eq!(command.title(), "Connect to a Templum…");
        assert!(command.defaults(false).is_empty() && command.defaults(true).is_empty());
        assert_eq!(defaults(false).first(command), None);
    }

    #[test]
    fn shifted_digits_match_both_spellings() {
        let keymap = defaults(false);
        // What a US keyboard reports for Ctrl+Shift+1.
        let pressed = Keystroke {
            key_char: Some("!".into()),
            ..typed("ctrl-shift-1")
        };
        assert_eq!(keymap.command(&pressed), Some(Command::FitAll));
        assert_eq!(keymap.command(&typed("ctrl-!")), Some(Command::FitAll));
    }

    #[test]
    fn remapping_replaces_and_empty_disables() {
        let keys = KeysConfig {
            new_terminal: Some(Keys::One("ctrl-alt-n".into())),
            copy: Some(Keys::Many(vec![])),
            ..KeysConfig::default()
        };
        let (keymap, warnings) = Keymap::new(&keys, false).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(
            keymap.command(&typed("ctrl-alt-n")),
            Some(Command::NewTerminal)
        );
        assert_eq!(
            keymap.command(&typed("ctrl-shift-t")),
            None,
            "old shortcut gone"
        );
        assert_eq!(keymap.command(&typed("ctrl-shift-c")), None, "disabled");
        assert_eq!(keymap.first(Command::NewTerminal), Some("ctrl-alt-n"));
        assert_eq!(keymap.first(Command::Copy), None);
    }

    #[test]
    fn an_invalid_shortcut_names_its_action() {
        let keys = KeysConfig {
            new_terminal: Some(Keys::One("ctrl-shft-t".into())),
            ..KeysConfig::default()
        };
        let err = Keymap::new(&keys, false).err().unwrap();
        assert!(
            err.contains("new_terminal") && err.contains("ctrl-shft-t"),
            "{err}"
        );
    }

    #[test]
    fn a_shared_shortcut_warns_and_the_first_action_wins() {
        let keys = KeysConfig {
            copy: Some(Keys::One("ctrl-shift-t".into())),
            ..KeysConfig::default()
        };
        let (keymap, warnings) = Keymap::new(&keys, false).unwrap();
        assert_eq!(
            keymap.command(&typed("ctrl-shift-t")),
            Some(Command::NewTerminal)
        );
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("ctrl-shift-t"), "{}", warnings[0]);
    }

    #[test]
    fn shortcuts_display_like_menus() {
        assert_eq!(display("ctrl-shift-t"), "Ctrl+Shift+T");
        assert_eq!(display("cmd-t"), "Cmd+T");
        assert_eq!(display("ctrl--"), "Ctrl+-", "the minus key");
        assert_eq!(display("-"), "-");
    }

    #[test]
    fn open_config_has_no_default_shortcut() {
        for macos in [false, true] {
            assert_eq!(defaults(macos).first(Command::OpenConfig), None);
        }
    }

    #[test]
    fn a_recorded_shortcut_parses_back() {
        for text in [
            "ctrl-shift-t",
            "cmd-,",
            "alt-f4",
            "ctrl--",
            "ctrl-shift-1",
            "f5",
        ] {
            let pressed = typed(text);
            assert_eq!(typed(&pressed.unparse()), pressed, "{text}");
        }
    }

    #[test]
    fn keys_config_sets_one_command() {
        let mut keys = KeysConfig::default();
        keys.set(Command::Settings, Some(Keys::One("ctrl-alt-s".into())));
        assert_eq!(
            keys.get(Command::Settings),
            Some(&Keys::One("ctrl-alt-s".into()))
        );
        assert_eq!(keys.get(Command::Copy), None);
        keys.set(Command::Settings, None);
        assert_eq!(keys, KeysConfig::default());
    }

    #[test]
    fn keystrokes_lists_every_shortcut_of_a_command() {
        let keymap = defaults(false);
        assert_eq!(
            keymap.keystrokes(Command::ZoomIn),
            [typed("ctrl-="), typed("ctrl-+")]
        );
        assert!(keymap.keystrokes(Command::OpenConfig).is_empty());
    }
}
