//! Keyboard shortcuts: the commands, their defaults per OS, and the keymap built from [keys].

use gpui_kit::{KeybindingKeystroke, Keystroke};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    NewTerminal,
    CloseTerminal,
    Copy,
    Paste,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FitAll,
    ThemePicker,
    OpenConfig,
    Settings,
    Quit,
}

impl Command {
    /// The [keys] order: when two commands share a shortcut, the earlier one wins.
    pub const ALL: [Command; 12] = [
        Command::NewTerminal,
        Command::CloseTerminal,
        Command::Copy,
        Command::Paste,
        Command::ZoomIn,
        Command::ZoomOut,
        Command::ZoomReset,
        Command::FitAll,
        Command::ThemePicker,
        Command::OpenConfig,
        Command::Settings,
        Command::Quit,
    ];

    /// Its key in [keys].
    pub fn name(self) -> &'static str {
        match self {
            Command::NewTerminal => "new_terminal",
            Command::CloseTerminal => "close_terminal",
            Command::Copy => "copy",
            Command::Paste => "paste",
            Command::ZoomIn => "zoom_in",
            Command::ZoomOut => "zoom_out",
            Command::ZoomReset => "zoom_reset",
            Command::FitAll => "fit_all",
            Command::ThemePicker => "theme_picker",
            Command::OpenConfig => "open_config",
            Command::Settings => "settings",
            Command::Quit => "quit",
        }
    }

    /// As the settings panel and its messages name it.
    pub fn title(self) -> &'static str {
        match self {
            Command::NewTerminal => "New terminal",
            Command::CloseTerminal => "Close terminal",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::ZoomIn => "Zoom in",
            Command::ZoomOut => "Zoom out",
            Command::ZoomReset => "Zoom 100%",
            Command::FitAll => "Fit all blocks",
            Command::ThemePicker => "Theme picker",
            Command::OpenConfig => "Edit config.toml",
            Command::Settings => "Settings",
            Command::Quit => "Quit Templum",
        }
    }

    /// Shift+1 is reported as "!" on some layouts: fit_all binds both.
    pub fn defaults(self, macos: bool) -> &'static [&'static str] {
        match (self, macos) {
            (Command::NewTerminal, false) => &["ctrl-shift-t"],
            (Command::NewTerminal, true) => &["cmd-t"],
            (Command::CloseTerminal, false) => &["ctrl-shift-w"],
            (Command::CloseTerminal, true) => &["cmd-w"],
            (Command::Copy, false) => &["ctrl-shift-c"],
            (Command::Copy, true) => &["cmd-c"],
            (Command::Paste, false) => &["ctrl-shift-v"],
            (Command::Paste, true) => &["cmd-v"],
            (Command::ZoomIn, false) => &["ctrl-=", "ctrl-+"],
            (Command::ZoomIn, true) => &["cmd-=", "cmd-+"],
            (Command::ZoomOut, false) => &["ctrl--"],
            (Command::ZoomOut, true) => &["cmd--"],
            (Command::ZoomReset, false) => &["ctrl-0"],
            (Command::ZoomReset, true) => &["cmd-0"],
            (Command::FitAll, false) => &["ctrl-shift-1", "ctrl-!"],
            (Command::FitAll, true) => &["cmd-shift-1", "cmd-!"],
            (Command::ThemePicker, false) => &["ctrl-shift-p"],
            (Command::ThemePicker, true) => &["cmd-shift-p"],
            (Command::OpenConfig, _) => &[],
            (Command::Settings, false) => &["ctrl-,"],
            (Command::Settings, true) => &["cmd-,"],
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
    pub close_terminal: Option<Keys>,
    pub copy: Option<Keys>,
    pub paste: Option<Keys>,
    pub zoom_in: Option<Keys>,
    pub zoom_out: Option<Keys>,
    pub zoom_reset: Option<Keys>,
    pub fit_all: Option<Keys>,
    pub theme_picker: Option<Keys>,
    pub open_config: Option<Keys>,
    pub settings: Option<Keys>,
    pub quit: Option<Keys>,
}

impl KeysConfig {
    pub fn get(&self, command: Command) -> Option<&Keys> {
        match command {
            Command::NewTerminal => self.new_terminal.as_ref(),
            Command::CloseTerminal => self.close_terminal.as_ref(),
            Command::Copy => self.copy.as_ref(),
            Command::Paste => self.paste.as_ref(),
            Command::ZoomIn => self.zoom_in.as_ref(),
            Command::ZoomOut => self.zoom_out.as_ref(),
            Command::ZoomReset => self.zoom_reset.as_ref(),
            Command::FitAll => self.fit_all.as_ref(),
            Command::ThemePicker => self.theme_picker.as_ref(),
            Command::OpenConfig => self.open_config.as_ref(),
            Command::Settings => self.settings.as_ref(),
            Command::Quit => self.quit.as_ref(),
        }
    }

    pub fn set(&mut self, command: Command, keys: Option<Keys>) {
        let slot = match command {
            Command::NewTerminal => &mut self.new_terminal,
            Command::CloseTerminal => &mut self.close_terminal,
            Command::Copy => &mut self.copy,
            Command::Paste => &mut self.paste,
            Command::ZoomIn => &mut self.zoom_in,
            Command::ZoomOut => &mut self.zoom_out,
            Command::ZoomReset => &mut self.zoom_reset,
            Command::FitAll => &mut self.fit_all,
            Command::ThemePicker => &mut self.theme_picker,
            Command::OpenConfig => &mut self.open_config,
            Command::Settings => &mut self.settings,
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
    shortcut
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => "-".to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(linux.command(&typed("ctrl-t")), None);
        let mac = defaults(true);
        assert_eq!(mac.command(&typed("cmd-t")), Some(Command::NewTerminal));
        assert_eq!(mac.command(&typed("cmd-,")), Some(Command::Settings));
        assert_eq!(mac.command(&typed("cmd-q")), Some(Command::Quit));
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
