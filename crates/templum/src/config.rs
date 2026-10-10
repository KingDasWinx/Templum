//! config.toml: every setting, typed and validated; polling for changes; saving edits.

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::WindowBackgroundAppearance;
use serde::Deserialize;
use templum_terminal::{CursorShape, CursorStyle, TermOptions};

use toml_edit::{DocumentMut, TomlError};

use crate::keys::{Keymap, KeysConfig};
use crate::theme::{self, DEFAULT_DARK, DEFAULT_LIGHT};

pub(crate) const FILE: &str = "config.toml";

/// How often config.toml is checked for changes.
pub(crate) const POLL: Duration = Duration::from_millis(500);

/// Everything config.toml can set. Every table and key is optional: an empty file is the
/// defaults.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub font: Font,
    pub window: WindowConfig,
    pub theme: ThemeConfig,
    pub shell: Shell,
    pub cursor: Cursor,
    pub terminal: TerminalConfig,
    pub keys: KeysConfig,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Font {
    /// None: the first installed font of the built-in monospace list.
    pub family: Option<String>,
    /// Points at zoom 1.
    pub size: f32,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: None,
            size: 14.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct WindowConfig {
    pub blur: Blur,
    /// The canvas's alpha when the window is translucent.
    pub opacity: f32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            blur: Blur::Auto,
            opacity: 0.72,
        }
    }
}

impl WindowConfig {
    pub fn translucent(&self) -> bool {
        match self.blur {
            Blur::Auto => theme::system_translucent(),
            Blur::On => true,
            Blur::Off => false,
        }
    }

    pub fn background(&self) -> WindowBackgroundAppearance {
        if self.translucent() {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Opaque
        }
    }
}

/// `"auto"` (blur where the system does), `true` or `false`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Blur {
    #[default]
    Auto,
    On,
    Off,
}

