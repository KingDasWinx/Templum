//! config.toml: every setting, typed and validated; polling for changes; saving edits.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use templum_terminal::{CursorShape, CursorStyle, TermOptions};

use toml_edit::{DocumentMut, TomlError};

use crate::keys::{Keymap, KeysConfig};
use crate::theme::{DEFAULT_DARK, DEFAULT_LIGHT};

pub(crate) const FILE: &str = "config.toml";

/// How often config.toml is checked for changes.
pub(crate) const POLL: Duration = Duration::from_millis(500);

/// Everything config.toml can set. Every table and key is optional: an empty file is the
/// defaults.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    pub font: Font,
    /// `[window]` held the glass options, now removed: a file that still has it loads.
    #[serde(rename = "window")]
    pub _window: serde::de::IgnoredAny,
    pub theme: ThemeConfig,
    pub shell: Shell,
    pub cursor: Cursor,
    pub terminal: TerminalConfig,
    pub keys: KeysConfig,
    /// `[agents.<id>]`: changes to one of Templum's AI agents (only what differs), or an agent of
    /// the user's own.
    pub agents: BTreeMap<String, AgentConfig>,
}

/// One AI agent's settings. For Templum's own, each missing key keeps its default; one of the
/// user's needs a name and a command.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AgentConfig {
    pub name: Option<String>,
    /// Typed into the shell to start it.
    pub command: Option<String>,
    /// Typed to bring it back when the canvas is restored; empty: a new session.
    pub resume: Option<String>,
    /// Listed in the canvas's menu.
    pub show: Option<bool>,
}

/// Letters, digits, `-` and `_`: an agent's id is a key in the saved canvas and in this file.
pub(crate) fn valid_agent_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
    let scrollback = config.terminal.scrollback;
    range(
        "terminal.scrollback",
        scrollback <= 100_000,
        "0 and 100000",
        scrollback.to_string(),
    )?;
    // Shortcuts are part of the file's validity: an invalid one keeps the last valid config.
    Keymap::new(&config.keys, cfg!(target_os = "macos"))?;
    for (id, agent) in &config.agents {
        if !valid_agent_id(id) {
            return Err(format!(
                "{FILE}: agents.{id:?}: an agent's id is letters, digits, - and _"
            ));
        }
        let ours = templum_terminal::Agent::from_name(id).is_some();
        let blank = |v: &Option<String>| v.as_deref().is_none_or(|v| v.trim().is_empty());
        if (!ours || agent.command.is_some()) && blank(&agent.command) {
            return Err(format!(
                "{FILE}: agents.{id}.command is needed (what starts it)"
            ));
        }
        if !ours && blank(&agent.name) {
            return Err(format!(
                "{FILE}: agents.{id}.name is needed (what menus show)"
            ));
        }
    }
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
    /// Fixed for a setting; an agent's id under `[agents]`.
    pub key: Cow<'static, str>,
    pub value: Option<toml_edit::Value>,
}

impl Edit {
    pub fn set(
        table: &'static str,
        key: impl Into<Cow<'static, str>>,
        value: impl Into<toml_edit::Value>,
    ) -> Self {
        Self {
            table,
            key: key.into(),
            value: Some(value.into()),
        }
    }

    pub fn remove(table: &'static str, key: impl Into<Cow<'static, str>>) -> Self {
        Self {
            table,
            key: key.into(),
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
                let table = doc
                    .entry(edit.table)
                    .or_insert(toml_edit::table())
                    .as_table_like_mut()
                    .ok_or(SaveError::NotATable(edit.table))?;
                // A new value in place keeps the key's comment lines and its `# …` on the line.
                match table
                    .get_mut(&edit.key)
                    .and_then(|item| item.as_value_mut())
                {
                    Some(old) => {
                        let mut value = value.clone();
                        *value.decor_mut() = old.decor().clone();
                        *old = value;
                    }
                    None => {
                        table.insert(&edit.key, toml_edit::Item::Value(value.clone()));
                    }
                }
            }
            None => remove_key(&mut doc, edit.table, &edit.key)?,
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
    write_whole(&path, &out).map_err(SaveError::Io)
}

/// Writes `text` to `path` whole: a temporary file beside it, renamed over it, so a crash
/// mid-write never leaves half a file. A linked file (from a dotfiles repository) stays linked: the file it points at is the
/// one replaced, with its permissions. A read-only file is left alone, as a plain write would.
pub(crate) fn write_whole(path: &Path, text: &str) -> io::Result<()> {
    let target = match std::fs::canonicalize(path) {
        Ok(target) => target,
        // A link to a file not made yet: make it where the link points.
        Err(_) => match std::fs::read_link(path) {
            Ok(link) => path.with_file_name("").join(link),
            Err(_) => path.to_path_buf(),
        },
    };
    let name = target
        .file_name()
        .map_or_else(|| FILE.to_string(), |n| n.to_string_lossy().into_owned());
    let old = std::fs::metadata(&target).ok();
    if old.as_ref().is_some_and(|m| m.permissions().readonly()) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{name} is read-only"),
        ));
    }
    let tmp = target.with_file_name(format!(".{name}.tmp"));
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        io::Write::write_all(&mut file, text.as_bytes())?;
        file.sync_all()?;
        if let Some(old) = &old {
            std::fs::set_permissions(&tmp, old.permissions())?;
        }
        std::fs::rename(&tmp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Removes `table.key`. The comment lines above a key belong to it in toml_edit: they move to
/// whatever follows (the next key, the next table's header, or the end of the file).
fn remove_key(doc: &mut DocumentMut, table: &'static str, key: &str) -> Result<(), SaveError> {
    let Some(item) = doc.get_mut(table) else {
        return Ok(());
    };
    let t = item
        .as_table_like_mut()
        .ok_or(SaveError::NotATable(table))?;
    let comments = t
        .key(key)
        .and_then(|k| k.leaf_decor().prefix())
        .and_then(|p| p.as_str())
        .filter(|p| p.contains('#'))
        .map(str::to_owned);
    let next = t
        .iter()
        .map(|(k, _)| k.to_owned())
        .skip_while(|k| k != key)
        .nth(1);
    t.remove(key);
    let Some(comments) = comments else {
        return Ok(());
    };
    if let Some(mut next) = next.and_then(|k| t.key_mut(&k)) {
        let decor = next.leaf_decor_mut();
        let rest = decor.prefix().and_then(|p| p.as_str()).unwrap_or("");
        decor.set_prefix(format!("{comments}{rest}"));
        return Ok(());
    }
    // The last key of its table: before the next table's header, or at the end.
    let at = doc
        .get(table)
        .and_then(|i| i.as_table())
        .and_then(|t| t.position());
    let next_table = doc
        .iter_mut()
        .filter_map(|(_, item)| item.as_table_mut())
        .filter(|t| t.position() > at)
        .min_by_key(|t| t.position());
    match next_table {
        Some(next) => {
            let decor = next.decor_mut();
            let rest = decor.prefix().and_then(|p| p.as_str()).unwrap_or("");
            decor.set_prefix(format!("{comments}{rest}"));
        }
        None => {
            let rest = doc.trailing().as_str().unwrap_or("").to_owned();
            doc.set_trailing(format!("{comments}{rest}"));
        }
    }
    Ok(())
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

    #[cfg(unix)]
    #[test]
    fn a_save_replaces_the_file_in_one_step() {
        use std::os::unix::fs::MetadataExt as _;
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[font]\nsize = 16\n");
        let before = std::fs::metadata(d.path().join(FILE)).unwrap().ino();
        save_theme(d.path(), true, "One Dark").unwrap();
        let after = std::fs::metadata(d.path().join(FILE)).unwrap().ino();
        assert_ne!(before, after, "written beside it, then renamed over it");
        let names: Vec<_> = std::fs::read_dir(d.path()).unwrap().collect();
        assert_eq!(names.len(), 1, "no temporary file left: {names:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_save_keeps_a_linked_file_linked_and_its_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = tempfile::tempdir().unwrap();
        let dotfiles = d.path().join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        let real = dotfiles.join(FILE);
        std::fs::write(&real, "[font]\nsize = 16\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        let dir = d.path().join("templum");
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&real, dir.join(FILE)).unwrap();
        save_theme(&dir, true, "One Dark").unwrap();
        let link = std::fs::symlink_metadata(dir.join(FILE)).unwrap();
        assert!(link.file_type().is_symlink(), "still a link");
        assert!(std::fs::read_to_string(&real).unwrap().contains("One Dark"));
        let mode = std::fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_a_file_not_made_yet_makes_it() {
        let d = tempfile::tempdir().unwrap();
        let dotfiles = d.path().join("dotfiles");
        std::fs::create_dir(&dotfiles).unwrap();
        let real = dotfiles.join(FILE);
        let dir = d.path().join("templum");
        std::fs::create_dir(&dir).unwrap();
        std::os::unix::fs::symlink(&real, dir.join(FILE)).unwrap();
        save_theme(&dir, true, "One Dark").unwrap();
        let link = std::fs::symlink_metadata(dir.join(FILE)).unwrap();
        assert!(link.file_type().is_symlink(), "still a link");
        assert!(std::fs::read_to_string(&real).unwrap().contains("One Dark"));
    }

    #[test]
    fn any_file_is_written_whole_and_named_in_errors() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("default.json");
        write_whole(&path, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let err = write_whole(&path, "[]").unwrap_err();
        assert_eq!(err.to_string(), "default.json is read-only");
    }

    #[test]
    fn a_read_only_file_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[font]\nsize = 16\n");
        let path = d.path().join(FILE);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let err = save_theme(d.path(), true, "One Dark").unwrap_err();
        assert_eq!(err.to_string(), "config.toml is read-only");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[font]\nsize = 16\n"
        );
    }

    use templum_terminal::{CursorShape, CursorStyle};

    #[test]
    fn an_empty_file_is_the_defaults() {
        let c = parse("").unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.font.size, 14.0);
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
    fn a_window_table_is_ignored() {
        // It held the glass options, now removed: a file written before still loads.
        assert_eq!(
            parse("[window]\nblur = false\nopacity = 0.9\n").unwrap(),
            Config::default()
        );
        assert!(parse("[window]\nanything = \"else\"\n").is_ok());
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

    #[test]
    fn agents_change_ours_and_add_the_users() {
        let c = parse(concat!(
            "[agents.claude]\n",
            "command = \"claude --model opus\"\n",
            "[agents]\n",
            "codex = { show = false }\n",
            "mine = { name = \"Mine\", command = \"mine --tui\", resume = \"\" }\n",
        ))
        .unwrap();
        let claude = &c.agents["claude"];
        assert_eq!(claude.command.as_deref(), Some("claude --model opus"));
        assert_eq!(claude.name, None, "unchanged: the default");
        assert_eq!(c.agents["codex"].show, Some(false));
        let mine = &c.agents["mine"];
        assert_eq!(mine.name.as_deref(), Some("Mine"));
        assert_eq!(
            mine.resume.as_deref(),
            Some(""),
            "empty: a new session on restore"
        );
    }

    #[test]
    fn an_agent_of_the_users_needs_a_name_and_a_command() {
        let err = parse("[agents.mine]\nname = \"Mine\"\n").unwrap_err();
        assert!(err.contains("agents.mine.command"), "{err}");
        let err = parse("[agents.mine]\ncommand = \"mine\"\n").unwrap_err();
        assert!(err.contains("agents.mine.name"), "{err}");
        let err = parse("[agents.claude]\ncommand = \"  \"\n").unwrap_err();
        assert!(err.contains("agents.claude.command"), "{err}");
        let err = parse("[agents.\"my agent\"]\nname = \"x\"\ncommand = \"x\"\n").unwrap_err();
        assert!(err.contains("my agent"), "{err}");
        let err = parse("[agents.claude]\nmodel = \"x\"\n").unwrap_err();
        assert!(err.contains("model"), "unknown keys still fail: {err}");
    }

    #[test]
    fn an_agent_is_saved_and_removed_whole() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "[agents]\n# mine\ncodex = { show = false }\n");
        let mut value = toml_edit::InlineTable::new();
        value.insert("command", "claude --model opus".into());
        save_edits(
            d.path(),
            &[Edit::set("agents", "claude".to_string(), value)],
        )
        .unwrap();
        let c = load(Some(d.path())).unwrap();
        assert_eq!(
            c.agents["claude"].command.as_deref(),
            Some("claude --model opus")
        );
        assert_eq!(c.agents["codex"].show, Some(false), "the others stay");
        save_edits(d.path(), &[Edit::remove("agents", "claude".to_string())]).unwrap();
        let c = load(Some(d.path())).unwrap();
        assert!(!c.agents.contains_key("claude"), "back to the default");
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# mine"), "{text}");
    }

    #[test]
    fn edits_keep_the_comments_around_a_key() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            concat!(
                "[font]\n",
                "# family = \"JetBrains Mono\"\n",
                "size = 16   # 6 to 72\n",
                "\n",
                "[theme]\n",
                "# mode = \"system\"\n",
                "light = \"One Light\"   # sunny\n",
            ),
        );
        save_edits(d.path(), &[Edit::set("font", "size", 18_i64)]).unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(
            text.contains(concat!(
                "# family = \"JetBrains Mono\"\n",
                "size = 18   # 6 to 72\n",
            )),
            "{text}"
        );
        save_edits(
            d.path(),
            &[Edit::remove("font", "size"), Edit::remove("theme", "light")],
        )
        .unwrap();
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("# family = \"JetBrains Mono\""), "{text}");
        assert!(text.contains("# mode = \"system\""), "{text}");
        assert!(!text.contains("size") && !text.contains("light"), "{text}");
        assert_eq!(load(Some(d.path())).unwrap(), Config::default());
    }
}