impl<'de> Deserialize<'de> for Blur {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Word(String),
        }
        match Raw::deserialize(d)? {
            Raw::Flag(true) => Ok(Blur::On),
            Raw::Flag(false) => Ok(Blur::Off),
            Raw::Word(w) if w == "auto" => Ok(Blur::Auto),
            Raw::Word(w) => Err(serde::de::Error::custom(format!(
                "expected \"auto\", true or false, found \"{w}\""
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ThemeConfig {
    pub mode: Mode,
    pub light: String,
    pub dark: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: Mode::System,
            light: DEFAULT_LIGHT.into(),
            dark: DEFAULT_DARK.into(),
        }
    }
}

impl ThemeConfig {
    /// The theme name for light or dark mode.
    pub fn name(&self, dark: bool) -> &str {
        if dark { &self.dark } else { &self.light }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    #[default]
    System,
    Light,
    Dark,
}

impl Mode {
    /// Whether the dark theme is in effect, given the system's appearance.
    pub fn dark(self, system_dark: bool) -> bool {
        match self {
            Mode::System => system_dark,
            Mode::Light => false,
            Mode::Dark => true,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Shell {
    /// None: $SHELL (PowerShell on Windows).
    pub program: Option<String>,
    /// Used with `program` only.
    pub args: Vec<String>,
    pub working_directory: WorkingDirectory,
}

/// Where new terminals start.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(from = "String")]
pub(crate) enum WorkingDirectory {
    /// The focused terminal's current folder (home when there is none).
    #[default]
    Inherit,
    Home,
    Path(PathBuf),
}

impl From<String> for WorkingDirectory {
    fn from(s: String) -> Self {
        match s.as_str() {
            "inherit" => Self::Inherit,
            "~" => Self::Home,
            _ => match (s.strip_prefix("~/"), std::env::home_dir()) {
                (Some(rest), Some(home)) => Self::Path(home.join(rest)),
                _ => Self::Path(PathBuf::from(s)),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Cursor {
    pub shape: Shape,
    pub blink: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Shape {
    #[default]
    Block,
    Bar,
    Underline,
}

impl Cursor {
    pub fn style(&self) -> CursorStyle {
        CursorStyle {
            shape: match self.shape {
                Shape::Block => CursorShape::Block,
                Shape::Bar => CursorShape::Beam,
                Shape::Underline => CursorShape::Underline,
            },
            blinking: self.blink,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct TerminalConfig {
    /// Lines of history per terminal.
    pub scrollback: usize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self { scrollback: 10_000 }
    }
}

impl Config {
    pub fn term_options(&self) -> TermOptions {
        TermOptions {
            scrollback: self.terminal.scrollback,
            cursor: self.cursor.style(),
        }
    }
}

/// Parses and validates config.toml. The error is ready to show to the user.
pub(crate) fn parse(text: &str) -> Result<Config, String> {
    let config: Config = toml::from_str(text).map_err(|e| {
        let (line, column) = e.span().map_or((1, 1), |s| line_col(text, s.start));
        format!("{FILE}, line {line}, column {column}: {}", e.message())
    })?;
    let range = |key: &str, ok: bool, limits: &str, value: String| {
        if ok {
            Ok(())
        } else {
            Err(format!(
                "{FILE}: {key} must be between {limits} (it is {value})"
            ))
        }
    };
    let size = config.font.size;
    range(
        "font.size",
        (6.0..=72.0).contains(&size),
        "6 and 72",
        size.to_string(),
    )?;
    let opacity = config.window.opacity;
    range(
        "window.opacity",
        (0.0..=1.0).contains(&opacity),
        "0 and 1",
        opacity.to_string(),
    )?;
    let scrollback = config.terminal.scrollback;
    range(
        "terminal.scrollback",
        scrollback <= 100_000,
        "0 and 100000",
        scrollback.to_string(),
    )?;
    // Shortcuts are part of the file's validity: an invalid one keeps the last valid config.
    Keymap::new(&config.keys, cfg!(target_os = "macos"))?;
    Ok(config)
}

/// `<dir>/config.toml`. No folder or no file: the defaults.
pub(crate) fn load(dir: Option<&Path>) -> Result<Config, String> {
    let Some(path) = dir.map(|d| d.join(FILE)) else {
        return Ok(Config::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
    }
}

/// What polling compares: config.toml's modification time and size (None: no file). Metadata
/// follows symlinks, so a config.toml linked from a dotfiles repository works too.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Stamp(Option<(SystemTime, u64)>);

impl Stamp {
    pub fn of(dir: Option<&Path>) -> Stamp {
        Stamp(
            dir.and_then(|d| std::fs::metadata(d.join(FILE)).ok())
                .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len())),
        )
    }
}

/// Templum's config folder: `$XDG_CONFIG_HOME/templum` or `~/.config/templum` on Linux and macOS
/// (the convention of terminal apps), `%APPDATA%\templum` on Windows.
pub(crate) fn dir() -> Option<PathBuf> {
    dir_from(
        cfg!(windows),
        std::env::var_os("APPDATA"),
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::home_dir(),
    )
}

fn dir_from(
    windows: bool,
    appdata: Option<OsString>,
    xdg: Option<OsString>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    let base = if windows {
        appdata.map(PathBuf::from)
    } else {
        xdg.filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
    };
    base.map(|b| b.join("templum"))
}

/// The 1-based line a parse error points at.
pub(crate) fn line_of(text: &str, err: &TomlError) -> usize {
    line_col(text, err.span().map_or(0, |s| s.start)).0
}

/// 1-based line and column of byte `at`.
fn line_col(text: &str, at: usize) -> (usize, usize) {
    let before = &text.as_bytes()[..at.min(text.len())];
    let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
    let column = before.iter().rev().take_while(|&&b| b != b'\n').count() + 1;
    (line, column)
}

#[derive(Debug)]
pub(crate) enum SaveError {
    /// No config folder could be found (no HOME, no APPDATA).
    NoDir,
    /// The file doesn't parse: it is left alone.
    Broken {
        line: usize,
    },
    /// `table` exists in the file but isn't a table (`theme = "Dracula"`).
    NotATable(&'static str),
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDir => write!(f, "no config folder"),
            Self::Broken { line } => write!(f, "{FILE} has an error on line {line}"),
            Self::NotATable(table) => write!(f, "`{table}` in {FILE} is not a table"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

/// One change to config.toml: `table.key` set to a value, or removed (back to its default).
#[derive(Clone, Debug)]
pub(crate) struct Edit {
    pub table: &'static str,
    pub key: &'static str,
    pub value: Option<toml_edit::Value>,
}

impl Edit {
    pub fn set(table: &'static str, key: &'static str, value: impl Into<toml_edit::Value>) -> Self {
        Self {
            table,
            key,
            value: Some(value.into()),
        }
    }

    pub fn remove(table: &'static str, key: &'static str) -> Self {
        Self {
            table,
            key,
            value: None,
        }
    }
}

/// Applies `edits` to `<dir>/config.toml`, keeping everything else (comments, other keys, line
/// endings, a byte-order mark). Creates the folder and the file; never writes over a file that
/// doesn't parse.
pub(crate) fn save_edits(dir: &Path, edits: &[Edit]) -> Result<(), SaveError> {
    let path = dir.join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(SaveError::Io(e)),
    };
    let crlf = text.contains("\r\n");
    let bom = text.starts_with('\u{feff}');
    let mut doc: DocumentMut = text.parse().map_err(|e| SaveError::Broken {
        line: line_of(&text, &e),
    })?;
    for edit in edits {
        match &edit.value {
            Some(value) => {
                doc.entry(edit.table)
                    .or_insert(toml_edit::table())
                    .as_table_like_mut()
                    .ok_or(SaveError::NotATable(edit.table))?
                    .insert(edit.key, toml_edit::Item::Value(value.clone()));
            }
            None => {
                if let Some(table) = doc.get_mut(edit.table) {
                    table
                        .as_table_like_mut()
                        .ok_or(SaveError::NotATable(edit.table))?
                        .remove(edit.key);
                }
            }
        }
    }
    // toml_edit writes LF and drops a byte-order mark: put the file's own back.
    let mut out = doc.to_string();
    if crlf {
        out = out.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    if bom && !out.starts_with('\u{feff}') {
        out.insert(0, '\u{feff}');
    }
    std::fs::create_dir_all(dir).map_err(SaveError::Io)?;
    std::fs::write(&path, out).map_err(SaveError::Io)
}

/// Sets `theme.dark` or `theme.light` (the theme picker's choice).
pub(crate) fn save_theme(dir: &Path, dark: bool, name: &str) -> Result<(), SaveError> {
    save_edits(
        dir,
        &[Edit::set(
            "theme",
            if dark { "dark" } else { "light" },
            name,
        )],
    )
}

/// The commented config.toml that `open_config` writes when there is none. Uncommented, every
/// `# key = value` line is the default; prose lines start with "##".
pub(crate) fn template(macos: bool) -> String {
    use crate::keys::Command;
    let mut t = String::from(concat!(
        "## Templum's configuration. Remove the \"# \" before a line (or \"## \" before an example)\n",
        "## and save: it applies at once.\n",
        "## All options: https://github.com/KingDasWinx/Templum#configuration\n\n",
        "[font]\n",
        "## Default: the first installed font of a built-in list. For example:\n",
        "## family = \"JetBrains Mono\"\n",
        "# size = 14   # 6 to 72\n\n",
        "[window]\n",
        "# blur = \"auto\"   # \"auto\" | true | false\n",
        "# opacity = 0.72   # the canvas when translucent, 0 to 1\n\n",
        "[theme]\n",
        "# mode = \"system\"   # \"system\" | \"light\" | \"dark\"\n",
        "# light = \"Catppuccin Latte\"\n",
        "# dark = \"Catppuccin Mocha\"\n\n",
        "[shell]\n",
        "## Default: $SHELL, or PowerShell on Windows. For example:\n",
        "## program = \"fish\"\n",
        "## args = [\"--login\"]\n",
        "# working_directory = \"inherit\"   # \"inherit\" | \"~\" | a path\n\n",
        "[cursor]\n",
        "# shape = \"block\"   # \"block\" | \"bar\" | \"underline\"\n",
        "# blink = false\n\n",
        "[terminal]\n",
        "# scrollback = 10000   # 0 to 100000\n\n",
        "[keys]\n",
    ));
    for command in Command::ALL {
        let quoted: Vec<String> = command
            .defaults(macos)
            .iter()
            .map(|k| format!("\"{k}\""))
            .collect();
        let value = match quoted.as_slice() {
            [one] => one.clone(),
            many => format!("[{}]", many.join(", ")),
        };
        t.push_str(&format!("# {} = {value}\n", command.name()));
    }
    t
}

/// `<dir>/config.toml`, written from the template first when it doesn't exist.
pub(crate) fn ensure_file(dir: &Path) -> io::Result<PathBuf> {
    let path = dir.join(FILE);
    if !path.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::write(&path, template(cfg!(target_os = "macos")))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, text: &str) {
        std::fs::write(dir.join(FILE), text).unwrap();
    }

    #[test]
    fn line_of_points_at_the_error() {
        let text = "[theme]\nlight = \"x\"\ndark = @nope\n";
        let err = text.parse::<toml_edit::DocumentMut>().unwrap_err();
        assert_eq!(line_of(text, &err), 3);
    }

    #[test]
    fn dir_follows_the_platform_conventions() {
        let xdg = Some(std::ffi::OsString::from("/x"));
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            dir_from(false, None, xdg, home.clone()),
            Some(PathBuf::from("/x").join("templum"))
        );
        assert_eq!(
            dir_from(false, None, Some("".into()), home.clone()),
            Some(PathBuf::from("/home/u").join(".config").join("templum"))
        );
        assert_eq!(dir_from(false, None, None, None), None);
        let appdata = Some(std::ffi::OsString::from("C:/Users/u/AppData/Roaming"));
        assert_eq!(
            dir_from(true, appdata, None, home),
            Some(PathBuf::from("C:/Users/u/AppData/Roaming").join("templum"))
        );
    }

    #[test]
    fn saving_keeps_comments_and_other_tables() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "# my settings\n[theme]\nlight = \"One Light\" # sunny\n\n[font]\nsize = 16\n",
        );
        save_theme(d.path(), true, "One Dark").unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# my settings"), "{text}");
        assert!(text.contains("light = \"One Light\" # sunny"), "{text}");
        assert!(text.contains("[font]\nsize = 16"), "{text}");
        let c = load(Some(d.path())).unwrap();
        assert_eq!(
            (c.theme.light.as_str(), c.theme.dark.as_str()),
            ("One Light", "One Dark")
        );
    }

    #[test]
    fn saving_creates_the_folder_and_the_file() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("templum");
        save_theme(&dir, false, "Catppuccin Frappé").unwrap();
        assert_eq!(load(Some(&dir)).unwrap().theme.light, "Catppuccin Frappé");
    }

    #[test]
    fn saving_never_overwrites_a_broken_file() {
        let d = tempfile::tempdir().unwrap();
        let broken = "[theme]\nlight = \"One Light\"\ndark = @nope\n";
        write(d.path(), broken);
        let err = save_theme(d.path(), true, "One Dark").unwrap_err();
        assert_eq!(err.to_string(), "config.toml has an error on line 3");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            broken
        );
    }

    #[test]
    fn saving_refuses_a_non_table_theme() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "theme = \"Dracula\"\n");
        let err = save_theme(d.path(), false, "One Light").unwrap_err();
        assert_eq!(err.to_string(), "`theme` in config.toml is not a table");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            "theme = \"Dracula\"\n"
        );
    }

    use templum_terminal::{CursorShape, CursorStyle};

    #[test]
    fn an_empty_file_is_the_defaults() {
        let c = parse("").unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.font.size, 14.0);
        assert_eq!(c.window.opacity, 0.72);
        assert_eq!(c.window.blur, Blur::Auto);
        assert_eq!(c.theme.mode, Mode::System);
        assert_eq!(c.theme.name(false), "Catppuccin Latte");
        assert_eq!(c.theme.name(true), "Catppuccin Mocha");
        assert_eq!(c.shell.working_directory, WorkingDirectory::Inherit);
        assert_eq!(c.terminal.scrollback, 10_000);
        assert_eq!(c.cursor.style(), CursorStyle::default());
    }

    #[test]
    fn reads_every_option() {
        let c = parse(
            r#"
[font]
family = "JetBrains Mono"
size = 16
[window]
blur = false
opacity = 0.9
[theme]
mode = "dark"
light = "One Light"
dark = "One Dark"
[shell]
program = "fish"
args = ["--login"]
working_directory = "~"
[cursor]
shape = "bar"
blink = true
[terminal]
scrollback = 500
"#,
        )
        .unwrap();
        assert_eq!(c.font.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(c.font.size, 16.0);
        assert_eq!((c.window.blur, c.window.opacity), (Blur::Off, 0.9));
        assert!(!c.window.translucent());
        assert!(c.theme.mode.dark(false), "dark is pinned");
        assert_eq!(c.theme.name(true), "One Dark");
        assert_eq!(c.shell.program.as_deref(), Some("fish"));
        assert_eq!(c.shell.args, ["--login"]);
        assert_eq!(c.shell.working_directory, WorkingDirectory::Home);
        let options = c.term_options();
        assert_eq!(options.scrollback, 500);
        assert_eq!(
            options.cursor,
            CursorStyle {
                shape: CursorShape::Beam,
                blinking: true
            }
        );
    }

    #[test]
    fn blur_takes_auto_true_or_false() {
        assert_eq!(
            parse("[window]\nblur = \"auto\"").unwrap().window.blur,
            Blur::Auto
        );
        assert_eq!(
            parse("[window]\nblur = true").unwrap().window.blur,
            Blur::On
        );
        assert!(parse("[window]\nblur = true").unwrap().window.translucent());
        let err = parse("[window]\nblur = \"maybe\"").unwrap_err();
        assert!(err.contains("maybe"), "{err}");
    }

    #[test]
    fn mode_light_pins_light() {
        let c = parse("[theme]\nmode = \"light\"").unwrap();
        assert!(!c.theme.mode.dark(true));
        assert!(Mode::System.dark(true) && !Mode::System.dark(false));
    }

    #[test]
    fn working_directory_forms() {
        let home = std::env::home_dir().unwrap();
        let wd = |s: &str| {
            parse(&format!("[shell]\nworking_directory = {s}"))
                .unwrap()
                .shell
                .working_directory
        };
        assert_eq!(wd("\"inherit\""), WorkingDirectory::Inherit);
        assert_eq!(wd("\"~\""), WorkingDirectory::Home);
        assert_eq!(wd("\"~/code\""), WorkingDirectory::Path(home.join("code")));
        assert_eq!(
            wd("\"/srv/x\""),
            WorkingDirectory::Path(PathBuf::from("/srv/x"))
        );
        // A Windows path as a TOML literal string: no escapes needed.
        assert_eq!(
            wd(r"'C:\Users\x'"),
            WorkingDirectory::Path(PathBuf::from(r"C:\Users\x"))
        );
    }

    #[test]
    fn unknown_keys_fail_with_line_and_column() {
        let err = parse("[font]\nsize = 14\n\nfnot = 1\n").unwrap_err();
        assert!(err.starts_with("config.toml, line 4, column 1:"), "{err}");
        assert!(err.contains("fnot"), "{err}");
    }

    #[test]
    fn out_of_range_values_fail_naming_the_key() {
        let err = parse("[font]\nsize = 100").unwrap_err();
        assert!(
            err.contains("font.size") && err.contains("6 and 72"),
            "{err}"
        );
        let err = parse("[window]\nopacity = 1.5").unwrap_err();
        assert!(
            err.contains("window.opacity") && err.contains("0 and 1"),
            "{err}"
        );
        let err = parse("[terminal]\nscrollback = 200000").unwrap_err();
        assert!(
            err.contains("terminal.scrollback") && err.contains("100000"),
            "{err}"
        );
    }

    #[test]
    fn load_gives_defaults_without_a_file_and_errors_for_a_bad_one() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(load(Some(d.path())).unwrap(), Config::default());
        assert_eq!(load(None).unwrap(), Config::default());
        write(d.path(), "[font\n");
        assert!(load(Some(d.path())).unwrap_err().contains("line 1"));
    }

    #[test]
    fn stamp_notices_edits_creation_and_deletion() {
        let d = tempfile::tempdir().unwrap();
        let none = Stamp::of(Some(d.path()));
        assert_eq!(none, Stamp::default());
        write(d.path(), "[font]\n");
        let created = Stamp::of(Some(d.path()));
        assert_ne!(created, none);
        write(d.path(), "[font]\nsize = 15\n");
        assert_ne!(Stamp::of(Some(d.path())), created);
        std::fs::remove_file(d.path().join(FILE)).unwrap();
        assert_eq!(Stamp::of(Some(d.path())), none);
    }

    #[test]
    fn stamp_notices_a_replace_by_rename() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[font]\n");
        let before = Stamp::of(Some(d.path()));
        // How vim and VS Code save: write elsewhere, then rename over the file.
        let tmp = d.path().join("config.toml.tmp");
        std::fs::write(&tmp, "[font]\nsize = 16\n").unwrap();
        std::fs::rename(&tmp, d.path().join(FILE)).unwrap();
        assert_ne!(Stamp::of(Some(d.path())), before);
    }

    #[test]
    fn keys_are_read_and_validated() {
        let c = parse("[keys]\nnew_terminal = \"ctrl-alt-n\"\nfit_all = [\"f11\"]\n").unwrap();
        assert_eq!(
            c.keys.new_terminal,
            Some(crate::keys::Keys::One("ctrl-alt-n".into()))
        );
        let err = parse("[keys]\nnew_terminal = \"ctrl-shft-t\"\n").unwrap_err();
        assert!(err.contains("new_terminal"), "{err}");
        let err = parse("[keys]\nnew_tab = \"ctrl-t\"\n").unwrap_err();
        assert!(err.contains("new_tab"), "unknown action: {err}");
    }

    /// The template with every `# key = value` line uncommented (prose lines start with "##").
    fn uncomment(text: &str) -> String {
        text.lines()
            .map(|l| l.strip_prefix("# ").unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_template_is_the_defaults_commented_or_not() {
        for macos in [false, true] {
            let t = template(macos);
            assert_eq!(parse(&t).unwrap(), Config::default(), "all commented");
            let mut c = parse(&uncomment(&t)).unwrap();
            let keys = std::mem::take(&mut c.keys);
            assert_eq!(c, Config::default(), "uncommented");
            let (from_template, _) = Keymap::new(&keys, macos).unwrap();
            let defaults = Keymap::new(&KeysConfig::default(), macos).unwrap().0;
            for command in crate::keys::Command::ALL {
                assert_eq!(from_template.first(command), defaults.first(command));
            }
        }
    }

    #[test]
    fn ensure_file_writes_the_template_once() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("templum");
        let path = ensure_file(&dir).unwrap();
        assert_eq!(path, dir.join(FILE));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            template(cfg!(target_os = "macos"))
        );
        std::fs::write(&path, "[font]\nsize = 20\n").unwrap();
        ensure_file(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[font]\nsize = 20\n",
            "kept"
        );
    }

    #[test]
    fn saving_keeps_crlf_and_the_bom() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "\u{feff}# mine\r\n[theme]\r\nlight = \"One Light\"\r\n",
        );
        save_theme(d.path(), true, "One Dark").unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.starts_with('\u{feff}'), "{text:?}");
        assert!(
            !text.replace("\r\n", "").contains('\n'),
            "LF only line: {text:?}"
        );
        assert!(text.contains("dark = \"One Dark\""), "{text:?}");
    }

    #[test]
    fn the_templates_example_lines_work_once_uncommented() {
        // "## key = value" example lines (font family, shell) become settings without "## ".
        let t = template(false);
        let text: String = t
            .lines()
            .map(|l| {
                let l = l
                    .strip_prefix("## ")
                    .filter(|rest| rest.contains(" = "))
                    .unwrap_or(l);
                l.strip_prefix("# ").unwrap_or(l)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let c = parse(&text).unwrap();
        assert_eq!(c.font.family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(c.shell.program.as_deref(), Some("fish"));
        assert_eq!(c.shell.args, ["--login"]);
    }

    #[test]
    fn edits_set_create_and_remove_keys() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            "# mine\n[font]\nsize = 16\nfamily = \"Iosevka\"\n\n[cursor]\nblink = true # on\n",
        );
        save_edits(
            d.path(),
            &[
                Edit::set("font", "size", 20_i64),
                Edit::remove("font", "family"),
                Edit::set("terminal", "scrollback", 500_i64),
                Edit::remove("keys", "copy"),
            ],
        )
        .unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# mine"), "{text}");
        assert!(text.contains("size = 20"), "{text}");
        assert!(!text.contains("family"), "{text}");
        assert!(text.contains("blink = true # on"), "{text}");
        assert!(text.contains("[terminal]"), "{text}");
        assert!(text.contains("scrollback = 500"), "{text}");
        assert!(
            !text.contains("[keys]"),
            "removing from a missing table adds nothing: {text}"
        );
        let c = load(Some(d.path())).unwrap();
        assert_eq!((c.font.size, c.font.family), (20.0, None));
        assert_eq!(c.terminal.scrollback, 500);
    }

    #[test]
    fn an_edit_to_a_non_table_names_it() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "font = 3\n");
        let err = save_edits(d.path(), &[Edit::set("font", "size", 20_i64)]).unwrap_err();
        assert_eq!(err.to_string(), "`font` in config.toml is not a table");
        let err = save_edits(d.path(), &[Edit::remove("font", "size")]).unwrap_err();
        assert_eq!(err.to_string(), "`font` in config.toml is not a table");
        assert_eq!(
            std::fs::read_to_string(d.path().join(FILE)).unwrap(),
            "font = 3\n"
        );
    }
}
